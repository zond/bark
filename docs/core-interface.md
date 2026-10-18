# Core interface

**Status:** fixed for Phase 0, 2026-10-14; the application interface (§4) revised
2026-10-15, roadmap D29; one node type, 2026-10-17, roadmap D46; the sleep signal (§3 rule 10), 2026-10-17,
roadmap D47; duty-cycled receive and the long preamble (§3 rules 4, 8, 11), 2026-10-17, roadmap D51; every sleepy node receives duty-cycled and `Action::Sleep` goes (§3 rule 10), 2026-10-18, roadmap D58. Roadmap decision D3. The types are in
`bark-proto/src/{time,radio,position,ids,event,node}.rs`; this page is the contract
around them that a driver (the simulator, the firmware) must honour and the core may rely on.

## 1. Shape

The core is a state machine behind the `Core` trait:

```rust
fn handle(&mut self, event: Event<'_>, now: Instant, emit: &mut dyn FnMut(Action));
fn next_wakeup(&self) -> Option<Instant>;
fn accepts_send(&self, now: Instant) -> bool;
```

One struct, `Node`, for every node; `Config::sleepy` makes it a sleepy node
(spec "Sleepy nodes"). `Relay` is the node with its heard table and delivery queue, and
`SleepyNode` the node without them, which a sleepy node does not keep; a `Relay` set up
sleepy keeps them empty and behaves the same (§6). A sleepy node's radio receives
duty-cycled whenever the node is not in a window or listening for its anchor's heartbeat;
when and until when, the core says with `Action::Sniff` (§3 rule 10). `Node::anchorless()`
says whether it is a sleepy node without an anchor (spec "Sleepy nodes alone").

The core never blocks, never sleeps, never allocates, and never reads a clock: `now` is an
argument. Every random choice comes from the `Rng` seeded at construction, so a core's
behaviour is a function of its config, its seed and its event sequence.

## 2. Time

`Instant` and `Duration` are integer microseconds; there is no other time unit to mix
them up with. `now` never decreases across calls on one core. Different cores have unrelated clocks; the protocol has no network time (spec "Position and time").

Wakeups: after every `handle` the driver reads `next_wakeup()`. When its clock reaches that
instant it calls `handle(Event::Wakeup, now)`. Late is allowed; early is not. The core keeps
all its own deadlines internally and exposes only the earliest, so there is one timer per
core and no timer identifiers cross the boundary.

Boot: the first `handle` call a core receives is `Event::Wakeup`, delivered at start
whatever `next_wakeup()` says. The core's boot listen (spec "Channel at boot") starts there.

## 3. Radio

The driver owns one half-duplex LoRa radio that is either listening on a channel or away on
an **excursion**: a channel activity check (`Action::Cad`), a signal strength reading
(`Action::Rssi`) or a transmission (`Action::Transmit`). `Action::Listen(c)` is not an excursion: it sets the resting channel,
takes effect at once, and the radio listens on `c` whenever no excursion is in progress.

Rules:

1. **One excursion outstanding.** After emitting `Cad`, `Rssi` or `Transmit` the core waits
   for its completion event (`CadDone`, `RssiSample`, `TxDone`) before emitting another.
   A driver may treat a violation as a bug and panic.
2. **Every excursion completes with exactly one event**, then the radio is listening on the
   resting channel again. The driver pays the retune, settle, CAD, RSSI and airtime costs and
   is deaf during them; the core learns how long an excursion took by comparing `now` at the
   completion event with `now` when it emitted the action. The thirteen-channel sweep is
   thirteen `Rssi` excursions issued one after the other.
3. **`Received` is a CRC-valid LoRa payload** heard on the resting channel, with the
   packet RSSI and SNR the radio reports. Nothing is received during an excursion.
4. **`Transmit` frames** satisfy `preamble_symbols >= radio::MIN_PREAMBLE_SYMBOLS` (12 at
   SF5, SX1262 datasheet §6.1.4) and `len <= MAX_FRAME`. The driver transmits at
   `Config::tx_power_dbm` on `Frame::channel` with `Frame::preamble_symbols` of preamble,
   then returns to the resting channel. The core asks for the shortest preamble, or for
   `sniff::LONG_PREAMBLE_SYMBOLS` (1396, 357.4 ms) on a frame to a node that receives
   duty-cycled (rule 10). The receiver is programmed for the long preamble, the longest a
   frame can have, which the datasheet asks where the length varies (§6.1.4); it still
   receives the short one.
5. **Listen-before-talk is the core's job.** The driver never checks the channel on its own;
   the core issues `Cad` or `Rssi` and decides.
6. **Duty cycle is the core's job.** The core keeps the duty-cycle budget (spec "Duty cycle cap") and
   defers; the driver never drops a frame.
7. **`Rssi` on the resting channel is not an excursion.** The radio keeps receiving while it
   reads its own channel, so the core may issue it at any time; it still completes with
   `RssiSample`. `Rssi` on another channel costs a retune and is an excursion.
8. **The driver may hold a `Cad` or `Rssi` excursion while a reception is in progress**, by at
   most the airtime of a full frame with the long preamble, 492.9 ms, so that a sweep does not
   cut a packet short (spec "Busy estimate").
   `Transmit` is never held. The core's timeouts allow for the hold.
9. **`Action::Trace` carries no obligation.** It reports protocol events (a dropped message
   and why, a deferral, a forward, a backtrack, a demotion, a channel move) for the simulator's
   metrics and the firmware's log. A driver may ignore it.
10. **`Action::Sniff(until)` lets the radio receive duty-cycled** on the resting channel
    until `until`: the SX1262's SetRxDutyCycle (datasheet §13.1.7) with a receive period of
    `sniff::SNIFF_RX` (6 ms, 384 steps of 15.625 µs) and a sleep period of
    `sniff::SNIFF_SLEEP` (349.125 ms, 22 344 steps). The radio then catches every frame
    whose preamble is `sniff::LONG_PREAMBLE_SYMBOLS` long and may miss any other; what it
    catches is `Received` as ever, and after it the driver starts the cycle again. The
    sniff ends at `until`, or sooner at the core's next `Listen`, `Cad`, `Rssi` or
    `Transmit`, which the driver carries out as ever, the radio then listening on the
    resting channel again; `until` is when the core next needs the radio of its own
    accord. A driver that keeps listening breaks nothing: the core handles whatever it
    receives. Every sleepy node asks for it whenever it has nothing under way and is
    neither in a window nor listening for its anchor's heartbeat (spec "Sleepy nodes",
    "Delivery", "Relay choice"); a node that is not sleepy never does.

The modulation is fixed (SF5, 125 kHz, CR 4/5, `radio.rs`) and is not negotiated.

## 4. Application

Every node has the same application interface: the readiness query
`accepts_send`, the commands `AppCommand::Send` and `AppCommand::SetPosition`, and the
one event `AppEvent::Message`.

`AppCommand::Send` originates a message to `(dst_pos, dst_id)` with an opaque payload of
at most 233 bytes (§7). The driver offers one only while `accepts_send(now)` is true, and
asks again after every `handle`, so an application waits for its node; a firmware's async
send awaits it. The core reports nothing back about the message, not even the first hop's
ACK, which a user could read as delivered: from the moment it is taken the node is either
sending it or waiting to. The protocol has no end-to-end acknowledgement (spec "Addresses"), so
whether the message arrived is for the app to find out inside its payload. A message the
node drops is reported only as `Trace::Dropped` (§3 rule 9).

A node holds one message of its application's at a time, until its first hop takes it.
It accepts a message while it is running, holds none, its send queue takes a full
frame on top of what it holds (spec "Send queue"), and not every neighbour is banned, or
for a sleepy node with a live anchor, the anchor (spec "Receiving DATA"). When only its
bucket and bans keep it from accepting, its `next_wakeup()` includes the moment both allow
it. A node whose remaining neighbours are all banned once it holds the message keeps it and
tries again when the first ban ends, or later after refusals in a row
(spec "A node's own message"); one for a sleepy node it holds for that finds the delivery
queue full waits for a place (spec "Return to parent"). A sleepy node's own message waits
as in spec "A sleepy node's own message".

A `Send` while `accepts_send` is false, or with a longer payload, breaks the contract like
a second outstanding excursion: the core ignores it, and the test and simulator drivers
panic or never offer it.

`AppCommand::SetPosition` updates own position, from a GPS fix that passed the plausibility
checks or a map point set in the app (spec "Position and time"); a relay's address changes
with it, and its sleepy nodes' (spec "Node identity"). `AppEvent::Message` delivers a payload to this node's application.
The core never inspects a payload (spec "Phone link": the app owns identity and encryption).

## 5. What the driver must simulate or implement

| Driver obligation | Simulator | Firmware |
|---|---|---|
| Clock | event-heap time | embassy monotonic |
| Wakeup when `now >= next_wakeup()` | heap entry per core, lazily invalidated | one embassy timer |
| `Listen` | update the per-channel listener set | `rx_switch_channel` |
| `Sniff` | receives a frame only if a window of the cycle, started at the `Sniff` and again after each frame it takes, holds four symbols of its preamble; the time is counted | ignored for now: the radio keeps listening; to do: SetRxDutyCycle, and the receiver programmed for the long preamble |
| `Cad` | deaf for CAD duration, result from the reception model | `prepare_for_cad` + `cad` |
| `Rssi` | deaf for retune + sample, dB-sum at the node | `get_rssi` after retune |
| `Transmit` | deaf for airtime, deliveries to hearers via the reception model | `prepare_for_tx` + `tx` |
| `Received` | reception model | `rx` with CRC check |
| Seed | per-run seed → per-node stream | nRF RNG at boot |

## 6. Memory

`Relay` holds its tables inline (spec "Implementation notes"). Sizes as built, 2026-10-17 (roadmap D47), checked by the
test in `tables/mod.rs`:

| Table | Entries × size | Total |
|---|---|---|
| Neighbours (spec "Neighbour table") | 64 × 64 B | 4.0 KiB |
| Heard sleepy nodes (spec "Heard table") | 256 × 32 B | 8.0 KiB |
| Delivery queue (spec "Delivery queue") | 41 × 264 B | 10.6 KiB |
| Message records (spec "Message records") | 1855 × 32 B | 58.0 KiB |
| Send queue (spec "Send queue") | (40 + 254) × 280 B | 80.4 KiB |
| Delivered window (spec "Delivery queue") | 41 × 16 B | 0.6 KiB |
| Duty budget (spec "Duty cycle cap") and busy estimate (spec "Busy estimate") | 16 B + 348 B | 0.4 KiB |
| **All** | | **162.1 KiB** |

The send queue's 40 regular places follow from the duty bucket, and its 254 hop places, one
for each hop count a message can be sent on with, keep overload from dropping messages
(spec "Send queue", roadmap D31). The records hold the 1561 messages a relay can handle in the
457.3 s a full send queue takes to drain at one ban per place, plus the 294 the send queue
holds, whose records are never given up (spec "Message records"). The delivery queue's 41 places hold what
arrives in one delivery TTL for a full heard table (spec "Delivery queue"). The whole `Relay` is
166 840 B (162.9 KiB, measured 2026-10-18): the tables and, in under 1 KiB around them, the
medium access, the hand-over, its one owed reply and its application's message while it
waits. A `SleepyNode` is 147 824 B (144.4 KiB), without the heard table and the delivery
queue, 18.6 KiB (roadmap D58). There is no fixed target;
the sizes follow from the design. The board's application has 255 KiB of RAM
(`bark-fw/memory.x`); the BLE controller takes under 5 KiB
(`implementation-research-2026-10-14.md`) and Embassy and the stacks about 16 KiB
(spec "Implementation notes"), which leaves 234 KiB, 71 KiB more than a node needs. Checked by the tests in
`tables/mod.rs` and `node/relay.rs`.

That holds only if the relay is not built on the stack. `Relay::new` returns a relay by
value, and on the board's target it takes 174 KiB of stack to build one (measured
2026-10-15 in the firmware's release build), more than the 93 KiB left beside a
relay in static memory. So `Relay::UNSET`, a relay with empty tables, is a constant a driver
can keep in a static, built at compile time, and `Relay::reset(config, seed)` sets it up in
place, as `Relay::new` would have; the firmware does this. The simulator, on the host, uses
`Relay::new`.

`Action::Transmit` and `AppEvent::Message` carry a 255-byte frame by value: drivers hold one
action at a time, and the core never allocates.

## 7. Wire-format choices made here (design O2)

- Position: 56-bit big-endian, latitude in the top 25 bits, longitude in the next 26, five
  spare bits zero (`position.rs`).
- Short position (spec "Position and time", roadmap D14): the low 16 bits of the latitude count, then
  the low 16 bits of the longitude count, big-endian (`ShortName` in `position.rs`). The
  receiver rebuilds the full position from its own as the one candidate within half a
  period (32 768 counts) on each axis; both axes wrap as their wire fields do.
- Node ID (spec "Node identity"): four opaque bytes; the firmware takes the low word of the
  chip's FICR DEVICEID, the simulator gives node `i` the ID `(i + 1) × 0x9E3779B1`.
- Message ID (spec "Packet formats", D15): 32-bit FNV-1a over `dst_pos || dst_id || payload`,
  `dst_pos` in its 7 wire bytes (`MsgId::of` in `ids.rs`; `Data::msg_id` on a decoded
  DATA).
- Everything else in spec "Packet formats" is encoded by the codecs in `wire.rs`. Choices made
  there, 2026-10-14:
  - Multi-byte integers (short positions, message IDs) are big-endian; a node ID is four
    opaque bytes in order, compared whole.
  - ACK `to` of a DATA is its `src_hop`.
  - A sleepy node's own DATA has `hops` zero. A relay sending for its own application
    sends `hops` 1, as if it had received the message from the application at 0: a
    receiver takes `hops` 0 for a sleepy node's own message (spec "Packet sender").
  - Application payload at most 233 bytes, what remains of 255 after the 22-byte DATA
    header; the decoder rejects anything longer.
  - DATA `time_left` counts units of 2.5 s (`wire::TTL_UNIT`), rounded down at each
    transmission (spec "Receiving DATA"); a DATA to a sleepy node carries what the message
    had when it reached the holder.
  - Types 0x1, 0x2, 0x4, 0x6, 0x7 and 0xB to 0xE are unused and rejected.
  - ACK `aux` bit 0 is set for a refusal (roadmap D32); the other `aux` bits of an ACK are
    sent as zero and ignored on receipt. A fixed-size packet with trailing bytes is
    rejected.

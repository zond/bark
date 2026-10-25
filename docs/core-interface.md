# Core interface

**Status:** fixed for Phase 0, 2026-10-14; the application interface (§4) revised
2026-10-15, roadmap D29; one node type, 2026-10-17, roadmap D46; the sleep signal (§3 rule 10), 2026-10-17,
roadmap D47; duty-cycled receive and the long preamble (§3 rules 4, 8, 11), 2026-10-17, roadmap D51; every sleepy node receives duty-cycled and `Action::Sleep` goes (§3 rule 10), 2026-10-18, roadmap D58; the firmware's duty-cycled receive and `AppEvent::GivenBack` (§3 rule 10, §4, §5), 2026-10-20; `AppCommand::Fix` and the bench frames at type 0x6 (§4, §7), 2026-10-21, roadmap D70; `AppCommand::Power` (§2, §4), 2026-10-21, roadmap D71; every node measures from `AppCommand::Fix` whether it is carried (§4), 2026-10-22, roadmap D72; the first rotation, `Action::Store`, `Config::stored` and `AppEvent::Boot` (§2, §4, §6, §7), 2026-10-24, roadmap D73; six-byte node IDs (§6, §7), 2026-10-24, roadmap D75; the first byte's two shapes, one ACK with the wait in `aux`, and the bench frames on channel 15 (§7), 2026-10-24, roadmap D76; the records and sizes after the airtime correction (§6, §7), 2026-10-25; `Action::Locate` (§4, §5), 2026-10-25, roadmap D80. Roadmap decision D3. The types are in
`bark-proto/src/{time,radio,position,ids,event,node}.rs`; this page is the contract
around them that a driver (the simulator, the firmware) must honour and the core may rely on.

## 1. Shape

The core is a state machine behind the `Core` trait:

```rust
fn handle(&mut self, event: Event<'_>, now: Instant, emit: &mut dyn FnMut(Action));
fn next_wakeup(&self) -> Option<Instant>;
fn accepts_send(&self, now: Instant) -> bool;
```

One struct, `Node`, for every node; its power makes it a relay or a sleepy node
(`AppCommand::Power`, §4; spec "Nodes", "Sleepy nodes"). `Relay` is the node with its
heard table and delivery queue, and `SleepyNode` the node without them, which a sleepy
node does not keep: it is sleepy whatever its power, for a driver whose node never
switches, and a `Relay` on battery keeps them empty and behaves the same (§6). `Node::sleepy()` says which it is now. A sleepy node's radio receives
duty-cycled whenever the node is not in a window or listening for its anchor's heartbeat;
when and until when, the core says with `Action::Sniff` (§3 rule 10). `Node::anchorless()`
says whether it is a sleepy node without an anchor (spec "Sleepy nodes alone").

The core never blocks, never sleeps, never allocates, and never reads a clock: `now` is an
argument. Every random choice comes from the `Rng` seeded at construction, so a core's
behaviour is a function of its config, its seed and its event sequence.

## 2. Time

`Instant` and `Duration` are integer microseconds; there is no other time unit to mix
them up with. `now` never decreases across calls on one core. Different cores have unrelated clocks; the protocol has no network time (spec "Position").

Wakeups: after every `handle` the driver reads `next_wakeup()`. When its clock reaches that
instant it calls `handle(Event::Wakeup, now)`. Late is allowed; early is not. The core keeps
all its own deadlines internally and exposes only the earliest, so there is one timer per
core and no timer identifiers cross the boundary.

Boot: the first `handle` call a core receives is `Event::Wakeup`, delivered at start
whatever `next_wakeup()` says. The core's first rotation (spec "Channel at boot") starts
there. The driver then reports the power at once, at the same instant (§4); until it does,
a `Relay` takes itself to be on external power. The driver builds the core with what it
kept of the core's state before the reboot, if anything, in `Config::stored` (§4).

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
    accord. The driver may let a packet already under way end first, by at most the
    airtime of rule 8, since either receive hears it; a packet that outlasts that
    starts the cycle again. A driver that keeps listening breaks nothing: the core handles whatever it
    receives. Every sleepy node asks for it whenever it has nothing under way and is
    neither in a window nor listening for its anchor's heartbeat (spec "Sleepy nodes",
    "Delivery", "Relay choice"); a node that is not sleepy never does.

The modulation is fixed (SF5, 125 kHz, CR 4/5, `radio.rs`) and is not negotiated.

## 4. Application

Every node has the same application interface: the readiness query
`accepts_send`, the commands `AppCommand::Send`, `AppCommand::SetPosition`,
`AppCommand::Fix` and `AppCommand::Power`, the events `AppEvent::Message`,
`AppEvent::GivenBack` and `AppEvent::Boot`, the store, `Action::Store` and
`Config::stored`, and the GPS request `Action::Locate`.

`AppCommand::Send` originates a message to `(dst_pos, dst_id)` with an opaque payload of
at most 226 bytes (§7). The driver offers one only while `accepts_send(now)` is true, and
asks again after every `handle`, so an application waits for its node; a firmware's async
send awaits it. The core reports nothing back about the message but a give-back (below), not even the first hop's
ACK, which a user could read as delivered: from the moment it is taken the node is either
sending it or waiting to. The protocol has no end-to-end acknowledgement (spec "Addresses"), so
whether the message arrived is for the app to find out inside its payload. A message the
node drops is reported only as `Trace::Dropped` (§3 rule 9).

A node holds one message of its application's at a time, until its first hop takes it.
It accepts a message from its boot on while it holds none, its send queue takes a full
frame on top of what it holds (spec "Send queue"), and not every neighbour is banned, or
for a sleepy node with a live anchor, the anchor (spec "Receiving DATA"). When only its
bucket and bans keep it from accepting, its `next_wakeup()` includes the moment both allow
it. A node whose remaining neighbours are all banned once it holds the message keeps it and
tries again when the first ban ends, or later after refusals in a row
(spec "Bans"); one for a sleepy node it holds for that finds the delivery
queue full waits for a place (spec "Return to parent"). A sleepy node's own message that
its first hop refuses goes back to the application (spec "A sleepy node's own message"):
the core emits `AppEvent::GivenBack` with the message's ID and `retry_at`, the end of the
wait the refusal asked for, and `Trace::Dropped` with `DropReason::Pushback` for the log;
`accepts_send` stays false until `retry_at`, which `next_wakeup()` includes. The driver
hands the event to the application, which may offer the message again then or give it up. One whose hand-over goes
unanswered waits in the node as in that section.

A `Send` while `accepts_send` is false, or with a longer payload, breaks the contract like
a second outstanding excursion: the core ignores it, and the test and simulator drivers
panic or never offer it.

`AppCommand::SetPosition` sets own position from a map point set in the app, which wins
over GPS: the node takes it and is fixed (spec "Position"). `AppCommand::Fix` hands
the core a GPS fix that passed the driver's plausibility checks; the driver hands each one,
and none while a map point is set. Every node, sleepy or not, decides from the fixes
whether it is carried; a sleepy or carried node takes each fix, and one that is neither
keeps its position (roadmap D70, D72). A node's address changes with its
position, and a relay's sleepy nodes' with it (spec "Node identity").

`Action::Locate(until)` asks the driver to run the GPS until `until` and hand the first
fix that passes its checks as `AppCommand::Fix`, then stop it; a later one moves the end.
Only a sleepy node asks, at the events spec "Beats" names; a relay's GPS runs all the
time (roadmap D80).

`AppCommand::Power` says whether external power, USB, is present. The driver sends it
right after the boot wakeup and at every change, and may repeat it. On external power
the node is a relay, on battery sleepy (spec "Nodes"): while it boots, from the report
on; running, from the change on, as spec "Sleepy nodes" says.

`AppEvent::Message` delivers a payload to this node's application.
The core never inspects a payload (spec "Phone link": the app owns identity and encryption).

`AppEvent::Boot { relays, ends }` tells the application how the node's first rotation goes
(spec "Channel at boot", "Phone link"): at boot, with no relay known and when the rotation
ends; each time the node learns a relay in it, with the relays it knows; and once at its
end, `ends` then `None`. A sleepy node's first rotation ends at the first relay it hears,
or at once when it resumes a stored anchor.

`Action::Store(stored)` asks the driver to keep `stored` across a reboot, in place of
what it kept before (spec "Implementation notes"). The core emits it at boot and then only
when the state changes: the home channel, but a carried sleepy node's, the place a fixed
node stands, the carried flag and a fixed sleepy node's anchor. The driver hands the last one back in `Config::stored`
when it builds the core at the next boot; a fresh board has `None`. `Stored::record` lays
it out as 32 bytes with a checksum, and `Stored::from_record` takes back only a whole
record. The firmware appends records to a page of flash (`bark-fw/src/store.rs`); the
simulator keeps the last one across a `restart` and drops it at a `wipe`.

## 5. What the driver must simulate or implement

| Driver obligation | Simulator | Firmware |
|---|---|---|
| Clock | event-heap time | embassy monotonic |
| Wakeup when `now >= next_wakeup()` | heap entry per core, lazily invalidated | one embassy timer |
| `Listen` | update the per-channel listener set | `rx_switch_channel` |
| `Sniff` | receives a frame only if a window of the cycle, started at the `Sniff` and again after each frame it takes, holds four symbols of its preamble; the time is counted | SetRxDutyCycle(384, 22 344) from STDBY_RC, the receive timer stopped at the header and not at the preamble, the receiver programmed for the long preamble; started again after each packet; the chip woken by NSS before any other command (`bark-driver`, `bark-fw/src/radio.rs`) |
| `Cad` | deaf for CAD duration, result from the reception model | `prepare_for_cad` + `cad` |
| `Rssi` | deaf for retune + sample, dB-sum at the node | `get_rssi` after retune |
| `Transmit` | deaf for airtime, deliveries to hearers via the reception model | `prepare_for_tx` + `tx` |
| `Received` | reception model | `rx` with CRC check |
| Seed | per-run seed → per-node stream | nRF RNG at boot |
| Power (§4), after the boot wakeup and at each change | the power the placement gives a node, or `[power]`'s process (`bark-sim/src/power.rs`) | VBUS detection: `USBREGSTATUS.VBUSDETECT`, read again at each POWER interrupt (`bark-fw/src/power.rs`) |
| GPS (§4) | `Action::Locate` starts a search; a fix after a hot or a cold start under the sky, none indoors for the share `[gps] no_sky` (`bark-sim/src/gps.rs`) | none yet: the L76K driver is a Phase 2 item, `bark-driver` logs the request |
| Store (§4) | the last `Stored` per node, kept across `restart`, dropped at `wipe` | a log of records in the flash page `STORE` (`bark-fw/src/store.rs`, `bark-image/src/layout.rs`) |

## 6. Memory

`Relay` holds its tables inline (spec "Implementation notes"). Sizes as built, 2026-10-25 (roadmap D75, D77 and the airtime correction), checked by the
test in `tables/mod.rs`:

| Table | Entries × size | Total |
|---|---|---|
| Neighbours (spec "Neighbour table") | 64 × 72 B | 4.5 KiB |
| Heard sleepy nodes (spec "Heard table") | 256 × 32 B | 8.0 KiB |
| Delivery queue (spec "Delivery queue") | 41 × 264 B | 10.6 KiB |
| Message records (spec "Message records") | 1571 × 40 B | 61.4 KiB |
| Send queue (spec "Send queue") | (39 + 254) × 280 B | 80.1 KiB |
| Delivered window (spec "Delivery queue") | 41 × 16 B | 0.6 KiB |
| Duty budget (spec "Duty cycle cap") and busy estimate (spec "Busy estimate") | 16 B + 368 B | 0.4 KiB |
| **All** | | **165.7 KiB** |

The send queue's 39 regular places follow from the duty bucket, and its 254 hop places, one
for each hop count a message can be sent on with, keep overload from dropping messages
(spec "Send queue", roadmap D31). The records hold the 1278 messages a relay can handle in the
463.4 s a full send queue takes to drain at one ban per place, plus the 293 the send queue
holds, whose records are never given up (spec "Message records"). The delivery queue's 41 places hold what
arrives in one delivery TTL for a full heard table (spec "Delivery queue"). The whole `Relay` is
170 760 B (166.8 KiB, measured 2026-10-25): the tables and, in about 1 KiB around them, the
medium access, the hand-over, its one owed reply, its application's message while it
waits, and its stored state as handed back and as last kept, 56 B. Every board runs it, since every node may be a relay (spec "Nodes"). A
`SleepyNode` is 151 744 B (148.2 KiB), without the heard table and the delivery
queue, 18.6 KiB (roadmap D58), for a driver whose node never switches, as the simulator's
tests can have. There is no fixed target;
the sizes follow from the design. The board's application has 255 KiB of RAM
(`bark-image/src/layout.rs`); the BLE controller takes under 5 KiB
(`implementation-research-2026-10-14.md`) and Embassy and the stacks about 16 KiB
(spec "Implementation notes"), which leaves 234 KiB, 67 KiB more than a node needs. Checked by the tests in
`tables/mod.rs` and `node/relay.rs`.

That holds only if the relay is not built on the stack. `Relay::new` returns a relay by
value, and on the board's target it takes 174 KiB of stack to build one (measured
2026-10-15 in the firmware's release build), more than the 88 KiB left beside a
relay in static memory. So `Relay::reset(config, seed)` sets up a relay in place, as
`Relay::new` would have, whatever it held before. A driver can keep the relay in a static
holding `Relay::UNSET`, a relay with empty tables built at compile time, or holding zero
bytes, which are a valid relay. The firmware does the second: a static of zero bytes is in
`.bss`, which takes no flash, where `Relay::UNSET`, whose empty tables are not all zeros,
would put the whole relay in flash, to be copied to RAM at boot. A constant in
`bark-fw/src/node.rs` stops the firmware building if zero bytes stop being a valid relay,
and a test in `bark-driver` checks that a relay reset in zeroed memory runs as a new one.
The simulator, on the host, uses `Relay::new`.

`Action::Transmit` and `AppEvent::Message` carry a 255-byte frame by value: drivers hold one
action at a time, and the core never allocates.

## 7. Wire-format choices made here (design O2)

- Position: 56-bit big-endian, latitude in the top 25 bits, longitude in the next 26, five
  spare bits zero (`position.rs`).
- Short position (spec "Positions on the wire", roadmap D14): the low 16 bits of the latitude count, then
  the low 16 bits of the longitude count, big-endian (`ShortName` in `position.rs`). The
  receiver rebuilds the full position from its own as the one candidate within half a
  period (32 768 counts) on each axis; both axes wrap as their wire fields do.
- Node ID (spec "Node identity"): six opaque bytes; the firmware takes the low 48 bits of the
  chip's FICR DEVICEID, all of DEVICEID[0] and the low 16 bits of DEVICEID[1]; the
  simulator gives node `i` the ID `(i + 1) × 0x9E3779B97F4B` modulo 2⁴⁸, or one drawn from
  the run's seed (`ids = "random"`).
- Message ID (spec "Packet formats", D15): 32-bit FNV-1a over `dst_pos || dst_id || payload`,
  `dst_pos` in its 7 wire bytes (`MsgId::of` in `ids.rs`; `Data::msg_id` on a decoded
  DATA).
- Everything else in spec "Packet formats" is encoded by the codecs in `wire.rs`. Choices made
  there, 2026-10-14:
  - Multi-byte integers (short positions, message IDs) are big-endian; a node ID is six
    opaque bytes in order, compared whole.
  - ACK `to` of a DATA is its `src_hop`.
  - A sleepy node's own DATA has `hops` zero. A relay sending for its own application
    sends `hops` 1, as if it had received the message from the application at 0: a
    receiver takes `hops` 0 for a sleepy node's own message (spec "Packet sender").
  - Application payload at most 226 bytes, what remains of 255 after the 29-byte DATA
    header; the decoder rejects anything longer.
  - DATA `time_left` counts units of 2.5 s (`wire::TTL_UNIT`), rounded down at each
    transmission (spec "Receiving DATA"); a DATA to a sleepy node carries what the message
    had when it reached the holder.
  - The first byte's top bit marks a heartbeat, whose channel field holds 0 to 12; the
    firmware's bench frames put 15 there, which nodes reject (`wire::bench_first`).
    Below a clear top bit, the next bit tells DATA from the ACK, and the six bits under
    it are `aux` (roadmap D76).
  - The second byte is the format version (roadmap D77). The core writes it: 1, its only
    format, in DATA and the ACK, and in a heartbeat as the newest it speaks. It rejects a
    DATA or ACK of another format, reads every heartbeat whatever its format, and keeps
    each neighbour's in `Neighbour::format`. A driver neither sets nor reads it.
  - An ACK's `aux` is 0 when it takes the offer and the wait the refuser asks for when
    it refuses, 1 to 63 bans of 1.58 s (spec "Bans"); taken or refused, an
    ACK is 18 bytes. A DATA's `aux` is the sender's home channel, and a DATA with either
    of the two high bits of `aux` set is rejected. A fixed-size packet with trailing
    bytes is rejected.

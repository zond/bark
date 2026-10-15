# Core interface

**Status:** fixed for Phase 0, 2026-10-14; the application interface (§4) revised
2026-10-15, roadmap D29. Roadmap decision D3. The types are in
`bark-proto/src/{time,radio,position,ids,event,node}.rs`; this page is the contract
around them that a driver (the simulator, the firmware) must honour and the core may rely on.

## 1. Shape

The core is a state machine behind the `Core` trait:

```rust
fn handle(&mut self, event: Event<'_>, now: Instant, emit: &mut dyn FnMut(Action));
fn next_wakeup(&self) -> Option<Instant>;
fn accepts_send(&self, now: Instant) -> bool;
```

Two roles, two structs: `Relay` and `Leaf`. `Node` is an enum over both for a driver that
switches role at run time (the firmware). The simulator holds `Vec<Relay>` and `Vec<Leaf>`
directly, so twenty thousand leaves cost twenty thousand `Leaf`s, not twenty thousand
relay-sized enums.

The core never blocks, never sleeps, never allocates, and never reads a clock: `now` is an
argument. Every random choice comes from the `Rng` seeded at construction, so a core's
behaviour is a function of its config, its seed and its event sequence.

## 2. Time

`Instant` and `Duration` are integer microseconds; there is no other time unit to mix
them up with. `now` never decreases across calls on one core. Different cores have unrelated clocks; the protocol has no network time (design §11).

Wakeups: after every `handle` the driver reads `next_wakeup()`. When its clock reaches that
instant it calls `handle(Event::Wakeup, now)`. Late is allowed; early is not. The core keeps
all its own deadlines internally and exposes only the earliest, so there is one timer per
core and no timer identifiers cross the boundary.

Boot: the first `handle` call a core receives is `Event::Wakeup`, delivered at start
whatever `next_wakeup()` says. The core's boot listen (design §6.2) starts there.

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
   `Config::tx_power_dbm` on `Frame::channel`, then returns to the resting channel.
5. **Listen-before-talk is the core's job.** The driver never checks the channel on its own;
   the core issues `Cad` or `Rssi` and decides.
6. **Duty cycle is the core's job.** The core keeps the duty-cycle budget (design §9.4) and
   defers; the driver never drops a frame.
7. **`Rssi` on the resting channel is not an excursion.** The radio keeps receiving while it
   reads its own channel, so the core may issue it at any time; it still completes with
   `RssiSample`. `Rssi` on another channel costs a retune and is an excursion.
8. **The driver may hold a `Cad` or `Rssi` excursion while a reception is in progress**, by at
   most one maximum frame airtime, so that a sweep does not cut a packet short (design §6.1).
   `Transmit` is never held. The core's timeouts allow for the hold.
9. **`Action::Trace` carries no obligation.** It reports protocol events (a dropped message
   and why, a deferral, a forward, a backtrack, a demotion, a channel move) for the simulator's
   metrics and the firmware's log. A driver may ignore it.

The modulation is fixed (SF5, 125 kHz, CR 4/5, `radio.rs`) and is not negotiated.

## 4. Application

Leaf and relay have the same application interface: the readiness query
`accepts_send`, the commands `AppCommand::Send` and `AppCommand::SetPosition`, and the
one event `AppEvent::Message`.

`AppCommand::Send` originates a message to `(dst_pos, dst_leaf)` with an opaque payload of
at most 234 bytes (§7). The driver offers one only while `accepts_send(now)` is true, and
asks again after every `handle`, so an application waits for its node; a firmware's async
send awaits it. The core reports nothing back about the message, not even the first hop's
ACK, which a user could read as delivered: from the moment it is taken the node is either
sending it or waiting to. The protocol has no end-to-end acknowledgement (design §9.1), so
whether the message arrived is for the app to find out inside its payload. A message the
node drops is reported only as `Trace::Dropped` (§3 rule 9).

Each role holds one message of its application's at a time, until its first hop takes it.
A relay accepts a message while it is running, holds none, and its send queue takes a full
frame on top of what it holds (design §9.3). When only its bucket keeps it from accepting,
its `next_wakeup()` includes the moment the bucket will have refilled enough. A relay whose
remaining neighbours are all banned keeps the message and tries again when the first ban
ends, or later after refusals in a row (design §9.3); one for its own leaf that finds the
delivery queue full waits for a place (design §9.2.5). A leaf accepts a message while it
holds none (design §10.2).

A `Send` while `accepts_send` is false, or with a longer payload, breaks the contract like
a second outstanding excursion: the core ignores it, and the test and simulator drivers
panic or never offer it.

`AppCommand::SetPosition` updates own position (relay identity changes with it, so the
driver does this rarely). `AppEvent::Message` delivers a payload to this node's application.
The core never inspects a payload (design §11: the app owns identity and encryption).

## 5. What the driver must simulate or implement

| Driver obligation | Simulator | Firmware |
|---|---|---|
| Clock | event-heap time | embassy monotonic |
| Wakeup when `now >= next_wakeup()` | heap entry per core, lazily invalidated | one embassy timer |
| `Listen` | update the per-channel listener set | `rx_switch_channel` |
| `Cad` | deaf for CAD duration, result from the reception model | `prepare_for_cad` + `cad` |
| `Rssi` | deaf for retune + sample, dB-sum at the node | `get_rssi` after retune |
| `Transmit` | deaf for airtime, deliveries to hearers via the reception model | `prepare_for_tx` + `tx` |
| `Received` | reception model | `rx` with CRC check |
| Seed | per-run seed → per-node stream | nRF RNG at boot |

## 6. Memory

`Relay` holds its tables inline (design §15). Sizes as built, 2026-10-15, checked by the
test in `tables/mod.rs`:

| Table | Entries × size | Total |
|---|---|---|
| Neighbours (§8.2) | 64 × 48 B | 3.0 KiB |
| Leaves (§10.2) | 256 × 16 B | 4.0 KiB |
| Delivery queue (§10.4) | 41 × 256 B | 10.3 KiB |
| Message records (§9.2.2) | 1788 × 32 B | 55.9 KiB |
| Send queue (§9.3) | (40 + 254) × 272 B | 78.1 KiB |
| Delivered window (§10.4) | 64 × 16 B | 1.0 KiB |
| Duty budget (§9.4) and busy estimate (§6.1) | 16 B + 348 B | 0.4 KiB |
| **All** | | **152.7 KiB** |

The send queue's 40 regular places follow from the duty bucket, and its 254 hop places, one
for each hop count a message can be sent on with, keep overload from dropping messages
(design §9.3, roadmap D31). The records hold the 1494 messages a relay can handle in the
449.7 s a full send queue takes to drain at one ban per place, plus the 294 the send queue
holds, whose records are never given up (§9.2.2). The delivery queue's 41 places hold what
arrives in one delivery TTL for a full leaf table (§10.4). The whole `Relay` is 157 040 B
(153.4 KiB): the tables and, in under 1 KiB around them, the medium access, the hand-over,
its one owed reply and its application's message while it waits. There is no fixed target;
the sizes follow from the design. The board's application has 255 KiB of RAM
(`bark-fw/memory.x`); the BLE controller takes under 5 KiB
(`implementation-research-2026-10-14.md`) and Embassy and the stacks about 16 KiB (design
§15), which leaves 234 KiB, 80 KiB more than the relay needs. Checked by the tests in
`tables/mod.rs` and `node/relay.rs`.

`Leaf` is 736 B, under the 1 KiB target, because the capacity scenario holds twenty
thousand of them (test in `node/leaf.rs`).
`Action::Transmit` and `AppEvent::Message` carry a 255-byte frame by value: drivers hold one
action at a time, and the core never allocates.

## 7. Wire-format choices made here (design O2)

- Position: 56-bit big-endian, latitude in the top 25 bits, longitude in the next 26, five
  spare bits zero (`position.rs`).
- Relay short name (design §8.1, roadmap D14): the low 16 bits of the latitude count, then
  the low 16 bits of the longitude count, big-endian (`ShortName` in `position.rs`). The
  receiver rebuilds the full position from its own as the one candidate within half a
  period (32 768 counts) on each axis; both axes wrap as their wire fields do.
- Message ID (design §14, D15): 32-bit FNV-1a over `dst_pos || dst_leaf || payload`,
  `dst_pos` in its 7 wire bytes (`MsgId::of` in `ids.rs`; `Data::msg_id` on a decoded
  DATA).
- Everything else in design §14 is encoded by the codecs in `wire.rs`. Choices made
  there, 2026-10-14:
  - Multi-byte integers (short names, message IDs) are big-endian; `leaf_id` is four
    opaque bytes in order. ACK `from` is a short name or a leaf ID, four bytes either way,
    compared whole.
  - The ACK of an ATTACH carries the leaf's ID as `id` and the relay's short name as
    `from`: an ATTACH is not a message and has no message ID.
  - A leaf's own DATA has `src_hop` zero and `hops` zero. A relay sending for its own
    application sends `hops` 1, as if it had received the message from the
    application at 0: a receiver takes `hops` 0 for a leaf's packet (design §9.2.1).
  - A DELIVER to a duty-cycled or hybrid leaf has a 2000-symbol preamble, the 512 ms
    of design §10.5 (`RxMode::preamble_symbols`).
  - Application payload at most 234 bytes, what remains of 255 after the 21-byte DATA
    header; the decoder rejects anything longer.
  - DELIVER carries at most the same 234-byte payload: it only ever carries a DATA
    payload onward.
  - ATTACH `aux`: receive mode 0 always listening, 1 duty-cycled, 2 hybrid (§10.5).
  - `aux` is unused in ACK, DELIVER and BUSY: sent as zero, ignored on receipt. A
    fixed-size packet with trailing bytes is rejected.
  - BUSY `from` is a relay's short name, big-endian like every other, and
    `id_low` the low 16 bits of the refused message's ID, big-endian
    (roadmap D30). A sender matches both against its hand-over.

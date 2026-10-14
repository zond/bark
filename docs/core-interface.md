# Core interface

**Status:** fixed for Phase 0, 2026-10-14. Roadmap decision D3. The types are in
`bark-proto/src/{time,radio,position,ids,event,node}.rs`; this page is the contract
around them that a driver (the simulator, the firmware) must honour and the core may rely on.

## 1. Shape

The core is a state machine behind the `Core` trait:

```rust
fn handle(&mut self, event: Event<'_>, now: Instant, emit: &mut dyn FnMut(Action));
fn next_wakeup(&self) -> Option<Instant>;
```

Two roles, two structs: `Relay` and `Leaf`. `Node` is an enum over both for a driver that
switches role at run time (the firmware). The simulator holds `Vec<Relay>` and `Vec<Leaf>`
directly, so twenty thousand leaves cost twenty thousand `Leaf`s, not twenty thousand
relay-sized enums.

The core never blocks, never sleeps, never allocates, and never reads a clock: `now` is an
argument. Every random choice comes from the `Rng` seeded at construction, so a core's
behaviour is a function of its config, its seed and its event sequence.

## 2. Time

`Instant` and `Duration` are integer microseconds, nothing else: a millisecond count can
never be added to a tick count by accident, because there is no tick count. `now` never decreases across calls on one
core. Different cores have unrelated clocks; the protocol has no network time (design §11).

Wakeups: after every `handle` the driver reads `next_wakeup()`. When its clock reaches that
instant it calls `handle(Event::Wakeup, now)`. Late is allowed; early is not. The core keeps
all its own deadlines internally and exposes only the earliest, so there is one timer per
core and no timer identifiers cross the boundary.

## 3. Radio

The driver owns one half-duplex LoRa radio in one of these states: listening on a channel,
or performing an excursion. Excursions are `Action::Cad`, `Action::Rssi` and
`Action::Transmit`. `Action::Listen(c)` is not an excursion: it sets the resting channel,
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
6. **Duty cycle is the core's job.** The core keeps the rolling budget (design §9.4) and
   defers; the driver never drops a frame.

The modulation is fixed (SF5, 125 kHz, CR 4/5, `radio.rs`) and is not negotiated.

## 4. Application

`AppCommand::Send` originates a message to `(dst_pos, dst_leaf)` with an opaque payload;
the core answers with `AppEvent::Accepted(msg_id)` and later `Delivered` or `Failed`.
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

`Relay` holds its tables inline: 64 neighbours, 256 leaves, 64 queued deliveries, a recent-RX
window. Target under 16 KiB (design §15 RAM budget). `Leaf` target under 512 bytes, because
the capacity scenario holds twenty thousand of them and they should fit in cache.
`Action::Transmit` and `AppEvent::Message` carry a 255-byte frame by value: drivers hold one
action at a time, and the core never allocates.

## 7. Wire-format choices made here (design O2)

- Position: 56-bit big-endian, latitude in the top 25 bits, longitude in the next 26, five
  spare bits zero (`position.rs`).
- Position hash for DFS visited lists: FNV-1a over the seven wire bytes folded to 16 bits.
- Everything else in design §14 is encoded by the Phase 0 codecs and documented there.

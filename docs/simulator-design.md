# Simulator design

**Status:** proposal, 2026-10-14. For discussion before Phase 1 starts.
Spec references are to `hrafnar/docs/civilian-433-mesh-design.md`.

## 1. Purpose

Answer, before and after hardware exists:

1. Does the protocol work as specified: discovery within one rotation, greedy forwarding with DFS recovery, leaf attach and delivery, channel selection that converges and does not oscillate?
2. What is the capacity in a realistic city deployment, and where does it bind first: channel busy fraction, per-relay 10% duty cycle, delivery queue, neighbour table?
3. Which values for the open thresholds (O3, O4) and which unknown radio parameters (adjacent-channel rejection, hearing radius, SF5 sensitivity) change the answer?

Non-goals: BLE, the phone app, crypto, GPS acquisition, battery chemistry. Leaf power modes (§10.5) are modelled only as receive-window behaviour and preamble length.

## 2. What the user supplies

A **scenario**: 

- **Nodes.** Each has a position (metres on a local grid; the simulator converts to the 51-bit WGS-84 wire form around a chosen origin so the real codec runs), a role (relay or leaf), and for relays whether it is rooftop or windowsill (affects nothing in the protocol, only the link generator). Nodes can be placed one by one, on a lattice with jitter, or by density per km².
- **Links.** A sparse pairwise receive-power table `rx_dbm[a][b]`: the power at b when a transmits at its configured TX power. This is the primary interface. Only pairs that can interact are stored: a pair whose receive power is below the sensitivity floor minus the interference margin (a weak signal can still add to the noise at a receiver, so the cut-off sits some dB below sensitivity; parameter) has no entry and never meets in the event loop. Generators fill the table in: log-distance path loss with a chosen exponent, a rooftop-to-rooftop bonus, indoor wall penetration loss per node, optional shadow-fading noise with a seed; each generator takes a cut-off distance from its own parameters so it never evaluates the full N² (20k leaves would otherwise mean 4×10⁸ entries). A table can also be loaded from a file so field measurements replace the model later; absent pairs are out of range.
- **Per-channel and time-varying links.** Each stored pair carries one base value plus a per-channel offset that evolves in time, so the table is really `rx_dbm[a][b][channel](t)`. Pure path loss is the same on all 13 channels: the band spans 1.74 MHz, which is 0.035 dB of free-space difference end to end. What does differ per channel is multipath fading, which is frequency-selective once channels are further apart than the coherence bandwidth (hundreds of kHz to a few MHz in a city), and the antenna match of a cheap whip, which can tilt a few dB across the band per node. Fading changes over time as people, vehicles and the leaf itself move. Default generator: a per-pair, per-channel Gauss-Markov offset with a standard deviation of a few dB and a correlation time of seconds to minutes, seeded; static nodes get a slower process than moving ones. The offsets are the reason a relay hears a neighbour's heartbeat on channel 3 and misses it on channel 9, the reason a home channel that was quiet becomes busy without any new transmitter, and therefore what exercises the move rule (§6.3), the neighbour-table expiry and the per-hop retry logic. The interferers above are the other, larger source of per-channel difference. A loaded table may give one value per pair or one per channel; the time process is applied on top in either case unless disabled.
- **Interferers.** Per channel, foreign busy sources (key fobs, LPD433 handhelds): a position, a power, and an on/off process. They are heard by RSSI and by CAD only if they look like LoRa (a flag).
- **Traffic.** Message arrival as a Poisson process per leaf with a rate, a payload-length distribution, and a destination distribution (uniform, or distance-biased so most texts are local). Leaves may be static, random-walk, or follow a trace. Leaves also have an availability process: periods where the phone is dark (pocket, metro) and the leaf does not ACK deliveries.
- **Radio parameters.** Everything the firmware will measure in Phase 2, with the datasheet value as default: SF5 sensitivity, noise floor, co-channel capture threshold, adjacent-channel rejection at ±1 and ±2 channels, CAD duration and miss probability, retune time, TX/RX turnaround, sweep schedule.
- **Protocol parameters.** The O3 thresholds, table sizes, intervals; defaults from the design.
- **Run.** Duration, seed, which metrics to log and at what cadence.

## 3. Model

### 3.1 Time and events

Discrete-event, microsecond resolution, single-threaded, deterministic for a given seed. A binary heap of `(time, sequence, event)`. Event kinds: node timer, transmission start, transmission end at each receiver, CAD/RSSI sample result, traffic arrival, leaf movement, interferer toggle.

### 3.2 Radio state per node

`Listening(channel)`, `Deaf(reason, until)` for retune, sweep, CAD, TX and turnaround, `Transmitting(channel, until)`. The protocol core asks for actions (`SetChannel`, `Cad`, `Rssi`, `Transmit`, `Sleep`); the simulator applies the datasheet timings and reports results as events. A node that is not `Listening(c)` for the whole duration of a packet on `c` does not receive it. This is how sweep loss (§6.1), heartbeat deafness (§7) and the per-hop retune cost (§9.3) are measured rather than assumed.

### 3.3 Reception

For a transmission from a on channel c, each node b that is listening on c (or on c±1, c±2 for adjacent-channel leakage) evaluates:

1. **Detectability.** `rx_dbm[a][b][c](t) ≥ sensitivity`, evaluated at the start of the packet; a packet is short compared with the correlation time, so the value is held for its duration. SF5 at 125 kHz is not in the datasheet table; default −119 dBm, extrapolated 2.5 dB per SF step from SF7's −124 dBm. Parameter.
2. **Capture.** LoRa receivers lock onto the first preamble they detect. The simulator tracks, per listening node, the packet it is locked to. A second same-channel arrival during the locked packet's preamble steals the lock if it is stronger by the capture threshold (datasheet co-channel rejection: 5 dB at SF7, SF5 unlisted; parameter, default 5 dB). After lock, the packet survives if its power exceeds the sum of all overlapping interferers, each attenuated by the rejection for its channel offset (co-channel 0 dB, ±1 channel ACR_1, ±2 channels ACR_2; both parameters, since the datasheet only gives 60 dB at 1.5 × BW offset), by the capture threshold. Otherwise it is lost, and the receiver stays deaf until the stronger interferer ends (it is now locked to garbage).
3. **CRC.** Hardware CRC is assumed perfect: a packet is either delivered intact or not at all.

Interferers participate in step 2 as transmitters that never carry packets.

### 3.4 Listen-before-talk

CAD: returns busy if any LoRa transmission on the channel at the node exceeds sensitivity, with a configurable miss probability (AN1200.48 gives detection rates; default 0). Duration per datasheet, about 1 ms at SF5 with 4 symbols. RSSI: returns the dB sum of everything on the channel at the node, including interferers and thermal noise; busy if above noise floor + 6 dB. Both make the node deaf for their duration.

### 3.5 Busy estimate and sweep

The once-per-second 13-channel sweep (§6.1) is modelled explicitly: 13 retunes, 13 RSSI samples, deaf for the total. The protocol core keeps `busy[c]` itself, so the move rule (§6.3) is the real code.

### 3.6 Airtime and duty cycle

Time on air from the SX126x datasheet formula for SF5/SF6 (6.25-symbol overhead, 12-symbol minimum preamble), computed from the actual encoded packet length. The protocol core's rolling one-hour budget is the real code; the simulator records when it blocks forwarding.

### 3.7 Leaves and phones

A leaf is a node whose protocol core is in leaf role: camps on its relay's channel, attaches, keeps alive, receives deliveries. Its receive mode (§10.5) sets whether it hears heartbeats continuously or only during awake rotations, and sets the preamble length on packets to it. The phone behind it is only an availability process and a traffic source. Movement re-evaluates the leaf's link row and column when its position changes (regenerated for that node from the generator within its cut-off distance, using a spatial index over node positions; a loaded table cannot move).

## 4. Metrics

Logged per run as CSV/JSON, with the scenario and seed embedded:

- Delivery ratio and NACK ratio, by distance bucket and hop count.
- End-to-end latency distribution (origination to leaf ACK), split into per-hop wait, LBT backoff, retransmits, queue dwell.
- Per-channel busy fraction over time, at a chosen set of observer nodes.
- Per-relay airtime per hour, forwarded bytes, duty-cycle blocks, delivery queue depth and expiries.
- Heartbeat: time for a new node to learn all neighbours in range; neighbour table size, evictions, false expiries.
- Home-channel moves per relay per hour, and the distribution of relays per channel in a collision domain (does measurement spread load like the §13 argument says?). Moves attributed to real interferers versus to fading alone, since the latter are a cost without a benefit.
- DFS: invocations, visited-list length, backtrack depth; messages dropped when the list fills.
- Collision accounting: packets lost to capture, to adjacent-channel leakage, to receiver deafness (retune, sweep, own TX), each separately, so the §6.1 and §9.3 cost arguments can be checked against numbers.

## 5. Architecture

```
bark-proto (no_std + alloc)
  node::Node            state machine: handle(Event, Instant) -> Actions
  wire::*               §14 codecs, position codec
  tables::*             neighbour, leaf, delivery queue, recent-rx, visited
  mac::*                busy estimator, channel selection, heartbeat schedule,
                        per-hop transfer, duty-cycle budget
  routing::*            greedy, DFS, owner rule

bark-sim (std)
  engine                event heap, clock, RNG
  radio                 per-node radio state, reception model (§3.3), timings
  links                 sparse rx_dbm table, generators, file load/save
  scenario              builder API + TOML file
  traffic               arrivals, destinations, leaf movement, availability
  metrics               collectors, CSV/JSON writers
  bin/bark-sim          run a scenario file, print a summary
```

The core's interface, to be fixed in Phase 0:

```rust
pub enum Event<'a> {
    Timer(TimerId),
    Received { channel: u8, bytes: &'a [u8], rssi_dbm: i16 },
    CadDone { channel: u8, busy: bool },
    RssiSample { channel: u8, dbm: i16 },
    TxDone,
    App(AppCommand),          // send, set role, set position
}

pub enum Action<'a> {
    SetChannel(u8),
    StartCad(u8),
    ReadRssi(u8),
    Transmit { channel: u8, preamble_symbols: u16, bytes: &'a [u8] },
    ArmTimer(TimerId, Instant),
    App(AppEvent),            // delivered, nack, status
}
```

Firmware maps `Action` onto `lora-phy` calls and embassy timers; the simulator maps them onto heap events. The core never blocks and never allocates per packet beyond its bounded tables.

### Core tables

| module | keyed by | bound | purpose |
|---|---|---|---|
| `tables::visited` | 16-bit position hash (§8.1) | per message, carried in the packet | DFS: which relays this message has already tried |
| `tables::recent_rx` | 32-bit `msg_id` | fixed window | drop duplicates and late retransmits |
| `tables::neighbours` | 7-byte position | 64 entries | channel, last-heard, `fail_count`; expire after three rotations |
| `tables::leaves` | 4-byte leaf ID | 256 entries | attached leaves and their keepalive age |
| `tables::delivery` | `msg_id` | 64 messages, 10 min TTL | messages waiting for a dark leaf |

`routing` is a pure function of (destination, neighbour table, visited list, own position): forward to the neighbour nearest the destination if one is nearer than we are, otherwise DFS over unvisited neighbours, otherwise NACK. The owner rule routes a leaf's traffic to the relay it is attached to. Link generators in `bark-sim` include a terrain-class model (free-space loss plus per-cell attenuation along the path) beside the log-distance one.

## 6. Scenario catalogue

| # | Scenario | Checks | Bench twin |
|---|---|---|---|
| S1 | Three relays in a line, A cannot hear C; one leaf at C | greedy forward, ACK, demotion when B dies, NACK path | design step 4 |
| S2 | 1 km² block: 10 windowsill relays, 80 leaves, 10 texts/leaf/day | attach, keepalive load (≈2%), delivery, queue, dark phones | step 5 |
| S3 | S2 plus an interferer on channel 6 and three rooftop relays that all hear each other | avoidance, move hysteresis, no joint jumps | step 3 |
| S4 | 28 km² domain: 270 relays at 9.6/km², 20k leaves, §13 rates | capacity table, where the 10% cap binds, heartbeat load per channel ≈2.5% | field test |
| S5 | S4 with the central relay removed / with a 2× hearing radius / with ACR_1 from 20 to 60 dB | sensitivity of S4 to the unknowns | C1 |
| S6 | Cold start: all S4 nodes boot within 10 minutes | boot listen (§6.2), identity collision offset, convergence time | |

## 7. Performance target

S4 for one simulated hour in a few minutes on the dev machine. Leaves are idle except for one keepalive per minute and occasional texts, so the event count is dominated by relay heartbeats (270 × 6/min) and sweeps (270 × 60/min × 13 samples). Reception evaluation is the hot path: keep, per channel, the set of nodes currently listening and the set of active transmissions, and iterate only the transmitter's stored link row (§2: pairs out of range have no entry, so a transmission touches its hearers and nobody else).

## 8. Validation of the simulator itself

- Unit tests that a single packet between two nodes has the datasheet airtime.
- A two-node capture test reproducing the 5 dB co-channel figure.
- S1 run on the four real boards in Phase 3 with the same scenario file; the simulator's parameters are then adjusted until the measured latency and loss match.

# Simulator design

**Status:** 2026-10-20. The simulator in `bark-sim` as it is. Spec references are to
`protocol.md`.

## 1. Purpose

Answer, before and after hardware exists:

1. Does the protocol work as specified: discovery within one rotation, greedy forwarding with depth-first search (DFS) around gaps, a sleepy node's anchor and delivery, channel selection that converges and does not oscillate?
2. What is the capacity in a realistic city deployment, and where does it bind first: channel busy fraction, per-relay 10% duty cycle, delivery queue, neighbour table?
3. Which values for the open thresholds (O3, O4) and which unknown radio parameters (adjacent-channel rejection, hearing radius, SF5 sensitivity) change the answer?

Non-goals: BLE, the app, encryption, GPS acquisition, battery chemistry.

## 2. What the user supplies

A **scenario**:

- **Nodes.** Each has a position (metres on a local grid; the simulator converts to the 51-bit WGS-84 wire form around a chosen origin so the real codec runs), the sleepy setting (a relay or a sleepy node), and for relays whether it is rooftop or windowsill (affects nothing in the protocol, only the link generator). Nodes can be placed one by one, on a lattice with jitter, or by density per km². A placement of people's nodes by density can make a share of them relays (`relay_share`), as a person's node is while it is on USB power: drawn apart from the positions, so the people stand where they would with none. These relays stay people: they send and receive texts like the sleepy nodes and may walk or ride, but they do not go dark and are switched on in the people's window.
- **Links.** A sparse pairwise receive-power table `rx_dbm[a][b]`: the power at b when a transmits at its configured transmit power. This is the primary interface. Only pairs that can interact are stored: a pair whose receive power is below sensitivity minus an interference margin (a parameter; a signal too weak to decode still adds to the noise) has no entry and never meets in the event loop. Generators fill the table in: log-distance path loss with a chosen exponent, a rooftop-to-rooftop bonus, indoor wall penetration loss per node, optional shadow-fading noise with a seed; each generator takes a cut-off distance from its own parameters so it never evaluates the full N² (20k sleepy nodes would otherwise mean 4×10⁸ entries). A scenario can instead give the table pair by pair (`model = "table"`), so field measurements can replace the model; absent pairs are out of range.
- **Per-channel and time-varying links.** Each stored pair carries one base value plus a per-channel offset that evolves in time, so the table is really `rx_dbm[a][b][channel](t)`. Path loss is the same on all 13 channels: the band spans 1.74 MHz, 0.035 dB of free-space difference end to end. What differs per channel is multipath fading, which varies with frequency once channels are further apart than the coherence bandwidth (hundreds of kHz to a few MHz in a city), and the antenna match of a cheap whip, which can tilt a few dB across the band per node. Fading changes over time as people, vehicles and the node itself move. Default generator: a per-pair, per-channel Gauss-Markov offset with a standard deviation of a few dB and a correlation time of seconds to minutes, seeded; static nodes get a slower process than moving ones. The offsets are why a relay hears a neighbour's heartbeat on channel 3 and misses it on channel 9, and why a quiet home channel becomes busy without any new transmitter; they exercise the move rule (spec "Channel moves"), the neighbour-table expiry and the per-hop retry logic. Interferers (below) are the other, larger source of per-channel difference. A table given pair by pair may give one value per pair or one per channel; the time process is applied on top in either case unless disabled.
- **Interferers.** Per channel, foreign busy sources (key fobs, LPD433 handhelds): a position, a power, and an on/off process. Signal strength readings (RSSI) always see them; channel activity detection (CAD) sees them only if they are LoRa (a flag).
- **Traffic.** Message arrival as a Poisson process per sleepy node with a rate, a payload-length distribution, and a destination distribution (uniform, or distance-biased so most texts are local). A share of the sleepy nodes may walk (§3.7). Sleepy nodes also have an availability process: dark periods (pocket, basement, metro) in which they are out of radio reach.
- **Radio parameters.** Everything the firmware will measure in Phase 2, with the datasheet value as default: SF5 sensitivity, noise floor, co-channel capture threshold, adjacent-channel rejection at ±1 and ±2 channels, CAD duration and miss probability, retune time, transmit/receive turnaround, sweep schedule.
- **Protocol parameters.** The O3 thresholds, table sizes, intervals; defaults from the design.
- **Run.** Duration, seed, which metrics to log and at what cadence.

## 3. Model

### 3.1 Time and events

Discrete-event, microsecond resolution, single-threaded, deterministic for a given seed. A binary heap of `(time, sequence, event)`, and beside it one place per node for its core's timer, ordered the same way: the core sets a new timer after every event, and one it replaced is dropped rather than left in the heap. Event kinds: node timer, transmission start, transmission end at each receiver, CAD/RSSI sample result, traffic arrival, a walker's movement, interferer toggle.

### 3.2 Radio state per node

`Listening(channel)`, `Deaf(reason, until)` for retune, sweep, CAD, TX and turnaround, `Transmitting(channel, until)`. The protocol core emits actions (`Listen`, `Cad`, `Rssi`, `Transmit`, `Sniff`; `core-interface.md` §3); the simulator applies the datasheet timings and reports results as events. A node that is not `Listening(c)` for the whole duration of a packet on `c` does not receive it. This is how sweep loss (spec "Busy estimate"), heartbeat deafness (§7) and the per-hop retune cost (spec "Hand-over") are measured rather than assumed.

### 3.3 Reception

For a transmission from a on channel c, each node b that is listening on c (or on c±1, c±2 for adjacent-channel leakage) evaluates:

1. **Signal.** `rx_dbm[a][b][c](t)`, taken at the start of the packet and held for its duration: a packet is short compared with the correlation time.
2. **Lock.** LoRa receivers lock onto the first preamble they detect. The simulator tracks, per listening node, the packet it is locked to. A second same-channel arrival during the locked packet's preamble steals the lock if it is stronger by the capture threshold (datasheet co-channel rejection: 5 dB at SF7, SF5 unlisted; parameter, default 5 dB); otherwise it is lost to capture.
3. **Symbol errors.** While locked, each symbol is lost with a probability that falls with its signal against noise plus interference; the packet survives if every symbol does (CR 4/5 detects errors but cannot correct them). Interference is every overlapping signal, each attenuated by the rejection for its channel offset (co-channel 0 dB, ±1 channel ACR_1, ±2 channels ACR_2; both parameters, since the datasheet only gives 60 dB at 1.5 × BW offset), and counts as noise weighted so that the capture threshold against interference alone costs what sensitivity costs against noise alone. Interference is followed as signals start and end, so only the symbols that overlap it face it. The curve is the error rate of noncoherent detection of 32 orthogonal symbols, placed so that the datasheet's reference packet (64 bytes) loses 1% at sensitivity. SF5 at 125 kHz is not in the datasheet table; default sensitivity −119 dBm, extrapolated 2.5 dB per SF step from SF7's −124 dBm. Parameter. `docs/reception-research-2026-10-15.md` gives the sources and the curve. The parameter `reception = "threshold"` replaces the curve by a line: a packet survives if it is at or above sensitivity and above its interference by the capture threshold.
4. **Loss.** A packet lost before its header ends frees the receiver. One lost later keeps it deaf, locked to garbage, until the packet's end.
5. **CRC.** Hardware CRC is assumed perfect: a packet is either delivered intact or not at all.

Interferers participate in steps 2 to 4 as transmitters that never carry packets. The losses are drawn from the run's seed.

### 3.4 Listen-before-talk

CAD: returns busy if any LoRa transmission on the channel at the node exceeds sensitivity, with a configurable miss probability (Semtech application note AN1200.48 gives detection rates; default 0). Duration per datasheet, about 1 ms at SF5 with 4 symbols. RSSI: returns the dB sum of everything on the channel at the node, including interferers and thermal noise; busy if above noise floor + 6 dB. Both make the node deaf for their duration.

### 3.5 Busy estimate and sweep

The once-per-second 13-channel sweep (spec "Busy estimate") is modelled explicitly: 13 retunes, 13 RSSI samples, deaf for the total. The protocol core keeps `busy[c]` itself, so the move rule (spec "Channel moves") is the real code.

### 3.6 Airtime and duty cycle

Time on air from the SX126x datasheet formula for SF5/SF6 (6.25-symbol overhead, 12-symbol minimum preamble), computed from the actual encoded packet length. The protocol core's duty-cycle token bucket is the real code; the simulator records when it blocks forwarding.

### 3.7 Sleepy nodes

Scenario files place relays and sleepy nodes (spec "Sleepy nodes"; `role = "leaf"` in the files): a sleepy node chooses an anchor, beats on its channel and takes deliveries in the window after each beat. Between its windows its radio receives duty-cycled when its core says so (`Action::Sniff`), and takes a frame only if a window of its cycle, started at the sniff and again after each frame it takes, holds four symbols of the preamble (spec "Duty-cycled receive and the long preamble"). The time receiving duty-cycled, the time listening and the time on air give its radio current. The person carrying it is only an availability process and a traffic source.

**Movement.** With `[movement] fraction` above 0 that share of the sleepy nodes walk, chosen from the run's seed. A walker stops indoors at a point for a time drawn from an exponential with mean `stop_s`, then walks outdoors at `speed_m_s` in a straight line in a random direction, as far as an exponential with mean `trip_m` takes it, folded back at the edges of the area the nodes were placed in, and stops again; it starts at its placed point, partway through a stop. Indoors its links have the wall loss, outdoors not. `relay_fraction` is the share of the relays placed as people that walk, a node on a power bank; other relays never move. A walking relay's address is the position its core holds. A walker's links change as it moves, so none is stored: each frame with a walker at either end works its receivers out when it reaches the air, from the same path loss and cut-off as the stored links (`walk.rs`). Their shadowing has the scenario's deviation and varies over the moving end's position, from independent draws `shadow_decorrelation_m` apart, interpolated between; their fading has the scenario's deviation and varies over time, from independent draws one correlation time apart: the scenario's at a stop, and while either end walks the coherence time of a receiver moving at walking speed, 0.423 λ / v = 0.21 s. A new stop or walk draws its fading afresh. A walker's core is handed a GPS fix, its position then, each time it sends a heartbeat (spec "Beats": it refreshes its fix before each beat); a walking relay's at each of its heartbeats, through the checks of `[gps]` if they are on. A table given pair by pair cannot hold a walker. An interferer reaches the nodes that stay where they were placed, and the walkers where they are when it comes on and again every 10 s while it is on, the interval at which the walkers' grids are rebuilt: a walk of 14 m, which moves the path loss of a walker 300 m away by under a decibel. Like the stored links to interferers, those to walkers have the wall at a stop and no shadowing.

**Vehicles.** With `[vehicles] fraction` above 0 that share of the sleepy nodes that do not walk, and with `relay_fraction` that share of the relays placed as people that do not walk, ride in vehicles along one straight route, `route`, between its two ends. A rider stops indoors at a point of the route for a time drawn from an exponential with mean `stop_s`, then is driven at `speed_m_s` to a point drawn uniformly on the route, and stops again; it starts at a point drawn on the route, partway through a stop. Driven, its links are those of a walker outdoors less the vehicle's body, `loss_db` (default 9 dB, the mean car penetration loss of 3GPP TR 38.901 §7.4.3.2), and their fading changes over the coherence time at its speed, 29 ms at 10 m/s.

### 3.8 Research models

Models used to test single rules (roadmap D59, `docs/unproven-2026-10-18.md`), each off unless a scenario asks for it, so that every other run is as before.

- **Shared IDs** (`[[shared_ids]]`). `count` pairs of nodes of a `kind`, two relays, a relay and a sleepy node, or two sleepy nodes, that hear each other above sensitivity both ways (`reach = "in"`) or have no link either way (`"out"`), drawn from the run's seed among nodes that do not move, no node in two; the second takes the first's ID. With `share = false` the same pairs are drawn and only marked. The simulator's own lookups by ID, which only attribute counts, then find one of the two.
- **Dark groups** (`[[dark_groups]]`). The `count` sleepy nodes that do not move nearest a node go out of reach together at `at_s` for `for_s` seconds, whatever their own availability process says, and come back to it.
- **GPS faults** (`[gps]`). From `at_s`, `count` relays or sleepy nodes that do not move, drawn from the run's seed, get fixes `distance_m` off in a direction of their own (a jump) or moving away at `speed_m_s` (a drift). A relay with a fault takes a fix every second, the receiver's usual output; a sleepy node one before each beat. With `checks = true` its driver hands a fix to the core only if it is no farther from the last one handed over than 50 m for a relay, and 50 m and `carried_speed_m_s` times the time since for a sleepy node, and no farther than 3 km, the longest hearing radius (spec "Physical layer"), from at least one neighbour the core keeps. The third check of spec "Position and time", the satellites' signal-to-noise, is not modelled.
- **Fixed homes** (`[[homes]]`). A node keeps the channel given from its boot and never moves its home (`Config::home_override`), for scenarios that need two nodes on one channel.
- **Relays that stand down** (`[stand_down] neighbours`). Of the relays placed as people (§2), each that hears at least that many relays with margin both ways, before fading, runs as a sleepy node instead, decided once before the run in an order drawn from the run's seed, each counting the relays still standing. It keeps its links. This is where a rule that stands relays down would settle, without its switching (roadmap D68).
- **An interferer that appears** (`[[interferers]] from_s`). Silent until then, so that relays that have chosen their homes must move off its channel.

The summary's `research` section has what they need: the pairs, the groups and their members, the nodes with a fault and their fixes, the times relays moved their homes, the home channel of each relay once a minute, and once a minute, for walkers and riders with an anchor, whether their anchor hears them, by whether they move and how long they have stopped. Heartbeats lost at their receivers are split by why: to another frame, to a deaf receiver, below sensitivity. Each message's row has the relays at its origin's and its destination's addresses, its destination's anchor, whether its origin moved, and whether it reached a node with its destination's ID that is not its destination.

## 4. Metrics

Logged per run as CSV/JSON, with the scenario and seed embedded:

- Delivery ratio and where undelivered messages were dropped, by distance bucket and hop count.
- End-to-end latency distribution (origination to the destination's ACK), split into per-hop wait, listen-before-talk backoff, retransmits, queue dwell.
- Per-channel busy fraction over time, at a chosen set of observer nodes.
- Per-relay airtime per hour, forwarded bytes, duty-cycle blocks, delivery queue depth and expiries.
- Heartbeat: time for a new node to learn all neighbours in range; neighbour table size, evictions, false expiries.
- Home-channel moves per relay per hour, and the distribution of relays per channel in a collision domain (does measurement spread load like the spec "Capacity and sizing" argument says?). Moves attributed to real interferers versus to fading alone, since the latter are a cost without a benefit.
- DFS: invocations, returns to a parent, straight-back sends, hop counts; messages dropped at the hop cap and at the origin.
- Collision accounting: packets lost to capture, to adjacent-channel leakage, to receiver deafness (retune, sweep, own transmission), each separately, so the spec "Busy estimate" and "Hand-over" cost arguments can be checked against numbers.

## 5. Architecture

```
bark-proto (no_std, no allocation: D8)
  node                  configuration, the node, the Core trait
  node::relay           the node: medium access, heartbeat, relay choice;
                        relay::forward the search and the owner rule,
                        relay::delivery delivery to sleepy nodes
  node::transfer        per-hop transfer
  tables                neighbours, heard, delivery queue, delivered window,
                        message records, send queue, busy estimate,
                        duty-cycle budget
  wire, position        spec "Packet formats" codecs, the position codec
  sniff                 duty-cycled receive and the long preamble

bark-sim (std)
  sim                   event heap, the cores, the driver side of the contract
  radio, phy            reception model (§3.3), the radio's numbers
  links, walk           sparse rx_dbm table and its generator, walkers' links
  scenario              builder API + TOML file
  metrics, summary      counters, JSON and CSV
  trace                 per-node event trace
  main                  bark-sim: run a scenario file, print a summary
```

The core's interface is `core-interface.md`. Firmware maps its actions onto `lora-phy` calls and one embassy timer; the simulator maps them onto heap events.

### Core tables

| module | keyed by | bound | purpose |
|---|---|---|---|
| message records | message ID (spec "Packet formats") | 1855 records, least recently active given up, never one of a message held (spec "Message records") | DFS parent and tried set; recognises repeats |
| `tables::neighbours` | 4-byte node ID | 64 entries | position, channel, last-heard, `fail_count`; expire after three rotations, a sleepy node's after two beat periods |
| `tables::heard` | 4-byte node ID | 256 entries | sleepy nodes heard beating, their channel and window (spec "Heard table") |
| `tables::delivery` | message ID | 41 messages; when the queue is full, the oldest queued 10 min or longer makes way for a new one (spec "Delivery queue") | messages waiting for a sleepy node |

`relay::forward` chooses the next hop from (destination, neighbour table, message record, own position) as in spec "Next hop", and applies the owner rule of spec "Owner".

## 6. Scenario catalogue

The files in `scenarios/` are the ones the tests and the roadmap's
measurements use. Each file says in its header what it is for, and S1 to
S3 are the twins of spec "Implementation notes" steps 4, 5 and 3.

| File | What it is | What it checks |
|---|---|---|
| `s1-line.toml` | S1: three relays in a line, a sleepy node at each end, one sending to the other every 30 s; the middle relay is removed at 20 minutes | forwarding, ACK, delivery; every message dropped at the first relay after the removal |
| `s1b-hidden.toml` | S1 with both sleepy nodes sending to each other at the same instant every 30 s | two senders that cannot hear each other handing to the middle relay at once |
| `s2-block.toml` | S2: ten relays and eighty indoor sleepy nodes in a square kilometre for an hour | anchors, beats, delivery queue, nodes that go dark |
| `s3-interferer.toml` | S3: S2 with a foreign transmitter on channel 6 from 1800 s and three rooftop relays that all hear each other | relays move off the channel, not all at once; sleepy nodes follow their anchors |
| `corridor.toml` | five relays in a line, a sleepy node at each end, both sending every 30 s from the same instant | two flows meeting head on at every relay |
| `ring.toml` | eight relays on an octagon, a sleepy node at each, each sending to the one three relays on every 2 s | relays at their duty cap whose waiting messages could close a circle |
| `mesh-burst.toml` | 25 relays at random in a 2.5 km square, a sleepy node at each; every one sends across the mesh at the same instant every 10 minutes | routing around holes; every relay meeting crossing traffic at once |
| `mesh-spread.toml` | the same mesh and flows, each sender starting at its own time; the middle relay restarts halfway | the same without the burst; routing around a relay that has lost its tables |
| `s4-capacity.toml` | S4: 28 km², 269 relays of which 53 on roofs, 20 000 indoor sleepy nodes, 23 texts per node per day, one settled hour; switched on over 10 and 30 minutes, random IDs, drifting clocks, a fifth of the people walking | capacity and where it binds; `capacity-research-2026-10-15.md`, `realism-2026-10-18.md` |
| `strip.toml` | three S4 domains side by side, 16 km by 5.3 km, a centre of 2.6 km by 2.6 km with twice the people in the middle one, texts to anyone in the strip | texts across the city, about eight hops; where the centre binds; `realism-2026-10-18.md` |
| `dark-groups.toml` | four square kilometres at S4's densities, a relay at the centre and a building of 400 people beside it; a run adds the group that goes dark | the delivery queue while a group is dark; `unproven-2026-10-18.md` |

The meshes' links follow S2's model with shadowing drawn from the run's
seed, so each seed is a different mesh: some have holes, and some leave
a relay that no other reaches.

**Switch-on.** Every node is off until it is switched on, at a time
drawn from the run's seed, uniformly within a window from the start
that the scenario gives for each kind: `[boot] relays_s` for nodes that
are not sleepy and `sleepy_s` for sleepy ones. Off, a node hears
nothing, sends nothing and originates nothing; its dark spells start
at its switch-on. The run has settled `settle_s` after the last
switch-on: `settle_s` is how long one node takes to settle after its
own. S4 switches its relays on over 10 minutes and its sleepy nodes over
30 (the file argues it); both windows 0, the default, switch every node
on at once, as every measurement before roadmap D52 did.

**Node IDs.** With `ids = "random"` each node's ID is drawn from the
run's seed, as a chip's factory ID is random (spec "Node identity"), so
two nodes in reach share a heartbeat interval, set by the ID's last byte
(spec "Heartbeat"), at the chance rate of one pair in 256. A draw that is
zero or another node's ID is drawn again, since the simulator keys its
own tables by ID; among a domain's 20 000 nodes two would share one about
one run in twenty. The default, `"sequence"`, gives node `i` the ID
`(i + 1) × 0x9e3779b1`, whose last bytes go through all 256 values in
turn, so that relays share an interval about a sixteenth as often as by
chance.

**Clocks.** The simulator's time is true time. With `[clocks]
drift_ppm` each node's clock runs at its own rate, drawn from the run's
seed uniformly within that many parts per million of true time: at true
time `t` it reads `t (1 + rate)`. Each core is handed the time its own
clock reads, and a time a core gives back, a wakeup or the end of a
sleep, is taken as the first true microsecond at which its clock reads
it, so that no timer fires early. Durations the radio takes, airtime and
retunes, are true. The rate is fixed for the run: temperature does not
move it. The default, 0, keeps every clock true.

**Restart.** A scripted `restart` powers a node off and on at once. The
node keeps its ID and position, and
loses everything else, as a solar relay does after a night. Its core is
built afresh with a new random seed and boots: a relay listens before
choosing a home channel (spec "Channel at boot") and takes no traffic until then,
while its neighbours still list it.

**Pushback.** Each origin's application holds at most one message. The
traffic model's message is handed to the origin's core at the first
moment the core says it will take one (`core-interface.md` §4): at once
if it is ready, or after the next event that makes it so. A message the
traffic model has for an origin whose application still waits with the
previous one is skipped. Latency counts from when the traffic model had
the message, so a busy network shows as latency. The summary gives how
many messages waited, how many still waited at the end, the longest wait
and how many were skipped. The simulator tags each message with its ID,
which it computes as the core does (`MsgId::of`), to follow its traces.

**Accounting.** Every message the traffic model sends ends up in one of
these:

- **delivered** to the destination's application;
- **lost**, where a core reported it in a `Dropped` trace, with the node
  and the reason;
- **in flight**: it is still held when the run ends, by its origin's
  application waiting to hand it over, in a send queue or a delivery
  queue;
- **unaccounted**: none of these. It was lost without a trace.

A message is also marked **unreachable** when, at the start of the run,
no chain of relays that hear each other above sensitivity joins its
origin relay (the sender itself, or the sending sleepy node's anchor) to
the destination's relay. Such a message is lost whatever
the protocol does, unless links just below sensitivity carry some of its
packets (§3.3); the summary counts it apart and gives the delivery ratio
over the reachable ones too.

The scenario tests require that no message to a reachable destination is
unaccounted. A message to an unreachable destination can end untraced
when it has split in two (spec "Why the search finds a path"): both copies search the same
relays, and a relay whose search for one copy has ended absorbs the
other.

## 7. Performance target

S4 for one simulated hour in a few minutes on the dev machine. Sleepy nodes are idle except for one beat every 150 to 300 s and occasional texts, so the event count is dominated by relay heartbeats (270 × 6/min) and sweeps (270 × 60/min × 13 samples). Reception evaluation is the hot path: keep, per channel, the set of nodes currently listening and the set of active transmissions, and iterate only the transmitter's stored link row (§2: pairs out of range have no entry, so a transmission touches its hearers and nobody else).

## 8. Validation of the simulator itself

- `bark-sim/tests/radio.rs` checks that a single packet has the datasheet airtime, that capture holds at the 5 dB threshold and fails below it, and the loss curve of §3.3.
- In Phase 3, S1 runs on four boards with the same scenario file, and the simulator's parameters are adjusted until the measured latency and loss match.

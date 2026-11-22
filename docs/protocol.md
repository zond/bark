# bark protocol: Civilian 433 MHz Mesh

**Status:** live specification. It states rules, wire formats and numbers,
each number with its source. It gives no reasons: decisions and their
reasons are in `roadmap.md` by D-number; the text with its reasons as it
stood before 2026-11-21 is `spec-reasons-2026-11-21.md`, and what an
earlier cut removed is `spec-reasons-2026-11-11.md`.
[Open items](#open-items) lists what is undecided or unmeasured. Where
code and this text disagree, one is wrong and the fix is a commit to both.

A mesh of radio nodes, licence-free in the EU and Ukraine, that carries
messages between phone apps across a city without other infrastructure.
Not designed against deliberate jamming.

## Summary

| Item | Decision |
|---|---|
| Hardware | LILYGO T-Echo, 433 MHz build; one firmware for every node ([hardware](#hardware)) |
| Band | 433.050–434.790 MHz, 10 mW ERP, ≤10 % duty cycle per node ([regulatory limits](#regulatory-limits)) |
| Modulation | LoRa SF5, 125 kHz, coding rate 4/5, fixed network-wide ([physical layer](#physical-layer)) |
| Channels | 13 × 125 kHz; each node chooses its **home channel**, the one it listens on, by measuring how busy each is; no fixed plan, no time sync ([home channel selection](#home-channel-selection)) |
| Medium access | Listen on the home channel except to transmit; ACKs go at once on the sender's home channel; everything else listens before talking; no time slots |
| Heartbeat | A 13-byte broadcast of ID, position and home channel, on each channel in turn ([heartbeat](#heartbeat)) |
| Neighbour discovery | Passive: a node hears every relay in range once a rotation of its heartbeats, on its own home channel ([heartbeat](#heartbeat)) |
| Routing | To the neighbour closest to the destination position, around gaps by depth-first search, every hop acknowledged; overload makes messages wait and never drops them ([forwarding](#forwarding)) |
| Addressing | A position and a 6-byte node ID; no registration, no network-wide tables ([addresses](#addresses)) |
| Nodes | One kind, with two measured flags, sleepy and carried ([nodes](#nodes)) |
| Phones | Bluetooth LE GATT link to a node; the app encrypts end to end ([phone link](#phone-link)) |
| Position | GPS unless a fix fails the checks or a map point is set; WGS-84 in 7 bytes ([position](#position)) |
| Time | No network time; every timer is local ([position](#position)) |
| Updates | Firmware images spread relay to relay as coded pieces, at a pace that leaves the channels to texts ([updates over the mesh](#updates-over-the-mesh)) |

## Hardware

**LILYGO T-Echo, 433 MHz build.** nRF52840, SX1262, Quectel L76K GNSS,
1.54" e-ink, BLE 5.0, 850 mAh Li-ion, USB-C charging, UF2 bootloader. The
LoRa module must be the 433 MHz build.

Rooftop relays, same chipset and firmware: **RAK WisBlock** RAK4631 EU433
(PN 111000) + RAK19003/19007 base + RAK12500 GPS in a solar enclosure, or
the **RAK WisMesh Repeater Mini V2** if a 433 MHz build exists (O12).

## Nodes

One protocol and firmware for every node. A node's role follows two flags
it measures of itself, independently, and publishes in its heartbeat
([packet formats](#packet-formats); roadmap D70–D72).

- **Sleepy**: on battery, unless it is a fixed relay whose battery is not
  low ([sleepy nodes](#sleepy-nodes)). On external power: not sleepy. A
  sleepy node beats instead of heartbeating, listens only in a window
  after each beat, receives duty-cycled between, forwards what it is
  given at once, anchors to a fixed relay, keeps no PO box.
- **Carried**: its GPS says it moves ([position](#position)). A map point
  set in the app clears the flag at once. Which fixes become the node's
  position:

  | node | takes |
  |---|---|
  | carried | each fix |
  | sleepy, fixed, no anchor | each far fix ([position](#position)); stands at its place again at a fix that is not far |
  | relay, fixed | none: it keeps its place (roadmap D63) |
  | sleepy with an anchor | none: its position is its anchor's, and its carried flag stays as it is ([beats](#beats)) |

  A carried relay is otherwise as any relay.

Only a node with neither flag holds mail for sleepy nodes, owns a
destination by its position ([owner](#owner)), or is an anchor
([relay choice](#relay-choice)). A node that holds no mail owns only what
is addressed to its ID.

**A forced role.** Holding the button
([screen and buttons](#screen-and-buttons)) flips the role: a relay turns
sleepy, leaving as at its low battery ([sleepy nodes](#sleepy-nodes)); a
sleepy node turns relay at once; each hold flips again. The forced role
overrides the grace, the carried flag and the battery's high level until
external power next changes; then the rules above decide again, at once.
A forced relay on battery turns sleepy at its low battery level, which
ends the force; on battery at that level a flip to relay is not taken. A
reboot forgets the force. A node built only to be sleepy takes none. The
heartbeat's flags say the role as it is (roadmap D160).

Terms. Each names a state, not a kind of node.

| term | meaning |
|---|---|
| **relay** | a node that is not sleepy, carried or not: listens all the time, heartbeats on every channel, forwards |
| **sleepy node** | a node that is sleepy |
| **fixed** | not carried |
| **anchor** | the fixed relay a sleepy node chooses ([relay choice](#relay-choice)); its position is the sleepy node's address, its home channel the sleepy node's |
| **anchorless** | a sleepy node that hears no fixed relay ([sleepy nodes alone](#sleepy-nodes-alone)); a heartbeat flag, which a relay also sets while its table is new ([heartbeat](#heartbeat)) |
| **beat** | a sleepy node's heartbeat ([beats](#beats)) |
| **window** | the seconds after a beat, or after a message a sleepy node takes, in which it listens ([delivery](#delivery)) |
| **holder** | a relay that holds a message for a sleepy node and hears its beat; most often its anchor |
| **PO box** | a relay's heard table and delivery queue |
| **long preamble** | a preamble that reaches a node receiving duty-cycled at any time ([duty-cycled receive and the long preamble](#duty-cycled-receive-and-the-long-preamble)) |
| **dark** | a sleepy node out of reach for minutes |
| **delivery TTL** | 600 s: the network keeps what it holds for a node at least this long |

## Regulatory limits

| | EU (ERC/REC 70-03, Annex 1, band g1; "f1" in editions before 2026) | Ukraine (NCEC decision 81 of 12.02.2025, row РІ 50-3-1, as amended by decision 212 of 06.05.2026) |
|---|---|---|
| Frequency | 433.050–434.790 MHz | 433.04–434.79 MHz |
| Power | 10 mW ERP | 10 mW ERP |
| Duty cycle | ≤10% | <10% |
| Channel spacing | no requirement | no requirement |
| Listen-before-talk | not required | not required |
| Modulation | any | any |

Kept by:

- an airtime token bucket per node that holds every hour within 10%
  ([duty cycle cap](#duty-cycle-cap));
- no transmission longer than 1 s: an SF5 packet is at most 139 ms,
  373 ms with the long preamble ([physical layer](#physical-layer));
- +10 dBm at the connector; with the stock antenna (≈0 dBd) at most
  +10 dBm ERP.

## Physical layer

RSSI: the received power the radio reports. CAD (channel activity
detection): the radio's check for a LoRa preamble; it works below the
noise floor.

| Parameter | Value | Source |
|---|---|---|
| Modulation | LoRa | SX1262 |
| Spreading factor | 5, network-wide, a compile-time constant | SX126x only |
| Bandwidth | 125 kHz | |
| Coding rate | 4/5 | |
| Raw bit rate | 15.6 kbps | |
| Symbol time | 0.256 ms | 2⁵ / 125 kHz |
| Preamble | 12 symbols (3.1 ms) + 6.25 symbols of sync; long, 927 symbols (237.3 ms), only to and from a node that receives duty-cycled | 12: SX1262 minimum at SF5 (datasheet §6.1.4); long: below |
| Airtime | an n-byte packet: its preamble, 14.25 symbols of sync and header, and 5 symbols for each 20 bits, or part of 20, of 8n + 16 | datasheet §6.1.4; `time_on_air` in `radio.rs` |
| 255-byte packet | 138.6 ms | |
| Sensitivity | ≈−120 dBm | |
| Noise floor (125 kHz, noise figure ≈5 dB) | ≈−118 dBm | RSSI cannot see a signal below it; CAD can |
| Sync word | 0x12 (private), written 0x1424 to the SX126x; a test build that must not join nodes in range uses another (the bench build: 0x21) | |
| CRC | on (LoRa hardware CRC) | |
| Maximum packet | 255 bytes | SX126x |
| Range, relay↔handheld, street | ≈750 m | |
| Range, relay↔handheld, indoors | ≈300 m | |
| Range, rooftop↔rooftop | several km; **hearing radius** ≈3 km | [capacity and sizing](#capacity-and-sizing) |

### Duty-cycled receive and the long preamble

Every frame has the shortest preamble except those to a node receiving
duty-cycled: a sleepy node outside its windows
([sleepy nodes](#sleepy-nodes)); without an anchor, always
([sleepy nodes alone](#sleepy-nodes-alone)). The long preamble goes only
on:

- DATA to a sleepy node outside its window;
- the beats of a node without an anchor, and a relay's answers to them;
- a sleepy node's beats to its anchorless neighbours.

The SX1262 receives duty-cycled itself (datasheet §13.1.7,
SetRxDutyCycle): receive for `rx`, about 1 ms to save and restore state,
sleep for `sleep`, start the TCXO, receive again; cycle
`C = rx + sleep + wake`. The TCXO start delay is probed at boot (roadmap
D88); the cycle is derived for `wake` = 1.5 ms: the 1 ms and the least
delay the probe lands on, 0.5 ms (roadmap D92). Every node keeps the same
**gap** between windows, `sleep + wake`: a node whose delay exceeds 0.5 ms
takes the excess off its sleep and adds half of it to its window
(`bark-proto/src/sniff.rs`, `periods`).

| Quantity | Value | Derivation |
|---|---|---|
| lock time `D` | 4 symbols, ≈1 ms | preamble a window must hold for the modem to lock; to be measured (roadmap Phase 2, bring-up step 1) |
| Window `rx` | 3.75 ms (240 steps of 15.625 µs) | `rx ≥ (wake + 2D + header) / 2` plus half a symbol, header = 14.25 symbols, from datasheet §13.1.7: Tpreamble + Theader ≤ 2 × rxPeriod + sleepPeriod |
| Sleep | 233.6 ms (14 952 steps) | cycle of least current, below |
| Gap | 235.1 ms | sleep + wake |
| Cycle `C` | 238.9 ms | |
| Long preamble | 927 symbols, 237.3 ms | ≥ gap + 2D |
| Mean current receiving duty-cycled | 0.129 mA | sleep at 1.2 µA (datasheet Table 3-5) |
| Heartbeat with the long preamble | 248.6 ms | |
| 255-byte frame with the long preamble | 372.8 ms | |

Cycle: `C = √(I_rx (rx + wake) P / I_tx)` with `I_rx` = 5.8 mA, `I_tx` =
32 mA (datasheet Table 3-6, 434 MHz) and beat period `P` = 60 s gives
238.9 ms.

## Channel plan

Thirteen channels of 125 kHz, 57.5 kHz of guard at each band edge:

```
f_k = 433.170 MHz + k × 0.125 MHz,   k = 0..12
      433.170, 433.295, 433.420, 433.545, 433.670, 433.795, 433.920,
      434.045, 434.170, 434.295, 434.420, 434.545, 434.670 MHz
```

No grid or position assigns channels
([home channel selection](#home-channel-selection)). Whether 125 kHz
spacing is enough: open item O5.

## Home channel selection

Every node has one **home channel**: it listens there whenever it is not
transmitting, and all traffic to it is sent there.

### Busy estimate

Per channel a node keeps `busy[c]`, an exponential moving average of a 0/1
busy reading. Foreign transmitters count as peers.

| | |
|---|---|
| reading | 1 if RSSI > the floor of all channels + 6 dB (≈−112 dBm), else 0 |
| a reading of another channel | taken four symbol periods, 1.02 ms, after the receiver starts on it |
| a reading of the home channel | taken four symbol periods after the receiver last started there and kept until asked for, for one reading and at most 13 s; with none, the receiver is started again for it (roadmap D225) |
| sweep | one channel a second, channels in turn: each channel every 13 s |
| window | α = 1/138 per sample: 30 minutes at one sample per channel every 13 s |
| a channel's noise floor | the median of that channel's readings in 1 dB bins, each reading's weight halved every noise half-life; with no readings, the floor of all channels; −118 dBm before the first reading (O11) |
| noise half-life | 900 s, three beat periods ([beats](#beats)), the same for every node |
| the floor of all channels | the 10th percentile of every channel's readings together, weighted the same way |
| the node's noise floor | where no channel is named: its home channel's |
| listen-before-talk | busy if RSSI > the checked channel's floor + 6 dB (roadmap D216) |
| noise floor of a node that does not sweep | its home channel's, from the readings its heartbeats' listen-before-talk takes there (roadmap D212) |
| a channel no longer read | empty within 16 half-lives (two-byte bins); then the floor of all channels |

A reading of another channel takes 1.9 ms off the home channel (roadmap
D88): retune 0.39 ms, four symbol periods 1.02 ms, read 0.05 ms, retune
home 0.39 ms.

A reading is taken only when the modem reports no preamble detected and no
packet in progress. On its home channel a node counts only what would stay
behind if it moved (roadmap D55):

- A home reading taken while DATA addressed to the node, the ACK of one of
  its own hand-overs, a piece of the image it gathers
  ([updates over the mesh](#updates-over-the-mesh)), or its own
  transmission was on the air is set aside. Every other home reading
  counts as on any channel.
- A home reading is held until the next, 13 s later, when it is known
  whether it fell in the node's own packet (roadmap D24).
- The radio keeps receiving while the node is given its home reading
  (`core-interface.md`, Radio, rules 7 and 8), and the reading is judged by
  when it is given: inside a packet addressed to the node, discarded;
  inside any other, counted. A packet for the node that fails its CRC
  counts.

### Channel at boot

A node takes part from switch-on. Its **first rotation** is a worst-case
rotation of heartbeats ([heartbeat](#heartbeat)), 195 s; a sleepy node's
is two, 390 s (roadmap D51, D73).

| | |
|---|---|
| home at boot | the stored one ([implementation notes](#implementation-notes)), else a random channel; a config override fixes it for bench work |
| from boot | it sweeps, takes and forwards traffic, and a relay heartbeats, its first rotation's heartbeats in a burst ([heartbeat](#heartbeat)) |
| end of the first rotation, without a stored home | move to `argmin busy[c]` unless home is within 0.01 of it; channels within 0.01 of the lowest tie, and the choice among them is random |
| a sleepy node | listens: at the first relay it hears it ends its first rotation and takes the relay as its anchor ([relay choice](#relay-choice)); with none by the end of two rotations it is anchorless ([sleepy nodes alone](#sleepy-nodes-alone)) |

In its first rotation a node owns no position but its own
([owner](#owner)), and holds its own application's message while it knows
no relay ([return to parent](#return-to-parent)).

### Channel moves

A relay moves only when `busy[home] − min_c busy[c] > 0.05` continuously
for 10 minutes; then it waits a random 0–5 minutes, checks again, and
moves to the quietest channel, ties broken as at boot.

Immediately after retuning it sends one heartbeat on the old channel,
naming the new one. There is no other announcement: neighbours that miss
it learn the new channel from the next rotation ([heartbeat](#heartbeat))
or from ACK failure ([hand-over](#hand-over)).

Its sleepy nodes follow:

- One in a window ([delivery](#delivery)) hears that heartbeat and follows
  at once; the moving relay takes the sleepy nodes its heard table has on
  the old channel to listen on the new one ([heard table](#heard-table)).
- One asleep and anchored to it follows at the next heartbeat it wakes for
  ([relay choice](#relay-choice)) and beats on the new channel soon after
  ([beats](#beats), roadmap D53).

## Position

A node's position is its GPS fix, unless the app sets a map point, which
wins. Which fixes a node takes as its position: [nodes](#nodes).

**A node's place.** A node has a place when it was built with one, has one
stored as a fixed node ([implementation notes](#implementation-notes)), or
has a map point. A carried node stores none (roadmap D133). A node with no
place:

- takes its first accepted fix as its place, fixed, at once, without the
  carried rule below; its driver checks that fix against no neighbour;
- sends no heartbeat before it has a place
  ([positions on the wire](#positions-on-the-wire));
- once placed, rebuilds the positions it heard near its place, and a relay
  heartbeats its burst ([heartbeat](#heartbeat)) from then, as at boot
  (roadmap D83).

**The map point.** The app sets and clears it ([settings](#settings)).
While set: the node takes no fix, its driver hands none, a relay's GPS
rests (below), a sleepy node starts no search ([beats](#beats)). Cleared:
the node stays where it is, says so at once ([heartbeat](#heartbeat)), and
its next accepted fix is its place (roadmap D84).

**Checks.** A fix is dropped when:

- it is farther than the hearing radius, 3 km
  ([physical layer](#physical-layer)), from every relay the node knows; a
  node that knows none takes it;
- the mean carrier-to-noise density (C/N0) of the satellites used in it is
  under 22 dB-Hz (provisional: O18, roadmap D234).

With no fix the node keeps its position. No bound applies to the distance
from the last fix (roadmap D63, D74).

**Allowance and carried radius.** Allowance: 50 m, the most two fixes of a
still node differ; used by the [owner](#owner) rule. Carried radius:
250 m, five allowances (roadmap D136).

**Fixed and carried.** Every node measures from its fixes whether it moves,
except a sleepy node with an anchor ([beats](#beats)). A node keeps its
place until an accepted fix lies beyond the carried radius of it (roadmap
D219).

| | |
|---|---|
| the place it stands | its first accepted fix, its map point, or the place it took when last made fixed |
| far fix | an accepted fix more than the carried radius from that place |
| carried | after 72 s of far fixes with none near between, counting only time in which the GPS gave fixes (72 s: 2 × 50 m at 1.4 m/s). A sleepy node without an anchor, which fixes once a delivery TTL ([beats](#beats)): two far fixes at least 72 s apart |
| time without fixes | while the GPS runs and gives no fix that passes the checks, the driver says so, and the time until the next fix does not count; a sleepy node's time between searches counts |
| fixed again | after four hours of fixes within the carried radius of one place: it takes the place it stood as a fixed node if its fix then lies within the carried radius of it, else that fix. That place is kept in RAM only: a node booted carried has none (roadmap D70) |
| map point | fixed at once; cleared, the next accepted fix is the place |
| power | a carried node plugged in is a carried relay until it has stayed four hours (roadmap D72); a fixed one stands at its position, and takes its first accepted fix within a search's limit, 60 s ([beats](#beats)), as its place if that fix is far (roadmap D85, D219) |

A node whose position changes heartbeats the new one
([node identity](#node-identity)). At its first far fix a relay that holds
mail flushes its delivery queue ([delivery](#delivery)).

**A relay's GPS.** Runs all the time on external power and for the grace
after it is lost ([sleepy nodes](#sleepy-nodes)). After the grace, and
whenever the position is a map point, it rests in standby (roadmap D81,
D82). A resting relay searches as a sleepy node does ([beats](#beats)),
with no back-off, at boot and when the radio says it may have moved: over
each worst-case rotation it sums the squared **surprises** of the
heartbeats of its fixed relay neighbours, each heartbeat's RSSI less that
neighbour's averaged RSSI before it ([neighbour table](#neighbour-table)),
and searches when the sum passes the point below. A search that finds a
far fix keeps the GPS running until the node is carried or a fix is near
again.

| | |
|---|---|
| fading | σ = 4 dB ([relay choice](#relay-choice), roadmap D38) |
| a surprise's variance | 4σ²/3, 21.3 dB² |
| the sum | the squared surprises of a rotation, 195 s, each over 4σ²/3 |
| the point | two standard deviations above the mean of a chi-square with k degrees, k the heartbeats, by Wilson–Hilferty: k (1 − 2/(9k) + 2 √(2/(9k)))³ |

**Time.** No network time: every timer is local and monotonic. A node
keeps the time of day for its display and the phone: UTC from a fix, or
from the phone while it has none ([settings](#settings)), carried on by its
own clock while the error can be under a minute (2.7 days at ±250 ppm),
and the phone's UTC offset (roadmap D129, D130). Past that it shows no
time.

## Heartbeat

Every node heartbeats. This section is a relay's; a sleepy node's are
rarer ([beats](#beats)), an anchorless node's carry the long preamble
([sleepy nodes alone](#sleepy-nodes-alone)).

A heartbeat ([packet formats](#packet-formats)): the packet's kind, the
node's home channel, the newest format it speaks, its flags, its ID
([node identity](#node-identity)), its short position
([positions on the wire](#positions-on-the-wire)). 13 bytes, 14.4 ms;
with an update's section 26 bytes, 22.08 ms
([updates over the mesh](#updates-over-the-mesh)).

| | |
|---|---|
| interval | fixed per node, from the last byte b of its ID: T = 5 s + 10 s × b / 256, mean 10 s (roadmap D47, D230) |
| first heartbeat | at a random time within T after boot |
| **burst** | the first 13 slots, a burst gap of 1.1 s apart, each with the anchorless flag; every slot after, T after the one before |
| channel | `k ← (k + 1) mod 13` every slot, from a random start, whether the heartbeat went or not |
| **rotation** | 13 heartbeats: ≈130 s, at most 13 × 15 s = 195 s |
| lateness | none early; at most 872.8 ms after its slot: a full frame with the long preamble, 372.8 ms, the ACK timeout, 100 ms, and two backoffs of 200 ms. A check held for a reception in progress waits at most that frame |
| deaf time | 19 ms per heartbeat |
| clocks | each node's within ±100 ppm of a rate all share: two part by up to 200 ppm (roadmap D230) |

**Procedure.** Retune to channel k; listen-before-talk, up to three checks
a random 0–200 ms apart; if every check is busy, skip; else send; retune
home.

| | |
|---|---|
| listen-before-talk | one channel activity detection with the channel's RSSI read four symbol periods into it ([busy estimate](#busy-estimate)): busy if the detection saw a preamble or the reading was above the channel's threshold. 3.1 ms from listening to listening, 2.18 ms of it the detection's 8 symbols and half a symbol of processing. The driver checks at boot that the reading taken during detection agrees with one taken in receive on the home channel, keeps the offset it finds, and reads after the detection if they do not agree (roadmap D88) |
| transmission | its airtime and 1.8 ms, from listening to listening (roadmap D88) |

**A radio that reads every channel busy.** A heartbeat skipped on every
channel with no clear check between, 39 busy checks in a row: the driver
resets the radio, measures its oscillator's start and the offset above
again, and logs it. Not applied to a sleepy node (roadmap D192).

**Slots.** A neighbour that has heard one heartbeat of a node knows its
interval from its ID: heard on channel c at time t, the next is on c + j at
t + j × T, and on c again at t + 13 × T.

**A heartbeat due and not heard.** A relay expects each neighbour's
heartbeat on its own channel once a rotation, in a window: from the slot,
less the lateness the last one heard may have had and the clocks' drift
since, to the slot plus that lateness, that drift and the airtime of a
heartbeat with an update's section; about 1.8 s. When the window closes
without the heartbeat, the relay enters a reading at its noise floor
([busy estimate](#busy-estimate)) into the neighbour's average
([neighbour table](#neighbour-table)), weighted by the share of the window
its radio listened on the channel. The slot passes to the next rotation
either way. A heartbeat of a burst is in no slot and restarts the node's
slots; one heard in a slot sets the slots afresh. A sleepy node counts
nothing (roadmap D103, D105).

**The burst.** Reaches every neighbour within 12 burst gaps, 13.0 s. A
neighbour times a relay's slots only by heartbeats without the anchorless
flag. Burst gap: a backoff of 200 ms, the lateness 872.8 ms and the
airtime 14.4 ms, 1087.2 ms.

**Outside the slots.** A relay past its own first rotation, neither
carried nor flushing to turn sleepy ([sleepy nodes](#sleepy-nodes)),
answers a heartbeat with the anchorless flag with one of its own on the
channel that heartbeat names, at a random time within 0–200 ms of it:

- a relay's, of its burst: once, ordinary preamble;
- a sleepy node's: as [sleepy nodes alone](#sleepy-nodes-alone) says,
  long preamble.

A node in its first rotation does not answer.

**A heartbeat at a change.** A node, sleepy or not, heartbeats once when
its sleepy or carried flag changes or its map point is cleared; a relay
also when a map point is set or its place moves to a fix it takes
([position](#position); roadmap D71, D72). That heartbeat:

- goes on its home channel, ordinary preamble (long if anchorless), with
  listen-before-talk;
- from a sleepy node, on its anchor's channel, is a beat to its holder,
  and its window follows ([delivery](#delivery));
- from a node that turns sleepy, goes on the channel it was at home on,
  unless its first beat goes there anyway ([sleepy nodes](#sleepy-nodes));
- is one heartbeat for any number of changes before it goes.

A position that changes with each fix is not announced: a carried relay's
slots carry it (roadmap D71).

## Neighbours

### Node identity

A 6-byte **node ID**: the low 48 bits of the chip's factory device ID
(nRF52840 FICR: DEVICEID[0] and the low 16 bits of DEVICEID[1]). It names
the node wherever a packet names one: DATA `dst_hop`, `src_hop`, `dst_id`;
ACK `from`, `to` ([packet formats](#packet-formats)).

An ID must be unique only among nodes one relay hears together (roadmap
D59, D75). This section and the heartbeat interval assume the low bits
spread as random bits (to check: `roadmap.md` Phase 2).

The ID is the node's identity. A node whose position changes keeps its ID
([position](#position)); its address changes
([addresses](#addresses)), and that of every sleepy node anchored to it.
Two nodes at one position are two nodes.

### Neighbour table

Per node, up to 64 entries, by node ID:

| Field | Content |
|---|---|
| id | from its heartbeat; the key |
| position | from its heartbeat, rebuilt from the short position ([positions on the wire](#positions-on-the-wire)), and its local form for distances |
| flags | sleepy, anchorless, carried, leaving, map point, from its last heartbeat ([packet formats](#packet-formats)); leaving holds until a heartbeat without it; map point is used by no rule |
| format | the newest format it speaks, from its heartbeat |
| channel | where to transmit to it: from its heartbeat or from `aux` of a DATA it sent ([packet sender](#packet-sender)), whichever came last |
| last_heard | local monotonic time |
| rssi_ema | averaged RSSI, each new reading counting half: every frame heard from it, and each heartbeat due on this channel and not heard, as a reading at the floor weighted by the share of its window the radio listened ([heartbeat](#heartbeat)) |
| fail_count | failed hand-overs to it in a row with no answer between (demotion, below) |
| banned_until | when its ban ends ([bans](#bans)) |
| service, last take, told | service time, below |
| line end | when the line of senders refused for want of it as the way on ends ([bans](#bans)) |
| update | the section of its last heartbeat that carried one, and when heard ([updates over the mesh](#updates-over-the-mesh)) |
| last_beat | when its last heartbeat heard in a slot started, or its last slot on this channel that passed unheard, and on which channel ([heartbeat](#heartbeat)) |

**Expiry.** After a time without a heartbeat or an answer (demotion,
below):

| | |
|---|---|
| a relay | 3 rotations, 390 s |
| a sleepy node with an anchor | two beat periods, 600 s ([beats](#beats)) |
| an anchorless node ([sleepy nodes alone](#sleepy-nodes-alone)) | 26 anchorless beat periods, 1200 s, and 872.8 ms (roadmap D47, D51) |
| a sleeping node's anchor | never; evicted for no newcomer ([relay choice](#relay-choice)) |

**Margin.** A neighbour has margin when `rssi_ema` is at least 4.1 dB
above the noise floor ([busy estimate](#busy-estimate)). Derivation: a
541-symbol full frame needs 0.43 dB more than the datasheet's 157-symbol
reference; sensitivity is 2.5 dB below the noise at SF5 (datasheet
Table 6-1); a fade of 4 dB: −2.07 + 4 = 1.93 dB above the floor; packet
RSSI is signal plus noise (datasheet §13.5.3): 10 log10(1 + 10^0.193) =
4.1 dB (roadmap D38). A neighbour without margin stays in the table and is
a next hop only when no other is left ([next hop](#next-hop)).

**Demotion.** A neighbour with `fail_count ≥ 3` is demoted until it
answers this relay or three rotations (390 s) pass; demoted, it ranks
behind every other neighbour with margin ([next hop](#next-hop)). An
**answer** is an ACK to this relay's hand-over to the neighbour, taking,
refusing or seen ([the ACK](#the-ack)), or a DATA the neighbour addresses
to this relay; it resets `fail_count`. Heartbeats and frames to other
nodes do not (roadmap D102, D106).

**Service time.** Per entry: a running average, each take counting one
part in 39, of the time from when a message began to wait for the
neighbour here, or from the neighbour's last take if later, to the ACK
that takes it. A message begins to wait when it is queued, or when routing
sends it to a different neighbour than before ([next hop](#next-hop)).
Included: unanswered attempts and their bans, this relay's bucket, its
turn behind other hand-overs. Excluded: time with nothing waiting, and the
waits the neighbour's refusals told. Before the first take: one ban.
Never more than 89 min ([bans](#bans)). Kept in 32 bits, in 1/1024 of a
ban.

**Eviction.** Table full and a relay not in it heard: the newcomer
replaces, in this order,

1. the least recently heard demoted entry;
2. the least recently heard entry, if it has missed a worst-case rotation;
3. if the newcomer is a fixed relay that holds mail, within 50 m, the
   owner radius ([owner](#owner)): the least recently heard entry beyond
   50 m (roadmap D68, D70).

Failing all three it is not added (roadmap D12, D69). A sleepy node with
an anchor replaces no entry.

A beat fills the neighbour table as any heartbeat, the entry marked
sleepy. At a node that is not sleepy it fills the heard table too if the
sleepy bit is alone ([heard table](#heard-table)).

### Neighbour status

A node asks a neighbour for its status when its app asks
([the neighbour map](#the-neighbour-map)). The **ask** names the node
asked and the node asking. The **answer** ([packet formats](#packet-formats)):

| field | bytes | content |
|---|---|---|
| flags | 1 | bit 0 placed ([position](#position)); bit 1 sleepy, bit 3 carried ([nodes](#nodes)); bit 2 anchored: sleepy with an anchor ([relay choice](#relay-choice)); bit 4 on external power; bits 5–7 zero, ignored |
| battery | 1 | 0 at or below the low level, about 20 % ([sleepy nodes](#sleepy-nodes)); 1–100 about that share of charge left, from its last reading; 255 not read, and on USB |
| heard | 1 | its live neighbours, at most 64 ([neighbour table](#neighbour-table)) |
| strong | 1 | of them, those with margin |
| proven | 1 | of them, those that have taken a message from it |
| gps | 1 | the mean C/N0, dB-Hz, of the satellites its GPS used in its latest reading, the fix kept or not ([position](#position)); 0 while its GPS rests and when it used none (roadmap D237) |
| version | 4 | the firmware image version it runs, 0 if not known ([updates over the mesh](#updates-over-the-mesh)) |
| gathers | 4 | the version of the image it gathers over the mesh, 0 if none |
| blocks | 1 | that image's blocks, the table's among them; 0 if none |
| complete | 1 | the blocks of it it holds from the first |
| have | 1 | the pieces it holds of the block after them |

Each is one frame, one hop, neither answered nor sent again. A node:

- asks a live neighbour only when its app asks;
- answers an ask addressed to it from a live neighbour, once; one from a
  node it does not hear it leaves unanswered;
- sends each on the other node's home channel, long preamble to a sleepy
  one, after a listen-before-talk as a heartbeat's, given up after its
  three checks, paid from the bucket with the reply reserve kept
  ([duty cycle cap](#duty-cycle-cap)); after the hand-overs, an answer
  before an ask;
- hands every answer addressed to it to its app.

Ask: 14 bytes, 15.68 ms. Answer: 31 bytes, 24.64 ms. With the long
preamble: 249.92 and 258.88 ms.

## Forwarding

### Addresses

A destination address is a position and a node ID: a relay's own position,
or a sleepy node's anchor's, and the recipient's ID
([node identity](#node-identity), [nodes](#nodes)). A sender learns an
address from an earlier message, a person's card
([the payload](#the-payload)), or a directory; the network has no lookup.
Sending to an area: open item O7.

The network carries no sender address and sends no failure notices; its
only acknowledgement is the per-hop ACK ([hand-over](#hand-over)).

The app hands a node a message only when the node says it will take one
([receiving DATA](#receiving-data)). Afterwards it hears one thing: a
refused message, given back with the wait the refusal asks: a sleepy
node's, refused by its anchor
([a sleepy node's own message](#a-sleepy-nodes-own-message)), or a
relay's, refused by its own gate (roadmap D29, D54).

### Duty cycle cap

**Token bucket.** A transmission may start only if the node's bucket holds
its airtime, which it takes out.

| | |
|---|---|
| depth | 6 s |
| fill | (360 s − 6 s) per hour, 9.83% of the time |
| empty bucket | a full frame waits 1.4 s |
| longest wait | 6 s / 9.83% = 61 s (roadmap D26) |

Any interval L carries at most 6 s + 9.83% × L on air: any hour at most
360 s, the 10% of ERC/REC 70-03 band g1 (proof in `tables/budget.rs`).

**Out of budget.** A hand-over the bucket cannot fit waits; it is not
dropped. Two reserves stay in the bucket:

- Replies that take on no work, refusals and seen answers among them,
  leave one heartbeat's airtime, 14.4 ms.
- Forwarding, and the ACK that takes on work, leave the **reply reserve**,
  845 ms: a heartbeat and 49 refusals. Derivation: over the longest ban,
  61.0 s, the bucket refills 6 s; 13 heartbeats take 187 ms; one DATA per
  155.5 ms gives 391 refusals, 6631 ms; the excess 819 ms is
  ⌈819 / 17.0⌉ = 49 refusals; plus 14.4 ms.

A relay below the reserve sends only heartbeats and replies. Without
budget for a reply it stays silent (roadmap D27, D29, D42, D106).

### Hand-over

A **hand-over** sends one DATA packet to one neighbour until that neighbour
answers or the relay gives up. One hand-over at a time; others wait in the
[send queue](#send-queue). The answer is an ACK that takes, refuses or has
seen the packet ([the ACK](#the-ack)).

1. Retune to the next hop's home channel, read before each attempt from
   its latest heartbeat or DATA `aux` ([neighbour table](#neighbour-table)).
2. Listen-before-talk: one check ([heartbeat](#heartbeat)); if busy, a
   random backoff of 0–50 ms and check again. Once one attempt's checks
   have found the channel busy for 500 ms, the hand-over gives the radio
   up for a retry wait ([retry wait](#retry-wait)) and listens again for
   the same neighbour, keeping the count of transmissions made, with a
   fresh 500 ms; nothing is counted against the neighbour. For a delivery
   to a sleepy node ([delivery](#delivery)), a give-up counts as an
   attempt.
3. Transmit, the sender's home channel in `aux`, in the format below,
   shortest preamble, or long to a sleepy neighbour
   ([duty-cycled receive and the long preamble](#duty-cycled-receive-and-the-long-preamble)).
4. Retune home at once.
5. Wait for the ACK on the home channel, timeout 100 ms. An ACK that takes
   the packet ends the hand-over: the neighbour has the message and is
   tried ([message records](#message-records)). An ACK that has seen it
   ends it without a take: the neighbour is tried and the message chooses
   its next hop at once.
6. On timeout: wait ([retry wait](#retry-wait)) and retry, up to 3
   attempts; an ACK that comes before the retry still answers. Then the
   hand-over has failed: count a failure against the neighbour and ban it
   ([bans](#bans)). The message keeps its place, untried at that
   neighbour, and is routed again with three fresh attempts
   ([next hop](#next-hop)). While the neighbour ranks first, the message
   waits for its ban and offers itself again; it goes elsewhere once the
   neighbour is demoted ([neighbour table](#neighbour-table)) or has left
   the table, and a forward whose failure leaves the neighbour demoted
   counts it tried while queued here ([next hop](#next-hop)). A return is
   dropped once its parent is demoted or gone
   ([return to parent](#return-to-parent)); the delivery queue keeps a
   delivery whatever its hand-over does
   ([delivery queue](#delivery-queue)). (Roadmap D89, D102, D106.)

**An attempt is a transmission.** A busy channel never moves a message to
another neighbour, nor ends a return. What moves a message: a seen answer
(step 5), a refusal ([bans](#bans)), its neighbour demoted or gone, or a
better one come (step 6). A busy channel holds the message until it
clears; the duty cycle cap does not bound that wait (roadmap D89, D96,
D111).

**Format.** A DATA goes in the newest format both speak: to a neighbour in
the table, the lower of this node's newest and the neighbour's, from its
last heartbeat; to one known only from a DATA it sent
([packet sender](#packet-sender)), that DATA's. The ACK answers in the
format of the DATA ([packet formats](#packet-formats)).

#### Retry wait

The wait before the retry after the k-th unanswered transmission is
uniform in [0, 2^k × 155.5 ms]: 0–311 ms, then 0–622 ms. The unit is a
full frame (138.6 ms) and its ACK (17.0 ms); for a frame with the long
preamble, 389.8 ms. A hand-over whose listen-before-talk gave up (step 2)
waits the window of the transmissions it has made, one unit before any,
and listens again (roadmap D22).

#### The ACK

The receiver retunes to the sender's channel, sends the ACK at once
without listening first, and retunes home. The ACK: first byte, format,
message ID, the IDs of the answering and the answered node; 18 bytes,
17.0 ms ([packet formats](#packet-formats)). Its first byte: wait 0 takes
the packet; a refusal carries the wait it asks ([bans](#bans)); the seen
flag with wait 0 says the receiver has seen the message and takes nothing.
ACKs arrive on the sender's home channel; a sleepy node's is its anchor's.
A node is deaf 20 ms per ACK it sends.

The sender matches the message ID and both node IDs against its
outstanding hand-over and ignores an ACK that does not match (roadmap
D32). A matching ACK, taking, refusing or seen, resets the neighbour's
failure count and refreshes its last-heard time
([neighbour table](#neighbour-table)).

**Seen** answers a copy of a message the receiver has a record of, holds
no copy of, and did not hand to the sender
([receiving DATA](#receiving-data) case 6). The receiver changes nothing
but its record. The sender acts as if the receiver had returned the
message at once: the receiver is tried, the count it recorded is spent
([hop count](#hop-count)), and the message chooses its next hop at once.
Nothing is banned, no failure is counted. A return answered seen is
dropped ([return to parent](#return-to-parent)). A seen ACK is paid from
the heartbeat reserve; a receiver that cannot afford it stays silent. A
lost seen ACK costs a retry, answered seen again
([receiving DATA](#receiving-data) case 3) (roadmap D89, D90).

**A take is final when the ACK has gone out.** The receiver decides on
reception whether it takes or refuses, but acts on a packet it takes, into
its send queue, delivery queue and record, only once the ACK has left the
antenna. An ACK that cannot go out, for want of budget or a radio fault,
leaves the packet untaken. A packet that gives no work
([receiving DATA](#receiving-data) cases 1, 3, 4 and 6) is noted on
reception (roadmap D93).

A node ACKs a packet as soon as it has received it, even while waiting for
an ACK of its own. It owes at most one reply at a time: a DATA that
arrives while it owes one is neither taken nor refused; a copy of the
packet the owed reply answers is answered by that reply (roadmap D29).

#### Bans

A neighbour that refuses a packet or fails a hand-over is **banned**: no
forward or return goes to it until the ban ends. A message whose next hop
it is waits for it, unless the neighbour refused that message
([next hop](#next-hop)).

**A refusal bans for the wait it asks.** An ACK that refuses, answering
the outstanding hand-over: no failure is counted, the neighbour is not
tried, and its ban becomes the wait asked, in place of any ban it had. The
message keeps its place in the send queue and is routed again with three
fresh attempts: round the refuser while the ban lasts, to the next
candidate at once, waiting for the ban only where no other is left
(roadmap D110). A node's own application's message does the same, waiting
longer where no other is left, as below (roadmap D115). A refused return
goes on as a forward. A refused delivery waits the told wait and keeps the
transmissions it has made.

**The told wait.** Unit: a **ban**, 1.58 s = 155.5 ms / 9.83 %, the time
the bucket takes to refill a full frame and its ACK
([duty cycle cap](#duty-cycle-cap); roadmap D29). No ban is shorter.

A refusal tells its wait as one of 63 codes (0 means taken;
[packet formats](#packet-formats)): codes 1 to 8 are 1 to 8 bans; each
code above is an eighth longer than the one below, rounded down to whole
bans and at least a ban longer. Codes 1 to 16 are one ban apart; code 63
is 3384 bans, 89 min. A longer wait is told as 89 min (roadmap D54, D109).

The senders a relay refuses stand in a **line**: each is told its own
place, rounded up to a code, places at least a ban apart. The wait told:

- *For want of room*, the bucket: the time until the bucket holds
  everything queued, the frame and its ACK, with the reply reserve left;
  past the bucket's depth, its whole refill. Each sender so refused is
  told one ban later than the one before.
- *For want of the way on*, at the gate ([send queue](#send-queue)), or
  for a message entering the network while its next hop is banned
  ([receiving DATA](#receiving-data)): a place in that next hop's own
  line, one service time after the last place given and at least a ban,
  or after the messages queued here for it if they take longer to go. The
  line's end is kept in the neighbour's entry; an end in the past counts
  as now. The sender is told its place less a hop's budget. The service
  time is the neighbour's ([neighbour table](#neighbour-table)), or, if
  longer, how long the longest-waiting message queued for it has waited
  since the neighbour last took one, its refusals' waits aside.
- *For want of anything else*, a place for the message's record or in the
  delivery queue, or at a node about to reboot
  ([send queue](#send-queue)): one ban, in the room's line.
- In every case, never less than the rest of the ban on the packet's next
  hop, rounded down and without a place in line.

**Silence doubles the ban from 1.58 s to 61 s.** The ban after the k-th
failed hand-over in a row is 1.58 s doubled k − 1 times, up to 61 s; a
longer ban the neighbour already has stands. An answer from the neighbour
resets its failure count ([neighbour table](#neighbour-table); roadmap
D42, D106). A failure does not count the neighbour as tried (roadmap D34,
D36). The message waits for the neighbour until three silences in a row
demote it ([hand-over](#hand-over) step 6; [bounds](#bounds)).

**A node's own message doubles its wait too.** Where no other candidate is
left, a node's own application's message waits after its k-th refusal in a
row the k-th ban after silence, or the told wait if longer, and starts
again from one ban once it is taken or dropped. Every refusal in a row
counts, gone round or not. A sleepy node's own message:
[a sleepy node's own message](#a-sleepy-nodes-own-message).

### Routing

A message travels as a DATA packet ([packet formats](#packet-formats))
from relay to relay, each hand-over acknowledged
([hand-over](#hand-over)). A relay hands it to its neighbour closest to
the destination position. Around a gap the relays search depth-first: a
relay with no neighbour left to try returns the message to the relay it
first got it from, which tries its next neighbour.

Each relay keeps a record per message ([message records](#message-records));
the packet carries its sender and a hop count. The search never searches
for an ID, but hands a message to the node it is addressed to where that
is a neighbour. A sleepy node that is a next hop ([next hop](#next-hop))
forwards as a relay does.

The **origin** of a message is the node that first sends it into the
network: the node a sleepy node handed its own message to, or a node
sending for its own application. The search starts there and, finding
nothing, ends there (roadmap D11, D16, D17).

#### Packet sender

A node acts on a DATA packet whose `dst_hop` is its own ID
([node identity](#node-identity)); of any other it notes only the sender's
channel and signal ([neighbour table](#neighbour-table)) and that a sleepy
sender is in reach ([heard table](#heard-table)). To send something back:
`dst_hop` = the packet's `src_hop`, on the channel in its `aux`
([packet formats](#packet-formats)); no neighbour-table entry is needed;
if there is one, `aux` becomes its channel.

A packet with `hops = 0` is a sleepy node's own message, and its receiver
is the origin ([hop count](#hop-count)).

#### Message records

A relay keeps a **record** for every message it has received or
originated:

| Field | Content |
|---|---|
| ID | the message ID ([packet formats](#packet-formats)) |
| parent | `src_hop` and `aux` of the packet that created the record; none at the origin. The channel is used only if that node is no longer in the neighbour table ([return to parent](#return-to-parent)) |
| handed | one bit per neighbour-table slot: the neighbours that took a hand-over of it, forward or return, by ACKing it ([hand-over](#hand-over)), those that answered seen, and those whose copy this relay answered seen ([the ACK](#the-ack)) |
| last packet | `src_hop` and `hops` of the last DATA of the message received, and whether it was answered seen: the same pair again is a retransmission, answered as the first was ([receiving DATA](#receiving-data)) |
| highest counts | the largest `hops` and the largest `wrong` ([hop count](#hop-count)) received for the message, or sent in a hand-over of it that was taken or answered seen |
| arrived | this node owns the destination ([owner](#owner)), or the node it is addressed to took it from this one |
| last active | when it last received or sent the message |

The **tried set** for a message: the parent, the neighbours in `handed`,
and a sleepy node that handed over its own message. Every other neighbour
stays a candidate, whether passed over, demoted, expired, absent from the
table at the time, refused or never answered. One exception is kept with
the queued message, not the record: a neighbour that a failed hand-over of
the message left demoted is tried for it while it is queued here
([next hop](#next-hop)). When a neighbour-table slot changes hands, its
bit is cleared in every record and queued message.

The record is also how a relay recognises a repeat
([receiving DATA](#receiving-data)).

**Eviction.** A record has no lifetime. A new record replaces the least
recently active one whose message the relay does not hold. A relay holds a
message while it is in its send queue ([send queue](#send-queue)), the
hand-over included, or set aside for a full delivery queue. The table
holds 649 records ([bounds](#bounds); roadmap D31).

#### Owner

A node **owns** a message's destination when:

- `dst_id` is its own ID; or
- its own position is `dst_pos`; or
- it is within 50 m of `dst_pos`, the allowance ([position](#position)),
  and no live, non-demoted neighbour is closer to it than itself.

Exceptions:

- A node is not the owner by position while the addressed node is a live
  neighbour it has not tried, other than a sleepy one with an anchor: it
  hands the message to that node as the next hop.
- A sleepy or carried node owns only what is addressed to its ID
  ([nodes](#nodes)); a sleepy or carried neighbour closer to `dst_pos`
  does not keep a node from owning it.
- A node in its first rotation owns no position but its own
  ([channel at boot](#channel-at-boot)).

A message whose destination is the sending node's own address is delivered
there without entering the search. The destination position decides who
holds a message, not the heard table (roadmap D21).

The addressed node delivers the message to its application. Any other
owner queues it for the addressed node ([delivery queue](#delivery-queue))
and sends it when the node listens ([delivery](#delivery)), whether or not
it hears the node now; a later copy of a delivered message is only ACKed
([receiving DATA](#receiving-data), case 1).

#### Next hop

One function chooses where a queued message goes, from the state when it
is asked: the neighbour table, the bans, the message's record, and two
marks its send-queue entry keeps, the neighbours that refused it and those
it held for until they were demoted (below). It is asked for each DATA
that arrives ([receiving DATA](#receiving-data)), for queued messages in
turn whenever a hand-over may start, until one may go, and again for the
one chosen as its hand-over starts ([send queue](#send-queue)). The
**candidates** are the live neighbour-table entries
([neighbour table](#neighbour-table)) not in the tried set
([message records](#message-records)).

**Order:**

1. the addressed node itself, if a candidate;
2. neighbours with margin before those without (roadmap D38);
3. within each, not demoted before demoted (roadmap D25, D36, D42);
4. within each, distance to the destination position in the local plane
   ([positions on the wire](#positions-on-the-wire)), nearest first;
5. equal distances by ID.

The relay sends to the first candidate once its ban has ended; with no
candidate it returns the message to its parent. A banned first candidate
is waited for, and the message is routed again when the ban ends
([overload waits](#overload-waits); roadmap D106). The next hop need not
be closer than the relay.

**Refusers.** The queued message keeps a bit per neighbour-table slot for
the neighbours that refused it ([bans](#bans)), cleared when the ban ends.
While a refuser is banned it is no candidate for that message, unless that
leaves none, or only a last resort the refuser was not; then the message
waits for the ban. A failed hand-over sets no bit
([hand-over](#hand-over) step 6).

**Leaving.** A neighbour whose last heartbeat had the leaving flag
([packet formats](#packet-formats)) is passed over for every message but
one that may end there (addressed to it or, if it holds mail, to a
position within the owner radius of it; [owner](#owner)): it is no
candidate while another is left that is no worse a last resort, and the
next hop where none is. No ban or demotion is counted. A message that may
end there and is refused by it waits for it in the delivery queue
([delivery queue](#delivery-queue)).

**Last resorts.** Tried only when the search would otherwise end here
([return to parent](#return-to-parent)):

- a sleepy neighbour with an anchor (its last heartbeat had the sleepy bit
  alone; [packet formats](#packet-formats)), sent to with the long
  preamble (roadmap D58);
- a carried relay ([nodes](#nodes)) from which nothing has been heard for
  its own heartbeat interval ([heartbeat](#heartbeat); roadmap D103,
  D105). Its ACK makes it fresh ([the ACK](#the-ack)).

For a message addressed to it, a stale carried relay is first, not a last
resort (roadmap D116a). A sleepy neighbour with an anchor is first for a
message addressed to it, sent at once with the long preamble, while the
relay **spares the long frame**:

- its bucket holds a full DATA frame with the long preamble, 372.8 ms,
  beyond its send queue, the messages of its delivery queue at the
  ordinary preamble, and the reply reserve
  ([duty cycle cap](#duty-cycle-cap));
- no such hand-over to the node has gone unanswered since it was last
  heard.

Otherwise it is a last resort, and the message goes on toward its address
([delivery](#delivery)). A hand-over to it unanswered in all its attempts
counts as any failed hand-over, and its message is routed again at once.
The owner of the position delivers to such a node from its delivery queue
under the same bucket condition ([delivery](#delivery); roadmap D116a,
D134). An anchorless neighbour is a candidate like any other, sent to with
the long preamble. The first hop of a sleepy node's own message is its
anchor while the anchor is live
([a sleepy node's own message](#a-sleepy-nodes-own-message)).

**The one-way link.** A message whose hand-over to a neighbour fails and
leaves it demoted, by this failure or an earlier one, counts that
neighbour as tried while it is queued here: it goes to the next candidate,
back to its parent, or, at the origin, ends. The neighbour stays a
candidate, last, for every message that has not tried it. The mark is kept
with the queued message, not the record; a copy the neighbour offers later
is no return ([receiving DATA](#receiving-data)).

A candidate that already has the message: if the message has arrived
there, or it holds a copy, it takes it and it goes no further; if it has
had the message and holds none, it answers seen, and the relay goes on to
its next candidate ([receiving DATA](#receiving-data),
[the ACK](#the-ack)).

#### Send queue

A regular room of 39 places and one hop place for each count from 1 to
254; the packet being handed over keeps its place. 39 = ⌈6 s / 155.5 ms⌉
([duty cycle cap](#duty-cycle-cap)).

A packet to send on enters the regular room only if the bucket holds the
reply reserve on top of the new frame, its ACK and everything queued
([receiving DATA](#receiving-data)).

A packet received with count h that the regular room refuses takes the
highest free hop place numbered min(h, 254) or lower, and keeps it until
it leaves the queue. A hop place holds its packet whatever the bucket
holds; the bucket decides when it is sent. At most 64 hop places are held
at once: a place is free when its number is unheld and fewer than 64 are
held; with 64 held the relay refuses the packet ([bans](#bans); roadmap
D155).

A message entering the network, a sleepy node's own with count 0 or a
node's own app's, takes no hop place ([addresses](#addresses)).

**The gate.** Before either, a relay takes a message to send on only if
its wait here would be within a hop's **budget**, 30 s, or its count is
higher than every one of theirs ([receiving DATA](#receiving-data)). Its
wait: its next hop's service time, as that next hop's line takes it
([bans](#bans)), for each message queued here, deliveries aside, that goes
to the same next hop. A queued message's count is the one it holds here,
one less than the count it is sent with. Each queued message is counted
where it was last routed ([next hop](#next-hop)): when queued, and again
whenever it is routed to be chosen or handed over; one being handed over
counts where it goes; one that ended here counts nowhere. A route kept to
a neighbour whose table slot a newcomer takes is forgotten. The gate
routes nothing itself (roadmap D231). It refuses the rest, each with a
place in that neighbour's line ([bans](#bans); roadmap D106).

**Before a reboot.** A node about to reboot into a new image
([updates over the mesh](#updates-over-the-mesh), the install) refuses
every DATA that would give it work, to send on, to hold for a sleepy node
or for its own application, as for want of room ([bans](#bans)), and takes
none from its application. A take it answered before stands.

**Choosing what to send.** Whenever no hand-over is under way and the
radio is free, the relay considers the queued messages in this order:

1. a delivery ([delivery](#delivery));
2. a packet that has waited since it was queued, for a ban or a wait of
   its own: the most travelled first, of equals the one that arrived first
   ([overload waits](#overload-waits));
3. the rest in the order they arrived.

It routes each message as it comes to it ([next hop](#next-hop)) and hands
over the first that may go: one whose next hop, or parent, is banned may
not go until the ban ends, and has then waited. A message whose next hop
as last routed is banned, and not gone round ([bans](#bans)), is routed
then too, though the relay may not come to it, and has waited if its way
is still banned. Each is routed so once a ban (roadmap D231).

#### Receiving DATA

A relay answers a DATA packet addressed to it
([packet sender](#packet-sender)) with an ACK that takes or refuses it,
only when it owes no other reply, and acts on a packet it takes once the
ACK has gone out ([the ACK](#the-ack)). Whether it takes the packet
depends on what the packet gives it to do:

- **Something to send on.** It routes the packet as the record it would
  have routes it ([next hop](#next-hop)) and takes it only if it passes
  the gate ([send queue](#send-queue)): into the regular room if the room
  has a place and the bucket holds, as it stands, the ACK, the airtime of
  everything queued and of the new frame, and the reply reserve
  ([duty cycle cap](#duty-cycle-cap)); else into a hop place, whatever the
  bucket holds ([overload waits](#overload-waits); roadmap D34).
- **Fresh work**, a message entering the network here: a sleepy node's own
  (count 0, [hop count](#hop-count)) or the node's own application's. As
  above but without a hop place; it also needs a live neighbour that is
  not banned, demoted or not, and is refused while the neighbour it would
  go to, were none banned, is banned (roadmap D34, D54). A relay with no
  live neighbour at all takes it, and the search ends as in
  [return to parent](#return-to-parent).
  - From its own application a node takes one message at a time, until its
    first hop takes it; its phones take turns ([sending](#sending)). It
    takes one while it holds none, is not draining
    ([send queue](#send-queue), Before a reboot), has room in its send
    queue and bucket for a full frame on top of what they hold, and not
    every neighbour is banned; for a sleepy node with a live anchor, not
    the anchor
    ([a sleepy node's own message](#a-sleepy-nodes-own-message)). The
    message then passes the gate at count 0 and its next hop must not be
    banned; failing either it goes back to the application with the wait a
    refusal would tell, at least a ban.
  - One from its own application for a sleepy node it holds for, when the
    delivery queue is full, is kept aside until the queue has a place.
- **Something to deliver.** Taken only if the bucket holds the reply
  reserve and an ACK and there is room: in the delivery queue for another
  node, in the inbox for its own application
  ([delivery queue](#delivery-queue), [receiving](#receiving)).
- **No work**, cases 1, 3, 4 and 6 below: answered, taken or seen, even
  when it would refuse work.

Otherwise it refuses: an ACK that refuses, on the channel in `aux`,
nothing changed ([bans](#bans)). A relay that cannot afford the reply
stays silent (roadmap D27, D29).

The packet is one of the following, tested in this order:

1. **Arrived:** the relay has delivered the message or holds it for a
   sleepy node, or its record says the message has arrived
   ([message records](#message-records)). ACK only.
2. **New:** no record. It creates one, the sender as parent (none if
   `hops = 0`). If it owns the destination ([owner](#owner)) it delivers;
   otherwise it chooses a next hop and sends.
3. **A retransmission:** the sender and the count are those of the last
   packet of the message received. ACK only, the answer the first got:
   taken, or seen (case 6).
4. **A copy while the relay holds one:** the message waits in its send
   queue or is set aside (above). ACK only; the relay's copy goes on. A
   hand-over whose frame has gone out does not count as holding one.
5. **A return, and the relay holds none:** the packet comes from a
   neighbour in the record's `handed` set, or from the neighbour the relay
   is still handing the message to, which also ends that hand-over. The
   relay takes it and goes on from its record: to its next candidate, back
   to its parent, or, at the origin, drops it. A copy from anywhere for a
   destination the relay owns now is taken the same way and delivered.
6. **A copy from elsewhere, and the relay holds none:** seen. The relay
   answers with the seen flag ([the ACK](#the-ack)), takes nothing and
   changes nothing but its record.

In every case the relay records the packet's sender and count as the last
packet, with whether it was answered seen, raises its highest counts where
the packet's are higher, and updates its last-active time (roadmap D35,
D90).

#### Return to parent

When [next hop](#next-hop) finds no candidate, the relay sends the message
back to its parent: DATA with `dst_hop` the parent's ID from the record,
on the parent's home channel as the neighbour table has it at the moment
of sending ([channel moves](#channel-moves)); only a parent no longer in
the table is sent to on the channel in the record. The return is routed
like any message, the parent its only candidate: it waits while the parent
is banned, and a neighbour that arrives meanwhile, untried, takes it
forward instead. The parent takes it as a return
([receiving DATA](#receiving-data), case 5) and continues with its next
neighbour. A parent that answers seen: the relay drops the message
([the ACK](#the-ack)).

The origin has no parent: with no candidate left it drops the message.
Nobody is told ([addresses](#addresses), roadmap D10).

An origin in its first rotation that knows no relay holds its own
application's message, and routes it at the first relay it hears, or when
the rotation ends ([channel at boot](#channel-at-boot)).

A message whose next hop is banned is neither returned nor dropped: it
waits ([overload waits](#overload-waits)); a node's own application's
message as in [bans](#bans). A return whose parent is demoted, or has left
the table, is dropped ([hand-over](#hand-over) step 6). A parent that
refuses the return has not failed, and a busy channel is no attempt
([hand-over](#hand-over) step 2, roadmap D111).

#### Hop count

The DATA header carries `hops`, the hand-overs so far, and `wrong`, the
search's wrong hops.

`hops`:

- A sleepy node with an anchor sends its own message with 0
  ([a sleepy node's own message](#a-sleepy-nodes-own-message)): the taker
  is the origin, the message has no parent, and the sender counts as
  tried.
- A relay sending for its own application sends 1.
- A relay sends its record's highest `hops` plus one
  ([message records](#message-records)), forwarding or returning.

The count is set when a hand-over's first frame goes out; its retries
repeat it ([hand-over](#hand-over)). A frame that waited in the send
queue, or goes to another neighbour after a failed hand-over, carries the
count as it stands when it goes. A receiver tells a retransmission by the
sender and the count ([receiving DATA](#receiving-data), case 3). The
count also numbers the hop places of the send queue.

`hops` takes one byte below 128, two below 16 384, three up to 2 097 151
([packet formats](#packet-formats)). A relay that would have to send a
count above 2 097 151 drops the message, except an owner delivering it to
its node ([delivery](#delivery)), which sends 2 097 151.

`wrong`: the sender of a hand-over adds one to its record's highest
`wrong` when the receiver's position, as the sender's neighbour table has
it, is not strictly nearer to `dst_pos` than the sender's own, or the
sender does not know it (a return to a parent no longer in its table).
Both distances are measured in the plane centred on `dst_pos`
([positions on the wire](#positions-on-the-wire)). A return is judged by
the same rule. Three hand-overs add nothing:

- one to the node the message is addressed to;
- an owner's delivery to its node ([delivery](#delivery));
- a sleepy node's own message to its anchor, sent with `wrong` 0.

`wrong` is set and repeated with `hops`, and only grows. A relay that
would have to send a `wrong` above its cap drops the message. The cap is
64, a node's setting; the byte holds 255 (roadmap D220).

#### Overload waits

Overload makes a message wait and never ends it. A message whose next hop
is banned, by a refusal or by silence, waits in its place in the send
queue, its airtime still counted, until the ban ends, and is routed again
then ([next hop](#next-hop)). A message refused by that next hop itself
goes round it where another candidate is left ([bans](#bans)). A message
that has waited since it was queued, for a ban of its next hop or a wait
of its own (the node's own message, refused or with no neighbour to take
it), goes ahead of every message that has not, the most travelled first,
for as long as it is queued: after its offer too, and whatever next hop it
is routed to later. Messages that have not waited keep their order
(roadmap D114).

A message ends only:

- when it is delivered;
- when its search runs out of neighbours
  ([return to parent](#return-to-parent));
- when a return's parent is demoted or gone ([hand-over](#hand-over)
  step 6), or answers it seen ([the ACK](#the-ack));
- when a newer message needs its place in the delivery queue
  ([delivery queue](#delivery-queue)), or when the relay holding it comes
  to own its destination while it is queued and finds that queue full
  ([owner](#owner));
- at the wrong-hop cap or the largest hop count ([hop count](#hop-count));
- when a node holding it dies (roadmap D31, D44, D106).

Never by age on its way; at its owner the delivery queue keeps it by its
own rule ([delivery queue](#delivery-queue)). With 64 hop places held a
relay refuses, and that stall is bounded by the gate's budget (roadmap
D29, D155).

### Why the search finds a path

If, while a message is routed, the links do not change, every hand-over to
a neighbour that answers succeeds within its attempts, and no record is
replaced, the message makes a depth-first traversal of the relays
reachable from the origin through neighbours that answer, nearest to the
destination first: it finds an owner if a path to one exists and the
traversal takes no more wrong hops than the cap of
[hop count](#hop-count), and otherwise returns to the origin.

Outside those conditions:

- **A lost record.** A message returning to a relay whose record has been
  replaced is new there; the search ends early, or wanders until the
  wrong-hop cap ends it.
- **A split.** A hand-over whose ACKs were all lost looks failed, so two
  copies search at once, each answered seen where the other has been. A
  copy ends where it meets the other held at a relay (case 4), where the
  message has arrived (case 1), at the origin, at a cap, or as a return to
  a parent that gave the child up and answers seen (case 6). Only the last
  can end the last copy.
- **Links that change** during the search, a parent's change of home
  channel among them: parts of the graph go unsearched.

### Bounds

| | |
|---|---|
| hand-overs per message | at most 2 097 152 that succeed ([hop count](#hop-count)); with no node moving, at most (c + 1)(R − 1) + c for a wrong-hop cap c and R relays reachable |
| a hand-over | at most 3.15 s: three attempts of 738.6 ms (500 ms of listen-before-talk, a 138.6 ms frame, the 100 ms ACK timeout) and the retry waits, 311 + 622 ms ([hand-over](#hand-over)); a channel that stays busy adds wait but no airtime |
| a neighbour that does not answer | at most 11 failed hand-overs, 33 frames, at a relay: until it leaves the table 390 s after its last heartbeat or answer, with bans of 1.58, 3.16, 6.3, 12.7, 25.3 and 50.6 s, 100 s, then 61 s |
| a message held on one neighbour that does not answer | at most 14.2 s: two bans, 1.58 and 3.16 s, and three hand-overs; then the neighbour is demoted. One that answers this relay between failures holds it while its bans last ([bans](#bans)) |
| life | no bound by time on its way ([overload waits](#overload-waits)); then in its owner's delivery queue until its node takes it or, once stale, its place is needed ([delivery queue](#delivery-queue)) |
| cost of a search | each link between two relays at most two hand-overs that succeed from each end, a send and a return, or one frame and its seen answer; a split adds a round trip; the wrong-hop cap ends a search after 64 hand-overs away from its destination (roadmap D36, D220) |
| message records | at most 649 per relay, below |
| state per relay | the RAM budget of [implementation notes](#implementation-notes) |

*Records.* A full send queue of 103 places (39 regular, 64 hop places),
each waiting one ban of 1.58 s, drains in 162.9 s. That time carries at
most 6 s + 9.83% × 162.9 s = 22.0 s of airtime
([duty cycle cap](#duty-cycle-cap)); a message costs at least an ACK
(18 bytes, 17.0 ms) and an empty DATA (29 bytes, 23.4 ms), 40.3 ms: at
most 546 messages. With the 103 held, whose records are never evicted:
649 (roadmap D42).

## Sleepy nodes

A sleepy node ([nodes](#nodes), roadmap D71) beats on its anchor's channel
([beats](#beats)), takes what its holder has for it in the window after
([delivery](#delivery)), and between its windows receives duty-cycled
([duty-cycled receive and the long preamble](#duty-cycled-receive-and-the-long-preamble)).

A sleepy node holds mail for nobody ([heard table](#heard-table),
[delivery queue](#delivery-queue)). It keeps the neighbour table and the
send queue, and forwards what it is given at once, long preamble to a
sleepy neighbour, short to any other. A carried relay keeps the mail it
holds and takes no new mail.

For a node gone dark the network keeps, for the delivery TTL, the node in
its holder's table and the messages waiting for it; the messages stay
longer while nothing needs their place.

**Plugged in and unplugged.** At boot a node is sleepy or not by its power
(roadmap D71). A sleepy node plugged in is a relay at once:

- it leaves its anchor;
- its address becomes its own position, or its first fix within a search's
  limit if that is far ([position](#position));
- it heartbeats as a relay, the first at once ([heartbeat](#heartbeat)),
  on its home channel, its anchor's until then.

| | |
|---|---|
| a fixed relay unplugged | unchanged for a **grace** of one hour; power back within it changes nothing. After the grace it relays until its battery is low |
| a carried relay unplugged, or a relay on battery that turns carried | sleepy at once (roadmap D152) |
| a relay on battery whose battery is low | turns sleepy, grace or not |
| a node its battery turned sleepy | relays again when its battery is high while it is fixed |
| any node on external power | relays |
| a fixed relay that reboots on battery | a relay, its grace counted from the boot ([implementation notes](#implementation-notes)) |

| | |
|---|---|
| battery low | a calibrated reading at or below 3.504 V, the cell's curve at 20 %, until one at or above the high level, or external power |
| battery high | a calibrated reading at or above 3.611 V, the curve at 30 % |
| reading | the cell's voltage, read on battery once a minute while the radio does not transmit |
| calibration | after 3 h on external power, the first reading an hour into battery is a full cell's, 4.128 V, the curve at 100 %, and every reading after it is scaled to match; a scale beyond ±3 % (the SAADC's gain error at gain 1/6, nRF52840 Product Specification) is not taken; the scale is kept across a reboot; before the first one readings are taken as read |
| the cell's curve | the charge left at a calibrated reading: 5 % + 60 %/V × u + 66 %/V² × u², u the volts above 3.300 V, rounded to the whole percent, within 0 and 100 %; a level's voltage is the curve's inverse, rounded to the millivolt (`docs/discharge-2026-11-11.md`; roadmap D210) |

The curve has no term for temperature (open item O24).

**Turning sleepy.** A node about to turn sleepy takes no more mail, owns
nothing by position, and flushes its delivery queue
([delivery](#delivery)), listening all the time until each message has had
its attempt, or for a delivery TTL at most; a carried relay has flushed
already, at its first far fix. Then it is sleepy, carried or fixed as it
was. It takes the strongest relay it knows as its anchor and listens a
rotation, as at a first choice ([relay choice](#relay-choice)), or is
anchorless with none.

What the flush did not deliver it **hands on**: each message goes into its
send queue, oldest first, as a forward from here toward its address, with
the count it arrived with ([hop count](#hop-count)), admitted as any
forward received with that count ([send queue](#send-queue)) and routed as
any other ([next hop](#next-hop)); one that finds no room waits in the
delivery queue until a hand-over ends. Its search starts here, as at an
origin: with no neighbour at all it waits for one, as the node's own
message does
([a sleepy node's own message](#a-sleepy-nodes-own-message)). Nothing held
is dropped at the turn ([overload waits](#overload-waits); roadmap D31).

**How long the battery lasts.** A node on battery shows how long it can go
on as it runs now ([screen and buttons](#screen-and-buttons); roadmap D169,
D189). It keeps two drains, in percent of the cell an hour: as a relay
with its GPS at rest, and as a sleepy node. Each starts at the figure
below and is moved by samples:

- A sample runs from a reading to the first one at least 13 h later while
  the node runs one of the two ways. It starts an hour after any change of
  that, of the power or of the calibration; a change, or a relay's GPS
  tracking, ends it unused.
- Its drop in percent over its hours moves that way's drain as a moving
  average over 83.7 h.
- Time left, a relay: until the low level at its drain. A sleepy node:
  until the cell is flat; above the low level the whole of what is left at
  its drain; at or below it the low level's share less the time since the
  first low reading.
- The drains are kept across a reboot
  ([implementation notes](#implementation-notes)).

| | |
|---|---|
| a relay's first drain | 0.836 % an hour: 6.41 mA of 767 mAh; radio 6.38 mA (5.8 mA listening, 32 mA for its airtime, `docs/battery-role-2026-10-25.md`), divider 12 µA, GPS in standby 20 µA |
| a sleepy node's | 0.0772 % an hour: 0.59 mA; radio 0.56 mA ([delivery](#delivery)), divider, GPS in standby |
| a sample | at least 13 h: a 1 % drop at a sleepy node's drain, 13.0 h, rounded up to the hour |
| the average | over 83.7 h: a relay from full to 30 % at its first drain |
| the cell | 767 mAh, the charge the curve's run gave |

A full relay on battery shows 3 days and then sleeps about 11 more; a full
sleepy node shows 54 days.

### Relay choice

**First choice.** A sleepy node takes as its anchor the first relay it
hears, at once. Only a fixed relay can be an anchor ([nodes](#nodes)); a
sleepy node whose last heartbeat had the anchorless bit is none
([sleepy nodes alone](#sleepy-nodes-alone)). The node then stays awake a
worst-case rotation and takes the strongest relay it heard, without the
margin below (roadmap D49). It listens on the anchor's home channel, and
its address follows the anchor ([addresses](#addresses)).

**After a reboot.** A sleepy node that boots with a stored anchor
([implementation notes](#implementation-notes)) takes it at once, with no
first rotation; it beats to it at once and once more within a heartbeat
interval, and listens a rotation for its heartbeat as when it checks the
anchor (below).

**Switching.** While it listens, at its first choice and when it checks
the anchor, a node moves to another relay only when that relay has been
heard over at least two heartbeats and its averaged RSSI is at least 8 dB
above the anchor's: 2σ of the difference at σ = 4 dB.

**Asleep.** A sleepy node keeps its anchor's neighbour entry whatever its
age, and wakes for the anchor's next heartbeat on its channel, at the time
the anchor's interval gives ([heartbeat](#heartbeat)). It listens from as
early as the heartbeat can come to as late, and sleeps again as soon as it
hears it:

- The time expected is kept from the earliest heartbeat heard, moved later
  only by the drift of two clocks ([heartbeat](#heartbeat)).
- As early: the time expected less three times the drift since the last
  heartbeat heard, at most 117 ms over a rotation (roadmap D52). As late:
  the drift, the heartbeat's greatest lateness and the airtime of one with
  an update's section more.
- A heartbeat of the anchor heard before the watch opens is no slot's: it
  shows the anchor alive and moves nothing (roadmap D62).
- A heartbeat of the anchor's burst ([heartbeat](#heartbeat)) is in no
  slot. A node whose anchor's last heartbeat heard was of its burst times
  the slots from the next one it hears while it chooses, and if its choice
  ends without one, checks the anchor (below).

**Checking the anchor.** After two rotations without the anchor's
heartbeat the node listens a worst-case rotation, as at its first choice,
keeps the anchor as soon as it hears it, and else takes the strongest
relay it heard. It keeps the anchor all the same when:

- it heard nothing at all: it watches for the anchor as before, and checks
  again only once it has heard any frame;
- it heard frames but no relay (roadmap D51).

A node takes no fix of its own while it has an anchor ([beats](#beats)); a
node back from the dark keeps its anchor (roadmap D46, D47).

**An anchor that turns carried or sleepy.** A node that hears its anchor's
heartbeat with the carried or the sleepy flag (roadmap D71) leaves it: at
once for the strongest other relay that is live in its table and not
carried, else for the strongest a rotation's listen finds, as when it
checks the anchor; with none, it is anchorless. The carried anchor's
heartbeats do not end that listen. A check that hears neither the anchor
nor another relay leaves an anchor last heard sleepy or carried. The
node's address changes as at any switch; what was sent to the old one
reaches it by its anchor's flush ([delivery](#delivery)) or not at all.

### Beats

A sleepy node with an anchor heartbeats on its home channel, the anchor's:
a **beat**, with the sleepy bit ([packet formats](#packet-formats)).

Each beat comes a random half to one **beat period** after the last,
P = 300 s, half the delivery TTL: mean 225 s, 16 beats an hour, 230 ms of
airtime.

| | |
|---|---|
| after choosing or changing its anchor | a beat at once and one more within a heartbeat interval, 5–15 s |
| after following its anchor to a new home channel ([channel moves](#channel-moves)) | one beat there at a random time within 0–15 s (roadmap D57) |

**To anchorless neighbours.** A sleepy node whose neighbour table holds
anchorless neighbours ([sleepy nodes alone](#sleepy-nodes-alone)) sends
each beat also on each channel they listen on, with the long preamble and
the sleepy bit alone: a long heartbeat, 249 ms
([duty-cycled receive and the long preamble](#duty-cycled-receive-and-the-long-preamble)),
4.0 s an hour per channel (roadmap D58). It does so for a neighbour only
once it has heard that neighbour's anchorless beat in two consecutive
rotations of its beats: at least 300 s apart (half a rotation: 13 beats,
each at least half an anchorless beat period after the last) and at most
601 s (a rotation and a heartbeat's lag). It stops when the entry expires
([neighbour table](#neighbour-table)) or the neighbour is heard with the
sleepy bit alone.

**GPS.** A sleepy node with an anchor works by the anchor's position: its
address, the position its beats carry, the centre of its flat plane and
the point it rebuilds short positions near
([positions on the wire](#positions-on-the-wire); a node carried farther
while it hears no relay: open item O19). It takes no fix, runs no search
and measures nothing of its motion: it keeps the carried flag it had when
it took the anchor until it is anchorless or a relay again
([position](#position); roadmap D80, D91). Its own position, the last by
its own fixes, is where it stands when plugged in
([sleepy nodes](#sleepy-nodes)).

Without an anchor a sleepy node searches when it turns anchorless and once
a delivery TTL while it stays so
([sleepy nodes alone](#sleepy-nodes-alone); roadmap D80). A search ends at
the first fix that passes the checks ([position](#position)), after 60 s,
or when the node takes an anchor. A node with no place searches until it
has one, anchor or not. A relay's GPS: [position](#position).

| | |
|---|---|
| with an anchor | no search: standby, 20 µA (L76K hardware design, Table 2) |
| search limit | 60 s: two of the 30 s frames that carry the satellites' orbits (IS-GPS-200) |
| after a search that found nothing | none for 9 h: 60 s at 29 mA is 0.48 mAh; once in 8.6 h keeps it within a tenth of the node's 0.56 mA ([delivery](#delivery)), rounded up to the hour |
| between searches | standby, 20 µA |

A sleepy node with an anchor does not sweep the channels
([busy estimate](#busy-estimate)) or move its home channel by the busy
estimate. It keeps its home channel's noise floor by the readings of its
own heartbeats' checks.

### Heard table

A node that is not sleepy keeps the sleepy nodes with an anchor it hears
beat in its **heard table**:

| | |
|---|---|
| size | 256 entries, by ID ([capacity and sizing](#capacity-and-sizing)) |
| each entry | the node's home channel; until when it listens ([delivery](#delivery)); whether a delivery to it went unanswered since its last beat; the mean RSSI of its last two beats |
| kept | two beat periods, the delivery TTL, after the last frame heard from the node: a beat, its own DATA, whose `aux` also names its home channel ([packet sender](#packet-sender)), or its ACK of a delivery (roadmap D53) |
| when full | a newcomer takes the place of an expired entry, else of the one heard longest ago (roadmap D61) |

### Delivery queue

Kept by nodes that are not sleepy, up to 41 messages per node; a node that
turns sleepy keeps what it held only until it hands it on
([sleepy nodes](#sleepy-nodes)). 41 = ⌈256 × 23 / 144⌉: the heard table's
nodes, 23 texts a day each ([capacity and sizing](#capacity-and-sizing)),
144 TTLs a day.

| | |
|---|---|
| age | counted from arrival in the queue |
| stale | queued the delivery TTL ago or longer; it stays, and goes at its node's next beat as any other |
| a message arriving at a full queue | takes the place of the oldest stale message; with none stale, a message for another node is refused ([receiving DATA](#receiving-data)) |
| **inbox** | every node, sleepy or not, keeps the messages for its own application by the same rule, 23 of them ([receiving](#receiving)) |

A queued message goes to the send queue one per node at a time, oldest
first: when it arrives, when the node beats, when the node takes the one
before, and whenever a hand-over ends and leaves room in the send queue;
there it goes first ([delivery](#delivery)). In the node's window it needs
a place in the regular room ([send queue](#send-queue)) and, in the
bucket, its own frame and the reply reserve, not the airtime of everything
queued (roadmap D55). One the bucket cannot fit yet goes the moment the
bucket will. One whose node has stopped listening by the time its
hand-over would start waits for the next beat. A failed delivery counts no
failure against anyone.

**A neighbour leaving the air.** A message refused by a neighbour whose
last heartbeat had the leaving flag, where the message may end (addressed
to it or to a position it may own; [next hop](#next-hop)), waits in the
queue for that neighbour, by the same rules of age and place. Once its
heartbeat comes without the flag, or its entry is gone, the message goes
into the send queue as a forward received with its count, and is routed as
any. Where the queue has no place, it goes round as any refused message. A
node that is sleepy, or leaving to turn sleepy, holds none (roadmap D186).

**Delivered IDs.** The owner remembers 41 delivered message IDs
([packet formats](#packet-formats)) and ACKs a repeat without delivering
it twice. When it needs the place, it forgets the one delivered or last
repeated longest ago; time alone forgets nothing.

A holder does not forward mail to a node's new anchor, and the sender is
not told ([addresses](#addresses)).

### Delivery

A message that reaches the owner of its position ([owner](#owner)) and is
for another node waits in the owner's delivery queue
([delivery queue](#delivery-queue)); while the owner hears that node beat,
it is the node's **holder**. The last hop is not fresh work
([receiving DATA](#receiving-data)).

It goes as DATA with the node's ID in `dst_hop` and `dst_id`, on the
node's home channel from the heard table, ordinary preamble in the node's
window; the node answers with an ordinary ACK ([the ACK](#the-ack)). A
delivery goes through the send queue as any hand-over
([hand-over](#hand-over)), ahead of the rest. A hand-over waiting between
its attempts, for the channel, a retry or the bucket, steps aside for it,
keeps its place, and is routed again when its turn comes, with fresh
attempts. The DATA carries the hop count as any hand-over
([hop count](#hop-count)).

**The window.**

| | |
|---|---|
| after each beat the holder hears | the node listens 3.39 s: an attempt the holder may have under way, 238.6 ms (a full frame and the ACK timeout), and one hand-over with all its attempts and retry waits, 3.15 s ([retry wait](#retry-wait); roadmap D50) |
| after each DATA the node answers | it listens one hand-over more, 3.15 s; the holder sends the next message for it then |
| end | a hand-over's time with no DATA for it: back to duty-cycled receive |
| the holder's bucket | once it would keep the next message past the hand-over, the node leaves its window and the rest wait for its next beat ([duty cycle cap](#duty-cycle-cap)) |
| a delivery unanswered | marks the node out of its window; its messages wait for its next beat |

**Outside the window, with the long preamble.** A message for a node the
holder hears, outside the node's window, goes at once as DATA with the
long preamble
([duty-cycled receive and the long preamble](#duty-cycled-receive-and-the-long-preamble)),
the node's oldest first and one at a time, while the holder spares the
long frame ([next hop](#next-hop)). Once the node has taken one, it
listens a hand-over more, and what follows goes with the ordinary
preamble. One that goes unanswered marks the node out of reach, and its
messages wait for its next beat (roadmap D112).

**Power.** A sleepy node with an anchor listens 3.39 s after each beat,
3.15 s after each message it takes, and about a second a rotation for its
anchor's heartbeat; it draws 0.56 mA at the design rate in the simulator,
63 days on the 850 mAh battery; without an anchor at most 0.61 mA,
58 days. The microcontroller is not counted (roadmap D47, D48, D50, D58,
D92).

**Outside the windows.** After the deliveries above, one at a time:

- **The flush.** From its first far fix ([position](#position)), or once
  it is to turn sleepy ([sleepy nodes](#sleepy-nodes), roadmap D71), a
  node sends each message in its delivery queue once, as DATA with the
  long preamble on the channel its node listens on, from the heard table,
  else the neighbour table, else its own home channel: one attempt,
  through the bucket as any send. A message whose ACK comes is done; the
  rest stay and go at beats until their place is needed. A full queue is
  41 × 372.8 ms = 15.3 s of airtime.
- **A node heard as a relay.** A holder that hears a heartbeat without the
  sleepy or the leaving flag from a node it holds messages for, carried or
  not, sends them at once, one after the other, ordinary preamble, on the
  home channel the heartbeat names (roadmap D66). Each is done on its ACK;
  unanswered ones wait for the node's next heartbeat or beat heard.

### A sleepy node's own message

A sleepy node with an anchor hands its own message to the anchor while the
anchor is live, with count 0 ([hop count](#hop-count)), and otherwise to
its next hop as any node does ([next hop](#next-hop)); one without an
anchor sends as a relay does ([sleepy nodes alone](#sleepy-nodes-alone);
roadmap D46). The anchor takes it only as fresh work
([receiving DATA](#receiving-data); roadmap D31, D34), and the node takes
a message from its application only while the anchor is not banned.

| | |
|---|---|
| refused | back to the application with the wait the refusal asks ([bans](#bans)); the node takes no message of the application's until that wait is over ([addresses](#addresses)) |
| unanswered | tried again after 60 s, doubled for each unanswered hand-over to the anchor in a row up to the delivery TTL, 600 s, or when the anchor's ban ends if later; until the anchor, or the anchor the node chooses next ([relay choice](#relay-choice)), takes it (roadmap D54) |
| no live neighbour at all | waits for one, routed again every 60 s; the application is not told |

### Sleepy nodes alone

A sleepy node that hears no fixed relay in its first rotation
([channel at boot](#channel-at-boot)) is in **anchorless mode**:

- it forwards at once what it is given;
- its address is its own position, and it owns it;
- a DATA for it does not wait for a beat.

It receives duty-cycled and has no window: every frame to it carries the
long preamble. It keeps the home channel it has, its last anchor's or the
one it chose at boot; it does not sweep or move.

It beats with the sleepy and anchorless bits and the long preamble, each
beat a random half to one **anchorless beat period** after the last, on
the next channel of the rotation each time ([heartbeat](#heartbeat)),
naming its home channel in its first byte. The period is the delivery TTL
over the 13 channels, 600 / 13 = 46.2 s; mean interval 34.6 s, 26 s of
airtime an hour.

Nodes in anchorless mode route among themselves and are a next hop like
any other ([next hop](#next-hop)), each DATA to one with the long
preamble. A relay that hears an anchorless beat answers it
([heartbeat](#heartbeat)) with the long preamble, at once and again 60 s
later, for two of the node's beat periods. A carried relay does not
answer. A sleeping node tells an answer from a heartbeat in its slot by
the time ([relay choice](#relay-choice), roadmap D62).

At the first relay it hears the node anchors as any sleepy node does and
leaves anchorless mode. Its neighbours then keep it as a last resort
([beats](#beats)).

## Phone link

A person's app talks to their node over Bluetooth LE, through one GATT
service on the nRF52840 (roadmap D5). The app owns identity and end-to-end
encryption; the node sees opaque payloads. A node serves the phones paired
with it, its **paired phones**, and on a public node any other phone too,
as a **guest**. Section numbers below are of the Bluetooth Core
Specification 5.0 ([sources](#sources)). How the app chooses a node and
where it sends a text: `bark-app/README.md`.

### Finding the node

The node advertises, connectable, every 1022.5 ms ([sources](#sources)),
while it has a link free ([pairing](#pairing); roadmap D120):

| | |
|---|---|
| advertisement | flags, and the service's UUID; on a public node another UUID in its place ([public nodes](#public-nodes)) |
| scan response | its name: `bark-` and the twelve hex digits of its ID |

### Pairing

A node has up to eight paired phones (roadmap D135, D168). Every
characteristic of the service needs a link encrypted with an authenticated
key, except, on a public node, those a guest needs
([public nodes](#public-nodes)); a read, write or subscription on any
other link is answered with the ATT error 0x05, insufficient
authentication (Vol 3 Part F §3.4.1.1). The node's name and appearance
stay open.

The pairing is LE Secure Connections with passkey entry (Vol 3 Part H
§2.3.5.6.3): the node shows a six-digit passkey on its screen as soon as
the pairing asks for it ([screen and buttons](#screen-and-buttons)) and
the person types it into the phone; only a bench build also writes it to
its USB log (roadmap D226). Every paired phone may do all that any paired
phone does. The app pairs only once the phone holds no bond for the node:
it has the phone remove any first, and where the phone keeps the bond it
tells the person and does not pair (roadmap D194).

**Records.** The node keeps a record for each person paired with it, at
most eight, in flash, each at most 128 bytes:

| | |
|---|---|
| bond | the phone's identity address, identity resolving key (Vol 6 Part B §1.3.2.2) and long-term key |
| tag, name | the person's recipient tag at the node and their name as their card has it, written to Recipient ([the service](#the-service)) |
| mailbox key | once they make the node their home ([settings](#settings)); may be marked removed (below) |
| confirmed | the number of the last of their messages the phone confirmed ([receiving](#receiving)) |
| day | when the person was last seen: their phone linked, a fetch of theirs taken ([mailbox](#mailbox)), or the record made |
| owner | whether their key is the owner's ([the node's owner](#the-nodes-owner)) |
| number | one more than the last the node gave |
| ring mark | the number of the newest item in the ring when the record was made ([mailbox](#mailbox), The ring) |

A record holds a bond, a home or both ([public nodes](#public-nodes),
[the node's owner](#the-nodes-owner)) and is freed when it holds neither.
Two phones of one person are two records with the same key and tag: an
item with their tag sets both records' bits; a fetch opens under the first
record whose key fits, a home before a removed key, and clears only that
record's bit; no rule merges them.

**A removed home.** A home removed, through Phones ([phones](#phones)) or
by the owner's command ([mailbox](#mailbox), The owner's commands), leaves
its key in the record, marked **removed**, with the record's last fetch
([mailbox](#mailbox), The answer). A removed key is no home: a fetch under
it carries no item and runs no command, it is never the owner's, and Home
and the owner's list count it as none. A record with a bond and a removed
key lists as paired only; one with only a removed key is not listed
([phones](#phones)). The mark goes when the same key is set again, by a
Home write ([settings](#settings)) or the owner's add; a leave forgets the
key with the rest of its person's records.

**Taking a record.** A pairing takes, in this order:

1. from a phone whose identity address a record's bond names: that record,
   replacing the bond and keeping the rest;
2. a free record;
3. the record that holds only a removed key whose person was seen longest
   ago, forgetting that key (the owner's add does the same);
4. on a node no phone is paired with: the home whose person was seen
   longest ago, the owner's last, forgetting that home;
5. else the node refuses: it answers the Pairing Request with Pairing
   Failed, unspecified reason (Vol 3 Part H §3.5.5), shows no code, and
   its screen says `PHONES FULL` for 30 s. Node's `room` tells an app
   before it pairs.

A pairing that is not authenticated (Just Works), or that does not bond,
changes nothing, and the node closes its link.

**Links.** Each paired phone keeps its link while in reach. The node takes
a link for each paired phone and one more: nine at most, 5.9 KB of static
RAM each, 52.9 KB (roadmap D150, D168; `bark-fw/README.md`, "Tasks and the
stack").

**The removed phone.** A phone whose bond the node no longer holds
([phones](#phones)) encrypts the link with its old key as it connects, and
the node ends the link with authentication failure (error code 0x05, Vol 1
Part F). A link that a phone holding a bond for the node loses before its
first read tells the app that the node is no longer the person's; the app
says so and offers to pair again.

### The service

The node offers an ATT MTU of 247: the 251-byte link-layer payload (Vol 6
Part B §2.4) less the L2CAP header's 4 (Vol 3 Part A §3.1). The app asks
for 247 by an MTU exchange (Vol 3 Part F §3.4.2) as soon as it connects.
The node also takes long writes and serves long reads (Vol 3 Part F
§3.4.6, §3.4.4.5). A notified value fits the 20 bytes a notification
carries at the default MTU of 23 (Vol 3 Part F §3.4.7.1; roadmap D119),
except Ask's, 23: at that MTU its notification lacks what the neighbour
gathers, which the app shows as nothing gathered (roadmap D158).

The service has one random UUID, its second 16 bits counting the service
and its characteristics in the order of this table:

| | UUID | properties | value | bytes |
|---|---|---|---|---|
| service | `28bc0001-a1b8-4893-9537-2ee3559d7644` | | | |
| Node | `28bc0002-…` | read | version(1) oldest(1) id(6) image(4) room(1) | 13 |
| Status | `28bc0003-…` | read, notify | flags(1) address(7) battery(2) relays(1) heard(1) boot_left(2) installing(2) inbox(1) phones(1) gps(1) | 19 |
| Send | `28bc0004-…` | write | dst_pos(7) dst_id(6) payload(0–224) | 13–237 |
| Inbox | `28bc0005-…` | read | msg_id(4) payload(0–224), empty when none; a guest's: number(3) msg_id(4) payload(0–224) | 0, 4–228, 7–231 |
| | | write | msg_id(4) | 4 |
| Given back | `28bc0006-…` | read, notify | msg_id(4) wait(2), empty when none | 0, 6 |
| Map point | `28bc0007-…` | write | 0 to clear; 1 then position(7) to set | 1, 8 |
| Clock | `28bc0008-…` | read | utc(4) offset(2) source(1) | 7 |
| | | write | utc(4) offset(2) | 6 |
| Home | `28bc0009-…` | read | set(1) mine(1) oldest(3) newest(3) | 8 |
| | | write | 0 to clear; 1 then key(32) sent(4) to set | 1, 37 |
| Neighbours | `28bc000a-…` | read | next(1) floor(1) own(7) entry_len(1), then per neighbour id(6) position(7) rssi(2) age(2) flags(1) | 10–244 |
| | | write | slot(1) | 1 |
| Update | `28bc000b-…` | read | state(1) refused(1) held(4) version(4) hash(32) | 42 |
| | | write | 0 then header(256) to begin; 1 to commit; 2 to reboot | 257, 1 |
| Image | `28bc000c-…` | write, write without response | offset(4) bytes(1–240) | 5–244 |
| Recipient | `28bc000d-…` | write | tag(1) version(1) oldest(1) name(0–32) | 3–35 |
| Public | `28bc000e-…` | read | public(1) | 1 |
| | | write | 0 private, 1 public, 2 unpair | 1 |
| Guest | `28bc000f-…` | write | number(3) | 3 |
| Ask | `28bc0010-…` | write | id(6) | 6 |
| | | read, notify | id(6) status(17), empty when none | 0, 23 |
| Phones | `28bc0011-…` | write | 0 then slot(1) to read from; 1 for a code; 2 then number(2) what(1) code(3) to remove | 2, 1, 7 |
| | | read | next(1), then per record number(2) flags(1) day(2) tag(1) name_len(1) name(0–32) | 1–244 |

Positions and IDs are in the radio's encodings
([positions on the wire](#positions-on-the-wire),
[node identity](#node-identity)); a message ID is the 32-bit hash of
[packet formats](#packet-formats); every integer is big-endian. The codecs
are `bark-link`. A write is a write request, except Image's, which may be
a write without response. A write that is not whole is answered with the
ATT error 0x0D, invalid length; one with a field the node does not take
with 0x13, value not allowed (Vol 3 Part F §3.4.1.1).

**Node.** The link version and `oldest`, the oldest link version whose
apps the node serves ([versions](#versions)); the node's ID; the version
of its firmware image; `room`, how many more phones may pair: the records
free, 0 when it would refuse a pairing, and 1 on a node nobody is paired
with that has none free ([pairing](#pairing)).

**Recipient.** The app writes it as it connects, after reading Node: the
person's recipient tag at this node ([the payload](#the-payload)); the
app's link version and the oldest node version it serves
([versions](#versions)); on a paired phone's link the person's name as
their card has it ([pairing](#pairing)). A guest writes no name, and the
node keeps a guest's tag only for its link
([public nodes](#public-nodes)).

**Status.** Notified at every change except the countdown of `boot_left`
and `gps`'s moves; one value is one notification and one read at a
connection:

| | |
|---|---|
| flags bit 0 | ready: the node takes a message from this phone now ([sending](#sending)) |
| bit 1 | on external power, USB |
| bits 2, 3, 4 | sleepy, carried, map point, as its heartbeat says them ([packet formats](#packet-formats)) |
| bit 5 | placed: it has a place, and `address` holds a position; else `address` is zero ([position](#position)) |
| bit 6 | its first rotation runs ([channel at boot](#channel-at-boot)) |
| bit 7 | anchorless ([sleepy nodes alone](#sleepy-nodes-alone)) |
| address | the position of its address: its own, or a sleepy node's anchor's ([addresses](#addresses)) |
| battery | millivolts as last read, uncalibrated; 0 on external power and before the first reading |
| relays | the relays it knows |
| heard | its live neighbours, sleepy or not ([neighbour status](#neighbour-status); roadmap D159) |
| boot_left | seconds left of its first rotation, rounded up; 0 once it is over |
| installing | the low 16 bits of the image version it drains to install ([updates over the mesh](#updates-over-the-mesh), the install); 0 when none. The node ends its phones' links for a swap, and the app waits for it (roadmap D182) |
| inbox | the phone's messages not yet confirmed ([receiving](#receiving)) |
| phones | a count, wrapping, that moves at each change to what Phones lists for this phone ([phones](#phones)): a phone paired, removed or unpaired, a home set or cleared, a person added or gone, the owner's mark moved, a name or tag written, a phone's link up or down, a day; 0 on a guest's link. The node tells its linked paired phones once the records are written; at a change, the app reads Phones again |
| gps | the GPS's signal, as a status asked tells it ([neighbour status](#neighbour-status)); not notified, the app reads it |

### Sending

The app writes Send while ready is set. The node answers once it has taken
the message, or with the ATT error 0x80, not ready, when it takes none
now; the app then waits for ready. The app hears of a taken message again
only if the node gives it back ([addresses](#addresses)). The node holds
one message of its app's at a time ([receiving DATA](#receiving-data)).

**Turns.** The node's phones, paired and guests, take turns, and the turn
is ready: the node sets it, while it takes a message, for one phone at a
time, and only for a phone that wants it (roadmap D184).

- A phone asks for the turn by writing Send while ready is not set for it,
  and wants it until it has it.
- The phone that has the turn keeps it until the node takes its message,
  its link ends, or it has had ready for a ban (1.58 s; [bans](#bans))
  while another phone wants it.
- The turn then goes to the next phone that wants it, in the order of the
  links, and stays where it is if none does; with no phone holding it, it
  goes to the next that wants it, else the next linked.
- A Send out of turn is answered 0x80, unless the turn passes to the phone
  at that write; then it is taken.
- An app with a message waits for ready after asking, and asks again if
  ready has not come within 12.64 s: a ban for each of the eight other
  links.

**Given back.** When the node gives the app's message back, it sets Given
back to the message's ID and the seconds left of the wait the refusal
asked, rounded up, and notifies it. The value is that phone's own, and
stays until the node takes another message from it, the wait counting down
to 0. A message given back to a guest whose link has ended is dropped. The
app finds the message by its ID among those it sent, computing each one's
as the node does, shows the person the wait, and offers it again once
ready is set, or gives it up, as the person chooses.

### Receiving

The node keeps every message for its application in its ring
([mailbox](#mailbox), The ring), in flash, with the recipient tag from the
payload's second byte ([the payload](#the-payload)), and confirms it to
the core once its slot is written ([receiving DATA](#receiving-data));
only then does it show it to a phone. A paired phone's messages are those
with its record's tag ([pairing](#pairing)); a record with no tag written
has none.

Inbox reads the oldest of the phone's messages after the last it
confirmed, with the message ID it arrived with, as stored. The app writes
the ID back, which confirms it; a write of another ID is answered 0x13.
Status's count is the phone's messages not yet confirmed. The ring keeps
confirmed messages until it wraps, the guests' as well
([public nodes](#public-nodes); roadmap D168).

### Settings

**Map point.** A write of 1 and a position sets the node's map point; 0
clears it ([position](#position)).

**Clock.** The app writes the phone's time, UTC seconds since 1970, and
its UTC offset in minutes, −720 to +840, at every connection; another
offset is answered 0x13. The node keeps the offset across a reboot and
takes the time only while it has none from its GPS
([position](#position)). A read gives the node's time (0 with none), the
offset it keeps, and the time's source: 0 none, 1 GPS, 2 the phone
(roadmap D130).

**Home.** A write of 1, the mailbox key and the phone's `sent`
([the payload](#the-payload)), later than any it gave before, makes the
node the home of the phone's person, the key kept in the phone's record;
0 forgets that key ([mailbox](#mailbox)).

- The node takes no fetch of the key whose `sent` is not later than the
  write's ([mailbox](#mailbox), The fetch; roadmap D224, D236).
- Written for a key the phone's record holds already, a home or removed
  ([pairing](#pairing)), or joining a record that holds it (next), the
  record keeps the later of its last fetch and the write's `sent`.
- A record that holds the same key and no bond then takes this phone's
  bond, name and tag, and the phone's own record is freed; the ring's
  marks stay that record's ([mailbox](#mailbox), The ring).
- Another person's key is removed only through Phones
  ([phones](#phones)) or by the owner's command ([mailbox](#mailbox), The
  owner's commands).
- A key set takes the owner's seat if it is empty
  ([the node's owner](#the-nodes-owner)).

A read says whether the node is anyone's home, whether it is this phone's
person's, and the numbers of the oldest and the newest item its ring
holds, 0 when none.

### The neighbour map

**Neighbours.** The node's neighbour table
([neighbour table](#neighbour-table)), paged. The app writes a slot of the
table, 0 first; a read gives the live neighbours in that slot and after
it, as many as fit, 13, and the slot to write next, 0 at the table's end.
The slot read from is 0 at each connection. A neighbour keeps its slot
while it stays in the table. A read starts with the slot to write next,
the node's noise floor in dBm, signed ([busy estimate](#busy-estimate)),
the node's own position, zero while it has none (for a sleepy node not its
address; [position](#position)), and the bytes of each neighbour, 18, by
which the app steps. Each neighbour: its ID, its position, `rssi_ema` in
tenths of a dBm, signed, the whole seconds since it was last heard, at
most 65 535, and its flags:

| | |
|---|---|
| bit 0 | sleepy |
| bit 1 | map point |
| bit 2 | demoted |
| bit 3 | banned ([bans](#bans)) |
| bit 4 | this sleepy node's anchor ([relay choice](#relay-choice)) |
| bit 5 | carried |
| bit 6 | its last heartbeat had the anchorless flag |
| bit 7 | it has taken a message from this node |

A link's **quality** is the margin of its averaged RSSI over the noise
floor ([neighbour table](#neighbour-table)): fragile below 4.1 dB; good
from 6.9 dB, a full frame through a fade of 2σ, 8 dB; fair between.

**Ask.** The app writes a neighbour's ID, and the node asks that neighbour
for its status ([neighbour status](#neighbour-status)); the write is
answered once the node has taken it, whether or not the ID is a live
neighbour's. The app asks the neighbours it shows when the person opens or
refreshes the map, and one neighbour when the person opens its details,
never at other times (retries: `bark-app/README.md`). Each answer the node
is given, the neighbour's ID and its status as the radio carries it, is
Ask's value from then on, notified to the paired phones; empty before the
first since boot.

### Updates from the phone

The app stages a new firmware image on the node, which the node's loader
swaps in at its next boot (roadmap D79; `bark-fw/README.md`, "Updates").
The image is a header of 256 bytes, holding its version, length and
SHA-256, then its bytes.

1. The app writes 0 and the header to Update to begin: the node forgets
   any image staged before, and takes the header if its version is above
   the node's own, a signature in it verifies under a key the node holds
   (roadmap D144), and no swap is asked for already.
2. The app writes the image to Image in pieces, each its offset in the
   image and as many bytes as one write carries, 240 at the MTU of 247,
   without a response; the node writes each into its external flash, and
   takes a piece only at the offset where the bytes it holds end. The app
   reads Update after every seven pieces (the node's host queues 8 ATT
   PDUs for a link) and goes on from the bytes held; a hash not its
   image's is another image being staged, which it begins again.
3. With every byte held, the app writes 1 to commit: the node checks the
   image against the header's hashes, and every other check its console's
   commit makes, and asks the loader for the swap.
4. The app writes 2 to reboot: the node answers, ends the link and resets.

Update's read: the state (0 idle, 1 receiving, 2 staged, 3 committed); why
the last begin, commit or reboot was refused, 0 if it was not; the bytes
held; the header's version and hash, zero while idle. A begin, commit or
reboot the node refuses is answered with the ATT error 0x81, refused:

| refused | |
|---|---|
| 1, header | not a header the node reads |
| 2, version | not above the version the node runs |
| 3, busy | a swap is asked for already: reboot first |
| 4, incomplete | not every byte is held |
| 5, hash | the bytes do not hash to the header's hashes, of the image and of its block table, and are forgotten |
| 6, battery | on battery at or below the low level ([implementation notes](#implementation-notes)); the image stays staged |
| 7, flash | the flash failed, and the image is forgotten |
| 8, signature | no signature in the header verifies under a key the node holds |

What is staged lasts across connections until the node resets (roadmap
D145). An image from the phone keeps itself once the node has started
([updates over the mesh](#updates-over-the-mesh), the install); the app
tells the person when the node still hears none 585.9 s after it came back
(roadmap D159, D164; `bark-app/README.md`, "Updating a node").

### Versions

Node's `version` and Recipient's say the link version each side speaks,
and their `oldest` the oldest version of the other side it still serves.
An app and a node serve each other when each side's version is at least
the other's oldest. When they do not, the node answers Recipient's write
with the ATT error 0x82, incompatible, and ends the link, and the app
tells the person which side to update (roadmap D177). This specification
is link version 1, and each side's oldest is its own version (roadmap
D188).

A later version may add characteristics and append fields to the values
the node writes: Node, Status, Given back, Clock, Home, Update, Ask, and
each neighbour of Neighbours; an app reads the fields it knows and ignores
the rest. The values the app writes change only with the version. A change
an older side cannot follow raises the newer side's `oldest`. A change to
what a field means takes a new service UUID.

**Changes to the table.** Besides the service, the node's attribute table
holds the GAP service, with the node's name and appearance, and the GATT
service, with Service Changed, indicate only (Vol 3 Part G §7.1), and the
database hash, readable on any link (Bluetooth Core Specification 5.1,
Vol 3 Part G §7.3). Every change to the table comes with a new link
version. The node keeps in each record the link version whose table its
phone last saw, and whether the phone has subscribed to Service Changed
(Vol 3 Part G §3.3.3.3). On a link encrypted with a bond, while the phone
is subscribed and the version it saw is not the node's, the node indicates
Service Changed over every handle, 0x0001 to 0xFFFF, and once the phone
confirms, keeps the node's version as seen. A pairing keeps the node's
version; a link without a bond is told nothing (roadmap D142).

### Public nodes

A node is public, serving guests as well as its paired phones, unless made
private. Any paired phone writes 0 to Public to make the node private and
1 to make it public; a read gives 1 for public. The setting is the node's,
kept across reboots and pairings; a node nobody has paired with is public
(roadmap D150).

A paired phone writes 2 to **unpair**: the node forgets that phone's bond,
in flash too, and keeps its record's home, if any ([pairing](#pairing)).
The node answers the write, then ends the link. No code is asked. With the
last bond gone the node is one nobody has paired with. The app removes the
phone's own copy of the bond once the node has answered; where the phone
does not let it, the person does. A link the phone encrypts with that copy
the node ends, as for a removed phone ([pairing](#pairing)).

On a public node:

- *The advertisement* holds, in place of the service's UUID, the public
  node's, `28bc1001-a1b8-4893-9537-2ee3559d7644`, which no attribute has.
  The scan response is the same.
- *Node, Status, Send, Inbox, Given back, Recipient and Guest* take any
  link, encrypted or not, and so do the subscriptions to Status and Given
  back. Map point, Clock, Home, Neighbours, Update, Image, Public, Ask and
  Phones stay the paired phones'.
- *Its one link more is a guest's* ([pairing](#pairing)); the node keeps
  no state for a guest beyond its link. A guest connects, writes its
  recipient tag at the node to Recipient, sends what it has, reads what
  waits for it, and leaves. The node ends a guest's link 30 s after it
  began (the ATT transaction's timeout, Vol 3 Part F §3.3.3); a phone that
  is not paired and writes Recipient while another guest's link is up is
  closed at once. Made private, the node closes the guest's link.
- *A guest's messages* are those in the ring with its tag, whatever their
  marks say ([mailbox](#mailbox), The ring), a record's tag or not; a
  guest's reading clears no bit. A guest writes to Guest the number of the
  last item it took, 0 at first; an Inbox read gives the oldest item after
  it with the guest's tag: its number, the message ID it arrived with and
  the payload, or nothing. A number above the newest item's counts as 0.
  The guest keeps the number, per node; a write to Inbox on its link is
  answered 0x13.

### Phones

Any paired phone reads the node's records ([pairing](#pairing)) and
removes one without that record's phone. The app writes 0 and a slot, 0
first; a read gives the records in that slot and after it, as many whole
ones as fit, and the slot to write next, 0 after the last. The slot read
from is 0 at each connection. The app reads Status after the last page,
and reads every page again if `phones` moved since before the first. Each
record: its number, its flags, the day its person was last seen
([pairing](#pairing)), counted from 1 on 2026-01-01 UTC
([the payload](#the-payload)), 0 when the node had no time on any of those
days, the person's recipient tag at the node, and their name, by its
length byte. The node writes a record again when its day changes, at most
once a day.

| flags | |
|---|---|
| bit 0 | paired: it holds a bond |
| bit 1 | home: it holds a mailbox key, not removed ([pairing](#pairing)) |
| bit 2 | its phone is linked now |
| bit 3 | it is the reading phone's |
| bit 4 | its tag is known; else the tag is 0 |
| bit 5 | its key is the owner's ([the node's owner](#the-nodes-owner)) |

**Removing, and making an owner.** The app writes 1, and the node draws a
fresh six-digit code on its screen at once
([screen and buttons](#screen-and-buttons)). Then the app writes 2, the
record's number, what (1 the bond, 2 the home, 3 both, 4 make the record's
key the owner's; [the node's owner](#the-nodes-owner)), and the code, as a
number.

| | |
|---|---|
| taken | within 30 s of drawing the code (the Security Manager's timeout, Vol 3 Part H §3.4) and with that code: the node forgets the bond, in flash too, ending its link if it has one, or marks the home removed ([pairing](#pairing)), or both, and frees the record if it then holds neither; or it moves the owner's mark |
| another code, or after the 30 s | ATT error 0x81, refused; the code is forgotten |
| a number no record has, or a part the record does not hold (a home to make the owner's among them) | 0x13; the code stays |

A removed bond's phone learns of it as the removed phone does
([pairing](#pairing)); a removed home's person at their phone's next link
to the node, from Home, or at their next fetch from elsewhere
([mailbox](#mailbox), The answer). The last bond removed leaves the node
one nobody has paired with ([public nodes](#public-nodes)).

### The node's owner

One person at a node may be its **owner**, who keeps the list of the
people whose home it is by commands on their fetches
([mailbox](#mailbox), The owner's commands). The node knows the owner by
their mailbox key: every record that holds that key is the owner's, and no
other is ([pairing](#pairing)).

- The seat is empty while no record holds the mark. A Home write of a key
  then takes it ([settings](#settings)), also from a phone whose record
  held that key already.
- At the node, a paired phone moves the mark to another record's key by
  the code the screen shows ([phones](#phones)); that record must hold a
  home. The owner before stays homed, without the mark.
- A record freed, or its home removed, in any way, takes the mark with it
  when no other record holds the key as a home.

The mark is a flag in the record, in flash. If it is found on two keys,
the node keeps it on the key of the first record, by place, that has it.

### The payload

The app owns it; this is its layout (roadmap D123 to D127, D150;
`docs/app-layer-2026-11-05.md`). The codec and the test vectors are
`bark-app-layer`.

*Keys.* A person's **identity** is an X25519 public key. Every key comes
from a 128-bit random seed, backed up as 12 words: the seed and the first
4 bits of its SHA-256, 11 bits a word, from a fixed list of 2048
(`bark-app-layer/src/words.txt`). All cryptography is libsodium's:

| | |
|---|---|
| master key | `crypto_generichash(seed, 32)` |
| identity key pair | `crypto_box_seed_keypair` of `crypto_kdf_derive_from_key(master, 1, "bark-key")` |
| mailbox key | `crypto_kdf_derive_from_key(master, 2, "bark-mbx")` |

No signatures; keys do not change.

*Forms.* A payload's first byte is its format: the version, 2, in the high
four bits, the form in the low four. Its second is the recipient tag
(below), in every version. A receiver drops a payload of a version or form
it does not know. A sender writes each payload in a version its receiver
is known to read: to a contact, at most the newest their card says (*the
card*, below); to the home, at most the newest its summaries say
([mailbox](#mailbox), The answer); before either is known, the oldest
version it writes. Version 2 is the only one (roadmap D238).

Version 15 and form 15 are reserved to say that the number follows in a
byte of its own; nothing writes them, and a receiver drops them as
unknown. The same holds for a card's or a summary's newest version of 15.

| form | to | after the format byte and the recipient tag | over the plaintext |
|---|---|---|---|
| 0, boxed | a contact | `siv` (16), ciphertext, under the pair's keys (below) | 18 |
| 1, introduction | someone who may not know the sender | `crypto_box_seal` output (ephemeral key 32, tag 16, ciphertext) | 50 |
| 2, fetch | the person's home node | `siv` (16), ciphertext, under the mailbox key's keys, bound to the home node's ID (below) | 18 |
| 3, summary | the person's current address, from their home node | `crypto_secretbox_easy` output under the reply key ([mailbox](#mailbox)) of the round's items (1), the items still unsent after it (2), flags (1) and what a command did (0 to 202); the nonce is not sent | 22 to 224 in all |
| 4, boxed item | the same | the place (1), then a kept boxed payload after its format byte and recipient tag, under the reply key's stream | 1 over the item |
| 5, introduction item | the same | the same, for a kept introduction | 1 over the item |

Forms 2 to 5 are the [mailbox](#mailbox)'s. The app writes at most
224 − 1 = 223 bytes.

*One value as nonce and proof.* Forms 0 and 2 carry one 16-byte value,
`siv`, in place of a random nonce and a tag (roadmap D221, D224).

| | |
|---|---|
| shared key | boxed: `crypto_box_beforenm` of the two identities; fetch: the mailbox key |
| MAC key | `crypto_kdf_derive_from_key(shared, 1, "bark-mac")` |
| encryption key | `crypto_kdf_derive_from_key(shared, 2, "bark-enc")` |
| `siv` | `crypto_generichash` to 16 bytes, keyed with the MAC key, of the format byte, for a fetch the home node's ID (6), and the plaintext |
| ciphertext | `crypto_secretbox_detached` of the plaintext under the encryption key with `siv` and 8 zero bytes as the nonce, its tag dropped: the plaintext XORed with XSalsa20's keystream from its 33rd byte |

The receiver runs the same call over the ciphertext, computes `siv` again
over the same bytes, a fetch's with its own node ID, compares the two in
constant time, and neither keeps nor acts on the plaintext unless they
match. The recipient tag is not hashed. The home's ID is not sent.

*Recipient tag.* The first byte of `crypto_generichash` of the
destination's node ID to 16 bytes, keyed with the recipient's identity; in
the clear. A boxed payload or an introduction carries its recipient's tag
at the node it is sent to, a fetch the requester's at its home node, and a
reply the one the fetch asks for. A node keeps a payload in its ring for
the phone whose tag it carries ([receiving](#receiving),
[public nodes](#public-nodes)); every app ignores it. A phone handed a
payload it cannot open drops it.

A boxed payload names no sender: the receiver tries the keys of each
contact until one opens it. An introduction carries the sender's card; its
sender is shown as unverified until a boxed payload from that identity
opens. A sender uses introductions until a payload from the receiver, of
either form, has opened at the sender. An introduction naming an identity
the receiver has verified is dropped. Payloads are not padded.

*Plaintext.* Forms 0 and 1 hold a header, then records:

| field | bytes | |
|---|---|---|
| flags | 1 | bit 0: an address follows; bit 1: the sender's identity is the greater of the two, byte by byte; the others zero |
| sent | 4 | the sender's clock, seconds since 2026-01-01 00:00 UTC, on each phone later than any it sent before |
| address | 0 or 13 | the address of the node the sender writes from |

| record | bytes | |
|---|---|---|
| text | 1 + rest | type 1, UTF-8, or 2, windows-1251 when that page holds the whole text; runs to the end, so it comes last |
| text part | 1 + 1 + rest | type 5, UTF-8, or 6, windows-1251 when that page holds the whole part; then the part's number less one in the high four bits and the count of parts less one in the low four, the count 2 to 16; runs to the end, so it comes last |
| reaction | 1 + 4 + rest | type 3, the `sent` of the text it answers, then one emoji in UTF-8: one extended grapheme cluster of at most 35 bytes, the longest in Unicode 17.0's emoji list; runs to the end, so it comes last |
| card | 1 + 1 + 38 to 100 | type 4, the card's length, then the card |
| home | 1 + 13 + 32 | type 7, a home node's address, then the mailbox key its owner had it take for the receiver ([mailbox](#mailbox), The owner's commands), with which the receiver may make it their home |

- A receiver drops a message whose bit 1 is not what the two identities
  say, and a text whose contact and `sent` it already has.
- The address is that of the node the sender's phone is connected to or
  was last; included until the contact has answered a message that carried
  it and whenever it changes.
- The app sends nothing by itself: a reaction is sent by the person's
  choice, and a text is sent again only when the person says so (roadmap
  D126).
- A reaction names the text by its first part's `sent`; the newest by its
  message's `sent` stands. A reader drops a message whose reaction is no
  character, more than one, or longer than the bound (roadmap D185, D187).

*Long texts.* A text longer than one message holds is split into parts,
each a message with its own `sent`. The parts take consecutive `sent`: a
part's `sent` less its number and one is the first part's, which names the
text. A part holds 223 − 18 − 5 − 2 = 198 bytes of text in a box without
an address; 16 parts hold 3 168 bytes (roadmap D137, D221, D222).

*The card*, shown as a QR code, 38 to 100 bytes:

| field | bytes | |
|---|---|---|
| version | 1 | 3 |
| flags | 1 | bit 0: a home address follows; bit 1: the node's address follows; bits 2, 3 zero; high four bits: the newest payload version the person's app reads |
| identity | 32 | |
| home time | 4 | when the home was set or cleared, as `sent` |
| home address | 0 or 13 | |
| node address, time | 0 or 13 + 4 | the node the phone is linked to, or was last, when the card is made or shown, unless it is the home; then when the phone was last linked to it, as `sent`: now, to the minute, while linked, else when the link ended |
| name | 0 to 32 | display name, UTF-8 |

As text: `BARK:` and the bytes in base32 (RFC 4648) without padding. A
card with a later time, or the same time, replaces a contact's card; the
person sends their card to their contacts when it changes. Without a home,
the node on the card keeps the person's mail in its ring, by their
recipient tag; the card says that node is **temporary** (roadmap D162).

A message has no ID field: the network identifies it by a hash of its
destination and payload ([packet formats](#packet-formats)). The app must
make every payload unique ([message records](#message-records)): `sent`,
which `siv` hashes, the introduction's ephemeral key, and a reply's round
and place do that.

## Mailbox

A person may set a node as their **home**, from the app over the phone
link; the app then puts its address on the person's card. Each person
paired with a node may make it their home, each with their own key
([pairing](#pairing), [settings](#settings)), and its owner adds people
with no phone at it (The owner's commands, below). It must be a fixed
relay ([nodes](#nodes)). The app gives it the mailbox key and nothing
else, by Home ([settings](#settings); `bark-link` `home`; roadmap D128;
`docs/app-layer-2026-11-05.md`, "The mailbox"). The mailbox is an
application on the node, using only the core's application interface
(`core-interface.md`, Application).

**The ring.** A node, a home or not, keeps every payload delivered to its
node ID, except a fetch, in a ring of slots in flash, numbered from 1, and
erases the oldest page when the ring is full; an item leaves the ring only
so ([receiving](#receiving)). The board's ring holds 1024 items (roadmap
D139). A slot's mark is a byte, a bit for each of the node's eight records
([pairing](#pairing)): a bit set says the item is that record's and not
yet sent to it.

| | |
|---|---|
| an item a reply can carry: a boxed payload or an introduction of at most 223 bytes ([the payload](#the-payload)) | written with the bits of the records whose tag it carries set, the rest clear |
| any other item | every bit clear: read at the node through Inbox; no fetch counts or sends it |
| a bit is cleared, without an erase | when a reply carries the item (below), and when the record's phone confirms it through Inbox |
| a record's bit in the marks of the items up to the newest when the record was made | counts for nothing; its phone still reads those with its tag ([receiving](#receiving)) |

**The fetch** is a form 2 payload to the home address whose plaintext is
`sent` (4), the requester's current address (13) and its recipient tag at
that address's node (1), and may end in a command (The owner's commands,
below): 36 to 102 bytes. The node takes it only if it opens under one of
its records' mailbox keys and its own node ID, and its `sent` is later
than the last fetch's that record took, which the node keeps for each; the
record's items are those with its tag. The node has that `sent` on flash
before it hands the round's first reply to the core (roadmap D224). A
fetch is sent only on a person's act: when they ask, or as a choice they
made calls for (roadmap D128).

**The answer.** The node answers a fetch once, to the fetch's address, one
reply at a time through its send slot: first a summary, then one reply for
each of the newest 32 of the record's items whose bit is set, newest
first.

- The summary: how many items the round carries, and how many of the
  record's items stay unsent after them, both as the fetch was taken. Its
  flags: bit 0, the record's key is the owner's
  ([the node's owner](#the-nodes-owner)) once any command of the fetch has
  run; high four bits, the newest payload version the home reads
  ([the payload](#the-payload)); the others zero. A command's outcome
  follows them.
- A fetch taken counts as its person seen ([pairing](#pairing)).
- The node clears an item's bit when it hands the item's reply to the
  core; nothing confirms it.
- A record has one round at a time, which a later fetch of its own ends;
  the rounds of several records run at once, their replies handed to the
  send slot in turn. An item that arrives after the fetch waits for the
  next one.
- A reboot ends a round: what it had not handed over stays unsent.
- A reply the home node's neighbours refuse is given back to the mailbox
  and goes again.

A fetch taken under a removed key ([pairing](#pairing)) is answered by its
summary alone: no item, none remaining, the owner's bit clear, outcome 5,
removed (The owner's commands, below); a command in it does not run. Its
`sent` is kept as any fetch's.

Every reply carries the fetch's recipient tag and is under the record's
**reply key**, `crypto_kdf_derive_from_key(mailbox key, 3, "bark-rpl")`,
with the nonce: the fetch's `sent` (4), the reply's place (1), 19 zero
bytes (roadmap D222).

| | |
|---|---|
| summary | form 3, place 0, sealed with its tag |
| item | place: its rank in the round, 1 to 32, in the clear. Form 4 for a boxed payload, 5 for an introduction; then the place; then the item without its format byte and recipient tag, encrypted by `crypto_secretbox_detached` with its tag dropped ([the payload](#the-payload)) under the reply key and the reply's nonce |

The requester rebuilds the item, its format byte from the reply's form and
the reply's recipient tag, and opens it as any boxed payload or
introduction; it tries its latest rounds in turn, and the round under
which the item opens is the reply's. The item's number is not sent.

A batch of 32 and its summary are the 33 full frames a full bucket sends
with the reply reserve left ([send queue](#send-queue)).

**The owner's commands.** A fetch's plaintext may end in one command, its
first byte saying which. A fetch whose command the node does not read
opens as no fetch.

| command | after its first byte | |
|---|---|---|
| 1, add | key (32), tag (1), name (0–32) | a record for a person with no phone at the node: a mailbox key, their recipient tag at the node, which the owner makes from their card, and their name as the card has it; a home, and no bond. Its last fetch is this fetch's `sent`. A record that holds the key removed is a home again instead, with this tag and name, keeping the later of its last fetch and this `sent` |
| 2, remove | number (2) | the home of the record of that number is removed ([pairing](#pairing)); a bond it holds stays |
| 3, list | | every record |
| 4, hand over | number (2) | the key of the record of that number, which holds a home, is the owner's; the owner before stays homed |
| 5, leave | | the node forgets the sender's key, in every record that holds it, freeing those with no bond |

All but leave are the owner's alone. The node runs the command as it takes
the fetch, has the records on flash before it hands the summary over, and
says in the summary, after its flags, what the command did, in a byte:

| outcome | |
|---|---|
| 0, ok | done; for a list, the records follow |
| 1, not allowed | the sender's key is not the owner's; nothing changed |
| 2, full | an add, and no record is free ([pairing](#pairing)) |
| 3, already there | an add of a key a record holds as a home; nothing changed |
| 4, no such home | no record of that number holds a home |
| 5, removed | the sender's home here was removed; nothing else was done (The answer, above) |

A list gives each record as a Phones read does ([phones](#phones)), its
flags' bit 3 marking the sender's own, in 224 − 22 − 1 = 201 bytes: when
the names do not all fit, each is cut, at a character's end, to the most
bytes at which all do.

- The app gives each person it adds a new random key, which reaches them
  from the owner (the home record, [the payload](#the-payload)).
- A record added takes no fetch of its key whose `sent` is not later than
  the add's ([settings](#settings)). Its day is the day it is made, and
  its tag gives it the items with that tag (The ring, above). If the
  person later pairs at the node and sets Home with their key, that record
  takes their phone's bond ([settings](#settings)).
- The rest of the round is an ordinary fetch's.
- A command that takes the sender's own key, a leave or the owner's
  removal of the home of their own record, is answered: the round keeps
  the key it was taken under and sends its summary alone, under that key's
  reply key, counting no item and, as remaining, every item of the
  record's it leaves unsent.

## Screen and buttons

A node has a 200 × 200 e-paper screen, a user button and a reset button
(`bark-fw/src/board.rs`; drawing: `bark-fw/README.md`, Display). The
protocol needs this of them:

| | |
|---|---|
| by default | a QR code of the project's page, `zond.github.io/bark`, and the node's ID under it |
| while a phone pairs | `PAIR CODE` and the passkey, drawn as soon as the pairing asks for it ([pairing](#pairing)), until the pairing ends or is abandoned |
| while a change to the phones waits | `CONFIRM` and its code ([phones](#phones)), at most 30 s |
| after a pairing refused for want of room | `PHONES FULL`, 30 s |
| user button held 3 s | flips the role ([nodes](#nodes)) the moment the 3 s have passed, while still held; the screen says nothing of a forced role |
| a shorter press | flips the picture between the code and the node's state |
| reset button | resets the chip; the node boots the image it ran |

## Capacity and sizing

The figures the sizes above derive from; orders of magnitude, the hearing
radius and the hop count each uncertain by 2×.

| | |
|---|---|
| city model (Kyiv) | ≈840 km², ≈400 km² built-up, ≈3 M people |
| relays | indoor coverage ≈9.6 /km² (≈3,800); outdoor only ≈1.9 /km² (≈750) |
| collision domain | ≈28 km² at a hearing radius of ≈3 km ([physical layer](#physical-layer)): ≈270 relays, ≈14 domains across the built-up city |
| one channel at SF5 | 1.95 kB/s raw; ≈680 B/s with listen-before-talk (≈35% usable) |
| a 100-byte text over 8 hops | ≈0.73 s of channel time: per hop a 129-byte DATA (74.6 ms) and its ACK (17.0 ms) |
| heartbeat load per channel | 270 × 14.4 ms / (10 s × 13) = 3.0% |
| city-wide | 14 × 13 × 0.32 / 0.73 s ≈ 80 texts/s, ≈80 kB/s |
| **design rate** | ≈23 texts a user a day at 300 000 users; with a share f of messages fetched from a [mailbox](#mailbox), 23 / (1 + f) |
| a text in a 100-byte payload | 39 to 52 bytes: the app's encryption and header take 48 to 61 ([the payload](#the-payload)) |
| sleepy nodes' beats | 230 ms an hour each ([beats](#beats)); over a domain's 20 000, 10 % of one channel's time over the 13 |

The per-relay 10% cap ([duty cycle cap](#duty-cycle-cap)) binds first in
the city centre.

## Updates over the mesh

A node given a new firmware image, by a phone or over USB, passes it to
the relays around it, a piece at a time. The protocol carries the pieces;
making them, storing them and checking signatures are the node's firmware
(`core-interface.md`, Application). A node's firmware chooses whose images
it takes.

**The image.** An image is named by the SHA-256 of its signed header, the
80 bytes its signatures cover: magic, format, version, length, the image's
own SHA-256 and that of its block table (`bark-image`, `lib.rs`,
`sign.rs`). A version names one image, and the mesh names an image by its
version. What travels is the image as packed, its 256-byte header and then
its bytes, in **blocks** (`bark-image`, `pieces.rs`):

| block | content |
|---|---|
| 0, the block table | the whole name, the first 8 bytes of the SHA-256 of each other block, then 8 bytes of the SHA-256 of all of that; one piece up to 26 blocks, two up to 48 |
| 1 and on | the packed image in blocks of 64 pieces of 247 bytes, the last block shorter and its last piece padded; block 1 begins with the signed header, and its entry in the table hashes it from the header's end |

Each block is sent as up to 255 coded pieces; any 64 distinct ones of a
block, or as many as it has, give it back (Reed-Solomon over GF(2⁸),
`code.rs`). Blocks decode and fail alone. An image has at most 48 blocks,
the table's among them: 47 blocks of packed image, 743 KB, which
`bark-fw/scripts/image.py pack` enforces.

Every block is checked against its entry in the table, and block 1's
header against the name the table begins with. A block that fails is
gathered again. A node whose blocks fail their check twice logs that two
builds may carry one number.

**The proof.** The header's signatures cover the SHA-256 of the table's
entries of blocks 1 on: not the name, nor the table's own entry. Once a
node holds blocks 0 and 1 its firmware checks (`bark-image`, `pieces.rs`,
`check_proof`) that:

- the header is of the version offered and above its own;
- its length gives the blocks offered;
- a signature verifies under a key it holds;
- its table hash is the table's.

Only then does the node count block 1 held: it holds the image's
**proof**. A proof that fails forgets the image. The firmware checks the
table hash of an image staged from a phone or over USB as it checks the
image's hash, and that of the image it runs before it serves it.

A node **holds** an image's blocks once each has decoded and passed its
hash, and block 1 the proof; it serves those, of an image whose proof it
holds, whether or not it holds the whole image. A block that failed is
never served.

**What a node tells.** A heartbeat may carry a **section** after its first
13 bytes ([packet formats](#packet-formats)), 7.68 ms more: the version
the node runs, 0 if it knows none; the newest image it holds or gathers,
by its version, the signer's key, its blocks and the pieces of its last,
or none; how many of its blocks the node holds from the first; how many
pieces it holds of the block after those, as it stands when sent. A node
that knows neither the version it runs nor an image tells no section.

A relay's heartbeats carry the section in a **rotation of sections**: once
on each channel, at its next heartbeat that goes there. A rotation of
sections starts:

- at boot;
- when the version it runs, its newest image or its blocks held changes;
- an hour after the last;
- in answer to a neighbour that **lags**, one whose section names neither
  an image nor a version run as new as the image the relay streams: at the
  relay's next heartbeat, unless a rotation of sections began within its
  own last rotation of heartbeats.

While a relay gathers, every heartbeat it sends on the home channel of its
**helper** carries the section too (roadmap D158). The helper is the relay
it last took a piece from; before its first piece, the best holder it
hears. A piece does not name its sender: the node takes for it a relay it
hears that holds that block, one at home on its own channel, else the one
that holds the most blocks from the first, then the lowest ID. A sleepy
node's beats carry the section for one beat after a change, in every beat
while it gathers, and in one an hour.

A node keeps each neighbour's last section in its neighbour table
([neighbour table](#neighbour-table)), and the images the sections name,
at most four. A fifth takes the place of the lowest version, and the
sections that named that one name none; one older than the four is kept as
naming none.

**Who sends what.**

1. A section **offers** the image it names only if its sender holds the
   image's proof, and to a sleepy node only if it is its anchor. A node
   that hears an offer of a version above the one it runs and the image it
   holds asks its firmware, once, whether its policy takes the image, and
   if it does gathers it.

   | | |
   |---|---|
   | a gather that has taken a piece within **3 990.9 s** (3 600 s + 2 × 195 s + 872.8 ms; `bark-proto`, `update.rs`, `fed_within`) | no offer replaces it |
   | a gather not fed for that long | an unproven one gives way to an older image, and is then held off (below); a proven one to a newer image |
   | an unproven gather | gives way to a newer image at once |
   | a newer image | taken once the gathered one is installed or rejected |
   | an unproven gather that has taken no piece for **104 866 s** (195 s + 2 × 3 763 × 13.86 s + 2 × 195 s + 872.8 ms; `unproven_within`) | given up; the firmware forgets the pieces |
   | held off | the node takes no offer of a version given up so, left unfed for an older one, or whose proof failed, for 104 866 s |
   | at a reboot | a gather not yet proven is forgotten; one proven is proven again |

2. A node that gathers hands its firmware every piece it has not handed
   before of every block it lacks, and no other: at most 48 blocks of 64.
   When the firmware says a block failed its hash, it takes that block's
   pieces again and does not count it held.
3. A relay **helps** a channel where a neighbour's section says it gathers
   the image the relay holds or has proven and lacks a block the relay
   holds: it **streams** there, at the gap or the pace (below), from the
   blocks it holds, to each gatherer from the first block it lacks.
   - To a sleepy gatherer only as its anchor (roadmap D215).
   - To another gatherer on another channel only if it is the best holder
     it hears for it: no relay it hears that is not at home there, holds
     more than the gatherer and is no farther from it has more blocks from
     the first, or as many and a lower ID. A relay elsewhere does not
     stand back for one at home there (roadmap D196, D200).
   - On at most two channels: its home, then those with the most
     neighbours lacking. It stops on a channel when no neighbour there
     lacks a block it streams there.
   - To a gatherer only on a **lease**: while it heard the gatherer's
     section within the gatherer's last two rotations and a heartbeat's
     lateness ([heartbeat](#heartbeat)), 2 × 13 × T + 872.8 ms, 391 s at
     the longest; for a sleepy gatherer two beat periods and the lateness,
     601 s ([beats](#beats)).
4. A stream **sweeps** the blocks in order. Of the block at hand it sends
   fresh pieces, each block's index climbing from a random start and
   wrapping at 255, until it has sent what the neighbour there furthest
   behind on that block lacks, by its section, over 1 − 20 %
   (`mesh-update-2026-11-07.md`). If that block is the lowest a neighbour
   there lacks, it goes on with fresh pieces of it, at the pace, until the
   neighbour's section shows it complete or its lease ends: the block's
   **tail**. Then it moves to the next block that a neighbour there lacks,
   and after the last starts the sweep again while any lacks something.
5. On its home channel a node that heard k pieces from other nodes since
   its last there sends its next there k + 1 paces after its last; having
   heard none, and on another channel, at the gap or the pace (below). The
   run after a slot heartbeat is the home channel's pace for a rotation
   sent together (sleepy nodes, below).

**Pace and priority.**

| | |
|---|---|
| **gap** | 4.16 s, 30 full frames of 138.6 ms: a node's next piece on a channel comes a gap after its last there if the bucket allows it with the reply reserve kept, its send queue holds nothing else and it has heard no other node's piece on that channel since its own last |
| **pace** | otherwise: at most one piece on a channel in 13.86 s, 100 full frames (roadmap D156) |
| channels | pieces on at most two channels in any one pace |
| behind the send queue | pieces wait behind the frames of the send queue ([send queue](#send-queue)) and go after the sweep's readings, but at most 9 frames of the send queue go before a piece that is due |
| paying | each piece is paid from the bucket as a forward is, leaving the reply reserve ([duty cycle cap](#duty-cycle-cap)) |
| listen-before-talk | as a heartbeat's ([heartbeat](#heartbeat)): up to three checks; if every one is busy the piece waits its pace |
| jitter | each pace, and a wait for the bucket, ends a random time within two frames, 277 ms, later |

**Sleepy nodes.** A sleepy node with an anchor gathers from its anchor
alone and serves nothing; one without an anchor takes no part. Its beats
carry its section, and it takes the pieces it hears in its windows
([delivery](#delivery)) and after its anchor's slot heartbeats.

While one of its sleepy nodes lacks a block the anchor holds, the anchor's
slot heartbeat on its home channel ([relay choice](#relay-choice)) carries
its section every rotation, naming the image it streams them, the one it
holds whole in place of a gather not yet proven, and a **run** of pieces
follows it at once (roadmap D213):

- the run is the pieces of its stream on that channel that the pace gives
  it in one rotation (9 in 130 s, 14 in 195 s), less the pieces it heard
  others send there since its last slot heartbeat, of which it remembers
  8, and one at least;
- while a sleepy node needs the stream there, its pieces go on that
  channel only in runs;
- each piece of a run pays from the bucket and listens before it talks; a
  busy check, or a bucket that cannot pay, ends the run until the next
  slot;
- a run goes before the send queue.

The node listens after its anchor's slot heartbeat while it gathers and
the section in that heartbeat, or the last it heard, says the anchor holds
a block it lacks: 163.6 ms, a full frame and 25 ms of turnaround, and
again after each piece that ends within that time (roadmap D214).

**The install.** A node that holds a gathered image whole checks it and
stages it as the phone's update does
([updates from the phone](#updates-from-the-phone)), noting that it came
over the mesh, and reboots into it once that loses nothing. From then on
it **drains**: it takes no message ([send queue](#send-queue)), while it
has the power a swap needs
([implementation notes](#implementation-notes)) and no phone's update
holds the staged slot; otherwise it takes messages again. While it drains
its heartbeats carry the leaving flag ([packet formats](#packet-formats)),
and Status's `installing` tells its phones the version
([the service](#the-service)), cleared when it takes messages again.

| | |
|---|---|
| it reboots | at the first moment its send queue, delivery queue and inbox are empty, its mailbox has no write under way, and no exchange with a phone is half done: a write not yet answered, a notification not yet sent, or a pairing. It ends a standing link before it resets |
| bound on the drain | the delivery TTL, 600 s |
| waits after the drain or its bound | for its mailbox at most 72 ms, six writes at a 12 ms erase (ZD25WQ16B datasheet, tSE); for a phone at most 30 s (Bluetooth Core Vol 3 Part F §3.3.3, Part H §3.4) |
| what it still holds at the bound | written to flash, each message's ID logged, and put back after the reboot: a message to send on into the send queue with its count and parent ([return to parent](#return-to-parent)); one in the delivery queue or inbox there, with its age and the time away (roadmap D201). Its application's own message, not yet taken by a first hop, is offered again as the application's, ahead of its phone's and its mailbox's |
| kept across the swap | the neighbour table, and the records of the messages active within the delivery TTL except that of its application's own message, every time in them aged by the swap's length ([heartbeat](#heartbeat); roadmap D180) |

At the reboot the loader swaps the image in and starts a watchdog of
600 s, which the image feeds only once it has confirmed itself; a reset
before then swaps the previous image back (roadmap D79).

| | |
|---|---|
| an image a person staged, from the phone or over USB | confirms itself once its core is set up and its radio is up |
| an image gathered over the mesh | confirms itself once the node has heard a frame from any node that decodes: a heartbeat, a DATA, an ACK, a piece or a status |
| none heard within 3 × 195 s + 872.8 ms = 585.9 s of its start | it resets itself, and the loader swaps the previous image back |
| the image back | keeps the failed image's version in a page of flash that no copy over USB erases, and never gathers an image of that version again. The page holds 1 024 versions; when full it is erased and starts again with the newest |

**What a node keeps** for an update: 1 984 bytes beside the tables, and
16 bytes in each neighbour's entry (`core-interface.md`, Memory).

## What the protocol omits

| Omitted | Instead |
|---|---|
| Neighbour lists in heartbeats, a network-wide node table | a 13-byte heartbeat; routing uses only direct neighbours |
| Addressing by ID with a lookup, or registration with a home relay | position + node ID; the owner resolves the ID locally ([owner](#owner)) |
| Time slots on a shared schedule | always listen, transmit once listen-before-talk clears |
| Cooperative position estimation | GPS or a map point ([position](#position)) |
| A fixed channel grid, heartbeats on all channels | a measured home channel and a rotating heartbeat ([home channel selection](#home-channel-selection)) |
| GFSK at 50 kbps | LoRa at SF5 ([physical layer](#physical-layer)) |
| Spreading one signal over the whole band | 13 separate channels |
| Frequency hopping | measured home channels |
| Sleepy nodes heartbeating as relays do | a beat every 150–300 s ([beats](#beats)) |

**No authentication.** Nothing on the air is authenticated. Any node in
radio range can forge any frame, within its duty cycle:

- refuse in a relay's name and ban it for up to 89 min ([bans](#bans));
- take a message in a relay's name, and the message is lost;
- send a heartbeat in an anchor's name and send its sleepy nodes elsewhere
  ([relay choice](#relay-choice));
- claim the position of a destination and own it ([owner](#owner));
- name in its section an image nobody holds, which its hearers gather
  until a real offer may replace it or they give it up
  ([updates over the mesh](#updates-over-the-mesh)).

The app's end-to-end encryption keeps payloads private and authentic
([the payload](#the-payload)); it does nothing for routing. A listener
near a home node can tie it, by timing, to the address its mail goes on to
([mailbox](#mailbox)).

**No location privacy.** Anyone in radio range learns every node's ID and
position from its heartbeats.

## Packet formats

Every packet begins with two bytes: its kind with that kind's one small
field, then the format version. A heartbeat's third byte holds its flags.

| first byte | packet | the low bits |
|---|---|---|
| `1000 cccc` | heartbeat | `cccc` the home channel, 0 to 12 |
| `00aa aaaa` | DATA | `aux`: 0 to 12, a message, and the sender's home channel; 13 to 63, a frame from node to node of kind `aux` |
| `s1aa aaaa` | ACK | `aux`, 0 if the offer is taken or seen, else the code of the wait the refuser asks, 1 to 63 ([bans](#bans)); `s` the seen flag ([the ACK](#the-ack)), with `aux` 0 (roadmap D76) |

The second byte is the **format version**. This specification is format 1.

| packet | the second byte |
|---|---|
| heartbeat | the newest format its sender speaks |
| DATA, ACK | the format of this frame |

A node speaks every format from 1 to its newest. A heartbeat's first 13
bytes are the same in every format; a later format appends after them. A
node reads the first 13 bytes of every heartbeat whatever its second byte
says, ignores what follows that it does not know, and keeps the sender's
newest format in its neighbour table ([neighbour table](#neighbour-table)).
A heartbeat shorter than 13 bytes is rejected. Which format a DATA and its
ACK take: [hand-over](#hand-over). Any change to what a field of DATA or
the ACK means, not only to where it is, takes a new format.

A heartbeat's third byte is its **flags**, each set on its own:

| bit | flag | meaning |
|---|---|---|
| 0 | sleepy | reachable in its window after its beat, or at any time with the long preamble ([nodes](#nodes)) |
| 1 | anchorless | the sender knows no node that holds mail. Set by a sleepy node without an anchor ([sleepy nodes alone](#sleepy-nodes-alone)), and by a relay on the heartbeats of its burst, which are in no slot ([heartbeat](#heartbeat)) |
| 2 | carried | the sender moves and holds mail for nobody ([nodes](#nodes)) |
| 3 | map point | its position is a map point set in the app, not a GPS fix ([position](#position)); set while that is so, kept across a reboot; no rule uses it |
| 4 | leaving | set while the node drains before a reboot into a new image ([updates over the mesh](#updates-over-the-mesh)) and while it flushes what it holds to turn sleepy ([sleepy nodes](#sleepy-nodes)); a sender goes round it while another way is left ([next hop](#next-hop); roadmap D181) |
| 5–7 | | zero; a later format may define them |

A receiver ignores the flag bits it does not know.

**Frames from node to node.** A DATA whose `aux` is 13 to 63 is not a
message: one hop, never routed, unencrypted, and unanswered unless its
kind says otherwise:

| `aux` | kind |
|---|---|
| 15 | an update's piece ([updates over the mesh](#updates-over-the-mesh)) |
| 16 | a neighbour's status asked ([neighbour status](#neighbour-status)) |
| 17 | a neighbour's status told, the answer to 16 |
| 13, 14 and 18 to 63 | reserved |

Such a frame keeps the first two bytes and lays out the rest for its kind.
It does not carry the sender's home channel in `aux`: a kind that expects
an answer finds it in the neighbour table or carries it in a byte of its
own.

**The heartbeat's section.** In format 1 a heartbeat is 13 bytes, or 26
with an update's section after the first 13
([updates over the mesh](#updates-over-the-mesh)). A later format that
appends more puts it after the section, and sends the section with an
image version of 0 when it has nothing to say.

**Rejected:** a heartbeat whose first byte has bit 4 or 5 set or whose
channel is 13 to 15; an ACK with the seen flag and a wait; a DATA whose
`aux` is a reserved kind; a DATA, ACK or frame from node to node in a
format the node does not speak; a DATA count in more bytes than it needs,
its first group 0, or with a third byte whose top bit is set. A heartbeat
byte with channel 15 starts the bench frames of the firmware's measurement
console (`bark-fw/README.md`).

A position is 7 bytes, the heartbeat's short position 4
([positions on the wire](#positions-on-the-wire)), a node ID 6
([node identity](#node-identity)). Multi-byte numbers are big-endian. The
LoRa hardware CRC covers everything.

```
HEARTBEAT first(1) format(1) flags(1) id(6) short_pos(4)                    = 13
          optional section, after the first 13:
          runs(4) version(4) key(1) blocks(1) last(1) complete(1)
          have(1)                                                           = 26
          runs = the version the node runs, 0 for none told;
          version = the newest image it holds or gathers, 0 for none;
          key = which signer's key signed it; blocks = its blocks, 2 to 48;
          last = the pieces of its last block, 1 to 64; complete = the
          blocks the node holds from the first, 2 or more when it offers
          the image; have = the pieces it holds of block `complete`
ACK       first(1) format(1) id(4) from(6) to(6)                            = 18
          the answer to a DATA, taken, refused or seen; sent on the channel
          in the answered packet's aux
          id = the message ID; from, to = the IDs of the answering and the
          answered node
DATA      first(1) format(1) dst_hop(6) src_hop(6) dst_pos(7) dst_id(6)
          wrong(1) hops(1 to 3) payload                                     = 29 + payload,
                                                                              30 from 128 hops,
                                                                              31 from 16 384
          dst_hop, src_hop = IDs of the receiving and the sending node;
          aux = sender's home channel, where the ACK goes;
          wrong, hops = the counts of [hop count](#hop-count); hops is the
          count in groups of 7 bits, the most significant first, one a
          byte, each byte's top bit set if another follows: one byte below
          128, two below 16 384, three up to 2 097 151
PIECE     first(1) format(1) version(4) block(1) index(1) piece(247)        = 255
          aux = 15; version = the image's; index = which coded piece of
          the block, 0 to 254
ASK       first(1) format(1) to(6) from(6)                                  = 14
          aux = 16; to, from = the IDs of the node asked and the node asking
STATUS    first(1) format(1) to(6) from(6) flags(1) battery(1) heard(1)
          strong(1) proven(1) gps(1) version(4) gathers(4) blocks(1)
          complete(1) have(1)                                               = 31
          aux = 17; to, from = the IDs of the node that asked and the node
          answering; the rest as [neighbour status](#neighbour-status) says
```

The **message ID** is a 32-bit FNV-1a hash of
`dst_pos || dst_id || payload`, computed by every node that handles the
message. DATA does not carry it; the ACK does. The app must keep payloads
unique ([the payload](#the-payload)).

The payload is at most 255 − 31 = 224 bytes.

### Positions on the wire

A position is WGS-84 latitude and longitude in fixed point, 25 + 26 bits
and 5 spare in 7 bytes; one count is 180° / 2²⁵ ≈ 0.6 m.

**Local plane.** Each node converts every position it handles into metres
on a flat plane centred on itself: `Δy = Δlat · R`,
`Δx = Δlon · R · cos(lat_self)`. Error at 50° N: 0.1 % at 5 km, 0.4 % at
20 km.

**Short position.** The heartbeat carries a node's position in 4 bytes:
the low 16 bits of its latitude count, then the low 16 bits of its
longitude count, big-endian. A receiver rebuilds the full position as the
one candidate within half a period of its own on each axis, a sleepy
node's anchor's while it has one ([beats](#beats)). The period is 65 536
counts: 39 km of latitude, 39 km × cos(latitude) of longitude (25 km at
50° N). DATA `dst_pos` is the full 7 bytes (roadmap D14).

## Implementation notes

**Rust stack:** `embassy-nrf` (nRF52840); the SX1262 driven by
`bark-driver`'s sequencer, its commands encoded by a vendored copy of
`lora-phy` (roadmap D4, D88); TrouBLE with the Nordic controller for BLE
(D5); the stock UF2 bootloader for flashing (D6), and a loader of our own
behind it that swaps in a new image and back if it does not confirm itself
(D79). The protocol core does no I/O of its own and owns its routing and
tables (D1, D3; `core-interface.md`). Crate layout: D7.

**Updates.** A node on battery whose last reading is at or below the low
level ([sleepy nodes](#sleepy-nodes)) starts no swap to a new image; on
external power nothing limits it (roadmap D143). A node stages only an
image above its own version and signed by a key it was built with, the
project's and, on a bench build, the bench key (roadmap D144, D154).

**RAM (256 KiB):** every board holds a relay's tables, the heard table and
the delivery queue included (roadmap D71); sizes: `core-interface.md`,
Memory; `bark-image/src/layout.rs`.

**Stored state.** A node keeps in flash, written only when it changes, and
comes back with ([channel at boot](#channel-at-boot)):

| | |
|---|---|
| home channel | a relay keeps it and makes no choice at the end of its first rotation; a carried sleepy node keeps the one it had when last fixed |
| the place a fixed node stands | its position after the reboot, and whether it is a map point ([position](#position)) |
| carried flag | kept; still since the boot |
| a fixed sleepy node's anchor, ID and position | taken at once ([relay choice](#relay-choice)) |
| whether it is a relay | a fixed relay that boots on battery relays on, its grace counted from the boot ([sleepy nodes](#sleepy-nodes)) |
| its battery readings' scale | from its last calibration ([sleepy nodes](#sleepy-nodes)) |
| the drains it learned on battery | a relay's and a sleepy node's, each in steps of 2.2 % of its first, up to 16 times it either way ([sleepy nodes](#sleepy-nodes)); one write a sample at most |
| record | 32 bytes with a checksum, appended to one of two 4 KiB pages of flash of 128 slots (`bark-fw/logic/src/store.rs`) |

Not kept: any timer; a carried node's place; a carried sleepy node's
anchor; the noise floor.

The same pages keep the phone link's state, in entries of one record or
four of their own: each phone's record ([pairing](#pairing)), the last
message its phone confirmed and the last fetch it took
([mailbox](#mailbox)), the public setting ([public nodes](#public-nodes))
and the UTC offset ([settings](#settings)). The last whole entry of each
kind holds; an entry spans its slots, whole or not.

Pages: a page's first slot is its head, the layout (3) and a 16-bit
generation, and is written last; a page whose head is whole and of
layout 3 is valid. At
boot the valid page of the later generation holds, compared modulo 2^16.
On a full page: erase the other page, write the latest entry of each
kind into it from its second slot, then its head, generation + 1; the
full page stays until the next such turn. No valid page, and the second
page of layout 2 (the same entries on that page alone): its entries are
read, and the first write turns to the first page. Otherwise, no valid
page: nothing is kept, and the first write turns to the first page
(roadmap D188, D241).

The bring-up order and the test rig: `roadmap.md`, Phase 2.

## Open items

Resolved items keep their numbers and are in `roadmap.md`. The steps are
those of the bring-up order, `roadmap.md` Phase 2.

| # | Item | Owner/when |
|---|---|---|
| O3 | Exact thresholds: busy threshold margin (6 dB), move hysteresis (0.05 / 10 min), demote count, ACK timeout | tune in steps 3–4 |
| O5 | Adjacent-channel rejection of the SX1262 at 125 kHz spacing, with a neighbour at full power; if inadequate, 11 channels at 150 kHz spacing, the same end centres, 433.170–434.670 MHz ([channel plan](#channel-plan)) | step 1 |
| O7 | Sending to an area (address = position + radius, node ID = broadcast) | later |
| O11 | Relay-side RSSI noise-floor calibration method and drift | step 3 |
| O12 | Confirm RAK WisMesh Repeater 433 availability for rooftop solar units | procurement |
| O16 | GPS wander of a still relay indoors: how far a still node's fixes lie from one of its own for 72 s ([position](#position)); the bench so far is in `docs/gps-bench-2026-11-06.md` | step 2 bench, roadmap D70, D136 |
| O18 | The lowest mean C/N0 of a fix that is taken, 22 dB-Hz by reasoning and by fixes indoors ([position](#position), `docs/gps-bench-2026-11-20.md`); what the L76K reads under an open sky, and what a spoofer's satellites show, are not yet measured | a node under an open sky, roadmap D74, D234 |
| O19 | A sleepy node carried farther than half a short position's period, 19.5 km north-south and less east-west, while it hears no relay: it rebuilds its next anchor's position a period off, and the neighbour check drops the fix that would correct it ([beats](#beats), [position](#position)) | later, roadmap D91 |
| O24 | The cell's curve in the cold: a discharge bench run at 0 °C or below, and one at a relay's current, against the curve at room temperature ([sleepy nodes](#sleepy-nodes)); one cell, one run so far | bench, roadmap D210 |

## Sources

- ERC/REC 70-03, Annex 1 (non-specific SRDs), band g1 433.050–434.790 MHz
  (edition of June 2026; earlier editions label it f1).
- NCEC (Ukraine) decision 81 of 12.02.2025 "generalised conditions", row
  РІ 50-3-1, as amended by decision 212 of 06.05.2026. Consultation
  (Aug–Sep 2026) on 865–868 MHz does not affect 433 MHz.
- EN 300 220-2 for polite-access (listen-before-talk) shapes (not required
  in this band).
- Semtech SX1261/2 datasheet: LoRa sensitivity table, CAD, RSSI, SF5/SF6.
- Bluetooth Core Specification 5.0 (Bluetooth SIG, 2016): the ATT MTU,
  writes, reads and errors (Vol 3 Part F), the L2CAP header (Vol 3
  Part A), UUIDs (Vol 3 Part B), advertising, the link-layer packet and
  its timing (Vol 6 Part B), attribute caching and Service Changed
  (Vol 3 Part G).
- Android API reference, `BluetoothGatt.requestConnectionPriority`: the
  connection interval of `CONNECTION_PRIORITY_HIGH`, 11.25 to 15 ms, and
  of `CONNECTION_PRIORITY_BALANCED`, 30 to 50 ms.
- Nordic SoftDevice Controller, `sdc_hci_vs.h`: a connection event of
  7.5 ms by default.
- Bluetooth Core Specification 5.1 (Bluetooth SIG, 2019): the database
  hash (Vol 3 Part G §7.3).
- libsodium documentation: "Public-key authenticated encryption"
  (`crypto_box`, `crypto_box_beforenm`), "Sealed boxes", "Secret-key
  authenticated encryption" (`crypto_secretbox`, its detached mode),
  "Key derivation" (`crypto_kdf`) and "Generic hashing"
  (`crypto_generichash`).
- RFC 4648, "The Base16, Base32, and Base64 Data Encodings": base32.
- FIPS 180-4, Secure Hash Standard: SHA-256, the backup's checksum and
  an image's hash and name.
- RFC 8032, "Edwards-Curve Digital Signature Algorithm (EdDSA)":
  Ed25519, an image's signature.
- Nordic nRF52840 Product Specification: the radio's transmit and
  receive currents.
- NDK NX2012SE 32.768 kHz crystal datasheet (MU01260): tolerance at
  25 °C, turnover temperature and parabolic coefficient.
- Apple, Accessory Design Guidelines for Apple Devices, "Advertising
  Interval", and Technical Q&A QA1931: the advertising intervals an
  iPhone finds quickly.

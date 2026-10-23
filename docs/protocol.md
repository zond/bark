# bark protocol: Civilian 433 MHz Mesh

**Status:** live specification, 2026-10-22. Decisions that refine it are in
`roadmap.md` with D-numbers; [open items](#open-items) lists what is still to be decided or
measured. Where the code and this text disagree, one of them is wrong and
the fix is a commit to both.

A mesh network of cheap radio nodes, legal without a licence in the EU and
Ukraine, that lets phone apps message each other across a city when
ordinary infrastructure is gone. It is **not** designed to survive
deliberate jamming; it is designed for infrastructure loss.

---

## Summary

| Item | Decision |
|---|---|
| Hardware | LILYGO T-Echo, 433 MHz variant (nRF52840 + SX1262 + L76K GPS + e-ink + BLE 5.0 + 850 mAh), one firmware for every node |
| Band | 433.050–434.790 MHz, 10 mW effective radiated power (ERP), ≤10% duty cycle per node (legal in the EU and Ukraine) |
| Modulation | LoRa, spreading factor (SF) 5, 125 kHz, coding rate 4/5, fixed network-wide |
| Channels | 13 × 125 kHz; each node picks its **home channel**, the one it listens on, by measuring how busy each channel is; no fixed plan, no time sync |
| Medium access | Listen on the home channel except to transmit (a sleepy node receives duty-cycled between its windows); acknowledgements (ACKs) come back on the sender's home channel; listen-before-talk, then transmit at once; no time slots |
| Heartbeat | A node's periodic 9-byte broadcast of its ID, position and home channel: a relay's every 5–15 s, fixed by its ID, on each of the 13 channels in turn; a sleepy node's every 150–300 s on its home channel; an anchorless node's every 23–46 s, with a long preamble, on each channel in turn |
| Neighbour discovery | Passive: a node hears every relay in range once per 13 heartbeats (~130 s) on its home channel; no scanning, no schedules |
| Routing | Forward to the neighbour closest to the destination **position**; route around gaps by depth-first search; ACK every hop; a neighbour that refuses or fails to answer is banned for a while, and after two failures in a row is a last resort; sleepy nodes forward only where no relay is in reach |
| Addressing | A position plus a 4-byte node ID: a relay's own position, a sleepy node's anchor's; no registration, no network-wide tables |
| Nodes | One kind, with two properties it measures of itself, each a heartbeat flag: sleepy on battery, carried when it moves; a relay is a node that is not sleepy ([nodes](#nodes)) |
| Phones | Bluetooth LE (BLE) GATT link to the node; the app encrypts end to end; the sender's position travels encrypted in the payload |
| Position | From GPS unless a fix fails the checks or the app sets a map point; WGS-84 in 7 bytes (≈0.6 m), the same everywhere on Earth; distances in a flat local plane at each node |
| Time | No network time, and GPS time is not used |

---

## Hardware

**LILYGO T-Echo, 433 MHz build.** nRF52840 (Nordic, Norway), SX1262
(Semtech, US, EAR99), Quectel L76K GNSS, 1.54" e-ink, BLE 5.0, 850 mAh
Li-ion, USB-C charging, UF2 bootloader. About $54 direct, ~€116 from EU
stock. Check that the LoRa module is the 433 MHz build, not the 868/915
build: the 868 board's antenna matching makes 433 MHz operation lossy and
out of spec.

Same chipset, same firmware, for rooftop relays:

- **RAK WisBlock** RAK4631 EU433 (PN 111000) + RAK19003/19007 base +
  RAK12500 GPS, in a solar enclosure.
- **RAK WisMesh Repeater Mini V2** (solar + 3200 mAh), if a 433 MHz build
  is available (O12).

The SX1262 is not European. No European part offers nRF52840 + SX1262
integration off the shelf at this price, and the premise is that anyone can
buy and plant a node. ST's STM32WL55 is the European fallback but has no
BLE.

---

## Nodes

Every node runs the same protocol and firmware. What it does is not a
setting: it follows two properties the node measures of itself, each on
its own, and its heartbeat publishes each as a flag
([packet formats](#packet-formats); roadmap D70, D71, D72).

- **Sleepy**: it is on battery, past a grace of an hour
  ([sleepy nodes](#sleepy-nodes)). On external power, USB on a
  windowsill or a solar box on a roof, a node is not sleepy. The
  nRF52840 detects USB power itself, so a person's node relays on its
  charger and sleeps in a pocket, with nothing to set. Sleepy decides
  how the node uses its radio: it beats rarely instead of heartbeating,
  listens only in a window after each beat, receives duty-cycled
  between, forwards what it is given at once, anchors to a fixed relay,
  and keeps no mailbox.
- **Carried**: its GPS says it moves ([position](#position)). A map
  point set in the app makes it not carried at once. Carried decides how
  the node uses its GPS: it takes each fix as its position, as a sleepy
  node does too, so that its heartbeats say where it is and its
  neighbours route to it there. A node that is neither keeps the place
  it stands as its position, so that no later fix, true or spoofed,
  moves it (roadmap D63). A carried relay is otherwise like any other.

The two combine in four ways, all of them real, and each flag means the
same whatever the other is:

| | not carried | carried |
|---|---|---|
| **not sleepy** | a node on USB at a window | a node on USB in a car |
| **sleepy** | a node on battery left somewhere | a node on battery in a pocket |

Holding mail for sleepy nodes, owning a destination by its position
([owner](#owner)) and being a sleepy node's anchor
([relay choice](#relay-choice)) need neither flag; each flag takes all
three away. Mail is held only where a sleepy node can count on finding
it. A carried node would leave its sleepy nodes behind, and it has no
position at which anyone can address it for long, so its senders learn
where it is at the app layer ([addresses](#addresses)); as an anchor it
would carry its sleepy nodes' addresses off with it. A node that holds
no mail owns only what is addressed to its ID, since owning a position
means holding the mail addressed to it.

The rest of this document uses these words:

- A **relay** is a node that is not sleepy, carried or not: it listens
  all the time, heartbeats every 5–15 s on every channel, and forwards.
  A **sleepy node** is one that is sleepy, and a **fixed** node one that
  is not carried. Like "anchor" and "holder", these words name a state a node
  is in, not a kind of node.
- A sleepy node's **anchor** is the relay it chooses, a fixed one. The
  anchor's position is the sleepy node's address, its home channel the
  sleepy node's, and its heartbeat the one the sleepy node watches to
  know it is still in reach. A sleepy node that hears no fixed relay is
  **anchorless** ([sleepy nodes alone](#sleepy-nodes-alone)).
- A **beat** is a sleepy node's heartbeat, and its **window** the few
  seconds after a beat, or after a message it takes, in which it
  listens.
- A **holder** is a relay that holds a message for a sleepy node and
  hears its beat; most often its anchor.
- A relay's **mailbox** is its heard table and its delivery queue: the
  sleepy nodes it hears and the messages it holds for them.
- The **long preamble** reaches a node that receives duty-cycled at any
  time; it is used only where nothing else reaches.
- A sleepy node goes **dark** when it is out of reach for minutes: a
  pocket, a basement or the metro. The network keeps what it holds for a
  node, and a message on its way, for the **delivery TTL**, 600 s.

---

## Regulatory limits

Both regimes allow any modulation in this band.

| | EU (ERC/REC 70-03, Annex 1, band g1; "f1" in editions before 2026) | Ukraine (NCEC decision 81 of 12.02.2025, row РІ 50-3-1, as amended by decision 212 of 06.05.2026) |
|---|---|---|
| Frequency | 433.050–434.790 MHz | 433.04–434.79 MHz |
| Power | 10 mW ERP | 10 mW ERP |
| Duty cycle | ≤10% | <10% |
| Channel spacing | no requirement | no requirement |
| Listen-before-talk | not required | not required |

The protocol keeps to them with an airtime token bucket per node that
holds every hour within 10% ([duty cycle cap](#duty-cycle-cap)), no
transmission longer than 1 s (an SF5 packet is at most 139 ms, 493 ms
with the long preamble, [physical layer](#physical-layer)), and +10 dBm
at the connector, which with the stock antenna (≈0 dBd gain) is at most
+10 dBm ERP.

The 868 MHz band does not work for Ukraine (no 500 mW sub-band; 868.0–868.6
MHz is 14 dBm at <1%), and 2.4 GHz has too little range. 433 MHz is the
only band with identical rules in both jurisdictions.

---

## Physical layer

RSSI (received signal strength) is the received power the radio reports.
CAD (channel activity detection) is the radio's check for a LoRa preamble
on the channel; it works below the noise floor.

| Parameter | Value | Note |
|---|---|---|
| Modulation | LoRa (chirp spread spectrum) | SX1262 native; `lora-phy` Rust driver exists |
| Spreading factor | 5, fixed network-wide | SF5/SF6 are SX126x-only; all nodes are SX126x. Compile-time constant, changeable only for bench work |
| Bandwidth | 125 kHz | |
| Coding rate | 4/5 | |
| Raw bit rate | 15.6 kbps at SF5 | 5.5 kbps at SF7 |
| Symbol time | 0.256 ms at SF5 | |
| Preamble | 12 symbols (3.1 ms) + 6.25 symbols of sync; **long only to and from a node that receives duty-cycled**: 1396 symbols (357.4 ms) | 12 is the SX1262 minimum at SF5 and SF6 (datasheet §6.1.4); the long one is derived below |
| 255-byte packet | 138.6 ms at SF5 | ≈400 ms at SF7. Airtime from the datasheet formula (§6.1.4), `time_on_air` in `radio.rs` |
| Sensitivity | ≈−120 dBm at SF5 | ≈−124 dBm at SF7 |
| Noise floor (125 kHz, noise figure ≈5 dB) | ≈−118 dBm | SF5 signals near sensitivity sit below the floor: RSSI cannot see them, CAD can |
| Sync word | private (not the LoRaWAN 0x34) | other LoRa networks and this one ignore each other's packets |
| CRC | on (LoRa hardware CRC) | |
| Maximum packet (MTU) | 255 bytes | SX126x maximum LoRa payload |
| Range, relay↔handheld, street | ≈750 m at SF5, ≈1 km at SF7 | |
| Range, relay↔handheld, indoors | ≈300 m at SF5, ≈400 m at SF7 | |
| Range, rooftop↔rooftop | several km | rooftop relays hear ≈3 km; this sets the size of a collision domain ([capacity and sizing](#capacity-and-sizing)) |

SF5 carries about 3× the bytes per unit of airtime of SF7 (the common LoRa
default) for a quarter less range. LoRa brings an existing driver, hardware
error correction, CRC and whitening, CAD, and ~15 dB of processing gain
against the key fobs and LPD433 handhelds that share the band. Why not
GFSK: [what the protocol omits](#what-the-protocol-omits).

SF is network-wide because an SX1262 receives one SF at a time, so a
mixed-SF network splits into parts that cannot hear each other. Starting
at SF7 would force a network-wide switch later; the deployment is
expected to start at a density where SF5 range suffices
([capacity and sizing](#capacity-and-sizing)).

### Duty-cycled receive and the long preamble

Every frame has the shortest preamble except those to a node whose radio
receives duty-cycled: a sleepy node outside its windows
([sleepy nodes](#sleepy-nodes)), which without an anchor is always
([sleepy nodes alone](#sleepy-nodes-alone)). The SX1262 does this on its
own (datasheet §13.1.7, SetRxDutyCycle). It receives for a window `rx`,
takes about 1 ms to save and restore its state, sleeps for `sleep`,
starts its oscillator (the TCXO, 5 ms as the firmware sets it), and
receives again: a cycle `C = rx + sleep + wake`, with `wake` = 6 ms.

It catches a preamble only if a window holds `D` of it, the time the
modem takes to lock, taken here as 4 symbols (≈1 ms; measured in
bring-up step 1, `roadmap.md` Phase 2). A
preamble that starts just too late for one window must reach `D` into
the next, so it is caught wherever it starts if it is at least
`sleep + wake + 2D` long. Once it has detected a preamble, the radio
waits `2 rx + sleep` for the header (datasheet §13.1.7: "Tpreamble +
Theader ≤ 2 × rxPeriod + sleepPeriod"), which ends 14.25 symbols after
the preamble. So the window must be at least `(wake + 2D + header) / 2`,
and half a symbol more to round the preamble up to whole symbols:
`rx` = 6.0 ms, in the radio's steps of 15.625 µs.

The sleep follows from the current. The node pays `I_rx (rx + wake) / C`
for its windows, at the 5.8 mA of listening, and `I_tx C / P` for its
own beats, each a preamble about `C` long every `P`, at 32 mA (datasheet
Table 3-6, the least listed for the SX1262 at 434 MHz). The sum is least
where the two are equal, `C = √(I_rx (rx + wake) P / I_tx)`: 361.1 ms at
`P` = 60 s, the beat interval the cycle was first set for, and within
7 % of that from 250 to 520 ms. Every sleepy node has the same cycle.
Without an anchor its beats come every 34.6 s
([sleepy nodes alone](#sleepy-nodes-alone)), where the least is 274 ms
and 361.1 ms costs 4 % more; with one they come every 225 s
([beats](#beats)), where the windows dominate whatever the cycle. So:

| Quantity | Value |
|---|---|
| Window `rx` | 6.0 ms (384 steps) |
| Sleep | 349.1 ms (22 344 steps) |
| Cycle | 361.1 ms |
| Long preamble | 1396 symbols, 357.4 ms |
| Mean current while receiving duty-cycled | 0.194 mA (the sleep at 1.2 µA, Table 3-5) |
| Heartbeat with the long preamble | 366.1 ms |
| 255-byte frame with the long preamble | 492.9 ms, under the 1 s of [regulatory limits](#regulatory-limits) |

A long frame costs its sender and its channel 30 times a heartbeat's
airtime and 3.6 times a full frame's. So ordinary traffic keeps the
shortest preamble: every node on a relay's channel would pay the long
one on every frame, and a relay forwards at the 10 % cap long before its
channel fills ([duty cycle cap](#duty-cycle-cap)). Long frames go only
where the receiver cannot be reached otherwise: DATA to a sleepy node
outside its window, the beats of a node without an anchor and a relay's
answers to them, and a sleepy node's beats to its anchorless neighbours
([beats](#beats)). Their cost is in
[sleepy nodes alone](#sleepy-nodes-alone).

---

## Channel plan

Thirteen channels of 125 kHz, with 57.5 kHz of guard at each band edge:

```
f_k = 433.170 MHz + k × 0.125 MHz,   k = 0..12
      433.170, 433.295, 433.420, 433.545, 433.670, 433.795, 433.920,
      434.045, 434.170, 434.295, 434.420, 434.545, 434.670 MHz
```

No fixed grid assigns channels, and no position derives them: each node
measures how busy each channel is and chooses
([home channel selection](#home-channel-selection)); why not a grid:
[what the protocol omits](#what-the-protocol-omits). Channel 6 is
centred on 433.920 MHz, the busiest spot in the band (key fobs,
sensors); the choice avoids it wherever it is busy. Whether 125 kHz
spacing is enough is open item O5 ([open items](#open-items)).

---

## Home channel selection

Every node has one **home channel**, on which it listens whenever it is
not transmitting. All traffic *to* a node is sent on its home channel.

### Busy estimate

Per channel, a node keeps `busy[c]`, an exponential moving average of a
0/1 busy reading:

- **Sweep:** one RSSI reading per channel per second, spread evenly, one
  every 1 s / 13 ≈ 77 ms, channels in turn, each taken only when the
  modem reports no preamble detected and no packet in progress. A
  reading is a retune (≈0.1 ms), a read (≈0.3 ms after it) and a retune
  home, ≈0.5 ms: ≈6.5 ms a second, 0.65% of the time off the home
  channel.
- **Cost:** spread readings lose no packet. The modem needs a length `D`
  of preamble to lock on
  ([duty-cycled receive and the long preamble](#duty-cycled-receive-and-the-long-preamble)),
  so a gap of length `g` loses a packet whose preamble starts too late
  before it for the modem to have locked, or too early in it to leave
  `D` after it: a window of `g + 2D − P`, with `P` the 12-symbol
  preamble, 3.1 ms ([physical layer](#physical-layer)). For one spread
  reading 0.5 + 2.0 − 3.1 < 0. One burst of all 13 readings, ≈5 ms,
  would lose 5 + 2.0 − 3.1 ≈ 4 ms of arrivals a second, 0.4%. Spreading
  loses nothing as long as `D ≤ (P − g) / 2` ≈ 1.3 ms, 5 symbols; if the
  bench shows the modem needs more, redo this comparison.
- **Reading:** 1 if RSSI > noise floor + 6 dB (≈−112 dBm), else 0. The
  average is of the 0/1 reading, not of the dBm: what matters is the
  fraction of time listen-before-talk would refuse.
- **Window:** α ≈ 1/600 per sample, ≈10 minutes at one sample per
  channel per second. One sample per heartbeat interval would give 60
  samples per window, ±4 points on a 10% channel, as large as the
  differences the rule acts on.
- **Home channel:** count only what would stay behind if the node moved.
  DATA addressed to the node, the ACKs of its own hand-overs, and its
  own transmissions go wherever it goes: counted, they would make a busy
  relay leave its channel for its own traffic and take that traffic
  along. So a home reading taken while one of them was on the air is set
  aside, and every other is the same 0/1 reading as on any other
  channel. The move rule compares home with the others, so all are read
  alike (roadmap D55).
- **Home readings and the node's own packets:** whether a reading fell in
  the node's own packet is known only when that packet ends, so a home
  reading is held until the next, one second later, longer than any
  packet (roadmap D24). The radio keeps receiving while it reads its own
  channel (`core-interface.md`, Radio, rules 7 and 8): a home reading
  inside a packet addressed to the node is discarded, one inside any
  other counts. A packet for the node that fails its CRC cannot be told
  from someone else's, and counts.
- **Noise floor:** calibrated per device at boot as a low percentile of
  RSSI readings over the first rotation, and recalibrated slowly.

Foreign transmitters (key fobs, LPD433 handhelds) count exactly like
peers. What hurts a node is busy time at its own receiver, which RSSI
measures directly; counting neighbours per channel would not, since two
neighbours that hear each other share a channel cleanly.

### Channel at boot

A node listens on a random channel for a worst-case rotation
([heartbeat](#heartbeat)), so that every relay in range has been heard
once, collecting sweeps and heartbeats. Then it sets home to
`argmin busy[c]`. Channels within 0.01 of the lowest count as tied, and
the choice among them is random, so that nodes that hear nothing do not
all pick channel 0. A config override exists for bench work.

A sleepy node listens two rotations, 390 s: a relay switched on with it
heartbeats only after a boot listen of its own, and once the node
receives duty-cycled it hears relays only by their answers (roadmap D51,
D52). It ends the listen at the first relay it hears, takes it as its
anchor and moves to the anchor's channel; one that hears none homes as
a relay does and keeps that channel ([relay choice](#relay-choice),
[sleepy nodes alone](#sleepy-nodes-alone)).

### Channel moves

A node moves only when `busy[home] − min_c busy[c] > 0.05` continuously
for 10 minutes; then it waits a random 0–5 minutes, checks again, and
moves to `argmin` with random tie-break. The hysteresis keeps moves
rare: without it, every rooftop relay in range of the same interference
sees the same ranking, and they all jump together.

Immediately after retuning it sends **one heartbeat on the old
channel**, which names the new one and retunes at once any neighbour
parked on the old channel. A sleepy node listening at the time, in a
window ([delivery](#delivery)), hears it and follows at once, so the
moving node sends to the sleepy nodes of its heard table on the old
channel that now listen on the new one ([heard table](#heard-table)).
A sleeping node anchored to it follows at the next heartbeat it wakes
for ([relay choice](#relay-choice)) and beats on the new channel soon
after, which tells the holder where it listens ([beats](#beats),
roadmap D53). There is no other announcement: neighbours that miss it
learn the new channel from the next rotation ([heartbeat](#heartbeat))
or from ACK failure ([hand-over](#hand-over)).

---

## Position

A node's position comes from its GPS (L76K), unless the fix is
unreliable or the app sets a map point, which wins over GPS; a rooftop
relay with no sky view of its own uses one. A fix is unreliable if it is
farther from the last accepted fix than the node can have moved since,
if it is inconsistent with the neighbours heard, or if the satellite
signal-to-noise ratio (C/N0) is not sane: each is a sign of jamming or
spoofing. Two fixes of a node that has not moved may differ by 50 m, the
allowance the [owner](#owner) rule and the move check of
[relay choice](#relay-choice) use. A node whose position changes
heartbeats the new one ([node identity](#node-identity)).

**Fixed and carried.** Every node measures from its fixes whether it is
carried, the same way whether it is sleepy or not. A fixed node's fixes
are compared with the place it stands: its first accepted fix, its map
point, or the fix it had when it was last made fixed. An accepted fix
farther than the allowance from that place is a **far fix**. After far
fixes for 72 s with none within the allowance between, the node is
carried. 72 s is how long a walker at 1.4 m/s, a common walking pace,
takes to cross the allowance's whole width, 2 × 50 m, in whole fixes of
the L76K's one a second: a still node's stray fix comes back within the
allowance, a walker's keeps going. A sleepy node takes a fix only before
each beat ([beats](#beats)), so for it the 72 s are two far fixes a beat
apart.

Which fixes a node takes as its position, and what a carried node gives
up, is in [nodes](#nodes). At the first far fix of a run a relay that holds mail flushes its delivery queue
([delivery](#delivery)): a node that is leaving will soon hear none of
its sleepy nodes' beats.

A carried node is fixed again once its fixes have stayed within the
allowance of one place for four hours, and keeps the fix it has then.
The way back is much slower than the way in: a node that stopped for
less would take mail for new sleepy nodes and strand it when it moved
on, while a node carried by mistake loses little, since its sleepy
nodes have anchored elsewhere already (roadmap D70). A map point makes
a node fixed at once. Power does not change whether a node is carried: a
carried node plugged in is a carried relay until it has stayed four
hours, and a fixed node plugged in keeps as its position the fix it has
then (roadmap D72).

A spoofed fix that drifts slower than the node can move passes the first
check, and makes a relay carried where D63 kept it out: the relay then
gives up its sleepy nodes and is routed to where the spoofer says. The
checks are expected to grow.

**Time.** GPS is used for position only, never for time, and there is
no network time: every timer is local and monotonic, and nothing in the
protocol depends on two nodes agreeing on a clock.

---

## Heartbeat

Every node heartbeats. This section describes a relay's; a sleepy node's
are rarer ([beats](#beats)), and an anchorless node's carry the long
preamble ([sleepy nodes alone](#sleepy-nodes-alone)).

| Field | Bits | Content |
|---|---|---|
| type | 4 | 0x8, with three flags in its low bits: sleepy, anchorless, carried ([packet formats](#packet-formats)) |
| channel | 4 | home channel index 0–12 |
| id | 32 | the node's ID ([node identity](#node-identity)) |
| position | 32 | the node's short position ([positions on the wire](#positions-on-the-wire)) |

9 bytes, 11.8 ms on air at SF5 ([physical layer](#physical-layer)): at
coding rate 4/5 the ninth byte fits in the eighth's block of symbols, so
naming the sender costs no airtime.

- **Timing:** a fixed interval T for each node, from the last byte b of
  its node ID: T = 5 s + 10 s × b / 256, mean 10 s. The first heartbeat
  comes at a random time within T after the boot listen, and each slot
  T after the one before.
- **Channel:** steps `k ← (k + 1) mod 13` every slot, from a random
  start, whether the heartbeat went or not. A fixed cycle reaches every
  neighbour once per 13 heartbeats, a **rotation** (≈130 s, at most
  13 × 15 s = 195 s); a random channel per heartbeat would need ≈41
  heartbeats on average to cover all 13.
- **Procedure:** retune to channel k, listen before talking (CAD + RSSI,
  ≈1 ms); if busy, retry after a random 0–200 ms up to three times, then
  skip; send; retune home. The node is deaf for ≈15 ms per heartbeat,
  ≈0.2%.

No scanning is needed: every relay's heartbeat visits all channels, so a
node listening on any one channel hears every relay in range within one
rotation. The heartbeat load per channel is in
[capacity and sizing](#capacity-and-sizing).

A neighbour that has heard one heartbeat of a node knows the node's
interval from its ID, and so when its next comes to any channel: on
channel c at time t, on channel c + j at t + j × T, and on c again at
t + 13 × T. A sleeping node uses this to wake for its anchor's
([relay choice](#relay-choice)). A heartbeat comes no earlier than its
slot and at most 992.9 ms later: it may find a hand-over attempt under
way, a full frame with the long preamble to a sleepy node outside its
window and the ACK timeout ([hand-over](#hand-over)), and then back off
twice before its last check, 492.9 + 100 + 2 × 200 ms. A check held for
a reception in progress waits less, at most that frame.

**Outside the slots.** A relay also heartbeats to answer a node without
an anchor ([sleepy nodes alone](#sleepy-nodes-alone)). And a node,
sleepy or not, heartbeats once, on its home channel, with the ordinary
preamble, the long one if it is anchorless, and listen-before-talk, when
its sleepy or carried flag changes or, if it is not sleepy, its map
point does ([position](#position), roadmap D71, D72). A sleepy node's,
on its anchor's channel, is a beat to its holder, and its window follows
([delivery](#delivery)).
Its neighbours on that channel then address it anew at once instead of
a rotation later, and an old anchor that holds mail for a node turned
relay hears it as a relay and hands the mail over
([delivery](#delivery)). A node that turns sleepy sends it on the
channel it was at home on, where its neighbours and its sleepy nodes
know it, unless its first beat goes there anyway
([sleepy nodes](#sleepy-nodes)).

It is one heartbeat for any number of changes before it goes, and the
flags change seldom: the sleepy flag is set only after an hour's grace
on battery ([sleepy nodes](#sleepy-nodes)), and the carried flag is
cleared only after four hours still ([position](#position)). A position
that changes with each fix is not announced. A carried relay's slots
carry it every T (roadmap D71);
a sleepy node, carried or not, is a last resort to its neighbours, and
its beats carry its fix ([beats](#beats)). Sleeping nodes hear none of this, receiving
duty-cycled; they learn a change at the next slot they wake for.

**The 256 intervals** are 39.1 ms apart, more than two heartbeats'
airtime, 2 × 11.8 = 23.7 ms, so two nodes whose intervals differ slide
past each other by more than an overlap at every heartbeat and meet at
most once in a row, as random intervals would. 10 s / 23.7 ms = 422
steps would do, and 256 is the largest power of two below it, one byte.
Two nodes in reach share an interval with probability 1/256, which
matters only if they are also in step, within 23.7 ms in a 10 s
interval, 0.24 % of such pairs; their heartbeats then meet each time
they are on one channel, as do those of a pair in step whose intervals
stand 2:1 or 3:2. Clock drift unlocks them: a 32.768 kHz watch crystal
is good to about ±50 ppm, so two clocks part by up to 1 ms per 10 s
heartbeat, and an overlap of 23.7 ms ends within 24 heartbeats, four
minutes, longer for clocks that drift alike. Nodes that boot together,
after a power cut, would start in step but for the random first phase
(roadmap D47).

---

## Neighbours

### Node identity

Every node has a 4-byte **node ID**: the low word of its chip's factory
device ID (nRF52840 FICR DEVICEID), the same at every boot, with no
setup step. It names the node wherever a packet names one: DATA
`dst_hop`, `src_hop` and `dst_id`, ACK `from` and `to`
([packet formats](#packet-formats)). Two of n nodes share an ID with
probability about n² / 2³³. That matters only among the nodes one node
keeps in its tables, 64 neighbours and 256 sleepy nodes
([heard table](#heard-table)): 1.2 × 10⁻⁵.

The ID, not the position, is a node's identity: a node whose position
changes keeps its ID ([position](#position)). Its address changes with
it ([addresses](#addresses)), and so does that of every sleepy node
anchored to it. Two nodes at one position are two nodes.

### Neighbour table

Per node, up to 64 entries, by node ID:

| Field | Content |
|---|---|
| id | from its heartbeat; the entry key |
| position | from its heartbeat, rebuilt from the short position ([positions on the wire](#positions-on-the-wire)), and its local form for distances |
| sleepy, anchorless | from its heartbeat's type ([packet formats](#packet-formats)): a sleepy neighbour is a last-resort next hop, sent to with the long preamble ([next hop](#next-hop)); an anchorless one is no anchor ([relay choice](#relay-choice)) |
| carried | from its heartbeat's type ([nodes](#nodes)): no anchor and no nearer owner ([owner](#owner)) |
| channel | where to transmit to it: from its heartbeat or from `aux` of a DATA it sent ([packet sender](#packet-sender)), whichever came last |
| last_heard | local monotonic time |
| rssi_ema | averaged RSSI of every frame heard from it, each new frame counting half; says whether it has margin |
| fail_count | consecutive ACK failures |
| banned_until | when its ban ends ([bans](#bans)) |
| last_beat | when its last heartbeat heard started, and on which channel: with its interval, when its next comes to any channel ([heartbeat](#heartbeat)) |

**Expiry.** An entry expires after 3 rotations (390 s) without a
heartbeat or an ACK from the neighbour ([the ACK](#the-ack)). A sleepy
entry with an anchor expires after two beat periods, 600 s: its beats
come once a period, on its anchor's channel only ([beats](#beats)), so a
node elsewhere renews it from the frames they exchange. An anchorless
entry ([sleepy nodes alone](#sleepy-nodes-alone)) expires after two of
its sender's worst-case rotations and a heartbeat's lag: 13 beats, one
on each channel, take at most 13 anchorless beat periods, so two
rotations take 26, 1200 s, and the lag is 1.0 s
([heartbeat](#heartbeat)). Two, because a listening receiver loses about
one beat in ten (roadmap D47): with one rotation a live neighbour, perhaps the only
way on where there is no relay, would drop out every tenth rotation, for
13 minutes; with two, one time in a hundred, and a stale entry costs a
hand-over's attempts and a ban (roadmap D51). A sleeping node keeps its
anchor's entry whatever its age, and evicts it for no newcomer
([relay choice](#relay-choice)).

**Margin.** A neighbour has margin when `rssi_ema` is at least 4.1 dB
above the noise floor ([busy estimate](#busy-estimate)): a 255-byte frame
is then lost no more than one time in a hundred, the datasheet's
standard for sensitivity, after a fade of 4 dB. A neighbour without
margin stays in the table, its heartbeats showing it alive, but is a
next hop only when no other is left ([next hop](#next-hop)). The 4.1 dB
follows from the radio. A packet arrives only if every symbol does: a
255-byte frame has 541 symbols, the datasheet's 64-byte reference packet
156, a heartbeat 46, so with the symbol error rate of the modulation (32
possible symbols, detected without phase) a full frame needs 0.44 dB
more signal than the reference for the same 1 % loss, and a heartbeat
0.46 dB less. Half of all heartbeats arrive even 3.2 dB below a full
frame's level, so a heartbeat says little about a full frame.
Sensitivity is 2.5 dB below the noise at SF5 (datasheet Table 6-1), so a
full frame's level is 2.5 − 0.44 = 2.06 dB below the noise floor, and
after the fade the signal must average −2.06 + 4 = 1.94 dB above it.
Packet RSSI is signal and noise together (datasheet §13.5.3), so it
reads 10 log10(1 + 10^0.194) = 4.1 dB above the floor. The fade, 4 dB,
is the standard deviation of fading over time and channel in the
simulated links, until Phase 2 measures it. The average must cover it
because a frame goes out on the neighbour's channel while this relay
hears the neighbour on its own, where the fading differs.

**Demotion.** A neighbour with two ACK failures in a row
(`fail_count ≥ 2`) is demoted until it ACKs a packet from this relay or
three rotations (390 s) pass, and is then a next hop only when no other
with margin is left ([next hop](#next-hop)). Its heartbeats do not clear
the demotion: a heartbeat proves the neighbour is alive, not that it
hears us, and a link that works one way only would otherwise cost every
message the failed attempts again. Expiry takes minutes; a failed
forward must demote at once.

**Eviction.** When the table is full and a relay not in it is heard, the
newcomer replaces, in this order:

1. the least recently heard demoted entry;
2. the least recently heard entry, if it has missed a worst-case
   rotation ([heartbeat](#heartbeat)), and so certainly a heartbeat on
   this channel;
3. if the newcomer holds mail and is within 50 m, the owner radius
   ([owner](#owner)), the least recently heard relay beyond 50 m
   (roadmap D68, D70).

Failing all three it is not added (roadmap D12). Replacing the least
recently heard entry outright fails at a rooftop relay that hears ≈270
relays ([capacity and sizing](#capacity-and-sizing)): about two
heartbeats a second arrive on its channel, most from relays not in the
table, so each entry would last about 64 / 2 ≈ 30 s and every failure
count would be forgotten within a minute. Keeping incumbents holds an
equally arbitrary 64, keeps their failure counts, and replaces exactly
the entries that are failing or silent. The table is then a sample of
the relays heard, weighted toward the strong links, which are heard
first and most often. Those lie where the relays are, which is where
destinations are, so a loaded relay whose best next hop is banned finds
another there; a table spread by bearing routes no better (roadmap
D69).

The relay within 50 m is kept because the owner rule reads from the
table that no relay that near is closer to a destination: one turned
away would have its messages claimed by its neighbour and queued for a
node that never beats. With every person's node a relay, 714 per km²,
5.6 are within 50 m on average (714 × π × 0.05²). A carried relay needs
no such place ([nodes](#nodes)). A sleepy node with an anchor replaces
no entry: it is a last resort ([next hop](#next-hop)), and a relay hears
the beats of every sleepy node on its channel, often more than 64, so
making way for them would push out the neighbours it routes through.

A beat ([beats](#beats)) fills the neighbour table like any heartbeat,
the entry marked sleepy. At a node that is not sleepy it fills the heard
table too if the sleepy bit is alone ([heard table](#heard-table)); a
node without an anchor has no window to deliver in
([sleepy nodes alone](#sleepy-nodes-alone)).

---

## Forwarding

### Addresses

A destination address is a position and a node ID
([node identity](#node-identity)): a relay's own position, or a sleepy
node's anchor's ([nodes](#nodes)), and the recipient's ID. A sender
learns an address from a previous message (the sender's address is
carried in the encrypted payload), from a QR code, or from a directory;
the network provides no lookup. Sending to an area is open item O7.

The network carries no sender address and sends no failure notices. Its
only acknowledgement is the per-hop ACK ([hand-over](#hand-over));
whether a message arrived, and what to do if it did not, is the app's
business, with whatever acknowledgement it puts inside the encrypted
payload.

The app hands a node a message only when the node says it will take
one ([receiving DATA](#receiving-data)), and hears nothing about it
afterwards: not the first hop's ACK, which a user could take for
delivery, and not a drop. From then on the node is sending the message
or waiting to, so a busy network makes its users wait rather than lose
their messages at the first step (roadmap D29). The one thing the app
hears is a sleepy node's refused message, given back with the wait the
refusal asks for, so that a person told to wait is not left with a
message that ages in a node
([a sleepy node's own message](#a-sleepy-nodes-own-message); roadmap
D54).

### Duty cycle cap

At SF5, 10% of airtime is ≈180 B/s per relay, so a busy relay runs out
of budget before its channel fills. Forwarding to the neighbour nearest
the destination does not spread load; central rooftop relays hit the
cap first.

**Token bucket.** Each node keeps its airtime in a bucket that holds at
most 6 s, one minute at 10%, and fills at (360 s − 6 s) per hour, 9.83%
of the time. A transmission may start only if the bucket holds its
airtime, which it takes out. Any interval of length L then carries at
most 6 s + 9.83% × L on air, so any hour at most 360 s: 10%, the limit
of ERC/REC 70-03 band g1, which is measured over an hour (proof in
`tables/budget.rs`). The rate is below 10% by the bucket's depth over
the hour, so that a burst cannot carry an hour over the limit. A node at
the cap sends steadily, 5.9 s of every minute; with its bucket empty a
full frame waits 1.4 s, and nothing waits longer than
6 s / 9.83% = 61 s (roadmap D26).

**Out of budget.** A hand-over the bucket cannot fit yet waits for it;
it is not dropped. Two reserves stay in the bucket:

- Replies, ACKs that take or refuse, leave one heartbeat's airtime,
  11.8 ms. The bucket refills that in 120 ms, far less than the 5 s
  between heartbeats ([heartbeat](#heartbeat)), so heartbeats always go.
- Forwarding, and the ACK that takes on a packet, leave the **reply
  reserve**, 242 ms: the heartbeat, and the refusals the bucket refills
  while a sender retries an unanswered hand-over. The retrying takes
  2.39 s: the two retry waits, 306 + 612 ms, and two more attempts, each
  up to 500 ms of listen-before-talk, a 138.6 ms full frame and the
  100 ms ACK timeout ([hand-over](#hand-over)). The bucket refills
  9.83 % × 2.39 s = 235.5 ms of it, ⌊235.5 / 14.4⌋ = 16 refusals.

A relay takes new work into the regular room of its send queue only if
its bucket holds the reply reserve on top of the work's ACK and of
everything it has queued, and otherwise into a hop place or not at all
([receiving DATA](#receiving-data)). So a hand-over waits for the bucket
only when retries, replies or a delivery that goes first
([delivery](#delivery)) have spent it since its packet was taken, or
when its packet holds a hop place. A relay below the reserve sends
nothing but heartbeats and replies until its bucket refills. Each sender
it refuses bans it ([bans](#bans)), so a sender costs it at most one
reply per ban. Without budget even for a reply it stays silent; its
neighbours' hand-overs to it fail, and they ban it, longer each time,
and route around it (roadmap D27, D29, D42).

### Hand-over

A **hand-over** sends one DATA packet to one neighbour until that
neighbour answers it or the relay gives up. A relay runs one hand-over
at a time; packets waiting their turn are held in its
[send queue](#send-queue). The answer is an ACK, which takes or refuses
the packet and names the message and both nodes ([the ACK](#the-ack)).

1. Retune to the next hop's home channel. Addresses and channels are
   read at the moment of sending, before each attempt, from the latest
   heartbeats (and DATA `aux`, [neighbour table](#neighbour-table)): a retry goes to a
   neighbour's new home if it has moved since the last attempt.
2. Listen-before-talk: CAD, then RSSI. Random backoff 0–50 ms if busy,
   retry; give up after 500 ms and choose the next hop again
   ([next hop](#next-hop)), without counting a failure against the neighbour. The
   neighbour is set aside for this hand-over only, so the next choice is
   a different one. For a return to the parent or a delivery to a
   sleepy node ([delivery](#delivery)), which have no other candidate, a give-up
   counts as one attempt.
3. Transmit. The packet carries the sender's home channel in `aux`
   ([packet formats](#packet-formats)). Its preamble is the shortest, or the long one to a
   sleepy neighbour, as the neighbour table has it then
   ([duty-cycled receive and the long preamble](#duty-cycled-receive-and-the-long-preamble)).
4. Retune home immediately.
5. Wait for the ACK **on the home channel**: timeout 100 ms (ACK airtime
   14.4 ms plus the receiver's retune and listen-before-talk).
6. On timeout: wait ([retry wait](#retry-wait)) and retry, up to 3
   attempts. Then the hand-over has failed: count a failure against the
   neighbour (demoted after 2 in a row, [neighbour table](#neighbour-table))
   and ban it ([bans](#bans)). The message waits in its place for the ban
   and is offered to the same neighbour again, with three fresh attempts.
   Once a failure has demoted the neighbour, the message instead chooses
   the next hop at once ([next hop](#next-hop)): the neighbour is set aside
   for this hand-over, as in step 2, but not counted as tried. A return
   waits for its parent the same way and is dropped when a failure
   demotes the parent ([return to parent](#return-to-parent)). The
   delivery queue keeps a delivery whatever happens to its hand-over
   ([delivery queue](#delivery-queue)).

#### Retry wait

The wait before the retry after the k-th unanswered transmission is
uniform in [0, 2^k × 153.0 ms]: 0–306 ms, then 0–612 ms. 153.0 ms is how
long one hand-over holds its receiver, a full frame (138.6 ms) and its
ACK (14.4 ms). Other than a weak link, the usual cause of a missing ACK
is a hidden terminal: a sender the first cannot hear transmitted to the
same receiver at the same time, and both miss the ACK and retry. Their
retries miss each other only if they start more than one hand-over
apart, which for full frames and a window of W units happens with
probability (1 − 1/W)²: never with one unit, 1/4 with two, 9/16 with
four. Doubling per attempt also spreads more than two such senders.

The listen-before-talk backoff of step 2 stays short: it waits for a
channel the sender can hear to clear, and checks again (roadmap D22). A
hand-over whose frame has the long preamble holds its receiver longer by
as much as its frame is longer, 354.3 ms, so its unit is 507.3 ms
([duty-cycled receive and the long preamble](#duty-cycled-receive-and-the-long-preamble)).

#### The ACK

The receiver retunes to the sender's channel, listens before talking,
sends the ACK, and retunes home. The ACK is the first byte, the message
ID, and the IDs of the answering and the answered node: 13 bytes,
14.4 ms ([packet formats](#packet-formats)). One that refuses is a
**NACK**, the same and a byte for the wait it asks ([bans](#bans)): 14
bytes at the same 14.4 ms, since at SF5 the 14th byte fits in the 13th's
block of symbols. Below, an ACK is either unless it says so. The sender
matches the ID and both names against its outstanding hand-over. An ACK
resets the neighbour's failure count and refreshes its last-heard time
([neighbour table](#neighbour-table)): a neighbour that answers is alive
whether or not its heartbeats are heard.

Both IDs are needed. Nodes share home channels, so without the answering
node's ID an ACK for the same message from another relay on the same
channel, which the depth-first search can produce, would end the wrong
hand-over and strand the message. And two senders can offer the same
message to one node at once: one retrying a hand-over whose ACK was
lost while another returns the message there, or the two copies of a
split message ([why the search finds a path](#why-the-search-finds-a-path)).
If the ACK named only the message and the relay, each sender would take
the answer to the other as its own. A taken one would end a hand-over
the relay never took, and the message would be lost; a refused one would
send a sender on from a relay that took its packet, and the message
would have a second copy (roadmap D32).

A node ACKs a packet as soon as it has received it, even while waiting
for an ACK of its own; if that makes it miss its own ACK, its next
attempt covers it.

A node owes at most one reply at a time. A DATA that arrives while its
reply to another packet still waits for the channel is neither taken nor
refused: the node says nothing and changes nothing, and the sender
retries as for a lost ACK. A second reply would go out only after the
first, which may wait for its channel until its own sender gives up,
100 ms; taking a packet it may not answer in time would let its sender
hand the message to someone else as well. A copy of the packet the owed
reply answers, sent again because that reply is late, is answered by it
(roadmap D29).

So ACKs too arrive on the home channel, which a node leaves only to
transmit ([home channel selection](#home-channel-selection)). A sleepy
node's home channel is its anchor's, so traffic between them never
retunes. A node is deaf for ≈15 ms per packet it forwards or ACKs; a
sender that gets no ACK retries. That is the cost of one radio.

#### Bans

A neighbour that refuses a packet or fails a hand-over is **banned**: no
candidate for any message ([next hop](#next-hop)) until the ban ends.
Three rules set how long.

**A refusal bans for the wait it asks.** A NACK that answers the
outstanding hand-over means the receiver does not take the packet
([receiving DATA](#receiving-data)). The sender treats it as a
listen-before-talk give-up (step 2): the neighbour is set aside for this
hand-over, not counted as tried, no failure is counted, and the next hop
is chosen at once. A return to the parent has no other candidate: it
waits in the send queue until the ban ends and goes again, the refused
transmission not counting as one of its attempts.

The ban is never shorter than 1.56 s, the time the refuser's bucket
takes to refill what the refused packet would have cost. A relay
refuses a packet to send on when its frame and the ACK that would take
it, at most 138.6 + 14.4 = 153.0 ms, do not fit in its bucket and no hop
place takes it, and the bucket refills that in
153.0 ms / 9.83 % = 1.56 s ([duty cycle cap](#duty-cycle-cap)); trying
sooner would only cost it another reply (roadmap D29).

The NACK tells the wait in whole bans of 1.56 s, rounded down
([packet formats](#packet-formats)). For want of room it is the time
until the bucket holds everything queued and the frame and its ACK, with
the reply reserve left, if nothing else comes. The senders refused stand
in line, each told one ban later than the one before, the time the
bucket takes to refill the frame and ACK it will bring back, so they
come back one after another as room frees. A refusal of fresh work for
the way on ([receiving DATA](#receiving-data)) asks for the rest of the
ban on that way, as learned, without a place in line: passed on so, a
wait cannot grow from relay to relay around a ring.

The places in line are one ban apart and nothing in the protocol acts
sooner, so the ban is the unit; a coarser one would merge neighbouring
places. A byte holds 255 bans, 398 s; a longer wait is told as the
longest, the sender coming back early to be refused again (roadmap
D54).

**Silence doubles the ban from 1.56 s to 61 s.** The ban after the k-th
failed hand-over in a row is 1.56 s doubled k − 1 times: 1.56 s, 3.11 s,
6.2 s and so on, up to 61 s, the longest anything waits for a bucket
([duty cycle cap](#duty-cycle-cap)). An ACK from the neighbour resets its
failure count and so the ladder (roadmap D42). A refusal measures one
thing, what one hand-over takes from the receiver's bucket. Silence says
only that the channel or the receiver is full, or the link gone, not how
full, so its ban grows. Without it, a neighbour that stops answering
under load is offered every other message at once, each offer up to
three more frames where frames are already being lost; past the cap that
loop is what breaks first.

The message whose hand-over failed waits for the same neighbour (step 6)
rather than going to the next, which is usually a longer way round on
the same full channels: going there at once undoes what the ban gains
(roadmap D42). Nor is the neighbour counted as tried: an attempt is lost
to a collision at the receiver far more often than its ACK goes astray
(roadmap D34), so the neighbour most likely does not have the message,
and it stays a candidate once its ban has ended. If it did take it, the
next offer costs one round trip: it holds the message and only ACKs, or
has finished with it and returns it ([receiving DATA](#receiving-data);
roadmap D36). Two silences in a row demote the neighbour, so a link
that is really gone costs one ban and one more hand-over before the
message moves on. A neighbour that is gone is tried again after each ban
until its entry expires; [bounds](#bounds) adds up what that costs.

**A node's own message waits on the same ladder.** Offered again every
1.56 s, a node's own application's message would put a full frame on the
refusing neighbour's channel that often, 9 % of it, hiding its ACKs, and
spend that neighbour's bucket as fast as it refills. So after its k-th
refusal in a row it waits the k-th step of the silence ladder, and starts
again from one ban once it is taken or dropped. A sleepy node's own
message has a wait of its own
([a sleepy node's own message](#a-sleepy-nodes-own-message)).

#### Send queue

The send queue has a regular room of 40 places and one hop place for
each count from 1 to 254; the packet being handed over keeps its place.
A packet to send on enters the regular room only if it fits in the
bucket with everything queued ([receiving DATA](#receiving-data)), so it
waits for the hand-overs ahead of it, not for the bucket to refill; a
delivery in its node's window, which goes ahead of them all, needs only
its own frame ([delivery queue](#delivery-queue)). The bucket sets the
40: it holds at most 6 s ([duty cycle cap](#duty-cycle-cap)), and a full
frame and the ACK that took it on cost 138.6 + 14.4 = 153.0 ms, so
⌈6 s / 153.0 ms⌉ = 40. With the reply reserve left in, a full bucket
takes at most 37 full frames, so for full frames the bucket is the
limit; shorter frames fill the 40 places first, and forty hand-overs of
them go out within a few seconds.

A packet received with count h that the regular room refuses takes the
highest free hop place numbered h or lower, and keeps it until it leaves
the queue. A hop place holds its packet whatever the bucket holds; the
bucket still decides when it is sent. A message entering the network, a
sleepy node's own with count 0 or a node's own app's, takes no hop
place, so overload stops new messages where they enter, at the app
([addresses](#addresses)). A packet received at 255 ends at the cap
([hop count](#hop-count)), so 254 hop places are enough. Packets are
handed over in the order they arrived, except that a delivery goes
first ([delivery](#delivery)), and that when bans end the packets that
waited for them go first ([overload waits](#overload-waits)).

#### Overload waits

Overload makes a message wait and never ends it. A message whose every
remaining candidate is banned, by refusals or by silence, waits in its
place in the send queue, its airtime still counted, until the earliest
ban ends, and is routed again then, at the origin or anywhere else. A
message ends only when it is delivered, when its search runs out of
neighbours ([return to parent](#return-to-parent)), when a return goes
unanswered twice in a row (step 6), when its time runs out on the way
([time to live](#time-to-live)), when a newer message needs its place in
the delivery queue ([delivery queue](#delivery-queue)), at the hop cap ([hop count](#hop-count)), or when a node holding it dies
(roadmap D31, D44).

Waiting alone can lock relays up: two neighbours whose queues are full
of messages for each other would refuse each other forever, since what
a relay holds drains only as it sends (roadmap D29). The hop places of
the send queue prevent it. Take the highest-numbered hop place held at
any relay, k. Its message has made at least k hand-overs and is sent
with a higher count, and place k + 1 is free at every relay, so the
neighbour it is offered to can take it, unless its count is at the cap,
where it ends. That message moves, then the next highest, and so on
down to the regular room.

For this the message that has come farthest must be offered when a ban
ends: a less travelled one offered first would be refused and ban the
neighbour again, each time. So when bans end, the messages that waited
for them go first, the most travelled first. The rest of the queue
keeps its order, since ordering it all by count would starve messages
that have just entered.

### Routing

A message travels as a DATA packet ([packet formats](#packet-formats))
from relay to relay, each hand-over acknowledged
([hand-over](#hand-over)). A relay hands it to its neighbour closest to
the destination position. Where that leads into a gap, a relay from
which no neighbour leads on, the relays route around it by depth-first
search: a relay with no neighbour left to try returns the message to the
relay it first got it from, which tries its next neighbour. Each relay
keeps a small record per message ([message records](#message-records)),
and the packet carries nothing for the search but its sender and a hop
count. The search only routes around gaps; it never searches for an ID.
A sleepy node that is a next hop ([next hop](#next-hop)) forwards as a
relay does, and what this section says of a relay holds for it then.

The **origin** of a message is the node that first sends it into the
network: the node a sleepy node handed its own message to, or a node
sending for its own application. The search starts there and, if it
finds nothing, ends there. The rules below are roadmap D11, D16 and
D17.

#### Packet sender

A node acts on a DATA packet whose `dst_hop` is its own ID
([node identity](#node-identity)) and ignores the rest. The packet names
its sender in `src_hop` and the sender's home channel in `aux`
([packet formats](#packet-formats)), which is all a relay needs to send
something back: `dst_hop` set to `src_hop`, on the channel in `aux`. It
needs no neighbour-table entry for the sender; if it has one, `aux`
becomes its channel there, as a heartbeat's would
([neighbour table](#neighbour-table)).

A packet with `hops = 0` is a sleepy node's own message, and the node
that receives it is the origin ([hop count](#hop-count)).

#### Message records

A relay keeps a **record** for every message it has received or
originated:

| Field | Content |
|---|---|
| ID | the message ID ([packet formats](#packet-formats)) |
| parent | `src_hop` and `aux` of the packet that created the record: the ID and home channel of the node the message first came from; none at the origin. The channel is used only if that node is no longer in the neighbour table ([return to parent](#return-to-parent)) |
| handed | the neighbours that took the message from this relay, each by ACKing a hand-over of it ([hand-over](#hand-over)), as one bit per neighbour-table slot |
| last packet | `src_hop` and `hops` of the last DATA of the message it received: the same pair again is a retransmission ([receiving DATA](#receiving-data)) |
| highest hops | the largest `hops` ([hop count](#hop-count)) received for the message, or sent in a hand-over of it that succeeded |
| arrived | whether the message has reached its owner: this node owns the destination ([owner](#owner)), or the node it is addressed to took it from this one |
| last active | when it last received or sent the message |

The relay's **tried set** for a message is those known to have it: the
parent, the neighbours in `handed`, and a sleepy node that handed over
its own message. A neighbour that has not ACKed the message stays a
candidate, whether it was passed over, being demoted, expired or not in
the table at the time, or was offered the message and refused it or
never answered ([hand-over](#hand-over)). When a neighbour-table slot
changes hands, its bit is cleared in every record. The record is also
how a relay recognises a repeat of a message it has handled
([receiving DATA](#receiving-data)); there is no separate window of
recent IDs.

**Eviction.** A record has no lifetime. It is kept until the table needs
its place; a new record then replaces the least recently active one
whose message the relay does not hold. A relay holds a message while it
is in its send queue ([send queue](#send-queue)), the hand-over included,
or set aside for a full delivery queue
([receiving DATA](#receiving-data)). Without its record the message
could not be routed when its turn comes, so a message is never lost for
want of room (roadmap D31). The table holds 1855 records
([bounds](#bounds)).

#### Next hop

One function chooses the next hop for a first send, for the next try
after the message comes back, and after a failed hand-over
([hand-over](#hand-over)). The **candidates** are the live
neighbour-table entries ([neighbour table](#neighbour-table)) except
those in the tried set ([message records](#message-records)), those
banned ([bans](#bans)), and those set aside for the current hand-over
because they did not take it ([hand-over](#hand-over) steps 2 and 6).

A sleepy neighbour with an anchor, whose last heartbeat had the sleepy
bit alone ([packet formats](#packet-formats)), is a **last resort**: a
candidate only when the search would otherwise end here, with no other
candidate left and none that is only banned, so that the message would
go back to its parent or be dropped ([return to parent](#return-to-parent)).
A ban is no reason to go through a node that sleeps; the message waits
for the ban ([overload waits](#overload-waits)). It is sent to with the
long preamble. Where a relay is in reach, that relay is the hop; the
last resort is for a pocket with no relay in that direction, where a
sleepy node that has anchored is the way out for the anchorless ones
behind it (roadmap D58). An anchorless neighbour is a
candidate like any other, sent to with the long preamble. The first hop
of a sleepy node's own message is its anchor while the anchor is live
([a sleepy node's own message](#a-sleepy-nodes-own-message)).

The **order** is: neighbours with margin
([neighbour table](#neighbour-table)) before those without; within each,
neighbours that are not demoted before demoted ones; within each,
distance to the destination position in the local plane
([positions on the wire](#positions-on-the-wire)), nearest first; equal
distances by ID. The relay sends to the first candidate. If there is
none but neighbours that are only banned, the message waits until the
earliest ban ends and is routed again then
([overload waits](#overload-waits)). If there is none at all, the relay
returns the message to its parent ([return to parent](#return-to-parent)).

Neighbours without margin come after every neighbour with margin,
demoted ones included, because fading lets a relay hear the 9-byte
heartbeat of a neighbour whose link usually cannot carry a full DATA
frame. Such a neighbour is often the farthest, so the nearest to the
destination; without this rule every message would try it first, three
DATA frames unanswered. It stays a last resort rather than left out,
because on a sparse line it may be the only way on, and a weak link
still carries some frames; when it takes one, its ACK counts toward its
margin (roadmap D38).

A demoted neighbour with margin is likewise tried only when every other
has been, its failures counted as usual. Demotion keeps a failing link
from costing every message its attempts while another route exists; it
should not drop a message whose only route is that link. Two hand-overs
lost to collisions demote a neighbour that works, and on a line it is
the only route, so without the last resort every message would be
dropped until the demotion lapsed, six and a half minutes. What a
neighbour that is really gone costs is in [bounds](#bounds) (roadmap
D25, D36, D42).

Nothing requires the next hop to be closer to the destination than the
relay itself. After the message comes back, the best neighbour left is
usually farther away, and such a rule would turn every gap into a dead
end. On a first send the nearest neighbour is chosen anyway.

A candidate may already have the message by another path; the relay
cannot know, since its neighbours listen on other channels. The
candidate takes it all the same ([receiving DATA](#receiving-data)). If
the message has arrived there, or the candidate holds another copy, it
goes no further; otherwise the candidate goes on with its own search,
and in time offers the message back or returns it, which the relay
takes as its return.

#### Receiving DATA

A relay answers a DATA packet addressed to it
([packet sender](#packet-sender)) before acting on it, with an ACK that
takes or refuses it, and only when it owes no other reply
([the ACK](#the-ack)). Whether it takes the packet depends on what the
packet gives it to do:

- **Something to send on.** It takes the packet into the regular room
  of its send queue only if the room has a place and the bucket
  ([duty cycle cap](#duty-cycle-cap)) holds, as it stands, the ACK, the
  airtime of every frame in the send queue and of the new one, and the
  reply reserve; failing that, into a hop place
  ([send queue](#send-queue)), whatever the bucket holds. So it refuses a
  message already in the network only when the hop places it may use
  are taken too, whatever its bans:
  [overload waits](#overload-waits) needs every such message to move on
  (roadmap D34).
- **A message entering the network here**, a sleepy node's own (count
  0, [hop count](#hop-count)) or the node's own application's, also needs a neighbour the relay
  could hand it to now: a live one that is not banned
  ([next hop](#next-hop)), demoted or not, since the message has tried
  none. A relay whose neighbours are all banned would only hold the
  message until a ban ends, and its offer could be refused again;
  refused here, the message waits at its origin, where the application
  sees it. A relay with no live neighbour at all takes it, and the
  search ends as in [return to parent](#return-to-parent). So a relay
  takes from sleepy nodes and its own application no more than it can
  start sending now (roadmap D34). It also refuses such a message while
  the neighbour it would go to, were none banned, is banned: the way on
  has pushed back, and the message would only wait and age here. Each
  neighbour's pushback is its own ban, so it does not hold up the other
  ways, and it ends with the ban. A message already in the network is
  not refused for that: its sender would send it round the pushback, and
  around a ring of relays such refusals kept each other up (roadmap
  D54). From its own application a node takes one message at a time,
  until its first hop takes it: while it is running, holds none, and has
  room in its send queue and bucket for a full frame on top of what they
  hold ([send queue](#send-queue)), and not every neighbour is banned, or,
  for a sleepy node with a live anchor, not the anchor
  ([a sleepy node's own message](#a-sleepy-nodes-own-message)). A
  message from its own application for a sleepy node it holds for, when
  the delivery queue is full, is kept aside until the queue has a place.
- **Something to deliver.** It takes the packet only if its bucket holds
  the reply reserve and an ACK and, for another node, its delivery
  queue has room ([delivery queue](#delivery-queue)).
- **No work**, cases 1, 3 and 4 below. It takes the packet even when it
  would refuse work: a refusal would send its sender on with a second
  copy.

Otherwise it refuses: after listening before talking it sends a NACK on
the channel in `aux`, changes nothing, and the sender tries elsewhere at
once or waits ([bans](#bans)). A relay that cannot afford even the
reply stays silent, and its sender retries as for a lost ACK (roadmap
D27, D29).

The packet is one of the following, tested in this order:

1. **Arrived:** the relay has delivered the message or holds it for a
   sleepy node ([delivery queue](#delivery-queue)), or its record says
   the message has arrived ([message records](#message-records)). ACK
   only.
2. **New:** the relay has no record for the message. It creates one,
   with the sender as parent (none if `hops = 0`). If it owns the
   destination ([owner](#owner)) it delivers; otherwise it chooses a next
   hop ([next hop](#next-hop)) and sends.
3. **A retransmission:** the sender and the count are those of the last
   packet of the message the relay received: the sender's ACK was lost
   and it sent the same packet again. ACK only.
4. **A copy while the relay holds one:** the message waits in its send
   queue or is set aside (above). ACK
   only: the two copies merge, and the relay's goes on. A hand-over
   whose frame has gone out does not count as holding one: its receiver
   may have taken it with only the ACK lost, and this packet may be that
   copy come back.
5. **A copy, and the relay holds none:** the relay takes it and goes on
   with its own search from its record. It checks ownership again (a
   closer neighbour may have been demoted since), then sends to its next
   candidate ([next hop](#next-hop)), returns the message to its parent
   ([return to parent](#return-to-parent)), or, at the origin, drops it.
   From a neighbour it handed the message to, this is that neighbour's
   return. From the neighbour it is still handing the message to, it
   also ends that hand-over: the neighbour has the message.

In every case the relay records the packet's sender and count as the
last packet, raises its highest count if the packet's is higher, and
updates its last-active time.

Cases 1, 3 and 4 end a packet without acting on it, and in each the
message lives on: it has arrived, the packet was acted on when it first
came, or another copy goes on, so no case ends the last copy of a
message ([overload waits](#overload-waits) lists what does). Nothing is
sent straight back: a relay that has had the message goes on from where
its own search stands, and its sender, if not tried, stays a candidate
and gets the message back in turn. A return goes from a relay to its
parent, and the parents, each set when its record was made, lead back
to the origin without a loop, so a message cannot go back and forth
between two relays that hold none of it (roadmap D35).

#### Return to parent

When [next hop](#next-hop) finds no candidate, every neighbour having
been tried or set aside, the relay sends the message back to its parent:
DATA with `dst_hop` the parent's ID from the record, on the parent's
home channel as the neighbour table has it at the moment of sending, so
a return follows a parent that has moved home since
([channel moves](#channel-moves)). Only a parent no longer in the table
is sent to on the channel in the record. The parent takes it as a
return ([receiving DATA](#receiving-data), case 5) and continues with its
next neighbour.

The origin has no parent. With no candidate left it drops the message:
no relay that can be reached from it through neighbours that answer owns
the destination. Nobody is told, the origin's own application included
([addresses](#addresses), roadmap D10): that is the cost of having no
end-to-end acknowledgement.

A message whose neighbours left are only banned is neither returned nor
dropped: it waits ([overload waits](#overload-waits)), a node's own
application's message as in [bans](#bans).

If a failed hand-over to the parent demotes it, after the message has
waited for the parent's ban and gone again ([hand-over](#hand-over) step
6), the relay drops the message and the search ends early. A parent that
refuses the return has not failed ([bans](#bans)).

#### Hop count

`hops` in the DATA header counts hand-overs. A sleepy node with an
anchor sends its own message with 0
([a sleepy node's own message](#a-sleepy-nodes-own-message)): the node
that takes it is its origin, the message has no parent, and the sender
counts as tried, so the search never returns to a node that sleeps. Sent
with 1, the message could take hop place 1 at its first hop, which
overload must not give to fresh work ([send queue](#send-queue)). A relay
sending for its own application sends 1, as if it had received the
message at 0, since 0 means "a sleepy node's own, start the search
here". A relay sends its record's highest
count plus one ([message records](#message-records)), whether forwarding
or returning to its parent. The count is set when a hand-over's first
frame goes out, and its retries repeat it ([hand-over](#hand-over)); a
frame that waited in the send queue, or goes to another neighbour after
a failed hand-over, carries the count as it stands when it goes.

So along the path of a message the count only grows, and a relay sends
a count for a message once, but in retries: to send another packet of
it, it must have received one since its last hand-over that succeeded,
or that hand-over failed and the packet goes elsewhere. After a wait for
a ban it may also go again to the neighbour that did not answer; if that
neighbour took the first, the second is in fact a retransmission to it.
A receiver therefore tells a retransmission by the sender and the count
([receiving DATA](#receiving-data), case 3). The count also numbers the
hop places of the send queue ([send queue](#send-queue)).

A relay that would have to send a count above 255, the largest the
one-byte field holds, drops the message, except an owner delivering it
to its node ([delivery](#delivery)), which sends 255: the search has
ended there, and dropping the message at the last step would leave it
in the delivery queue to be dropped again at every try. A search that
keeps its records ends on its own
([why the search finds a path](#why-the-search-finds-a-path)); the cap
ends one that has lost its way because a record was replaced or the
message split in two, and it ends the chain of hop places
([overload waits](#overload-waits)). 255 is generous: the longest
straight route across the city of
[capacity and sizing](#capacity-and-sizing) (≈30 km at the ≈750 m
street range of [physical layer](#physical-layer)) is about 40
hand-overs, leaving about 200 for routing around gaps.

#### Time to live

A message lives in the network for at most the delivery TTL, 600 s
([nodes](#nodes)), from when a node takes it from its application.
Without an end by time, a message the network has taken lives as long as
hand-overs to someone succeed: past the cap, long after its sender and
its destination's queue would have given it up, keeping the relays at
their duty cap (roadmap D42, D44).

Every DATA carries the time the message has left, `time_left`, in whole
units of 2.5 s ([packet formats](#packet-formats)); the node that took
the message from its application starts it at the TTL. Each
transmission, retries included, carries what is left at that moment,
rounded down, so no node claims more than the message has; a sleepy
node's waits for its anchor count against it. A relay that takes a
packet to send on counts the message's time from the packet: what it
carries less the frame's time on air, since the sender stamps the time
as the frame goes out.

With one unit or less left, a message can no longer be sent with any,
and its time is gone. The relay drops it wherever it waits in the send
queue, for its turn, the bucket or a ban, in a hop place or as a return,
and drops one whose frame is on air at its next attempt. A packet that
arrives with its time gone is no work: the relay takes it, as in cases
1, 3 and 4 of [receiving DATA](#receiving-data), and the message ends
there. A return or a copy carries the time like any DATA, so the
parent's copy (case 5) ends by the same clock. A message to deliver is
not affected: at its owner the delivery queue keeps it until its node
takes it or a newer message needs its place, whatever was left
([delivery queue](#delivery-queue)).

2.5 s is the finest unit at which one byte holds the TTL, as 240.
Rounding down loses up to a unit at each hand-over, a whole one at a
hand-over that follows at once, so such a message has time for 239
hand-overs, near the 255 of the hop cap ([hop count](#hop-count)). At
10 s it would have time for 59, fewer than some searches under load take
to reach their owner (roadmap D44). Rounded to the nearest instead, a
message that stays just under half a unit at each relay would lose no
time at all.

#### Owner

A node **owns** a message's destination when:

- `dst_id` is its own ID: it is the addressed node, wherever it is; or
- its own position is `dst_pos`; or
- it is within 50 m of `dst_pos` and no live, non-demoted neighbour is
  closer to it than itself. The relay at that position has then probably
  died or moved; 50 m is what two GPS fixes of a node that has not moved
  may differ by ([position](#position)), so a relay this close is, for
  the radio, in the same place.

A node farther than 50 m with no closer neighbour is at a gap, not the
owner, and the search goes on ([next hop](#next-hop)). A sleepy or
carried node owns only what is addressed to its ID and sends anything
else on ([nodes](#nodes)), and a sleepy or carried neighbour closer to
`dst_pos` does not keep a node from owning it.

A message whose destination is the sending node's own address is
delivered there without entering the search. Otherwise the destination
position decides who holds it: a relay that hears the sleepy node but
does not own the position routes the message on like any other. A
sleepy node that has just moved to another anchor stays in the old
relay's heard table for a delivery TTL ([heard table](#heard-table)), so
the table disagrees with the address exactly when the address is new;
delivering from the table would queue the message for a node that has
gone (roadmap D21).

The addressed node delivers the message to its application. Any other
owner queues it for the addressed node
([delivery queue](#delivery-queue)) and sends it when the node listens
([delivery](#delivery)), whether or not it hears the node now; a later
copy of a delivered message is only ACKed
([receiving DATA](#receiving-data), case 1). A message for a relay that
is gone waits there until a newer message needs its place, and ends.


### Why the search finds a path

Suppose that while a message is being routed the links do not change,
every hand-over to a neighbour that answers succeeds within its attempts,
and no record is replaced. Then:

- The message moves as a single packet: a relay sends only what it has
  just received or originated, and every ACK-only case of
  [receiving DATA](#receiving-data) sends nothing.
- Each stay of the message at a relay ends with a hand-over that
  succeeds, or at the origin with a drop ([next hop](#next-hop),
  [return to parent](#return-to-parent)), and there are finitely many of
  those: a relay hands the message to each neighbour at most once, since
  one that takes it is tried ([message records](#message-records)), and
  the parents lead to the origin without a loop. So the search ends. A
  neighbour that does not answer costs a bounded number of failed
  hand-overs ([bounds](#bounds)), and the hop cap bounds the stays
  ([hop count](#hop-count)).
- A send to a relay that already has the message hands it the search
  ([receiving DATA](#receiving-data), case 5), and the search comes back
  to the sender as a return, since the sender stays one of its
  candidates unless it has tried it. A relay whose search is unfinished
  gets the message back that way or through the parents, so none is left
  with neighbours untried.

So the message makes a depth-first traversal of the relays reachable
from the origin through neighbours that answer, nearest to the
destination first; a relay met again is searched on from where it
stood, not skipped. A depth-first traversal visits every relay it can
reach; it stops at the first owner, or returns to the origin when there
is none. If a path to an owner exists, the message finds it, provided
the traversal fits in the 255 hand-overs of [hop count](#hop-count).

What breaks it:

- **A lost record.** A message returning to a relay whose record has
  been replaced is new there; the relay searches again from itself, and
  the relays above it never get the message back. The search ends early,
  or wanders until the hop cap ends it.
- **A split.** A hand-over whose ACKs were all lost looks failed, so the
  sender moves on while the receiver also carries the message: two copies
  search at once. They share the relays' records, so each goes on from
  where the other left a relay. The sender has not marked the receiver
  tried, so a copy that comes back to it is offered there again. A copy
  ends where it meets the other held at a relay ([receiving DATA](#receiving-data), case 4),
  where the message has arrived (case 1), at the origin with the search,
  or at the hop cap. A copy is never ended where it is the last one.
- **Links that change** during the search, including a parent that moves
  to another home channel ([return to parent](#return-to-parent)): parts of the graph go
  unsearched.

### Bounds

- **Hand-overs per message:** at most 256 that succeed (the first, at
  count 0, and 255 more, [hop count](#hop-count)). A relay hands the message to each
  neighbour at most once per record, in at most 3 attempts. A neighbour
  that does not answer costs 3 attempts each time it is offered a
  message, and each failure bans it for every message, the ban doubling
  up to 61 s ([bans](#bans)). So a dead neighbour costs a relay at most 11
  failed hand-overs, 33 frames, whatever the messages, before it leaves
  the table 390 s after its last heartbeat or ACK: the bans 1.56, 3.11, 6.2,
  12.5, 24.9 and 49.8 s add up to 98 s, and 61 s bans fill the rest.
  After two failures it is a last resort ([neighbour table](#neighbour-table)).
- **Life:** at most the delivery TTL in the network, whatever its
  search and waits ([time to live](#time-to-live)), so no longer than
  that in a send queue, then in its owner's delivery queue until its node
  takes it or, once stale, its place is needed
  ([delivery queue](#delivery-queue)).
- **Cost of a search:** each link between two relays costs at most two
  hand-overs that succeed from each end: a send and a return. In the
  usual case it is two in all; a split adds a round trip on the link
  whose ACKs were lost. A link that does not answer costs three attempts
  each time the message is routed past it, so a search that comes back
  through a relay often, as one with no owner does, pays for it at every
  stay (roadmap D36). Within the hop cap a search can explore about 64 to
  128 links beyond the route that led into the gap.
- **Message records:** at most 1855 per relay ([message records](#message-records)). A message
  sent on can come back, returned by a child or offered by a split copy,
  and its record must still be here then. On its way it may wait at each
  relay as long as a full send queue takes to drain, each of the 294
  places waiting out one ban of 1.56 s ([bans](#bans)): 457.3 s. A ban after
  silence lasts up to 61 s ([bans](#bans)), so where every way on is
  silent a message can wait longer; its own record is kept while it is
  held, and this bound covers the wait behind refusals only (roadmap
  D42). Every message a relay handles costs at least an ACK (13 bytes,
  14.4 ms) and one DATA packet with no payload (22 bytes, 18.2 ms: at
  SF5 and coding rate 4/5 the 22nd byte fits in the 21st's block of
  symbols, [physical layer](#physical-layer)), together 32.6 ms. Any 457.3 s carries at most a full bucket and its
  refill, 6 s + 9.83% × 457.3 s = 51.0 s of airtime ([duty cycle cap](#duty-cycle-cap)), so a
  relay handles at most 1561 messages in that time. The
  records of held messages are never evicted and may be older, so the
  table has room for the 294 a send queue holds on top: 1855.
- **State per relay:** the RAM budget of [implementation notes](#implementation-notes).

---

## Sleepy nodes

A sleepy node ([nodes](#nodes), roadmap D71) beats on its anchor's
channel ([beats](#beats)), takes what its holder has for it in the
window after ([delivery](#delivery)), and between its windows receives
duty-cycled
([duty-cycled receive and the long preamble](#duty-cycled-receive-and-the-long-preamble)).

A sleepy node holds mail for nobody. The mailbox
([heard table](#heard-table), [delivery queue](#delivery-queue)) is a
relay's, kept so that it can deliver to a sleepy node in its window
without the long preamble. A sleepy node keeps the neighbour table and
the send queue, and forwards what it is given at once, with the long
preamble to a sleepy neighbour and the short one to any other.
Everything else it does as any node. A carried relay keeps its mailbox
but takes no new mail into it ([nodes](#nodes)).

For a node that has gone dark the network keeps, for the delivery TTL
([nodes](#nodes)), the node in its holder's table
([heard table](#heard-table)) and the messages waiting for it, which
stay longer while nothing needs their place
([delivery queue](#delivery-queue)).

**Plugged in and unplugged.** At boot a node is sleepy or not by its
power at once (roadmap D71): nobody depends on it yet. A sleepy node
plugged in is a relay at once. It leaves its anchor, its address becomes
its own position, which it keeps from then on unless it is carried
([position](#position)), and it heartbeats as a relay, the first time at
once ([heartbeat](#heartbeat)). Its old anchor hears that heartbeat on
the node's home channel until then, its own, and hands over what it
holds for the node ([delivery](#delivery)).

A relay unplugged changes nothing for a **grace** of one hour: it
relays, owns and holds mail as before, and power back within the grace
is as if nothing happened, so a charger moved across the room costs
nobody anything. The grace costs the node an hour of listening at
5.8 mA
([duty-cycled receive and the long preamble](#duty-cycled-receive-and-the-long-preamble)),
0.68 % of the 850 mAh battery ([hardware](#hardware)). A shorter one
would turn a node sleepy at every brief unplug, and each turn moves its
sleepy nodes, and their addresses, to other anchors.

When the grace ends the node takes no more mail, owning nothing by
position ([nodes](#nodes)), and flushes its
delivery queue ([delivery](#delivery)), listening all the time until
each message has had its attempt; a relay that turned carried in its
grace has flushed already, at its first far fix. Then it is sleepy. It
holds nothing, so what the flush did not deliver is dropped, and it
takes as its anchor the strongest relay it knows, as at the end of a
boot listen ([relay choice](#relay-choice)), or is anchorless with none.
It stays carried or fixed as it was ([position](#position)). Its sleepy
nodes hear no more slot heartbeats of it, and leave it as an anchor
heard sleepy ([relay choice](#relay-choice)).

### Relay choice

Listening on any one channel, a node hears every relay in range once per
rotation ([heartbeat](#heartbeat)), and keeps it in its neighbour table
with the averaged RSSI of what it heard from it
([neighbour table](#neighbour-table)).

**First choice.** A sleepy node takes as its anchor the first relay it
hears, at once, so it is anchorless only while it hears none. Only a
fixed relay can be an anchor ([nodes](#nodes)). A node whose last
heartbeat had the anchorless bit is none either: it holds and forwards
messages but leaves that mode at the first relay it hears, so an address
pointing at it would soon point at a node that holds nothing
([sleepy nodes alone](#sleepy-nodes-alone)).

The node then stays awake a worst-case rotation
([heartbeat](#heartbeat)), by when every relay in range has visited its
channel, and takes the strongest it heard, without the margin below: few
can have its address yet, so the move costs little. Receiving
duty-cycled, it would hear no other relay's heartbeat, and the first
relay heard would stay its anchor however faint. Moving to each stronger
relay as it is heard would chase the fading instead: one heartbeat of a
relay no better is above the anchor's average half the time (roadmap
D49). The node listens on the anchor's home channel, and its address
follows the anchor ([addresses](#addresses)).

**Switching.** While it listens, at its first choice and when it checks
the anchor, a node moves to another relay only when that relay has been
heard over at least two heartbeats and its averaged RSSI is at least
8 dB above the anchor's: two standard deviations of the difference, so
that a relay no better takes the node on 2.3 % of comparisons. A relay's
heartbeats reach the node's channel about 130 s apart, longer than the
60 s the fading lasts, so they are nearly independent. Under fading of
σ = 4 dB (the scenarios' figure) a mean of two heartbeats has σ/√2, and
the difference of two such means σ√(1/2 + 1/2) = 4 dB. The neighbour
table's average lets each new frame count half, which varies less than a
mean of two (σ²/3 against σ²/2), so 8 dB is a little more than two
standard deviations there.

**Asleep.** Between its windows a sleepy node receives duty-cycled,
which catches no heartbeat with the ordinary preamble, so it hears its
anchor's rotating heartbeat only if it listens when the rotation brings
it to its channel. It does: it keeps the anchor's neighbour entry
whatever its age, and wakes for the anchor's next heartbeat on its
channel, at the time the anchor's interval gives
([heartbeat](#heartbeat)). It listens from as early as the heartbeat can
come to as late, and sleeps again as soon as it hears it:

- It keeps the time it expects from the earliest heartbeat heard, moved
  later only by the drift of two clocks, ±50 ppm each, since a late one
  says only that the anchor was busy.
- As early is the time expected less three times the drift since the
  last heartbeat heard, at most 59 ms over a rotation: once for the
  drift itself, and twice because a heartbeat held up by a busy anchor
  may have moved the time expected a drift later while its slot was a
  drift early (roadmap D52). As late is the drift, the heartbeat's
  greatest delay ([heartbeat](#heartbeat)) and its airtime more.
- A heartbeat of the anchor heard before the watch opens is no slot's:
  an answer to a node without an anchor on its channel
  ([sleepy nodes alone](#sleepy-nodes-alone)), or the one after a move
  ([channel moves](#channel-moves)). It shows the anchor alive and moves
  nothing (roadmap D62).

Hearing the heartbeat is how the node knows the anchor is alive, and how
it follows it to a new home channel ([channel moves](#channel-moves)):
the heartbeat names the anchor's home, and the next on the new channel
comes when the rotation brings it there.

**Checking the anchor.** Two rotations without the anchor's heartbeat
mean it is gone: the node listens a worst-case rotation, as at its first
choice, keeps the anchor as soon as it hears it, and else takes the
strongest relay it heard. One that heard nothing at all is out of reach,
in a pocket, a basement or the metro, not without its anchor: it keeps
the anchor, watches for it as before, and checks again only once it has
heard any frame again. One that heard frames but no relay keeps it too:
giving it up for anchorless mode ([sleepy nodes alone](#sleepy-nodes-alone))
makes nodes at the edge of a relay's reach switch to and fro (roadmap
D51).

A node also checks after moving farther from where it last chose than
takes the anchor's signal down by the switching margin:
`r × (10^(8 / 36) − 1) = 0.67 r` for an anchor r away under the
scenarios' path-loss exponent of 3.6, and no less than 50 m, the GPS
allowance ([position](#position)). Between its windows a node hears no
relay's heartbeat, so it switches by the rule above mainly while it
checks. A node back from the dark keeps its anchor: a new one would
strand the messages sent to the old address (roadmap D46, D47).

**An anchor that turns carried or sleepy.** A node that hears its
anchor's heartbeat with the carried or the sleepy flag (roadmap D71)
leaves it: at once for the strongest other relay that is live in its
table and not carried, else for the strongest a rotation's listen finds,
as when it checks the anchor; with none, it is anchorless
([sleepy nodes alone](#sleepy-nodes-alone)). The carried anchor's
heartbeats do not end that listen. A check that hears neither the anchor
nor another relay leaves an anchor last heard sleepy or carried, where
it would keep one merely unheard. The node's address changes as at any
switch; what was sent to the old one reaches it by its anchor's flush
([delivery](#delivery)), or is lost unless its sender learns the new
address.

### Beats

A sleepy node with an anchor heartbeats on its home channel, the
anchor's, where the anchor and any other node on that channel hear it:
a **beat**. The heartbeat has the sleepy bit
([packet formats](#packet-formats)): reach the sender cheaply in its
window after its beat, which its holder does, or at any time with the
long preamble, a last resort ([next hop](#next-hop)).

Each beat comes a random half to one **beat period** after the last,
P = 300 s: half the delivery TTL, so that two beats are never more than
a TTL apart. A holder that keeps a node for a TTL then loses it only
after two lost beats, and keeps a node that has gone no longer than its
messages ([heard table](#heard-table)). The spread keeps two nodes whose
beats met once from meeting again. At the mean interval of 225 s a node
beats 16 times an hour, 189 ms of airtime. The interval is random, not
fixed, since nobody wakes for a beat: a holder hears it as it listens.

After it chooses or changes its anchor a node beats at once and once
more within a heartbeat interval, 5–15 s, so that one lost beat does not
hide it from its new holder for a period. After it follows its anchor to
a new home channel ([channel moves](#channel-moves)) it beats there once,
at a random time within a heartbeat interval, 0–15 s. Without it, its
holder knows only the old channel, and has no window to send in until
the node's next beat, up to a period later; every node of the anchor
follows on the same heartbeat, so beating at once they would meet
(roadmap D57).

**To anchorless neighbours.** A sleepy node whose neighbour table holds
anchorless neighbours ([sleepy nodes alone](#sleepy-nodes-alone)) sends
each beat also on each channel they listen on, with the long preamble.
They receive duty-cycled and would not hear a short beat, and the long
one, with the sleepy bit alone, tells them of a last resort they can
reach with the long preamble, not of an anchor. It hears them because
their beats carry the long preamble and step through every channel.

It does so for a neighbour only once it has heard that neighbour's
anchorless beat in two consecutive rotations of its beats: at least half
a rotation apart, 13 beats each at least half an anchorless beat period
after the last, 300 s, and at most a rotation and a heartbeat's lag,
601 s. It stops when the entry expires
([neighbour table](#neighbour-table)) or the neighbour is heard with the
sleepy bit alone. A neighbour heard anchorless once may be switching on,
and will anchor within minutes. One still anchorless a rotation later is
in a pocket with no relay, whose nodes mostly know their way out
already, from the time both were anchorless, so waiting a rotation costs
them little. Each such beat is a long heartbeat, 366 ms
([duty-cycled receive and the long preamble](#duty-cycled-receive-and-the-long-preamble)),
5.9 s an hour per channel at sixteen beats (roadmap D58).

A sleepy node may have moved since its last beat, so it refreshes its
GPS fix before each beat. GPS goes off after a fix, and off after 10
minutes without one (searching under jamming draws ≈27 mA, ≈1.3 days).
A sleepy node with an anchor does not sweep the channels
([busy estimate](#busy-estimate)) or move its home channel by the busy
estimate ([channel moves](#channel-moves)): receiving duty-cycled, it
could not.

### Heard table

A node that is not sleepy keeps the sleepy nodes with an anchor it hears
beat in its **heard table**: 256 entries, by ID, each with the node's
home channel, until when it listens ([delivery](#delivery)), whether a
delivery to it went unanswered since its last beat, and the mean RSSI of
its last two beats. An entry is kept for two beat periods, the delivery
TTL, after the last frame heard from the node: a beat, its own DATA,
whose `aux` also names its home channel ([packet sender](#packet-sender)),
or its ACK of a delivery. A node that answers is in reach whether or not
its beats are heard, and a busy holder misses beats, while it transmits
or when another frame meets the beat there. A node reached only in the
windows its beats open renews its entry with each beat; one kept awake
by a flow longer than a TTL is renewed by its ACKs, since a holder at
its cap misses beats (roadmap D53).

When the table is full a newcomer takes the place of an expired entry,
else of the one heard longest ago. A node that beats now listens now, so
a holder for more nodes than its table keeps still sends each its mail
after each beat, and a full table does not leave the nodes it has no
place for without their mail until it expires (roadmap D61). 256 covers
≈80 sleepy nodes per relay expected at indoor density in Kyiv with 10 %
adoption, 400 at 50 % ([capacity and sizing](#capacity-and-sizing)).

### Delivery

A message that reaches the owner of its position ([owner](#owner)) and is
for another node waits in the owner's delivery queue
([delivery queue](#delivery-queue)); while the owner hears that node
beat, it is the node's holder. It goes to the node as DATA addressed to
the node's ID, in `dst_hop` and `dst_id` alike, on the node's home
channel from the heard table, with the ordinary preamble, and the node
answers with an ordinary ACK ([the ACK](#the-ack)). Admission is
unchanged: the last hop is not fresh work
([receiving DATA](#receiving-data)).

A delivery goes through the send queue as any hand-over
([hand-over](#hand-over)), but ahead of the rest of the queue. A
hand-over waiting between its attempts, for the channel, a retry or the
bucket, steps aside for it and keeps its place; a return keeps the
attempts it has made.

**The window.** A delivery goes only while the node listens: for a
window of 3.37 s from each beat the holder hears, long enough for one
hand-over of a DATA that goes first to complete. That is the attempt the
holder may have under way, a full frame and the ACK timeout, 238.6 ms,
then the DATA's hand-over with all its attempts and retry waits, 3.13 s
([hand-over](#hand-over), [retry wait](#retry-wait)). A hand-over cut
short by the window would leave its message to wait a beat period
(roadmap D50). A node whose beat brought nothing goes back to receiving
duty-cycled after that.

Having answered a DATA, the node listens one hand-over more, 3.13 s, and
its holder knows it does: nothing is under way ahead of the next message
for it, which goes then. The node goes back to receiving duty-cycled
once a hand-over passes with no DATA for it. So the holder sends what it
holds one after the other on one beat, a message that arrives meanwhile
goes at once, and a flow to the node less than a hand-over apart keeps
it awake while it lasts. A repeat of a DATA whose ACK was lost finds the
node still listening. Only the holder's bucket caps a burst
([duty cycle cap](#duty-cycle-cap)): once the bucket would keep the next
message past the hand-over, the node leaves its window and the rest wait
for its next beat. A delivery that goes unanswered marks the node out of
its window, and its messages wait for its next beat.

The DATA carries the hop count as any hand-over ([hop count](#hop-count))
and the time the message had left when it arrived: the wait in the
delivery queue does not count against it
([time to live](#time-to-live)).

So a message waits for its node a mean of about two minutes, at most a
beat period. A node listens 3.37 s after each beat, 54 s an hour, 3.13 s
after each message it takes, and for its anchor's heartbeat, under a
second a rotation, and receives duty-cycled between
([duty-cycled receive and the long preamble](#duty-cycled-receive-and-the-long-preamble)).
What it listens besides, checking its anchor after the dark, depends on
how often it is out of reach (roadmap D47, D48, D50, D58).

**Outside the windows.** Two kinds of delivery go outside the windows,
after the deliveries in a window and one at a time.

- **The flush.** From its first far fix ([position](#position)), or the
  end of its grace on battery ([sleepy nodes](#sleepy-nodes), roadmap
  D71), a node sends each message in its delivery queue once, as DATA
  with the long preamble on the channel its node listens on, from the
  heard table, or on its own home channel if it has not heard the node.
  It is one attempt, through the bucket as any send. A message whose
  ACK comes is done; the rest stay and go at beats as before until their
  place is needed. The node is leaving: once it has gone its sleepy
  nodes' beats no longer reach it, and the long preamble reaches them
  now, whatever their windows. Each message is flushed once, so a still
  node whose fixes stray past the allowance again pays no more. The
  cost is bounded: a full queue, 41 messages of 492.9 ms, is 20.2 s of
  airtime, three and a half buckets, paced by the bucket.
- **A node heard as a relay.** A holder that hears a heartbeat without
  the sleepy flag from a node it holds messages for, carried or not,
  sends them at once, one after the other, with the ordinary preamble
  on the home channel the heartbeat names. Each is done on its ACK;
  unanswered ones wait for the node's next heartbeat or beat heard. Such
  a node listens all the time and opens no window, so its messages
  would otherwise wait until their place is needed (roadmap D66). It is
  a node that slept and now relays, or a relay whose messages a
  neighbour within 50 m owned because it did not keep it in its table
  ([neighbour table](#neighbour-table)).

### Delivery queue

Kept by nodes that are not sleepy, up to 41 messages per node. A message
queued the delivery TTL ago or longer is **stale**: it stays, and goes at
its node's next beat as any other, until a message arrives at a full
queue; then the oldest stale message makes way for it. Age alone drops
nothing: a message dropped when no other needs its place is lost for
nothing, and its node may yet beat, back from a pocket or a basement, or
heard at last by a holder that missed its beats. The queue's TTL starts
afresh when the message arrives: it is the wait for a node that sleeps
or is dark, not the way there ([time to live](#time-to-live)). When the
queue is full and holds no stale message, a message for another node is
refused ([receiving DATA](#receiving-data)): what is queued stays, and
the sender learns at once.

A queued message goes to the send queue one per node at a time, oldest
first: when it arrives, when the node beats, when the node takes the one
before, and whenever a hand-over ends and leaves room in the send queue;
there it goes first ([delivery](#delivery)). In the node's window it
needs a place in the regular room ([send queue](#send-queue)) and, in the
bucket, its own frame and the reply reserve, not the airtime of
everything queued, which it goes ahead of. Counted behind the forwards,
it would be kept out by a holder at its cap while the node listens, and
wait a beat period or expire (roadmap D55). One the bucket cannot fit yet
goes the moment the bucket will. One whose node has stopped listening by
the time its hand-over would start waits for the next beat. A failed
delivery counts no failure against anyone: the node is not a neighbour
the holder routes through.

Nodes in one train or one basement go dark together, so the queue holds
what arrives in a TTL for every node a holder can keep, its heard
table's 256 ([heard table](#heard-table)), at the 23 texts a day each
that the network carries ([capacity and sizing](#capacity-and-sizing)):
256 × 23 / 144 ten-minute TTLs in a day = 40.9, so 41 messages. A flow
to one node that pauses longer than a hand-over waits for the next beat,
at most 41 messages a period, and loses what does not fit.

The owner remembers 41 delivered message IDs
([packet formats](#packet-formats)) and ACKs a repeat without delivering
it twice. When it needs the place, it forgets the one delivered or last
repeated longest ago; time alone forgets nothing. 41 is the delivery
queue's size, for the same reason.

Nothing covers a node that has moved out of reach of every holder that
has its mail: the old holder does not forward to the new anchor, the
mail waits there until its place is needed, and the sender is not told
([addresses](#addresses)).

### A sleepy node's own message

A sleepy node with an anchor hands its own message to the anchor while
the anchor is live, with count 0 ([hop count](#hop-count)), and
otherwise to its next hop as any node does ([next hop](#next-hop)); one
without an anchor sends as a relay does
([sleepy nodes alone](#sleepy-nodes-alone)). Handed to the neighbour
nearest its destination instead, the first hop would often be a faint,
far relay (roadmap D46). The anchor takes it only as fresh work
([receiving DATA](#receiving-data); roadmap D31, D34), and the node
takes a message from its application only while the anchor is not
banned.

A refused message goes back to the application with the wait the
refusal asks for ([bans](#bans)), and the node takes no message of the
application's until that wait is over; the application may offer it
again then, or give it up, and tell the person either way. Held by the
node instead, a refused message aged at its origin until its time ran
out.

One whose hand-over goes unanswered is tried again a fade later, 60 s,
doubled for each unanswered hand-over to the anchor in a row, or when
the anchor's ban ends if that is later. A fade lasts about a minute (the
scenarios' correlation time), and the three attempts of one hand-over, a
few hundred milliseconds apart, all fall inside it, where the ban's
ladder would offer it every few seconds to a node whose channel is
full. Silence says no more than that the channel or the anchor is full
([bans](#bans)), so the wait doubles as the ban does: tried again every
fade instead, a text its anchor never takes would cost its sender three
frames a minute on the anchor's channel for its whole TTL, where the
anchor hears its other nodes' beats (roadmap D54).

A message that finds no live neighbour at all waits for one, routed
again every 60 s, until its time runs out
([time to live](#time-to-live)): a node in a pocket loses its
neighbours by going dark and finds them again. The application is not
told ([addresses](#addresses)).

### Sleepy nodes alone

A sleepy node that hears no fixed relay at the end of its boot listen
([channel at boot](#channel-at-boot)) has no anchor: it is in
**anchorless mode**. It knows no node that holds mail, so no address
can point at a node that holds its mail and hears its beat. Instead it
does a relay's work where there is none: it forwards at once what it is
given, with the ordinary time to live, its address is its own position
and it owns it, and a DATA for it does not wait for a beat. It receives
duty-cycled, as every sleepy node does between its windows, and has no
window: every frame to it carries the long preamble. It keeps the home
channel it chose at boot; it does not sweep or move.

It beats with the sleepy and anchorless bits and the long preamble, each
beat a random half to one **anchorless beat period** after the last, on
the next channel of the rotation each time ([heartbeat](#heartbeat)),
naming its own channel in `aux`. The period is the delivery TTL over the
13 channels, 600 / 13 = 46.2 s: beats at most a period apart visit every
channel within a TTL, so a relay in reach on any channel hears the node,
and two nodes in anchorless mode find each other, before anything the
network holds for them runs out ([sleepy nodes](#sleepy-nodes)). At the
mean interval of 34.6 s the beats cost 38 s an hour of airtime, 11 % of
its budget, and 0.34 mA at 32 mA of transmit current.

Nodes in anchorless mode route among themselves and are a next hop like
any other ([next hop](#next-hop)), each DATA to one with the long
preamble. A relay that hears an anchorless beat answers with a
heartbeat of its own on the channel the beat names, with the long
preamble, at once and again a fade later, 60 s
([a sleepy node's own message](#a-sleepy-nodes-own-message)), for two of
the node's beat periods, so that one lost answer does not cost a
rotation. A carried node does not answer, since nobody anchors to it,
nor does a relay whose grace on battery is over
([sleepy nodes](#sleepy-nodes)). A receiver cannot tell an answer from
a heartbeat in its slot by the frame; a sleeping node tells them apart
by the time ([relay choice](#relay-choice), roadmap D62).

At the first relay it hears the node anchors as any sleepy node does
([relay choice](#relay-choice)) and leaves anchorless mode. Its
neighbours then keep it as a last resort ([next hop](#next-hop)), and
the anchorless ones behind it learn where it listens from its long beats
on their channels ([beats](#beats)) and from the frames they exchange
with it.

---

## Phone link

BLE 5.0 GATT service on the nRF52840 (TrouBLE, roadmap D5), with
characteristics for send, receive-notify, node status and position set;
there is no sleepy or carried setting ([nodes](#nodes), roadmap D71). The app owns
identity and end-to-end encryption; the node sees opaque payloads. The
sender's address, position and node ID, travels inside the encrypted
payload, so that only the recipient learns where the sender is.
Encryption scheme: open item O1.

A message has no ID field: the network identifies it by a hash of its
destination and payload ([packet formats](#packet-formats)). Two
messages to one destination with identical payloads are one message to
every relay that still keeps the first one's record
([message records](#message-records), [bounds](#bounds)), and the second
is not delivered. The app must make every payload unique; end-to-end
encryption with a fresh nonce per message does that.

**What the network does not defend against.** Nothing on the air is
authenticated. Anyone with a 433 MHz LoRa radio can send heartbeats with
any position, attract traffic to it and drop it, send packets in another
node's name, or replay what it has heard. The app's end-to-end
encryption keeps payloads private and authentic; it does nothing for
routing. This is the scope stated at the top.

---

## Capacity and sizing

Rationale, not specification.

Kyiv model: ≈840 km², ≈400 km² built-up, ≈3 M people. Indoor coverage
needs ≈9.6 relays/km² (≈3,800 relays); outdoor-only ≈1.9/km² (≈750).
Rooftop relays hear ≈3 km ([physical layer](#physical-layer)), so a
collision domain, the area within which transmissions interfere, is
≈28 km² with ≈270 relays, ≈14 domains across the built-up city.

One 125 kHz channel at SF5 carries 1.8 kB/s raw and ≈640 B/s with
listen-before-talk (≈35% usable). A 100-byte text over 8 hops with ACKs
takes ≈0.67 s of channel time: per hop a 122-byte DATA packet (69.4 ms)
and its ACK (14.4 ms).

| Plan | Heartbeat load per channel | City-wide data | Texts/s | Texts per user per day at 300k users |
|---|---|---|---|---|
| Single channel, 60 s heartbeats | 8% | 7 kB/s | 7 | 2 |
| 7-colour fixed grid, heartbeats on all colours | 11–14% | 37 kB/s | 37 | 11 |
| **13 channels, measured selection, rotating heartbeat (this design)** | **≈2.5%** | **≈85 kB/s** | **≈80** | **≈23** |

These are orders of magnitude; the hearing radius and hop count are each
uncertain by 2×. The table assumes a text crosses the city. The city
centre is the hot spot, and the per-relay 10% cap
([duty cycle cap](#duty-cycle-cap)) binds there first.

Sleepy nodes' beats cost what [beats](#beats) lists, 189 ms an hour
each, and relays nothing for them. Summed over the domain's 20 000
sleepy nodes they take 1.05 s of airtime a second, 8 % of one channel's
time over the 13, where every relay hears every other. A sleepy node in
anchorless mode costs more ([sleepy nodes alone](#sleepy-nodes-alone)):
its long beats, a relay's answers, and the long preamble on every DATA
to it.

---

## What the protocol omits

Common mesh and radio mechanisms left out, and why.

| Omitted | Instead | Why |
|---|---|---|
| Neighbour lists in heartbeats and a network-wide node table | a 9-byte heartbeat; routing uses only direct neighbours | spreading tables through the network does not scale to thousands of relays at LoRa rates; addresses are positions, so there is nothing to look up |
| Addressing by ID with a lookup, or registration with a home relay | position + node ID; the owner resolves the ID locally ([forwarding](#forwarding)) | registration traffic grows with users; position addressing needs none |
| Time slots on a shared schedule | always listen, transmit as soon as listen-before-talk clears | no time sync exists, and slots add latency |
| Cooperative position estimation | GPS or a map point ([position](#position)) | every node knows its own position |
| A fixed channel grid, heartbeats on all channels | a measured home channel and a rotating heartbeat ([home channel selection](#home-channel-selection)) | 49% of every channel at indoor density; where a rooftop relay hears ~3 km and a relay covers ~700 m, a grid only spreads load evenly, which measurement does as well while adapting to hot spots and foreign interference |
| GFSK at 50 kbps | LoRa at SF5 ([physical layer](#physical-layer)) | 3× more per channel than SF5 at 40% of the range, and needs a modem driver and error correction written from scratch; revisit only if relays saturate at SF5, since nothing above the modem would change |
| Spreading one signal over the whole band (DSSS/CDMA) | 13 separate channels | the SX1262 cannot, and in random meshes without power control separate channels carry 5–10× more (Weber et al., [sources](#sources)) |
| Frequency hopping | measured home channels | hopping on a common schedule has the capacity of one channel; per-receiver schedules are fixed channels plus time sync; random channels per packet need ≥20 ms preambles on every packet; no regulatory benefit at 433 MHz |
| Sleepy nodes heartbeating as relays do, every 10 s | a beat every 150–300 s ([beats](#beats)) | collisions happen at the shared relay whatever the sleepy node's range: 72% of a channel at 400 sleepy nodes per relay |

---

## Packet formats

The first byte of every packet is `type:4 | aux:4`; `aux` is the home
channel in heartbeats and type-specific flags elsewhere. A position is
7 bytes, the heartbeat's short position 4
([positions on the wire](#positions-on-the-wire)), and a node ID 4
([node identity](#node-identity)). The LoRa hardware CRC covers
everything.

```
HEARTBEAT 0x8 to 0xF: first(1) id(4) short_pos(4)                          = 9
          type bit 0 = sleepy (0x9); bit 1 = anchorless, only with sleepy
          (0xB); bit 2 = carried, with any of these (0xC, 0xD, 0xF;
          roadmap D70, D72); aux = home channel, 0 to 12, in all
          four bits
ACK       0x3: first(1) id(4) from(4) to(4)                                 = 13
          the answer to a DATA that takes it; sent on the channel in
          the answered packet's aux; aux = 0
          from, to = the IDs of the answering and the answered node
NACK      0x7: first(1) id(4) from(4) to(4) wait(1)                         = 14
          the answer to a DATA that refuses it, as the ACK; wait = how
          long the refuser asks its sender to wait, in bans of 1.56 s,
          rounded down ([bans](#bans)); aux = 0
DATA      0x5: first(1) dst_hop(4) src_hop(4) dst_pos(7) dst_id(4) hops(1)
               time_left(1) payload                                         = 22 + payload
          dst_hop, src_hop = IDs of the receiving and the sending node;
          aux = sender's home channel, so the receiver knows where to ACK;
          time_left = the time the message has left, in 2.5 s, rounded down,
          stamped at each transmission ([time to live](#time-to-live)); 240 is the 600 s TTL
Types 0x1, 0x2, 0x4, 0xA and 0xE are unused; nodes reject them like any unknown type.
Type 0x6 is reserved for the bench frames of the
firmware's measurement console (`bark-fw/README.md`); nodes reject it like any
unknown type (roadmap D70; 0xF before).
```

The heartbeat's flags tell others how to treat its sender. They are in
its type, since `aux` holds the home channel in all four of its bits and
the position fills the four bytes after the ID. The three flags take the
type's three low bits, and 0x8 is the one base whose eight codes, 0x8 to
0xF, miss the ACK, the NACK and DATA; the bench frames are at 0x6, which
no flag reaches. A heartbeat does not say where its sender's position
came from: no receiver needs to know.

- **Sleepy**, bit 0: "I sleep. Reach me cheaply in my window after my
  beat, or at any time with the long preamble." ([nodes](#nodes))
- **Anchorless**, bit 1: "I know no node I can anchor to, one that is
  neither sleepy nor carried. If you are one, tell me." ([nodes](#nodes))
- **Carried**, bit 2: "I move. I take each GPS fix and hold mail for
  nobody." ([nodes](#nodes))

The flags say what the node is, not what it asks, so each is set on its
own, except that anchorless needs sleepy: a node that listens all the
time is a relay and has no anchor to miss, so 0xA is no heartbeat. 0xC
is a relay on the move, and 0xD and 0xF a sleepy node on the move, with
an anchor and without.

The **message ID** is a 32-bit FNV-1a hash of
`dst_pos || dst_id || payload`, computed by every node that handles the
message; it is the same on every hop because none of these fields
changes. DATA does not carry it; the ACK does, to name what it
acknowledges. The app must keep payloads unique
([phone link](#phone-link)).

`dst_hop` lets every node on a channel discard packets not addressed to
it after 5 bytes. `src_hop` and `aux` name the sender and its home
channel, where the ACK goes and where a return is sent
([routing](#routing)). The payload is at most 255 − 22 = 233 bytes; the
sender's address is inside it, encrypted ([phone link](#phone-link)).

### Positions on the wire

A position is WGS-84 latitude and longitude in fixed point, 25 + 26 bits
and 5 spare in 7 bytes, ≈0.6 m, the same everywhere on Earth: one firmware and one
app, with no per-deployment origin. A 1 m grid over the Earth's
5.1 × 10¹⁴ m² needs 49 bits, so 51 bits is within 2 bits of the minimum;
decimetres would need 56. Plain fixed point keeps the distance
arithmetic trivial.

Routing needs distances, not coordinates: each node converts every
position it handles into metres on a flat plane centred on itself,
`Δy = Δlat · R` and `Δx = Δlon · R · cos(lat_self)`, with
`cos(lat_self)` a per-node constant. The error grows with
Δlat · tan(lat): at 50° N it is 0.1 % at 5 km and 0.4 % at 20 km, enough
to reorder only near-ties between candidate hops.

**Short position.** The heartbeat carries a node's position in 4 bytes:
the low 16 bits of its latitude count, then the low 16 bits of its
longitude count, big-endian. One count is 180° / 2²⁵ ≈ 0.6 m, so the low
16 bits repeat every 65 536 counts: 39 km of latitude and
39 km × cos(latitude) of longitude (25 km at 50° N), far beyond any
hearing radius (≈3 km, [physical layer](#physical-layer)). A receiver
rebuilds the full position as the one candidate within half a period of
its own on each axis. That holds to latitudes near 80°, where the
east-west period shrinks toward the hearing radius. DATA `dst_pos` stays
7 bytes because in a large city a node may be more than half a period
from the destination (roadmap D14).

---

## Implementation notes

**Rust stack:** `embassy-nrf` (nRF52840), `lora-phy` from git (SF5, CAD,
RSSI; roadmap D4), TrouBLE with the Nordic controller for BLE (D5), the
stock UF2 bootloader for flashing (D6). The protocol core does no I/O of
its own and owns its routing and tables (D1, D3; `core-interface.md`).
Crate layout is D7.

**RAM (256 KiB):** every board holds a relay's tables, the heard table
and the delivery queue included, since every node may turn relay
(roadmap D71); their sizes, and how they fit beside Embassy and the
stacks (≈16 KiB) in the 255 KiB the application has once 1 KiB is kept
for the boot record and the panic message (`bark-fw/memory.x`), are in
`core-interface.md`, Memory.

The bring-up order and the test rig are in `roadmap.md`, Phase 2.

---

## Open items

Resolved items keep their numbers and are in `roadmap.md`. The steps
are those of the bring-up order, `roadmap.md` Phase 2.

| # | Item | Owner/when |
|---|---|---|
| O1 | End-to-end encryption scheme in the app (key exchange, address-in-payload format) | before step 6 |
| O3 | Exact thresholds: busy threshold margin (6 dB), move hysteresis (0.05 / 10 min), demote count, ACK timeout | tune in steps 3–4 |
| O5 | Adjacent-channel rejection of the SX1262 at 125 kHz spacing, with a neighbour at full power; if inadequate, 11 channels at 150 kHz spacing, the same end centres, 433.170–434.670 MHz ([channel plan](#channel-plan)) | step 1 |
| O7 | Sending to an area (address = position + radius, node ID = broadcast) | later |
| O11 | Relay-side RSSI noise-floor calibration method and drift | step 3 |
| O12 | Confirm RAK WisMesh Repeater 433 availability for rooftop solar units | procurement |
| O16 | GPS wander of a still relay indoors: whether a node at a windowsill sees fixes beyond the 50 m allowance for 72 s in a row and turns carried ([position](#position)); if it does, a far fix beyond twice the allowance as well, which a walker passes in the same 72 s | step 2 bench, roadmap D70 |
| O17 | How a solar relay reports its power: its panel charges the battery by day and not at night, and a relay that turned sleepy every evening would move its sleepy nodes each time ([nodes](#nodes), [sleepy nodes](#sleepy-nodes)) | before rooftop boards, roadmap D71 |

---

## Sources

- ERC/REC 70-03, Annex 1 (non-specific SRDs), band g1 433.050–434.790 MHz
  (edition of June 2026; earlier editions label it f1).
- NCEC (Ukraine) decision 81 of 12.02.2025 "generalised conditions", row
  РІ 50-3-1, as amended by decision 212 of 06.05.2026. Consultation
  (Aug–Sep 2026) on 865–868 MHz does not affect 433 MHz.
- EN 300 220-2 for polite-access (listen-before-talk) shapes (not required in this band).
- Semtech SX1261/2 datasheet: LoRa sensitivity table, CAD, RSSI, SF5/SF6.
- Weber, Andrews, Yang, de Veciana, "Transmission capacity of wireless ad
  hoc networks with outage constraints", IEEE Trans. IT 2005 (frequency
  hopping vs direct sequence).

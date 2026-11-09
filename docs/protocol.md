# bark protocol: Civilian 433 MHz Mesh

**Status:** live specification. Decisions that refine it are in
`roadmap.md` with D-numbers; [open items](#open-items) lists what is still
to be decided or measured. Where the code and this text disagree, one of
them is wrong and the fix is a commit to both.

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
| Medium access | Listen on the home channel except to transmit (a sleepy node receives duty-cycled between its windows); acknowledgements (ACKs) come back on the sender's home channel, sent at once; everything else listens before talking, then transmits at once; no time slots |
| Heartbeat | A node's periodic 13-byte broadcast of its ID, position and home channel: a relay's every 5–15 s, fixed by its ID, on each of the 13 channels in turn, its first 13 after boot 1.1 s apart; a sleepy node's every 150–300 s on its home channel; an anchorless node's every 23–46 s, with a long preamble, on each channel in turn |
| Neighbour discovery | Passive: a node hears every relay in range once per 13 heartbeats (~130 s) on its home channel; a relay just switched on is heard by all within ~15 s, and those that hear it answer; no scanning, no schedules |
| Boot | A node takes traffic from the second it is switched on, and keeps its home channel and a sleepy node's anchor across a reboot |
| Routing | Forward to the neighbour closest to the destination **position**; route around gaps by depth-first search; ACK every hop; a neighbour that refuses is gone round for the wait it asks, one that fails to answer is waited for, and after three failures in a row with no answer it ranks behind the others; sleepy nodes forward only where no relay is in reach |
| Addressing | A position plus a 6-byte node ID: a relay's own position, a sleepy node's anchor's; no registration, no network-wide tables |
| Nodes | One kind, with two properties it measures of itself, each a heartbeat flag: sleepy on battery (a fixed relay only once its battery is low), carried when it moves; a relay is a node that is not sleepy ([nodes](#nodes)) |
| Phones | Bluetooth LE (BLE) GATT link to the node; the app encrypts end to end; the sender's position travels encrypted in the payload |
| Position | From GPS unless a fix fails the checks or the app sets a map point, and a sleepy node's anchor's while it has one; WGS-84 in 7 bytes (≈0.6 m), the same everywhere on Earth; distances in a flat local plane at each node |
| Time | No network time, and GPS time is not used |
| Updates | A new firmware image passes from relay to relay as coded pieces: heartbeats tell what each node holds and lacks, and a relay streams the blocks it holds where a neighbour lacks them, block by block, at most one piece in 4.16 s from a sender alone on a channel, 3.3 % of its time, and one in 13.86 s, 1 %, shared by senders that hear each other or have texts to send; the node's firmware decides which images it takes ([updates over the mesh](#updates-over-the-mesh)) |

---

## Hardware

**LILYGO T-Echo, 433 MHz build.** nRF52840 (Nordic, Norway), SX1262
(Semtech, US, EAR99), Quectel L76K GNSS, 1.54" e-ink, BLE 5.0, 850 mAh
Li-ion, USB-C charging, UF2 bootloader. Check that the LoRa module is the
433 MHz build, not the 868/915 build: the 868 board's antenna matching
makes 433 MHz operation lossy and out of spec.

Same chipset, same firmware, for rooftop relays:

- **RAK WisBlock** RAK4631 EU433 (PN 111000) + RAK19003/19007 base +
  RAK12500 GPS, in a solar enclosure.
- **RAK WisMesh Repeater Mini V2** (solar + 3200 mAh), if a 433 MHz build
  is available (O12).

*Why a radio from outside Europe.* The SX1262 is not European, but
anyone must be able to buy and plant a node, and no European part offers
nRF52840 + SX1262 integration off the shelf (ST's STM32WL55, the
European fallback, has no BLE).

---

## Nodes

Every node runs the same protocol and firmware. What it does is not a
setting: it follows two properties the node measures of itself, each on
its own, and its heartbeat publishes each as a flag
([packet formats](#packet-formats); roadmap D70, D71, D72).

- **Sleepy**: it is on battery, unless it is a fixed relay whose
  battery is not low ([sleepy nodes](#sleepy-nodes)). On external power, USB on a
  windowsill or a solar box on a roof, a node is not sleepy. Sleepy
  decides how the node uses its radio: it beats rarely instead of
  heartbeating, listens only in a window after each beat, receives
  duty-cycled between, forwards what it is given at once, anchors to a
  fixed relay, and keeps no mailbox.
- **Carried**: its GPS says it moves ([position](#position)). A map
  point set in the app makes it not carried at once. Carried decides how
  the node uses its GPS: it takes each fix as its position, as a sleepy
  node without an anchor does too. A node that is neither keeps the
  place it stands as its position. A sleepy node with an anchor takes no
  fix: its position is its anchor's, and it keeps the flag it has
  ([beats](#beats)). A carried relay is otherwise like any other.

*Why nothing to set.* The nRF52840 detects USB power itself, so a
person's node relays on its charger and sleeps in a pocket.

*Why a carried node takes each fix.* Its heartbeats then say where it
is, and its neighbours route to it there. A fixed node keeps its place so
that no later fix, true or spoofed, moves it (roadmap D63).

The two combine in four ways, all of them real, and each flag means the
same whatever the other is:

| | not carried | carried |
|---|---|---|
| **not sleepy** | a node on USB at a window | a node on USB in a car |
| **sleepy** | a node on battery left somewhere | a node on battery in a pocket |

Holding mail for sleepy nodes, owning a destination by its position
([owner](#owner)) and being a sleepy node's anchor
([relay choice](#relay-choice)) need neither flag; each flag takes all
three away. A node that holds no mail owns only what is addressed to its
ID, since owning a position means holding the mail addressed to it.

*Why either flag takes them away.* Mail is held only where a sleepy node
can count on finding it. A carried node would leave its sleepy nodes
behind, and as an anchor it would carry its sleepy nodes' addresses off
with it. It has no position at which anyone can address it for long, so
its senders learn where it is at the app layer
([addresses](#addresses)).

The rest of this document uses these words:

- A **relay** is a node that is not sleepy, carried or not: it listens
  all the time, heartbeats every 5–15 s on every channel, and forwards.
  A **sleepy node** is one that is sleepy, and a **fixed** node one that
  is not carried. Like "anchor" and "holder", these words name a state a
  node is in, not a kind of node.
- A sleepy node's **anchor** is the relay it chooses, a fixed one. The
  anchor's position is the sleepy node's address, its home channel the
  sleepy node's, and its heartbeat the one the sleepy node watches to
  know it is still in reach. A sleepy node that hears no fixed relay is
  **anchorless** ([sleepy nodes alone](#sleepy-nodes-alone)). A third
  heartbeat flag says so, and a relay sets it too while its table is new
  ([heartbeat](#heartbeat)).
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
  node for at least the **delivery TTL**, 600 s.

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

The protocol keeps to them with:

- an airtime token bucket per node that holds every hour within 10%
  ([duty cycle cap](#duty-cycle-cap));
- no transmission longer than 1 s: an SF5 packet is at most 139 ms,
  373 ms with the long preamble ([physical layer](#physical-layer));
- +10 dBm at the connector, which with the stock antenna (≈0 dBd gain)
  is at most +10 dBm ERP.

*Why 433 MHz.* It is the only band with identical rules in both
jurisdictions and enough range: 868 MHz has no 500 mW sub-band in
Ukraine, and 2.4 GHz too little range.

---

## Physical layer

RSSI (received signal strength) is the received power the radio reports.
CAD (channel activity detection) is the radio's check for a LoRa preamble
on the channel; it works below the noise floor.

| Parameter | Value | Note |
|---|---|---|
| Modulation | LoRa (chirp spread spectrum) | SX1262 native; `lora-phy` Rust driver exists |
| Spreading factor | 5, fixed network-wide | SF5/SF6 are SX126x-only; all nodes are SX126x. A compile-time constant |
| Bandwidth | 125 kHz | |
| Coding rate | 4/5 | |
| Raw bit rate | 15.6 kbps at SF5 | 5.5 kbps at SF7 |
| Symbol time | 0.256 ms at SF5 | |
| Preamble | 12 symbols (3.1 ms) + 6.25 symbols of sync; **long only to and from a node that receives duty-cycled**: 927 symbols (237.3 ms) | 12 is the SX1262 minimum at SF5 and SF6 (datasheet §6.1.4); the long one is derived below |
| 255-byte packet | 138.6 ms at SF5 | ≈400 ms at SF7. Airtime from the datasheet formula (§6.1.4), `time_on_air` in `radio.rs`: an n-byte packet takes its preamble, 14.25 symbols of sync and header, and 5 symbols for each 20 bits, or part of 20, of 8n + 16 |
| Sensitivity | ≈−120 dBm at SF5 | ≈−124 dBm at SF7 |
| Noise floor (125 kHz, noise figure ≈5 dB) | ≈−118 dBm | SF5 signals near sensitivity sit below the floor: RSSI cannot see them, CAD can |
| Sync word | 0x12, the private one, written 0x1424 to the SX126x (not LoRaWAN's 0x34) | other LoRa networks and this one ignore each other's packets |
| CRC | on (LoRa hardware CRC) | |
| Maximum packet (MTU) | 255 bytes | SX126x maximum LoRa payload |
| Range, relay↔handheld, street | ≈750 m at SF5, ≈1 km at SF7 | |
| Range, relay↔handheld, indoors | ≈300 m at SF5, ≈400 m at SF7 | |
| Range, rooftop↔rooftop | several km | rooftop relays hear ≈3 km; this sets the size of a collision domain ([capacity and sizing](#capacity-and-sizing)) |

*Why SF5.* It carries about 3× the bytes per unit of airtime of SF7
(the common LoRa default) for a quarter less range. LoRa brings an
existing driver, hardware error correction, CRC and whitening, CAD, and
~15 dB of processing gain against the key fobs and LPD433 handhelds that
share the band. Why not GFSK:
[what the protocol omits](#what-the-protocol-omits).

*Why network-wide.* An SX1262 receives one SF at a time, so a mixed-SF
network splits into parts that cannot hear each other. Starting at SF7
would force a network-wide switch later; the deployment is expected to
start at a density where SF5 range suffices
([capacity and sizing](#capacity-and-sizing)).

### Duty-cycled receive and the long preamble

Every frame has the shortest preamble except those to a node whose radio
receives duty-cycled: a sleepy node outside its windows
([sleepy nodes](#sleepy-nodes)), which without an anchor is always
([sleepy nodes alone](#sleepy-nodes-alone)). Long frames go only where
the receiver cannot be reached otherwise:

- DATA to a sleepy node outside its window;
- the beats of a node without an anchor, and a relay's answers to them;
- a sleepy node's beats to its anchorless neighbours.

Their cost is in [sleepy nodes alone](#sleepy-nodes-alone).

The SX1262 receives duty-cycled on its own (datasheet §13.1.7,
SetRxDutyCycle). It receives for a window `rx`, takes about 1 ms to
save and restore its state, sleeps for `sleep`, starts its oscillator
(the TCXO, with a delay the firmware probes at boot and keeps in use,
0.5 ms on the bench's board at room temperature, roadmap D88), and
receives again: a cycle `C = rx + sleep + wake`. The cycle is derived
for `wake` = 1.5 ms, the 1 ms and a delay of 0.5 ms, the least the
probe can land on (roadmap D92).

The long preamble is one length for the whole network, but a node's
delay is its own and rises when the board gets cold. What every node
keeps is the **gap** it is deaf for between two windows, `sleep + wake`:
a node whose delay is longer than 0.5 ms takes the excess off its sleep
and adds half of it to its window (`bark-proto/src/sniff.rs`,
`periods`). So every sleepy node is reached with the same preamble:

| Quantity | Value |
|---|---|
| Window `rx` | 3.75 ms (240 steps) |
| Sleep | 233.6 ms (14 952 steps) |
| Gap | 235.1 ms |
| Cycle | 238.9 ms |
| Long preamble | 927 symbols, 237.3 ms |
| Mean current while receiving duty-cycled | 0.129 mA (the sleep at 1.2 µA, Table 3-5) |
| Heartbeat with the long preamble | 248.6 ms |
| 255-byte frame with the long preamble | 372.8 ms, under the 1 s of [regulatory limits](#regulatory-limits) |

*Why the long preamble is that long.* The radio catches a preamble only
if a window holds `D` of it, the time the modem takes to lock, taken
here as 4 symbols (≈1 ms; to be measured in bring-up step 1,
`roadmap.md` Phase 2). A preamble that starts just too late for one window must reach
`D` into the next, so it is caught wherever it starts if it is at least
`gap + 2D` long.

*Why that window.* Once it has detected a preamble, the radio waits
`2 rx + sleep` for the header (datasheet §13.1.7: "Tpreamble + Theader ≤
2 × rxPeriod + sleepPeriod"), which ends 14.25 symbols after the
preamble. So the window must be at least `(wake + 2D + header) / 2`, and
half a symbol more to round the preamble up to whole symbols: `rx` =
3.75 ms, in the radio's steps of 15.625 µs. The header's wait and the
two detections set most of it: with no wake at all it would still be
3 ms.

*Why that sleep.* The sleep follows from the current. The node pays
`I_rx (rx + wake) / C` for its windows, at the 5.8 mA of listening, and
`I_tx C / P` for its own beats, each a preamble about `C` long every
`P`, at 32 mA (datasheet Table 3-6, the least listed for the SX1262 at
434 MHz). The sum is least where the two are equal,
`C = √(I_rx (rx + wake) P / I_tx)`: 238.9 ms at `P` = 60 s, and within
7 % of that from 165 to 345 ms.

Without an anchor a node's beats come every 34.6 s, where the least is
181 ms and 238.9 ms costs 4 % more; with one they come every 225 s
([beats](#beats)), where the windows dominate whatever the cycle.

*Why ordinary traffic keeps the shortest preamble.* A long frame costs
its sender and its channel 17 times a heartbeat's airtime and 2.7 times
a full frame's. Every node on a relay's channel would pay the long one
on every frame, and a relay forwards at the 10 % cap long before its
channel fills ([duty cycle cap](#duty-cycle-cap)).

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
([home channel selection](#home-channel-selection)). Channel 6 is
centred on 433.920 MHz, the busiest spot in the band (key fobs,
sensors); the choice avoids it wherever it is busy.

Why not a grid: [what the protocol omits](#what-the-protocol-omits).
Whether 125 kHz spacing is enough is open item O5
([open items](#open-items)).

---

## Home channel selection

Every node has one **home channel**, on which it listens whenever it is
not transmitting. All traffic *to* a node is sent on its home channel.

### Busy estimate

Per channel, a node keeps `busy[c]`, an exponential moving average of a
0/1 busy reading. Foreign transmitters (key fobs, LPD433 handhelds)
count exactly like peers.

| | |
|---|---|
| reading | 1 if RSSI > noise floor + 6 dB (≈−112 dBm), else 0 |
| a reading of another channel | taken four symbol periods, 1.02 ms, after the receiver starts on it |
| a reading of the home channel | taken as the radio listens, without a restart |
| sweep | one channel a second, channels in turn: each channel every 13 s |
| window | α = 1/138 per sample, 30 minutes at one sample per channel every 13 s |
| noise floor | the 10th percentile of every reading, all channels pooled in 1 dB bins, each bin halved whenever they hold two windows of samples, about an hour; −118 dBm before the first reading (open item O11) |

A reading of another channel takes 1.9 ms off the home channel
(roadmap D88):

| | |
|---|---|
| retune there: standby, the frequency, receive | 0.39 ms |
| four symbol periods | 1.02 ms |
| the read | 0.05 ms |
| retune home | 0.39 ms |

A reading is taken only when the modem reports no preamble detected and
no packet in progress. On its home channel a node counts only what
would stay behind if it moved:

- A home reading taken while DATA addressed to the node, the ACK of one
  of its own hand-overs, or its own transmission was on the air is set
  aside. Every other is the same 0/1 reading as on any other channel.
- Whether a reading fell in the node's own packet is known only when
  that packet ends, so a home reading is held until the next, 13 s
  later, longer than any packet (roadmap D24).
- The radio keeps receiving while it reads its own channel
  (`core-interface.md`, Radio, rules 7 and 8): a home reading inside a
  packet addressed to the node is discarded, one inside any other
  counts. A packet for the node that fails its CRC cannot be told from
  someone else's, and counts.

*Why the node's own traffic is set aside.* DATA addressed to the node,
the ACKs of its own hand-overs, and its own transmissions go wherever it
goes: counted, they would make a busy relay leave its channel for its
own traffic and take that traffic along. The move rule compares home
with the others, so all are read alike (roadmap D55).

*Why not the dBm.* The average is of the 0/1 reading, not of the dBm:
what matters is the fraction of time listen-before-talk would refuse.

*Why foreign transmitters count.* What hurts a node is busy time at its
own receiver, which RSSI measures directly; counting neighbours per
channel would not, since two neighbours that hear each other share a
channel cleanly.

*Why four symbol periods.* The receiver's RSSI is not right at once:
it reads nothing for about 150 µs, then climbs a step each symbol
period, and a strong signal reads as one fixed value whatever its level
until about 1 ms. From four symbol periods it reads the level, and the
settled home channel reads the same, so home needs no restart (roadmap
D86, `bench-2026-10-30.md`). Read sooner, every other channel looks
quieter than home, and a node moves at every chance the move rule
gives ([channel moves](#channel-moves)).

*Why one channel a second.* A reading of another channel deafens the
node for `g` = 1.9 ms. The modem needs a length `D` of preamble to lock
on, four symbols
([duty-cycled receive and the long preamble](#duty-cycled-receive-and-the-long-preamble)),
so a gap loses a packet whose preamble starts too late before it for
the modem to have locked, or too early in it to leave `D` after it: a
window of `g + 2D − P`, with `P` the 12-symbol preamble, 3.1 ms
([physical layer](#physical-layer)). That is 1.9 + 2.0 − 3.1 = 0.8 ms
a reading.

| | one channel a second | every channel each second |
|---|---|---|
| deaf | 1.9 ms a second, 0.19 % | 24 ms, 2.4 % |
| packet starts lost | 0.8 ms a second, 0.08 % | 11 ms, 1.1 % |
| samples a channel in a window | 138 in 30 minutes | 600 in 10 minutes |

Thirteen readings a second would cost every relay 1 % of what it hears;
one a second costs a thirteenth of that. The price is fewer samples: 138
a channel take 30 minutes, which only slows moves, and moves are meant
to be rare ([channel moves](#channel-moves)). 138 samples give ±2.6
points on a 10 % channel, half the difference the move rule acts on
(roadmap D86).

### Channel at boot

A node takes part from the second it is switched on. Its **first
rotation** is the worst-case rotation that follows
([heartbeat](#heartbeat)), 195 s, by when it has heard every relay in
range and has the sweeps of a rotation; a sleepy node's is two, 390 s:

| | |
|---|---|
| home at boot | the one it stored before the reboot ([implementation notes](#implementation-notes)), else a random channel |
| from boot | it sweeps, takes and forwards traffic, and a relay heartbeats, its first rotation's heartbeats 1.1 s apart ([heartbeat](#heartbeat)) |
| end of the first rotation, without a stored home | it moves to `argmin busy[c]` unless its home is within 0.01 of it; channels within 0.01 of the lowest count as tied, and the choice among them is random |
| a sleepy node | listens: at the first relay it hears it ends its first rotation and takes the relay as its anchor ([relay choice](#relay-choice)); with none by the end of two rotations it is anchorless ([sleepy nodes alone](#sleepy-nodes-alone)) |

A config override fixes the home for bench work.

*Why a random channel.* Nodes that hear nothing, and nodes switched on
together after a power cut, then do not all start on channel 0.

*Why 15 readings suffice.* The first rotation reads each channel 15
times ([busy estimate](#busy-estimate)), ±8 points on a 10 % channel:
enough to keep off a channel an interferer or a crowd of relays holds,
not to rank quiet channels against each other. Ranking them is the move
rule's, on its 30-minute window.

*Why it keeps a home among the quietest.* The move at the end is a move
like any other ([channel moves](#channel-moves)): neighbours that missed
its announcement send to the old channel for up to a rotation. A home
as quiet as the quietest gains nothing by it.

*Why two rotations for a sleepy node.* A listening receiver loses about
one heartbeat in ten (roadmap D47), so in one rotation a node with one
relay in reach misses it one time in ten, at the edge of its reach more
often, and in two one time in a hundred. Listening finds a relay at no
cost to anyone; anchorless mode finds it by long beats and long answers,
and a node is in it far longer (roadmap D51, D73).

While a node is in its first rotation its table may lack relays that
are in range. It owns no position but its own ([owner](#owner)), and it
holds its own application's message while it knows no relay
([return to parent](#return-to-parent)).

### Channel moves

A relay moves only when `busy[home] − min_c busy[c] > 0.05`
continuously for 10 minutes; then it waits a random 0–5 minutes, checks
again, and moves to the quietest channel, ties broken as at boot.

*Why the hysteresis.* It keeps moves rare: without it, every rooftop
relay in range of the same interference sees the same ranking, and they
all jump together.

Immediately after retuning it sends **one heartbeat on the old
channel**, which names the new one and retunes at once any neighbour
parked on the old channel. There is no other announcement: neighbours
that miss it learn the new channel from the next rotation
([heartbeat](#heartbeat)) or from ACK failure ([hand-over](#hand-over)).

Its sleepy nodes follow it:

- A sleepy node listening at the time, in a window
  ([delivery](#delivery)), hears it and follows at once, so the moving
  node sends to the sleepy nodes of its heard table on the old channel
  that now listen on the new one ([heard table](#heard-table)).
- A sleeping node anchored to it follows at the next heartbeat it wakes
  for ([relay choice](#relay-choice)) and beats on the new channel soon
  after, which tells the holder where it listens ([beats](#beats),
  roadmap D53).

---

## Position

A node's position is its GPS fix, unless the app sets a map point, which
wins. A rooftop relay with no sky view uses one.

**A node's place.** A node has a place when it was given one where it
was built, has one stored as a fixed node
([implementation notes](#implementation-notes)), or the app sets a map
point. One with none takes its first accepted fix as its place, fixed,
at once, and sends no heartbeat before it; it rebuilds the positions it
heard near its place then, and a relay heartbeats its first rotation's
burst ([heartbeat](#heartbeat)) from then, as at boot (roadmap D83).

*Why no heartbeat before.* A heartbeat says where the node is, and
carries only the short position ([positions on the wire](#positions-on-the-wire)):
a node with no place can rebuild neither its neighbours' positions nor
its own beyond the short form's period, so any position or address it
gave would send its neighbours elsewhere. For the same reason its driver
checks its first fix against no neighbour.

*Why a carried node's stored place is none.* It is where the node
stood when last fixed, not where it is, and a node that boots there
rebuilds every neighbour's short position near it: a period off, every
true fix is beyond them all and dropped, and the node heartbeats a
position its neighbours rebuild as far from them, so their fixes are
dropped too (roadmap D133).

*Why the first fix and not the fixed rule.* Measured from a position it
never had, the first fix is a far fix, and the node would be carried
for four hours, holding no mail, at the place it has just found.

**The map point.** The app sets and clears it. While it is set, the
node takes no fix, its driver hands none, a relay's GPS rests (below)
and a sleepy node starts no search ([beats](#beats)); cleared, the node stays where it is, says so at once
([heartbeat](#heartbeat)), and its next accepted fix is its place
(roadmap D84).

*Why the app clears it.* A person set it, so a person ends it; a fix
cannot, since none is read while it is set.

A fix is dropped as unreliable, a sign of spoofing, when:

- it is farther than the hearing radius, 3 km
  ([physical layer](#physical-layer)), from every relay the node knows;
  a node that knows none takes it;
- the carrier-to-noise densities (C/N0) of the satellites used in it are
  not what a real sky gives; the band is a bench number (O18).

A jammed receiver gives no fix, and the node keeps the position it has.

Two fixes of a still node may differ by 50 m. That is the **allowance**
the [owner](#owner) rule uses. Indoors at a window they wander farther:
a fix of a still node may lie up to 250 m from another of its fixes
for minutes. That is the **carried radius**.

**Fixed and carried.** Every node measures from its fixes whether it
moves, but a sleepy node with an anchor, which takes none and keeps the
flag it has ([beats](#beats)):

| | |
|---|---|
| the place it stands | its first accepted fix, its map point, or the fix it had when last made fixed |
| far fix | an accepted fix more than the carried radius from that place |
| carried | after 72 s of far fixes with none near between, counting only time in which the GPS gave fixes |
| fixed again | after four hours of fixes within the carried radius of one place; it keeps the fix it has then |
| map point | fixed at once; cleared, the next accepted fix is the place |
| power | a carried node plugged in is a carried relay until it has stayed four hours (roadmap D72); a fixed one takes its first accepted fix within a search's limit, 60 s ([beats](#beats)), as its place, and else keeps the fix it has (roadmap D85) |

Which fixes a node takes as its position, and what a carried node gives
up, is in [nodes](#nodes). A node whose position changes heartbeats the
new one ([node identity](#node-identity)). At its first far fix a relay
that holds mail flushes its delivery queue ([delivery](#delivery)), since
once it has left it will hear none of its sleepy nodes' beats.

*Why 250 m.* The place is one fix, which may itself lie at the edge of
the wander, so the radius must hold the wander's whole width. On the
bench of 2026-11-06 three still boards at a window had fixes up to
199 m from one of their own fixes for 72 s, and no two of their fixes
lay more than 218 m apart; measured against the allowance's 50 m, the
three turned carried ten times in three and a half hours (roadmap
D136). 250 m, five allowances, clears both. A walker at 1.4 m/s, a
common walking pace, is past it after 179 s and carried after 251 s,
351 m away; at a brisk 1.5 m/s after 239 s. Until then it heartbeats
the place it left, and the [owner](#owner) rule sends mail for that
place to it as if it were still there.

*Why 72 s.* A walker crosses the allowance's whole width, 2 × 50 m, in
72 of the L76K's one-a-second fixes, so it is carried an owner area
beyond the radius. A still node's stray fix comes back inside; a
walker's keeps going. A sleepy node without an anchor fixes once a
delivery TTL ([beats](#beats)), so for it 72 s is two far fixes at least
72 s apart.

*Why only time with fixes.* While the GPS runs and gives no fix, the
node may be anywhere; two far fixes with a minute without a fix between
are not a minute far. The driver says when the GPS, running, stops
giving fixes that pass the checks, and that time until the next fix
does not count. Its searches end at a fix, so a sleepy node's time
between them counts.

*Why four hours.* A node that stopped for less would take mail for new
sleepy nodes and strand it when it moved on. A node carried by mistake
loses little, since its sleepy nodes have anchored elsewhere already
(roadmap D70).

*Why a node plugged in takes its first fix.* A sleepy node with an
anchor takes no fix ([beats](#beats)), so its last fix may be hours and
kilometres old. Measured from it, the fix at the place
it was plugged in is a far fix, and the node would be carried for four
hours there, holding no mail. Until it was plugged in nobody addressed
it at its own position, only at its anchor's; from its first heartbeat
as a relay its sleepy nodes and its own messages may, and their
addresses change with its position ([node identity](#node-identity)). A
relay's GPS runs on external power, so under the sky the first fix comes
within a search's limit and few have the old place by then. A fix that
comes later is of a node that does not see the sky, as likely a stray as
a move, and is checked as any other.

*Why no bound on the distance from the last fix.* Such a bound holds a
node wherever one wrong rejection leaves it, since every true fix after
it is far from the last accepted one too. And a sleepy node fixes
seldom, so in a car it moves farther between fixes than any bound a
walker passes allows, and the bound would hide the motion the carried rule
looks for. A spoofed jump beyond the hearing radius the neighbour check
catches; a spoofed drift inside it no bound on distance catches, only
the C/N0 check or a map point (roadmap D63, D74).

**A relay's GPS.** A relay's GPS runs all the time on external power
and for the grace after it is lost ([sleepy nodes](#sleepy-nodes)).
After the grace, and whenever its position is a map point, it rests in
standby (roadmap D82). A resting relay searches as a sleepy node does
([beats](#beats)), with no back-off, at boot and when the radio says it
may have moved: over each worst-case rotation it sums the squared
**surprises** of the heartbeats of its fixed relay neighbours, each
heartbeat's RSSI less that neighbour's averaged RSSI before it
([neighbour table](#neighbour-table)), and searches when the sum passes
the point below. A search that finds a far fix keeps the GPS running
until the node is carried or a fix is near again.

| | |
|---|---|
| fading | σ = 4 dB, the fade depth the switching margin assumes ([relay choice](#relay-choice), roadmap D38) |
| a surprise's variance | 4σ²/3, 21.3 dB²: the heartbeat's σ² and the average's σ²/3, each frame counting half in it |
| the sum | the squared surprises of a rotation, 195 s, each over 4σ²/3; for a node that does not move, chi-square with as many degrees as heartbeats |
| the point | two standard deviations above the sum's mean, by the Wilson–Hilferty approximation, k (1 − 2/(9k) + 2 √(2/(9k)))³ for k heartbeats; the switching margin's convention ([relay choice](#relay-choice)) |

*Why it runs through the grace.* A person who unplugs a node and walks
off with it does so then, and the 72 s of far fixes that make it
carried need the GPS running; carried on battery, it is sleepy then,
and takes fixes as a sleepy node does ([beats](#beats)). A relay on battery past the grace is one
left where it stands, in an outage (roadmap D81), and the L76K's 29 mA
beside the radio's 6.4 mA would take its battery to the low level in 19
hours instead of 106.

*Why the radio.* Moving changes every neighbour's signal: by
36 log10(1 + x/r) dB for a move x towards or away from a neighbour r
away under the path-loss exponent of 3.6, and more where the node steps
out of a building or past the walls that shadow it. A node that does
not move sees only fading.

*Why the test errs towards silence.* A frame in a deep fade is not
heard, so the heartbeats that reach a relay are cut off below
sensitivity, and their RSSI adds the noise to the signal; a still
relay's surprises vary less than 4σ²/3, measured a third to a half of it
(roadmap D82). It searches for nothing less often than the 2.3 % of
rotations the point allows.

*Why no back-off.* At the most the point allows, 2.3 % of 18.5
rotations an hour, a relay that never sees the sky searches 0.42 times
an hour, 60 s each at 29 mA: 0.20 mA, 3 % of its 6.4 mA.

*Why a map point rests it.* No fix moves a node with a map point
([nodes](#nodes)), so the GPS has nothing to do.

**Time.** The protocol has no network time: every timer is local and
monotonic, and nothing depends on two nodes agreeing on a clock. A node
keeps the time of day for the people who read it, its display and the
phone: UTC from a fix, or from the phone while it has none
([phone link](#phone-link)), carried on by its own clock while the error
can be under a minute, and the phone's UTC offset (roadmap D129, D130).

*Why under a minute.* A clock that shows minutes needs no better, and
past that the node shows no time rather than a wrong one: on a crystal
taken at ±250 ppm, 2.7 days after the fix.

---

## Heartbeat

Every node heartbeats. This section describes a relay's; a sleepy node's
are rarer ([beats](#beats)), and an anchorless node's carry the long
preamble ([sleepy nodes alone](#sleepy-nodes-alone)).

| Field | Bits | Content |
|---|---|---|
| heartbeat | 4 | `1000`: the packet is a heartbeat ([packet formats](#packet-formats)) |
| channel | 4 | home channel index 0–12 |
| format | 8 | the newest format the node speaks ([packet formats](#packet-formats)) |
| flags | 8 | sleepy, anchorless, carried, map point ([packet formats](#packet-formats)) |
| id | 48 | the node's ID ([node identity](#node-identity)) |
| position | 32 | the node's short position ([positions on the wire](#positions-on-the-wire)) |

13 bytes, 14.4 ms on air at SF5 ([physical layer](#physical-layer)); with
an update's section, 30 bytes, 23.36 ms
([updates over the mesh](#updates-over-the-mesh)).

| | |
|---|---|
| interval | a fixed T for each node, from the last byte b of its node ID: T = 5 s + 10 s × b / 256, mean 10 s |
| first heartbeat | at a random time within T after boot |
| **burst** | the first 13 slots, a burst gap of 1.1 s apart, each with the anchorless flag; every slot after it T after the one before |
| channel | steps `k ← (k + 1) mod 13` every slot, from a random start, whether the heartbeat went or not |
| **rotation** | 13 heartbeats, which reach every neighbour once: ≈130 s, at most 13 × 15 s = 195 s |
| lateness | none early; at most 872.8 ms after its slot |
| deaf time | 19 ms per heartbeat (procedure below), 0.2 % |

**Procedure.** Retune to channel k, listen before talking (one check,
3.1 ms), up to three checks a random 0–200 ms apart; if every check is
busy, skip; else send; retune home.

| | |
|---|---|
| listen-before-talk | one channel activity detection with the channel's RSSI read four symbol periods into it ([busy estimate](#busy-estimate)): busy if the detection saw a preamble or the reading was above the threshold. 3.1 ms from listening to listening, 2.18 ms of it the detection's 8 symbols and half a symbol of processing (roadmap D88) |
| transmission | its airtime and 1.8 ms more, from listening to listening (roadmap D88) |

*Why the reading inside the detection.* Read after it, the reading
needs the receiver started again on the channel and four symbol
periods to settle, 1.1 ms, and a return to listening between the two
checks, as many ms again; read inside it, the receiver is up already,
and the reading is right from four symbol periods in, as in receive.
The datasheet documents the reading for receive only, so the driver
checks at boot that the two agree on the home channel, keeps the
offset it finds, and reads after the detection if they do not
(roadmap D88).

No scanning is needed: every relay's heartbeat visits all channels, so a
node listening on any one channel hears every relay in range within one
rotation. The heartbeat load per channel is in
[capacity and sizing](#capacity-and-sizing).

A neighbour that has heard one heartbeat of a node knows the node's
interval from its ID, and so when its next comes to any channel: on
channel c at time t, on channel c + j at t + j × T, and on c again at
t + 13 × T. A sleeping node uses this to wake for its anchor's
([relay choice](#relay-choice)), and a relay to tell a heartbeat that
did not come.

**A heartbeat due and not heard.** A relay, which listens all the time,
expects each neighbour's heartbeat on its own channel once a rotation,
in a window around the slot: from the slot less what the last one
heard may have waited behind its own (the lateness above) and the
clocks' drift since, to the slot plus that lateness, that drift and the
heartbeat's airtime, about 1.8 s. When the window closes without the
heartbeat, the relay enters a reading at its noise floor
([busy estimate](#busy-estimate)) into the neighbour's average
([neighbour table](#neighbour-table)), weighted by the share of the
window through which its radio was listening on the channel: a
heartbeat it was away for, reading another channel, sending, or under
another frame, it could not have heard. The slot passes to the next rotation either way.
A heartbeat of a burst is in no slot and says the node's slots start
anew; a heartbeat heard in a slot sets the slots afresh. A sleepy node,
which hears only in its windows, counts nothing.

*Why a reading at the floor.* An average of the heartbeats that got
through stands above the link: a neighbour at or below sensitivity is
heard on its good moments and averaged on those (roadmap D103).
Counted as what was sent, heard or not, the average is of the link
(roadmap D105). A heartbeat sent and not heard had a packet RSSI under the level
half of all heartbeats arrive at, 3.1 dB below a full frame's
([neighbour table](#neighbour-table)): 5.2 dB of signal below the
noise, which packet RSSI reads as 1.2 dB above the floor. The floor
itself, what the radio reads of an empty channel, is the reading an
absent frame gives, and the two differ by 0.6 dB in an average that
counts a reading half. A heartbeat the neighbour skipped for a busy
channel or deferred for its budget is counted the same, wrongly: a skip
needs three checks busy, one time in a thousand on a channel busy a
tenth of the time, and the next frame heard halves the error.

*Why weighted by the window.* The radio reads another channel once a
second ([busy estimate](#busy-estimate)) and sends its own heartbeats
and ACKs, so few windows pass with it listening throughout. A heartbeat
that came at some moment of the window was lost to the radio's absence
with the probability of the absence's share of the window, and with the
rest to the link; a frame the radio did not decode is not known to it,
so a collision it lost counts as the link's.

*Why a fixed cycle.* A fixed cycle reaches every neighbour once per
rotation; a random channel per heartbeat would need ≈41 heartbeats on
average to cover all 13.

*Why a heartbeat can be late.* It may find a hand-over attempt under
way, a full frame with the long preamble to a sleepy node outside its
window and the ACK timeout ([hand-over](#hand-over)), and then back off
twice before its last check, 372.8 + 100 + 2 × 200 ms. A check held for
a reception in progress waits less, at most that frame.

**The burst.** A relay's first rotation reaches every neighbour within
12 burst gaps of its first heartbeat, 13.0 s, where its later rotations
take up to 195 s; the 13 heartbeats cost no more airtime than
any rotation's. The anchorless flag on them asks the relays that hear
one to answer it (below), so that the new relay learns its neighbours
as fast as they learn it, and tells every receiver that the heartbeat
is in no slot: a neighbour times a relay's slots only by its heartbeats
without the flag.

*Why 1.1 s.* The answers to one heartbeat of the burst come within a
backoff of it, 200 ms, and may then wait as long behind their slot as
any heartbeat, 872.8 ms, and take a heartbeat's airtime, 14.4 ms: 1087.2
ms. The next heartbeat goes after them, so that the node is on its home
when they arrive.

**Outside the slots.** A relay that is past its own first rotation,
neither carried nor flushing to turn sleepy
([sleepy nodes](#sleepy-nodes)), answers a heartbeat with the
anchorless flag with one of its own on the channel the heartbeat names,
at a random time within a listen-before-talk backoff, 0–200 ms, of it:

- a relay's, of its burst: once, with the ordinary preamble, since it
  listens all the time;
- a sleepy node's: as [sleepy nodes alone](#sleepy-nodes-alone) says,
  with the long preamble.

*Why within a backoff.* Every relay on the channel hears the same
heartbeat; checking the channel in the same instant, they would all
find it free and send together.

*Why a node in its first rotation does not answer.* Nodes switched on
together learn each other from their bursts, which are heartbeats like
any other; answers would add to the busiest minute after a power cut.

And a node, sleepy or not, heartbeats once when its sleepy or
carried flag changes or its map point is cleared, and a relay also
when a map point is set or its place moves to a fix it takes
([position](#position), roadmap D71, D72). That heartbeat:

- goes on its home channel, with the ordinary preamble, the long one if
  it is anchorless, and listen-before-talk;
- from a sleepy node, on its anchor's channel, is a beat to its holder,
  and its window follows ([delivery](#delivery));
- from a node that turns sleepy, goes on the channel it was at home on,
  where its neighbours and its sleepy nodes know it, unless its first
  beat goes there anyway ([sleepy nodes](#sleepy-nodes));
- is one heartbeat for any number of changes before it goes.

A position that changes with each fix is not announced. Sleeping nodes
hear none of this, receiving duty-cycled; they learn a change at the
next slot they wake for.

*Why a heartbeat at a change.* Its neighbours on that channel then
address it anew at once instead of a rotation later, and an old anchor
that holds mail for a node turned relay hears it as a relay and hands
the mail over. The flags change seldom: the sleepy flag only as a
carried node goes on or off its charger or a fixed relay's battery runs
low or recovers, and the carried flag is cleared only after four hours
still.

*Why not at each fix.* A carried relay's slots carry its position every
T (roadmap D71); a sleepy node, carried or not, is a last resort to its
neighbours, and its beats carry its address: its anchor's position, or
its last fix without an anchor ([beats](#beats)).

**The 256 intervals** are 39.1 ms apart, more than two heartbeats'
airtime, 2 × 14.4 = 28.8 ms, so two nodes whose intervals differ slide
past each other by more than an overlap at every heartbeat and meet at
most once in a row, as random intervals would. 10 s / 28.8 ms = 347
steps would do, and 256 is the largest power of two below it, one byte.

*Why a shared interval does little harm.* Two nodes in reach share an
interval with probability 1/256, which matters only if they are also in
step, within 28.8 ms in a 10 s interval, 0.29 % of such pairs; their
heartbeats then meet each time they are on one channel, as do those of a
pair in step whose intervals stand 2:1 or 3:2.

Clock drift unlocks them: a 32.768 kHz watch crystal is good to about
±50 ppm, so two clocks part by up to 1 ms per 10 s heartbeat, and an
overlap of 28.8 ms ends within 29 heartbeats, under five minutes,
longer for clocks that drift alike. Nodes that boot together, after a
power cut, would start in step but for the random first phase (roadmap
D47).

---

## Neighbours

### Node identity

Every node has a 6-byte **node ID**: the low 48 bits of its chip's
factory device ID (nRF52840 FICR DEVICEID: the 32 bits of DEVICEID[0]
and the low 16 of DEVICEID[1]), the same at every boot, with no setup
step. It names the node wherever a packet names one: DATA `dst_hop`,
`src_hop` and `dst_id`, ACK `from` and `to`
([packet formats](#packet-formats)).

*Why six bytes.* An ID must be unique only among nodes close enough
that one relay hears them together; a pair that shares one there loses
about a quarter of its mail to each other (roadmap D59). At the density
of [capacity and sizing](#capacity-and-sizing), 714 nodes per km² in a
city of N = 300 000, a node has ρπr² others within a hearing range r
([physical layer](#physical-layer)), and the city N ρπr² / 2 such pairs:

| | indoors, r = 300 m | street, r = 750 m |
|---|---|---|
| nodes within r of a node | 200 | 1 260 |
| pairs within r in the city | 3.0 × 10⁷ | 1.9 × 10⁸ |
| chance that one pair shares a 4-byte ID | 0.7 % | 4.4 % |
| chance that one pair shares a 6-byte ID | 1.1 × 10⁻⁷ | 6.7 × 10⁻⁷ |

So with four bytes a city at the design density has such a pair about
one time in fifty, and with six one time in millions. Eight would make
it rarer for nothing and cost as much again: each two bytes in the ID
fields add about 6 % to a text's channel time, 86.4 → 91.5 → 96.6 ms a
hop for four, six and eight bytes ([capacity and sizing](#capacity-and-sizing)).

*The low bits are assumed random.* The chip's documentation promises a
unique 64-bit device ID, not 48 random low bits. This section, and the
heartbeat interval taken from the ID's last byte
([heartbeat](#heartbeat)), assume they spread as random bits would,
which the bench boards are to check (`roadmap.md` Phase 2).

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
| sleepy, anchorless | from its heartbeat's flags ([packet formats](#packet-formats)): a sleepy neighbour is a last-resort next hop, sent to with the long preamble ([next hop](#next-hop)); an anchorless sleepy one is no anchor ([relay choice](#relay-choice)), and an anchorless relay is in its burst ([heartbeat](#heartbeat)) |
| carried | from its heartbeat's flags ([nodes](#nodes)): no anchor and no nearer owner ([owner](#owner)), and a last resort once stale ([next hop](#next-hop)) |
| format | from its heartbeat: the newest format it speaks ([packet formats](#packet-formats)), which sets the format of a DATA to it ([hand-over](#hand-over)) |
| map point | from its heartbeat's flags: its position is a map point a person set ([packet formats](#packet-formats)); kept and shown, used by no rule |
| channel | where to transmit to it: from its heartbeat or from `aux` of a DATA it sent ([packet sender](#packet-sender)), whichever came last |
| last_heard | local monotonic time |
| rssi_ema | averaged RSSI of every reading of its link, each new one counting half: every frame heard from it, and each of its heartbeats due on this channel and not heard, as a reading at the floor weighted by the share of its window the radio listened through ([heartbeat](#heartbeat)); says whether it has margin |
| fail_count | failed hand-overs to it in a row, with no answer from it between ([demotion](#neighbour-table)) |
| banned_until | when its ban ends ([bans](#bans)) |
| service, last take, told | how long this relay takes to get one message through to it, when it last took one, and the waits its refusals have told since ([service time](#neighbour-table)) |
| line end | when the line of senders refused for want of it as the way on ends ([bans](#bans)) |
| update | the section of its last heartbeat that carried one: the version it runs, the newest image it holds or gathers and how much of it it holds, and when it was heard, for a helper's lease ([updates over the mesh](#updates-over-the-mesh)) |
| last_beat | when its last heartbeat heard in a slot started, or its last slot on this channel that passed unheard, and on which channel: with its interval, when its next comes to any channel ([heartbeat](#heartbeat)) |

**Expiry.** An entry expires after a time without a heartbeat or an
answer from the neighbour (demotion, below):

| | |
|---|---|
| a relay | 3 rotations (390 s) |
| a sleepy node with an anchor | two beat periods, 600 s |
| an anchorless node ([sleepy nodes alone](#sleepy-nodes-alone)) | two of its sender's worst-case rotations and a heartbeat's lag |
| a sleeping node's anchor | never: the node keeps its anchor's entry whatever its age, and evicts it for no newcomer ([relay choice](#relay-choice)) |

*Why a sleepy entry lasts longer.* Its beats come once a period, on its
anchor's channel only ([beats](#beats)); a node elsewhere renews it from
the frames they exchange.

*Why two rotations for an anchorless entry.* Its 13 beats, one a
channel, take at most 13 anchorless beat periods, so two rotations take
26, 1200 s, and a heartbeat's lag adds at most 872.8 ms
([heartbeat](#heartbeat)). A listening receiver loses about one beat in
ten (roadmap D47), so with one rotation a live neighbour, perhaps the
only way on where there is no relay, would drop out every tenth
rotation, for 13 minutes; with two, one time in a hundred, while a stale
entry costs only a hand-over's attempts and a ban (roadmap D51).

**Margin.** A neighbour has margin when `rssi_ema` is at least 4.1 dB
above the noise floor ([busy estimate](#busy-estimate)). A neighbour
without margin stays in the table, its heartbeats showing it alive, but
is a next hop only when no other is left ([next hop](#next-hop)).

*Why that margin.* A 255-byte frame is then lost no more than one time
in a hundred, the datasheet's standard for sensitivity, after a fade of
4 dB. A packet arrives only if every symbol does: a 255-byte frame has
541 symbols, the datasheet's 64-byte reference packet 157, a heartbeat
56, so with the modulation's symbol error rate (32 symbols, detected
without phase) a full frame needs 0.43 dB more signal than the
reference for the same 1 % loss, and a heartbeat 0.39 dB less; half of
all heartbeats arrive even 3.1 dB below a full frame's level, so a
heartbeat says little about a full frame. Sensitivity is 2.5 dB below
the noise at SF5 (datasheet Table 6-1), so a full frame's level is
2.5 − 0.43 = 2.07 dB below the noise floor, and after the fade it needs
−2.07 + 4 = 1.93 dB above it. Packet RSSI is signal and noise together
(datasheet §13.5.3): 10 log10(1 + 10^0.193) = 4.1 dB above the floor.

The fade, 4 dB, is the standard deviation of fading over time and
channel in the simulated links, until Phase 2 measures it. The average
must cover it because a frame goes out on the neighbour's channel while
this relay hears the neighbour on its own, where the fading differs.

**Demotion.** A neighbour with three failed hand-overs in a row
(`fail_count ≥ 3`) is demoted until it answers this relay or three
rotations (390 s) pass, and then ranks behind every other neighbour
with margin ([next hop](#next-hop)). An **answer** is an ACK that
answers this relay's hand-over to the neighbour, taking, refusing or
seen ([the ACK](#the-ack)), or a DATA the neighbour addresses to this
relay; it resets `fail_count`, so failures are in a row only with no
answer between them. Heartbeats, and frames to other nodes, do not.
Expiry takes minutes; demotion takes seconds, and is the test that the
neighbour no longer hears this relay ([hand-over](#hand-over) step 6).

*Why three.* A failed hand-over cannot tell a receiver that never got
the frame from one that took the message and lost its ACKs, or could
not afford them. A receiver silent for want of reply budget, its
reserve spent by a burst of senders ([duty cycle cap](#duty-cycle-cap)),
answers again once its bucket has refilled what a sender's retries cost
it. The retrying takes at most 2.41 s, the two retry waits, 311 +
622 ms, and two more attempts of up to 500 ms of listen-before-talk, a
138.6 ms full frame and the 100 ms ACK timeout ([hand-over](#hand-over));
the bucket refills 9.83 % × 2.41 s = 237 ms in that time, 13 refusals.
The message waits out the neighbour's ban between offers ([bans](#bans)),
1.58 s and then 3.16 s, so the third offer comes 4.74 s after the first
failure, when such a receiver can answer; the second may still find it
short. Two would demote it before it could answer; later offers would
mostly meet a receiver that answers nobody, each at the cost of its wait
(roadmap D102, D106).

*Why any answer clears it, and nothing else.* A refusal or a seen ACK
shows, as a take does, that the neighbour heard the DATA and this relay
its reply. A DATA addressed to this relay shows nearly as much: the
neighbour chose this relay from its own table, so this relay's
heartbeats reach it, and it sends on this relay's channel now. A
heartbeat, or a frame to another node, shows only that the neighbour is
alive, not that it hears this relay; a link that works one way would
otherwise cost every message the failed attempts again.

**Service time.** Each entry keeps how long this relay takes to get one
message through to the neighbour, the spacing of its refusals' line
([bans](#bans)): a running average, each take counting one part in 39,
of the time from when a message began to wait for the neighbour here,
or from the neighbour's last take if later, to the ACK that takes it. A
message begins to wait when it is queued, or when routing sends it to a
different neighbour than before ([next hop](#next-hop)). The time folds
in what held the message, unanswered attempts and their bans, this
relay's bucket and its turn behind other hand-overs, and leaves out time
with nothing waiting, which says nothing of how fast the neighbour
takes, and the waits its refusals told, the neighbour's own measure,
passed on. Before the first take it is one ban, and it is never more
than 89 min, the shortest and the longest told wait ([bans](#bans)).

*Why 39.* The neighbour's bucket sets its state: at its cap it takes a
full frame a ban, and its bucket refills from empty in 61 s
([duty cycle cap](#duty-cycle-cap)), so 39 takes span one whole refill.
Fewer would follow the luck of single takes; more would remember a
bucket that has since refilled. It is kept in 32 bits, in 1/1024 of a
ban, a step of 1.5 ms; 16 bits would hold only 64 bans, 101 s.

**Eviction.** When the table is full and a relay not in it is heard, the
newcomer replaces, in this order:

1. the least recently heard demoted entry;
2. the least recently heard entry, if it has missed a worst-case
   rotation, and so certainly a heartbeat on this channel;
3. if the newcomer is a fixed relay, one that holds mail, within 50 m,
   the owner radius ([owner](#owner)), the least recently heard entry
   beyond 50 m (roadmap D68, D70).

Failing all three it is not added (roadmap D12). A sleepy node with an
anchor replaces no entry.

*Why keep incumbents.* A rooftop relay hears ≈270 relays
([capacity and sizing](#capacity-and-sizing)), about two heartbeats a
second on its channel, most from relays not in the table: replacing the
least recently heard outright, each entry would last about
64 / 2 ≈ 30 s and every failure count be forgotten within a minute.
Keeping incumbents holds an equally arbitrary 64 with their failure
counts, and replaces exactly the entries that fail or are silent. The
table is then a sample weighted toward the strong links, heard first and
most often; they lie where relays, and so destinations, are, and a table
spread by bearing routes no better (roadmap D69).

*Why the relay within 50 m is kept.* The owner rule reads from the table
that no relay that near is closer to a destination: one turned away
would have its messages claimed by its neighbour and queued for a node
that never beats. With every person's node a relay, 714 per km², 5.6 are
within 50 m (714 × π × 0.05²). A carried relay needs no such place
([nodes](#nodes)).

*Why a sleepy node replaces none.* It is a last resort, and a relay
hears every sleepy node beating on its channel, often more than 64:
making way for them would push out the neighbours it routes through.

A beat fills the neighbour table like any heartbeat, the entry marked
sleepy. At a node that is not sleepy it fills the heard table too if the
sleepy bit is alone ([heard table](#heard-table)); a node without an
anchor has no window to deliver in.

### Neighbour status

A person who looks at a map of their node's neighbours in the app sees
what each neighbour's own screen shows, as if standing at it: the node
asks each neighbour shown for its status, and each answers.

The **ask** names the node asked and the node asking, and nothing else.
The **answer**, the status, says ([packet formats](#packet-formats)):

| field | bytes | content |
|---|---|---|
| flags | 1 | bit 0 placed, it has a place ([position](#position)); bit 1 sleepy and bit 3 carried ([nodes](#nodes)); bit 2 anchored, sleepy with an anchor ([relay choice](#relay-choice)); bit 4 on external power, USB; bits 5 to 7 zero, ignored |
| battery | 1 | 0 at or below the low level, about 20 % ([sleepy nodes](#sleepy-nodes)); 1 to 100 about that share of its charge left, from its last reading; 255 not read, and on USB, where the reading is the charger's |
| heard | 1 | its live neighbours, at most 64 ([neighbour table](#neighbour-table)) |
| strong | 1 | of them, those heard with the margin a full frame needs through a fade, likely to hear it too |
| proven | 1 | of them, those that have taken a message from it, proven to hear it |
| version | 4 | the version of the firmware image it runs, 0 if not known, 4 bytes as in a heartbeat's section ([updates over the mesh](#updates-over-the-mesh)) |
| gathers | 4 | the version of the image it gathers over the mesh, 0 if none |
| blocks | 1 | that image's blocks, the table's among them; 0 if none |
| complete | 1 | the blocks of it it holds from the first |
| have | 1 | the pieces it holds of the block after them |

Each is a frame from node to node ([packet formats](#packet-formats)):
one hop, and neither answered nor sent again. A node:

- asks a live neighbour only when its app asks, as a person opens or
  refreshes the map;
- answers an ask addressed to it from a live neighbour, once; one from
  a node it does not hear it leaves unanswered, since that node's home
  channel is not known to it and its answer would likely not arrive;
- sends each on the other node's home channel, with the long preamble
  to a sleepy one, which receives duty-cycled
  ([duty-cycled receive](#duty-cycled-receive-and-the-long-preamble)),
  after a listen-before-talk as a heartbeat's, given up after its three
  checks, and paid from the bucket with the reply reserve kept, as an
  update's piece is ([duty cycle cap](#duty-cycle-cap)); after the
  hand-overs, so that a text never waits behind one, an answer before
  an ask;
- hands every answer addressed to it to its app.

The ask is 14 bytes, 15.68 ms on air, the answer 30, 23.36 ms; with the
long preamble 249.92 and 257.60 ms.

*Why only on a look.* Said in every heartbeat, the same would cost
every node every rotation, for a map that is seldom open; asked, it
costs the network nothing between looks. A lost ask or answer costs the
person a refresh.

*Why the image gathered.* During an update a person at the map sees
which neighbours still gather and how far they are, as each one's
screen shows it, without a console at the node; the last three fields
are its heartbeats' section's ([updates over the mesh](#updates-over-the-mesh)).
*Why its blocks.* The status then fills 13 blocks of symbols at SF5
exactly, so the byte costs no airtime, and the held blocks can be shown
against all.

---

## Forwarding

### Addresses

A destination address is a position and a node ID: a relay's own
position, or a sleepy node's anchor's, and the recipient's ID
([node identity](#node-identity), [nodes](#nodes)). A sender learns an
address from a previous message (the sender's address is carried in the
encrypted payload), from a QR code, or from a directory; the network
provides no lookup. Sending to an area is open item O7.

The network carries no sender address and sends no failure notices. Its
only acknowledgement is the per-hop ACK ([hand-over](#hand-over));
whether a message arrived, and what to do if it did not, is the app's
business, with whatever acknowledgement it puts inside the encrypted
payload.

The app hands a node a message only when the node says it will take
one ([receiving DATA](#receiving-data)), and hears nothing about it
afterwards: not the first hop's ACK, and not a drop. The one thing the
app hears is a refused message, given back with the wait the refusal
asks for: a sleepy node's, refused by its anchor
([a sleepy node's own message](#a-sleepy-nodes-own-message)), or a
relay's, refused by its own gate ([receiving DATA](#receiving-data)).

*Why the app hears so little.* A user could take the first hop's ACK
for delivery. From then on the node is sending the message or waiting
to, so a busy network makes its users wait rather than lose their
messages at the first step (roadmap D29). A refused message is given
back so that a person told to wait is not left with a message that ages
in a node (roadmap D54).

### Duty cycle cap

At SF5, 10% of airtime is ≈180 B/s per relay, so a busy relay runs out
of budget before its channel fills. Forwarding to the neighbour nearest
the destination does not spread load; central rooftop relays hit the
cap first.

**Token bucket.** Each node keeps its airtime in a bucket. A
transmission may start only if the bucket holds its airtime, which it
takes out.

| | |
|---|---|
| depth | at most 6 s, one minute at 10% |
| fill | (360 s − 6 s) per hour, 9.83% of the time |
| a node at the cap | sends steadily, 5.9 s of every minute; with its bucket empty a full frame waits 1.4 s |
| longest wait | 6 s / 9.83% = 61 s (roadmap D26) |

*Why that rate.* Any interval of length L then carries at most
6 s + 9.83% × L on air, so any hour at most 360 s: 10%, the limit of
ERC/REC 70-03 band g1, which is measured over an hour (proof in
`tables/budget.rs`). The rate is below 10% by the bucket's depth over
the hour, so that a burst cannot carry an hour over the limit.

**Out of budget.** A hand-over the bucket cannot fit yet waits for it;
it is not dropped. Two reserves stay in the bucket:

- Replies that take on no work, refusals and seen answers among them,
  leave one heartbeat's airtime, 14.4 ms. The bucket refills that in 146 ms, far less than the 5 s
  between heartbeats ([heartbeat](#heartbeat)), so heartbeats always go.
- Forwarding, and the ACK that takes on work, leave the **reply
  reserve**, 845 ms: the heartbeat, and 49 refusals, enough for a relay
  at its cap to answer every DATA it receives for the longest ban.

*Why the reply reserve.* A relay at its cap that falls silent is
banned by its senders, longer each time, and after three silences
demoted ([bans](#bans)), and its refusals no longer tell the wait back;
it must answer every DATA for as long as the longest ban. A relay receives at most one DATA
per full frame and the ACK that answers it, 138.6 + 17.0 = 155.5 ms,
and sends a heartbeat at most every 5 s ([heartbeat](#heartbeat)). Over
the longest ban, 61.0 s, the bucket refills exactly its depth, 6 s,
while 13 heartbeats take 187 ms and the 391 refusals that fit in the
rest take 391 × 17.0 = 6631 ms: 819 ms more, ⌈819 / 17.0⌉ = 49 refusals
([the ACK](#the-ack)). With the heartbeat's 14.4 ms that is 845 ms.
A relay at the reserve answers nine DATA in ten from its refill alone,
15.3 of every 17.0 ms.

A relay below the reserve sends nothing but heartbeats and replies until
its bucket refills. Each sender it refuses bans it, so a sender costs it
at most one reply per ban ([bans](#bans)). Without budget even for a
reply it stays silent; its neighbours' hand-overs to it fail, and they
ban it, longer each time, and after three failures demote it
(roadmap D27, D29, D42, D106).

### Hand-over

A **hand-over** sends one DATA packet to one neighbour until that
neighbour answers it or the relay gives up. A relay runs one hand-over
at a time; packets waiting their turn are held in its
[send queue](#send-queue). The answer is an ACK, which takes, refuses or
has seen the packet and names the message and both nodes
([the ACK](#the-ack)).

1. Retune to the next hop's home channel, read before each attempt from
   its latest heartbeat or DATA `aux` ([neighbour table](#neighbour-table)),
   so that a retry follows a neighbour that has moved.
2. Listen-before-talk: one check, the CAD with the channel's RSSI read
   during it ([heartbeat](#heartbeat)); if busy, a random backoff of
   0–50 ms and check again. Once one attempt's checks have found the
   channel busy for 500 ms, the hand-over gives the radio up for a
   retry wait ([retry wait](#retry-wait)) and listens again for the same
   neighbour, with the transmissions it has made and a fresh 500 ms;
   nothing is counted against the neighbour. For a delivery to a sleepy
   node ([delivery](#delivery)), which its queue keeps if it fails and
   which waits for the node's next beat anyway, a give-up counts as an
   attempt.
3. Transmit, the sender's home channel in `aux`, in the format below,
   with the shortest preamble, or the long one to a sleepy neighbour
   ([duty-cycled receive and the long preamble](#duty-cycled-receive-and-the-long-preamble)).
4. Retune home at once.
5. Wait for the ACK **on the home channel**: timeout 100 ms, the ACK's
   17.0 ms, the receiver's retunes and turnarounds, and a reply it owes
   already, which goes first ([the ACK](#the-ack)). An ACK that takes
   the packet ends the hand-over: the neighbour has the message and is
   tried ([message records](#message-records)). An ACK that has seen it
   ends it too, without a take: the neighbour is tried and the message
   chooses its next hop at once.
6. On timeout: wait ([retry wait](#retry-wait)) and retry, up to 3
   attempts; an ACK that comes before the retry still answers. Then the
   hand-over has failed: count a failure against the neighbour and ban
   it ([bans](#bans)). The message keeps its place, untried at that
   neighbour, and is routed again with three fresh attempts
   ([next hop](#next-hop)). While the neighbour ranks first, the message
   waits for its ban, however other messages have grown it, and offers
   itself again; it goes elsewhere once the neighbour is demoted
   ([neighbour table](#neighbour-table)) or has left the table, and a
   forward whose failure leaves the neighbour demoted counts it tried
   while queued here ([next hop](#next-hop)). A return is dropped once
   its parent is demoted or gone ([return to parent](#return-to-parent));
   the delivery queue keeps a delivery whatever its hand-over does
   ([delivery queue](#delivery-queue)).

*Why the same neighbour until it is demoted.* Moving on from a receiver
that took the message and lost its ACKs leaves two copies searching
apart, each repeating the other's work (roadmap D89). A receiver that
holds the message answers any later offer
([receiving DATA](#receiving-data) case 4), and one silent for want of
reply budget answers once its reserve has refilled, which the demotion
waits for ([neighbour table](#neighbour-table)). A neighbour that
answers this relay at all hears it, and going round it takes the message
the long way on the same full channels, unless it refused this message
for want of room ([bans](#bans)). How long other messages have made its
ban says nothing about whether it hears this relay, so a grown ban is
waited out (roadmap D102, D106).

**An attempt is a transmission.** A busy channel never moves a message
to another neighbour, nor ends a return (step 2). What moves a message
is a seen answer (step 5), a refusal ([bans](#bans)), or a change in the
state it is routed from: its neighbour demoted or gone, or a better one
come (step 6). A channel that stays busy holds the message, as a ban
does, until it clears; the message spends no airtime meanwhile, so the
duty cycle cap does not bound the wait (roadmap D96, D111).

*Why a busy channel moves nothing.* The channel is heard at the sender
and says nothing about the neighbour: not that it lacks the message,
which a transmission may have brought it, nor that it is full or gone.
Passed over before any transmission, the message takes the longer way
round on the same full channels; passed over after one, it is offered
to a second neighbour while the first may hold it, and the two copies
search on apart (roadmap D89, D96).

**Format.** A DATA goes to a neighbour in the newest format both speak:
to a neighbour in the table the lower of this node's newest and the
neighbour's, from its last heartbeat; to one known only from a DATA it
sent ([packet sender](#packet-sender)), that DATA's. The ACK answers in
the format of the DATA ([packet formats](#packet-formats)).

*Why a byte in every packet.* Nodes are planted by anyone and updated by
hand, one at a time, so a new format must work beside nodes that never
get it: an updated node talks to its old neighbours in the old format,
and their heartbeats tell it, at no traffic, which speaks what. The byte
costs no airtime in the heartbeat and the ACK, whose 13 and 18 bytes
fill the same 6 and 8 blocks of symbols at SF5 as 11 and 17 would. In
DATA it adds a block, 1.28 ms, for two payload lengths in five: 0.51 ms
on average, under 1 % of a text's frame, and nothing for the 100-byte
text of [capacity and sizing](#capacity-and-sizing).

#### Retry wait

The wait before the retry after the k-th unanswered transmission is
uniform in [0, 2^k × 155.5 ms]: 0–311 ms, then 0–622 ms. For a hand-over
whose frame has the long preamble the unit is 389.8 ms. The
listen-before-talk backoff of step 2 stays short: it waits for a channel
the sender can hear to clear, and checks again (roadmap D22). A
hand-over whose listen-before-talk gave up (step 2) waits the window of
the transmissions it has made, one unit before any, and listens again.

*Why that unit.* 155.5 ms is how long one hand-over holds its receiver,
a full frame (138.6 ms) and its ACK (17.0 ms). A hand-over whose frame
has the long preamble holds its receiver longer by as much as its frame
is longer, 234.2 ms
([duty-cycled receive and the long preamble](#duty-cycled-receive-and-the-long-preamble)).

*Why a random wait that doubles.* Other than a weak link, the usual
cause of a missing ACK is a hidden terminal: a sender the first cannot
hear transmitted to the same receiver at the same time, and both miss
the ACK and retry. Their retries miss each other only if they start more
than one hand-over apart, which for full frames and a window of W units
happens with probability (1 − 1/W)²: never with one unit, 1/4 with two,
9/16 with four. Doubling per attempt also spreads more than two such
senders.

*Why a give-up waits the same window.* The backoff waits for a frame to
end, and a channel busy for 500 ms, longer than any frame, is held by
one hand-over after another: the wait is in their unit, and the senders
waiting for the same channel come apart as retrying ones do.

#### The ACK

The receiver retunes to the sender's channel, sends the ACK at once,
without listening first, and retunes home. The ACK is the first byte, the
format, the message ID, and the IDs of the answering and the answered
node: 18 bytes, 17.0 ms ([packet formats](#packet-formats)). Its first
byte says taken, refused or seen: a wait of 0 takes the packet, a
refusal puts there the wait it asks for ([bans](#bans)), and the seen
flag, with a wait of 0, says the receiver has seen the message and takes
nothing. ACKs arrive, like everything else for a node, on its home
channel; a sleepy node's is its anchor's, so traffic between them never
retunes. A node is deaf for 20 ms per ACK it sends, its two retunes, the
transmission's own costs and the airtime ([heartbeat](#heartbeat)), and a
sender that gets no ACK retries: the cost of one radio.

The sender matches the ID and both names against its outstanding
hand-over, and ignores an ACK that does not match. One that matches,
taking, refusing or seen, resets the neighbour's failure count and
refreshes its last-heard time ([neighbour table](#neighbour-table)): a
neighbour that answers is alive whether or not its heartbeats are heard.

*Why both IDs.* Nodes share home channels, so without the answering
node's ID an ACK for the same message from another relay on the same
channel, which the depth-first search can produce, would end the wrong
hand-over and strand the message. And two senders can offer the same
message to one node at once, one retrying a hand-over whose ACK was lost
while another returns the message there, or the two copies of a split
message ([why the search finds a path](#why-the-search-finds-a-path)):
if the ACK named only the message and the relay, each would take the
other's answer as its own. A take would end a hand-over the relay never
took, and the message would be lost; a refusal would send a sender on
from a relay that took its packet, and the message would have a second
copy (roadmap D32).

**Seen** answers a copy of a message the receiver has a record of, holds
no copy of, and did not hand to the sender
([receiving DATA](#receiving-data) case 6). The receiver changes nothing
but its record. The sender treats the answer as if the receiver had
returned the message the moment it got it: the receiver is tried, the
count it recorded is spent ([hop count](#hop-count)), and the message
chooses its next hop at once. Nothing is banned and no failure is
counted on either side. A return answered seen is dropped
([return to parent](#return-to-parent)). A seen ACK is sent from the
heartbeat reserve like any answer that takes no work; a receiver that
cannot afford it stays silent, and the sender's hand-over fails as for
any silence. A seen ACK that is lost costs the sender a retry, answered
seen again ([receiving DATA](#receiving-data) case 3).

*Why seen, not taken.* A copy that reaches a relay that has handled the
message and holds none of it is the search crossing itself: the relay's
own search goes on when its child returns the message, or is over.
Taken and searched on from the relay's record, the copy would start that
search again and search on until the hop cap ended it; taken and
dropped, it would end, and with it a single search whose way round a gap
crosses its own path. Seen, the copy stays with its sender, which has
the one record that knows what it has still to try (roadmap D89, D90).

**A take is final when the ACK has gone out.** The receiver decides on
reception whether it takes or refuses the packet, but acts on a packet
it takes, into its send queue, its delivery queue and its record, only
once the ACK has left the antenna. An ACK that cannot go out, for want
of budget or a fault of the radio, leaves the packet untaken, and the
sender's retry brings it again as a new packet. A packet that gives no
work ([receiving DATA](#receiving-data) cases 1, 3, 4 and 6) is noted on
reception: nothing is lost if its ACK never goes.

*Why at once, and why final only then.* Being unable to send is not
having answered. An ACK that waited for a busy channel could lapse past
the sender's timeout with the packet taken, and the sender would hand
the message to someone else as well: two searchers. The sender listens
for the ACK on its home channel and expects nothing else there in its
100 ms, so the ACK collides with whatever is on that channel whether it
listens first or not; listening only moves the collision into the ACK's
backoff. The band's rules ask a duty cycle and no listen-before-talk
([regulatory limits](#regulatory-limits)), and the ACK is paid from the
bucket like every frame. Sent at once, a reply fails only for budget,
which the receiver checks before it owes it, and the take at the ACK
makes a reply that fails all the same harmless (roadmap D93).

A node ACKs a packet as soon as it has received it, even while waiting
for an ACK of its own; if that makes it miss its own ACK, its next
attempt covers it. It owes at most one reply at a time: a DATA that
arrives while it still owes one is neither taken nor refused, and its
sender retries as for a lost ACK, while a copy of the packet the owed
reply answers, sent again because that reply is late, is answered by it
(roadmap D29). A second reply would go out only after the first and its
retunes, and taking a packet it may not answer in time would let its
sender hand the message to someone else as well.

#### Bans

A neighbour that refuses a packet or fails a hand-over is **banned**: no
forward or return goes to it until the ban ends. A message whose next
hop it is waits for it, unless the neighbour refused that message
([next hop](#next-hop)). Three rules set how long.

**A refusal bans for the wait it asks.** An ACK that refuses, answering
the outstanding hand-over, means the receiver does not take the packet
([receiving DATA](#receiving-data)). No failure is counted and the
neighbour is not tried. Its ban becomes the wait asked, in place of any
ban it had, shorter or longer: it is the neighbour's latest word. The
message keeps its place in the send queue and is routed again with
three fresh attempts: round the refuser while the ban lasts, to the
next candidate at once, and waiting for the ban only where no other is
left (roadmap D110). A node's own application's message does the same,
waiting longer where no other is left, as below (roadmap D115). A
refused return goes on as a forward. A refused delivery waits the told
wait and keeps the transmissions it has made. Silence, by contrast,
holds the message ([hand-over](#hand-over) step 6).

*Why a refused message goes round.* A refusal says the refuser has no
room for the message now: its wait for the way on is past a hop's
budget of 30 s, or its bucket is spent ([send queue](#send-queue)). A
neighbour beside it often has room, and the refuser has not taken the
message, so going round it leaves no copy. This holds at the origin as
anywhere: a node with several neighbours offers its own message where
there is room, while a sleepy node has one candidate, its anchor, and
is told to wait ([a sleepy node's own message](#a-sleepy-nodes-own-message)).
Where every way on is slow, every candidate refuses, the message waits
here, this relay's queue for its next hop grows past the budget, and
its gate refuses in turn: the wait travels back hop by hop to the
origin, where the application is told it.

**The told wait.** The ban is never shorter than 1.58 s: a relay refuses
a packet to send on when its frame and the ACK that would take it, at
most 138.6 + 17.0 = 155.5 ms, do not fit in its bucket and no hop place
takes it, and the bucket refills that in 155.5 ms / 9.83 % = 1.58 s
([duty cycle cap](#duty-cycle-cap)); trying sooner would only cost the
refuser another reply (roadmap D29). This ban is the unit of every
told wait: nothing in the protocol acts sooner.

A refusal tells its wait as one of 63 codes, since 0 means taken
([packet formats](#packet-formats)): codes 1 to 8 are 1 to 8 bans, and
each code above is an eighth longer than the one below, rounded down to
whole bans and at least a ban longer, so codes 1 to 16 are one ban apart
and code 63 is 3384 bans, 89 min. A longer wait is told as 89 min, and
the sender comes back early to be refused again (roadmap D54, D109).

The senders a relay refuses stand in a **line**, each told to come back
at its own place, so that they return one after another as room frees,
not together to be refused again at the cost of a frame and an ACK
each. A place is rounded up to a code, so a sender comes back at most a
ban, or an eighth of a long wait, late; places are at least a ban apart,
or they would get the same code. The wait told is:

- *For want of room*, the bucket: the time until the bucket holds
  everything queued, the frame and its ACK, with the reply reserve left;
  past the bucket's depth, its whole refill, by when what is queued has
  gone. Each sender so refused is told one ban later than the one
  before, the refill of the frame and ACK it will bring back.
- *For want of the way on*, at the gate ([send queue](#send-queue)), or
  for a message entering the network while its next hop is banned
  ([receiving DATA](#receiving-data)): a place in that next hop's own
  line, one service time after the last place given and at least a ban,
  or after the messages queued here for it if they take longer to go.
  The line's end is kept in the neighbour's entry, and an end in the
  past counts as now, so time shortens the line by itself. The sender is
  told its place less a hop's budget: the gate then takes it, unless
  more has come meanwhile. The service time is the neighbour's
  ([neighbour table](#neighbour-table)), or, if longer, how long the
  longest waiting message queued for it has waited since the neighbour
  last took one, its refusals' waits aside, so that a rising load shows
  before the next take.
- *For want of anything else*, a place for the message's record or in
  the delivery queue: one ban, in the room's line.
- In every case, never less than the rest of the ban on the
  packet's next hop, rounded down and without a place in line: the
  packet could not move sooner here.

*Why a line per next hop.* The senders refused for one next hop wait for
the same thing, the queue for it here to shorten to a hop's budget. It
shortens by about a message a service time, so the senders are spaced by
the time this relay takes to get one message through. The gate weighs
the queue, not the line: a place is a promise to a sender that will come
back, and counted against the gate it would refuse that sender, its own
place in its way.

*Why the next hop's ban.* A message refused here would wait here for its
next hop's ban whatever room it found ([next hop](#next-hop)). Told only
the bucket's refill, a sender would come back in a second or two to be
refused again, and meanwhile take messages entering the network, since
its own queue moves. Told the ban, it bans this relay as long, its own
messages for this next hop wait as long, its queue for it grows, and its
gate refuses in turn: the wait travels back toward the origin per next
hop, not per relay.

*Why a wait passed on stands outside the line.* The rest of a ban on the
way on is the rest of a wait told further on. It counts from when it was
told and is rounded down at each refusal, so it only shrinks as it is
passed back. Given a place in line it would grow at every relay, without
end around a ring of relays that refuse each other. A line is spaced by
this relay's own measure, which leaves out the waits the next hop told
([neighbour table](#neighbour-table)), so it does not compound either.

*Why these codes.* At the short end a wait is a few bans, the room's
line or a ban after silence, and places a ban apart need codes of their
own. At the long end a line under overload grows a service time with
each sender refused and reaches minutes, and a sender told too short a
wait pays a frame and an ACK to be refused again. Steps of an eighth
reach 89 min, more than an hour; steps of a ninth would reach only
44 min, and a code exact to the ban only 63 bans.

**Silence doubles the ban from 1.58 s to 61 s.** The ban after the k-th
failed hand-over in a row is 1.58 s doubled k − 1 times, 1.58, 3.16,
6.3 s and so on, up to 61 s, the longest anything waits for a bucket; a
longer ban the neighbour has already stands. An answer from the
neighbour resets its failure count and so the doubling
([neighbour table](#neighbour-table), roadmap D42, D106). The message
waits for the neighbour until three silences in a row demote it
([hand-over](#hand-over) step 6), so a link that is really gone costs
the first message two bans and two more hand-overs, and later messages
one hand-over, trying it last; [bounds](#bounds) adds up the rest.

*Why silence doubles.* A refusal measures what one hand-over takes from
the receiver's bucket. Silence says only that the channel or the
receiver is full, or the link gone, not how full. Without the doubling,
a neighbour that stops answering under load is offered every other
message at once, each offer up to three more frames where frames are
already being lost.

*Why not counted as tried.* An attempt is lost to a collision at the
receiver far more often than its ACK goes astray (roadmap D34), so the
neighbour most likely lacks the message. If it did take it, the next
offer costs one round trip: it holds the message and only ACKs, or has
finished with it and returns it (roadmap D36).

**A node's own message doubles its wait too.** Where no other
candidate is left, a node's own application's message waits after its
k-th refusal in a row the k-th ban after silence, or the told wait if
longer, and starts again from one ban once it is taken or dropped.
Every refusal in a row counts, gone round or not, so the origin offers
no faster for having more neighbours to be refused by. Offered every
1.58 s, it would put a full frame on the refuser's channel that often,
9 % of it, hiding its ACKs, and spend the refuser's bucket as fast as
it refills. A sleepy node's own message has a wait of its own
([a sleepy node's own message](#a-sleepy-nodes-own-message)).

### Routing

A message travels as a DATA packet ([packet formats](#packet-formats))
from relay to relay, each hand-over acknowledged
([hand-over](#hand-over)). A relay hands it to its neighbour closest to
the destination position. Where that leads into a gap, a relay from
which no neighbour leads on, the relays route around it by depth-first
search: a relay with no neighbour left to try returns the message to the
relay it first got it from, which tries its next neighbour.

Each relay keeps a small record per message
([message records](#message-records)), and the packet carries nothing
for the search but its sender and a hop count. The search only routes
around gaps; it never searches for an ID, though it hands a message to
the node it is addressed to where that is a neighbour. A sleepy node
that is a next hop ([next hop](#next-hop)) forwards as a relay does, and what this
section says of a relay holds for it then.

The **origin** of a message is the node that first sends it into the
network: the node a sleepy node handed its own message to, or a node
sending for its own application. The search starts there and, if it
finds nothing, ends there. The rules below are roadmap D11, D16 and
D17.

#### Packet sender

A node acts on a DATA packet whose `dst_hop` is its own ID
([node identity](#node-identity)); of any other it notes only the
sender's channel and signal ([neighbour table](#neighbour-table)) and
that a sleepy sender is in reach ([heard table](#heard-table)). The packet names
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
| handed | the neighbours that have had the message from this relay or offered it one: those that took a hand-over of it, a forward or a return, by ACKing it ([hand-over](#hand-over)), those that answered seen, and those whose copy this relay answered seen ([the ACK](#the-ack)), as one bit per neighbour-table slot |
| last packet | `src_hop` and `hops` of the last DATA of the message it received, and whether it was answered seen: the same pair again is a retransmission, answered as the first was ([receiving DATA](#receiving-data)) |
| highest hops | the largest `hops` ([hop count](#hop-count)) received for the message, or sent in a hand-over of it that was taken or answered seen |
| arrived | whether the message has reached its owner: this node owns the destination ([owner](#owner)), or the node it is addressed to took it from this one |
| last active | when it last received or sent the message |

The relay's **tried set** for a message is those known to have it: the
parent, the neighbours in `handed`, and a sleepy node that handed over
its own message. A neighbour that has neither taken nor seen the message
stays a candidate, whether it was passed over, being demoted, expired or
not in the table at the time, or was offered the message and refused it
or never answered ([hand-over](#hand-over)). One exception is kept with
the queued message, not the record: a neighbour that a failed hand-over
of the message left demoted is tried for it while it is queued here,
though it does not have it ([next hop](#next-hop)). When a neighbour-table slot
changes hands, its bit is cleared in every record and queued message.

The record is also how a relay recognises a repeat of a message it has
handled ([receiving DATA](#receiving-data)); there is no separate window
of recent IDs.

**Eviction.** A record has no lifetime. It is kept until the table needs
its place; a new record then replaces the least recently active one
whose message the relay does not hold. A relay holds a message while it
is in its send queue ([send queue](#send-queue)), the hand-over included,
or set aside for a full delivery queue. The table holds 667 records
([bounds](#bounds)). Without its record the message could not be routed
when its turn comes, so a message is never lost for want of room
(roadmap D31).

#### Owner

A node **owns** a message's destination when:

- `dst_id` is its own ID: it is the addressed node, wherever it is; or
- its own position is `dst_pos`; or
- it is within 50 m of `dst_pos` and no live, non-demoted neighbour is
  closer to it than itself.

*Why the third case.* The relay at that position has then probably died
or moved; 50 m is what two GPS fixes of a node that has not moved may
differ by ([position](#position)), so a relay this close is, for the
radio, in the same place.

A node farther than 50 m with no closer neighbour is at a gap, not the
owner, and the search goes on ([next hop](#next-hop)). Nor is a node
the owner by position while the addressed node is a live neighbour it
has not tried, other than a sleepy one with an anchor: it hands the
message to that node as the next hop. A sleepy or carried node owns
only what is addressed to its ID and sends anything else on
([nodes](#nodes)), and a sleepy or carried neighbour closer to `dst_pos`
does not keep a node from owning it. A node in its first
rotation owns no position but its own: its table may not yet hold the
closer relay ([channel at boot](#channel-at-boot)).

A message whose destination is the sending node's own address is
delivered there without entering the search. Otherwise the destination
position decides who holds it: a relay that hears the sleepy node but
does not own the position routes the message as any other, which sends
it to the node at once where the relay spares the long frame
([next hop](#next-hop)).

*Why not the heard table.* A sleepy node that has just moved to another
anchor stays in the old relay's heard table for a delivery TTL
([heard table](#heard-table)), so the table disagrees with the address
exactly when the address is new; delivering from the table would queue
the message for a node that has gone (roadmap D21).

The addressed node delivers the message to its application. Any other
owner queues it for the addressed node
([delivery queue](#delivery-queue)) and sends it when the node listens
([delivery](#delivery)), whether or not it hears the node now; a later
copy of a delivered message is only ACKed
([receiving DATA](#receiving-data), case 1). A message for a relay that
is gone waits there until a newer message needs its place, and ends.

#### Next hop

One function chooses where a queued message goes, from the state as it
is when it is asked: the neighbour table, the bans, the message's
record, and two marks its send-queue entry keeps, the neighbours that
refused it and those it held for until they were demoted (below). It is
asked for every queued message whenever a hand-over may start
([send queue](#send-queue)), and for each DATA that arrives
([receiving DATA](#receiving-data)). The **candidates** are the live
neighbour-table entries ([neighbour table](#neighbour-table)) except
those in the tried set ([message records](#message-records)).

The **order** is:

- the addressed node itself, if it is a candidate, before all others;
- neighbours with margin before those without;
- within each, neighbours that are not demoted before demoted ones;
- within each, distance to the destination position in the local plane
  ([positions on the wire](#positions-on-the-wire)), nearest first;
- equal distances by ID.

The relay sends to the first candidate once its ban has ended, and with
none returns the message to its parent. A ban does not take a neighbour
out: a banned next hop is waited for, and the message is routed again
when the ban ends ([overload waits](#overload-waits)). The one
exception is a neighbour that refused the message ([bans](#bans)): the
queued message keeps a bit per neighbour-table slot for its refusers,
cleared when the ban ends, and while a refuser is banned it is no
candidate for that message, unless that leaves none, or only a last
resort the refuser was not; then the message waits for the ban. A
failed hand-over sets no bit ([hand-over](#hand-over) step 6).

*Why a ban is a wait.* A neighbour that does not answer stays the first
candidate until it is demoted or gone, for the reasons of
[hand-over](#hand-over) step 6; the neighbour table holds the failures in
a row and the ban for every message alike, so the message needs no
state of its own for it (roadmap D106). A refusal is the one thing a
message keeps, since it says the neighbour has no room for that message;
the messages it did not refuse still wait for it, so a full next hop
fills this relay's queue for it and the wait goes back toward the
origin. A neighbour that arrives, recovers or expires changes the answer
the next time a message is routed, and nothing else does.

**Last resorts.** Some candidates are tried only when the search would
otherwise end here, the message going back to its parent or being
dropped ([return to parent](#return-to-parent)):

- a sleepy neighbour with an anchor, whose last heartbeat had the sleepy
  bit alone ([packet formats](#packet-formats)), sent to with the long
  preamble;
- a carried relay ([nodes](#nodes)) once nothing has been heard from it
  for its own heartbeat interval ([heartbeat](#heartbeat)).

A stale carried relay is no last resort for a message addressed to it:
it is first all the same. A sleepy neighbour with an anchor is first for
a message addressed to it while the relay **spares the long frame**,
and it is sent the message at once with the long preamble:

- the relay's bucket holds a full DATA frame with the long preamble,
  372.8 ms, beyond everything else it has to send: its send queue, the
  messages of its delivery queue at the ordinary preamble, and the
  reply reserve ([duty cycle cap](#duty-cycle-cap));
- no such hand-over to the node has gone unanswered since it was last
  heard.

Otherwise it stays a last resort, and the message goes on toward its
address, its anchor's position, where the anchor delivers it
([delivery](#delivery)). A hand-over to it unanswered in all its
attempts counts as any failed hand-over, and its message is routed
again at once. An anchorless neighbour is a candidate like any other,
sent to with the long preamble. The first hop
of a sleepy node's own message is its anchor while the anchor is live
([a sleepy node's own message](#a-sleepy-nodes-own-message)).

*Why a sleepy neighbour is a last resort.* A ban is no reason to go
through a node that sleeps. Where a relay is in reach, it is the hop;
the last resort is for a pocket with no relay in that direction, where a
sleepy node that has anchored is the way out for the anchorless ones
behind it (roadmap D58). A message addressed to it does not go through
it but ends there, which is the case below.

*Why a carried relay goes stale.* It walks: in its interval, at most
15 s, 21 m at 1.4 m/s (`node.rs` `WALKING_SPEED_M_S`), within the 50 m a
position is good to ([position](#position)); by the time its heartbeat
comes back to this channel, a rotation, it has walked 180 m on average
and 270 m at most, a hop's length in the dense city, and may have gone
indoors, 14 dB down ([physical layer](#physical-layer)). A DATA sent to
a stale walker mostly reaches nobody, and under load such frames were
most of the channel's overload (roadmap D103, D105). Its ACK makes it
fresh again ([the ACK](#the-ack)), so a walker in conversation stays a
next hop.

*Why margin comes first.* Fading lets a relay hear the 13-byte heartbeat
of a neighbour whose link usually cannot carry a full DATA frame
([neighbour table](#neighbour-table)). Such a neighbour is often the
farthest, so the nearest to the destination, and every message would
try it first, three frames unanswered. It stays a candidate rather than
left out, because on a sparse line it may be the only way on, and a weak
link still carries some frames; each it takes counts toward its margin
(roadmap D38).

*Why a demoted neighbour is still tried.* Demotion keeps a failing link
from costing every message its attempts while another route exists; it
should not drop a message whose only route is that link. Three
hand-overs lost to collisions demote a link that works, and on a line
it is the only route, so without this every message would be dropped
until the demotion lapsed, six and a half minutes. What a neighbour that
is really gone costs is in [bounds](#bounds) (roadmap D25, D36, D42).

**The one-way link.** A message whose hand-over to a neighbour fails and
leaves it demoted, by this failure or an earlier one, counts that
neighbour as tried while it is queued here, and its search goes on: to
the next candidate, back to its parent, or, at the origin, it ends. The
neighbour stays a candidate, last, for every message that has not tried
it. The mark is kept with the queued message, not in the record: the
neighbour does not have the message, and a copy it offers later is no
return ([receiving DATA](#receiving-data)).

*Why a demotion ends the wait.* A neighbour whose heartbeats this relay
hears but which never hears it stays live, its heartbeats renewing it.
Waited for as the only candidate left, it would hold the message for as
long as it beats, and this relay's part of the search would never end.
Three hand-overs in a row unanswered are the test that it does not hear
this relay, so there the message lets it go. The cost falls on that one
message: where three collisions demote a working link that is the only
way on, it is returned or, at its origin, dropped; the next message
tries the link last.

*Why the next hop need not be closer.* After the message comes back, the
best neighbour left is usually farther from the destination than the
relay itself, and such a rule would turn every gap into a dead end. On a
first send the nearest neighbour is chosen anyway.

*Why the addressed node goes first.* A node owns what is addressed to
its ID wherever it is ([owner](#owner)), and a carried node is seldom at
the position a message bears: the search for it ends only when it meets
the node, and otherwise goes through the city to the hop cap
([hop count](#hop-count)). Nothing is looked up: the relay reads its
neighbour table, which it ranks anyway. A stale walker costs three
frames unanswered at most, against hundreds of hand-overs that only it
can end. A sleepy node with an anchor is not lost that way: its address
is its anchor's position, and the anchor, which owns that position,
sends it the message ([delivery](#delivery)). The relay that hears it
saves the hops to the anchor and, where the anchor cannot spare the
long frame, the wait for the node's beat, a mean of 225 s
([beats](#beats)). Where relays hear one another's sleepy nodes it
also saves the texts that a search through them lost. The long frame
costs 2.7 times a full frame's air, so a relay sends it only from air
it has spare, as the anchor does. A condition on the node's channel as
well, the move threshold of [channel moves](#channel-moves), was
measured and is not used: it refused mostly on the channel where an
anchor serves its own nodes, and held their texts for the beat (roadmap
D134). One unanswered hand-over says the node is out of that relay's
reach, and its anchor, which hears its beats, is left to deliver.

The rule holds at the owner of the position too, which would otherwise
queue the message for the node's beat on its own channel, up to a
rotation away, but for a sleepy node with an anchor: the owner delivers
to it from its delivery queue, with the same bucket condition
([delivery](#delivery); roadmap D116a, D134).

A candidate may already have the message by another path; the relay
cannot know, since its neighbours listen on other channels. If the
message has arrived there, or the candidate holds a copy, the candidate
takes it and it goes no further; if the candidate has had the message
and holds none, it answers seen, and the relay goes on to its next
candidate ([receiving DATA](#receiving-data), [the ACK](#the-ack)).

#### Send queue

The send queue has a regular room of 39 places and one hop place for
each count from 1 to 254; the packet being handed over keeps its place.
A packet to send on enters the regular room only if the bucket holds
the reply reserve on top of the new frame, its ACK and everything
queued ([duty cycle cap](#duty-cycle-cap),
[receiving DATA](#receiving-data)), so it waits for the hand-overs
ahead of it, not for the bucket to refill. A hand-over waits for the
bucket only when its packet holds a hop place, or when retries, replies
or a delivery that goes first have spent it since its packet was taken
([delivery](#delivery)).

*What sets the 39.* The bucket holds at most 6 s
([duty cycle cap](#duty-cycle-cap)), and a full frame and the ACK that
took it on cost 138.6 + 17.0 = 155.5 ms, so ⌈6 s / 155.5 ms⌉ = 39. With
the reply reserve left in, a full bucket takes at most 33 full frames,
so for full frames the bucket is the limit; shorter frames fill the 39
places first, and 39 hand-overs of them go out within a few seconds.

A packet received with count h that the regular room refuses takes the
highest free hop place numbered h or lower, and keeps it until it leaves
the queue. A hop place holds its packet whatever the bucket holds; the
bucket still decides when it is sent. A packet received at 255 ends at
the cap ([hop count](#hop-count)), so 254 hop places are enough.

Of the 254 places, 64 can be held at once: a place is free when its
number is unheld and fewer than 64 places are held, and with 64 held the
relay refuses the packet as any other refusal ([bans](#bans)). Packets
with counts up to c hold at most c places, so holding 64 needs packets
of count 64 or more, and while fewer are held the places behave as if
all 254 were there. In the simulator they are almost never used: at
the design rate and at twice it no relay of the district held one, and
at the design rate in the city at most 3 at once, at 18 of 20 269 nodes.
The 64 cut the send queue and the records it sets from 293 entries and
1613 records to 103 and 667: 190 entries of 304 bytes and 946 records
of 40 bytes, 95 600 bytes of RAM (roadmap D155).

A message entering the network, a sleepy node's own with count 0 or a
node's own app's, takes no hop place, so overload stops new messages
where they enter, at the app ([addresses](#addresses)).

**The gate.** Before either, a relay takes a message to send on only if
its wait here would be within a hop's **budget**, 30 s, or its
count is higher than every one of theirs ([receiving DATA](#receiving-data)).
Its wait is its next hop's service time, as that next hop's line takes it
([bans](#bans)), for each message queued here, deliveries aside, that goes to the same
next hop now. The count of a queued message is the one it holds here,
one less than the count it is sent with. Every queued message is
counted where routing sends it now ([next hop](#next-hop)), not where it
was going when it came, so the gate needs no state beyond the queue and
the neighbour table. It refuses the rest, each with a place in that
neighbour's line ([bans](#bans)).

*Why a wait per next hop.* What a relay holds for a neighbour beyond
what keeps the way to it busy only waits, and its wait is latency for
every message behind it. Refused instead, a message waits at the relay
before, whose queue for the way grows in its turn, and the wait travels
back toward the origin, where the application is told it
([bans](#bans)). A room per relay could not do this: a banned
neighbour's messages would fill it and keep out the messages for every
other way. It weighs time, not a number of messages: a way at its
neighbour's cap takes one message a ban, one with free air ten in that
time, so no one number suits both (roadmap D106).

*Why 30 s.* The budget should refuse as soon as a hop is slower than
it is at the design's rate, and never at that rate: 30 s is the first
round figure above the longest wait the gate meets at that rate in the
simulated city. A text takes 8 to 15 hops there, so the budget bounds
the wait in relays' queues at 4 to 8 minutes, the time of the
hand-overs aside (roadmap D106).

*What it costs.* Messages for one next hop that arrive faster than it
takes them, the last no further travelled, are not all held: the ones
past the budget wait their place in line at the relay before, at the
cost of a refused DATA frame and an ACK each. At the design's rate that
is rare; under load it is the price of the wait reaching the origin.

**Choosing what to send.** Whenever no hand-over is under way and the
radio is free, the relay routes every queued message from the state as
it is then ([next hop](#next-hop)); a message whose next hop, or parent,
is banned is not sendable until the ban ends. Of the sendable, packets
are handed over in the order they arrived, except that:

- a delivery goes first ([delivery](#delivery));
- a packet that has waited since it was queued, for a ban or a wait of
  its own, goes ahead of those that have not, the most travelled first
  ([overload waits](#overload-waits)).

The chosen hand-over then waits for the bucket if it must
([duty cycle cap](#duty-cycle-cap)); a shorter frame behind it does not
go first, since with the bucket at the cap it would always fit sooner
and the longer frame would never go. Routing every message costs at
most the queue's entries times the neighbour table's. Of it each entry
keeps where it was last routed and since when, for the service time
([neighbour table](#neighbour-table)), and whether its next hop has
been banned since it was queued; the relay keeps when the earliest ban
that held a message back ends.

*Why route at each chance.* The table changes between one chance to
send and the next: a ban ends, a neighbour arrives, recovers or is
demoted. Routed only when its turn came, a message waiting for one ban
would miss a better way that opened meanwhile.

#### Receiving DATA

A relay answers a DATA packet addressed to it
([packet sender](#packet-sender)) with an ACK that takes or refuses it,
only when it owes no other reply, and acts on a packet it takes once
the ACK has gone out ([the ACK](#the-ack)). Whether it takes the packet
depends on what the packet gives it to do:

- **Something to send on.** It routes the packet as the record it would
  have routes it ([next hop](#next-hop)), and takes it only if it passes
  the gate ([send queue](#send-queue)); then into the regular room of
  its send queue if the room has a place and the bucket holds, as it
  stands, the ACK, the airtime of everything queued and of the new
  frame, and the reply reserve ([duty cycle cap](#duty-cycle-cap)), else
  into a hop place, whatever the bucket holds. So a message already in
  the network is refused only at the gate or when the hop places it may
  use are taken, whatever the bans: [overload waits](#overload-waits)
  needs every such message to move on (roadmap D34).
- **Fresh work**, a message entering the network here: a sleepy node's
  own (count 0, [hop count](#hop-count)) or the node's own
  application's. Besides the above, without a hop place, it needs a
  live neighbour that is not banned, demoted or not, since the message
  has tried none, and is refused while the neighbour it would go to,
  were none banned, is banned: the way on has pushed back. A relay with
  no live neighbour at all takes it, and the search ends as in
  [return to parent](#return-to-parent).
  - From its own application a node takes one message at a time, until
    its first hop takes it, and a public node's phones take turns at it
    ([phone link](#phone-link), Public): while it is running, holds none, has room
    in its send queue and bucket for a full frame on top of what they
    hold, and not every neighbour is banned, or, for a sleepy node with
    a live anchor, not the anchor
    ([a sleepy node's own message](#a-sleepy-nodes-own-message)). The
    message then passes the gate at count 0 and its next hop must not
    be banned; failing either it goes back to the application with the
    wait a refusal would tell, at least a ban.
  - One from its own application for a sleepy node it holds for, when
    the delivery queue is full, is kept aside until the queue has a
    place.
- **Something to deliver.** It takes the packet only if its bucket holds
  the reply reserve and an ACK and it has room for the message: in its
  delivery queue for another node, in its inbox for its own application
  ([delivery queue](#delivery-queue), [phone link](#phone-link)).
- **No work**, cases 1, 3, 4 and 6 below. It answers the packet, taken
  or seen, even when it would refuse work: a refusal would send its
  sender on with a second copy.

Otherwise it refuses: an ACK that refuses on the channel in `aux`,
nothing changed, and the sender waits and offers it again
([bans](#bans)). A relay that cannot afford even the reply stays
silent, and its sender retries as for a lost ACK (roadmap D27, D29).

*Why fresh work needs a free way on.* A relay whose neighbours are all
banned would only hold the message until a ban ends, and its offer could
be refused again; refused here, the message waits at its origin, where
the application sees it (roadmap D34). Taken against a next hop's
pushback, it would only wait and age here; each neighbour's pushback is
its own ban, so it holds up no other next hop and ends with the ban. A
message already in the network is spared, because its sender would wait
for this relay while this relay waits for the way on, and around a ring
of relays such refusals would keep each other up (roadmap D54).

The packet is one of the following, tested in this order:

1. **Arrived:** the relay has delivered the message or holds it for a
   sleepy node, or its record says the message has arrived
   ([message records](#message-records)). ACK only.
2. **New:** the relay has no record for the message. It creates one,
   with the sender as parent (none if `hops = 0`). If it owns the
   destination ([owner](#owner)) it delivers; otherwise it chooses a
   next hop and sends.
3. **A retransmission:** the sender and the count are those of the last
   packet of the message the relay received: the sender's ACK was lost
   and it sent the same packet again. ACK only, the answer the first
   packet got: taken, or seen (case 6).
4. **A copy while the relay holds one:** the message waits in its send
   queue or is set aside (above). ACK only: the two copies merge, and
   the relay's goes on. A hand-over whose frame has gone out does not
   count as holding one: its receiver may have taken it with only the
   ACK lost, and this packet may be that copy come back.
5. **A return, and the relay holds none:** the packet comes from a
   neighbour in the record's `handed` set, or from the neighbour the
   relay is still handing the message to, which also ends that
   hand-over: the neighbour has the message. The relay takes it and goes
   on with its own search from its record: it sends to its next
   candidate, returns the message to its parent, or, at the origin,
   drops it. A copy from anywhere for a destination the relay owns now
   (a closer neighbour may have been demoted since) is taken the same
   way and delivered.
6. **A copy from elsewhere, and the relay holds none:** seen. The relay
   answers with the seen flag ([the ACK](#the-ack)), takes nothing and
   changes nothing but its record: the sender has the message and
   searches on from its own record, so it is tried here. The sender
   counts this relay tried and goes on to its next candidate at once
   ([the ACK](#the-ack)).

In every case the relay records the packet's sender and count as the
last packet, with whether it was answered seen, raises its highest count
if the packet's is higher, and updates its last-active time.

Cases 1, 3, 4 and 6 end a packet without acting on it, and in each the
message lives on: it has arrived, the packet was answered when it first
came, another copy goes on, or, seen, the copy stays with its sender, so
no case ends the last copy of a message
([overload waits](#overload-waits) lists what does).

Nothing is sent straight back, and no copy searches twice over the same
ground: a relay that has had the message goes on from where its own
search stands when the message returns to it, and a copy that reaches
it from elsewhere searches on from its sender's record, past this
relay. A return goes from a relay to its parent, and the parents, each
set when its record was made, lead back to the origin without a loop,
so a message cannot go back and forth between two relays that hold none
of it (roadmap D35).

*Why a return is told by the record, not the packet.* The packet names
its sender, and the record says whether that sender has the message
from here. A child whose every ACK was lost is not in `handed`, so its
return is answered seen and ends there, and what it searched stays in
the relays' records for the parent's other copy, if any, to meet as
seen. That is the one case in which a copy ends at a relay that holds
none, and it takes three hand-overs to that child failing with every
ACK lost, which demote it ([hand-over](#hand-over) step 6; roadmap D35,
D90).

#### Return to parent

When [next hop](#next-hop) finds no candidate, every neighbour having
been tried, the relay sends the message back to its parent: DATA with
`dst_hop` the parent's ID from the record, on the parent's home channel
as the neighbour table has it at the moment of sending. The return is
routed like any message, the parent its only candidate: it waits while
the parent is banned, and a neighbour that arrives meanwhile, untried,
takes it forward instead.

So a return follows a parent that has moved home since
([channel moves](#channel-moves)). Only a parent no longer in the table
is sent to on the channel in the record. The parent takes it as a
return ([receiving DATA](#receiving-data), case 5) and continues with
its next neighbour. A parent that answers seen has gone on without this
relay's copy, which it never knew the relay had: the relay drops it,
having nothing left to try ([the ACK](#the-ack)).

The origin has no parent. With no candidate left it drops the message:
no relay that can be reached from it through neighbours that answer owns
the destination. Nobody is told, the origin's own application included
([addresses](#addresses), roadmap D10): that is the cost of having no
end-to-end acknowledgement.

An origin in its first rotation that knows no relay yet holds its own
application's message instead, and routes it at the first relay it
hears, or when the rotation ends
([channel at boot](#channel-at-boot)).

*Why hold it.* Within seconds of its boot a node has heard no relay
yet, though its burst brings it the relays in range within about 15 s
([heartbeat](#heartbeat)): dropped then, the person's first text would
be lost to the boot alone.

A message whose next hop is banned is neither returned nor dropped: it
waits ([overload waits](#overload-waits)), a node's own application's
message as in [bans](#bans).

A return whose parent is demoted, or has left the table, is dropped
and the search ends early ([hand-over](#hand-over) step 6). A parent that refuses the
return has not failed, and a busy channel is no attempt: the return
waits for it to clear ([hand-over](#hand-over) step 2, roadmap D111).

#### Hop count

`hops` in the DATA header counts hand-overs:

- A sleepy node with an anchor sends its own message with 0
  ([a sleepy node's own message](#a-sleepy-nodes-own-message)): the node
  that takes it is its origin, the message has no parent, and the sender
  counts as tried, so the search never returns to a node that sleeps.
- A relay sending for its own application sends 1, as if it had received
  the message at 0, since 0 means "a sleepy node's own, start the search
  here".
- A relay sends its record's highest count plus one
  ([message records](#message-records)), whether forwarding or returning
  to its parent.

The count is set when a hand-over's first frame goes out, and its
retries repeat it ([hand-over](#hand-over)); a frame that waited in the
send queue, or goes to another neighbour after a failed hand-over,
carries the count as it stands when it goes.

*Why not start higher.* Sent with 1, the message could take hop
place 1 at its first hop, which overload must not give to fresh work
([send queue](#send-queue)).

So along the path of a message the count only grows, and a relay sends
a count for a message once, but in retries: to send another packet of
it, it must have received one since its last hand-over that succeeded or
was answered seen, which spends the count the receiver recorded, or that
hand-over failed and the packet goes elsewhere. After a wait for a ban
it may also go again to the neighbour that did not answer; if that
neighbour took the first, the second is in fact a retransmission to it.
A receiver therefore tells a retransmission by the sender and the count
([receiving DATA](#receiving-data), case 3). The count also numbers the
hop places of the send queue.

A relay that would have to send a count above 255, the largest the
one-byte field holds, drops the message, except an owner delivering it
to its node ([delivery](#delivery)), which sends 255: the search has
ended there, and dropping the message at the last step would leave it
in the delivery queue to be dropped again at every try.

*Why the cap.* A search that keeps its records ends on its own
([why the search finds a path](#why-the-search-finds-a-path)); the cap
ends one that has lost its way because a record was replaced or the
message split in two, and it ends the chain of hop places
([overload waits](#overload-waits)). 255 is generous: the longest
straight route across the city of
[capacity and sizing](#capacity-and-sizing) (≈30 km at the ≈750 m
street range of [physical layer](#physical-layer)) is about 40
hand-overs, leaving about 200 for routing around gaps.

#### Overload waits

Overload makes a message wait and never ends it. A message whose next
hop is banned, by a refusal or by silence, waits in its place in the
send queue, its airtime still counted, until the ban ends, and is
routed again then, at the origin or anywhere else ([next hop](#next-hop)).
A message refused by that next hop itself goes round it where another
candidate is left ([bans](#bans)).
A message that has waited since it was queued, for a ban of its next
hop or a wait of its own (the node's own message, refused or with no
neighbour to take it), goes ahead of every message that has not, the
most travelled first, for as long as it is queued: after its offer
too, and whatever next hop it is routed to later. The messages that have not waited keep
their order, since ordering them all by count would starve those that
have just entered.

A message ends only:

- when it is delivered;
- when its search runs out of neighbours
  ([return to parent](#return-to-parent));
- when a return's parent is demoted or gone
  ([hand-over](#hand-over) step 6), or answers it seen
  ([the ACK](#the-ack));
- when a newer message needs its place in the delivery queue
  ([delivery queue](#delivery-queue)), or when the relay holding it
  comes to own its destination while it is queued and finds that queue
  full ([owner](#owner));
- at the hop cap ([hop count](#hop-count));
- when a node holding it dies (roadmap D31, D44, D106).

Never by age on its way: at its owner the delivery queue keeps it by
its own rule ([delivery queue](#delivery-queue)).

*Why no end by time.* Overload makes a message wait where it is
refused, at the neighbour that refused it ([bans](#bans)), so a relay
that cannot keep up fills the queues of the relays that send to it, and
they refuse in turn, back to the origin, where the application is told
to wait ([receiving DATA](#receiving-data)). An end by time would throw
away what the network has taken and could still deliver; a person
would rather have a text late than lose it after the network took it
(roadmap D44, D106). The cost: under a long overload the messages the
network has taken outrank new ones, and the origins are refused while
older messages are still carried.

*Why hop places.* Waiting alone can lock relays up: two neighbours whose
queues are full of messages for each other would refuse each other
forever, since what a relay holds drains only as it sends (roadmap
D29). The hop places of the send queue prevent it, and the gate keeps
them able to.

Take the highest count held at any relay, k. Its message is sent with
k + 1 or more. At the neighbour it is offered to, every queued message
holds k or less, so the gate lets it in, and place k + 1 is free there
as at every relay, since a place is never numbered above its message's
count, so the queue takes it, unless its count is at the cap, where it
ends. That message moves, then the next highest, and so on down to the
regular room. With 64 places held at a relay the argument stops there:
it refuses, and a stall is bounded by the gate's budget, after which the
message goes round its refuser where another candidate is left
([bans](#bans)), back to its parent once none is
([return to parent](#return-to-parent)), or ends as
[overload waits](#overload-waits) lists. That is the overload case
only, past what the simulator has shown at twelve times the design
rate (roadmap D155).

*Why the gate does not lock up.* A send queue of one place per relay,
which held whatever arrived first, locked up under crossing traffic
once relays held messages for each other (roadmap D29). The gate
differs in the two ways the argument above needs: it bars one next
hop, not the relay, so a message for any other next hop still enters; and a message with a higher count passes it, so the
most travelled message always finds room.

*Why the most travelled first.* For this the message that has come
farthest must be offered when a ban ends: a less travelled one offered
first would be refused and ban the neighbour again, each time. The gate
lets a higher count in whatever the wait on its way, so the message
offered first is one the next relay's gate lets in.

The same holds at every offer to a loaded way, not only when a ban
ends: the gate refuses a lower count once the way's wait passes the
budget, so a less travelled message offered first there costs a DATA
frame and a refusal and bans the neighbour again. A message that has
waited has met a loaded way and is likely to meet one again, so it
keeps its place ahead for as long as it is queued: the messages that
meet the gate go by count, and those on free ways keep their order.
Ending that place at the offer, or when the message is routed to
another next hop, refused more and delivered less under overload;
ordering every message by count left the least travelled longest in
the queues (roadmap D114).

### Why the search finds a path

Suppose that while a message is being routed the links do not change,
every hand-over to a neighbour that answers succeeds within its
attempts, and no record is replaced. Then:

- The message moves as a single packet: a relay sends only what it has
  just received or originated, and every ACK-only case of
  [receiving DATA](#receiving-data) sends nothing.
- Each stay of the message at a relay ends with a hand-over that
  succeeds, or at the origin with a drop ([next hop](#next-hop),
  [return to parent](#return-to-parent)), and there are finitely many of
  those: a relay hands the message to each neighbour at most once, since
  one that takes it is tried ([message records](#message-records)), and
  the parents lead to the origin without a loop. So the search ends.
- A neighbour that does not answer costs a bounded number of failed
  hand-overs ([bounds](#bounds)), and the hop cap bounds the stays
  ([hop count](#hop-count)).
- A send to a relay that already has the message and holds none of it
  is answered seen (receiving DATA, case 6): the sender marks it tried
  and goes on with its own candidates, and the relay's own search, if
  unfinished, goes on when the message returns to it from its child. So
  a relay met again is neither searched twice nor skipped: its
  neighbours are its own to try, and the message stays a single copy. A relay whose
  search is unfinished gets the message back through the parents, so
  none is left with neighbours untried.

So the message makes a depth-first traversal of the relays reachable
from the origin through neighbours that answer, nearest to the
destination first. A depth-first traversal visits every relay it can
reach; it stops at the first owner, or returns to the origin when there
is none. If a path to an owner exists, the message finds it, provided
the traversal fits in the 255 hand-overs of [hop count](#hop-count).
The search cannot stall on a seen answer, since the sender keeps the
message and its next choice is its own; nor loop, since the message
moves only when a hand-over is taken, and each relay hands it to each
neighbour at most once.

What breaks it:

- **A lost record.** A message returning to a relay whose record has
  been replaced is new there; the relay searches again from itself, and
  the relays above it never get the message back. The search ends early,
  or wanders until the hop cap ends it.
- **A split.** A hand-over whose ACKs were all lost looks failed, so the
  sender moves on while the receiver also carries the message: two
  copies search at once. They share the relays' records: a copy that
  reaches a relay the other has crossed is answered seen and searches on
  past it, into relays neither has reached, or back up its own parents.
  A copy ends where it meets the other held at a relay (receiving DATA,
  case 4), where the message has arrived (case 1), at the origin with
  the search, at the hop cap, or as a return to a parent that gave the
  child up and answers seen (case 6). Only the last can end a copy that
  is the last one, and it takes every ACK of three hand-overs on one
  link lost.
- **Links that change** during the search, including a parent that moves
  to another home channel: parts of the graph go unsearched.

### Bounds

| | |
|---|---|
| hand-overs per message | at most 256 that succeed (the first, at count 0, and 255 more, [hop count](#hop-count)) |
| a neighbour that does not answer | at most 11 failed hand-overs, 33 frames, at a relay, whatever the messages |
| a message held on one neighbour that does not answer | at most 14.2 s: two waits, of 1.58 and 3.16 s, the ban's first two steps, since the third failure in a row demotes the neighbour and moves the message on, and three hand-overs of at most 3.15 s on a free channel ([hand-over](#hand-over) step 6); one that answers this relay between failures holds it while its bans last ([bans](#bans)) |
| life | no bound by time on its way: it ends as [overload waits](#overload-waits) lists; then in its owner's delivery queue until its node takes it or, once stale, its place is needed ([delivery queue](#delivery-queue)) |
| cost of a search | each link between two relays at most two hand-overs that succeed from each end, a send and a return, or one frame and its seen answer |
| message records | at most 667 per relay ([message records](#message-records)) |
| state per relay | the RAM budget of [implementation notes](#implementation-notes) |

*What a neighbour that does not answer costs.* Each failed hand-over to
it bans it for every message, the ban doubling up to 61 s
([bans](#bans)), until it leaves the table 390 s after its last
heartbeat or answer: the bans 1.58, 3.16, 6.3, 12.7, 25.3 and 50.6 s add
up to 100 s, and 61 s bans fill the rest. After three failures it ranks
behind the others ([neighbour table](#neighbour-table)) and the message
that waited for it moves on, so the first message pays two bans and
three hand-overs, 14.2 s, and every later one a hand-over, 3.15 s, until
the demotion lapses and the count starts again. A hand-over is at most
three attempts of 738.6 ms, listen-before-talk until it gives up, a full
frame and the ACK timeout, and the retry waits, 0.93 s
([hand-over](#hand-over)). A channel that stays busy adds its wait to a
forward's or a return's hand-over, but no airtime (step 2). A neighbour
that answers this relay between failures is not demoted and holds a
message as long as its bans, with no bound by count: it hears this
relay, and each offer is another chance for the frame to get through;
if it refuses the message, the message goes round it where another
candidate is left ([next hop](#next-hop)).

*What a search costs.* In the usual case it is two hand-overs in all
per link; a split adds a round trip on the link whose ACKs were lost. A
link that does not answer costs three attempts each time the message is
routed past it, so a search that comes back through a relay often, as
one with no owner does, pays for it at every stay (roadmap D36). Within
the hop cap a search can explore about 64 to 128 links beyond the route
that led into the gap.

*Why that many records.* A message sent on can come back, returned by a
child or offered by a split copy, and its record must still be here
then. On its way it may wait at each relay as long as a full send queue
takes to drain, each of the 103 places, 39 regular and 64 hop places,
waiting out one ban of 1.58 s: 162.9 s. A ban after silence lasts up to 61 s, so where every way on is
silent a message can wait longer; its own record is kept while it is
held, and this bound covers a wait of one ban per place behind
refusals only (roadmap D42); a refusal may ask for more
([bans](#bans)), and a message held longer than the records last can
come back to a relay that has replaced its record, where the hop cap
ends a search that loses its way ([hop count](#hop-count)).

Every message a relay handles costs at least an ACK (18 bytes, 17.0 ms)
and one DATA packet with no payload (28 bytes, 22.1 ms,
[physical layer](#physical-layer)), together 39.0 ms.

Any 162.9 s carries at most a full bucket and its refill,
6 s + 9.83% × 162.9 s = 22.0 s of airtime
([duty cycle cap](#duty-cycle-cap)), so a relay handles at most 564
messages in that time. The records of held messages are never evicted
and may be older, so the table has room for the 103 a send queue holds
on top: 667.

---

## Sleepy nodes

A sleepy node ([nodes](#nodes), roadmap D71) beats on its anchor's
channel ([beats](#beats)), takes what its holder has for it in the
window after ([delivery](#delivery)), and between its windows receives
duty-cycled
([duty-cycled receive and the long preamble](#duty-cycled-receive-and-the-long-preamble)).

A sleepy node holds mail for nobody. The mailbox is a relay's, kept so
that it can deliver to a sleepy node in its window without the long
preamble ([heard table](#heard-table),
[delivery queue](#delivery-queue)). A sleepy node keeps the neighbour
table and the send queue, and forwards what it is given at once, with
the long preamble to a sleepy neighbour and the short one to any other.
Everything else it does as any node. A carried relay keeps its mailbox
but takes no new mail into it. A node that turns sleepy hands on what
its mailbox still holds (plugged in and unplugged, below).

For a node that has gone dark the network keeps, for the delivery TTL,
the node in its holder's table and the messages waiting for it, which
stay longer while nothing needs their place.

**Plugged in and unplugged.** At boot a node is sleepy or not by its
power at once (roadmap D71): nobody depends on it yet. A sleepy node
plugged in is a relay at once:

- it leaves its anchor;
- its address becomes its own position: the place it has, or its first
  fix within a search's limit if one comes ([position](#position)),
  which it keeps from then on unless it is carried;
- it heartbeats as a relay, the first time at once
  ([heartbeat](#heartbeat)). Its old anchor hears that heartbeat on the
  node's home channel until then, its own, and hands over what it holds
  for the node.

A fixed relay unplugged changes nothing for a **grace** of one hour: it
relays, owns and holds mail as before, and power back within the grace
is as if nothing happened. After the grace it relays on until its
**battery is low**. A carried relay has no grace: unplugged, it turns
sleepy at once, and so does a relay on battery that turns carried. A
relay on battery whose battery is low turns sleepy, grace or not. A
node its battery turned sleepy relays again when its battery is high
while it is fixed, and any node on external power. A fixed relay that reboots on battery is
a relay, its grace counted from the boot
([implementation notes](#implementation-notes)).

| | |
|---|---|
| battery low | a calibrated reading at or below 3.741 V, until one at or above the high level, or external power |
| battery high | a calibrated reading at or above 3.775 V |
| reading | the cell's voltage, read on battery once a minute while the radio does not transmit |
| calibration | after 3 h on external power, the first reading an hour into battery is a full cell's, 4.177 V, and every reading after it is scaled to match; a scale beyond ±3 % is not taken; the scale is kept across a reboot, and before the first one readings are taken as read |

The three voltages are a typical Li-ion cell's open-circuit voltage at
20, 30 and 100 % of its charge, read after an hour's rest (Maxim
Integrated, Application Note 4189, Table 1).

*Why a fixed relay relays on.* In a grid outage every relay in a
district loses its power in the same second; if each turned sleepy an
hour later, every sleepy node there would lose its anchor in the same
minute and go anchorless ([sleepy nodes alone](#sleepy-nodes-alone)),
and the domain's channels would fill with long frames. On battery a
fixed relay draws about 6.4 mA (5.8 mA listening, 32 mA for its
airtime), so 80 % of the 850 mAh cell lasts it four days; the fifth
left keeps it asleep for twelve and a half more at 0.56 mA ([delivery](#delivery)).

*Why a carried relay has no grace.* A fixed relay relays on for the
sleepy nodes that anchor to it. A node in a pocket holds no mail and
nobody anchors to it ([nodes](#nodes)), so relaying on would keep
little for anyone, and an hour of it would cost the node 0.68 % of its
850 mAh battery, listening at 5.8 mA instead of sleeping, each time it
leaves its charger (roadmap D152).

*Why those levels.* At 20 % a relay has relayed through four fifths of
its cell and keeps the rest for its owner. The curve is flat there,
34 mV from 20 to 30 %; the gap is several times a reading's noise and
the load's drop across the cell, so the role does not flap, and a cell
rises without external power only on a solar box, which then relays
again.

*Why it calibrates.* The SAADC's gain error at gain 1/6 is up to ±3 %
(nRF52840 Product Specification, SAADC), which at 3.74 V spans 4 to 57 %
of a cell's charge. A cell that has finished charging and rested reads
the curve's 100 %: the T-Echo's 500 mA charger brings an empty 850 mAh
cell to its end voltage within 1.7 h, its constant-voltage end is
assumed to take under 1.3 h more, and AN4189's readings rest an hour.
Scaled so, a board is left with the charger's tolerance on its end
voltage, about 1 %, and turns between 14 and 32 %. A scale beyond the
gain error is of a cell that was not full. A relay unplugged from a full
charge calibrates within its first hour, days before it is low; only a
discharge begun short of full, before any calibration, turns anywhere
between 4 and 57 %.

*Why once a minute.* A reading a minute late is under a tenth of a
percent of the cell at the most a board draws, below a reading's noise,
and a reading costs a sample of microseconds.

*Why an hour.* A charger moved across the room then costs nobody
anything, and a node taken off its charger to be carried away is seen
carried within it ([position](#position)).

When it is to turn sleepy the node takes no more mail, owning nothing
by position, and flushes its delivery queue, listening all the time until
each message has had its attempt, or for a delivery TTL at most; a
carried relay has flushed already, at its first far fix.

*Why a TTL at most.* The flush goes through the duty budget as any
delivery, and where the channels are full it could wait for hours with
the node listening at full current. A TTL is what the network promises a
node it cannot reach.

Then it is sleepy, and stays carried or fixed as it was. It takes the
strongest relay it knows as its anchor and listens a rotation, as at a
first choice ([relay choice](#relay-choice)), or is anchorless with
none. Its sleepy nodes hear no more slot heartbeats of it, and leave it
as an anchor heard sleepy. What the flush did not deliver it **hands
on**: each message goes into its send queue, oldest first, as a forward
from here toward its address, with the count it arrived with
([hop count](#hop-count)), admitted as any forward received with that
count ([send queue](#send-queue)), and routed as any other
([next hop](#next-hop)), through its anchor where that is its only
neighbour; and a message that finds no room waits in the delivery queue
until a hand-over ends and makes some. Its search starts here, as at an
origin: with no neighbour at all it waits for one, as the node's own
message does ([a sleepy node's own message](#a-sleepy-nodes-own-message)).
Nothing the node held is dropped at the turn.

*Why hand it on.* A message ends only as
[overload waits](#overload-waits) lists (roadmap D31), and a node that walks off or runs low is none of those.
Its address is still where the network can find the message's node: the
relay that owns it now, or the node itself as a neighbour, takes it on.

### Relay choice

Listening on any one channel, a node hears every relay in range once per
rotation ([heartbeat](#heartbeat)), and keeps it in its neighbour table
with the averaged RSSI of what it heard from it
([neighbour table](#neighbour-table)).

**First choice.** A sleepy node takes as its anchor the first relay it
hears, at once, so it is anchorless only while it hears none. Only a
fixed relay can be an anchor ([nodes](#nodes)). A sleepy node whose
last heartbeat had the anchorless bit is none either: it holds and forwards
messages but leaves that mode at the first relay it hears, so an address
pointing at it would soon point at a node that holds nothing
([sleepy nodes alone](#sleepy-nodes-alone)).

The node then stays awake a worst-case rotation, by when every relay in
range has visited its channel, and takes the strongest it heard, without
the margin below. The node listens on the anchor's home channel, and its
address follows the anchor ([addresses](#addresses)).

*Why a rotation awake.* Receiving duty-cycled, it would hear no other
relay's heartbeat, and the first relay heard would stay its anchor
however faint. Moving to each stronger relay as it is heard would chase
the fading instead: one heartbeat of a relay no better is above the
anchor's average half the time (roadmap D49). No margin is needed then:
few can have its address yet, so the move costs little.

**After a reboot.** A sleepy node that boots with a stored anchor
([implementation notes](#implementation-notes)) takes it at once, with
no first rotation, beats to it at once and once more within a heartbeat
interval, and listens a rotation for its heartbeat as when it checks the
anchor (below).

*Why at once.* Its address is the anchor's position, which its senders
still use, and its holder may hold mail for it.

**Switching.** While it listens, at its first choice and when it checks
the anchor, a node moves to another relay only when that relay has been
heard over at least two heartbeats and its averaged RSSI is at least
8 dB above the anchor's.

*Why that margin.* It is two standard deviations of the difference, so
that a relay no better takes the node on 2.3 % of comparisons. A relay's
heartbeats reach the node's channel about 130 s apart, longer than the
60 s the fading lasts, so they are nearly independent.

Under fading of σ = 4 dB (the scenarios' figure) a mean of two
heartbeats has σ/√2, and the difference of two such means
σ√(1/2 + 1/2) = 4 dB. The neighbour table's average lets each new
frame count half, which varies less than a mean of two (σ²/3 against
σ²/2), so 8 dB is a little more than two standard deviations there.

**Asleep.** A sleepy node keeps its anchor's neighbour entry whatever
its age, and wakes for the anchor's next heartbeat on its channel, at
the time the anchor's interval gives. It listens from as early as the
heartbeat can come to as late, and sleeps again as soon as it hears it:

- It keeps the time it expects from the earliest heartbeat heard, moved
  later only by the drift of two clocks, ±50 ppm each, since a late one
  says only that the anchor was busy.
- As early is the time expected less three times the drift since the
  last heartbeat heard, at most 59 ms over a rotation: once for the
  drift itself, and twice because a heartbeat held up by a busy anchor
  may have moved the time expected a drift later while its slot was a
  drift early (roadmap D52). As late is the drift, the heartbeat's
  greatest delay and its airtime more.
- A heartbeat of the anchor heard before the watch opens is no slot's:
  an answer to a node without an anchor on its channel, or the one after
  a move ([channel moves](#channel-moves)). It shows the anchor alive
  and moves nothing (roadmap D62).
- A heartbeat of the anchor's burst ([heartbeat](#heartbeat)) is in no
  slot either. A node whose anchor's last heartbeat heard was of its
  burst times the slots from the next one it hears while it chooses, and
  if its choice ends without one, checks the anchor (below).

*Why it wakes.* Between its windows a sleepy node receives duty-cycled,
which catches no heartbeat with the ordinary preamble, so it hears its
anchor's rotating heartbeat only if it listens when the rotation brings
it to its channel. Hearing the heartbeat is how the node knows the
anchor is alive, and how it follows it to a new home channel: the
heartbeat names the anchor's home, and the next on the new channel comes
when the rotation brings it there.

**Checking the anchor.** Two rotations without the anchor's heartbeat
mean it is gone: the node listens a worst-case rotation, as at its first
choice, keeps the anchor as soon as it hears it, and else takes the
strongest relay it heard. Two kinds of node keep the anchor all the
same:

- One that heard nothing at all is out of reach, in a pocket, a basement
  or the metro, not without its anchor: it keeps the anchor, watches for
  it as before, and checks again only once it has heard any frame again.
- One that heard frames but no relay keeps it too: giving it up for
  anchorless mode makes nodes at the edge of a relay's reach switch to
  and fro (roadmap D51).

Between its windows a node hears no relay's heartbeat, so it switches by
the rule above mainly while it checks. A node that moves checks when it
has missed its anchor twice: it takes no fix of its own while it has one
([beats](#beats)). A node back from the dark keeps
its anchor: a new one would strand the messages sent to the old address
(roadmap D46, D47).

**An anchor that turns carried or sleepy.** A node that hears its
anchor's heartbeat with the carried or the sleepy flag (roadmap D71)
leaves it: at once for the strongest other relay that is live in its
table and not carried, else for the strongest a rotation's listen finds,
as when it checks the anchor; with none, it is anchorless. The carried
anchor's heartbeats do not end that listen. A check that hears neither
the anchor nor another relay leaves an anchor last heard sleepy or
carried, where it would keep one merely unheard.

The node's address changes as at any switch; what was sent to the old
one reaches it by its anchor's flush ([delivery](#delivery)), or is lost
unless its sender learns the new address.

### Beats

A sleepy node with an anchor heartbeats on its home channel, the
anchor's, where the anchor and any other node on that channel hear it:
a **beat**. The heartbeat has the sleepy bit
([packet formats](#packet-formats)): reach the sender cheaply in its
window after its beat, which its holder does, or at any time with the
long preamble, which a relay holding a message for it does when it
spares the long frame ([next hop](#next-hop)).

Each beat comes a random half to one **beat period** after the last,
P = 300 s. At the mean interval of 225 s a node beats 16 times an hour,
230 ms of airtime.

*Why that period.* It is half the delivery TTL, so that two beats are
never more than a TTL apart. A holder that keeps a node for a TTL then
loses it only after two lost beats, and keeps a node that has gone for
a TTL, as long as its messages are kept at least
([heard table](#heard-table)).

*Why random.* The spread keeps two nodes whose beats met once from
meeting again. The interval is random, not fixed, since nobody wakes for
a beat: a holder hears it as it listens.

After it chooses or changes its anchor a node beats at once and once
more within a heartbeat interval, 5–15 s, so that one lost beat does not
hide it from its new holder for a period. After it follows its anchor to
a new home channel ([channel moves](#channel-moves)) it beats there once,
at a random time within a heartbeat interval, 0–15 s.

*Why a beat after a move.* Without it, its holder knows only the old
channel, and has no window to send in until the node's next beat, up to
a period later; every node of the anchor follows on the same heartbeat,
so beating at once they would meet (roadmap D57).

**To anchorless neighbours.** A sleepy node whose neighbour table holds
anchorless neighbours ([sleepy nodes alone](#sleepy-nodes-alone)) sends
each beat also on each channel they listen on, with the long preamble.
Each such beat is a long heartbeat, 249 ms
([duty-cycled receive and the long preamble](#duty-cycled-receive-and-the-long-preamble)),
4.0 s an hour per channel at sixteen beats (roadmap D58).

It does so for a neighbour only once it has heard that neighbour's
anchorless beat in two consecutive rotations of its beats: at least half
a rotation apart, 13 beats each at least half an anchorless beat period
after the last, 300 s, and at most a rotation and a heartbeat's lag,
601 s. It stops when the entry expires
([neighbour table](#neighbour-table)) or the neighbour is heard with the
sleepy bit alone.

*Why long beats.* Anchorless nodes receive duty-cycled and would not
hear a short beat, and the long one, with the sleepy bit alone, tells
them of a last resort they can reach with the long preamble, not of an
anchor. The sleepy node hears them because their beats carry the long
preamble and step through every channel.

*Why two rotations.* A neighbour heard anchorless once may be switching
on, and will anchor within minutes. One still anchorless a rotation
later is in a pocket with no relay, whose nodes mostly know their way
out already, from the time both were anchorless, so waiting a rotation
costs them little.

**GPS.** A sleepy node with an anchor works by the anchor's position: it
is its address, the position its beats carry, the centre of its flat
plane and the point it rebuilds the short positions it hears near
([positions on the wire](#positions-on-the-wire)). It takes no fix, runs
no search and measures nothing of its motion: it keeps the carried flag
it had when it took the anchor until it is anchorless or a relay again
([position](#position); roadmap D91). Its own place, the last it had by
its own fixes, is what it stands at when plugged in
([sleepy nodes](#sleepy-nodes)).

Without an anchor a sleepy node runs its GPS (roadmap D80): it searches
when it turns anchorless and once a delivery TTL while it stays so,
since its address is then its own position
([sleepy nodes alone](#sleepy-nodes-alone)). A search ends at the first
fix that passes the checks ([position](#position)), after 60 s, or when
the node takes an anchor; one that finds nothing holds every search
back for 9 hours. A node with no place searches until it has one, anchor
or not, since without a fix of its own it rebuilds no position it hears
([position](#position)). A relay's GPS is in [position](#position).

| | |
|---|---|
| with an anchor | no search: standby, 20 µA (L76K hardware design, Table 2) |
| search limit | 60 s: two of the 30 s frames that carry the satellites' orbits (IS-GPS-200), so that a cold start under the open sky, 30 s (Table 2), survives one lost frame |
| after a search that found nothing | none for 9 h: 60 s at the L76K's 29 mA is 0.48 mAh, and once in 9 h, 0.054 mA, keeps a node that never sees the sky within a tenth of its 0.56 mA ([delivery](#delivery)); 8.6 h is the least that does, rounded up to the hour |
| between searches | standby, 20 µA, which keeps what a hot start, 2 s, needs (Table 2) |

*Why none with an anchor.* Hearing its anchor each rotation says the
node is within reach of a fixed relay, which is all its address needs:
the address is the anchor's position. Its own fixes bought it two
things. The carried flag, which nothing reads while it sleeps: a sleepy
node holds mail for nobody and is nobody's anchor whatever the flag
([nodes](#nodes)), and what the flag decides when the node is plugged
in, the relay's own GPS settles then, a fixed node by its first fix
within a search's limit and a carried one by four hours still
([position](#position)). And a check of the anchor after a move of
0.67 of the way to it, which the anchor missed twice makes a rotation or
two later ([relay choice](#relay-choice)) (roadmap D80, D91). Indoors the L76K, which acquires only 18 dB
below the open sky, mostly finds nothing, and a search before each beat
would hold it on at 29 mA, 1.2 days on the battery (roadmap D80).

*Why the anchor's position serves to rebuild.* A short position is
rebuilt as the one within half a period of the node's own, 19.5 km
north-south ([positions on the wire](#positions-on-the-wire)), and every
node a sleepy node hears is within the hearing radius, 3 km, of it, so
within 6 km of its anchor. Its own fix served the same within reach of
the relays it knew, and no better beyond: a node carried farther than
half a period while it heard no relay rebuilds its next anchor's
position a period off, and the fix a search found then would lie
farther than the hearing radius from every relay it knows and be dropped
([position](#position)). That trip is open item O19.

A sleepy node with an anchor does not sweep the channels
([busy estimate](#busy-estimate)) or move its home channel by the busy
estimate: receiving duty-cycled, it could not.

### Heard table

A node that is not sleepy keeps the sleepy nodes with an anchor it hears
beat in its **heard table**:

| | |
|---|---|
| size | 256 entries, by ID |
| each entry | the node's home channel; until when it listens ([delivery](#delivery)); whether a delivery to it went unanswered since its last beat; the mean RSSI of its last two beats |
| kept | two beat periods, the delivery TTL, after the last frame heard from the node: a beat, its own DATA, whose `aux` also names its home channel ([packet sender](#packet-sender)), or its ACK of a delivery |
| when full | a newcomer takes the place of an expired entry, else of the one heard longest ago |

*Why any frame renews it.* A node that answers is in reach whether or
not its beats are heard, and a busy holder misses beats, while it
transmits or when another frame meets the beat there. A node reached
only in the windows its beats open renews its entry with each beat; one
kept awake by a flow longer than a TTL is renewed by its ACKs, since a
holder at its cap misses beats (roadmap D53).

*Why a full table does no harm.* A node that beats now listens now, so a
holder for more nodes than its table keeps still sends each its mail
after each beat, and a full table does not leave the nodes it has no
place for without their mail until it expires (roadmap D61).

*Why that size.* 256 is three times the ≈80 sleepy nodes per relay
expected at indoor density in Kyiv with 10 % adoption
([capacity and sizing](#capacity-and-sizing)); at 50 %, ≈400, a full
table does no harm (above).

### Delivery queue

Kept by nodes that are not sleepy, up to 41 messages per node; a node
that turns sleepy keeps what it held only until it hands it on
([sleepy nodes](#sleepy-nodes)). A message
queued the delivery TTL ago or longer is **stale**: it stays, and goes at
its node's next beat as any other, until a message arrives at a full
queue; then the oldest stale message makes way for it. When the queue is
full and holds no stale message, a message for another node is refused
([receiving DATA](#receiving-data)): what is queued stays, and the
sender learns at once. Every node, sleepy or not, keeps the messages
for its own application by the same rule, in its inbox
([phone link](#phone-link)).

A message's age counts from its arrival in the queue: the delivery TTL
is the wait for a node that sleeps or is dark, not for the way there.

*Why age alone drops nothing.* A message dropped when no other needs its
place is lost for nothing, and its node may yet beat, back from a pocket
or a basement, or heard at last by a holder that missed its beats.

A queued message goes to the send queue one per node at a time, oldest
first: when it arrives, when the node beats, when the node takes the one
before, and whenever a hand-over ends and leaves room in the send queue;
there it goes first ([delivery](#delivery)). In the node's window it
needs a place in the regular room ([send queue](#send-queue)) and, in
the bucket, its own frame and the reply reserve, not the airtime of
everything queued, which it goes ahead of.

*Why not behind the forwards.* Counted behind them, it would be kept out
by a holder at its cap while the node listens, and wait a beat period or
expire (roadmap D55).

One the bucket cannot fit yet goes the moment the bucket will. One whose
node has stopped listening by the time its hand-over would start waits
for the next beat. A failed delivery counts no failure against anyone:
the node is not a neighbour the holder routes through.

*Why that size.* Nodes in one train or one basement go dark together, so
the queue holds what arrives in a TTL for every node a holder can keep,
its heard table's 256 ([heard table](#heard-table)), at the 23 texts a
day each that the network carries
([capacity and sizing](#capacity-and-sizing)): 256 × 23 / 144 ten-minute
TTLs in a day = 40.9, so 41 messages.

A flow to one node that pauses longer than a hand-over waits for the
next beat, at most 41 messages a period, and the rest are refused.

The owner remembers 41 delivered message IDs
([packet formats](#packet-formats)) and ACKs a repeat without delivering
it twice. When it needs the place, it forgets the one delivered or last
repeated longest ago; time alone forgets nothing. 41 is the delivery
queue's size, for the same reason.

Nothing covers a node that has moved out of reach of every holder that
has its mail: the old holder does not forward to the new anchor, the
mail waits there until its place is needed, and the sender is not told
([addresses](#addresses)).

### Delivery

A message that reaches the owner of its position ([owner](#owner)) and is
for another node waits in the owner's delivery queue
([delivery queue](#delivery-queue)); while the owner hears that node
beat, it is the node's holder. Admission is unchanged: the last hop is
not fresh work ([receiving DATA](#receiving-data)).

It goes to the node as DATA addressed to the node's ID, in `dst_hop` and
`dst_id` alike, on the node's home channel from the heard table, with
the ordinary preamble in the node's window, and the node answers with an ordinary ACK
([the ACK](#the-ack)). A delivery goes through the send queue as any
hand-over ([hand-over](#hand-over)), but ahead of the rest of the queue.
A hand-over waiting between its attempts, for the channel, a retry or
the bucket, steps aside for it, keeps its place, and is routed again
when its turn comes, with fresh attempts.

**The window.** A delivery goes while the node listens: for a window of
3.39 s from each beat the holder hears. A node whose beat brought
nothing goes back to receiving duty-cycled after that. One kind of
delivery goes outside the window instead (below).

*Why that window.* It is long enough for one hand-over of a DATA that
goes first to complete. That is the attempt the holder may have under
way, a full frame and the ACK timeout, 238.6 ms, then the DATA's
hand-over with all its attempts and retry waits, 3.15 s
([retry wait](#retry-wait)). A hand-over cut short by the window would
leave its message to wait a beat period (roadmap D50).

Having answered a DATA, the node listens one hand-over more, 3.15 s, and
its holder knows it does: nothing is under way ahead of the next message
for it, which goes then. The node goes back to receiving duty-cycled
once a hand-over passes with no DATA for it.

So the holder sends what it holds one after the other on one beat, a
message that arrives meanwhile goes at once, and a flow to the node less
than a hand-over apart keeps it awake while it lasts. A repeat of a DATA
whose ACK was lost finds the node still listening.

Only the holder's bucket caps a burst ([duty cycle cap](#duty-cycle-cap)):
once the bucket would keep the next message past the hand-over, the node
leaves its window and the rest wait for its next beat. A delivery that
goes unanswered marks the node out of its window, and its messages wait
for its next beat.

The DATA carries the hop count as any hand-over ([hop count](#hop-count)).

**Outside the window, with the long preamble.** A message for a node
the holder hears, outside the node's window, goes at once as DATA with
the long preamble
([duty-cycled receive and the long preamble](#duty-cycled-receive-and-the-long-preamble)),
the node's oldest first and one at a time, if the holder's bucket holds
that frame beyond everything else it has to send: its send queue, every
other message in its delivery queue at the ordinary preamble, and the
reply reserve ([duty cycle cap](#duty-cycle-cap)). Once the node has
taken one, it listens a hand-over more, as after any delivery, and
what follows goes with the ordinary preamble. A delivery outside the
window that goes unanswered marks the node out of reach, and its
messages wait for its next beat.

*Why outside the window.* A person waiting for a text feels the wait
for the node's beat, minutes. A long frame costs the holder airtime,
which the bucket condition keeps to what it has spare, and the node the
hand-over it listens after taking it. One unanswered frame says the node is
dark or gone, so no more go to it before it beats (roadmap D112).

So a message for a node in reach usually goes at once. One the bucket
cannot spare, or for a node whose last delivery went unanswered, waits
for the node's next beat: a mean of about two minutes, at most a beat
period. A node listens 3.39 s after each beat, 54 s an hour, 3.15 s
after each message it takes, and for its anchor's heartbeat, under a
second a rotation, and receives duty-cycled between
([duty-cycled receive and the long preamble](#duty-cycled-receive-and-the-long-preamble)).
What it listens besides, checking its anchor after the dark, depends on
how often it is out of reach (roadmap D47, D48, D50, D58).

A sleepy node with an anchor draws 0.56 mA at the design rate in the
simulator, three quarters of it listening, 63 days on the 850 mAh
battery; without an anchor at most 0.61 mA, 58 days. The
microcontroller is not counted (roadmap D92).

**Outside the windows.** Two more kinds of delivery go outside the
windows, after the deliveries above and one at a time.

- **The flush.** From its first far fix ([position](#position)), or
  once it is to turn sleepy ([sleepy nodes](#sleepy-nodes), roadmap
  D71), a node sends each message in its delivery queue once, as DATA
  with the long preamble on the channel its node listens on, from the
  heard table, else the neighbour table, else its own home channel.
  It is one attempt, through the bucket as any send. A message whose ACK
  comes is done; the rest stay and go at beats as before until their
  place is needed.
- **A node heard as a relay.** A holder that hears a heartbeat without
  the sleepy flag from a node it holds messages for, carried or not,
  sends them at once, one after the other, with the ordinary preamble on
  the home channel the heartbeat names. Each is done on its ACK;
  unanswered ones wait for the node's next heartbeat or beat heard.

*Why the flush.* The node is leaving: once it has gone its sleepy nodes'
beats no longer reach it, and the long preamble reaches them now,
whatever their windows. Each message is flushed once, so a still node
whose fixes stray past the carried radius again pays no more. The cost is
bounded: a full queue, 41 messages of 372.8 ms, is 15.3 s of airtime,
two and a half buckets, paced by the bucket.

*Why a node heard as a relay.* Such a node listens all the time and
opens no window, so its messages would otherwise wait until their place
is needed (roadmap D66). It is a node that slept and now relays, or a
relay whose messages a neighbour within 50 m owned because it did not
keep it in its table ([neighbour table](#neighbour-table)).

### A sleepy node's own message

A sleepy node with an anchor hands its own message to the anchor while
the anchor is live, with count 0 ([hop count](#hop-count)), and
otherwise to its next hop as any node does ([next hop](#next-hop)); one
without an anchor sends as a relay does
([sleepy nodes alone](#sleepy-nodes-alone)). The anchor takes it only as
fresh work ([receiving DATA](#receiving-data); roadmap D31, D34), and
the node takes a message from its application only while the anchor is
not banned.

*Why the anchor.* Handed to the neighbour nearest its destination
instead, the first hop would often be a faint, far relay (roadmap D46).

A refused message goes back to the application with the wait the
refusal asks for ([bans](#bans)), and the node takes no message of the
application's until that wait is over; the application may offer it
again then, or give it up, and tell the person either way
([addresses](#addresses)).

One whose hand-over goes unanswered is tried again a fade later, 60 s,
doubled for each unanswered hand-over to the anchor in a row up to the
delivery TTL, 600 s, or when the anchor's ban ends if that is later. It
is tried until the anchor, or the anchor the node chooses next
([relay choice](#relay-choice)), takes it.

*Why a fade.* A fade lasts about a minute (the scenarios' correlation
time), and the three attempts of one hand-over, a few hundred
milliseconds apart, all fall inside it, where the doubling bans would
offer it every few seconds to a node whose channel is full.

*Why it doubles.* Silence says no more than that the channel or the
anchor is full, so the wait doubles as the ban does: tried again every
fade instead, a text its anchor never takes would cost its sender three
frames a minute on the anchor's channel, where the anchor hears its
other nodes' beats (roadmap D54). The wait stops doubling at the
delivery TTL, as long as the anchor keeps a node that has gone dark in
its table ([heard table](#heard-table)), so a text whose anchor is back
is not held longer than that.

A message that finds no live neighbour at all waits for one, routed
again every 60 s: a node in a pocket loses its neighbours by going dark
and finds them again. The application is not
told ([addresses](#addresses)).

### Sleepy nodes alone

A sleepy node that hears no fixed relay in its first rotation
([channel at boot](#channel-at-boot)) has no anchor: it is in
**anchorless mode**. It knows no node that holds mail, so no address
can point at a node that holds its mail and hears its beat. Instead it
does a relay's work where there is none:

- it forwards at once what it is given;
- its address is its own position, and it owns it;
- a DATA for it does not wait for a beat.

It receives duty-cycled, as every sleepy node does between its windows,
and has no window: every frame to it carries the long preamble. It
keeps the home channel it has, its last anchor's or the one it chose at
boot; it does not sweep or move.

It beats with the sleepy and anchorless bits and the long preamble, each
beat a random half to one **anchorless beat period** after the last, on
the next channel of the rotation each time ([heartbeat](#heartbeat)),
naming its home channel in its first byte. The period is the delivery TTL over the
13 channels, 600 / 13 = 46.2 s. At the mean interval of 34.6 s the beats
cost 26 s an hour of airtime, 7.2 % of its budget, and 0.23 mA at 32 mA
of transmit current.

*Why that period.* Beats at most a period apart visit every channel
within a TTL, so a relay in reach on any channel hears the node, and two
nodes in anchorless mode find each other, within the least time the
network keeps what it holds for them ([sleepy nodes](#sleepy-nodes)).

Nodes in anchorless mode route among themselves and are a next hop like
any other ([next hop](#next-hop)), each DATA to one with the long
preamble. A relay that hears an anchorless beat answers it
([heartbeat](#heartbeat)) with the long preamble, at once and again a
fade later, 60 s
([a sleepy node's own message](#a-sleepy-nodes-own-message)), for two of
the node's beat periods, so that one lost answer does not cost a
rotation. A carried relay does not answer, since nobody anchors to it.

A receiver cannot tell an answer from a heartbeat in its slot by the
frame; a sleeping node tells them apart by the time
([relay choice](#relay-choice), roadmap D62).

At the first relay it hears the node anchors as any sleepy node does and
leaves anchorless mode. Its neighbours then keep it as a last resort,
and the anchorless ones behind it learn where it listens from its long
beats on their channels ([beats](#beats)) and from the frames they
exchange with it.

---

## Phone link

A person's app talks to their node over Bluetooth LE, through one GATT
service on the nRF52840 (TrouBLE, roadmap D5). The app owns identity
and end-to-end encryption; the node sees opaque payloads. Section
numbers below are of the Bluetooth Core Specification 5.0
([sources](#sources)).

**Finding the node.** The node advertises, connectable, every
1022.5 ms: its flags and the service's UUID, which the app scans for,
in the advertisement; its name, `bark-` and the twelve hex digits of
its ID as its display shows them, in the scan response. A node is
public unless its owner makes it private, and a public node advertises
otherwise and takes more links (Public, below); a private node takes
one connection at a time and does not advertise while it has one
(roadmap D120).

*Why the name in the scan response.* A legacy advertisement holds 31
bytes (Vol 6 Part B §2.3.1): the flags take 3 and a 128-bit UUID 18,
which leaves 10, too few for the name's 17.

*Why 1022.5 ms.* A person who opens the app waits about a second for
the node to show, and an iPhone finds a node quickly only at one of the
intervals Apple's accessory guidelines list, of which this is the one
nearest a second ([sources](#sources)). Faster costs a sleepy node more: an
advertising event on the three channels is about 2 ms of the radio at
4.8 mA, about 11 µA at this interval, near a tenth of its duty-cycled
receive (an estimate from the nRF52840 Product Specification's radio
currents, for the bench to measure).

*Why one connection.* One person's phone owns a private node: two
phones would share one inbox and one message slot. A public node shares
them by rules of their own (Public, below).

**Pairing.** A node is one phone's: the phone paired with it (roadmap
D135). Every characteristic of the service needs a link encrypted with
an authenticated key, but on a public node those a guest needs (Public,
below); a read, write or subscription on any other link
is answered with the ATT error 0x05, insufficient authentication (Vol 3
Part F §3.4.1.1), which is the phone's cue to pair. The node's name and
appearance stay open, as the advertisement says them anyway.

The pairing is LE Secure Connections with passkey entry (Vol 3 Part H
§2.3.5.6.3). The node can display and cannot take input, so the method
is that the node shows a six-digit passkey and the person types it into
the phone (Vol 3 Part H §2.3.5.1). The node draws the passkey on its
screen as soon as the pairing asks for it, on the line `PAIR CODE`; the
same line says `PHONE UNPAIRED` before any phone has paired, when the
node is public all the same, then `PHONE PUBLIC`, or `PHONE PRIVATE` on
a node its owner has made private. Only an open build, for the bench,
also writes the passkey to its USB log, as a protected node's log must
not let a cable stand in for being at the node (roadmap D138). The node keeps
one bond in flash across reboots: the phone's identity address, the
identity resolving key by which it knows the phone's changing address
(Vol 6 Part B §1.3.2.2), and the long-term key. A new pairing replaces
the bond. A pairing that is not authenticated, as with a phone that has
no keyboard (Just Works), or that does not bond, changes nothing, and
the node closes its link.

*Why the bond is the ownership.* A node has one inbox and one mailbox
key, so it serves one person (why one connection, above). The passkey
shows only on the node's screen, so a phone can pair only while its
person is at the node: being there is what a person can show without
accounts or a server, and it is also what lets the rightful owner take
the node back from anyone. Every characteristic needs the bond, Home
among them, so only the owner's phone sets or clears the mailbox key
([mailbox](#mailbox)); there is no second check.

**The replaced phone.** A phone whose bond the node replaced learns it
at its next connection. The phone encrypts the link with its old key as
soon as it connects; the node holds no such key and ends the link,
authentication failure (error code 0x05, Vol 1 Part F), before the app
has read anything. So a link that a phone holding a bond for the node
loses before its first read tells the app that the node is no longer
the person's; the app says so and offers to pair again, which takes the
code on the node's screen.

*Why end the link.* The node could instead refuse the key and keep the
link unencrypted (LE Long Term Key Request Negative Reply, Vol 4 Part E
§7.8.26), and the phone's first read would fail with insufficient
authentication. But a phone refused its key may start pairing by itself
and ask its person for a code before the app can say why. The BLE host
ends the link, and the node keeps that. A new pairing leaves the mailbox key as
it was: the node goes on answering the former owner's fetches until the
new owner writes their own key or clears it.

**MTU.** The node offers an ATT MTU of 247, the longest link-layer
payload with the data length extension, 251 bytes (Vol 6 Part B §2.4),
less the L2CAP header's 4 (Vol 3 Part A §3.1). The app asks for 247 by
an MTU exchange (Vol 3 Part F §3.4.2) as soon as it connects; every
value but an update's begin (Updates, below) then fits one ATT PDU and
one link-layer packet. The node also
takes long writes and serves long reads (Vol 3 Part F §3.4.6, §3.4.4.5),
so a phone that settles on less still works, in more round trips. A
notified value fits the 20 bytes a notification carries at the default
MTU of 23 (Vol 3 Part F §3.4.7.1) (roadmap D119), but Ask's, 22: at that
MTU its notification carries all but what the neighbour gathers, which
the app then shows as nothing gathered (roadmap D158).

*Why both.* An Android app must ask for the MTU, and iOS chooses it
itself, which the app cannot raise; a notification has no long form.

**The service.** One random UUID, its second 16 bits counting the
service and its characteristics:

| | UUID | properties | value | bytes |
|---|---|---|---|---|
| service | `28bc0001-a1b8-4893-9537-2ee3559d7644` | | | |
| Node | `28bc0002-…` | read | version(1) id(6) image(4) | 11 |
| Status | `28bc0003-…` | read, notify | flags(1) relays(1) boot_left(2) address(7) battery(2) inbox(1) | 14 |
| Send | `28bc0004-…` | write | dst_pos(7) dst_id(6) payload(0–227) | 13–240 |
| Inbox | `28bc0005-…` | read | msg_id(4) payload(0–227), empty when none; a guest's: number(3) msg_id(4) payload(0–227) | 0, 4–231, 7–234 |
| | | write | msg_id(4) | 4 |
| Given back | `28bc0006-…` | read, notify | msg_id(4) wait(2), empty when none | 0, 6 |
| Map point | `28bc0007-…` | write | 0 to clear; 1 then position(7) to set | 1, 8 |
| Clock | `28bc0008-…` | read | utc(4) offset(2) source(1) | 7 |
| | | write | utc(4) offset(2) | 6 |
| Home | `28bc0009-…` | read | set(1) oldest(3) newest(3) | 7 |
| | | write | 0 to clear; 1 then key(32) to set | 1, 33 |
| Neighbours | `28bc000a-…` | read | floor(1) next(1) entry_len(1) own(7), then per neighbour id(6) position(7) rssi(2) age(2) flags(1) | 10–244 |
| | | write | slot(1) | 1 |
| Update | `28bc000b-…` | read | state(1) refused(1) held(4) version(4) hash(32) | 42 |
| | | write | 0 then header(256) to begin; 1 to commit; 2 to reboot | 257, 1 |
| Image | `28bc000c-…` | write, write without response | offset(4) bytes(1–240) | 5–244 |
| Recipient | `28bc000d-…` | write | tag(1) | 1 |
| Public | `28bc000e-…` | read, write | public(1) | 1 |
| Guest | `28bc000f-…` | write | number(3) | 3 |
| Ask | `28bc0010-…` | write | id(6) | 6 |
| | | read, notify | id(6) status(16), empty when none | 0, 22 |

Positions and IDs are in the radio's encodings
([positions on the wire](#positions-on-the-wire),
[node identity](#node-identity)), a message ID is the 32-bit hash of
[packet formats](#packet-formats), and every integer is big-endian.
The codecs are `bark-link`, shared by the firmware and the tests. A
write is a write request, which the node answers, but for Image's,
which may be a write without response (Updates, below); a write that is not
whole is answered with the ATT error 0x0D, invalid length, and one with
a field the node does not take with 0x13, value not allowed (Vol 3 Part F §3.4.1.1).

**Node.** The link version, 10 in this specification; the node's ID, the
second half of its address ([addresses](#addresses)); and the version
of its firmware image.

**Status.** How the node stands now, notified at every change but the
countdown of `boot_left`:

| | |
|---|---|
| flags bit 0 | ready: the node takes a message from this phone now |
| bit 1 | on external power, USB |
| bits 2, 3, 4 | sleepy, carried, map point, as its heartbeat says them ([packet formats](#packet-formats)) |
| bit 5 | placed: it has a place, and `address` holds a position; else `address` is zero ([position](#position)) |
| bit 6 | its first rotation runs ([channel at boot](#channel-at-boot)) |
| bit 7 | anchorless ([sleepy nodes alone](#sleepy-nodes-alone)) |
| relays | the relays it knows |
| boot_left | seconds left of its first rotation, rounded up; 0 once it is over |
| address | the position of its address: its own, or a sleepy node's anchor's ([addresses](#addresses)) |
| battery | millivolts as last read, uncalibrated; 0 on external power and before the first reading |
| inbox | messages in the inbox |

So the app can show "listening, 3 relays heard, 1:40 to go", and the
address the person gives others to reach them. Sleepy and carried are
measured, not set ([nodes](#nodes)).

**Sending.** The app writes Send while ready is set. The node answers
once it has taken the message, or with the ATT error 0x80, not ready,
when it takes none now; the app then waits for ready. A message taken
is the node's: the app hears of it again only if the node gives it
back ([addresses](#addresses)). The node holds one message of its app's
at a time ([receiving DATA](#receiving-data)), so the phone is the
queue: it keeps the rest and offers the next when ready is set again.
On a public node its phones take turns at that one message (Public,
below).

**Given back.** When the node gives the app's message back, it sets
Given back to the message's ID and the seconds left of the wait the
refusal asked for, rounded up, and notifies it. The value stays until
the node takes another message, the wait counting down to 0. The app
finds the message by its ID among those it sent, computing each one's
as the node does, shows the person the wait, and offers it again once
ready is set, or gives it up, as the person chooses.

*Why the value stays.* The phone may not be connected when the message
is given back, and the node takes no other message before the wait is
over, so the app learns of it whenever it next connects.

**Receiving.** The node keeps each message for its app in an inbox of
23, oldest first, by the delivery queue's rule: a message stale there
makes way for a new one, and while the inbox is full of fresh messages
the node refuses the next ([delivery queue](#delivery-queue),
[receiving DATA](#receiving-data); roadmap D118, D122). Inbox reads the
oldest; the app keeps it and writes its ID back, and the node then
forgets it. A write of another ID is answered 0x13. Status's count says
when to read.

*Why read and confirmed, not notified.* The phone is often not
connected: the app is closed, the phone is out of range or off. A
notification is confirmed by nobody, an indication only by the phone's
Bluetooth stack, not by the app that must keep the message; the ID
written back is the app's own word that it has it.

*Why 23.* The texts a person receives in a day at the design rate
([capacity and sizing](#capacity-and-sizing)), for a phone that
connects once a day. The inbox, like the message the node holds, is
not kept across a reboot.

*Why the delivery queue's rule.* A message the node has taken is not
lost for want of room: refused, it waits at its sender, as one for a
sleepy node does. A phone away longer than the delivery TTL has its
oldest message give way rather than hold up every sender, as a dark
node's mail does.

**Map point.** A write of 1 and a position sets the node's map point,
and of 0 clears it ([position](#position)).

**Clock.** The app writes the phone's time, UTC seconds since 1970, and
its UTC offset in minutes, −720 to +840, at every connection; another
offset is answered 0x13. The node keeps the offset across a reboot and
takes the time only while it has none from its GPS
([position](#position)). A read gives the node's time, the offset it
keeps, and where the time came from: 0 none, 1 GPS, 2 the phone; with
none the time is 0. From link version 2 (roadmap D130).

**Home.** A write of 1 and the mailbox key makes the node its person's
home, and of 0 makes it nobody's ([mailbox](#mailbox)); the node keeps
it across a reboot. A read says whether it is a home and the numbers of
the oldest and the newest item its ring holds, 0 when none. While it is
a home, the inbox is its ring: Inbox reads the oldest item after the
last the phone confirmed, with the ID it arrived with, the written ID
confirms it, and Status's count is the items not yet confirmed; the
ring keeps them either way ([mailbox](#mailbox)). From link version 3.

*Why the ID it arrived with, kept.* An ID is a hash of the node's
address ([packet formats](#packet-formats)), which changes whenever the
node's position does, at every fix while it is carried
([position](#position)); worked out again at each read, an item's ID
would change between the phone's read and its confirmation.

*Why the offset from the phone.* Turning a position into a time zone
takes the zones' boundaries and rules, which the node has no room for
and which change by law; the phone has them, set by its person.

**Neighbours.** The node's neighbour table ([neighbour
table](#neighbour-table)), for the app to draw the node and its links
on a map. The app writes a slot of the table, 0 first, and a read gives
the live neighbours in that slot and after it, as many as fit, 13, and
the slot to write next, 0 once the page reached the table's end; 64
neighbours take five reads. The slot read from is 0 at each connection.
A read starts with the node's noise floor in dBm, signed
([busy estimate](#busy-estimate)), the slot to write next, the bytes of
each neighbour, 18 in this version, by which the app steps, and the
node's own position, zero while it has none, which for a sleepy node is
not its address ([position](#position)). Each neighbour then gives its ID, its
position, the averaged RSSI of its link (`rssi_ema`) in tenths of a
dBm, signed, the whole seconds since it was last heard, at most 65 535,
and its flags:

| | |
|---|---|
| bit 0 | sleepy |
| bit 1 | map point |
| bit 2 | demoted |
| bit 3 | banned ([bans](#bans)) |
| bit 4 | this sleepy node's anchor, the relay it sends and receives through ([relay choice](#relay-choice)) |
| bit 5 | carried |
| bit 6 | its last heartbeat had the anchorless flag |
| bit 7 | it has taken a message from this node, so it hears this node too |

A link's **quality** is the margin of its averaged RSSI over the noise
floor, reckoned as the node's margin is ([neighbour
table](#neighbour-table)): fragile below that margin, 4.1 dB, where a
full frame is lost more than one time in a hundred in a fade of one
standard deviation, 4 dB, and the node sends through the neighbour only
when no other is left; good from 6.9 dB, where a full frame gets
through a fade of two, 8 dB, which comes one time in 44, the convention
of the switching margin ([relay choice](#relay-choice)); fair between.
The floor and the RSSI in tenths are there so that the app reads the
margin as the node does. From link version 4.

*Why pages from a slot.* The table holds 64 neighbours of 18 bytes, more
than one value holds, and every value fits one PDU (MTU, above). A
neighbour keeps its slot while it stays in the table, so when one
leaves between two reads the others stay where they were, and the next
page skips none of them; counted from the first live one, they would
shift down by one, and one would be skipped.

**Ask.** The app writes a neighbour's ID, and the node asks that
neighbour for its status ([neighbour status](#neighbour-status)); the
write is answered once the node has taken it, whether or not the ID is
a live neighbour's, as nobody answers an ask of anything else. The app
asks each neighbour it shows, once, when the person opens or refreshes
the map, never by itself. Each answer the node is given, the
neighbour's ID and its status as the radio carries it, is Ask's value
from then on, notified to the paired phone; the value is empty before
the first since the node booted. An answer comes within about a second
of the ask, or not at all. The app shows it on the neighbour in the
words of the neighbour's own screen. From link version 9; the image it
gathers, the status's last 7 bytes, from 10.

*Why a characteristic of its own.* The answer is notified, and a
notified value is the characteristic's own: Neighbours' is a page of
the table. A write of an ID to Neighbours would also change what the
app writes there, which only a new version may.

**Updates.** The app stages a new firmware image on the node, which
the node's loader swaps in at its next boot (roadmap D79;
`bark-fw/README.md`, "Updates"): so a node on a roof is updated from
the street. The image is a header of 256 bytes, which holds its
version, its length and its SHA-256, and then its bytes. The app writes
0 and the header to Update to begin: the node forgets any image staged
before, and takes the header if its version is above the node's own,
a signature in it verifies under a key the node holds (roadmap D144), and
no swap is asked for already. Then the app writes the image to Image in
pieces, each its offset in the image and as many bytes as one write
carries, 240 at the MTU of 247, without a response; the node writes each
into its external flash, and takes a piece only at the offset where the
bytes it holds end. Update's read gives the state, 0 idle, 1 receiving,
2 staged, every byte held, 3 committed; why the last begin, commit or
reboot was refused, 0 if it was not; the bytes held; and the header's
version and hash, zero while idle. The app reads it after every seven
pieces and goes on from the bytes held, so that a piece or a connection
lost is sent again from there; a hash not its image's is another image
being staged, which it begins again. With every byte held, the app
writes 1 to commit: the node checks the image against the header's hash,
and every other check its console's commit makes, and asks the loader
for the swap. Then it writes 2 to reboot: the node answers, ends the
link and resets. A begin, a commit or a reboot the node refuses is
answered with the ATT error 0x81, refused, and Update says why:

| refused | |
|---|---|
| 1, header | not a header the node reads |
| 2, version | not above the version the node runs |
| 3, busy | a swap is asked for already: reboot first |
| 4, incomplete | not every byte is held |
| 5, hash | the bytes do not hash to the header's hash, and are forgotten |
| 6, battery | on battery at or below the low level ([implementation notes](#implementation-notes)); the image stays staged, to commit on external power |
| 7, flash | the flash failed, and the image is forgotten |
| 8, signature | no signature in the header verifies under a key the node holds |

What is staged lasts across connections until the node resets. From
link version 6 (roadmap D145).

*Why without a response, seven at a time.* A write request is answered
in a later connection event than the one that carried it, so a phone
that waits for each answer sends one piece every two events; without
a response it sends as many as an event carries. The node's host
queues 8 ATT PDUs for a link and drops one more, so seven pieces and
the read fit; the read is answered only once the node has written the
pieces before it, so it is also the flow control.

*How long it takes.* An app asks Android for a short connection
interval for the transfer, 11.25 to 15 ms ([sources](#sources)). A
packet of 251 bytes takes 2.12 ms on the 1 Mbit/s PHY, 2.5 ms with the
node's empty answer and the two 150 µs gaps between (Vol 6 Part B
§2.1, §4.1.1), so the node's connection event of 7.5 ms and its three
receive buffers take three pieces an event. Seven pieces take three
events at 15 ms; the node writes them meanwhile, each 240 bytes over its
external flash's bus at about 1 Mbit/s, 2 ms, two page programs on
average, 2.6 ms, and a sector erase every 17 pieces, 0.6 ms each: 36 ms.
The read's answer goes in the next event and the next pieces in the one
after: about five events, 75 ms, for 1680 bytes, 22 KB/s. An image of
400 KB takes about 18 s, and the commit's hash, a read of it over the
same bus, about 3 s more. At Android's default of 30 to 50 ms the node's
writing hides inside the three events, and the same five events take
twice as long or more, 40 to 60 s; with a write request for each piece,
two events a piece, it would be 50 s at 15 ms.

**Versions.** A later link version may add characteristics and append
fields to the values the node writes, Node, Status, Given back, Update,
Ask, and each neighbour of Neighbours; an app
reads the fields it knows and ignores the rest, as a node reads a
heartbeat. The values the app writes change only with the version, and
the app writes the form of the node's. A change to what a field means
takes a new service UUID, so that an app that does not know it finds no
node and asks the person to update it. An app that finds a version
above its own tells the person the node's firmware is newer (roadmap
D117).

**Changes to the table.** Besides the service, the node's attribute
table holds the GAP service, with the node's name and appearance, and
the GATT service, with Service Changed, indicate only (Vol 3 Part G
§7.1), and the database hash, readable on any link (Bluetooth Core
Specification 5.1, Vol 3 Part G §7.3). Every change to the table comes
with a new link version. The node keeps with the bond the link version
whose table the phone last saw, and whether the phone has subscribed to
Service Changed, as a bonded client's subscription is kept across
connections (Vol 3 Part G §3.3.3.3). On a link encrypted with the bond,
while the phone is subscribed and the version it saw is not the node's,
the node indicates Service Changed over every handle, 0x0001 to 0xFFFF,
and once the phone confirms, keeps the node's version as seen. A new
pairing keeps the node's version, as the phone discovers the table as it
pairs; a link without the bond is told nothing. The app discovers the
table again, without Android's copy, when the table it finds has no
Service Changed. From link version 5 (roadmap D142).

*Why.* A phone keeps the table of a bonded node across connections and
discovers it again only when the node says it changed (Vol 3 Part G
§2.5.2); with an old table, its handles name other attributes, or none.
Before version 5 the table had no Service Changed, so a phone had to
forget the bond and pair again after an update that changed it, and the
app's own check is what carries a phone across the update to version 5.
The version says which table the phone saw, as the app already reads it
and every change to the table takes one: a change to the table without
a new version is not allowed. Every handle, because the phone then
discovers the whole table, a few round trips once per version. The hash
lets a phone that reads it as it connects find a change before the
indication comes.

**Public.** A node is public, serving phones that have not paired with
it, guests, as well as its owner's, unless its owner makes it private:
a shared node serves the people around it with nothing to set, and an
owner who wants it theirs alone says so. The paired phone writes 0 to
Public to make the node private and 1 to make it public again; a read
gives 1 for public. A node nobody has paired with is public, and so is
one whose bond was kept before this setting existed. The node keeps the
setting with the bond across reboots, and a new pairing keeps it
(roadmap D150). Public needs the bond, as Pairing says, so only the
owner sets it.

On a public node:

- *The advertisement* holds, in place of the service's UUID, the
  public node's, `28bc1001-a1b8-4893-9537-2ee3559d7644`, which no
  attribute has: an app finds the public nodes in reach in a scan,
  without connecting. The scan response is as before. The node
  advertises while it has a link free.
- *Node, Status, Send, Inbox, Given back, Recipient and Guest* take
  any link, encrypted or not, and so do the subscriptions to Status and
  Given back; Map point, Clock, Home, Neighbours, Update, Image,
  Public and Ask stay the paired phone's, as on any node.
- *It takes 2 links*, the owner's and one guest's, and keeps no state
  for a guest beyond its link. A guest's link is short: the guest
  connects, writes its recipient tag at the node to Recipient (The
  payload, below), sends what it has, reads what waits for it, and
  leaves, so that any number of guests share the one link in turn. The
  node ends a guest's link 30 s after it began, the ATT transaction's
  timeout (Vol 3 Part F §3.3.3), longer than a guest's errand; a phone
  that is not the owner's and writes Recipient while another guest's
  link is up is closed at once, as the other link is the owner's to
  take. Made private again, the node closes the guest's link.
- *Every message for the node's application goes into the ring*, the
  one a home keeps ([mailbox](#mailbox)), whether or not the node is a
  home, with the recipient tag it carries in the payload's second
  byte. The owner's, those with the owner's tag, kept with the bond, or
  every one while the bond holds none yet, are the owner's inbox, read
  through Inbox as a home's are; on a node nobody has paired with none
  is. The rest are the guests': each is written as sent, so no fetch of
  the owner's sends it and the owner's counts pass over it, and it
  leaves the ring only as the ring wraps, as the owner's do. A guest
  reads its own through Inbox, after the number of the last item it
  took, which it writes to Guest, 0 at first: a read gives the oldest
  item after it with the guest's tag, its number, the message ID it
  arrived with and the payload, or nothing; a number above the newest
  item's, from before the ring began again, counts as 0. The guest keeps the number,
  for each node; it confirms nothing, as reading takes nothing from the
  ring, and a write to Inbox on its link is answered 0x13. A guest that was away reads
  what came meanwhile, as long as the ring holds it.
- *Phones take turns at the one message the node holds for them*
  (Sending; [receiving DATA](#receiving-data)), and the turn is Ready:
  the node sets it, while it takes a message, for one phone at a time.
  The turn goes to the other phone connected when the node takes the
  message of the phone that has it, when that phone's link ends, or
  when that phone has had Ready for a ban without writing
  ([bans](#bans): 1.58 s, the shortest wait the protocol tells). A Send
  from a phone whose turn it is not is answered 0x80, not ready, as on
  any node; an app writes only while Ready is set, so only one that does
  not keep to that meets it. A message given back to a guest whose link
  has ended is dropped: the guest's app still holds it, as taken.

*Why the public node's UUID, not a flag beside the service's.* The 10
bytes the advertisement has left beside the flags and the service's
UUID hold one more AD structure (Core Specification Supplement, Part
A, Data types); those that carry a value of one's own name it by a company
identifier or a 16- or 32-bit UUID that the Bluetooth SIG assigns, of
which bark holds none, and the form with a 128-bit UUID, service data,
takes 19 bytes. A second UUID costs no byte, and both phone systems
filter a scan by service UUID in the controller, where a flag would be
read only by the app.

*Why one guest's link, and short.* The radio stack's RAM per link is
the cost, 5.6 KB of static RAM measured: the controller's 2.4 KB by
`sdc.h`'s formulas, the host's 1.2 KB, and the task that serves it,
1.9 KB, all of it out of the stack, which must keep 24 KiB, the most the
boards have used and a margin (`bark-fw/README.md`, "Tasks and the
stack"). With 2 links it keeps 24.1 KiB; a third left 14.6 KiB, and two
boards with it faulted at boot (roadmap D150). A link held open buys a
guest nothing the ring does not: the ring already holds mail for a phone
that is away, as it does the owner's, so a guest needs a link only for
its errand.

*Why the ring, by tag.* The tag says whose a message is without opening
it, and the ring keeps it in flash until it wraps whether its phone is
there or not, across reboots; the guest's number says what it has read,
so the node keeps nothing per guest. The core's inbox, one queue for
the application, would hold every message behind one for a guest who
is away.

*Why Inbox, with a number written to Guest.* A guest's mail is its
inbox, read as the owner's is; it carries the item's number, which the
guest writes back to read on. A readable value of its own would cost
its room in RAM again, 235 B, which the stack's floor does not leave.

*Why the turn is Ready.* One message at a time is the node's rule for
its application ([receiving DATA](#receiving-data)), and Ready is
already how the node says it takes one: one rule for one slot. Set for
every phone at once, the slot would go to whichever write reached the
node first, and a phone with a queue would keep it. A refusal with a
wait would need the person, as the app offers a given-back message
again only when they say so; an app waits for Ready by itself. A phone
with nothing to send passes the turn on after a ban, as nothing in the
protocol acts sooner.

**The payload.** The app owns it; this is its layout (roadmap D123 to
D127, D150). The reasons, and the alternatives a reader would ask about, are
in `docs/app-layer-2026-11-05.md`; the codec and the vectors the app is
tested against are `bark-app-layer`.

*Keys.* A person's **identity** is an X25519 public key. Every key
comes from a 128-bit random seed, which the person backs up as 12 words:
the seed and the first 4 bits of its SHA-256, 11 bits a word, from a
fixed list of 2048 (`bark-app-layer/src/words.txt`). All cryptography is
libsodium's: the master key is `crypto_generichash(seed, 32)`, the
identity's key pair `crypto_box_seed_keypair` of
`crypto_kdf_derive_from_key(master, 1, "bark-key")`, and the mailbox key
`crypto_kdf_derive_from_key(master, 2, "bark-mbx")`. No signatures, and
keys do not change.

*Forms.* A payload's first byte is its format: the version, 2, in the
high four bits, the form in the low four. Its second is the recipient
tag (below), which every version keeps there, as a public node reads
it. A receiver drops a payload of a version or form it does not know.

| form | to | after the format byte and the recipient tag | over the plaintext |
|---|---|---|---|
| 0, boxed | a contact | nonce (24, random), `crypto_box_easy` output (tag 16, ciphertext) | 42 |
| 1, introduction | someone who may not know the sender | `crypto_box_seal` output (ephemeral key 32, tag 16, ciphertext) | 50 |
| 2, fetch | the person's home node | nonce (24, random), `crypto_secretbox_easy` output under the mailbox key | 42 |
| 3, reply | the person's current address, from their home node | `crypto_secretbox_easy` output under the mailbox key of the item's number (3), the count of items still unsent (2) and the item; the nonce is not sent | 23 |

Forms 2 and 3 are the [mailbox](#mailbox)'s. The app writes at most
227 − 23 = 204 bytes, so that its mailbox can send any payload on as a
reply.

*Recipient tag.* The first byte of `crypto_generichash` of the
destination's node ID to 16 bytes, its shortest output, keyed with the
recipient's identity; in the clear. A boxed payload or an introduction
carries its recipient's tag at the node it is sent to, a fetch the
requester's at its home node, and a reply the one the fetch asks for. A
public node keeps a payload in its ring for the phone whose tag it
carries (Public, above); every other node and every app ignores it.

*Why keyed with the identity, over the node.* Only someone who holds
the recipient's card can make the tag, and it differs from node to
node, so it ties together only the payloads for one person at one
node, which their destination address already groups by node.

*Why one byte.* A phone handed a payload that is not its own fails to
open it and drops it (Public, above), so a wrong match costs a read
and a failed open, not a message. A node's ring holds the mail of its
owner and of its guests, read by tag: the owner's inbox passes over a guest's item unless the
tags match, and a guest reads, beside its own, the items of anyone whose
tag at the node is the same, each other person's with odds of 1 in 256,
0.4 %; a second byte would make that rarer at a byte on every message,
and would tie a person's payloads at a node together more surely. A boxed payload names no sender: the receiver tries the key of
each contact, `crypto_box_beforenm` of each made once, until one opens
it. An introduction carries the sender's card; its sender is shown as
unverified until a boxed payload from that identity opens. It is used
until a payload from the receiver, of either form, has opened at the
sender: sealed to the sender's identity, it shows that the receiver
holds the sender's card. Waiting for a boxed one instead would leave two
people who hold each other's cards sending introductions for ever, as
only a box verifies. An introduction naming an identity the receiver
has verified is dropped. Payloads are not padded.

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
| acknowledge | 1 + 4 | type 3, the `sent` of the text it answers |
| card | 1 + 1 + 38 to 83 | type 4, the card's length, then the card |

A receiver drops a message whose bit 1 is not what the two identities
say, and a text whose contact and `sent` it already has. The address is
that of the node the sender writes from, the one their phone is
connected to or was last, included until the contact has answered a
message that carried it and whenever it changes; a message without one
says it has not changed. The app sends nothing
by itself: an acknowledgement is a message the person chooses to send,
and a text is sent again only when the person says so (roadmap D126).

*Long texts.* A text longer than one message holds is split into
parts, each a message of its own with its own `sent`, delivered and
acknowledged as a text is. The parts take consecutive `sent`, so a
part's `sent` less its number and one is the first part's, which names
the text: the receiver puts parts together in any order of arrival,
knows from the count which have not come, and needs no byte more for
it. A part costs one byte over a whole text, 155 bytes of text in a box
without an address; 16 parts hold about 2.5 KB (roadmap D137).

*The card*, what people exchange, shown as a QR code: version (1), 2;
flags (1), bit 0 set when a home address follows, the others zero; the
identity (32); when the home was set or cleared, as `sent` (4); the
home node's address (0 or 13); and a display name in UTF-8 (0 to 32):
38 to 70 bytes without a home, 51 to 83 with one. As text, `BARK:` and
the bytes in base32 (RFC 4648, its base32 alphabet) without padding. A
card with a later time replaces a contact's card; the person sends
their card to their contacts when it changes.

*Why only a home.* A home's [mailbox](#mailbox) keeps a person's mail
until they fetch it. Any other node's address is only where they were
when the card was made, and where they write from reaches the other
side in every message's header. A person without a home can be written
to once they have written.

*Why the time without a home.* Clearing the home changes the card too,
and a card that drops it must replace an older one only if it is later.

**Where a text goes.** The app keeps a contact's home, from their card,
apart from the address their messages were last written from, with the
age of the newest message from them, counted from its `sent`, or from
its arrival when `sent` is ahead of the phone's clock. While that age
is under a minute, a text goes to where they write from; past
it, with a home, the person chooses between where they were and the
home's mailbox, for each text, and without one it goes to where they
were. With a home and nothing written from, it goes to the home; with
neither, the contact cannot be written to yet (roadmap D132).

*Why where they write from.* A reply goes straight to where the person
is, rather than to their home's mailbox to wait for a fetch, at
13 bytes on a message after a change.

*Why a minute.* In a minute a person on foot, at a brisk 1.5 m/s, moves
about 90 m, well within the ≈750 m a relay hears a handheld in the
street ([physical layer](#physical-layer)); a person driving has left
that reach by then, so a longer wait would not make the guess better.
Past it the app cannot tell whether they are still there, and the
person may know.

The sender's address travels inside the encrypted payload, so that the
relays past the first hop do not learn who sent it: a DATA with hop
count 0 names its sending node, and anyone in radio range of that node
learns its node ID and position from its heartbeats
([what the protocol omits](#what-the-protocol-omits)).

A message has no ID field: the network identifies it by a hash of its
destination and payload ([packet formats](#packet-formats)). The app
must make every payload unique; the random nonce, the introduction's
ephemeral key and a reply's round do that.

*Why unique.* Two messages to one destination with identical payloads
are one message to every relay that still keeps the first one's record
([message records](#message-records), [bounds](#bounds)), and the second
is not delivered.

---

## Mailbox

A person may set a node as their **home**, from the app over the phone
link; the app then puts its address on the person's card. It must be a
fixed relay, neither sleepy nor carried ([nodes](#nodes)), so that its
address stays. The app gives it the mailbox key and nothing else, by the
Home characteristic ([phone link](#phone-link); its values: `bark-link`
`home`),
so it cannot read the messages it keeps (roadmap D128;
`docs/app-layer-2026-11-05.md`, "The mailbox").

**The ring.** The home node keeps every payload delivered to its node ID
but a fetch in a ring of slots in flash, numbered from 1, and erases the
oldest page when the ring is full; an item leaves the ring only so. A
public node keeps its guests' payloads there too, written as sent
([phone link](#phone-link), Public). The
board's ring holds 1024 items (roadmap D139). A slot's mark says that
its item has been sent; it is cleared without an erase, so the item
stays. Its phone link's inbox reads from the ring, and an item the phone
confirms there is marked sent too ([phone link](#phone-link), Home).

**The fetch** is a form 2 payload to the home address whose plaintext
is `sent` (4), the requester's current address (13) and its recipient
tag at that address's node (1), which the home cannot make, as it does
not know the identity ([phone link](#phone-link), The payload). The node takes
it only if it opens under the mailbox key and its `sent` is later than
the last fetch's it took, which it keeps.

**The answer.** The node answers a fetch once, to the fetch's address:
one reply for each of the newest 32 items not marked sent, newest
first, one at a time through its send slot, or, when there is none, one
reply that says nothing is new. It marks an item sent when it hands the
item's reply to the core; nothing confirms it. An item that arrives
after the fetch waits for the next one. Each reply is a form 3 payload
with the fetch's recipient tag, sealed under the mailbox key with the
nonce: the fetch's `sent` (4), the reply's place (3), 17 zero bytes. An item's place is its rank in the
round, 1 to 32, and its plaintext is its number (3), the count of items
still unsent after the round's when the fetch was taken (2), and the
item; an item longer than the app writes goes empty. The reply that says
nothing is new has place 0 and an empty plaintext, 18 bytes; none
remains. The requester tries places 1 to 32 on a reply that carries an
item. A reboot ends a round, so that no place seals two items: what it
had not handed over stays unsent.

*Why nothing new is an answer.* A person pressed a button and waits:
without it they cannot tell "no mail" from a fetch or a reply lost on
the way. It costs one short frame, and the node keeps nothing open, so
mail moves only when the person asks (roadmap D128).

*Why a batch with a count.* The fetch's address goes stale when the
person moves, so a round must end while it is good: the network keeps
mail for a node for the delivery TTL, 600 s ([nodes](#nodes)). And one
press should cost the air of a batch, not of all the ring holds; the
count tells the person what is left, and they decide whether to fetch
again. A full bucket sends 33 full frames with the reply reserve left
([send queue](#send-queue)), so a batch of 32 leaves the home node, and
passes each relay on its way, at the pace of its hand-overs, without
waiting for a refill, and leaves a frame for the relay's other traffic.
Under load a hop's budget bounds a reply's wait in relays' queues at 4
to 8 minutes over a crossing ([send queue](#send-queue)), within the
TTL. The home node sends one reply at a time through its send slot, and
the network's back-pressure paces a batch: a reply the requester's node
has no room for is refused and waits at its sender
([phone link](#phone-link), Receiving), and one the home node's
neighbours refuse is given back to the mailbox and goes again.

*Why a sent item stays.* Nothing tells the home node that a reply
arrived. One lost on the way is still in the ring, and the phone reads
it at home after the last item it confirmed; a text it already has is
not kept twice ([phone link](#phone-link)).

*Why the number inside.* Places start at 1 in every round, so a
requester finds each reply in at most 32 tries without knowing which
numbers the ring holds; the number, which only it learns, names the
item in the ring.

**When to fetch.** Only when the person asks, so that a mailbox costs
air only when a person wants it (roadmap D128).

The mailbox is an application on the node, beside the phone link: it
uses the core's application interface (`core-interface.md`, Application) and
nothing else, and the simulator runs it the same way.

---

## Capacity and sizing

Rationale, not specification.

Kyiv model: ≈840 km², ≈400 km² built-up, ≈3 M people. Indoor coverage
needs ≈9.6 relays/km² (≈3,800 relays); outdoor-only ≈1.9/km² (≈750).
Rooftop relays hear ≈3 km ([physical layer](#physical-layer)), so a
collision domain, the area within which transmissions interfere, is
≈28 km² with ≈270 relays, ≈14 domains across the built-up city.

One 125 kHz channel at SF5 carries 1.95 kB/s raw (15.6 kbps) and
≈680 B/s with listen-before-talk (≈35% usable). A 100-byte text over 8 hops with ACKs
takes ≈0.72 s of channel time: per hop a 128-byte DATA packet (73.3 ms)
and its ACK (17.0 ms).

| Plan | Heartbeat load per channel | City-wide data | Texts/s | Texts per user per day at 300k users |
|---|---|---|---|---|
| Single channel, 60 s heartbeats | 6.5% | ≈6 kB/s | ≈6 | ≈2 |
| 7-colour fixed grid, 10 s heartbeats on all colours | 39% | none | none | none |
| **13 channels, measured selection, rotating heartbeat (this design)** | **≈3.0%** | **≈80 kB/s** | **≈80** | **≈23** |

These are orders of magnitude; the hearing radius and hop count are each
uncertain by 2×. The table assumes a text crosses the city. The city
centre is the hot spot, and the per-relay 10% cap
([duty cycle cap](#duty-cycle-cap)) binds there first. Each row is
the 14 domains times the row's channels times what of a channel's 35 %
the heartbeats leave, over 0.72 s a text, and a day of that over the
300 000 users; a text is about 1 kB of DATA on the air. The heartbeat
load is a domain's 270 relays' heartbeats, 14.4 ms each, on the
channels they visit:

- one channel, a heartbeat every 60 s: 270 × 14.4 ms / 60 s = 6.5 %,
  and 14 × 0.285 / 0.72 s ≈ 6 texts/s;
- seven colours, each relay heartbeating on every colour every 10 s,
  the mean interval of [heartbeat](#heartbeat):
  270 × 14.4 ms / 10 s = 39 %, more than the 35 % a channel can use,
  so no room for texts;
- this design, one heartbeat every 10 s on each relay's next channel:
  270 × 14.4 ms / (10 s × 13) = 3.0 %, and
  14 × 13 × 0.32 / 0.72 s ≈ 80 texts/s, over a day 23 for each user,
  the **design rate** the rest of this document uses.

The app's encryption and header take 48 to 61 bytes of a payload
([phone link](#phone-link)), so the 100-byte payload above holds a text
of 39 to 52 bytes. A message fetched from a [mailbox](#mailbox) crosses
the city twice: with a share f of messages fetched, the design rate is
23 / (1 + f) texts a person a day.

Sleepy nodes' beats cost what [beats](#beats) lists, 230 ms an hour
each, and relays nothing for them. Summed over the domain's 20 000
sleepy nodes they take 1.28 s of airtime a second, 10 % of one channel's
time over the 13, where every relay hears every other. A sleepy node in
anchorless mode costs more ([sleepy nodes alone](#sleepy-nodes-alone)):
its long beats, a relay's answers, and the long preamble on every DATA
to it.

## Updates over the mesh

A node given a new firmware image, by a phone or over USB, passes it to
the relays around it, and they to theirs, a piece at a time, at a pace
that leaves the channels to the texts. The protocol carries the pieces;
the code that makes them, the flash they wait in and the signature
check are the node's firmware below it (`core-interface.md`,
Application). A node's firmware chooses whose images it takes, so that
a node of another implementation is never overwritten against its will.

**The image.** An image is named by the SHA-256 of its signed header,
the 48 bytes of its header that its signatures cover: magic, format,
version, length and the image's own SHA-256 (`bark-image`, `sign.rs`).
The name is 32 bytes; the mesh names an image by its first 4. What
travels is the image as packed, its 256-byte header and then its bytes,
in **blocks**: block 0 is the block table, the whole name and the first
8 bytes of the SHA-256 of each other block, one piece up to 27 blocks
and two up to 48; then the packed image, in blocks of 64 pieces of 247
bytes, the last block shorter and its last piece padded (`bark-image`,
`pieces.rs`). Block 1 begins with the signed header, which hashes to
the name. Each block is sent as up to 255 coded pieces, and any 64
distinct ones of a block, or as many as it has, give it back (a
Reed-Solomon code over GF(2⁸), the firmware's, `code.rs`). Today's node
image, 478 704 bytes (the open build of main `9b693a0` on 2026-11-09,
`bark-fw/README.md`, "Build and package"), packs to 478 960 bytes,
1 940 pieces in 31 blocks, 30 of 64 and one of 20; its table is two
pieces, 32 blocks in all. An image has at most 48 blocks, the
table's among them, one for each lane the firmware keeps pieces in
(`pieces.rs`): 47 blocks of packed image, 743 KB, which
`bark-fw/scripts/image.py pack` refuses to pass.

*Why 4 bytes of the name.* Two images on the air at once in one place
share their first 4 bytes with odds of 2⁻³²; nothing on the air is
authenticated anyway ([what the protocol omits](#what-the-protocol-omits)),
and a piece of the wrong image fails its block's hash and is gathered
again. The firmware checks the whole name in block 0 against the signed
header in block 1.

A node **holds** an image's blocks once each has decoded and passed its
hash; it serves those, whether it holds the whole image or some of its
blocks.

**What a node tells.** A heartbeat may carry a **section** after its
first 13 bytes ([packet formats](#packet-formats)): the version the
node runs, 0 if it knows none; the newest image it holds or gathers, by
its version, the first 4 bytes of its name, the signer's key, its
blocks and the pieces of its last, or none; how many of its blocks the
node holds from the first; and how many pieces it holds of the block
after those. A node that knows neither the version it runs nor an image
tells no section. The count of pieces goes as it stands when sent.

A relay's heartbeats carry the section in a **rotation of sections**:
once on each channel, at its next heartbeat there, so that a heartbeat
skipped for a busy channel leaves its channel to the next one there
instead of missing it. A rotation of sections starts:

- at boot, which is a change;
- when any of the first three changes, the version it runs, its newest
  image or its blocks held;
- an hour after the last;
- in answer to a neighbour that **lags**, one whose section names
  neither an image nor a version run as new as the image the relay
  streams: at the relay's next heartbeat, unless a rotation of sections
  began within its own last rotation of heartbeats, however many lag.

While a relay gathers, every heartbeat it sends on the home channel of
its **helper** carries the section as well, which is once a rotation:
the helper is the relay it last took a piece from, and before its first
piece the best holder it hears. A piece does not name its sender, so
the node takes for it the relay whose stream carries that block to it
by rule 3 below: one at home on its own channel that holds the block,
else the one that holds the most blocks from the first, then the lowest
ID. A sleepy node's beats carry the section for one beat after a change,
in every beat while it gathers, and in one an hour: its anchor hears
every beat.

*Why boot is a change.* A node that boots knows nothing of its
neighbours' images, nor they of its, and telling them at once lets a
holder answer it within a rotation (below). It costs a rotation's 13
sections, 8.96 ms more each, once a boot.

*Why a gatherer says it every rotation.* A helper streams a block until
the gatherer's section shows it complete (rule 4), and a rotation after
a change reaches a channel only if that heartbeat finds it free. On the
bench a gatherer completed a block while three of its heartbeats were
skipped for busy channels, then moved home twice, and its holder,
hearing no section, streamed that block's tail, over 370 pieces, for
more than an hour (roadmap D158). Said once a rotation on the helper's own channel, the
section reaches the helper within a rotation of any change. It costs one
section a rotation for each gatherer: with a domain's 270 relays
([capacity and sizing](#capacity-and-sizing)) all gathering, 270 ×
8.96 ms over 130 s and 13 channels, 0.14 % of each channel's time.

*Why answer a node that lags.* A node that runs an older image learns
of a new one only from a holder's section, which would otherwise ride
hourly; answered, it learns within a rotation of telling its own,
which it does at boot. Each answer costs a rotation's 13 sections,
116.5 ms. Once a rotation however many lag bounds a relay to a section
in every heartbeat; a neighbour that takes the image stops lagging,
and one whose policy refuses it is answered at most each time it tells
its section, once an hour.

A node keeps each neighbour's last section in its neighbour table
([neighbour table](#neighbour-table)), as it keeps the rest of what the
neighbour's heartbeats say, and the images the sections name once, at
most four: the release spreading, the one before it, an older one of a
node that was away through a release, and one of another key, a
bench's, since a release follows the last no sooner than the day it
takes to reach every relay. A fifth takes the place of the lowest
version, and the sections that named that one name none; one older than
the four is kept as naming none. No node that runs a newer image
gathers the lowest.

*Why in the heartbeat.* One frame kind, the piece: the neighbour table knows what
each neighbour holds and lacks as it knows its channel, and there is
nothing to ask, to answer or to time. The cost is that a node learns of
a new image within a rotation of its heartbeats, 130 s, where a frame
of its own would tell it within a pace.

*Why the shape of the image in it.* The firmware sets up a block's lane
and knows how many pieces give back each block before the first piece
comes; block 0's own count follows from the blocks. The two bytes cost
one block of symbols, 1.28 ms: a heartbeat with its section is 30
bytes, 23.36 ms on air ([physical layer](#physical-layer)), against 13
bytes and 14.4 ms.

**Who sends what.**

1. A node that hears a section naming an image of a version above the
   one it runs and the one it gathers asks its firmware, once, whether
   its policy takes the image. If it does, the node gathers it; an image
   of a higher version heard later replaces it.
2. A node that gathers hands its firmware every piece it has not handed
   before of every block it lacks, and no other: at most 48 blocks of
   64, the lanes the firmware keeps them in (`bark-image`, `pieces.rs`).
   When the firmware says a block failed its hash, it takes that
   block's pieces again, and does not count it held.
3. A relay **helps** a channel where a neighbour's section says it
   gathers the image the relay gathers or holds and lacks a block the
   relay holds: it **streams** there, at the gap or the pace (below),
   from the blocks it holds. On its home channel it streams to every gatherer
   at home there. On another channel it streams only the blocks past
   those a relay at home there holds, by that relay's section, and to a
   gatherer only if it is the best holder it hears for it: no relay it
   hears that is not at home there, holds more than the gatherer and is
   no farther from it has more blocks from the first, or as many and a
   lower ID. It streams on at most two channels: its home, then those
   with the most neighbours lacking. It stops on a channel when no
   neighbour there lacks a block it streams there. It streams to a
   gatherer only on a **lease**: while it heard the gatherer's section
   within the gatherer's last two rotations and a heartbeat's lag
   ([heartbeat](#heartbeat)), 2 × 13 × T + 872.8 ms, 261 s at the mean
   interval and 391 s at the longest; a sleepy gatherer's rotation is
   its beat, so two beat periods and the lag, 601 s
   ([beats](#beats)).
4. A stream **sweeps** the blocks in order. Of the block at hand it
   sends fresh pieces, each block's index climbing from a random start
   and wrapping at 255, until it has sent what the neighbour there
   furthest behind on that block lacks, by its section, and a margin
   for loss: that count over 1 − 20 %, the share of pieces a
   gatherer missed in the simulator (20.5 % of the pieces sent to
   relays that lacked them, in the district at the design rate over an
   update's first 6 hours, `mesh-update-2026-11-07.md`). If that block
   is the lowest a neighbour there lacks, it goes on with fresh pieces
   of it, at the pace, until the neighbour's section shows it complete
   or its lease ends: the block's **tail**. Then it moves to the next block that a
   neighbour there lacks, skipping the blocks none lacks, and after the
   last starts the sweep again while any lacks something.
5. On its home channel, where it hears the others, a node that heard k
   pieces from other nodes since its last there sends its next there
   k + 1 paces after its last; having heard none, and on another
   channel, at the gap or the pace (pace and priority, below). The
   piece after a slot heartbeat is apart from this (sleepy nodes,
   below).

*Why a stream.* Every piece on a channel serves every node there that
lacks its block, whenever that node began, so the nodes of a channel
are in step by construction and need no agreement on a block. *Why not
one code over the whole image.* A receiver would have to solve for
1 940 unknowns at once from pieces read over the external flash's bus,
which its bit-banged bus makes slow, and a bad piece would show only at
the end; blocks of 64 decode as they complete and fail alone. *Why a
node serves the blocks it holds.* An image then moves through a chain
of relays, and every relay that gathers becomes a source as its blocks
pass. *Why it serves only what passed.* A block that fails its hash
would spread as garbage, and the gatherer could not tell it from a
forger's. *Why every block.* A node keeps whatever comes, so no piece
on its channel is wasted on it.

*Why a sweep in order.* A stream that sent one piece of each block in
turn would finish every block together, at the end, so a neighbour
could pass nothing on until it held the whole image. A sweep finishes
the blocks one by one from the first, and the neighbour streams each
on to the next hop as it passes: the image goes down a chain a block
behind, not an image behind. A node that joins late takes the sweep
from the block at hand and the blocks before it in the next sweep.
*Why the furthest behind and a margin.* The stream serves everyone on
the channel at once, so it stays on a block until the neighbour that
lacks most of it should have enough; with fewer, it would leave that
neighbour a block short until the next sweep, a sweep later. The
margin makes up the pieces lost on the way on average; a neighbour
that lost more takes the rest in the block's tail, if it is the lowest
it lacks, else in the next sweep. *Why a tail.* A holder that stopped
after a quota cut short would wait for the neighbour's next section to
learn what is still missing; the tail keeps the stream going until the
section shows the block, and at the pace it costs at most 1 % of the
channel. *Why a lease.* Without one the tail runs on while the
gatherer's sections go unheard, as on the bench (why a gatherer says it
every rotation, above). The gatherer says its section once a
rotation where its helper hears it, so two rotations ride out one lost
or skipped, and a stream to a gatherer that fell silent, moved away or
completed stops within two; a gatherer heard again is served again at
once. *Why a random first
piece.* Two helpers of a block then seldom send the same piece.

*Why the helpers share the pace.* Every helper keeps its own pace, so a
channel with several streaming would carry their sum: in a district run
with no such rule the update's frames came to 7 % of each channel's
time once 70 relays held the image. Helpers at home on a channel hear
each other there and each takes its share, so the channel carries one
piece a pace whatever their number. *Why the
sweep on a channel belongs to the relays at home there.* A relay hears
only its home channel, so only the relays at home on a channel can
share its pace; pieces from elsewhere would add up unheard. A stream
from another channel seeds a channel until a relay at home there has
caught up, and the chain then runs within each channel. Of the relays
elsewhere only the best holder near a gatherer streams to it, since
they cannot hear each other's pieces there; a relay stands back only
for one that is no farther from the gatherer, which reaches it as well.
In the district at the design rate over an update's first 6
hours, leaving a channel's blocks to the relays at home there cut the
pieces' airtime from 92 400 s to 69 000 s and the channels' busy share
from 4.2 to 3.5 %, and the best holder's rule to 56 900 s and 3.2 %,
the relays a little behind (`mesh-update-2026-11-07.md`). *Why share and not leave the
channel to one.* Every helper keeps streaming, each from the blocks it
holds, so a neighbour that lacks a block one helper does not hold still
has it from another, and the channel loses nothing when a helper goes.

**Pace and priority.** The pace is a floor, not the rate. A node sends
its next piece on a channel a **gap** after its last there, 4.16 s, a
full frame's 138.6 ms thirty times over, if the bucket allows it with
the reply reserve kept, its send queue holds nothing else and it has
heard no other node's piece on that channel since its own last;
otherwise at the pace: at most one on a channel in 13.86 s, a full
frame a hundred times over. One node's pieces take at most 3.3 % of an
idle channel's time, and 1 % of a loaded one's. A node sends pieces on
at most two channels in any one pace. They wait
behind the frames of the send queue ([send queue](#send-queue)) and go
after the sweep's readings, but at most 9 frames of the send queue go
before a piece that is due; then it goes before the next. Each is paid
from the bucket as a forward is, leaving the reply reserve
([duty cycle cap](#duty-cycle-cap)), and listens before it talks as a
heartbeat does ([heartbeat](#heartbeat)): up to three checks, and if
every one is busy the piece waits its pace. The pieces go on whichever
channel the pace frees first. A wait for the bucket ends a random time
within two frames later too, so that two holders the bucket holds alike
do not send together. Each pace ends a random time within two
frames, 277 ms, after the piece: two senders that start a pace
together then part by more than a frame, and hear each other, a quarter
of the time each pace.

*Why a floor.* The mechanism is the same at any rate: the sweep, the
sections and the sharing do not change with how fast the pieces go.
The duty cap already bounds a relay's air, and a text at a relay never
waits behind a piece: a piece goes only when the send queue is empty,
and the reply reserve stays in the bucket. Where another sender is
heard, or texts are queued, the pieces fall back to the pace.

*Why 4.16 s.* With no gap an idle holder sends as fast as its bucket
refills a full frame, 138.6 ms × 600 / 59 = 1.41 s
([duty cycle cap](#duty-cycle-cap)), and texts in the update's first
hours meet the pieces on the channels. The gap trades how soon the
relays hold the image against what the pieces take of a channel,
138.6 ms over the gap. In the district at the design rate, 36 hours,
seed 1 (`mesh-update-2026-11-07.md`, roadmap D156):

| gap | a channel at most | relays holding 50 / 90 / 100 % | texts of the first 12 h, latency p90 / p99 | relays' busy estimate, hours 1–3 | pieces lost by relays that lacked them |
|---|---|---|---|---|---|
| 1.41 s, the bucket | 9.8 % | 2.3 / 2.8 / 5.8 h | 40.9 / 1 389 s | 4.3 % | 35.9 % |
| 2.77 s | 5 % | 4.0 / 5.3 / 10.3 h | 21.7 / 1 377 s | 2.8 % | 23.8 % |
| **4.16 s, chosen** | **3.3 %** | **5.3 / 6.3 / 11.5 h** | **21.3 / 1 290 s** | **2.0 %** | **20.5 %** |
| 6.93 s | 2 % | 7.5 / 8.5 / 14.2 h | 20.2 / 1 267 s | 1.2 % | 18.7 % |
| 13.86 s, the pace | 1 % | 15.5 / 16.3 / 23.5 h | 19.6 / 1 268 s | 0.8 % | 17.1 % |

Without the update the texts of the first 12 hours wait 17.8 s at the
90th percentile and 1 053 s at the 99th. From 2.77 s to 6.93 s that
90th percentile hardly moves, so the gap buys margin on the channels
rather than latency: at 4.16 s the relays' channels read 2.0 % busy in
the update's first hours, clear of the 3 to 4 % where the network
collapses, against 2.8 % at 2.77 s, for the relays reaching 90 % an hour
later.

*Why 1 % of a channel.* In the simulator a relay's channel is busy 1.0
to 1.4 % of the time at the design rate of
[capacity and sizing](#capacity-and-sizing), 2.0 to 2.8 % at twice it,
and the network collapses at three times
(`collapse-research-2026-10-16.md`,
`bandwidth-research-2026-10-16.md`). A channel's pieces at 1 % add
less than the design rate itself.

*Why nine.* A node at its cap sends 9.83 full frames in one pace, its
bucket's fill over 13.86 s ([duty cycle cap](#duty-cycle-cap)). With
nine frames of the send queue let past, the piece is the tenth: under
full load the update takes a tenth of the node's airtime, a full frame
about every 14 s, the same as the pace when the node is idle. A busy
node's updates go as fast as an idle one's, and its texts lose a tenth
of their airtime while it serves.

*Why two channels.* A node that sends on another channel is deaf on its
own for the frame, as for a heartbeat, and a hand-over to it fails.
Two channels at 1 % each cost it about 2 % of its hand-overs.

**What it costs.** While no update runs, a relay's section rides one
rotation of its heartbeats an hour, 8.96 ms more on each of 13: over a
domain's 270 relays ([capacity and sizing](#capacity-and-sizing)),
270 × 13 × 8.96 ms an hour over 13 channels, 0.07 % of each channel's
time. While one runs, a relay's count of blocks grows at most once in
64 paces, 14.8 minutes, the fewest pieces of a block at the pace: at
most 4.1 rotations an hour more, 5.1 in all, 0.34 % of each channel's
time, and each gatherer's section once a rotation on its helper's
channel 0.14 % more ([what a node tells](#updates-over-the-mesh)), so
the heartbeats take at most 3.5 % against 3.0 % without. Were the
section in every heartbeat while an update runs, they would take
270 × 23.36 ms / (10 s × 13) = 4.9 %. A boot, and an answer to a
neighbour that lags, each add a rotation of sections, 116.5 ms. A busy channel that gathers adds
a piece a pace, 1 % of its time, however many help; an idle one a piece
a gap, 3.3 %. Today's image takes a sweep of 2 428 pieces from a helper
that holds it to a neighbour that holds none, at the margin of rule 4:
2.8 hours a gap apart on an idle channel, 9.3 hours at the pace.

**Sleepy nodes.** A sleepy node with an anchor gathers from its anchor
alone, and serves nothing. Its beats carry its section, and it takes
the pieces it hears in its windows ([delivery](#delivery)) and after
its anchor's slot heartbeats. The anchor sends one piece right after
each of its slot heartbeats on its home channel, where its sleepy nodes
listen for that heartbeat ([relay choice](#relay-choice)), while a
sleepy node there lacks a block it holds: the next piece of its stream
on that channel. That piece keeps no pace: it is fixed by the rotation,
one frame of 138.6 ms in a rotation of 130 s
([heartbeat](#heartbeat)), 0.1 % of the channel for each anchor, so it
neither waits for the home channel's shared pace nor counts toward it.
A node that hears a piece end within 143.6 ms of a heartbeat it heard
takes it for that sender's slot piece and does not count it. The piece
still pays from the bucket and listens before it talks, and if the
channel is busy it waits for the next slot. A stream only sleepy nodes
need goes only then. Every sleepy node of the anchor
takes the same piece, so all of them gather at once. The node listens
after its anchor's slot heartbeat for that piece while it gathers and
the anchor's section says it holds a block the node lacks: 143.6 ms,
the anchor's check of the channel, about 3 ms, the transmission's
start, under 1 ms ([heartbeat](#heartbeat)), and a full frame, with a
margin to 5 ms. It hears the anchor's section in the slot heartbeat it
wakes for anyway. A sleepy node without an anchor takes no part.

*What it costs a sleepy node.* One piece a rotation of its anchor, 13
of its heartbeats, 130 s at the mean interval
([heartbeat](#heartbeat)): today's image takes 2 428 rotations at
the margin of rule 4, 88 hours, 3.7 days, and as many
listens of 143.6 ms, 349 s, at the 5.8 mA of listening
([duty-cycled receive and the long preamble](#duty-cycled-receive-and-the-long-preamble)):
0.56 mAh, 0.07 % of the 850 mAh battery
([hardware](#hardware)). Its beats carry the section while it gathers,
8.96 ms more on each of its 16 beats an hour, 143 ms an hour on the
230 ms of its beats ([beats](#beats)): 13 s over the 88 hours, 0.1 mAh
at the 32 mA of transmitting
([sleepy nodes alone](#sleepy-nodes-alone)).
The pieces it hears in its windows shorten its time. The nodes of an
anchor gather together, so a district's sleepy nodes take days, not
days for each.

*Why the slot piece keeps no pace.* Shared, the pace let an anchor send
after few of its slot heartbeats wherever several streamed on its
channel: in the district at the design rate, over an update's first
24 hours, the shared pace held 39 % of the pieces due after slot
heartbeats, and listen-before-talk, the bucket and the anchor's own
pace together under 1 % (`mesh-update-2026-11-07.md`). Its own load is bounded by the
rotation, and a sleepy node can take a piece at no other time.

*Why after the slot heartbeat.* Every sleepy node of the anchor listens
for it in the same moment, so one frame reaches all of them; in a
node's own windows a frame reaches one node, and each window would be
paid by the channel's pace.

**What a node keeps.** The image it holds; the one it gathers, the
first block it lacks, the blocks that passed, a bit each, and the
pieces handed to its firmware of every block, a bit each, 48 × 32
bytes; per channel, the block its sweep is at, the pieces of it sent
and the pieces to send; each block's next piece; the last two channels
it sent pieces on and when; the times of the last 8 pieces it heard
from others on its home channel; what its section last told, the
channels its rotation of sections has still to reach, when that
rotation began and when the hour's is due, whether it owes a lagging
neighbour an answer, and its helper's ID: 1 888 bytes beside the
tables, and 16 bytes in each neighbour's entry for its section and
when it was heard (`core-interface.md`, Memory).

---

## What the protocol omits

Common mesh and radio mechanisms left out, and why.

| Omitted | Instead | Why |
|---|---|---|
| Neighbour lists in heartbeats and a network-wide node table | a 13-byte heartbeat; routing uses only direct neighbours | spreading tables through the network does not scale to thousands of relays at LoRa rates; addresses are positions, so there is nothing to look up |
| Addressing by ID with a lookup, or registration with a home relay | position + node ID; the owner resolves the ID locally ([owner](#owner)) | registration traffic grows with users; position addressing needs none |
| Time slots on a shared schedule | always listen, transmit as soon as listen-before-talk clears | no time sync exists, and slots add latency |
| Cooperative position estimation | GPS or a map point ([position](#position)) | every node knows its own position |
| A fixed channel grid, heartbeats on all channels | a measured home channel and a rotating heartbeat ([home channel selection](#home-channel-selection)) | 39% of every channel at indoor density, 270 relays × 14.4 ms every 10 s; where a rooftop relay hears ~3 km and a relay covers ~700 m, a grid only spreads load evenly, which measurement does as well while adapting to hot spots and foreign interference |
| GFSK at 50 kbps | LoRa at SF5 ([physical layer](#physical-layer)) | 3× more per channel than SF5 at 40% of the range, and needs a modem driver and error correction written from scratch; revisit only if relays saturate at SF5, since nothing above the modem would change |
| Spreading one signal over the whole band (DSSS/CDMA) | 13 separate channels | the SX1262 cannot, and in random meshes without power control separate channels carry 5–10× more (Weber et al., [sources](#sources)) |
| Frequency hopping | measured home channels | hopping on a common schedule has the capacity of one channel; per-receiver schedules are fixed channels plus time sync; random channels per packet need ≥20 ms preambles on every packet; no regulatory benefit at 433 MHz |
| Sleepy nodes heartbeating as relays do, every 10 s | a beat every 150–300 s ([beats](#beats)) | collisions happen at the shared relay whatever the sleepy node's range: 58% of a channel at 400 sleepy nodes per relay, 400 × 14.4 ms every 10 s |

**No authentication.** The network is open, and nothing on the air is
authenticated. Any node in radio range, by a bug or on purpose, can
forge any frame and disrupt its district, within its duty cycle and
unnoticed:

- refuse in a relay's name and ban it for up to 89 min ([bans](#bans));
- take a message in a relay's name, and the message is lost;
- send a heartbeat in an anchor's name and send its sleepy nodes
  elsewhere ([relay choice](#relay-choice));
- claim the position of a destination and own it ([owner](#owner)).

The design resists collisions and overload, not adversaries. Anyone in
radio range learns every node's ID and position from its heartbeats;
nobody farther away does. The app's end-to-end encryption keeps payloads
private and authentic ([phone link](#phone-link)); it does nothing for
routing, and a listener near a home node can tie it, by timing, to the
address its mail goes on to ([mailbox](#mailbox)).

*Why no authentication.* A real node of the network with a few
judicious bugs, by accident or intent, breaks large parts of it whether
or not frames are signed, since it is a legitimate participant. Signing
every frame would also need the hardware locked so that its holder
cannot extract the key, which an open device anyone can buy and build
cannot be. And no radio network withstands a serious adversary, who can
jam it in any case.

*Why no location privacy.* A node tells its position every rotation on
a channel anyone can hear. A node that took a new ID would be linked to
the old one by anyone who heard the old one fall silent and the new one
appear in the same place.

---

## Packet formats

Every packet begins with two bytes that say what it is: its kind with
that kind's one small field, then the format version. A heartbeat's
third byte holds its flags. Each field has one place.

| first byte | packet | the low bits |
|---|---|---|
| `1000 cccc` | heartbeat | `cccc` the home channel, 0 to 12 |
| `00aa aaaa` | DATA | `aux`: 0 to 12, a message, and the sender's home channel; 13 to 63, a frame from node to node of kind `aux` |
| `s1aa aaaa` | ACK | `aux`, 0 if the offer is taken or seen, else the code of the wait the refuser asks for, 1 to 63 ([bans](#bans)); `s` the seen flag ([the ACK](#the-ack)), with `aux` 0 |

The second byte is the **format version**. This specification is
format 1.

| packet | the second byte |
|---|---|
| heartbeat | the newest format its sender speaks |
| DATA, ACK | the format of this frame |

A node speaks every format from 1 to its newest. A heartbeat's first 13
bytes are the same in every format, and anything a later format appends
comes after them. So a node reads the first 13 bytes of every heartbeat
whatever its second byte says, ignores what follows, and keeps the
sender's newest format in its neighbour table
([neighbour table](#neighbour-table)). A heartbeat shorter than 13 bytes
is rejected. Which format a DATA and its ACK take is in
[hand-over](#hand-over).

*Why read only the first 13 bytes.* A later format can then add to what
a heartbeat says, and an old node still sees the neighbour. 13 bytes
fill 6 blocks of symbols exactly at SF5, so the first byte appended
costs a block, 1.28 ms.

From the first node planted, any change to what a field of DATA or the
ACK means, not only to where it is, takes a new format: an old node
would read the new meaning as the old.

A heartbeat's third byte is its **flags**. Each says one thing about the
sender and is set on its own:

| bit | flag | meaning |
|---|---|---|
| 0 | sleepy | "I sleep. Reach me cheaply in my window after my beat, or at any time with the long preamble." ([nodes](#nodes)) |
| 1 | anchorless | "I know no node that holds mail, one that is neither sleepy nor carried. If you are one, tell me." A sleepy node without an anchor sets it ([sleepy nodes alone](#sleepy-nodes-alone)), and a relay on the heartbeats of its burst, while its table is new; those are in no slot ([heartbeat](#heartbeat)) |
| 2 | carried | "I move. I hold mail for nobody, and take each GPS fix unless I sleep with an anchor." ([nodes](#nodes)) |
| 3 | map point | "My position is a map point a person set in the app, not a GPS fix." ([position](#position)) |
| 4–7 | | zero; a later format may define them |

A receiver ignores the flag bits it does not know.

*Why a flags byte.* A later format can then add to what a heartbeat
says without a new heartbeat layout, which every node, old or new, must
read. It costs no airtime: 13 bytes fill the same 6 blocks of symbols
at SF5 as 11.

A node sets the map point while its position is the map point, keeps it
across a reboot, and clears it when the app clears it ([position](#position)). Its
neighbours keep it ([neighbour table](#neighbour-table)); no rule uses
it yet.

*Why a map point is told.* Every node is assumed honest, since the
design breaks otherwise ([what the protocol omits](#what-the-protocol-omits)),
so a map point is a position a person vouched for, where a fixed relay's
first GPS fix could have been spoofed. Which checks may lean on that is
for later; until then the bit is carried, kept and shown.

**Frames from node to node.** A DATA whose `aux` is 13 to 63 is not a
message: it is one hop, never routed, unencrypted and unanswered unless
its kind says otherwise, and `aux` names its kind:

| `aux` | kind |
|---|---|
| 13, 14 | free: an update's announcement and request had them, and the heartbeat's section replaced both |
| 15 | an update's piece ([updates over the mesh](#updates-over-the-mesh)) |
| 16 | a neighbour's status asked ([neighbour status](#neighbour-status)) |
| 17 | a neighbour's status told, the answer to 16 |
| 18 to 63 | reserved for later kinds |

Such a frame keeps the first two bytes and lays out the rest for its
kind: none has a hop, a destination or a count, and the piece needs
every byte. It has lost the sender's home channel that `aux` carries in
a message, so a kind that expects an answer finds it in the neighbour
table, from the sender's heartbeats ([neighbour table](#neighbour-table)),
as a status asked does, or carries it in a byte of its own.

**The heartbeat's section.** In format 1 a heartbeat is 13 bytes, or 30
with an update's section after the first 13
([updates over the mesh](#updates-over-the-mesh)); a node that does not
know the section reads the first 13 as it reads any heartbeat. A later
format that appends more puts it after the section, and sends the
section with an image version of 0 when it has nothing to say.

A node rejects a heartbeat whose first byte has bit 4 or 5 set or whose
channel is 13 to 15, an ACK with the seen flag and a wait, a DATA whose
`aux` is a reserved kind, and a DATA, ACK or frame from node to node in
a format it does not speak. A
heartbeat byte with channel 15,
which no node sends, starts the bench frames of the firmware's
measurement console (`bark-fw/README.md`).

A position is 7 bytes, the heartbeat's short position 4
([positions on the wire](#positions-on-the-wire)), and a node ID 6
([node identity](#node-identity)). The LoRa hardware CRC covers
everything.

```
HEARTBEAT first(1) format(1) flags(1) id(6) short_pos(4)                    = 13
          optional section, after the first 13:
          runs(4) version(4) image(4) key(1) blocks(1) last(1)
          complete(1) have(1)                                               = 30
          runs = the version the node runs, 0 for none told;
          version = the newest image it holds or gathers, 0 for none;
          image = the first 4 bytes of that image's name; key = which
          signer's key signed it; blocks = its blocks, 2 to 48; last =
          the pieces of its last block, 1 to 64; complete = the blocks
          the node holds from the first; have = the pieces it holds of
          block `complete`
ACK       first(1) format(1) id(4) from(6) to(6)                            = 18
          the answer to a DATA, taken, refused or seen; sent on the channel
          in the answered packet's aux
          from, to = the IDs of the answering and the answered node
DATA      first(1) format(1) dst_hop(6) src_hop(6) dst_pos(7) dst_id(6)
          hops(1) payload                                                   = 28 + payload
          dst_hop, src_hop = IDs of the receiving and the sending node;
          aux = sender's home channel, so the receiver knows where to ACK
PIECE     first(1) format(1) image(4) block(1) index(1) piece(247)          = 255
          aux = 15; index = which coded piece of the block, 0 to 254
ASK       first(1) format(1) to(6) from(6)                                  = 14
          aux = 16; to, from = the IDs of the node asked and the node asking
STATUS    first(1) format(1) to(6) from(6) flags(1) battery(1) heard(1)
          strong(1) proven(1) version(4) gathers(4) blocks(1)
          complete(1) have(1)                                               = 30
          aux = 17; to, from = the IDs of the node that asked and the node
          answering; the rest as [neighbour status](#neighbour-status) says
```

Multi-byte numbers in the section and the status, the versions, are
big-endian.

*Why the wait in the first byte.* Every answer is then one packet, of
one length and one airtime, 17.0 ms, and the reply reserve counts one
figure. A byte of its own would make the refusal a second packet, and
at 19 bytes it would take a block of symbols more than the 18-byte ACK
(roadmap D76).

The **message ID** is a 32-bit FNV-1a hash of
`dst_pos || dst_id || payload`, computed by every node that handles the
message; it is the same on every hop because none of these fields
changes. DATA does not carry it; the ACK does, to name what it
acknowledges. The app must keep payloads unique
([phone link](#phone-link)).

`dst_hop` lets every node on a channel discard packets not addressed to
it after 8 bytes. `src_hop` and `aux` name the sender and its home
channel, where the ACK goes and where a return is sent
([routing](#routing)). The payload is at most 255 − 28 = 227 bytes; the
sender's address is inside it, encrypted.

### Positions on the wire

A position is WGS-84 latitude and longitude in fixed point, 25 + 26 bits
and 5 spare in 7 bytes, ≈0.6 m, the same everywhere on Earth: one
firmware and one app, with no per-deployment origin.

*Why that size.* A 1 m grid over the Earth's 5.1 × 10¹⁴ m² needs 49
bits, so 51 bits is within 2 bits of the minimum; decimetres would need
56. Plain fixed point keeps the distance arithmetic trivial.

Routing needs distances, not coordinates: each node converts every
position it handles into metres on a flat plane centred on itself,
`Δy = Δlat · R` and `Δx = Δlon · R · cos(lat_self)`, with
`cos(lat_self)` a per-node constant. The error grows with
Δlat · tan(lat): at 50° N it is 0.1 % at 5 km and 0.4 % at 20 km, enough
to reorder only near-ties between candidate hops.

**Short position.** The heartbeat carries a node's position in 4 bytes:
the low 16 bits of its latitude count, then the low 16 bits of its
longitude count, big-endian. A receiver rebuilds the full position as
the one candidate within half a period of its own on each axis, a
sleepy node's anchor's while it has one ([beats](#beats)). DATA
`dst_pos` stays 7 bytes because in a large city a node may be more than
half a period from the destination (roadmap D14).

*Why the short position is unambiguous.* One count is 180° / 2²⁵ ≈
0.6 m, so the low 16 bits repeat every 65 536 counts: 39 km of latitude
and 39 km × cos(latitude) of longitude (25 km at 50° N), far beyond any
hearing radius (≈3 km, [physical layer](#physical-layer)). That holds to
latitudes near 80°, where the east-west period shrinks toward the
hearing radius.

---

## Implementation notes

**Rust stack:** `embassy-nrf` (nRF52840); the SX1262 driven by
`bark-driver`'s sequencer, its commands encoded by a vendored copy of
`lora-phy` (roadmap D4, D88); TrouBLE with the Nordic controller for
BLE (D5); the stock UF2 bootloader for flashing
(D6), and a loader of our own behind it that swaps in a new image and
back if it does not confirm itself (D79). The protocol core does no I/O of
its own and owns its routing and tables (D1, D3; `core-interface.md`).
Crate layout is D7.

**Updates.** A node on battery whose last reading is at or below the
low level ([sleepy nodes](#sleepy-nodes)) starts no swap to a new image;
on external power nothing limits it (roadmap D143). A cell that gives
out in the middle of a swap leaves the node with no image to run until
it is charged, where without the swap it would run on asleep for days.
A node stages only an image above its own version and signed by a key
it was built with, the project's and, on a bench build, the bench key
(roadmap D144, D154).

**RAM (256 KiB):** every board holds a relay's tables, the heard table
and the delivery queue included, since every node may turn relay
(roadmap D71); their sizes, and how they fit beside Embassy, the
BLE stack and the call stack in the 255 KiB the application has once 1 KiB is kept
for the bootloader's words and the panic message (`bark-image/src/layout.rs`),
are in `core-interface.md`, Memory.

**Stored state.** A node keeps in flash what changes slowly and saves a
rotation after a reboot, written only when it changes, and comes back
with it ([channel at boot](#channel-at-boot)):

| | |
|---|---|
| home channel | a relay keeps it and makes no choice at the end of its first rotation, so that its neighbours, which still list it, reach it at once; a carried sleepy node keeps the one it had when last fixed |
| the place a fixed node stands | its position after the reboot, as before it, and whether it is a map point ([position](#position)) |
| carried flag | kept, still since the boot |
| a fixed sleepy node's anchor, ID and position | taken at once ([relay choice](#relay-choice)) |
| whether it is a relay | a fixed relay that boots on battery relays on ([sleepy nodes](#sleepy-nodes)) |
| its battery readings' scale | from its last calibration ([sleepy nodes](#sleepy-nodes)) |
| record | 32 bytes with a checksum, appended to a 4 KiB page of flash, which is erased when its 128 slots are full (`bark-fw/src/store.rs`) |

*Why not time.* The board has no clock that runs while it is off, so
every timer starts afresh: a carried node is still since the boot, and a
relay on battery begins its grace again.

*Why not a carried node's place, nor a carried sleepy node's anchor and
home.* They change as it goes, its anchor at each relay it passes, and
each change would be a write; after a reboot it is somewhere else.

*Why not the noise floor.* The sweep takes a reading a second, and
their 10th percentile is at the floor within a minute or two
([busy estimate](#busy-estimate)); kept, it would be written whenever it
crossed a 1 dB step.

*Why the page lasts.* The nRF52840's flash is rated for 10 000 erase
cycles, and a page holds 128 records between erases: 1.28 million
writes, 35 years at a hundred a day.

The bring-up order and the test rig are in `roadmap.md`, Phase 2.

---

## Open items

Resolved items keep their numbers and are in `roadmap.md`. The steps
are those of the bring-up order, `roadmap.md` Phase 2.

| # | Item | Owner/when |
|---|---|---|
| O3 | Exact thresholds: busy threshold margin (6 dB), move hysteresis (0.05 / 10 min), demote count, ACK timeout | tune in steps 3–4 |
| O5 | Adjacent-channel rejection of the SX1262 at 125 kHz spacing, with a neighbour at full power; if inadequate, 11 channels at 150 kHz spacing, the same end centres, 433.170–434.670 MHz ([channel plan](#channel-plan)) | step 1 |
| O7 | Sending to an area (address = position + radius, node ID = broadcast) | later |
| O11 | Relay-side RSSI noise-floor calibration method and drift | step 3 |
| O12 | Confirm RAK WisMesh Repeater 433 availability for rooftop solar units | procurement |
| O16 | GPS wander of a still relay indoors: how far a still node's fixes lie from one of its own for 72 s ([position](#position)). Bench 2026-11-06, three boards at a window by day: up to 199 m, no two fixes more than 218 m apart, so the carried radius is 250 m (roadmap D136). The night after, 116 598 fixes, one board's fixes lay beyond 250 m for 63 s, 329 m at the farthest at HDOP 6.7, and it turned carried; counted only at HDOP 3 or under, no run beyond 200 m lasted (`docs/gps-bench-2026-11-06.md`, "The night") | step 2 bench, roadmap D70, D136 |
| O18 | The C/N0 band a real sky gives at the L76K, open sky to indoors, and what a spoofer's satellites show ([position](#position)). Bench 2026-11-06: indoors at a window 8 to 32 dB-Hz a satellite, a fix's mean 12 to 26 (`docs/gps-bench-2026-11-06.md`); open sky and a spoofer not yet | step 2 bench, roadmap D74 |
| O19 | A sleepy node carried farther than half a short position's period, 19.5 km north-south and less east-west, while it hears no relay: it rebuilds its next anchor's position a period off, and the neighbour check drops the fix that would correct it ([beats](#beats), [position](#position)) | later, roadmap D91 |

---

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
  (`crypto_box`), "Sealed boxes", "Secret-key authenticated encryption"
  (`crypto_secretbox`), "Key derivation" (`crypto_kdf`) and "Generic
  hashing" (`crypto_generichash`).
- RFC 4648, "The Base16, Base32, and Base64 Data Encodings": base32.
- FIPS 180-4, Secure Hash Standard: SHA-256, the backup's checksum and
  an image's hash and name.
- RFC 8032, "Edwards-Curve Digital Signature Algorithm (EdDSA)":
  Ed25519, an image's signature.
- Nordic nRF52840 Product Specification: the radio's transmit and
  receive currents.
- Apple, Accessory Design Guidelines for Apple Devices, "Advertising
  Interval", and Technical Q&A QA1931: the advertising intervals an
  iPhone finds quickly.
- Weber, Andrews, Yang, de Veciana, "Transmission capacity of wireless ad
  hoc networks with outage constraints", IEEE Trans. IT 2005 (frequency
  hopping vs direct sequence).

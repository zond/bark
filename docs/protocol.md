# bark protocol: Civilian 433 MHz Mesh

**Status:** live specification, 2026-10-14. Decisions that refine it are in
`roadmap.md` with D-numbers; §16 lists what is still to be decided or
measured. Where the code and this text disagree, one of them is wrong and
the fix is a commit to both.

A mesh network of cheap radio nodes, legal without a licence in the EU and
Ukraine, that lets phone apps message each other across a city when
ordinary infrastructure is gone. It is **not** designed to survive
deliberate jamming; it is designed for infrastructure loss.

---

## 1. Summary

| Item | Decision |
|---|---|
| Hardware | LILYGO T-Echo, 433 MHz variant (nRF52840 + SX1262 + L76K GPS + e-ink + BLE 5.0 + 850 mAh), one firmware for all roles |
| Band | 433.050–434.790 MHz, 10 mW effective radiated power (ERP), ≤10% duty cycle per node (legal in the EU and Ukraine) |
| Modulation | LoRa, spreading factor (SF) 5, 125 kHz, coding rate 4/5, fixed network-wide |
| Channels | 13 × 125 kHz; each node picks its **home channel**, the one it listens on, by measuring how busy each channel is; no fixed plan, no time sync |
| Medium access | Always listening on the home channel, leaving it only to transmit; acknowledgements (ACKs) come back on the sender's home channel; listen-before-talk before every transmission; transmit as soon as the channel is clear; no time slots |
| Heartbeat | A relay's periodic 8-byte broadcast of its home channel and position, every 5–15 s at random, on a channel that steps through all 13 in order |
| Neighbour discovery | Passive: a node hears every neighbour's heartbeat once per 13 heartbeats (~130 s) on its own home channel; no scanning, no schedules |
| Routing | Forward to the neighbour closest to the destination **position**; route around gaps by depth-first search; ACK every hop; stop using a neighbour that fails to ACK |
| Addressing | A relay position plus a 4-byte leaf ID; a relay's position is its only identity; no registration, no network-wide tables |
| Roles | **Relay**: a fixed node on mains, USB or solar that sends heartbeats, forwards, and holds messages for its leaves. **Leaf**: a carried node on battery that attaches to one relay and otherwise stays silent |
| Phones | Bluetooth LE (BLE) GATT link to the node; the app encrypts end to end; the sender's position travels encrypted in the payload |
| Position | WGS-84 latitude/longitude, 51-bit fixed point in 7 bytes (≈0.6 m), the same everywhere on Earth; distances computed in a flat local approximation at each node; GPS when available, else entered by the user; relays use surveyed coordinates; never used for timing |
| Time | No network time synchronisation of any kind |

---

## 2. Hardware

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

The role is a runtime setting. Carried on battery: leaf. Plugged into USB
on a windowsill, or in a solar box on a roof: relay.

---

## 3. Regulatory envelope

Both regimes allow any modulation in this band.

| | EU (ERC/REC 70-03, Annex 1, band g1; "f1" in editions before 2026) | Ukraine (NCEC decision 81 of 12.02.2025, row РІ 50-3-1, as amended by decision 212 of 06.05.2026) |
|---|---|---|
| Frequency | 433.050–434.790 MHz | 433.04–434.79 MHz |
| Power | 10 mW ERP | 10 mW ERP |
| Duty cycle | ≤10% | <10% |
| Channel spacing | no requirement | no requirement |
| Listen-before-talk | not required | not required |

Implement: a rolling one-hour airtime budget per node capped at 10%; no
single transmission longer than 1 s (SF5 packets are ≤139 ms anyway).
Transmit power such that ERP ≤ +10 dBm with the fitted antenna: with the
stock antenna (≈0 dBd gain) that is +10 dBm at the connector.

The 868 MHz band does not work for Ukraine (no 500 mW sub-band; 868.0–868.6
MHz is 14 dBm at <1%), and 2.4 GHz has too little range. 433 MHz is the
only band with identical rules in both jurisdictions.

---

## 4. Physical layer

| Parameter | Value | Note |
|---|---|---|
| Modulation | LoRa (chirp spread spectrum) | SX1262 native; `lora-phy` Rust driver exists |
| Spreading factor | 5, fixed network-wide | SF5/SF6 are SX126x-only; all nodes are SX126x. Compile-time constant, changeable only for bench work |
| Bandwidth | 125 kHz | |
| Coding rate | 4/5 | |
| Raw bit rate | 15.6 kbps at SF5 | 5.5 kbps at SF7 |
| Symbol time | 0.256 ms at SF5 | |
| Preamble | 12 symbols (3.1 ms) + 6.25 symbols of sync; **no long preambles** except to duty-cycled leaves (§10.5) | 12 is the SX1262 minimum at SF5 and SF6 (datasheet §6.1.4) |
| 255-byte packet | 138.6 ms at SF5 | ≈400 ms at SF7. Airtime from the datasheet formula (§6.1.4), `time_on_air` in `radio.rs` |
| Sensitivity | ≈−120 dBm at SF5 | ≈−124 dBm at SF7 |
| Noise floor (125 kHz, noise figure ≈5 dB) | ≈−118 dBm | SF5 signals near sensitivity sit below the floor: signal strength (RSSI) cannot see them, channel activity detection (CAD) can |
| Sync word | private (not the LoRaWAN 0x34) | other LoRa networks and this one ignore each other's packets |
| CRC | on (LoRa hardware CRC) | |
| Maximum packet (MTU) | 255 bytes | SX126x maximum LoRa payload |
| Range, relay↔handheld, street | ≈750 m at SF5, ≈1 km at SF7 | |
| Range, relay↔handheld, indoors | ≈300 m at SF5, ≈400 m at SF7 | |
| Range, rooftop↔rooftop | several km | rooftop relays hear ≈3 km; this sets the size of a collision domain (§13) |

RSSI is the received power the radio reports; CAD is the radio's check for
a LoRa preamble on the channel, and works below the noise floor.

SF5 carries about 3× the bytes per unit of airtime of SF7 (the common LoRa
default) for a quarter less range. LoRa brings an existing driver, hardware
error correction, CRC and whitening, CAD, and ~15 dB of processing gain
against the key fobs and LPD433 handhelds that share the band. GFSK: §13.

SF is network-wide because an SX1262 receives one SF at a time, so a
mixed-SF network splits into parts that cannot hear each other. Starting
at SF7 would force a network-wide switch later; the deployment is expected
to start at a density where SF5 range suffices (indoor coverage at ≈9.6
relays/km², §13).

---

## 5. Channel plan

Thirteen channels of 125 kHz with 57.5 kHz guard at each band edge:

```
f_k = 433.170 MHz + k × 0.125 MHz,   k = 0..12
      433.170, 433.295, 433.420, 433.545, 433.670, 433.795, 433.920,
      434.045, 434.170, 434.295, 434.420, 434.545, 434.670 MHz
```

Channel 6 is centred on 433.920 MHz, the busiest spot in the band (key
fobs, sensors). The selection rule in §6 avoids it wherever it is busy.

**Bench item C1:** verify SX1262 adjacent-channel rejection at 125 kHz
spacing with a neighbour at full power. If it is inadequate, fall back to
11 channels at 150 kHz spacing (same end centres, 433.170–434.670 MHz).

Channels are not assigned by a fixed grid or derived from position; each
node measures and chooses (§6). Why not a grid: §13.

---

## 6. Home channel selection

Every node has one **home channel**, on which it listens whenever it is
not transmitting. All traffic *to* a node is sent on that node's home
channel.

### 6.1 Busy estimate

Per channel, keep `busy[c]`, an exponential moving average of a 0/1 busy
reading:

- **Sweep:** one RSSI reading per channel per second, spread evenly: one
  reading every 1 s / 13 ≈ 77 ms, channels in turn, each taken only when
  the modem reports no preamble detected and no packet in progress.
  A reading is: retune (≈0.1 ms), read (≈0.3 ms after retune), retune
  home; ≈0.5 ms, so ≈6.5 ms per second, 0.65% of the time off the home
  channel.
- **Cost:** while the node is away it can miss a packet. The modem needs
  some length `D` of preamble to lock on, taken here as 4 symbols
  (≈1 ms; measure in bring-up step 1, §15). A packet is lost if its
  preamble starts too late before the gap for the modem to have locked,
  or too early in it to leave `D` after it: a window of `g + 2D − P` per
  gap of length `g`, with `P` the preamble, 12 symbols (3.1 ms, §4). For
  one spread reading, 0.5 + 2.0 − 3.1 < 0: no packet is lost. One burst
  of all 13 readings, ≈5 ms, would lose 5 + 2.0 − 3.1 ≈ 4 ms of arrivals
  per second, 0.4%. Spreading loses nothing as long as
  `D ≤ (P − g) / 2` ≈ 1.3 ms, 5 symbols; if the bench shows the modem
  needs more, redo this comparison.
- **Reading:** 1 if RSSI > noise floor + 6 dB (≈−112 dBm), else 0.
  Average the 0/1 reading, not the dBm: what matters is the fraction of
  time listen-before-talk would refuse.
- **Window:** α ≈ 1/600 per sample (≈10 minutes at one sample per channel
  per second). One sample per heartbeat interval would give 60 samples
  per window, ±4 points on a 10% channel, as large as the differences the
  rule acts on.
- **Home channel:** also count the node's own receive time (preamble
  detected to packet end, from modem interrupts) as busy, so the home
  estimate is more accurate than the sampled ones. The bias is in the safe
  direction for a "should I move" decision.
- **Noise floor:** calibrate per device at boot as a low percentile of RSSI
  readings over the first rotation; recalibrate slowly.

Foreign transmitters (key fobs, LPD433 handhelds) count exactly like
peers. What hurts a node is busy time at its own receiver, and RSSI
measures that directly; counting neighbours per channel would not, since
two neighbours that hear each other share a channel cleanly.

### 6.2 Boot

Listen on a random channel for one full rotation (≈150 s), collecting
sweeps and heartbeats. Then set home to `argmin busy[c]`, breaking ties
randomly (±0.01), so that nodes that hear nothing do not all pick
channel 0. A config override exists for bench work.

### 6.3 Move rule

Move only when `busy[home] − min_c busy[c] > 0.05` continuously for
10 minutes; then wait a random 0–5 minutes, re-check, and move to
`argmin` with random tie-break. Immediately after retuning, send **one
heartbeat on the old channel** (it carries the new channel): that retunes
the node's leaves and any neighbour parked on the old channel at once.
No other announcement. Neighbours that miss it learn the new channel from
the next rotation (≤130 s) or from ACK failure (§9.3).

The hysteresis keeps moves rare. Without it, every rooftop relay in range
of the same interference sees the same ranking and they all jump together.

---

## 7. Heartbeat

Every relay sends a heartbeat. Leaves do not (§10).

| Field | Bits | Content |
|---|---|---|
| type | 4 | 0x1 heartbeat with surveyed position, 0x2 heartbeat with GPS-derived position |
| channel | 4 | home channel index 0–12 |
| lat | 25 | WGS-84 latitude, fixed point: 180° / 2²⁵ ≈ 0.6 m |
| lon | 26 | WGS-84 longitude, fixed point: 360° / 2²⁶ ≈ 0.6 m at the equator, 0.4 m at 50° N |
| spare | 5 | zero |

8 bytes, 11.8 ms on air at SF5 (§4). There is no node ID: a relay's position
**is** its identity (§8.1).

The position is global: one firmware and one app everywhere, no
per-deployment origin. A 1 m grid over the Earth's 5.1 × 10¹⁴ m² needs
49 bits, so 51 bits in 7 bytes is within 2 bits of the minimum;
decimetre precision would need 58 bits. Plain fixed-point latitude and
longitude keeps distance math trivial (§11).

**Timing:** interval uniformly random in [5, 15] s, mean 10 s.
**Channel:** steps `k ← (k + 1) mod 13` per heartbeat, from a random
start. The time is random, the channel order is not: a fixed cycle
reaches every neighbour once per 13 heartbeats (≈130 s), where a random
channel per heartbeat would need ≈41 heartbeats on average to cover all
13.
**Procedure:** retune to channel k, listen-before-talk (CAD + RSSI,
≈1 ms); if busy, retry after a random 0–200 ms up to three times, then
skip; send; retune home. The node is deaf for ≈15 ms per heartbeat, ≈0.2%.

This is why no scanning is needed: every node sits on its home channel,
every neighbour's heartbeat visits all channels, so each neighbour lands
on your channel once per rotation. A newly started node, or a leaf,
listening on any one channel hears every relay in range within one
rotation.

Heartbeat load per channel among 270 rooftop relays that all hear each
other: ≈2.5%.

---

## 8. Neighbour table

### 8.1 Relay identity is position

A relay has no node ID. Its identity is `(x, y)` from its heartbeat, which
is also its address (§9.1). Consequences:

- A relay **freezes** its position once accepted: a surveyed position from
  the app, or the first plausible GPS fix. Later GPS readings never update
  it. A changed position is a new identity and needs an explicit reset
  from the app.
- At boot, during the one-rotation listen (§6.2), a relay that hears its
  own exact position in another relay's heartbeat moves itself by 1 m
  and listens again. This is tidiness, not correctness: two relays at one
  position behave like one relay with two radios (both ACK, both forward,
  and duplicates are dropped downstream).
- A packet field that only names a relay carries the relay's **short
  name**: the low 16 bits of its latitude count followed by the low
  16 bits of its longitude count, 4 bytes, big-endian, in that order.
  These fields are ATTACH `relay` and DATA `dst_hop` and `src_hop` (§14).
  One count is 180° / 2²⁵ ≈ 0.6 m, and the low 16 bits repeat every
  65 536 counts: 39 km of latitude, and 39 km × cos(latitude) of
  longitude (25 km at 50° N). Two relays share a short name only if they
  are in the same 0.6 m cell, which is the same identity in the full form
  too, or at least 25 km apart along one axis, far beyond any hearing
  radius (≈3 km, §13). So a short name is unambiguous among everything a
  node can hear, down to latitudes near 80° N or S, where the east-west
  period shrinks toward the hearing radius. A receiver rebuilds the full
  position from a short name and its own position, as the one candidate
  within half a period on each axis, and looks tables up by it; a short
  name that rebuilds to a position nowhere near is garbage. DATA
  `dst_pos` stays 7 bytes because in a large city a relay may be more
  than half a period from the destination (roadmap D14).
- Leaves keep a 4-byte ID because they move. The app generates it at
  random; a collision matters only between leaves attached to the same
  relay (≈2 × 10⁻⁵ at 400 leaves). Neither role needs a setup step.

### 8.2 Table

Per node, up to 64 entries:

| Field | Content |
|---|---|
| x, y | from heartbeat; the entry key and the neighbour's identity |
| channel | from heartbeat (the channel to transmit to this neighbour on) |
| last_heard | local monotonic time |
| rssi_ema | averaged RSSI, for a leaf's choice of relay and for tie-breaking |
| fail_count | consecutive ACK failures |

- **Expire** after 3 rotations (390 s) without a heartbeat.
- **Demote** on ACK failure: `fail_count += 1`; a neighbour with
  `fail_count ≥ 2` is not used as a next hop until it ACKs a packet from
  this relay or three rotations (390 s) pass. Its heartbeats keep it in
  the table but do not clear the demotion: a heartbeat proves the
  neighbour is alive, not that it hears us, and a link that works one way
  only would otherwise cost every message the failed attempts again.
  Heartbeat expiry takes minutes; a failed forward must demote at once.
- **Eviction** when full and a relay not in the table is heard: the
  newcomer replaces the least recently heard demoted entry; failing that,
  the least recently heard entry if it has not been heard for a
  worst-case rotation, 13 × 15 s = 195 s (§7), so it has certainly missed
  a heartbeat on this channel; failing that, the newcomer is not added
  (roadmap D12). Replacing the least recently heard entry outright does
  not work for a rooftop relay that hears ≈270 relays (§13): about two
  heartbeats a second arrive on its channel, most from relays not in the
  table, so each entry would last about 64 / 2 ≈ 30 s and every failure
  count would be forgotten within a minute. Keeping incumbents holds an
  equally arbitrary 64, keeps their failure counts, and replaces exactly
  the entries that are failing or silent. Choosing a set spread around
  the relay, which would serve greedy routing better, is left to the
  simulator (open item O4).

Leaves are not in the neighbour table; they are in the leaf table (§10).

---

## 9. Forwarding

### 9.1 Addresses

A destination address is `(x, y, leaf_id)`: the position of the relay the
recipient is attached to, plus the recipient's 4-byte ID. Relays have
`leaf_id = 0`, meaning the relay itself, so a relay's address is its
identity (§8.1). A sender learns an address from a previous message (the
sender's address is carried in the encrypted payload), from a QR code, or
from a directory; the network provides no lookup. Sending to an area is
open item O7.

The network carries no sender address and sends no failure notices. The
only acknowledgement in the protocol is the per-hop ACK (§9.3); whether a
message arrived, and what to do if it did not, is the app's business, with
whatever acknowledgement it puts inside the encrypted payload.

### 9.2 Routing

A message travels as a DATA packet (§14) handed from relay to relay, each
hand-over acknowledged (§9.3). A relay hands the message to its neighbour
closest to the destination position. Where that leads into a gap, a relay
from which no neighbour leads on, the relays route around it by
depth-first search (DFS): a relay with no neighbour left to try returns
the message to the relay it first got it from, which tries its next
neighbour. The search is shared out among the relays: each keeps a small
record per message (§9.2.2), and the packet carries nothing for it but
the name of the relay that sent it and a hop count. DFS only routes
around gaps; it never searches for an ID.

The **origin** of a message is the relay that first sends it into the
network: the relay a leaf handed it to, or a relay sending for its own
application. The search starts and, if it finds nothing, ends there.

The rules below are roadmap D11, D16 and D17.

#### 9.2.1 The sender of a packet

A relay acts on a DATA packet whose `dst_hop` is its own short name
(§8.1) and ignores the rest. The packet names its sender by `src_hop`,
the sender's short name, and gives the sender's home channel in `aux`
(§14). Those two are all a relay needs to send something back: `dst_hop`
set to `src_hop`, on the channel in `aux`. It needs no neighbour-table
entry for the sender.

A packet with `hops = 0` comes from a leaf (§9.2.6): the receiving relay
is the origin, and the message has no parent.

Where a relay needs the sender's full position, to place it in the order
of §9.2.3, it rebuilds it from the short name and its own position
(§8.1).

#### 9.2.2 Message records

A relay keeps a **record** for every message it has received or
originated:

| Field | Content |
|---|---|
| ID | the message ID (§14) |
| parent | `src_hop` and `aux` of the packet that created the record: the short name and home channel of the relay the message first came from; none at the origin |
| last child | the neighbour it last sent the message to, as a short name; none before the first send |
| handed | the neighbours this relay has handed the message to, as one bit per neighbour-table slot |
| highest hops | the largest `hops` (§9.2.6) received for the message |
| owned | whether this relay owns the destination (§9.2.7) |
| returned | whether this relay has returned the message to its parent, or dropped it as origin |
| last active | when it last received or sent the message |

The relay's **tried set** for the message is the parent and the
neighbours in `handed`. A neighbour that was passed over without being
handed the message, because it was demoted, expired or not in the table
at the time, is not tried: if it later offers the message, it is sent
straight back (§9.2.4, case 6), not dropped. When a neighbour-table slot
changes hands, its bit is cleared in every record.

The record is also how a relay recognises a repeat of a message it has
handled (§9.2.4); there is no separate window of recent IDs.

**Expiry.** A record is dropped after 188 s without the message being
received or sent; each receipt or send restarts the clock. 188 s is how
long the longest search the hop cap allows takes when every hand-over
succeeds at the first attempt but waits the longest for the channel and
carries a full frame: 255 hand-overs (§9.2.6) × (500 ms listen-before-talk
give-up + 138.6 ms airtime of a 255-byte frame + 100 ms ACK timeout,
§9.3) = 188.3 s. So in a search without failures no record expires while
the message is away exploring beyond it. A message waiting in the relay's
send queue for the duty-cycle budget (§9.4), which can take minutes, is
still here, and keeps its record alive: without it the message could not
be routed when its turn comes.

**Bound and eviction.** A relay holds at most 626 records; when full, a
new record replaces the least recently active one. 626 is the most
messages a relay can handle in one record lifetime within its duty cycle
(§3): every message it handles costs at least an ACK (9 bytes, 11.8 ms)
and one DATA packet with no payload (21 bytes, 18.2 ms), together
30.0 ms; 10% of 188.3 s is 18.8 s of airtime, and 18.8 s / 30.0 ms = 626.
A record is 32 bytes, so 20 KB.

#### 9.2.3 Choosing the next hop

One function chooses the next hop for a first send, for the next try after
the message comes back, and after a failed hand-over (§9.3). The
**candidates** are the neighbour-table entries that are live and not
demoted (§8.2), except those in the tried set (§9.2.2).

The **order** is distance to the destination position in the local plane
(§11), nearest first; equal distances are ordered by short name. The
relay sends to the first candidate. If there is none, it returns the message to its
parent (§9.2.5).

Nothing requires the next hop to be closer to the destination than the
relay itself. After the message comes back, the best neighbour left is
usually farther away, and such a rule would turn every gap into a dead
end. On a first send the nearest neighbour is chosen anyway, so progress
toward the destination is the normal case. The neighbour the message has
just come from is never chosen: it is the parent or the last child, both
tried.

A candidate may already have the message, having got it by another path.
The relay cannot know this: its neighbours listen on other channels and
it does not overhear them. The candidate sends the message straight back
(§9.2.4, case 6), and the relay, which counts it as its last child, moves
on to the next.

#### 9.2.4 Receiving a DATA packet

A relay ACKs a DATA packet addressed to it (§9.2.1, §9.3) before acting
on it, with one exception: if acting on it means sending it on and the
relay's send queue is full (§9.3) or its airtime budget is spent (§9.4),
it neither ACKs nor changes anything, and the sender will try elsewhere.

The packet is one of the following, tested in this order:

1. **New:** the relay has no record for the message ID. It creates one,
   with the sender as parent (none if `hops = 0`). If it owns the
   destination (§9.2.7), it delivers; otherwise it chooses a next hop
   (§9.2.3) and sends.
2. **Already owned:** the record says owned. ACK only.
3. **A copy:** `hops` is no higher than the record's highest hops. This is
   a retransmission of a packet already handled, whose ACK was lost.
   ACK only.
4. **From the last child:** the message has come back. Either the child
   found no way on, or the child already had the message and sent it
   straight back (case 6). If the relay is still retrying the hand-over
   to that child, the hand-over is complete. If the relay has already
   returned the message to its parent (`returned`), its search here is
   over: it sends the packet straight back as in case 6, so the child
   marks it handed and moves on; otherwise a copy of the message would
   die here with nobody searching. Else the relay checks ownership again
   (a closer neighbour may have been demoted since) and otherwise chooses
   the next hop (§9.2.3).
5. **From the parent or an earlier child (in `handed`):** ACK only. From
   the parent, this means the parent lost its record or the message has
   split in two (§9.5); from an earlier child, that a hand-over the relay
   gave up on (§9.3) had in fact succeeded, and the child has searched and
   returned the message. Either way this relay's search has already moved
   on, and a second one would only duplicate it.
6. **From any other relay:** that relay offered the message here not
   knowing this relay has it. Send it straight back, to `src_hop` on the
   channel in `aux`; that relay will see it as a return from its last
   child. The record does not change.

In every case the relay
updates the record's highest hops, if the packet's is higher, and its
last-active time.

#### 9.2.5 Returning to the parent, and the end of the search

When §9.2.3 finds no candidate, the relay sends the message back to its
parent: a DATA packet with `dst_hop` the parent's short name, on the
parent's channel, both from the record. The parent receives it from its
last child (§9.2.4, case 4) and continues with its next neighbour.

The origin has no parent. With no candidate left it drops the message: no
relay that can be reached from it, through neighbours that answer, owns
the destination. Nobody is told (§9.1, roadmap D10).

If the hand-over to the parent fails (§9.3), the relay drops the message
and the search ends early. The usual cause is a parent that has since
moved to another home channel (§6.3).

#### 9.2.6 The hop counter

`hops` in the DATA header counts hand-overs. A leaf sends 0; a relay
sending for its own application sends 1, as if it had received the
message at 0, since 0 means "from a leaf" (§9.2.1); every relay sends
what it received plus one, whether it is forwarding, returning to its
parent or sending straight back. A retransmission repeats the count. So
along the path of a message the count only grows, and a count no higher
than one a relay has already received for the message marks a copy
(§9.2.4, case 3).

A relay that would have to send a count above 255, the largest the
one-byte field holds, drops the message. A search that keeps its records
ends on its own (§9.5); the cap ends one that has lost its way because a
record expired or was replaced, or because the message split in two.
255 is generous: the longest straight route across the city of §13
(≈30 km at the ≈750 m street range of §4) is about 40 hand-overs, leaving
about 200 for routing around gaps.

#### 9.2.7 The owner

A relay **owns** the destination `(x, y)` when:

- its own position is `(x, y)`, that is, `dst_pos`: it is the addressed
  relay; or
- it is within 50 m of `(x, y)` and no live, non-demoted neighbour is
  closer to `(x, y)` than itself. The addressed relay has then probably
  died or moved; 50 m is the distance §11 allows between two GPS fixes of
  a relay that has not moved, so a relay this close is, for the radio, in
  the same place.

A relay farther than 50 m with no closer neighbour is at a gap, not the
owner, and the search goes on (§9.2.3).

A message whose destination is the sending node's own address is
delivered there without entering the search. The destination position
alone decides who delivers: a relay that has the leaf in its leaf table
but does not own the position routes the message on like any other. A
leaf that has just moved to another relay stays in the old relay's table
until it times out (180 s, §10.2), so the table disagrees with the
address exactly when the address is new; delivering from the table would
queue the message for a leaf that has gone (roadmap D21).

The owner looks `leaf_id` up in its leaf table and delivers (§10.3), or
queues (§10.4). If `leaf_id = 0` the message is for the relay itself: the
addressed relay delivers it to its own application; an owner within 50 m
drops it, since the relay it was for is gone.

An owner that is not the addressed relay and does not know `leaf_id` may
ask its direct neighbours once whether they hold it (one hop, one round;
the packet is open item O13); if none does, it queues. Nothing wider: a
wider search is a flood.

The owner remembers delivered message IDs for the delivery TTL (§10.4),
and a later copy of the message, from any neighbour, is ACKed and not
delivered again.

### 9.3 Per-hop transfer

A **hand-over** sends one DATA packet to one neighbour until that
neighbour ACKs it or the relay gives up. A relay runs one hand-over at a
time. An ACK carries the message ID and the name of the relay or leaf
sending it, and matches the outstanding hand-over only if both agree.
Packets waiting their turn are held in the relay's send queue.

1. Retune to the next hop's home channel.
2. Listen-before-talk: CAD, then RSSI. Random backoff 0–50 ms if busy,
   retry; give up after 500 ms and choose the next hop again (§9.2.3),
   without counting a failure against the neighbour. The neighbour is set
   aside for this hand-over only, so the next choice is a different one.
   For a return to the parent, a packet sent straight back, or a DELIVER,
   which have no other candidate, a give-up counts as one attempt.
3. Transmit. The packet carries the sender's home channel in `aux` (§14).
4. Retune home immediately.
5. Wait for the ACK **on the home channel**: timeout 100 ms (ACK airtime
   11.8 ms plus the receiver's retune and listen-before-talk).
6. On timeout: wait a random time, retry, up to 3 attempts. Then the
   hand-over has failed: count a failure against the neighbour (§8.2:
   demoted after 2 consecutive failures) and choose the next hop again
   (§9.2.3). The neighbour is now the last child, so the next one in order
   is taken, or the message goes back to the parent.

**Retry wait.** The wait before the retry after the k-th unanswered
transmission is uniform in [0, 2^k × 150.4 ms]: 0–301 ms, then 0–602 ms.
150.4 ms is how long one hand-over holds its receiver, a full frame
(138.6 ms) and its ACK (11.8 ms). The usual cause of a missing ACK, other
than a weak link, is a sender the first one cannot hear that transmitted
to the same receiver at the same time (a hidden terminal): both miss the
ACK and both retry. Their retries miss each other only if they start
more than one hand-over apart, which for full frames and a window of W
units happens with probability (1 − 1/W)²: never with one unit, 1/4 with
two, 9/16 with four. Doubling per attempt also spreads more than two such
senders. The listen-before-talk backoff of step 2 stays short: it waits
for a channel the sender can hear to clear, and checks again (roadmap
D22).

An ACK resets the neighbour's failure count and refreshes its last-heard
time (§8.2): a neighbour that answers is alive whether or not its
heartbeats are heard. A failed hand-over
that was itself a return to the parent, or a packet sent straight back
(§9.2.4, case 6), is not retried elsewhere: the message is dropped.

The receiver retunes to the sender's channel, listens before talking,
sends the ACK, and retunes home. The ACK is the first byte, the message
ID and the short name of the acknowledging relay, or the leaf ID of the
acknowledging leaf (9 bytes, 11.8 ms, §14). The sender matches both
against its outstanding hand-over. The name is needed because relays
share home channels: without it, an ACK for the same message from a
different relay on the same channel, which the depth-first search can
produce, would end the wrong hand-over and strand the message. A relay
ACKs a packet as soon as it has received it, even while waiting for an
ACK of its own; if that makes it miss its own ACK, its next attempt
covers it.

So: **a node receives everything, ACKs included, on its home channel, and
leaves it only to transmit.** A leaf's home channel is its relay's, so
leaf ↔ relay traffic and its ACKs never retune. A relay is deaf for
≈15 ms per packet it forwards or ACKs; a sender that gets no ACK retries.
That is the cost of having one radio.

**Send queue.** At most 16 packets, the one being handed over included,
first in first out. When it is full, a packet that would need a place is
not ACKed (§9.2.4), so its sender retries and then routes around this
relay as it would around a failing one. 16 is what a relay can send in
the time its upstream sender spends retrying a hand-over: the two retry
waits, 301 + 602 ms, plus 2 × (100 ms ACK timeout + 500 ms
listen-before-talk give-up + 138.6 ms full frame), 2.38 s in all, and a
full frame and its ACK take 138.6 + 11.8 = 150.4 ms, so
⌈2.38 s / 150.4 ms⌉ = 16. A packet that would wait longer than that would
get on sooner by the upstream relay choosing another next hop.

### 9.4 Duty cycle cap

At SF5, 10% of airtime is ≈180 B/s per relay, so a busy relay runs out of
budget before its channel fills. Greedy forwarding does not spread load;
central rooftop relays will hit the cap first. When the budget is
exhausted the relay stops forwarding and drops what it cannot send, but
keeps sending heartbeats: it holds back the airtime they need (11.8 ms
every 10 s on average, 0.12%, §7). Without budget it cannot ACK either,
so its neighbours' hand-overs to it fail and they route around it.

### 9.5 Why the search finds a path

Suppose that while a message is being routed the links do not change,
every hand-over to a neighbour that answers succeeds within its attempts,
and no record expires or is replaced. Then:

- The message moves as a single packet: a relay sends only what it has
  just received or originated, and every ACK-only case of §9.2.4 sends
  nothing.
- A relay holding the message either owns the destination, sends to its
  next candidate, or returns it to its parent. Candidates advance in a
  fixed order and never repeat, so each relay sends to each neighbour at
  most once, and the search ends.
- A send to a relay that already has the message comes straight back
  without changing that relay's record (§9.2.4, case 6), as if the
  neighbour had been a dead end.

So the message makes a depth-first traversal of the relays reachable from
the origin through neighbours that answer, nearest to the destination
first. A depth-first traversal visits every relay it can reach; it stops
at the first owner, or returns to the origin when there is none. If a path
to an owner exists, the message finds it, provided the traversal fits in
the 255 hand-overs of §9.2.6.

What breaks it:

- **A lost record.** A message returning to a relay whose record has
  expired or been replaced is new there; the relay searches again from
  itself, and the relays above it never get the message back. The search
  ends early, or wanders until the hop cap ends it.
- **A split.** A hand-over whose ACKs were all lost looks failed, so the
  sender moves on while the receiver also carries the message: two copies
  search at once. A copy that reaches a relay holding a record for the
  message is mostly absorbed there (§9.2.4, cases 2, 3 and 5); otherwise each
  ends by its own search or by the hop cap.
- **Links that change** during the search, including a parent that moves
  to another home channel (§9.2.5): parts of the graph go unsearched.

### 9.6 Bounds

- **Hand-overs per message:** at most 256 that succeed (the first, at
  count 0, and 255 more, §9.2.6). A relay makes at most 3 attempts per
  neighbour it tries, and tries each neighbour at most once per record.
- **Cost of a search:** each link between two relays costs at most two
  hand-overs from each end: a send and the return or the send back. In
  the usual case it is two in all. Within the hop cap a search can
  explore about 64 to 128 links beyond the route that led into the gap.
- **State per relay:** at most 626 records of 32 bytes, 20 KB
  (§9.2.2); a send queue of 16 packets of up to 255 bytes, 4.1 KB (§9.3);
  the owner's delivered IDs, kept with the delivery queue (§10.4).

---

## 10. Leaves

A leaf is a carried node on battery. It **never sends heartbeats** (at
household density that would be hundreds of percent of airtime on every
channel) and never forwards.

### 10.1 Relay choice

Listening on any one channel, a leaf hears every relay in range once per
rotation (≈130 s, at most 13 × 15 s = 195 s). It keeps, for its relay and
the four strongest others it hears, the mean RSSI of the last two
heartbeats. Four covers the relays an indoor leaf reaches at the design
density: π × (300 m)² × 9.6 relays/km² ≈ 2.7 (§4, §13).

**First choice.** A leaf without a relay attaches to the strongest it has
heard once it has listened for a worst-case rotation, 195 s, from the
first heartbeat it heard; by then every relay in range has visited its
channel. A leaf with a message to send does not wait: it attaches to the
strongest heard so far.

**Switching.** An attached leaf moves to another relay only when that
relay has been heard over at least two heartbeats and its mean is at
least 8 dB above its own relay's. The margin is two standard deviations
of the difference of two such means under fading of σ = 4 dB (the
scenarios' figure): a relay's heartbeats reach the leaf's channel about
130 s apart, longer than the 60 s the fading lasts, so they are nearly
independent, a mean of two has σ/√2, and the difference of two means
σ√(1/2 + 1/2) = 4 dB. A relay that is no better takes the leaf on 2.3 %
of comparisons. The leaf attaches to the new relay before leaving the old
one. Vehicles cannot keep up and rely on the delivery queue plus
re-attaching on arrival.

A leaf that hears no relay for 3 rotations stays on any channel and
waits; every relay in range will appear within one rotation.

### 10.2 Attach and keepalive

`ATTACH {type, leaf_id, relay, flags}` (`relay` is the relay's short
name, §8.1) is sent to the relay on its
home channel with listen-before-talk and ACK. The flags carry the leaf's
receive mode (§10.5). Repeat every 60 s as a keepalive, and immediately on
changing relay. The relay's **leaf table** (`leaf_id, last_seen, rssi,
rx_mode`) is separate from the neighbour table; entries expire after 3
missed keepalives (180 s). Leaf table size: 256 entries per relay (≈80
leaves per relay expected at indoor density in Kyiv with 10% adoption; 400
at 50%).

Keepalive load per channel at 80 leaves per relay: ≈2%; at 400: ≈12%.
That is why the interval is 60 s and not 10 s.

**Keeping the relay.** A hand-over to the relay that fails, keepalive or
message (§9.3: three attempts), is tried again one keepalive period
later, ATTACH first. The leaf gives the relay up only when hand-overs
have failed in two periods in a row, or when it has heard neither a
heartbeat nor an ACK from it for as long as a neighbour entry lasts
(§8.2: 390 s), whichever comes first. A fade lasts about a minute (the
scenarios' correlation time), as long as one period, so one fade costs a
keepalive, not the relay: the three attempts of one hand-over, a few
hundred milliseconds apart, all fall inside it. Having given up its
relay, the leaf takes the strongest other one it has heard (§10.1).

**A message without a relay.** A message the leaf cannot hand over,
because it has no relay or its relay does not answer, is held and tried
again as above, for up to the delivery TTL (10 minutes, §10.4), the time
the network itself would keep it. Only then is it reported failed. The
leaf holds one message at a time (roadmap D23).

### 10.3 Delivery

The owning relay sends the message to the leaf on the relay's own home
channel (the leaf listens there), listen-before-talk first, and waits for
the leaf's ACK. Preamble length per the leaf's receive mode (§10.5).

### 10.4 Delivery queue

Bounded, not a mailbox: up to 64 messages per relay, kept for 10 minutes
(the delivery TTL). Retry delivery on the leaf's next keepalive and every
60 s. On expiry the message is dropped. The owner remembers the last 64
delivered message IDs (§14) for the TTL and ACKs a repeat without
delivering it twice; 64 is 400 leaves (§10.2) × 23 texts a day (§13) over
the 144 ten-minute TTLs in a day. A
queued message costs no airtime; a failed delivery costs a retry across
the city; the common case is a phone in a pocket, a basement, or the metro
for a few minutes. RAM is not the constraint (64 × 255 B = 16 KB of 256 KB).

The queue plus the sender's own retry covers a leaf that has moved to
another relay; the old relay does not forward to the new one. Neighbouring
relays cannot overhear deliveries, because they are on different channels.

### 10.5 Leaf receive mode

| Mode | Leaf current | 850 mAh | Preamble on packets to the leaf | Hears heartbeats |
|---|---|---|---|---|
| Always listening | ≈5.8 mA | ≈6 days | 12 symbols (§4) | yes |
| Duty-cycled (sleep, CAD every 500 ms) | ≈0.3 mA + sleep | months | ≥512 ms | no: blind to relay changes |
| Hybrid: full listen one rotation every 10 min, CAD between | ≈1.3 mA | ≈25 days | ≥512 ms | during the awake rotation |

The leaf declares its mode in ATTACH; the relay sets the preamble to
match. Long preambles cost little airtime because deliveries are rare
(≈0.5% of a channel per day at 80 leaves × 10 messages). Default: always
listening while on USB or charging, hybrid on battery. GPS off after a
fix, and off after 10 minutes without a fix (searching under jamming draws
≈27 mA, ≈1.3 days).

---

## 11. Position, time, phone link

**Position.** GPS (L76K) is used for position only and is not trusted
blindly: accept a fix only if plausible (within 50 m of the last accepted
position for a stationary relay, consistent with neighbours, satellite
signal-to-noise C/N0 sane). Rooftop and windowsill relays use surveyed or
user-entered coordinates from the app, which override GPS. Positions on
the wire are WGS-84 latitude and longitude as fixed point, 25 + 26 bits in
7 bytes, ≈0.6 m. Routing needs distances, not coordinates: each node
converts every position it handles into metres on a flat plane centred on
itself, `Δy = Δlat · R` and `Δx = Δlon · R · cos(lat_self)`, with
`cos(lat_self)` a per-node constant. The error grows with
Δlat · tan(lat): at 50° N it is 0.1% at 5 km and 0.4% at 20 km, enough to
reorder only near-ties between candidate hops. Neighbour-table entries
store both the wire form (identity) and the local form (distance). Every
relay knows its own position; nothing is estimated cooperatively.

**Time.** No network time. GPS time is never used. All timers are local
monotonic. Nothing in the protocol depends on two nodes agreeing on a
clock.

**Phone link.** BLE 5.0 GATT service on the nRF52840 (TrouBLE, roadmap
D5): characteristics for send, receive-notify, node status, position set,
role set. The app owns identity and end-to-end encryption; the node sees
opaque payloads. The sender's address `(x, y, leaf_id)` travels inside the
encrypted payload so that only the recipient learns where the sender is.
Encryption scheme: open item O1.

A message has no ID field: the network identifies it by a hash of its
destination and payload (§14). Two messages to the same destination with
identical payloads are therefore one message to every relay that still
remembers the first (up to the 10-minute delivery TTL, §10.4), and the
second is not delivered. The app must make every payload unique;
end-to-end encryption with a fresh nonce per message does that.

**What the network does not defend against.** Nothing on the air is
authenticated. Anyone with a 433 MHz LoRa radio can send heartbeats with
any position, attract traffic to it and drop it, send packets in another
relay's name, or replay what it has heard. The app's end-to-end
encryption keeps payloads private and authentic; it does nothing for
routing. This is the scope stated at the top: infrastructure loss, not an
adversary on the air.

---

## 12. What the protocol deliberately omits

Common mesh mechanisms left out, and why.

| Omitted | Instead | Why |
|---|---|---|
| Neighbour lists in heartbeats and a network-wide node table | 8-byte position-only heartbeat; routing uses only direct neighbours | spreading tables through the network does not scale to thousands of relays at LoRa rates; addresses are positions, so there is nothing to look up |
| Sequence numbers in heartbeats | none | they exist to age shared tables; without those they carry nothing |
| A node ID on every node | relays are identified by position; only leaves carry a 4-byte ID, generated by the app | relays do not move, and an ID would duplicate the address |
| Time slots on a shared schedule | always listen, transmit as soon as listen-before-talk clears | no time sync exists, and slots add latency |
| Cooperative position estimation | every relay knows its position from GPS or survey | §11 |
| Heartbeats on one fixed channel | rotating channel, random 5–15 s interval | §7 |
| Addressing by ID with a lookup | position + leaf ID; the owning relay resolves the ID locally | §9 |

---

## 13. Capacity and sizing (rationale, not spec)

Kyiv model: ≈840 km², ≈400 km² built-up, ≈3 M people. Indoor coverage
needs ≈9.6 relays/km² (≈3,800 relays); outdoor-only ≈1.9/km² (≈750).
Rooftop relays hear ≈3 km, so a collision domain, the area within which
transmissions interfere, is ≈28 km² with ≈270 relays, ≈14 domains across
the built-up city.

One 125 kHz channel at SF5 carries 1.8 kB/s raw and ≈640 B/s with
listen-before-talk (≈35% usable). A 100-byte text over 8 hops with ACKs
takes ≈0.65 s of channel time: per hop a 121-byte DATA packet (69.4 ms)
and its ACK (11.8 ms).

| Plan | Heartbeat load per channel | City-wide data | Texts/s | Texts per user per day at 300k users |
|---|---|---|---|---|
| Single channel, 60 s heartbeats | 8% | 7 kB/s | 7 | 2 |
| 7-colour fixed grid, heartbeats on all colours | 11–14% | 37 kB/s | 37 | 11 |
| **13 channels, measured selection, rotating heartbeat (this design)** | **≈2.5%** | **≈85 kB/s** | **≈80** | **≈23** |
| + per-packet power control (later) | | ≈160 kB/s | ≈150 | ≈45 |

These are orders of magnitude; the hearing radius and hop count are each
uncertain by 2×. The city centre is the hot spot, and the per-relay 10%
cap (§9.4) binds there first.

Common proposals that do not fit:

- **GFSK at 50 kbps:** 3× more per channel than SF5, at 40% of the range,
  and needs a modem driver and error correction written from scratch.
  Revisit only if relays saturate at SF5; nothing above the modem would
  change.
- **Spreading one signal over the whole band (DSSS/CDMA):** the SX1262 has
  no I/Q path; a correlator board costs an order of magnitude more and
  draws 100+ mA; at 1.74 MHz and 15 kbps the processing gain is 20 dB, so
  a transmitter 300 m away drowns a wanted one 1 km away. Weber–Andrews:
  in random meshes without power control, M channels beat M-chip spreading
  by M^(1−2/α), 5–10× here. Surviving that distance ratio would need
  ≈37 dB of processing gain, which this band cannot give.
- **Frequency hopping (FHSS):** hopping on a common schedule has the
  capacity of one channel; per-receiver schedules equal fixed channels
  plus time sync; random channels per packet with scanning receivers cost
  ≥20 ms preambles on every packet. No regulatory benefit at 433 MHz
  (the duty cycle applies to any modulation).
- **A fixed hexagonal channel grid with heartbeats on all channels:** 49%
  of every channel at indoor density. Where a rooftop relay hears ~3 km
  and a relay covers ~700 m, a grid only spreads load evenly, which
  measurement does as well while adapting to hot spots and foreign
  interference.
- **Home-relay registration (Mobile IP style):** registration traffic
  grows with users; position addressing needs none.
- **Leaf heartbeats every 10 s:** collisions happen at the shared relay
  whatever the leaf's range; 72% of a channel at 400 leaves per relay.

---

## 14. Packet formats (proposal; finalised in O2)

The first byte of every packet is `type:4 | aux:4`; `aux` is the home
channel in heartbeats and type-specific flags elsewhere. Positions are
7 bytes: 25-bit latitude, 26-bit longitude, 5 spare bits (§7). A relay's
short name is 4 bytes cut from its position (§8.1). The LoRa hardware
CRC covers everything.

```
HEARTBEAT 0x1/0x2: first(1) pos(7)                                          = 8
          0x1 surveyed position, 0x2 GPS-derived position; aux = home channel
ACK       0x3: first(1) id(4) from(4)                                       = 9
          sent on the channel in the acknowledged packet's aux; aux unused
          from = the acknowledging relay's short name, or the leaf's ID
ATTACH    0x4: first(1) leaf_id(4) relay(4)                                 = 9
          relay = short name of the relay attached to; aux = leaf receive mode (§10.5)
          ACKed with id = leaf_id read as a 32-bit number, from = the relay's short name
DATA      0x5: first(1) dst_hop(4) src_hop(4) dst_pos(7) dst_leaf(4) hops(1)
               payload                                                      = 21 + payload
          dst_hop, src_hop = short names of the receiving and the sending relay;
          aux = sender's home channel, so the receiver knows where to ACK
DELIVER   0x6: first(1) dst_leaf(4) id(4) payload                           = 9 + payload
          relay → leaf on the relay's own channel; leaf ACKs on the same channel
Type 0x7 is unassigned. Type 0xF is reserved for the bench frames of the
firmware's measurement console (`bark-fw/README.md`); nodes reject it like any
unknown type.
```

The **message ID** is a 32-bit FNV-1a hash of
`dst_pos || dst_leaf || payload`, computed
by every node that handles the message; it is the same on every hop
because none of these fields changes. DATA does not carry it; ACK and
DELIVER do, ACK to name what it acknowledges and DELIVER so the leaf can
ACK it. The app must keep payloads unique (§11).

`dst_hop` lets every node on a channel discard packets not addressed to it
after 5 bytes. `src_hop` names the relay that sent the packet, and `aux`
its home channel, which is where the ACK goes; routing uses them to know
where a message came from (§9.2). A leaf sending its own message sets
`src_hop` to 0 and `hops` to 0. The payload is at most
255 − 21 = 234 bytes. The sender's address is inside the encrypted
payload, not in the header.

---

## 15. Implementation notes

**Rust stack:** `embassy-nrf` (nRF52840), `lora-phy` from git (SF5, CAD,
RSSI; roadmap D4), TrouBLE with the Nordic controller for BLE (D5), the
stock UF2 bootloader for flashing (D6). The protocol core does no I/O of
its own and owns its routing and tables (D1, D3; `core-interface.md`).
Crate layout is D7.

**RAM budget (256 KB):** neighbour table 64 × 40 B = 2.5 KB; leaf table
256 × 16 B = 4 KB; delivery queue 64 × 256 B = 16 KB; message records
(§9.2.2) 626 × 32 B = 20 KB; send queue 16 × 262 B = 4.1 KB; delivered
window 1 KB; busy estimates and duty budget under 1 KB; 49 KB in all,
measured. BLE controller ≈5 KB; Embassy and stacks ≈16 KB.

**Bring-up order** (each step is a milestone on real boards):

1. `lora-phy` at SF5/125 kHz on two T-Echo 433: transmit, receive, CAD,
   RSSI read, retune time measured. **Bench item C1** (adjacent-channel
   rejection) here.
2. Listen-before-talk + rotating heartbeat + neighbour table on three
   boards: each board lists the other two with channel and position within
   one rotation.
3. Busy estimate + home-channel selection: put a continuous-wave
   interferer on one channel, watch boards avoid it; check that three
   boards that all hear each other do not keep switching.
4. Greedy forwarding + per-hop ACK across three relays in a line, with
   attenuators or distance so A cannot hear C. Demotion when the middle
   relay is powered off.
5. Leaf attach, keepalive, delivery, queue. **This is the loop every
   user feels; prototype it before anything clever.**
6. BLE GATT + minimal phone app.
7. Field test: a dozen relays on windowsills across a few blocks.

**Test rig:** at least three T-Echo 433, SMA attenuators, one interferer
source (a fourth board is fine), a USB logic/UART sniffer for timing.

---

## 16. Open items

| # | Item | Owner/when |
|---|---|---|
| O1 | End-to-end encryption scheme in the app (key exchange, address-in-payload format) | before step 6 |
| O2 | Finalise packet formats (§14) | step 4 |
| O3 | Exact thresholds: busy threshold margin (6 dB), move hysteresis (0.05 / 10 min), demote count, ACK timeout | tune in steps 3–4 |
| O4 | Neighbour-table eviction for rooftop relays that hear hundreds: whether a set spread around the relay beats D12's rule (§8.2) | simulator |
| O5 | Bench item C1: adjacent-channel rejection at 125 kHz spacing; fallback 11 × 150 kHz | step 1 |
| O7 | Sending to an area (address = position + radius, leaf_id = broadcast) | later |
| O8 | Per-packet transmit power control (prefer short hops, cut rooftop hearing radius): ≈2× capacity | later |
| O9 | Repository/crate layout | resolved: roadmap D7 |
| O11 | Relay-side RSSI noise-floor calibration method and drift | step 3 |
| O12 | Confirm RAK WisMesh Repeater 433 availability for rooftop solar units | procurement |
| O13 | Packet for the owner's one-round question to its neighbours: do you hold this `leaf_id`? (§9.2.7) | step 5 |
| O14 | Hidden terminals: the 0–50 ms retry backoff was shorter than a full frame (139 ms), so two senders that cannot hear each other and collided once collided again on every retry | resolved: roadmap D22, §9.3 retry wait |

---

## 17. Sources

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

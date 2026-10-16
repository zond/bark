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
| Roles | **Relay**: a fixed node on mains, USB or solar that sends heartbeats, forwards, and holds messages for its leaves. **Leaf**: a carried node on battery that attaches to one relay, answers its polls, and otherwise stays silent |
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

Implement: an airtime token bucket per node that keeps every hour within
10% (§9.4); no single transmission longer than 1 s (SF5 packets are
≤139 ms anyway).
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
- **Home channel:** count only what would stay behind if the node moved.
  Packets addressed to the node (DATA, ATTACH), the ACKs of its own
  hand-overs, and its own transmissions and ACKs go wherever it goes:
  counting them would make a busy relay leave its channel for its own
  traffic and take that traffic along. So a home reading is 1 if RSSI is
  above the threshold and no packet addressed to the node was on the air
  when it was taken; otherwise it is the fraction of the time since the
  last home reading spent receiving packets addressed to others
  (heartbeats included), from the packet lengths. Whether a reading fell
  in the node's own packet is known only when that packet ends, so a home
  reading is held until the next, one second later, longer than any
  packet (roadmap D24). A home reading may fall inside a reception, since
  the radio keeps receiving while it reads its own channel
  (`core-interface.md` rules 7 and 8): a reading inside a packet addressed
  to the node is discarded, one inside any other packet counts. A packet
  for the node that fails its CRC cannot be told from someone else's and
  counts.
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
| spare | 5 | zero. A relay reads them as the sender's transmit power, a research prototype that is off (`power-research-2026-10-15.md`) |

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
| channel | the channel to transmit to this neighbour on: from its heartbeat, or from `aux` of a DATA packet it sent (§9.2.1), whichever came last |
| last_heard | local monotonic time |
| rssi_ema | averaged RSSI of every frame heard from it: heartbeats, its DATA and its ACKs; each new frame counts half. Says whether it has margin |
| fail_count | consecutive ACK failures |
| banned_until | when the ban of its last refusal ends (§9.3) |

- **Expire** after 3 rotations (390 s) without a heartbeat or an ACK
  from it (§9.3).
- **Margin.** A neighbour has margin when `rssi_ema` is at least 4.1 dB
  above the noise floor (§6.1). A 255-byte frame is then still lost no
  more than one time in a hundred, the standard the datasheet sets for
  sensitivity, after a fade of 4 dB. A neighbour without margin is a
  next hop only when no other is left (§9.2.3). It stays in the table:
  its heartbeats show it is alive. The 4.1 dB is worked out from the
  radio:
  - A packet arrives only if every one of its symbols does. A 255-byte
    frame has 541 symbols, the datasheet's 64-byte reference packet 156,
    a heartbeat 46. With the symbol error rate of the modulation (32
    possible symbols, detected without phase), a 255-byte frame needs
    0.44 dB more signal than the reference packet for the same 1 % loss.
    A heartbeat needs 0.46 dB less, and half of all heartbeats still
    arrive 3.2 dB below the full frame's level. So a heartbeat says
    little about a full frame.
  - Sensitivity is 2.5 dB below the noise at SF5 (datasheet Table 6-1).
    The full frame's level is therefore 2.5 − 0.44 = 2.06 dB below the
    noise floor.
  - The fade is 4 dB, the standard deviation of fading over time and
    channel in the simulated links. A frame goes out on the neighbour's
    channel, and this relay hears the neighbour on its own, where the
    fading is different, so the average must cover a fade it cannot see.
    No one has measured this on our hardware yet; it is to be replaced by
    Phase 2 measurements.
  - So the signal must average −2.06 + 4 = 1.94 dB above the noise
    floor. The radio's packet RSSI is the power in the channel, signal
    and noise together (datasheet §13.5.3), so it reads
    10 log10(1 + 10^0.194) = 4.1 dB above the floor.
- **Demote** on ACK failure: `fail_count += 1`; a neighbour with
  `fail_count ≥ 2` is a next hop only when no other with margin is left
  (§9.2.3) until it ACKs a packet from this relay or three rotations (390 s)
  pass. Its heartbeats keep it in
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

The app hands a node a message only when the node says it will take one,
and hears nothing about it afterwards: not the first hop's ACK, which a
user could take for delivery, and not a drop. From then on the node is
either sending the message or waiting to. Each node holds one message
of its app's at a time, until its first hop takes it. A relay takes one
while it is running, holds none, and its send queue and bucket have room
for a full frame on top of what they hold (§9.3); a leaf takes one while
it holds none (§10.2). A node whose neighbours are busy keeps the
message and offers it again (§9.2.5, §10.2), so a busy network makes its
users wait rather than lose their messages at the first step (roadmap
D29).

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
entry for the sender; if it has one, `aux` becomes its channel there,
as a heartbeat's would (§8.2).

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
| parent | `src_hop` and `aux` of the packet that created the record: the short name and home channel of the relay the message first came from; none at the origin. The channel is used only if that relay is no longer in the neighbour table (§9.2.5) |
| handed | the neighbours that took the message from this relay, each by ACKing a hand-over of it (§9.3), as one bit per neighbour-table slot |
| last packet | `src_hop` and `hops` of the last DATA of the message it received (0 and 0 from a leaf): the same pair again is a retransmission (§9.2.4) |
| highest hops | the largest `hops` (§9.2.6) received for the message, or sent in a hand-over of it that succeeded |
| arrived | whether the message has reached its owner: this relay owns the destination (§9.2.7), or the relay it is addressed to took it from this one |
| last active | when it last received or sent the message |

The relay's **tried set** for the message is the parent and the
neighbours in `handed`: those known to have the message. A neighbour that
has not ACKed it is not tried, whether it was passed over, because it was
demoted, expired or not in the table at the time, or was offered the
message and refused it or never answered (§9.3): it stays a candidate.
When a neighbour-table slot changes hands, its bit is cleared in every
record.

The record is also how a relay recognises a repeat of a message it has
handled (§9.2.4); there is no separate window of recent IDs.

**Eviction.** A record has no lifetime. It is kept until the table
needs its place; a new record then takes the place of the least recently
active one among those whose message the relay does not hold. A relay
holds a message while it is in its send queue (§9.3), the hand-over
included, or set aside for a full delivery queue (§9.2.5). Without its
record the message could not be routed when its turn comes, so a message
is never lost for want of room (roadmap D31).

**Bound.** A relay holds at most 1695 records. A message sent on can come
back, returned by a child or offered by a split copy, and its record must
still be here then. On its way it may wait at each relay as long as a
full send queue takes to drain, each of the 294 places waiting out one
ban of 1.56 s (§9.3): 457.3 s. Every message a relay handles costs at
least an ACK (13 bytes, 14.4 ms) and one DATA packet with no payload (21
bytes, 18.2 ms), together 32.6 ms; 10% of 457.3 s is 45.7 s of airtime,
so a relay handles at most 1401 messages in that time (§3). The records
of held messages are never evicted and may be older, so the table has
room for the 294 a send queue holds on top: 1695. A record is 32 bytes,
so 54 KB.

#### 9.2.3 Choosing the next hop

One function chooses the next hop for a first send, for the next try after
the message comes back, and after a failed hand-over (§9.3). The
**candidates** are the neighbour-table entries that are live (§8.2),
except those in the tried set (§9.2.2), those banned (§9.3), and those
set aside for the current hand-over because they did not take it (§9.3
steps 2 and 6, refusal).

The **order** is: neighbours with margin (§8.2) before those without;
within each, neighbours that are not demoted before demoted ones; within
each, distance to the destination position in the local plane (§11),
nearest first; equal distances are ordered by short name. The
relay sends to the first candidate. If there is none but neighbours that
are only banned, the message waits until the earliest ban ends and is
routed again then (§9.3). If there is none at all, the relay returns the
message to its parent (§9.2.5).

A neighbour without margin is tried only after every neighbour with
margin, demoted ones included. Fading lets a relay hear the 8-byte
heartbeat of a neighbour whose link usually cannot carry a full DATA
frame. Such a neighbour is often the farthest, so the nearest to the
destination, and without this rule every message would try it first and
send three DATA frames that are not answered. It stays a last resort, and
is not left out, because on a sparse line it may be the only way on, and
a weak link still carries some frames. When it does take one, its ACK
is heard and counts toward its margin (roadmap D38).

Among neighbours with margin, a demoted one is thus a last resort: tried
only when every other has been, its failures counted as usual. Demotion keeps a failing link
from costing every message its attempts while another route exists; it
should not drop a message whose only route is that link. Two hand-overs
lost to collisions demote a neighbour that works, and on a line it is
the only route, so without the last resort every message is dropped
until the demotion lapses, six and a half minutes. A neighbour that is
really gone costs a message three attempts each time the message is
routed past it, until its entry expires (§8.2: 390 s after its last
heartbeat) (roadmap D25, D36). A
banned neighbour is not tried at all: it has just said it cannot take
the message, and trying it again before its bucket has refilled would
only cost it another reply (roadmap D29).

Nothing requires the next hop to be closer to the destination than the
relay itself. After the message comes back, the best neighbour left is
usually farther away, and such a rule would turn every gap into a dead
end. On a first send the nearest neighbour is chosen anyway, so progress
toward the destination is the normal case.

A candidate may already have the message, having got it by another path.
The relay cannot know this: its neighbours listen on other channels and
it does not overhear them. The candidate takes it all the same (§9.2.4).
If the message has arrived there, or the candidate holds another copy,
it goes no further; otherwise the candidate goes on with its own search,
and in time offers the message back or returns it, and the relay takes
that as its return.

#### 9.2.4 Receiving a DATA packet

A relay answers a DATA packet addressed to it (§9.2.1, §9.3) before
acting on it, with an ACK that says whether it takes the packet or
refuses it (§14). It answers only when it owes no other reply (§9.3).
It takes a packet that would give it something to send on into the
regular room of its send queue only if the room has a place for it and
its airtime bucket (§9.4) holds, as it stands, the ACK, the airtime of
every frame in the send queue and of the new one, and the reply reserve.
Failing that, it takes the packet into a free hop place numbered no
higher than the packet's `hops`, whatever the bucket holds (§9.3). A
packet from a leaf, and its own application's message, also need a
neighbour the relay could hand it to now: a live one that is not
banned (§9.2.3, §9.3), demoted or not, since a message entering here has
tried none. A relay whose neighbours are all banned would only hold the
message until a ban ends, and its offer could be refused again; refused
here, the message waits at its origin, where the application sees it. A relay with no live neighbour at all
takes it, and the search ends as in §9.2.5. So a relay takes from its
leaves and its own application no more than it can start sending now,
and refuses a message already in the network only when the hop places
it may use are taken too, whatever its bans: §9.3 needs every such
message to move on (roadmap D34). It takes a packet to deliver only
if its bucket holds the reply reserve and an ACK and, for one of its
leaves, its delivery queue has room (§10.4). Otherwise it refuses: it
sends an ACK that says refused, on the channel in `aux` after listening
before talking, changes nothing, and the sender tries elsewhere at once
or waits (§9.3). A packet that gives it no work, cases 1, 3 and 4 below,
is taken even then: a refusal would send its sender on with a second
copy. A relay
that cannot afford even the reply stays silent, and its sender retries
as for a lost ACK (roadmap D27, D29).

The packet is one of the following, tested in this order:

1. **Arrived:** the relay has delivered the message or holds it for a
   leaf (§10.4), or its record says the message has arrived (§9.2.2).
   ACK only.
2. **New:** the relay has no record for the message. It creates one, with
   the sender as parent (none if `hops = 0`). If it owns the destination
   (§9.2.7), it delivers; otherwise it chooses a next hop (§9.2.3) and
   sends.
3. **A retransmission:** the sender and the count are those of the last
   packet of the message the relay received. The sender's ACK was lost
   and it sent the same packet again. ACK only.
4. **A copy while the relay holds one:** the message waits in its send
   queue or is set aside (§9.2.5). ACK only: the two copies merge, and the
   relay's goes on. A hand-over under way whose frame has gone out does
   not count: its receiver may have taken it with only the ACK lost, and
   this packet may be that copy come back.
5. **A copy, and the relay holds none:** the relay takes it and goes on
   with its own search from its record. It checks ownership again (a
   closer neighbour may have been demoted since), then sends to its next
   candidate (§9.2.3), or returns the message to its parent (§9.2.5), or,
   at the origin, drops it. From a neighbour it handed the message to,
   this is that neighbour's return. From the neighbour it is still
   handing the message to, it also ends that hand-over: the neighbour has
   the message.

In every case the relay records the packet's sender and count as the
last packet, raises its highest count if the packet's is higher, and
updates its last-active time.

Only cases 1, 3 and 4 end a packet without acting on it, and in each the
message lives on: it has arrived, the packet was acted on when it first
came, or another copy goes on. So no case ends the last copy of a
message. Nothing is sent straight back: a relay that has had the message
goes on from where its own search stands, and its sender, if the relay
has not tried it, stays a candidate and gets the message back in turn.
A return goes from a relay to its parent, and the parents, each set when
its record was made, lead back to the origin without a loop, so a
message cannot go back and forth between two relays that hold none of
it (roadmap D35).

#### 9.2.5 Returning to the parent, and the end of the search

When §9.2.3 finds no candidate, because every neighbour has been tried
or set aside, the relay sends the message back to its parent: a
DATA packet with `dst_hop` the parent's short name, from the record, on
the parent's home channel as the neighbour table has it at the moment of
sending, so a return follows a parent that has moved home (§6.3) since
it handed the message over. Only a parent no longer in the table is sent
to on the channel in the record. The parent takes it as a return
(§9.2.4, case 5) and continues with its next neighbour.

The origin has no parent. With no candidate left it drops the message:
no relay that can be reached from it through neighbours that answer
owns the destination. Nobody is told (§9.1, roadmap D10), the origin's
own application included: that is the cost of having no end-to-end
acknowledgement.

A message whose neighbours left are only banned is neither returned nor
dropped: it waits (§9.3), at the origin or anywhere else. The relay's
own application's message waits the same way, longer after refusals in
a row (§9.3), as a leaf's does (§10.2), and one for one of the relay's
own leaves that finds the delivery queue full is kept aside until the
queue has a place. Until a first hop takes it, the application is given
no other message (§9.1), nor one while every neighbour is banned
(§9.2.4).

If the hand-over to the parent fails (§9.3), the relay drops the message
and the search ends early. A parent that refuses the return has not
failed: the return waits for the ban and goes again (§9.3).

#### 9.2.6 The hop counter

`hops` in the DATA header counts hand-overs. A leaf sends 0; a relay
sending for its own application sends 1, as if it had received the
message at 0, since 0 means "from a leaf" (§9.2.1). A relay sends its
record's highest count plus one (§9.2.2), whether it is forwarding,
or returning to its parent. The count is set when a hand-over's first
frame goes out, and its retries repeat it (§9.3). The highest count is
the largest the relay has received for the message or sent in a
hand-over that succeeded. So along the path of a message the count only
grows, and a relay sends a count for a message once, but in retries: to
send another packet of it, it must have received one since its last
hand-over that succeeded, or that hand-over failed and the packet goes
elsewhere. After a wait for a ban it may also go again to the neighbour
that did not answer; if that neighbour took the first, the second is in
fact a retransmission to it. A receiver therefore tells a retransmission
by the sender and the count (§9.2.4, case 3). A frame that waited in the send queue, or
goes to another neighbour after a failed hand-over, carries the count as
it stands when it goes. The count also numbers the hop places of the
send queue (§9.3).

A relay that would have to send a count above 255, the largest the
one-byte field holds, drops the message. A search that keeps its records
ends on its own (§9.5); the cap ends one that has lost its way because a
record was replaced, or because the message split in two, and it ends
the chain of hop places (§9.3).
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

The owner remembers the message IDs it has delivered (§10.4), and a
later copy of the message, from any neighbour, is ACKed and not
delivered again.

### 9.3 Per-hop transfer

A **hand-over** sends one DATA packet to one neighbour until that
neighbour answers it or the relay gives up. A relay runs one hand-over
at a time. The answer is an ACK, which says whether the neighbour takes
the packet or refuses it. It carries the message ID, the name of the
relay or leaf that answers and the name of the one it answers, and
answers the outstanding hand-over only if all three agree. Packets
waiting their turn are held in the relay's send queue.

1. Retune to the next hop's home channel. Addresses and channels are
   read at the moment of sending, before each attempt, from the latest
   heartbeats (and DATA `aux`, §8.2): a retry goes to a neighbour's new
   home if it has moved since the last attempt, and a DELIVER takes the
   leaf's receive mode as the leaf table has it then (§10.5).
2. Listen-before-talk: CAD, then RSSI. Random backoff 0–50 ms if busy,
   retry; give up after 500 ms and choose the next hop again (§9.2.3),
   without counting a failure against the neighbour. The neighbour is set
   aside for this hand-over only, so the next choice is a different one.
   For a return to the parent or a DELIVER, which have no other
   candidate, a give-up counts as one attempt.
3. Transmit. The packet carries the sender's home channel in `aux` (§14).
4. Retune home immediately.
5. Wait for the ACK **on the home channel**: timeout 100 ms (ACK airtime
   14.4 ms plus the receiver's retune and listen-before-talk).
6. On timeout: wait a random time, retry, up to 3 attempts. Then the
   hand-over has failed: count a failure against the neighbour (§8.2:
   demoted after 2 consecutive failures) and choose the next hop again
   (§9.2.3). The neighbour is set aside for this hand-over, as in step
   2, but not counted as tried: in the simulated loaded corridor an
   attempt was lost to a collision at the receiver about four times as
   often as by its ACK going astray (roadmap D34), so the neighbour most
   likely does not have the message. It stays a candidate when the
   message is next routed here. If it did take it, the next offer costs
   one round trip: it holds the message and only ACKs, or has finished
   with it and returns it (§9.2.4) (roadmap D36).

**Refusal.** An ACK that answers the outstanding hand-over and says
refused means the receiver does not take the packet (§9.2.4). The
sender treats it as a listen-before-talk give-up (step 2): the
neighbour is set aside for this hand-over, not counted as tried, no
failure is counted, and the next hop is chosen at once. A return to the
parent has no other candidate: it waits
in the send queue until the ban ends and is sent again, and the refused
transmission does not count as one of its attempts. The neighbour is
also **banned** for 1.56 s. A relay refuses a packet to send on when the
frame and the ACK that takes it, at most 138.6 + 14.4 = 153.0 ms, do not
fit in its bucket on top of what it holds and no hop place takes it
(§9.2.4), and its bucket refills that much in 153.0 ms / 9.83 % = 1.56 s
(§9.4). Until then it is no candidate for any message (§9.2.3). A leaf
refused by its relay keeps the relay and the message, and offers the
message again 1.56 s later (§10.2).

**Overload makes a message wait, never ends it.** A message whose every
remaining candidate is banned waits in its place in the send queue,
with its airtime still counted (§9.2.4), until the earliest ban ends,
and is routed again then, at the origin or anywhere else. A message
ends only when it is delivered, when a hand-over's attempts all go
unanswered (step 6), when its leaf never wakes (§10.4), at the hop cap
(§9.2.6), or when a node holding it dies (roadmap D31).

Waiting alone can lock relays up: two neighbours whose queues are full
of messages for each other would refuse each other forever, since what
a relay holds drains only as it sends (roadmap D29). The hop places of
the send queue prevent it. Take the highest-numbered hop place held at
any relay, k. Its message has made at least k hand-overs and is sent
with a higher count, and place k + 1 is free at every relay, so the
neighbour it is offered to can take it, unless its count is at the cap,
where it ends. That message moves, then the next highest, and so on down
to the regular room. For this the message that has come farthest must
be offered when a ban ends: a less travelled one offered first would be
refused and ban the neighbour again, each time. So when bans end, the
messages that waited for them go first, the most travelled first; the
rest of the queue keeps its order, as ordering it all by count would
starve messages that have just entered.

The relay's own application's message, offered once a ban, would be a
full frame every 1.56 s on the refuser's channel, 9 %, hiding its ACKs,
and would spend the holder's bucket as fast as it refills; so after the
k-th refusal in a row it waits one ban times 2^(k−1), 1.56 s, 3.11 s,
6.2 s and so on, up to the bucket's whole refill, 61 s, the longest
anything waits for a bucket (§9.4), and the wait starts again from one
ban once the message is taken or dropped.

An ACK names the node it answers because two senders can offer the same
message to the same relay at once: one retrying a hand-over whose ACK
was lost while another returns the message there, or the two copies of
a split message (§9.5). If the ACK named only the message and the
relay, each sender would take the answer to the other as its own. A
taken one would end a hand-over the relay never took, and the message
would be lost; a refused one would send a sender on from a relay that
took its packet, and the message would have a second copy (roadmap
D32). A leaf's DATA names no sender (`src_hop` 0, §14), so an ACK to a
leaf names none either; two leaves do not offer the same message, whose
ID covers the payload.

**Retry wait.** The wait before the retry after the k-th unanswered
transmission is uniform in [0, 2^k × 153.0 ms]: 0–306 ms, then 0–612 ms.
153.0 ms is how long one hand-over holds its receiver, a full frame
(138.6 ms) and its ACK (14.4 ms). The usual cause of a missing ACK, other
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
that was itself a return to the parent is not retried elsewhere: the
message is dropped.

The receiver retunes to the sender's channel, listens before talking,
sends the ACK, and retunes home. The ACK is the first byte, whose `aux`
says whether the packet is refused, the message ID, and the names of the
answering and the answered node, each a relay's short name or a leaf's
ID (13 bytes, 14.4 ms, §14). The sender matches the ID and both names
against its outstanding hand-over. The answering node's name is needed
because relays share home channels: without it, an ACK for the same
message from a different relay on the same channel, which the
depth-first search can produce, would end the wrong hand-over and
strand the message; the answered node's name, for the reason above. A relay
ACKs a packet as soon as it has received it, even while waiting for an
ACK of its own; if that makes it miss its own ACK, its next attempt
covers it.

A relay owes at most one reply at a time, an ACK or an ATTACHED
(§10.2). A packet, DATA or ATTACH, that arrives while its reply to
another packet is still waiting for the channel is neither taken nor
refused: the relay says nothing and changes
nothing, and the sender retries as for a lost ACK. A second reply would
go out only after the first, which may wait for its channel until its
own sender gives up, 100 ms; taking a packet it may not be able to
answer in time would let its sender hand the message to someone else
as well. A copy of the packet the owed reply answers, sent again because
that reply is late, is answered by it (roadmap D29).

So: **a node receives everything, ACKs included, on its home channel, and
leaves it only to transmit.** A leaf's home channel is its relay's, so
leaf ↔ relay traffic and its ACKs never retune. A relay is deaf for
≈15 ms per packet it forwards or ACKs; a sender that gets no ACK retries.
That is the cost of having one radio.

**Send queue.** A regular room of 40 places, and one hop place for each
count from 1 to 254; the packet being handed over keeps its place. A
relay queues a packet in the regular room only if everything queued,
that packet included, fits in its bucket as it stands (§9.2.4), so a
packet there waits for the hand-overs ahead of it, not for the bucket
to refill. The number of places follows: the bucket holds at most 6 s
(§9.4), and a full frame and the ACK that took it on cost 138.6 +
14.4 = 153.0 ms, so ⌈6 s / 153.0 ms⌉ = 40. With the reply reserve left
in, a full bucket takes at most 37 full frames, so for full frames the
bucket is the limit; shorter frames fill the 40 places first, and forty
hand-overs of them go out within a few seconds.

A packet received with count h that the regular room refuses takes the
highest free hop place numbered h or lower, and keeps it until it leaves
the queue. A hop place holds its packet whatever the bucket holds; the
bucket still decides when it is sent. A packet from a leaf or from the
relay's own application has count 0 and no hop place, and is not taken
while every neighbour is banned (§9.2.4), so overload stops new messages
where they enter, at the application (§9.1). A packet
received at 255 ends at the cap (§9.2.6), so 254 hop places are enough.
Packets are handed over in the order they arrived, except that, when
bans end, the packets that waited for them go first, as above.

### 9.4 Duty cycle cap

At SF5, 10% of airtime is ≈180 B/s per relay, so a busy relay runs out of
budget before its channel fills. Greedy forwarding does not spread load;
central rooftop relays will hit the cap first.

**Token bucket.** Each node keeps its airtime in a bucket that holds at
most 6 s, one minute at 10%, and fills at (360 s − 6 s) per hour, 9.83%
of the time. A transmission may start only if the bucket holds its
airtime, which it takes out. Any interval of length L then carries at
most 6 s + 9.83% × L on air, so any hour at most 360 s: 10%, the limit
of ERC/REC 70-03 band g1, which is measured over an hour (proof in
`tables/budget.rs`). The rate is lower than 10% by the bucket's depth
over the hour, so that a burst cannot carry an hour over the limit. A
node at the cap sends steadily, 5.9 s of every minute; once its bucket
is empty, a full frame waits 1.4 s for it, and nothing waits longer
than 6 s / 9.83% = 61 s (roadmap D26).

**Out of budget.** A hand-over the bucket cannot fit yet waits for it;
it is not dropped. Two reserves stay in the bucket. Replies, ACKs that
take or refuse, and ATTACHED, polls included (§10.2), leave one
heartbeat's airtime, 11.8 ms, which the
bucket refills in 120 ms, far less than the 5 s between heartbeats
(§7), so heartbeats always go. Forwarding, and the ACK that takes on a
packet, leave the **reply reserve**: the refusals the bucket refills
while a sender retries an unanswered hand-over, and the heartbeat. The
retrying takes the two retry waits, 306 + 612 ms, and two more attempts
of 500 ms listen-before-talk give-up, a 138.6 ms full frame and the
100 ms ACK timeout (§9.3), 2.39 s in all; the bucket refills 9.83 % ×
2.39 s = 235.5 ms of it, ⌊235.5 / 14.4⌋ = 16 refusals, and with the
heartbeat the reserve is 242 ms. A relay takes new work into its
regular room only if its bucket holds the reply reserve on top of the
work's ACK and of everything it has queued to send (§9.2.4); otherwise
it takes it into a hop place or refuses it. A hand-over therefore waits for the bucket
only when retries or replies have spent it since its packet was taken,
or when its packet holds a hop place. A relay below the reserve sends nothing but heartbeats
and replies until its bucket refills. Each sender it refuses bans it
for 1.56 s (§9.3), so a sender costs it at most one reply per ban.
Without budget even for a reply it stays silent, its neighbours'
hand-overs to it fail, and they route around it (roadmap D27, D29).

### 9.5 Why the search finds a path

Suppose that while a message is being routed the links do not change,
every hand-over to a neighbour that answers succeeds within its attempts,
and no record is replaced. Then:

- The message moves as a single packet: a relay sends only what it has
  just received or originated, and every ACK-only case of §9.2.4 sends
  nothing.
- A relay holding the message either owns the destination, sends to its
  next candidate, or returns it to its parent. A neighbour that takes the
  message is tried and never offered it again from that record, so each
  relay hands the message to each neighbour at most once; a return goes
  up the parents, which lead to the origin without a loop. So each stay
  of the message at a relay ends with a hand-over that succeeds, or at
  the origin with a drop, and there are finitely many of those: the
  search ends. A neighbour that does not answer is never tried; it costs
  three attempts each time the message is routed past it, and the hop
  cap bounds the stays (§9.2.6).
- A send to a relay that already has the message hands it the search
  (§9.2.4, case 5): it goes on with its own candidates. The sender is
  one of them unless the relay has tried it, so the search comes back
  to the sender as a return, and the sender goes on with its own. A
  relay whose search is unfinished gets the message back that way or
  through the parents, so none is left with neighbours untried.

So the message makes a depth-first traversal of the relays reachable from
the origin through neighbours that answer, nearest to the destination
first; a relay met again is searched on from where it stood, not
skipped. A depth-first traversal visits every relay it can reach; it stops
at the first owner, or returns to the origin when there is none. If a path
to an owner exists, the message finds it, provided the traversal fits in
the 255 hand-overs of §9.2.6.

What breaks it:

- **A lost record.** A message returning to a relay whose record has
  been replaced is new there; the relay searches again from
  itself, and the relays above it never get the message back. The search
  ends early, or wanders until the hop cap ends it.
- **A split.** A hand-over whose ACKs were all lost looks failed, so the
  sender moves on while the receiver also carries the message: two copies
  search at once. They share the relays' records, so each goes on from
  where the other left a relay. The sender has not marked the receiver
  tried, so a copy that comes back to it is offered there again. A copy
  ends where it meets the other held at a relay (§9.2.4, case 4), where
  the message has arrived (case 1), at the origin with the search, or at
  the hop cap. A copy is never ended where it is the last one.
- **Links that change** during the search, including a parent that moves
  to another home channel (§9.2.5): parts of the graph go unsearched.

### 9.6 Bounds

- **Hand-overs per message:** at most 256 that succeed (the first, at
  count 0, and 255 more, §9.2.6). A relay hands the message to each
  neighbour at most once per record, in at most 3 attempts. A neighbour
  that does not answer costs 3 attempts each time the message is routed
  past it: once per stay of the message at the relay, and once more
  after each wait for a ban. After two such failures it is a last resort
  (§8.2), and a dead one leaves the table within 390 s.
- **Cost of a search:** each link between two relays costs at most two
  hand-overs that succeed from each end: a send and a return. In the
  usual case it is two in all; a split adds a round trip on the link
  whose ACKs were lost. A link that does not answer costs three attempts
  each time the message is routed past it, so a search that comes back
  through a relay often, as one with no owner does, pays for it at every
  stay (roadmap D36). Within the hop cap a search can
  explore about 64 to 128 links beyond the route that led into the gap.
- **State per relay:** at most 1695 records of 32 bytes, 54 KB
  (§9.2.2); a send queue of 294 packets of up to 255 bytes, 75 KB (§9.3);
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

### 10.2 Attach and polls

**Attaching.** `ATTACH {type, leaf_id, relay, flags}` (`relay` is the
relay's short name, §8.1) is sent to the relay on its home channel with
listen-before-talk and the retries of §9.3. The flags carry the leaf's
receive mode (§10.5). The relay answers at once with `ATTACHED {type,
leaf_id, next_poll, waiting}`, as it would with an ACK; that is what
tells the leaf it is attached. `waiting`, in the first byte's spare
bits, counts the messages the relay holds for the leaf (§10.5). A leaf
sends an ATTACH that attaches at the start, after it chooses another
relay (§10.1), and after its polls stop (below), and at no other time.
The relay's **leaf table** (`leaf_id`, its next slot, the slot of its
last poll and of the first it left unanswered, when it was last told and
last heard, `rx_mode`, and a few counts) is separate from the neighbour
table. Leaf table size: 256 entries per relay (≈80 leaves per relay
expected at indoor density in Kyiv with 10% adoption; 400 at 50%). A
relay whose table is full does not answer, and the leaf attaches
elsewhere.

**Polls.** From then on the relay checks that the leaf is there, not the
leaf that the relay is. It polls one leaf at a time: at each poll it
sends the leaf an ATTACHED, and the leaf answers with one ATTACH marked
as a reply, which the relay does not answer. The poll is sent by the
relay after its own channel check, so every neighbour of the relay hears
it, and the answer follows right behind it. Keeping its relay, a leaf
never speaks at a time it chose itself, where its frame would meet DATA
it cannot hear (roadmap D41); it does so only to attach. `next_poll` in
every ATTACHED is the delay from the end of that frame to the start of
the leaf's next slot. It is not a period: the leaf knows only when its
next poll comes, and each ATTACHED names the next one afresh, so the
relay may change its period without telling anyone.

**The period.** With L leaves in its table, each is polled once a period

P = L × N, but at least 10 s and at most 300 s, with **N = 4 s**,

while the relay's home channel is quiet, and P = 300 s while it is not.
No two slots are closer than the spacing S = P / L, which is N unless a
bound holds the period. Each poll, sent or skipped, gives the leaf the
first time at least one period after its slot that lies S from every
other leaf's next slot; a new leaf gets the first such time a period
after it attached. So the relay never polls two leaves closer than S,
and with the table steady the round settles to one poll every S, each
leaf every P: every slot of the round is used, so the polls are spread
evenly over it however the leaves attached, together after a mass start
or a restart or one by one. When L or the rule changes, the slots
already named hold, and each leaf's next slot follows the new period;
the round settles within a period.

| | Value | From |
|---|---|---|
| ATTACHED, ATTACH | 7 and 9 bytes, 10.56 and 11.84 ms | §14 |
| Relay airtime, quiet, 3 to 75 leaves | 10.56 ms / N = 0.26 %, 2.7 % of the duty budget | the bucket fills at 9.83 % (§9.4) |
| The same, full table of 256 | 10.56 ms / (300 s / 256) = 0.90 %, 9.2 % of the budget | |
| The same at 80 leaves, quiet or not | 80 × 10.56 ms / 300 s = 0.28 %, 2.9 % of the budget | |
| Channel time at 80 leaves, polls and answers | 80 × 22.40 ms / 300 s = 0.60 % | |
| Channel time at 10 leaves | quiet, 22.40 ms / 4 s = 0.56 %; busy, 0.075 % | |
| Shortest period, one or two leaves | 10 s | |

Three bounds set the period. While L × N lies between 10 s and 300 s, N
bounds the share of the relay's budget polls take: each costs the relay
one ATTACHED, 10.56 ms, so at one every N the relay spends 10.56 ms / N
whatever its table holds. Polls may take 3 % of the budget, what those
of roadmap D39 took at the ≈70 leaves of a relay in the capacity domain,
where the domain worked at twice the §13 rate; so N ≥ 10.56 ms / (3 % ×
9.83 %) = 3.6 s, and N = 4 s, 2.7 %; N = 300 s / 256 = 1.17 s would take
a tenth of the budget at every table of more than 8 leaves, and the
domain collapsed at a lower rate with it (roadmap D41). At more than 300
s / N = 75 leaves the 300 s cap wins: it is half the delivery TTL, so a
gone leaf is kept at most a period past the life of its messages
(below), and the slots are 300 s / L apart, never closer than 300 s /
256 = 1.17 s, so that a full table costs 0.90 % of the relay's time, 9.2
% of its budget. The 10 s floor serves small cells: a phone back from
the dark there is heard within 10 s, for 0.12 % of each leaf's time on
air.

**Only on a quiet channel.** Every shortened period is extra frames on
the relay's channel, and extra frames on a loaded channel are what tip
the capacity domain into collapse (`collapse-research-2026-10-16.md`).
So the relay shortens its period below 300 s only while its own
measure of its home channel, the `busy` of §6.1 that the move rule
reads, says the channel is quiet: it becomes quiet below 3.5 % and
stays so until 4.5 %. The domain works at the §13 rate and at twice
it, where relays read a median of 3.2 to 3.4 % and 5.3 to 5.8 %
(roadmap D41); 4.5 % lies halfway between them in load, and a relay
below it that polls one leaf every N adds at most 0.56 % of the channel
in polls and answers, which keeps it under the median of twice the
rate. The band between 3.5 and 4.5 % is the hysteresis. It is 2.4 times
the median standard deviation of a relay's reading over time at a steady
load, 0.41 %, and more than the 0.56 % a co-channel neighbour's
shortened polls can add to the reading; a relay's own polls and the
answers to them do not count in its own reading (§6.1), so shortening
cannot by itself end the quiet. The busy estimate moves slowly, over 10
minutes (§6.1), so the rule changes the period a few times an hour at
most.

**Lateness and the window.** A poll goes before the heartbeat and the
hand-over, after an owed reply, and not while the relay waits for an ACK.
So it waits at most for a hand-over already under way, a full frame and
its ACK timeout, 238.6 ms, then for an owed ACK, 114.4 ms, and checks the
channel with what is left of **500 ms**, listen-before-talk's give-up time
(§9.3); a poll that cannot go out within 500 ms of its slot is skipped,
and counts as unanswered. A leaf that sleeps (§10.5) hears only a long
preamble, and 512 ms of preamble would make each poll 49 times longer.
Instead it wakes for its slot and listens with the ordinary receiver:
from the slot less the drift of two crystal clocks of ±50 ppm each over
the time since the last ATTACHED (30 ms after a 300 s period) and half
the 10 ms unit of `next_poll`, to 500 ms plus the poll's 10.56 ms after
the slot, plus the same margin: about 0.58 s a period, 11 µA on average
at the 5.8 mA of listening at 300 s. A 32.768 kHz tuning-fork crystal is
±20 ppm at 25 °C and −0.034 ppm/°C² away from it, so ±50 ppm holds from
−5 to +55 °C. The nRF52840's internal RC oscillator (±500 ppm, Product
Specification, CLOCK), which the firmware's clock runs on today, would
widen the window to 1.1 s; a sleeping leaf should time its slot from a
crystal. An always-listening leaf keeps the same window: it takes a
poll only there, so that a poll from a relay it has left, on the same
channel and for the same leaf ID, is not taken for its own relay's.

**The answer.** After a poll the relay listens for the answer as it
does for the ACK of a hand-over (§9.3): until the answer comes or the
ACK timeout, 100 ms, has passed, it sends nothing but an owed reply.
Otherwise its next frame, a hand-over queued behind the poll, starts a
few milliseconds after the poll ends and makes it deaf to the answer.
On a relay at its cap whose traffic repeats in step with its period,
that happened at every poll of one leaf and the relay dropped a leaf
that was there (roadmap D41).

**Lost frames.** A lost ATTACHED that answers an ATTACH is a lost ACK:
the leaf retries its ATTACH (§9.3), and the relay, which has the leaf
already, answers with the same slot. A leaf whose window passes empty,
because the poll or its answer was lost, the relay skipped it, or the
leaf was out of reach, no longer knows when its next poll comes. It
takes the next poll whenever it comes, and if none has come by then,
attaches again as below.

**Keeping the leaf.** The relay drops a leaf whose polls have gone
unanswered for a delivery TTL: at the first slot at least 600 s after
the first poll it left unanswered, once the answer to that one is
overdue, 0.6 s after its slot. So a phone dark for less than a TTL,
the five minutes of a metro ride or a basement (§10.4), is still in the
table when it is back, and its messages are still here; a gone leaf is
kept at most a TTL and a period, 15 minutes, after its last answer, a
period past the life of the messages held for it. Queued messages of a
dropped leaf stay in the delivery queue until their TTL, as for any
leaf not in the table, and go to it if it attaches again within it.

**Keeping the relay.** A leaf does not give its relay up on a clock: a
phone in a pocket or the metro is out of reach of every relay, and
should find its relay when it is back. A leaf whose window has passed
empty stays silent until it hears a heartbeat, from any relay, and then
sends an ATTACH that attaches after a random wait of 0 to 60 s, unless
a poll comes first. Only heartbeats count, not other leaves' frames, so
that phones dark together in one train do not wake each other. A leaf
that hears no heartbeat tries once a rotation after the window,
13 × 15 s = 195 s, the longest any relay in reach can stay unheard (§7),
and after each ATTACH unanswered at all its attempts waits again for a
heartbeat or a rotation. The ATTACH goes to the leaf's own relay if it
has heard that relay since the polls stopped; if it has not, but has
heard another, its relay is dead or out of reach, and it attaches to
the strongest of those it has heard since (§10.1). A relay that still
has the leaf answers with its slot unchanged; one that dropped it takes
it as new. A message the relay did not take at any attempt (§9.3) is
tried again 60 s later. A fade lasts about a minute (the scenarios'
correlation time), so one fade costs a wait, not the relay: the three
attempts of one hand-over, a few hundred milliseconds apart, all fall
inside it. The leaf gives the relay up when hand-overs have failed in
two retries in a row and it has heard another relay since the first
failure, and then chooses as in §10.1. Having heard none, it may be out
of reach of every relay, and keeps the relay.

What this costs and gains:

- A dark phone sends at most one ATTACH, three frames, per rotation.
- A phone back in reach before its window is polled at its next slot.
  One that missed a window is heard at its first heartbeat and 0 to
  60 s later, or at the next poll if that is sooner. With r relays in
  earshot, a heartbeat reaches the leaf's channel every 130 s / r on
  average (§7); at the design density's 2.7 relays indoors (§10.1) that
  is every 48 s, so the phone is heard 78 s after its return on
  average, and at most 195 + 60 = 255 s.
- A train of n phones surfacing by one relay offers it n / 60 ATTACHes a
  second. For 300 phones that is 5 a second, 53 ms of ATTACHED in each
  second, within the 98 ms a second the bucket refills (§9.4), rather
  than hundreds at once. The relay gives them slots a period on, spread
  by the spacing like any others.
- A leaf whose relay has died and that hears another moves at its next
  window, a heartbeat from the other relay and up to 60 s: at most
  P + 0.6 s + 195 s + 60 s after its last poll, 556 s at 300 s, 266 s at
  10 s.
- A leaf that hears no other relay keeps trying its own once a rotation
  for ever.

**When each side knows.**

| | Known to | After |
|---|---|---|
| A leaf goes dark, or away | its relay, which counts its poll unanswered | its next slot: within P |
| | its relay, which drops it | 600 s after its first unanswered poll, and at most one period more |
| A dark leaf comes back | its relay, still its relay | its next slot if it missed none; else its first heartbeat and 0 to 60 s, at most 255 s |
| A leaf moves to another relay | the new relay | at once, its first ATTACH |
| | the old relay | a TTL and up to a period after its last answer there |
| A relay dies | its leaves, which move | their next window, a heartbeat from another relay and 0 to 60 s: at most P + 256 s |

**Moves.** A relay that moves home channel (§6.3) keeps its leaves and
their slots and polls them on the new channel. An always-listening leaf
follows at once on the heartbeat the relay sends on the old channel, or
at the relay's next heartbeat on its channel if it missed that one,
within a rotation (≤195 s); a hybrid leaf follows in its next awake
rotation (≤10 minutes, §10.5). A leaf that misses its window meanwhile
attaches again as above, on whichever channel its relay's heartbeat
names by then. A leaf that moves to another relay stops answering the
old one's polls, which drops it a TTL later; until then a message sent
to the old address waits there as for a dark leaf (§10.4).

**A message without a relay.** A message the leaf cannot hand over,
because it has no relay or its relay does not answer, is held and tried
again as above, for up to the delivery TTL (10 minutes, §10.4), the time
the network itself would keep it. Only then is it dropped, and the app
is not told (§9.1). The leaf holds one message at a time (roadmap D23)
and takes the next from its app once this one is handed over or
dropped. A relay that refuses the message, because it has no room or
every way on from it is banned (§9.2.4), is busy, not failing: the
leaf keeps it, counts no failed period, and offers the message again
one ban, 1.56 s, after the first refusal, then twice as long after each
further one in a row, up to the bucket's refill of 61 s, as a relay
with its own message does (§9.3): a leaf on an overloaded relay would
otherwise probe it with a full frame every 1.56 s (roadmap D27, D29).

### 10.3 Delivery

The owning relay sends the message to the leaf on the relay's own home
channel (the leaf listens there), listen-before-talk first, and waits for
the leaf's ACK. Preamble length per the leaf's receive mode (§10.5).
A leaf, like a relay, owes one reply at a time: a DELIVER that arrives
while its ACK of another is still waiting for the channel is ignored,
and the relay's retry brings it again (§9.3, roadmap D29).

### 10.4 Delivery queue

Bounded, not a mailbox: up to 41 messages per relay, kept for 10 minutes
(the delivery TTL). On expiry the message is dropped. When the queue is
full, a message for one of the relay's leaves is refused (§9.2.4): what
is queued stays, and the sender learns at once.

A queued message goes to the send queue (§9.3) as a DELIVER, one per
leaf at a time and oldest first, when it arrives, when the leaf's
previous one is ACKed, and whenever a hand-over ends and leaves room in
the send queue. It needs room as any packet does (§9.3); one the bucket
cannot fit yet goes the moment the bucket will. A leaf whose DELIVER
went unanswered is not listening: its messages wait for its next answer
to a poll (§10.2) and are retried every 60 s, not each time room frees
up.

A message stays long only for a leaf that does not answer. A leaf that
listens takes its DELIVER at the first hand-over: a duty-cycled or
hybrid one checks the channel every 500 ms, and the 512 ms preamble
spans the check (§10.5). One that does not is a phone in a pocket, a
basement or the metro for a few minutes, and its messages wait until the
first retry after it is back, at most 60 s later, or its answer to a
poll (§10.2), or until the TTL. Leaves in one train or one basement go
dark together, so the queue holds what arrives in a TTL for every leaf a
relay can serve, its leaf table's 256 (§10.2), at the 23 texts a day
each that the network carries (§13): 256 × 23 / 144 ten-minute TTLs in a
day = 40.9, so 41 messages, 10.3 KB.

The owner remembers 41 delivered message IDs (§14) and ACKs a repeat
without delivering it twice. When it needs the place, it forgets the one
delivered or last repeated longest ago; time alone forgets nothing. 41
is the delivery queue's size: what a relay delivers in one TTL for a
full leaf table. A queued
message costs no airtime; a failed delivery costs a retry across the
city.

The queue plus the sender's own retry covers a leaf that has moved to
another relay; the old relay does not forward to the new one. Neighbouring
relays cannot overhear deliveries, because they are on different channels.

### 10.5 Leaf receive mode

| Mode | Leaf current | 850 mAh | Preamble on packets to the leaf | Hears heartbeats |
|---|---|---|---|---|
| Always listening | ≈5.8 mA | ≈6 days | 12 symbols (§4) | yes |
| Duty-cycled (sleep, CAD every 500 ms) | ≈0.3 mA + sleep | months | ≥512 ms | no: blind to relay changes |
| Hybrid: full listen one rotation every 10 min, CAD between | ≈1.3 mA | ≈25 days | ≥512 ms | during the awake rotation |

The leaf declares its mode in ATTACH; the relay sets the preamble of
DELIVER to match. Long preambles cost little airtime because deliveries
are rare (≈0.5% of a channel per day at 80 leaves × 10 messages). Polls
keep the short preamble: a sleeping leaf listens for its poll at the
time the last one named (§10.2).

Each ATTACHED also says how many messages the relay holds for the leaf,
`waiting`, up to 15. A sleeping leaf that answers a poll counting n of
them stays
listening with the ordinary receiver, unless it has them all sooner, for
as long as the relay can take to bring them: the DELIVERs that may be
ahead of them, a full send queue of full frames and their ACKs,
40 × 152.96 ms = 6.12 s (§9.3), and each of the n with all its attempts,
3.13 s (§10.2), so 6.12 s + n × 3.13 s, at most 53 s. While that lasts
the relay sends
those n with the 12-symbol preamble; a message that arrived after the
poll, or one still queued after the wait, gets the long one. So the
messages of a sleeping leaf that is in reach reach it at its next poll
at the latest, at the short preamble's airtime, and the long preamble
is kept for messages between polls. The leaf core runs only the
always-listening mode today; the relay side of this is built and
tested. Default: always
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
takes ≈0.67 s of channel time: per hop a 121-byte DATA packet (69.4 ms)
and its ACK (14.4 ms).

| Plan | Heartbeat load per channel | City-wide data | Texts/s | Texts per user per day at 300k users |
|---|---|---|---|---|
| Single channel, 60 s heartbeats | 8% | 7 kB/s | 7 | 2 |
| 7-colour fixed grid, heartbeats on all colours | 11–14% | 37 kB/s | 37 | 11 |
| **13 channels, measured selection, rotating heartbeat (this design)** | **≈2.5%** | **≈85 kB/s** | **≈80** | **≈23** |
| + per-packet power control (later) | | ≈160 kB/s | ≈150 | ≈45 |

These are orders of magnitude; the hearing radius and hop count are each
uncertain by 2×. The city centre is the hot spot, and the per-relay 10%
cap (§9.4) binds there first.

The relays' polls of their leaves (§10.2) cost a relay one ATTACHED,
10.56 ms, per poll, and its channel the poll and the 11.84 ms answer. At
the ≈70 leaves of a relay in the capacity domain the period is 280 s on
a quiet channel and 300 s on a busy one, so polls take 0.26 % of the
relay's time, 2.7 % of its budget, and 0.56 % of its channel; at 80
leaves the 300 s cap holds and it is 0.28 %, 2.9 % and 0.60 %. A full
table of 256 takes 0.90 % of the relay's time, 9.2 % of its budget.
Summed over the domain, the 20 000 leaves are polled 67 times a second
whatever the rule, 1.49 s of airtime a second, 11.5 % of one channel's
time over the 13, where every relay hears every other. A relay with few
leaves polls them more often only while its channel is quiet: at 10
leaves every 40 s, 0.56 % of its channel, against 0.075 % at 300 s.

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
ACK       0x3: first(1) id(4) from(4) to(4)                                 = 13
          the answer to a DATA or DELIVER, taken or refused; sent on
          the channel in the answered packet's aux; aux bit 0 = refused
          from = the answering relay's short name, or the leaf's ID
          to = the answered relay's short name, or the leaf's ID; for a
          leaf's DATA, which names no sender, 0
ATTACH    0x4: first(1) leaf_id(4) relay(4)                                 = 9
          relay = short name of the relay attached to; aux bits 0-2 = leaf
          receive mode (§10.5), aux bit 3 = answers a poll; one that attaches
          is answered with ATTACHED, one that answers a poll is not answered
DATA      0x5: first(1) dst_hop(4) src_hop(4) dst_pos(7) dst_leaf(4) hops(1)
               payload                                                      = 21 + payload
          dst_hop, src_hop = short names of the receiving and the sending relay;
          aux = sender's home channel, so the receiver knows where to ACK
DELIVER   0x6: first(1) dst_leaf(4) id(4) payload                           = 9 + payload
          relay → leaf on the relay's own channel; leaf ACKs on the same channel,
          to = the short name of the relay it is attached to
ATTACHED  0x7: first(1) leaf_id(4) next_poll(2)                             = 7
          relay → leaf on the relay's own channel: the answer to an ATTACH
          that attaches, or a poll (§10.2); next_poll = the delay from the
          end of this frame to the leaf's next slot, big-endian, in 10 ms,
          not a period: each ATTACHED names the next slot afresh; 16 bits
          reach 655 s, more than the 600 s TTL that bounds the period;
          aux = messages the relay holds for the leaf (§10.4, §10.5), 15
          for 15 or more
Type 0xF is reserved for the bench frames of the
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

**RAM budget (256 KB):** neighbour table 64 × 48 B = 3.0 KB; leaf table
256 × 16 B = 4 KB; delivery queue 41 × 256 B = 10.3 KB; message records
(§9.2.2) 1695 × 32 B = 54 KB; send queue (40 + 254) × 272 B = 80 KB;
delivered window 41 × 16 B = 0.7 KB; busy estimates and duty budget under
1 KB; 154 KB in all, measured. BLE controller ≈5 KB; Embassy and stacks ≈16 KB. The
application has 255 KB; 1 KB at the bottom is kept for the boot record and
the panic message (`bark-fw/memory.x`).

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
5. Leaf attach, polls, delivery, queue. **This is the loop every
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
| O15 | A relay at its duty-cycle cap refuses only when its send queue is full, and its queue then drains at the bucket's rate: a packet can wait ≈23 s there (with three frames a message, 72 s median in the three-hour test) where the sender could have chosen another next hop at once. Whether to bound the queue by its drain time at the bucket's rate, and how | resolved: roadmap D29 rule 1, §9.2.4, §9.3 send queue |

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

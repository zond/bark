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
single transmission longer than 1 s (SF5 packets are ≤140 ms anyway).
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
| Preamble | 8 symbols (≈2 ms) + sync; **no long preambles** except to duty-cycled leaves (§10.5) | |
| 255-byte packet | ≈140 ms at SF5 | ≈400 ms at SF7 |
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

- **Sweep:** once per second, when the modem reports no preamble detected
  and no packet in progress: retune through all 13 channels, one RSSI
  reading each (≈0.3 ms after retune), retune home. ≈5 ms, ≈0.5% of the
  time off the home channel.
- **Cost:** a packet whose preamble overlaps the 5 ms gap is lost and
  retransmitted after the ACK timeout. With the 8-symbol preamble that is
  ≈0.5% of arrivals, well below loss to collisions. Spreading the 13
  samples across the second does not help by itself: the window lost per
  gap is `g + 2D − P` (gap, time the modem needs to lock, preamble
  length), and with P ≈ 2D thirteen small gaps lose as much as one burst.
  Spreading plus a 12-symbol preamble would remove the loss, but costs
  1 ms on every packet, ≈3% of airtime on a text plus its ACK, six times
  the loss it prevents. If the loss ever matters, sweep every 2 s instead.
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

8 bytes, ≈12 ms on air at SF5. There is no node ID: a relay's position
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

- **Expire** after 3 rotations (≈400 s) without a heartbeat.
- **Demote** on ACK failure: `fail_count += 1`; a neighbour with
  `fail_count ≥ 2` is not used as a next hop until its next heartbeat
  resets it. Heartbeat expiry takes minutes; a failed forward must demote
  at once.
- **Eviction** when full: least recently heard. (Open item O4: rooftop
  relays hear ~270 relays; a geographically diverse set is better for
  greedy routing than the 64 most recent.)

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

### 9.2 Greedy forwarding

Forward to the neighbour (relay, not demoted) closest to the destination
position in straight-line distance. If no neighbour is closer than self,
the packet has reached a gap and is routed around it by depth-first search
(DFS).

Each relay keeps, for every message it has forwarded and for a bounded
time, the neighbour it first received the message from (its parent) and
the neighbours it has tried. At a dead end it forwards to the nearest
untried neighbour that is not in the packet's visited list; when none is
left it returns the message to its parent, which continues with its own
next untried neighbour. The originating relay with nothing left to try
drops the message.

The packet's visited list holds the hash of every relay that has forwarded
it, appended on each forward, and is bounded; when it fills, the packet is
dropped. Its last entry identifies the previous hop, matched against the
neighbour table by hash, with the home channel the packet names for its
sender (§14) as tie-break. A
repeat that arrives from the parent is a retransmission and is only ACKed;
a repeat from a neighbour we forwarded to is a backtrack. DFS is only for
routing around gaps, never for searching for an ID.

A relay **owns** a destination when it is the closest relay the packet can
reach: no non-demoted neighbour is closer to `(x, y)`. The owner looks
`leaf_id` up in its leaf table and delivers (§10.3), or queues (§10.4). If
`leaf_id = 0`, the owner is the destination.

If the owner is not within 50 m of `(x, y)`, the addressed relay has
probably died or moved. The owner may ask its direct neighbours once
whether they hold `leaf_id` (one hop, one round); if none does, queue.
Nothing wider: a wider search is a flood.

### 9.3 Per-hop transfer

1. Retune to the next hop's home channel.
2. Listen-before-talk: CAD, then RSSI. Random backoff 0–50 ms if busy,
   retry; give up after 500 ms and choose another next hop.
3. Transmit. The packet carries the sender's home channel (§14).
4. Retune home immediately.
5. Wait for the ACK **on the home channel**: timeout 100 ms (ACK airtime
   ≈10 ms plus the receiver's retune and listen-before-talk).
6. On timeout: random backoff, retry, up to 3 attempts; then demote the
   neighbour and choose another next hop.

The receiver retunes to the sender's channel, listens before talking,
sends the ACK, and retunes home. The ACK is the first byte and the
`msg_id` (5 bytes, ≈10 ms). It has no target address; the sender matches
it by `msg_id` against its outstanding transmissions.

So: **a node receives everything, ACKs included, on its home channel, and
leaves it only to transmit.** A leaf's home channel is its relay's, so
leaf ↔ relay traffic and its ACKs never retune. A relay is deaf for
≈15 ms per packet it forwards or ACKs; a sender that gets no ACK retries.
That is the cost of having one radio. A receiver that has already handled
a message classifies a repeat by who sent it: from its parent, a
retransmission, ACKed and otherwise ignored; from a neighbour it forwarded
to, a backtrack (§9.2).

### 9.4 Duty cycle cap

At SF5, 10% of airtime is ≈180 B/s per relay, so a busy relay runs out of
budget before its channel fills. Greedy forwarding does not spread load;
central rooftop relays will hit the cap first. When the budget is
exhausted the relay stops forwarding and drops what it cannot send, but
keeps sending heartbeats.

---

## 10. Leaves

A leaf is a carried node on battery. It **never sends heartbeats** (at
household density that would be hundreds of percent of airtime on every
channel) and never forwards.

### 10.1 Relay choice

Listening on its relay's home channel, a leaf hears every relay in range
once per rotation (≈130 s). It keeps an averaged RSSI per relay and
switches to another relay when that relay has been stronger by ≥6 dB for
two consecutive heartbeats. It attaches to the new relay before leaving
the old one. At walking pace a leaf hears each relay two to four times
while in its coverage; vehicles cannot keep up and rely on the delivery
queue plus re-attaching on arrival.

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

### 10.3 Delivery

The owning relay sends the message to the leaf on the relay's own home
channel (the leaf listens there), listen-before-talk first, and waits for
the leaf's ACK. Preamble length per the leaf's receive mode (§10.5).

### 10.4 Delivery queue

Bounded, not a mailbox: up to 64 messages per relay, kept for 10 minutes
(the delivery TTL). Retry delivery on the leaf's next keepalive and every
60 s. On expiry the message is dropped. The owner remembers delivered
message IDs (§14) for the TTL and ACKs a repeat without delivering it
twice. A
queued message costs no airtime; a failed delivery costs a retry across
the city; the common case is a phone in a pocket, a basement, or the metro
for a few minutes. RAM is not the constraint (64 × 255 B = 16 KB of 256 KB).

The queue plus the sender's own retry covers a leaf that has moved to
another relay; the old relay does not forward to the new one. Neighbouring
relays cannot overhear deliveries, because they are on different channels.

### 10.5 Leaf receive mode

| Mode | Leaf current | 850 mAh | Preamble on packets to the leaf | Hears heartbeats |
|---|---|---|---|---|
| Always listening | ≈5.8 mA | ≈6 days | 8 symbols | yes |
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
takes ≈0.56 s of channel time.

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
ACK       0x3: first(1) id(4)                                               = 5
          sent on the channel in the acknowledged packet's aux; aux unused
ATTACH    0x4: first(1) leaf_id(4) relay(4)                                 = 9
          relay = short name of the relay attached to; aux = leaf receive mode (§10.5)
DATA      0x5: first(1) dst_hop(4) src_hop(4) dst_pos(7) dst_leaf(4) hops(1)
               payload                                                      = 21 + payload
          dst_hop, src_hop = short names of the receiving and the sending relay;
          aux = sender's home channel, so the receiver knows where to ACK
DELIVER   0x6: first(1) dst_leaf(4) id(4) payload                           = 9 + payload
          relay → leaf on the relay's own channel; leaf ACKs on the same channel
Type 0x7 is unassigned.
```

The **message ID** is a 32-bit hash, `hash32` (`position.rs`), of
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

**RAM budget (256 KB):** neighbour table 64 × 32 B = 2 KB; leaf table
256 × 16 B = 4 KB; delivery queue 64 × 216 B = 14 KB; recent-message
window, delivered window, busy estimates and duty budget together under
3 KB; BLE controller ≈5 KB; Embassy and stacks ≈16 KB.

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
| O4 | Neighbour-table eviction policy for rooftop relays that hear hundreds | step 2 |
| O5 | Bench item C1: adjacent-channel rejection at 125 kHz spacing; fallback 11 × 150 kHz | step 1 |
| O7 | Sending to an area (address = position + radius, leaf_id = broadcast) | later |
| O8 | Per-packet transmit power control (prefer short hops, cut rooftop hearing radius): ≈2× capacity | later |
| O9 | Repository/crate layout | resolved: roadmap D7 |
| O11 | Relay-side RSSI noise-floor calibration method and drift | step 3 |
| O12 | Confirm RAK WisMesh Repeater 433 availability for rooftop solar units | procurement |

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

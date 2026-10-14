# bark protocol: Civilian 433 MHz Mesh

**Status:** live specification, 2026-10-14. Consolidated from the October 2026
design dialogue and maintained here; implementation decisions that refine it
are recorded in `roadmap.md` with D-numbers, and the "Open items" section
lists what must still be decided or measured. Where the code and this text
disagree, one of them is wrong and the fix is a commit to both.

A civilian resilience mesh on off-the-shelf hardware, legal without a licence
in both the EU and Ukraine, that gives phone apps a way to message each other
across a city when ordinary infrastructure is gone. It is **not** designed to
resist electronic warfare. Scope is infrastructure loss, not jamming.

---

## 1. Summary

| Item | Decision |
|---|---|
| Hardware | LILYGO T-Echo, 433 MHz variant (nRF52840 + SX1262 + L76K GPS + e-ink + BLE 5.0 + 850 mAh), one firmware for all roles |
| Band | 433.050–434.790 MHz, 10 mW ERP, ≤10% duty cycle per node (legal EU and Ukraine) |
| Modulation | LoRa, SF5, 125 kHz, CR 4/5. Fixed network-wide from day one; no SF migration path |
| Channels | 13 × 125 kHz; each node picks its **home channel** by measured busy fraction (RSSI), no grid, no time sync |
| MAC | Always listening on home channel, leaving it only for own transmit bursts; ACKs arrive on the sender's home channel; listen-before-talk (CAD + RSSI) before every transmission; transmit as soon as clear; no slots |
| Heartbeat | 8 bytes (type and channel nibbles, then a 7-byte global position), every 5–15 s uniformly random, on a channel that steps through all 13 in order |
| Neighbour discovery | Passive: a node hears every neighbour once per 13 heartbeats (~130 s) on its own home channel; no scanning, no schedules |
| Routing | Greedy forwarding to a destination **position**, DFS void recovery, per-hop ACK, demote neighbour on ACK failure |
| Addressing | A relay position plus a 4-byte leaf ID; a relay's position is its only identity; no registration, no gossip, no global table |
| Roles | Relay (mains/USB/solar, beacons, forwards, holds leaf table and delivery queue) and leaf (battery, attaches to one relay, otherwise silent) |
| Phones | BLE GATT to the node; app does end-to-end encryption; sender position travels encrypted in the payload |
| Position | Global WGS-84 latitude/longitude, 51-bit fixed point in 7 bytes (≈0.6 m), no per-deployment origin; distance math in a local tangent plane at each node; GPS when available, else user-entered; never used for timing; relays use surveyed coordinates |
| Time | No network time synchronisation of any kind |

---

## 2. Hardware

**LILYGO T-Echo, 433 MHz build.** nRF52840 (Nordic, Norway), SX1262
(Semtech, US, EAR99), Quectel L76K GNSS, 1.54" e-ink, BLE 5.0, 850 mAh
Li-ion, USB-C charging, UF2 bootloader. About $54 direct, ~€116 from EU
stock. Confirm the LoRa module is the 433 build (SX1262 with 433 MHz
matching), not the 868/915 build; the 868 board's matching network makes
433 MHz operation lossy and out of spec.

Alternatives, same chipset, same firmware:

- **RAK WisBlock** RAK4631 EU433 (PN 111000) + RAK19003/19007 base +
  RAK12500 GPS, for rooftop relays in solar enclosures; a RAK13300 in an
  IO slot would give a second SX1262 if a dedicated scanner is ever needed.
- **RAK WisMesh Repeater Mini V2** (solar + 3200 mAh) for roofs, if a 433
  build is confirmed available.

The SX1262 is not European. The exception is accepted because no European
part offers nRF52840 + SX1262-class integration off the shelf at this price,
and the product premise is that any user can buy and plant a node.
STM32WL55 (ST) is the European fallback but has no BLE.

Roles are a runtime toggle. Carried on battery: leaf. Plugged into USB on a
windowsill, or in a solar box on a roof: relay.

---

## 3. Regulatory envelope

Both regimes are modulation-agnostic in this band.

| | EU (ERC/REC 70-03, Annex 1, band g1; "f1" in editions before 2026) | Ukraine (NCEC decision 81 of 12.02.2025, row РІ 50-3-1, as amended by decision 212 of 06.05.2026) |
|---|---|---|
| Frequency | 433.050–434.790 MHz | 433.04–434.79 MHz |
| Power | 10 mW ERP | 10 mW ERP |
| Duty cycle | ≤10% | <10% |
| Channel spacing | no requirement | no requirement |
| LBT | not required | not required |

Implement: rolling one-hour airtime budget per node capped at 10%; a
single transmission never exceeds 1 s (SF5 packets are ≤140 ms anyway).
TX power setting such that ERP ≤ +10 dBm with the fitted antenna: with the
stock antenna (≈0 dBd) that is +10 dBm at the connector.

The 868 MHz band was rejected for Ukraine (no 500 mW sub-band; 868.0–868.6
MHz is 14 dBm at <1%), and 2.4 GHz for range. 433 MHz is the only band
with identical rules in both jurisdictions.

---

## 4. Physical layer

| Parameter | Value | Note |
|---|---|---|
| Modulation | LoRa (CSS) | SX1262 native; `lora-phy` Rust driver exists |
| Spreading factor | 5, fixed network-wide | SF5/SF6 are SX126x-only; all nodes are SX126x. Compile-time constant, changeable only for bench work |
| Bandwidth | 125 kHz | |
| Coding rate | 4/5 | |
| Raw bit rate | 15.6 kbps at SF5 | 5.5 kbps at SF7 |
| Symbol time | 0.256 ms at SF5 | |
| Preamble | 8 symbols (≈2 ms) + sync; **no long preambles** except for duty-cycled leaves (§10.5) | |
| 255-byte packet | ≈140 ms at SF5 | ≈400 ms at SF7 |
| Sensitivity | ≈−120 dBm at SF5 | ≈−124 dBm at SF7 |
| Noise floor (125 kHz, NF≈5 dB) | ≈−118 dBm | SF5 signals near sensitivity sit below the floor: RSSI cannot see them, CAD can |
| Sync word | private (not the LoRaWAN 0x34) | keeps LoRaWAN/Meshtastic from decoding us and vice versa |
| CRC | on (LoRa hardware CRC) | |
| MTU | 255 bytes | SX126x maximum LoRa payload |
| Range, relay↔handheld, street | ≈750 m at SF5, ≈1 km at SF7 | |
| Range, relay↔handheld, indoors | ≈300 m at SF5, ≈400 m at SF7 | |
| Range, rooftop↔rooftop | several km | rooftop relays hear ≈3 km; this sets the collision domain |

Why SF5 rather than GFSK: about 3× the bytes per airtime of SF7 for a
quarter less range, with the existing driver, hardware FEC/CRC/whitening,
channel activity detection, and ~15 dB of chirp processing gain against
the key fobs and LPD433 handhelds that share the band. GFSK at 50 kbps
would give a further 3× at 40% of the range and would need a modem driver
and an FEC written from scratch. Rejected for now; nothing above the modem
would change if it is revisited.

SF is a network-wide constant because an SX1262 receives one SF at a time,
so a mixed-SF network partitions. The network starts at SF5 and stays
there: an SF7 start would need a flag-day migration later, and the
deployment is expected to begin at a density where SF5 range suffices
(indoor coverage at ≈9.6 relays/km², §13). The SF7 column in the table
above is kept for comparison only.

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

There is no fixed colouring and no position-derived channel. Capacity
analysis (§13) showed that in this regime, where a rooftop relay's hearing
radius (~3 km) is far larger than a cell (~700 m), a fixed hex colouring
only partitions load evenly; measured selection does the same and also
adapts to hot spots and foreign interferers.

---

## 6. Home channel selection

Every node has one **home channel**, on which it listens whenever it is
not transmitting. All traffic *to* a node is sent on that node's home
channel.

### 6.1 Busy estimate

Per channel, maintain `busy[c]`, an exponential moving average of a binary
busy indicator:

- **Sweep:** once per second, when the modem reports no preamble detected
  or packet in progress: retune through all 13 channels, one RSSI reading
  each (≈0.3 ms dwell after retune), retune home. ≈5 ms, ≈0.5%
  off-channel.
- **Cost, and why it is not worth optimising:** a packet whose preamble
  overlaps the 5 ms gap is lost and retransmitted after the ACK timeout.
  With the 8-symbol preamble that is ≈0.5% of arrivals, well below CSMA
  collision loss. Spreading the 13 samples across the second does not
  help by itself: the lost window per gap is `g + 2D − P` (gap, modem lock
  requirement, preamble length), and with P ≈ 2D thirteen small gaps lose
  as much as one burst. Spreading plus a 12-symbol preamble would remove
  the loss entirely, but costs 1 ms on every packet, ≈3% of airtime on a
  text plus its ACK, six times the loss it prevents. Rejected. If the
  loss ever matters, sweep every 2 s instead.
- **Indicator:** 1 if RSSI > noise floor + 6 dB (≈−112 dBm), else 0.
  Average the indicator, not the dBm: the quantity of interest is the
  fraction of time listen-before-talk would refuse.
- **Window:** α ≈ 1/600 per sample (≈10 minutes at one sample per channel per second).
  One sample per heartbeat interval is too sparse; 60 samples per window
  gives ±4 points on a 10% channel, the same size as the differences the
  rule acts on.
- **Home channel:** additionally count the node's own receive time
  (preamble detected to packet end, from modem IRQs) as busy, so the home
  estimate is better than the sampled ones. That bias is in the safe
  direction for a "should I move" decision.
- **Noise floor:** calibrate per device at boot as a low percentile of RSSI
  readings over the first rotation; recalibrate slowly.

Foreign transmitters (key fobs, LPD433 handhelds) count exactly like peers.
Peer counting from heartbeats was considered and rejected: two neighbours
on one channel that hear each other share it cleanly under LBT; what hurts
is busy time at the receiver, and RSSI measures that directly.

### 6.2 Boot

Listen on a random channel for one full rotation period (≈150 s),
collecting sweeps and heartbeats. Then set home to `argmin busy[c]`,
breaking ties randomly (±0.01). Nodes that hear nothing must not all pick
channel 0. A config override exists for bench work.

### 6.3 Move rule

Move only when `busy[home] − min_c busy[c] > 0.05` continuously for
10 minutes; then wait a uniformly random 0–5 minutes, re-check, and move
to `argmin` with random tie-break. Immediately after retuning, send **one
heartbeat on the old channel** (it carries the new channel): that retunes
the node's leaves and any neighbour parked on the old channel at once.
No other announcement. Neighbours that miss it learn the new channel from
the next rotation (≤130 s) or from ACK failure (§9.3).

Hysteresis exists to make moves rare, not to announce them. Without it,
every rooftop relay in a domain sees the same ranking and jumps together.

---

## 7. Heartbeat

Every node that forwards (relay role) sends a heartbeat. Leaves do not
(§10).

| Field | Bits | Content |
|---|---|---|
| type | 4 | 0x1 heartbeat with surveyed position, 0x2 heartbeat with GPS-derived position |
| channel | 4 | home channel index 0–12 |
| lat | 25 | WGS-84 latitude, fixed point: 180° / 2²⁵ ≈ 0.6 m |
| lon | 26 | WGS-84 longitude, fixed point: 360° / 2²⁶ ≈ 0.6 m at the equator, 0.4 m at 50° N |
| spare | 5 | zero |

8 bytes, ≈12 ms on air at SF5 (a 15-byte heartbeat would be ≈15 ms, so
this cuts heartbeat load by a fifth). No node ID: a relay's position
**is** its identity (§8.1). No sequence number: there is no gossip to age
out, relays do not move, and an unsigned counter adds nothing. No flags
byte: the variants that matter are type codes.

The position is global: one firmware and one app everywhere, no
per-deployment origin. A 1 m grid over the Earth's 5.1 × 10¹⁴ m² needs
49 bits, so 51 bits in 7 bytes is within 2 bits of the minimum;
decimetre precision would need 58 bits. S2 cells, geohash and UTM were
considered and all land at 50–51 bits for 1 m; plain fixed-point
latitude/longitude is the one with trivial distance math (§11).

**Timing:** interval uniformly random in [5, 15] s, mean 10 s.
**Channel:** steps deterministically, `k ← (k + 1) mod 13` per heartbeat,
with a random start. The time is randomised, the channel is not: a
deterministic cycle reaches every neighbour once per 13 heartbeats
(≈130 s); a random channel per heartbeat would need ≈41 heartbeats on
average to cover all 13 (coupon collector).
**Procedure:** retune to channel k, listen-before-talk (CAD + RSSI, ≈1 ms),
if busy retry after a random 0–200 ms up to three times, then skip; send;
retune home. The node is deaf for ≈15 ms per heartbeat, ≈0.2%.

Why this works with no scanning: every node sits still on its home
channel; every neighbour's heartbeat rotates through all channels; so each
neighbour lands on your channel once per rotation. A joiner, or a leaf,
listening on any one channel hears every relay in range within one
rotation. (This is the Bluetooth LE advertising pattern.)

Per-channel heartbeat load in a 270-relay rooftop domain: ≈2.5%.

---

## 8. Neighbour table

### 8.1 Relay identity is position

A relay has no node ID. Its identity is `(x, y)` from its heartbeat, which
is also its address (§9.1); the two were redundant. Consequences:

- A relay **freezes** its position once accepted: a surveyed entry from
  the app, or the first plausible GPS fix. Later GPS readings never update
  it. A changed position is a new identity and needs an explicit reset
  from the app.
- At boot, during the one-rotation listen (§6.2), a relay that hears its
  own exact position in another relay's heartbeat offsets itself by 1 m
  and listens again. Two relays at one position would in any case behave
  like one relay with two radios: both ACK, both forward, and duplicates
  are suppressed downstream. The rule is tidiness, not correctness.
- Hop references in packets (`dst_hop`, `dst_pos`, `relay_pos`) are the
  full 7-byte position. Hashing them to 2–3 bytes was considered and rejected:
  the saving is one 20-bit LoRa block, ≈1.3 ms per packet, not worth a
  collision case. The DFS visited list is the exception: it carries a
  16-bit hash per entry so that a 16-deep list is 32 bytes rather than 112,
  and a collision there only skips a candidate.
- Leaves keep a 4-byte ID because they move. The app generates it at
  random; a collision matters only between leaves attached to the same
  relay (≈2 × 10⁻⁵ at 400 leaves). No provisioning step exists for either
  role.

### 8.2 Table

Per node, up to 64 entries:

| Field | Content |
|---|---|
| x, y | from heartbeat; the entry key and the neighbour's identity |
| channel | from heartbeat (the channel to transmit to this neighbour on) |
| last_heard | local monotonic time |
| rssi_ema | for leaf relay choice and tie-breaking |
| fail_count | consecutive ACK failures |

- **Expire** after 3 rotations (≈400 s) without a heartbeat.
- **Demote** on ACK failure: `fail_count += 1`; a neighbour with
  `fail_count ≥ 2` is skipped for next-hop selection until its next
  heartbeat resets it. Liveness by heartbeat alone takes minutes; a failed
  forward must demote at once.
- **Eviction** when full: least recently heard. (Open item O4: rooftop
  relays hear ~270 relays; a geographically diverse set is better for
  greedy routing than the 64 most recent.)

No neighbour table entry for leaves; leaves live in the leaf table (§10).

---

## 9. Forwarding

### 9.1 Addresses

A destination address is `(x, y, leaf_id)`: the position of the relay the
recipient is attached to, plus the recipient's 4-byte ID. Relays have
`leaf_id = 0`, i.e. the relay itself, so a relay's address and its identity
are the same six bytes (§8.1). A sender learns an address from a
previous message (the sender address is carried in the encrypted payload),
from a QR code, or from a directory; the network provides no lookup.
Geocast to an area is a natural extension (open item O7).

### 9.2 Greedy forwarding

Forward to the neighbour (relay role, not demoted) that minimises
Euclidean distance to the destination position. If no neighbour is closer
than self, enter DFS void recovery: forward to the nearest non-demoted
neighbour not in the packet's visited list, appending self; a node with no
unvisited neighbour returns the packet to the hop it came from, which tries
its next candidate. The visited list is carried in the packet and bounded;
when it fills, the packet is NACKed. DFS is for geographic voids only; it is never used
to search for an ID.

A relay **owns** a destination when it is the closest relay the packet can
reach: no non-demoted neighbour is closer to `(x, y)`. The owner looks
`leaf_id` up in its leaf table and delivers (§10.3), or queues (§10.4). If
`leaf_id = 0`, the owner is the destination.

If the owner is not within 50 m of `(x, y)`, the addressed relay has
probably died or moved. The owner may ask its direct neighbours once
whether they hold `leaf_id` (one hop, one round); if none does, queue and
NACK. Nothing wider: a wider search is a flood.

### 9.3 Per-hop transfer

1. Retune to the next hop's home channel.
2. Listen-before-talk: CAD, then RSSI. Random backoff 0–50 ms if busy,
   retry; give up after 500 ms and reselect the next hop.
3. Transmit. The packet carries the sender's home channel (§14).
4. Retune home immediately. Do not dwell on the other channel.
5. Wait for the ACK **on the home channel**: timeout 100 ms (ACK airtime
   ≈10 ms plus the receiver's retune and listen-before-talk).
6. On timeout: random backoff, retry, up to 3 attempts; then demote the
   neighbour and reselect.

The receiver retunes to the sender's channel, listen-before-talk, sends
the ACK, retunes home. ACK: first byte, msg_id (5 bytes, ≈10 ms). It has
no target address; the sender matches it by `msg_id` against its
outstanding transmissions.

The rule this gives is uniform: **a node receives everything, ACKs
included, on its home channel, and leaves it only for its own transmit
bursts.** A leaf's home channel is its relay's, so leaf ↔ relay traffic
and its ACKs never retune at all. A relay is deaf for ≈15 ms per packet it
forwards or ACKs; a sender that gets no ACK simply retries, and the
duplicate is suppressed by `msg_id`. Accepted cost of one radio.

### 9.4 Duty cycle cap

At SF5, 10% of airtime is ≈180 B/s per relay. A busy relay saturates
before its channel does. Greedy forwarding does not load-balance; central
rooftop relays will be the first to hit this. When the budget is
exhausted the relay stops forwarding (NACKs with reason `busy`) but keeps
heartbeating.

---

## 10. Leaves

A leaf is a carried node on battery. It **never heartbeats on the rotating
schedule** (at household density that would be hundreds of percent of
airtime on every channel) and never forwards.

### 10.1 Relay choice

Camped on its relay's home channel, a leaf hears every relay in range once
per rotation (≈130 s). It keeps `rssi_ema` per relay and switches to
another relay when that relay has been stronger by ≥6 dB for two
consecutive sightings. Handover is make-before-break. Walking pace gives
two to four sightings per relay per cell; vehicles cannot track and rely
on the delivery queue plus re-attach on arrival.

A leaf that hears no relay for 3 rotations sweeps: it stays on any channel
and waits; every relay in range will appear within one rotation.

### 10.2 Attach and keepalive

`ATTACH {type, leaf_id, relay_pos, flags}` unicast to the relay on its home
channel with LBT and ACK. Flags carry the leaf's receive mode (§10.5).
Repeat every 60 s as keepalive, and immediately on relay change. The
relay's **leaf table** (`leaf_id, last_seen, rssi, rx_mode`) is separate
from the neighbour table; entries expire after 3 missed keepalives
(180 s). Leaf table size: 256 entries per relay (≈80 leaves per relay
expected at indoor density in Kyiv with 10% adoption; 400 at 50%).

Keepalive cost per channel at 80 leaves per relay: ≈2%; at 400: ≈12%.
This is why the interval is 60 s and not 10 s.

### 10.3 Delivery

The owning relay transmits the message to the leaf on the relay's own home
channel (the leaf is camped there), LBT first, and waits for the leaf's
ACK. Preamble length per the leaf's declared receive mode (§10.5).

### 10.4 Delivery queue

Bounded, not a mailbox: up to 64 messages per relay, TTL 10 minutes.
Retry delivery on the leaf's next keepalive and every 60 s. On expiry,
route a NACK to the sender address. Rationale: a queued message costs no
airtime, a failed delivery costs a NACK and a retry across the city; the
common case is a phone in a pocket, a basement, or the metro for a few
minutes. RAM is not the constraint (64 × 255 B = 16 KB of 256 KB).

A handoff pointer (new relay notifies the old one, which forwards queued
messages) was considered and dropped as complexity: the queue plus the
sender's own retry covers the moved-leaf case. Overhearing by neighbouring
relays does not work because neighbouring relays are on different
channels.

### 10.5 Leaf receive mode

| Mode | Leaf current | 850 mAh | Preamble on packets to the leaf | Hears heartbeats |
|---|---|---|---|---|
| Always listening | ≈5.8 mA | ≈6 days | 8 symbols | yes |
| Duty-cycled (sleep, CAD every 500 ms) | ≈0.3 mA + sleep | months | ≥512 ms | no: blind to relay changes |
| Hybrid: full listen one rotation every 10 min, CAD between | ≈1.3 mA | ≈25 days | ≥512 ms | during the awake rotation |

The leaf declares its mode in ATTACH flags; the relay sets the preamble
accordingly. Long preambles are cheap in airtime because deliveries are
rare (≈0.5% of a channel per day at 80 leaves × 10 messages). Default:
always listening while USB/charging, hybrid on battery. GPS off after a
fix, and off after 10 minutes without a fix (searching under jamming is
≈27 mA, ≈1.3 days).

---

## 11. Position, time, phone link

**Position.** GPS (L76K) is used for position only and is treated as an
untrusted prior: accept a fix only if plausible (within 50 m of the last
accepted position for a stationary relay, neighbour consistency, C/N0
sanity). Rooftop and windowsill relays use surveyed or user-entered
coordinates from the app, which override GPS. Positions on the wire are
global WGS-84 latitude and longitude as fixed point, 25 + 26 bits in
7 bytes, ≈0.6 m. Routing needs distances, not coordinates: each node
converts every position it handles into metres in a local tangent plane
centred on itself, `Δy = Δlat · R` and `Δx = Δlon · R · cos(lat_self)`,
with `cos(lat_self)` a per-node constant. That is accurate to better than
0.1% out to hundreds of kilometres, far beyond any hop. Neighbour-table
entries store both the wire form (identity) and the local form
(distance). No UKF, no trilateration: every relay is an anchor.

**Time.** No network time. GPS time is never used. All timers are local
monotonic. Nothing in the protocol depends on two nodes agreeing on a
clock.

**Phone link.** BLE 5.0 GATT service on the nRF52840 (nrf-softdevice S140
or TrouBLE): characteristics for send, receive-notify, node status,
position set, role set. WiFi was rejected (power, no WiFi on the board).
The app owns identity and end-to-end encryption; the node sees opaque
payloads. The sender's address `(x, y, leaf_id)` travels inside the
encrypted payload so that only the recipient learns where the sender is.
Crypto scheme: open item O1.

---

## 12. What the protocol deliberately omits

Each of these is a familiar mesh mechanism that was considered and left out,
with the reason, so that nobody re-proposes it without a new argument.

| Omitted | Instead | Why |
|---|---|---|
| Neighbour gossip in heartbeats and a global node table | 7-byte position-only heartbeat; routing uses only direct neighbours | gossip convergence does not scale to thousands of relays at LoRa rates; addresses are positions, so there is nothing to look up |
| Sequence numbers in heartbeats | none | they exist to age gossip; without gossip they carry nothing |
| A node ID on every node | relays are identified by position; only leaves carry a 4-byte ID, generated by the app | relays do not move, and an ID would duplicate the address |
| Slotted transmission on a shared schedule | always listen, transmit as soon as listen-before-talk clears | no time sync exists, and slots add latency |
| A protocol time unit scaled by the duty cycle | τ = one packet airtime (MTU at SF5, ≈140 ms); duty cycle is a separate per-node budget (§9.4) | scaling τ by the duty cycle inflates every protocol timer tenfold |
| Cooperative position estimation (filters, trilateration, calibration rounds) | every relay knows its position from GPS or survey and is an anchor | §11 |
| One table for all nodes | a neighbour table for relays (§8) and a leaf table for attached leaves (§10) | thousands of leaves must not enter routing |
| Heartbeats on one fixed channel | rotating channel, random 5–15 s interval | §7 |
| ID-addressed delivery with a lookup | position + leaf ID; the owning relay resolves the ID locally | §9 |
| Planar positions in a per-deployment frame | 51-bit global latitude/longitude on the wire; local tangent-plane metres for distance | no per-deployment origin; one firmware everywhere |
| Long bans of failed neighbours | demotion until the neighbour's next heartbeat (§8.2) | the heartbeat is the liveness signal; a timer would duplicate it |

---

## 13. Capacity and sizing (rationale, not spec)

Kyiv model: ≈840 km², ≈400 km² built-up, ≈3 M people. Indoor coverage
needs ≈9.6 relays/km² (≈3,800 relays); outdoor-only ≈1.9/km² (≈750).
Rooftop relays hear ≈3 km, so a collision domain is ≈28 km² with ≈270
relays, ≈14 domains across the built-up area.

One 125 kHz channel at SF5 carries 1.8 kB/s raw and ≈640 B/s under
carrier sense (≈35% usable). A 100-byte text over 8 hops with ACKs is
≈0.56 s of channel time.

| Plan | Heartbeat load per channel | City-wide data | Texts/s | Texts per user per day at 300k users |
|---|---|---|---|---|
| Single channel, 60 s heartbeats | 8% | 7 kB/s | 7 | 2 |
| 7-colour fixed grid, beacons on all colours | 11–14% | 37 kB/s | 37 | 11 |
| **13 channels, measured selection, rotating heartbeat (this design)** | **≈2.5%** | **≈85 kB/s** | **≈80** | **≈23** |
| + per-packet power control (later) | | ≈160 kB/s | ≈150 | ≈45 |

These are order-of-magnitude; the hearing radius and hop count are each
uncertain by 2×. The centre of the city is the hot spot, and the per-relay
10% cap (§9.4) binds there first.

Rejected for capacity reasons, so that nobody re-proposes them:

- **GFSK 50 kbps:** 3× more per channel than SF5, at 40% range, with a
  modem driver and FEC to write. Revisit only if cells saturate at SF5.
- **DSSS/CDMA over the whole band:** the SX1262 has no I/Q path; a
  correlator board costs an order of magnitude more and draws 100+ mA; at
  1.74 MHz and 15 kbps the processing gain is 20 dB, so a transmitter
  300 m away blocks a wanted one 1 km away (near-far). Weber–Andrews: for
  random meshes without power control, M channels beat M-chip spreading by
  M^(1−2/α), 5–10× here. A DSSS radio survives that near-far ratio only
  with ≈37 dB of processing gain, which this band cannot give.
- **FHSS:** common-schedule hopping is single-channel capacity; per-receiver
  sequences equal static channels plus time sync; per-packet random
  channels with scanning receivers cost ≥20 ms preambles on every packet.
  No regulatory benefit at 433 MHz (duty cycle is modulation-agnostic).
- **Fixed hex-grid colouring with beacons on all colours:** 49% of every
  channel at indoor density; and in this regime it only partitions load,
  which measurement does as well while adapting to hot spots.
- **Scheduled neighbour listening with long-preamble beacons:** works, but
  needs per-neighbour timing state and a 20 ms preamble; the rotating
  heartbeat gets the same result with none of it.
- **Home-relay registration (Mobile IP style):** registration traffic
  scales with users; position addressing has no registration.
- **Leaf heartbeats every 10 s:** collisions happen at the shared relay
  regardless of leaf range; 72% of a channel at 400 leaves per relay.
- **No delivery queue, rely on overhearing:** neighbouring relays are on
  different channels and overhear nothing.

---

## 14. Packet formats (proposal; implementer finalises in O2)

The first byte of every packet is `type:4 | aux:4`; `aux` is the home
channel in heartbeats and type-specific flags elsewhere. Positions are
7 bytes: 25-bit latitude, 26-bit longitude, 5 spare bits (§7). LoRa
hardware CRC covers everything.

```
HEARTBEAT 0x1/0x2: first(1) pos(7)                                          = 8
          0x1 surveyed position, 0x2 GPS-derived position; aux = channel
ACK       0x3: first(1) msg_id(4)                                           = 5
          sent on the original sender's home channel; aux unused
ATTACH    0x4: first(1) leaf_id(4) relay_pos(7)                             = 12
          aux = leaf receive mode (§10.5)
DATA      0x5: first(1) dst_hop(7) msg_id(4) dst_pos(7) dst_leaf(4) hops(1)
               visited_len(1) visited(2×n) payload                          = 25 + 2n + payload
          aux = sender's home channel, so the receiver knows where to ACK
NACK      0x7: DATA layout; payload = reason(1) + original msg_id(4)
DELIVER   0x6: first(1) dst_leaf(4) msg_id(4) payload                       = 9 + payload
          relay → leaf on the relay's own channel; leaf ACKs on the same channel
```

`dst_hop` lets every node on a channel discard packets not addressed to it
after 8 bytes. There is no `src_hop`: the ACK goes to the channel in
`aux` and is matched by `msg_id` on the sender side, demotion is a
sender-side decision, and DFS backtracking uses the visited list. `msg_id` is a 32-bit random value chosen at
origination and kept across hops; duplicate suppression keeps a recent-RX
window on it. Sender address is inside the encrypted
payload, not in the header.

---

## 15. Implementation notes

**Rust stack:** `embassy-nrf` (nRF52840), `lora-phy` from git (SF5, CAD,
RSSI; roadmap D4), TrouBLE with the Nordic controller for BLE (D5), the
stock UF2 bootloader for flashing (D6). The protocol core is sans-IO and
owns its routing and tables (D1, D3; `core-interface.md`). Crate layout is
D7.

**RAM budget (256 KB):** neighbour table 64 × 32 B = 2 KB; leaf table
256 × 16 B = 4 KB; delivery queue 64 × 272 B = 17 KB; recent-RX window
256 × 8 B = 2 KB; busy estimates 13 floats; SoftDevice 64 KB; Embassy +
stacks ≈16 KB. Comfortable.

**Bring-up order** (each step is a demonstrable milestone on real boards):

1. `lora-phy` at SF5/125 kHz on two T-Echo 433: TX, RX, CAD, RSSI read,
   retune time measured. **Bench item C1** (adjacent-channel rejection)
   here.
2. LBT + rotating heartbeat + neighbour table on three boards: each board
   lists the other two with channel and position within one rotation.
3. Busy estimate + home-channel selection: inject a CW interferer on one
   channel, watch boards avoid it; verify no oscillation with three boards
   that all hear each other.
4. Greedy forwarding + per-hop ACK across three relays in a line, with
   attenuators or distance so A cannot hear C. Demotion on a powered-off
   middle relay.
5. Leaf attach, keepalive, delivery, queue, NACK. **This is the loop every
   user feels; prototype it before anything clever.**
6. BLE GATT + minimal phone app.
7. Field test: a dozen relays on windowsills across a few blocks.

**Test rig:** at least three T-Echo 433, SMA attenuators, one CW/LoRa
interferer source (a fourth board is fine), a USB logic/UART sniffer for
timing.

---

## 16. Open items

| # | Item | Owner/when |
|---|---|---|
| O1 | End-to-end crypto scheme in the app (key exchange, address-in-payload format) | before step 6 |
| O2 | Finalise packet formats (§14), msg_id scheme, visited-list bound | step 4 |
| O3 | Exact thresholds: busy threshold margin (6 dB), move hysteresis (0.05 / 10 min), demote count, ACK timeout | tune in steps 3–4 |
| O4 | Neighbour-table eviction policy for rooftop relays that hear hundreds | step 2 |
| O5 | Bench item C1: adjacent-channel rejection at 125 kHz spacing; fallback 11 × 150 kHz | step 1 |
| O7 | Geocast to an area (address = position + radius, leaf_id = broadcast) | later |
| O8 | Per-packet TX power control (prefer short hops, cut rooftop hearing radius): ≈2× capacity | later |
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
- EN 300 220-2 for polite-access shapes (not required in this band).
- Semtech SX1261/2 datasheet: LoRa sensitivity table, CAD, RSSI, SF5/SF6.
- Weber, Andrews, Yang, de Veciana, "Transmission capacity of wireless ad
  hoc networks with outage constraints", IEEE Trans. IT 2005 (FH vs DS).

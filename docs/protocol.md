# bark protocol: Civilian 433 MHz Mesh

**Status:** live specification, 2026-10-17. Decisions that refine it are in
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
| Medium access | Always listening on the home channel, leaving it only to transmit; acknowledgements (ACKs) come back on the sender's home channel; listen-before-talk before every transmission; transmit as soon as the channel is clear; no time slots |
| Heartbeat | A node's periodic 9-byte broadcast of its ID, position and home channel: a relay's every 5–15 s at random, on a channel that steps through all 13 in order; a sleepy node's every 150–300 s, on its home channel only |
| Neighbour discovery | Passive: a node hears every neighbour's heartbeat once per 13 heartbeats (~130 s) on its own home channel; no scanning, no schedules |
| Routing | Forward to the neighbour closest to the destination **position**; route around gaps by depth-first search; ACK every hop; a neighbour that fails to answer is banned for a while, and after two failures in a row is used only as a last resort; sleepy nodes forward only where no relay is in reach |
| Addressing | A position plus a 4-byte node ID: a relay's own position, a sleepy node's anchor's; no registration, no network-wide tables |
| Nodes | One kind, with one setting. **Relay**: a node that is not sleepy, fixed, on mains, USB or solar, that heartbeats, forwards, and holds messages for the sleepy nodes it hears. **Sleepy node**: a carried node on battery that chooses a relay as its anchor, heartbeats rarely on the anchor's channel, listens briefly after each heartbeat, and forwards only where no relay is in reach |
| Phones | Bluetooth LE (BLE) GATT link to the node; the app encrypts end to end; the sender's position travels encrypted in the payload |
| Position | WGS-84 latitude/longitude, 51-bit fixed point in 7 bytes (≈0.6 m), the same everywhere on Earth; distances computed in a flat local approximation at each node; GPS when available, else entered by the user; relays use surveyed coordinates; never used for timing |
| Time | No network time synchronisation of any kind |

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

Every node runs the same protocol with one runtime setting, **sleepy**.
Carried on battery: sleepy ([sleepy nodes](#sleepy-nodes)). Plugged into USB on a
windowsill, or in a solar box on a roof: not sleepy, a **relay**.

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
holds every hour within 10% ([duty cycle cap](#duty-cycle-cap)), no transmission longer than
1 s (an SF5 packet is at most 139 ms), and a transmit power that gives
at most +10 dBm ERP with the fitted antenna: +10 dBm at the connector
with the stock antenna (≈0 dBd gain).

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
| Preamble | 12 symbols (3.1 ms) + 6.25 symbols of sync; **no long preambles** | 12 is the SX1262 minimum at SF5 and SF6 (datasheet §6.1.4) |
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
GFSK: [capacity and sizing](#capacity-and-sizing).

SF is network-wide because an SX1262 receives one SF at a time, so a
mixed-SF network splits into parts that cannot hear each other. Starting
at SF7 would force a network-wide switch later; the deployment is expected
to start at a density where SF5 range suffices (indoor coverage at ≈9.6
relays/km², [capacity and sizing](#capacity-and-sizing)).

---

## Channel plan

Thirteen channels of 125 kHz with 57.5 kHz guard at each band edge:

```
f_k = 433.170 MHz + k × 0.125 MHz,   k = 0..12
      433.170, 433.295, 433.420, 433.545, 433.670, 433.795, 433.920,
      434.045, 434.170, 434.295, 434.420, 434.545, 434.670 MHz
```

Channels are not assigned by a fixed grid or derived from position: each
node measures how busy each channel is and chooses
([home channel selection](#home-channel-selection)); why not a grid: [capacity and sizing](#capacity-and-sizing). Channel 6 is
centred on 433.920 MHz, the busiest spot in the band (key fobs, sensors);
the choice avoids it wherever it is busy.

**Bench item C1:** verify SX1262 adjacent-channel rejection at 125 kHz
spacing with a neighbour at full power. If it is inadequate, fall back to
11 channels at 150 kHz spacing (same end centres, 433.170–434.670 MHz).

---

## Home channel selection

Every node has one **home channel**, on which it listens whenever it is
not transmitting. All traffic *to* a node is sent on that node's home
channel.

### Busy estimate

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
  (≈1 ms; measure in bring-up step 1, [implementation notes](#implementation-notes)). A packet is lost if its
  preamble starts too late before the gap for the modem to have locked,
  or too early in it to leave `D` after it: a window of `g + 2D − P` per
  gap of length `g`, with `P` the preamble, 12 symbols (3.1 ms, [physical layer](#physical-layer)). For
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
  Packets addressed to the node (DATA), the ACKs of its own
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
  (`core-interface.md`, Radio, rules 7 and 8): a reading inside a packet addressed
  to the node is discarded, one inside any other packet counts. A packet
  for the node that fails its CRC cannot be told from someone else's and
  counts.
- **Noise floor:** calibrate per device at boot as a low percentile of RSSI
  readings over the first rotation; recalibrate slowly.

Foreign transmitters (key fobs, LPD433 handhelds) count exactly like
peers. What hurts a node is busy time at its own receiver, and RSSI
measures that directly; counting neighbours per channel would not, since
two neighbours that hear each other share a channel cleanly.

### Channel at boot

Listen on a random channel for a worst-case rotation, 195 s
([heartbeat](#heartbeat)), so that every relay in range has certainly been heard once, collecting
sweeps and heartbeats. Then set home to `argmin busy[c]`. Channels whose
busy value is within 0.01 of the lowest count as tied, and the choice
among them is random, so that nodes that hear nothing do not all pick
channel 0; a sleepy node moves to its anchor's channel once it has one
([relay choice](#relay-choice)). A config override exists for bench work.

### Channel moves

Move only when `busy[home] − min_c busy[c] > 0.05` continuously for
10 minutes; then wait a random 0–5 minutes, re-check, and move to
`argmin` with random tie-break. Immediately after retuning, send **one
heartbeat on the old channel** (it carries the new channel): that retunes
the sleepy nodes anchored to it and any neighbour parked on the old
channel at once.
No other announcement. Neighbours that miss it learn the new channel from
the next rotation (≤130 s) or from ACK failure ([hand-over](#hand-over)).

The hysteresis keeps moves rare. Without it, every rooftop relay in range
of the same interference sees the same ranking and they all jump together.

---

## Position and time

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
reorder only near-ties between candidate hops. Every node knows its own
position; nothing is estimated cooperatively.

The position is global: one firmware and one app everywhere, no
per-deployment origin. A 1 m grid over the Earth's 5.1 × 10¹⁴ m² needs
49 bits, so 51 bits in 7 bytes is within 2 bits of the minimum;
decimetre precision would need 56 bits. Plain fixed-point latitude and
longitude keeps distance math trivial.

**Short position.** The heartbeat carries a node's position in 4 bytes:
the low 16 bits of its latitude count followed by the low 16 bits of its
longitude count, big-endian. One count is 180° / 2²⁵ ≈ 0.6 m, and the
low 16 bits repeat every 65 536 counts: 39 km of latitude, and
39 km × cos(latitude) of longitude (25 km at 50° N), far beyond any
hearing radius (≈3 km, [physical layer](#physical-layer)). A receiver rebuilds the full
position as the one candidate within half a period of its own on each
axis. That holds down to latitudes near 80° N or S, where the east-west
period shrinks toward the hearing radius. DATA `dst_pos` stays 7 bytes
because in a large city a node may be more than half a period from the
destination (roadmap D14).

**Time.** No network time. GPS time is never used. All timers are local
monotonic. Nothing in the protocol depends on two nodes agreeing on a
clock.

---

## Heartbeat

Every node sends heartbeats. This section describes a relay's; a sleepy
node's are rarer and stay on its home channel ([beats](#beats)).

| Field | Bits | Content |
|---|---|---|
| type | 4 | 0x1 from a relay with a surveyed position, 0x2 from a relay with a GPS-derived position, 0x8 from a sleepy node |
| channel | 4 | home channel index 0–12 |
| id | 32 | the node's ID ([node identity](#node-identity)) |
| position | 32 | the node's short position ([position and time](#position-and-time)) |

9 bytes, 11.8 ms on air at SF5 ([physical layer](#physical-layer)): at coding rate 4/5 the ninth
byte fits in the eighth's block of symbols, so 9 bytes take the 20
payload symbols of 8, and naming the sender costs no airtime.

- **Timing:** interval uniformly random in [5, 15] s, mean 10 s.
- **Channel:** steps `k ← (k + 1) mod 13` per heartbeat, from a random
  start. The time is random, the channel order is not: a fixed cycle
  reaches every neighbour once per 13 heartbeats (≈130 s), where a random
  channel per heartbeat would need ≈41 heartbeats on average to cover all
  13.
- **Procedure:** retune to channel k, listen-before-talk (CAD + RSSI,
  ≈1 ms); if busy, retry after a random 0–200 ms up to three times, then
  skip; send; retune home. The node is deaf for ≈15 ms per heartbeat,
  ≈0.2%.

This is why no scanning is needed: every node sits on its home channel,
every neighbour's heartbeat visits all channels, so each neighbour lands
on each node's home channel once per rotation. A newly started node, or a
sleepy node, listening on any one channel hears every relay in range
within one rotation.

Heartbeat load per channel among 270 rooftop relays that all hear each
other: ≈2.5% ([capacity and sizing](#capacity-and-sizing)).

---

## Neighbours

### Node identity

Every node has a 4-byte **node ID**: the low word of its chip's factory
device ID (nRF52840 FICR DEVICEID), the same at every boot, with no
setup step. It names the node wherever a packet names one: DATA
`dst_hop`, `src_hop` and `dst_id`, ACK `from` and `to` ([packet formats](#packet-formats)). Two
nodes share an ID with probability about n² / 2³³ among n nodes; it
matters only among the nodes one node keeps in its tables, 64 neighbours
and 256 sleepy nodes ([heard table](#heard-table)), 1.2 × 10⁻⁵.

A relay **freezes** its position once accepted: a surveyed position from
the app, or the first plausible GPS fix. Later GPS readings never update
it. A changed position is a new address ([addresses](#addresses)), its own and that of
every sleepy node anchored to it, and needs an explicit reset from the
app. Two nodes at one position are two nodes.

### Neighbour table

Per node, up to 64 entries, by node ID:

| Field | Content |
|---|---|
| id | from its heartbeat; the entry key |
| position | from its heartbeat, rebuilt from the short position ([position and time](#position-and-time)), and its local form for distances |
| sleepy | from its heartbeat's type |
| channel | the channel to transmit to this neighbour on: from its heartbeat, or from `aux` of a DATA packet it sent ([packet sender](#packet-sender)), whichever came last |
| last_heard | local monotonic time |
| rssi_ema | averaged RSSI of every frame heard from it: heartbeats, its DATA and its ACKs; each new frame counts half. Says whether it has margin |
| fail_count | consecutive ACK failures |
| banned_until | when its ban ends: the ban of its last refusal, or of its last failed hand-over, longer after each failure in a row ([refusal](#refusal), [hand-over](#hand-over) step 6) |

- **Expire** after 3 rotations (390 s) without a heartbeat or an ACK
  from it ([the ACK](#the-ack)); a sleepy node's entry after two beat periods, 600 s,
  as it beats at most every 300 s ([beats](#beats)).
- **Margin.** A neighbour has margin when `rssi_ema` is at least 4.1 dB
  above the noise floor ([busy estimate](#busy-estimate)). A 255-byte frame is then still lost no
  more than one time in a hundred, the standard the datasheet sets for
  sensitivity, after a fade of 4 dB. A neighbour without margin is a
  next hop only when no other is left ([next hop](#next-hop)). It stays in the table:
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
  `fail_count ≥ 2` is demoted until it ACKs a packet from this relay or
  three rotations (390 s) pass, and is a next hop only when no other with
  margin is left ([next hop](#next-hop)). Its heartbeats keep it in the table but do not
  clear the demotion: a heartbeat proves the
  neighbour is alive, not that it hears us, and a link that works one way
  only would otherwise cost every message the failed attempts again.
  Heartbeat expiry takes minutes; a failed forward must demote at once.
- **Eviction** when full and a relay not in the table is heard: the
  newcomer replaces the least recently heard demoted entry; failing that,
  the least recently heard entry if it has not been heard for a
  worst-case rotation, 13 × 15 s = 195 s ([heartbeat](#heartbeat)), so it has certainly missed
  a heartbeat on this channel; failing that, the newcomer is not added
  (roadmap D12). Replacing the least recently heard entry outright does
  not work for a rooftop relay that hears ≈270 relays ([capacity and sizing](#capacity-and-sizing)): about two
  heartbeats a second arrive on its channel, most from relays not in the
  table, so each entry would last about 64 / 2 ≈ 30 s and every failure
  count would be forgotten within a minute. Keeping incumbents holds an
  equally arbitrary 64, keeps their failure counts, and replaces exactly
  the entries that are failing or silent. Choosing a set spread around
  the relay, which would serve greedy routing better, is left to the
  simulator (open item O4). A sleepy newcomer takes only a free entry or
  a stale or demoted sleepy one; a relay newcomer replaces the least
  recently heard sleepy entry before applying the rule above, since
  sleepy nodes are next hops only where no relay is ([next hop](#next-hop)).

A sleepy node's heartbeat also fills the heard table, which holders use
to deliver to it ([heard table](#heard-table)).

---

## Forwarding

### Addresses

A destination address is a position and a node ID ([node identity](#node-identity)):
a relay's own position, or a sleepy node's anchor's ([relay choice](#relay-choice)),
and the recipient's ID. A sender learns an address from a previous
message (the sender's address is carried in the encrypted payload), from
a QR code, or from a directory; the network provides no lookup. Sending
to an area is open item O7.

The network carries no sender address and sends no failure notices. The
only acknowledgement in the protocol is the per-hop ACK
([hand-over](#hand-over)); whether a message arrived, and what to do if it did not,
is the app's business, with whatever acknowledgement it puts inside the
encrypted payload.

The app hands a node a message only when the node says it will take one,
and hears nothing about it afterwards: not the first hop's ACK, which a
user could take for delivery, and not a drop. From then on the node is
either sending the message or waiting to. Each node holds one message
of its app's at a time, until its first hop takes it. A node takes one
while it is running, holds none, its send queue and bucket have room
for a full frame on top of what they hold ([send queue](#send-queue)), and not every
neighbour is banned ([receiving DATA](#receiving-data)); for a sleepy node with a live anchor,
the anchor is not banned ([a sleepy node's own message](#a-sleepy-nodes-own-message)). A node whose neighbours are
busy keeps the message and offers it again ([a node's own message](#a-nodes-own-message)), so a
busy network makes its users wait rather than lose their messages at the
first step (roadmap D29).

### Duty cycle cap

At SF5, 10% of airtime is ≈180 B/s per relay, so a busy relay runs out of
budget before its channel fills. Forwarding to the neighbour nearest the
destination does not spread load; central rooftop relays will hit the cap
first.

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
it is not dropped for that. Two reserves stay in the bucket. Replies (ACKs that
take or refuse) leave one
heartbeat's airtime, 11.8 ms, which the bucket refills in 120 ms, far
less than the 5 s between heartbeats ([heartbeat](#heartbeat)), so heartbeats always
go. Forwarding, and the ACK that takes on a packet, leave the **reply
reserve**: the refusals the bucket refills while a sender retries an
unanswered hand-over, and the heartbeat. The retrying takes the two
retry waits, 306 + 612 ms, and two more attempts, each up to 500 ms of
listen-before-talk, a 138.6 ms full frame and the 100 ms ACK timeout
([hand-over](#hand-over)), 2.39 s in all; the bucket refills 9.83 % × 2.39 s = 235.5 ms
of it, ⌊235.5 / 14.4⌋ = 16 refusals, and with the heartbeat the reserve
is 242 ms. A relay takes new work into the regular room of its send
queue only if its bucket holds the reply reserve on top of the work's ACK
and of everything it has queued to send, and otherwise into a hop place
or not at all ([receiving DATA](#receiving-data)), so a hand-over
waits for the bucket only when retries or replies have spent it since
its packet was taken, or when its packet holds a hop place. A relay
below the reserve sends nothing but heartbeats and replies until its
bucket refills. Each sender it refuses bans it for 1.56 s ([refusal](#refusal)), so
a sender costs it at most one reply per ban. Without budget even for a
reply it stays silent, its neighbours' hand-overs to it fail, and they
ban it, longer each time, and route around it (roadmap D27, D29, D42).

### Hand-over

A **hand-over** sends one DATA packet to one neighbour until that
neighbour answers it or the relay gives up. A relay runs one hand-over
at a time; packets waiting their turn are held in its [send queue](#send-queue). The
answer is an ACK, which takes or refuses the packet and
names the message and both nodes ([the ACK](#the-ack)).

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
   ([packet formats](#packet-formats)).
4. Retune home immediately.
5. Wait for the ACK **on the home channel**: timeout 100 ms (ACK airtime
   14.4 ms plus the receiver's retune and listen-before-talk).
6. On timeout: wait ([retry wait](#retry-wait)) and retry, up to 3 attempts. Then
   the hand-over has failed: count a failure against the neighbour
   ([neighbour table](#neighbour-table): demoted after 2 consecutive failures) and **ban** it as a
   refusal does ([refusal](#refusal)), for every message. The ban after the k-th
   failure in a row is one refusal's ban doubled k − 1 times: 1.56 s,
   3.11 s, 6.2 s and so on, up to the bucket's refill, 61 s; an ACK from
   the neighbour resets its failure count and so starts the ladder again
   (roadmap D42). The message waits in its place for that ban and is
   offered to the same neighbour again, with three fresh attempts. Once a
   failure has demoted the neighbour, the message instead chooses the
   next hop at once ([next hop](#next-hop)): the neighbour is set aside for this
   hand-over, as in step 2, but not counted as tried. In the simulated
   loaded corridor an attempt was lost to a collision at the receiver
   about four times as often as by its ACK going astray (roadmap D34), so
   the neighbour most likely does not have the message. It stays a
   candidate when the message is next routed here, once its ban has
   ended. If it did take it, the next offer costs one round trip: it
   holds the message and only ACKs, or has finished with it and returns
   it ([receiving DATA](#receiving-data)) (roadmap D36). A return waits for its parent the
   same way and is dropped when a failure demotes the parent
   ([return to parent](#return-to-parent)). A delivery is not affected: the delivery queue
   keeps it ([delivery queue](#delivery-queue)).

#### Retry wait

The wait before the retry after the k-th unanswered
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

#### The ACK

The receiver retunes to the sender's channel, listens
before talking, sends the ACK, and retunes home. The ACK is the first
byte, whose `aux` says whether the packet is refused, the message ID,
and the IDs of the answering and the answered node (13 bytes, 14.4 ms,
[packet formats](#packet-formats)). The sender
matches the ID and both names against its outstanding hand-over.

An ACK resets the neighbour's failure count and refreshes its last-heard
time ([neighbour table](#neighbour-table)): a neighbour that answers is alive whether or not
its heartbeats are heard.

The answering node's ID is needed because nodes share home channels:
without it, an ACK for the same message from a different relay on the
same channel, which the depth-first search can produce, would end the
wrong hand-over and strand the message. The answered node's ID is
needed because two senders can offer the same message to the same node
at once: one retrying a hand-over whose ACK was lost while another
returns the message there, or the two copies of a split message
([why the search finds a path](#why-the-search-finds-a-path)). If the ACK named only the message and the relay,
each sender would take the answer to the other as its own. A taken one
would end a hand-over the relay never took, and the message would be
lost; a refused one would send a sender on from a relay that took its
packet, and the message would have a second copy (roadmap D32).

A node ACKs a packet as soon as it has received it, even while waiting
for an ACK of its own; if that makes it miss its own ACK, its next
attempt covers it.

A node owes at most one reply at a time. A DATA that arrives while its
reply to another packet is still waiting for the channel is neither
taken nor refused: the node says nothing and changes nothing, and the sender
retries as for a lost ACK. A second reply would go out only after the
first, which may wait for its channel until its own sender gives up,
100 ms; taking a packet it may not be able to answer in time would let
its sender hand the message to someone else as well. A copy of the
packet the owed reply answers, sent again because that reply is late,
is answered by it (roadmap D29).

So: **a node receives everything, ACKs included, on its home channel, and
leaves it only to transmit.** A sleepy node's home channel is its
anchor's, so traffic between them never retunes. A node is deaf for
≈15 ms per packet it forwards or ACKs; a sender that gets no ACK retries.
That is the cost of having one radio.

#### Refusal

An ACK that answers the outstanding hand-over and says
refused means the receiver does not take the packet ([receiving DATA](#receiving-data)). The
sender treats it as a listen-before-talk give-up (step 2): the neighbour
is set aside for this hand-over, not counted as tried, no failure is
counted, and the next hop is chosen at once. A return to the parent has
no other candidate: it waits in the send queue until the ban ends and is
sent again, and the refused transmission does not count as one of its
attempts. The neighbour is also **banned** for 1.56 s. A relay refuses a
packet to send on when the frame and the ACK that takes it, at most
138.6 + 14.4 = 153.0 ms, do not fit in its bucket on top of what it holds
and no hop place takes it ([receiving DATA](#receiving-data)), and its bucket refills that
much in 153.0 ms / 9.83 % = 1.56 s ([duty cycle cap](#duty-cycle-cap)). Until then it is no
candidate for any message ([next hop](#next-hop)): trying it again before its bucket
has refilled would only cost it another reply (roadmap D29).

#### Silence is pushback

A refusal measures one thing, what one
hand-over takes from the receiver's bucket, and the bucket refills that
in one ban. Silence says only that the channel or the receiver is full,
or that the link is gone, and not how full; so its ban doubles with
each silence in a row (step 6), as the wait of a refused message does
([a node's own message](#a-nodes-own-message)), and ends at the longest anything waits for a bucket. Without
the ban a neighbour that stops answering under load is offered every
other message at once, each offer up to three more frames where frames
are already being lost; in the simulated capacity domain at three times
the texting rate of [capacity and sizing](#capacity-and-sizing) that loop is what breaks first. The
message whose hand-over failed waits for the same neighbour rather than
going to the next one: the next one is usually a longer way round, which
costs more frames on the same full channels, and measured, going there
at once undoes what the ban gains (roadmap D42). Silence twice in a row
demotes the neighbour, and a link that is really gone then costs one ban
and one more hand-over before the message moves on; a neighbour that is
gone is tried again after each ban until its entry expires
([neighbour table](#neighbour-table)).

#### A node's own message

A node's own application's message,
offered once a ban, would be a full frame every 1.56 s on the channel of
the neighbour that refused it, 9 %, hiding its ACKs, and would spend the
holder's bucket as fast as it refills; so after the k-th refusal in a row it waits on the
ladder of step 6, one ban times 2^(k−1), up to the bucket's whole refill,
61 s, the longest anything waits for a bucket ([duty cycle cap](#duty-cycle-cap)), and the
wait starts again from one ban once the message is taken or dropped.

#### Send queue

The send queue has a regular room of 40 places and one hop place for
each count from 1 to 254; the packet being handed over keeps its place. A
packet enters the regular room only if it fits in the bucket with
everything queued ([receiving DATA](#receiving-data)), so it waits for the hand-overs ahead
of it, not for the bucket to refill. The number of places follows: the
bucket holds at most 6 s ([duty cycle cap](#duty-cycle-cap)), and a full frame and the ACK
that took it on cost 138.6 + 14.4 = 153.0 ms, so ⌈6 s / 153.0 ms⌉ = 40.
With the reply reserve left in, a full bucket takes at most 37 full
frames, so for full frames the bucket is the limit; shorter frames fill
the 40 places first, and forty hand-overs of them go out within a few
seconds.

A packet received with count h that the regular room refuses takes the
highest free hop place numbered h or lower, and keeps it until it leaves
the queue. A hop place holds its packet whatever the bucket holds; the
bucket still decides when it is sent. A sleepy node's own message, with
count 0, and a node's own application's message take no hop place, so
overload stops new messages where they enter, at the application
([addresses](#addresses)). A
packet received at 255 ends at the cap ([hop count](#hop-count)), so 254 hop places
are enough. Packets are handed over in the order they arrived, except
that, when bans end, the packets that waited for them go first ([overload waits](#overload-waits)).

#### Overload waits

Overload makes a message wait and never ends it; only its time does. A
message whose every remaining candidate is banned, by refusals or by
silence, waits in its place in the send queue, with its airtime still
counted, until the earliest ban ends, and is routed again then, at the
origin or anywhere else. A message ends only when it is delivered, when
its search runs out of neighbours ([return to parent](#return-to-parent)), when a return goes
unanswered twice in a row (step 6), when its time runs out on the way
([receiving DATA](#receiving-data)), when its sleepy node does not take it within the
delivery queue's TTL ([delivery queue](#delivery-queue)), at the hop cap
([hop count](#hop-count)), or when a node holding it dies (roadmap D31, D44).

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

### Routing

A message travels as a DATA packet ([packet formats](#packet-formats)) handed from relay to
relay, each hand-over acknowledged ([hand-over](#hand-over)). A relay hands the
message to its neighbour closest to the destination position. Where that
leads into a gap, a relay from which no neighbour leads on, the relays
route around it by depth-first search (DFS): a relay with no neighbour
left to try returns the message to the relay it first got it from, which
tries its next neighbour. The search is shared out among the relays: each
keeps a small record per message ([message records](#message-records)), and the packet carries
nothing for it but the name of the relay that sent it and a hop count.
DFS only routes around gaps; it never searches for an ID. A sleepy node
forwards as a relay does wherever it is a next hop ([next hop](#next-hop)), and what
this section says of a relay holds for it then.

The **origin** of a message is the node that first sends it into the
network: the node a sleepy node handed its own message to, or a node
sending for its own application. The search starts and, if it finds
nothing, ends there.

The rules below are roadmap D11, D16 and D17.

#### Packet sender

A node acts on a DATA packet whose `dst_hop` is its own ID
([node identity](#node-identity)) and ignores the rest. The packet names its sender by
`src_hop`, the sender's ID, and gives the sender's home channel
in `aux` ([packet formats](#packet-formats)). Those two are all a relay needs to send something
back: `dst_hop` set to `src_hop`, on the channel in `aux`. It needs no
neighbour-table entry for the sender; if it has one, `aux` becomes its
channel there, as a heartbeat's would ([neighbour table](#neighbour-table)).

A packet with `hops = 0` is a sleepy node's own message ([hop count](#hop-count)):
the receiving node is the origin, the message has no parent, and the
sender counts as tried ([a sleepy node's own message](#a-sleepy-nodes-own-message)).

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

The relay's **tried set** for the message is the parent, the
neighbours in `handed`, and a sleepy node that handed over its own
message: those known to have the message. A neighbour that
has not ACKed it is not tried, whether it was passed over, because it was
demoted, expired or not in the table at the time, or was offered the
message and refused it or never answered ([hand-over](#hand-over)): it stays a
candidate. When a neighbour-table slot changes hands, its bit is cleared
in every record.

The record is also how a relay recognises a repeat of a message it has
handled ([receiving DATA](#receiving-data)); there is no separate window of recent IDs.

**Eviction.** A record has no lifetime. It is kept until the table
needs its place; a new record then takes the place of the least recently
active one among those whose message the relay does not hold. A relay
holds a message while it is in its send queue ([send queue](#send-queue)), the hand-over
included, or set aside for a full delivery queue ([return to parent](#return-to-parent)). Without
its record the message could not be routed when its turn comes, so a
message is never lost for want of room (roadmap D31). The table holds
1855 records ([bounds](#bounds)).

#### Next hop

One function chooses the next hop for a first send, for the next try
after the message comes back, and after a failed hand-over
([hand-over](#hand-over)). The **candidates** are the neighbour-table entries that are
live ([neighbour table](#neighbour-table)), except those in the tried set ([message records](#message-records)), those
banned ([refusal](#refusal), [silence is pushback](#silence-is-pushback)), and those set aside for the current hand-over because
they did not take it ([hand-over](#hand-over) steps 2 and 6, [refusal](#refusal)). Sleepy neighbours
are candidates only where no relay neighbour is live, and then only
they: a node routes through sleepy nodes only where no relay is in
reach. Measured, trying them after every relay instead delivered the
same; this rule needs no rule for a relay that is only banned (roadmap
D46). The first hop of a sleepy node's own message is its anchor while
the anchor is live ([a sleepy node's own message](#a-sleepy-nodes-own-message)).

The **order** is: neighbours with margin ([neighbour table](#neighbour-table)) before those
without; within each, neighbours that are not demoted before demoted
ones; within each, distance to the destination position in the local
plane ([position and time](#position-and-time)), nearest first; equal distances are ordered by
ID. The relay sends to the first candidate. If there is none but
neighbours that are only banned, the message waits until the earliest
ban ends and is routed again then ([overload waits](#overload-waits)). If there is none at all,
the relay returns the message to its parent ([return to parent](#return-to-parent)).

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
only when every other has been, its failures counted as usual. Demotion
keeps a failing link from costing every message its attempts while
another route exists; it should not drop a message whose only route is
that link. Two hand-overs lost to collisions demote a neighbour that
works, and on a line it is the only route, so without the last resort
every message is dropped until the demotion lapses, six and a half
minutes. A neighbour that is really gone costs its relay at most 11 failed
hand-overs, whatever the messages, until its entry expires
([bounds](#bounds); [neighbour table](#neighbour-table): 390 s after its last heartbeat) (roadmap D25, D36, D42). A
banned neighbour is not tried at all ([refusal](#refusal), [silence is pushback](#silence-is-pushback)).

Nothing requires the next hop to be closer to the destination than the
relay itself. After the message comes back, the best neighbour left is
usually farther away, and such a rule would turn every gap into a dead
end. On a first send the nearest neighbour is chosen anyway, so progress
toward the destination is the normal case.

A candidate may already have the message, having got it by another path.
The relay cannot know this: its neighbours listen on other channels and
it does not overhear them. The candidate takes it all the same
([receiving DATA](#receiving-data)). If the message has arrived there, or the candidate holds
another copy, it goes no further; otherwise the candidate goes on with
its own search, and in time offers the message back or returns it, and
the relay takes that as its return.

#### Receiving DATA

A relay answers a DATA packet addressed to it ([packet sender](#packet-sender)) before
acting on it, with an ACK that takes or refuses the packet. It answers
only when it owes no other reply ([the ACK](#the-ack)).
Whether it takes the packet depends on what the packet gives it to do:

- **Something to send on.** It takes the packet into the regular room of
  its send queue only if the room has a place for it and its airtime
  bucket ([duty cycle cap](#duty-cycle-cap)) holds, as it stands, the ACK, the airtime of every
  frame in the send queue and of the new one, and the reply reserve.
  Failing that, it takes the packet into a hop place
  ([send queue](#send-queue)), whatever the bucket holds. So it refuses a message already in
  the network only when the hop places it may use are taken too, whatever
  its bans: [overload waits](#overload-waits) needs every such message to move on (roadmap D34).
- **A message entering the network here**, a sleepy node's own message
  (count 0) or the node's own application's message, also needs a
  neighbour the relay
  could hand it to now: a live one that is not banned ([next hop](#next-hop)), demoted
  or not, since the message has tried none. A relay whose neighbours are
  all banned would only hold the message until a ban ends, and its offer
  could be refused again; refused here, the message waits at its origin,
  where the application sees it. A relay with no live neighbour at all
  takes it, and the search ends as in [return to parent](#return-to-parent). So a relay takes
  from sleepy nodes and its own application no more than it can start
  sending now (roadmap D34).
- **Something to deliver.** It takes the packet only if its bucket holds
  the reply reserve and an ACK and, for another node, its delivery
  queue has room ([delivery queue](#delivery-queue)).
- **No work**, cases 1, 3 and 4 below. It takes the packet even when it
  would refuse work: a refusal would send its sender on with a second
  copy.

Otherwise it refuses: it sends an ACK that says refused, on the channel
in `aux` after listening before talking, changes nothing, and the sender
tries elsewhere at once or waits ([refusal](#refusal)). A relay that cannot afford
even the reply stays silent, and its sender retries as for a lost ACK
(roadmap D27, D29).

The packet is one of the following, tested in this order:

1. **Arrived:** the relay has delivered the message or holds it for a
   sleepy node ([delivery queue](#delivery-queue)), or its record says the message has arrived
   ([message records](#message-records)). ACK only.
2. **New:** the relay has no record for the message. It creates one, with
   the sender as parent (none if `hops = 0`). If it owns the destination
   ([owner](#owner)), it delivers; otherwise it chooses a next hop ([next hop](#next-hop)) and
   sends.
3. **A retransmission:** the sender and the count are those of the last
   packet of the message the relay received. The sender's ACK was lost
   and it sent the same packet again. ACK only.
4. **A copy while the relay holds one:** the message waits in its send
   queue or is set aside ([return to parent](#return-to-parent)). ACK only: the two copies merge,
   and the relay's goes on. A hand-over under way whose frame has gone
   out does not count: its receiver may have taken it with only the ACK
   lost, and this packet may be that copy come back.
5. **A copy, and the relay holds none:** the relay takes it and goes on
   with its own search from its record. It checks ownership again (a
   closer neighbour may have been demoted since), then sends to its next
   candidate ([next hop](#next-hop)), or returns the message to its parent
   ([return to parent](#return-to-parent)), or, at the origin, drops it. From a neighbour it
   handed the message to, this is that neighbour's return. From the
   neighbour it is still handing the message to, it also ends that
   hand-over: the neighbour has the message.

In every case the relay records the packet's sender and count as the
last packet, raises its highest count if the packet's is higher, and
updates its last-active time.

Only cases 1, 3 and 4 end a packet without acting on it, and in each the
message lives on: it has arrived, the packet was acted on when it first
came, or another copy goes on. So no case ends the last copy of a
message; only its time can (below). Nothing is sent straight back: a
relay that has had the message goes on from where its own search stands, and its sender, if the relay
has not tried it, stays a candidate and gets the message back in turn.
A return goes from a relay to its parent, and the parents, each set when
its record was made, lead back to the origin without a loop, so a
message cannot go back and forth between two relays that hold none of
it (roadmap D35).

**Time left.** A message lives in the network for at most the delivery
TTL, 600 s ([sleepy nodes](#sleepy-nodes)), from when a node took it from its application.
Every DATA carries the time the message has left, `time_left`, in whole
units of 2.5 s, rounded down ([packet formats](#packet-formats)). That node starts it at
the TTL and, like any holder, sends what is left at each transmission,
so a sleepy node's waits for its anchor count against it. A relay that takes a
packet to send on counts the message's time from the packet: what it
carries less the frame's time on air, since the sender stamps the time
as the frame goes out. Each transmission, retries included, carries
what is left at that moment, rounded down, so no node claims more than
the message has. With one unit or less left a message can no longer be
sent with any, and its time is gone: the relay drops it wherever it
waits in the send queue, for its turn, for the bucket, for a ban, in a
hop place or as a return, and one whose frame is on air at its next
attempt. A packet that arrives with its time gone is no work: the relay
takes it, as in cases 1, 3 and 4, and the message ends there. A message
to deliver is not affected: at its owner the delivery queue keeps it for
its own TTL, whatever was left, and the DATA that takes it to its sleepy
node carries the time it arrived with ([delivery](#delivery)). A return or a copy carries the time like
any DATA, so the parent's copy (case 5) ends by the same clock.

Without an end by time, a message the network has taken lives as long
as hand-overs to someone succeed. In the simulated capacity domain at
three times the texting rate of [capacity and sizing](#capacity-and-sizing), past the point where
it collapses, messages lived up to an hour and cost about 1500 frames
each, long after their sender and their destination's queue would have
given them up, and they kept the relays at their duty cap (roadmap D42,
D44). The TTL is the one the delivery queue keeps: the time the network
keeps a message ([sleepy nodes](#sleepy-nodes)).

Rounding down loses up to a unit at each hand-over, a whole one at a
hand-over that follows at once. 2.5 s is the finest unit at which one
byte holds the TTL, as 240, so a message whose hand-overs follow each
other at once has time for 239 of them, near the 255 of the hop cap
([hop count](#hop-count)); at 10 s it would have time for 59, fewer than some searches
under load take to reach their owner (roadmap D44). Rounded to the
nearest instead, a message that stays just under half a unit at each
relay would lose no time at all.

#### Return to parent

When [next hop](#next-hop) finds no candidate, because every neighbour has been
tried or set aside, the relay sends the message back to its parent: a
DATA packet with `dst_hop` the parent's ID, from the record, on
the parent's home channel as the neighbour table has it at the moment of
sending, so a return follows a parent that has moved home
([channel moves](#channel-moves)) since it handed the message over. Only a parent no longer
in the table is sent to on the channel in the record. The parent takes
it as a return ([receiving DATA](#receiving-data), case 5) and continues with its next
neighbour.

The origin has no parent. With no candidate left it drops the message:
no relay that can be reached from it through neighbours that answer
owns the destination. Nobody is told ([addresses](#addresses), roadmap D10), the
origin's own application included: that is the cost of having no
end-to-end acknowledgement.

A message whose neighbours left are only banned is neither returned nor
dropped: it waits ([overload waits](#overload-waits)), at the origin or anywhere else,
until its time is gone ([receiving DATA](#receiving-data)); a node's own application's
message waits as in [a node's own message](#a-nodes-own-message). An application's message for a
sleepy node this relay holds for that finds the delivery queue full is
kept aside until the queue has a place.

If a failed hand-over to the parent demotes it, after the message has
waited for the parent's ban and gone again ([hand-over](#hand-over) step 6), the
relay drops the message and the search ends early. A parent that refuses the return
has not failed: the return waits for the ban and goes again
([refusal](#refusal)).

#### Hop count

`hops` in the DATA header counts hand-overs. A sleepy node with an
anchor sends its own message with 0; a relay sending for its own application sends 1, as if
it had received the message at 0, since 0 means "a sleepy node's own,
start the search here" ([packet sender](#packet-sender)). A relay
sends its record's highest count plus one ([message records](#message-records)), whether it
is forwarding or returning to its parent. The count is set when a
hand-over's first frame goes out, and its retries repeat it
([hand-over](#hand-over)). The highest count is the largest the relay has received for
the message or sent in a hand-over that succeeded. So along the path of
a message the count only grows, and a relay sends a count for a message
once, but in retries: to send another packet of it, it must have
received one since its last hand-over that succeeded, or that hand-over
failed and the packet goes elsewhere. After a wait for a ban it may also
go again to the neighbour that did not answer; if that neighbour took
the first, the second is in fact a retransmission to it. A receiver
therefore tells a retransmission by the sender and the count
([receiving DATA](#receiving-data), case 3). A frame that waited in the send queue, or goes
to another neighbour after a failed hand-over, carries the count as it
stands when it goes. The count also numbers the hop places of the send
queue ([send queue](#send-queue)).

A relay that would have to send a count above 255, the largest the
one-byte field holds, drops the message. A search that keeps its records
ends on its own ([why the search finds a path](#why-the-search-finds-a-path)); the cap ends one that has lost
its way because a record was replaced, or because the message split in
two, and it ends the chain of hop places ([overload waits](#overload-waits)). 255 is generous:
the longest straight route across the city of [capacity and sizing](#capacity-and-sizing) (≈30 km at
the ≈750 m street range of [physical layer](#physical-layer)) is about 40 hand-overs, leaving
about 200 for routing around gaps.

#### Owner

A node **owns** a message's destination when:

- `dst_id` is its own ID: it is the addressed node, wherever it is; or
- its own position is `dst_pos`; or
- it is within 50 m of `dst_pos` and no live, non-demoted neighbour is
  closer to it than itself. The relay at that position has then probably
  died or moved; 50 m is the distance [position and time](#position-and-time) allows between
  two GPS fixes of a relay that has not moved, so a relay this close is,
  for the radio, in the same place.

A node farther than 50 m with no closer neighbour is at a gap, not the
owner, and the search goes on ([next hop](#next-hop)).

A message whose destination is the sending node's own address is
delivered there without entering the search. Otherwise the destination
position decides who holds it: a relay that hears the sleepy node but
does not own the position routes the message on like any other. A sleepy
node that has just moved to another anchor stays in the old relay's heard
table for a delivery TTL ([heard table](#heard-table)), so the table disagrees with the
address exactly when the address is new; delivering from the table would
queue the message for a node that has gone (roadmap D21).

The addressed node delivers the message to its application. Any other
owner queues it for the addressed node ([delivery queue](#delivery-queue)) and sends it when
the node listens ([delivery](#delivery)), whether or not it hears the node now; a
later copy of a message it has delivered is only ACKed
([receiving DATA](#receiving-data), case 1). A message for a relay that is gone waits there
for the queue's TTL and ends.

An owner that does not hear the addressed node may ask its direct
neighbours once whether they hear it (one hop, one round; the packet is
open item O13); if none does, it queues. Nothing wider: a wider search is
a flood.

### Why the search finds a path

Suppose that while a message is being routed the links do not change,
every hand-over to a neighbour that answers succeeds within its attempts,
and no record is replaced. Then:

- The message moves as a single packet: a relay sends only what it has
  just received or originated, and every ACK-only case of
  [receiving DATA](#receiving-data) sends nothing.
- A relay holding the message either owns the destination, sends to its
  next candidate, or returns it to its parent. A neighbour that takes the
  message is tried and never offered it again from that record, so each
  relay hands the message to each neighbour at most once; a return goes
  up the parents, which lead to the origin without a loop. So each stay
  of the message at a relay ends with a hand-over that succeeds, or at
  the origin with a drop, and there are finitely many of those: the
  search ends. A neighbour that does not answer is banned and then demoted
  ([hand-over](#hand-over) step 6), so it costs its relay at most 11 failed hand-overs
  ([bounds](#bounds)), and the hop cap bounds the stays ([hop count](#hop-count)).
- A send to a relay that already has the message hands it the search
  ([receiving DATA](#receiving-data), case 5): it goes on with its own candidates. The sender
  is one of them unless the relay has tried it, so the search comes back
  to the sender as a return, and the sender goes on with its own. A
  relay whose search is unfinished gets the message back that way or
  through the parents, so none is left with neighbours untried.

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
  up to 61 s ([hand-over](#hand-over)). So a dead neighbour costs a relay at most 11
  failed hand-overs, 33 frames, whatever the messages, before it leaves
  the table 390 s after its last heartbeat: the bans 1.56, 3.11, 6.2,
  12.5, 24.9 and 49.8 s add up to 98 s, and 61 s bans fill the rest.
  After two failures it is a last resort ([neighbour table](#neighbour-table)).
- **Life:** at most the delivery TTL, 600 s, from a node taking it from
  the application to its owner ([receiving DATA](#receiving-data)), whatever
  its search and however long it waits; then up to a TTL more in the
  owner's delivery queue. So a message costs the network at most 600 s
  of waiting and handing over, and holds a place in a send queue no
  longer than that.
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
  places waiting out one ban of 1.56 s ([refusal](#refusal)): 457.3 s. A ban after
  silence lasts up to 61 s ([hand-over](#hand-over) step 6), so where every way on is
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
- **State per relay:** the records at 32 bytes each, 58.0 KiB; a send queue
  of 294 places of 280 bytes, each a frame of up to 255 bytes and its
  bookkeeping, 80.4 KiB ([send queue](#send-queue)); the owner's
  delivered IDs, kept with the delivery queue ([delivery queue](#delivery-queue)).

---

## Sleepy nodes

A sleepy node is a carried node on battery: a node with the sleepy
setting ([hardware](#hardware)). It heartbeats rarely and on one channel, listens for a
short window after each heartbeat, and the messages for it wait at a
relay it chooses, its **anchor**. It sleeps only while it has an anchor: one
that hears no relay listens all the time and acts as a relay, since no
one could hold its messages ([sleepy nodes alone](#sleepy-nodes-alone)). Everything else it does as
any node.

A sleepy node can go **dark** for a few minutes: a phone in a pocket, a
basement or the metro. What the network keeps for a node that does not
answer, it keeps for the **delivery TTL**, 10 minutes (600 s): the
messages waiting for it ([delivery queue](#delivery-queue)) and the node in its holder's
table ([heard table](#heard-table)). The network keeps a message on its way for the
same time ([receiving DATA](#receiving-data)).

### Relay choice

Listening on any one channel, a node hears every relay in range once per
rotation (≈130 s, at most 13 × 15 s = 195 s; [heartbeat](#heartbeat)), and keeps it in
its neighbour table with the averaged RSSI of what it heard from it
([neighbour table](#neighbour-table)).

**First choice.** A sleepy node takes as its anchor the strongest relay
it has heard a worst-case rotation, 195 s, after it first heard one; by
then every relay in range has visited its channel. It listens on the
anchor's home channel, and its address is the anchor's position and its
own ID ([addresses](#addresses)). It follows the anchor to a new home channel at the
anchor's next heartbeat it hears ([channel moves](#channel-moves)).

**Switching.** It moves to another relay only when that relay has been
heard over at least two heartbeats and its averaged RSSI is at least
8 dB above the anchor's. A relay's heartbeats reach the node's channel
about 130 s apart, longer than the 60 s the fading lasts, so they are
nearly independent. Under fading of σ = 4 dB (the scenarios' figure) a
mean of two heartbeats has σ/√2, and the difference of two such means
σ√(1/2 + 1/2) = 4 dB; 8 dB is two of those standard deviations, and a
relay that is no better takes the node on 2.3 % of comparisons. The
neighbour table's average lets each new frame count half, which varies
less than a mean of two (σ²/3 against σ²/2), so 8 dB is a little more
than two standard deviations there. Vehicles cannot keep up and rely on
the delivery queue and a new anchor on arrival.

**Back from the dark.** A node whose anchor has left its neighbour table
chooses again as at first: a rotation after it next hears a relay. If it
hears its anchor meanwhile, it keeps it. Taking the first relay heard
instead changed anchors two and a half times as often, and the messages
sent to the old address waited out their TTL (roadmap D46).

### Beats

A sleepy node heartbeats only on its home channel, the anchor's, where
its holder listens: a **beat**. The heartbeat has the sleepy type
([heartbeat](#heartbeat)). It beats at a random time in the second half of each
**beat period**, P = 300 s: half the delivery TTL, so that a holder
keeps it for two periods, a TTL, and one lost beat does not lose it,
while a node that has gone is kept no longer than its messages
([heard table](#heard-table)). The spread keeps two nodes whose beats met once from
meeting again. After it chooses or changes its anchor it beats at once
and once more within a heartbeat interval, 5–15 s, so that one lost
beat does not hide it from its new holder for a period.

At a mean interval of 225 s a node beats 16 times an hour, 189 ms of
airtime; measured with the beats after anchor changes, 212 ms an hour.

A sleepy node with an anchor does not move its home channel by the busy
estimate ([channel moves](#channel-moves)), and does not sweep the channels ([busy estimate](#busy-estimate)): asleep it
could not.

### Heard table

A node keeps the sleepy nodes it hears beat in its **heard table**,
256 entries, by ID: the node's home channel, until when it listens
([delivery](#delivery)), whether a delivery to it went unanswered since its
last beat, and the mean RSSI of its last two beats. An entry is kept for
two beat periods after the last beat, the delivery TTL. When the table
is full a newcomer takes the place of an expired entry, else of the
weakest if the newcomer is heard stronger: the strongest are the nodes
most likely to have chosen this relay as their anchor. 256 covers ≈80
sleepy nodes per relay expected at indoor density in Kyiv with 10 %
adoption, 400 at 50 % ([capacity and sizing](#capacity-and-sizing)).

### Delivery

A message that reaches the owner of its position ([owner](#owner)) and is
for another node waits in the owner's delivery queue
([delivery queue](#delivery-queue)): the owner is that node's **holder**. It goes to the node
as DATA addressed to the node's ID, `dst_hop` and `dst_id` alike, on the
node's home channel from the heard table, with the ordinary preamble,
and the node answers with an ordinary ACK ([the ACK](#the-ack)). It goes through
the send queue as any hand-over ([hand-over](#hand-over)), but only while the node
listens: for a **window** of 9.25 s from each beat the holder hears,
renewed by each message the node takes. The window is what the holder
may need to bring one message: the hand-overs that may be ahead of it,
a full send queue of full frames each with its ACK,
40 × 152.96 ms = 6.12 s ([send queue](#send-queue)), and the message with all its
attempts and retry waits, 3.13 s ([hand-over](#hand-over)). The node is not told
how many messages wait, so it listens as long again after each it
takes. A delivery that goes unanswered marks the node asleep, and its
messages wait for its next beat.

The DATA carries the hop count as any hand-over ([hop count](#hop-count)) and the
time the message had left when it arrived: the delivery queue's wait
does not count against it ([receiving DATA](#receiving-data)).

So a message waits for its node a mean of about two minutes, at most a
beat period. Listening 9.25 s after each beat is 148 s an hour, 4 % of
the time: 0.24 mA on average at the 5.8 mA of listening, about five
months of the 850 mAh battery for the listening alone. GPS goes off after a fix, and
off after 10 minutes without one (searching under jamming draws
≈27 mA, ≈1.3 days).

### Delivery queue

Bounded, not a mailbox: up to 41 messages per node, kept for the
delivery TTL ([sleepy nodes](#sleepy-nodes)). On expiry the message is dropped. The
same TTL bounds a message's way through the network ([receiving DATA](#receiving-data));
the queue's starts afresh when the message arrives, since it is the wait
for a node that sleeps or is dark. When the queue is full, a message for
another node is refused ([receiving DATA](#receiving-data)): what is queued stays, and
the sender learns at once.

A queued message goes to the send queue one per node at a time and
oldest first: when it arrives, when the node beats, when the node takes
the one before, and whenever a hand-over ends and leaves room in the send
queue. It needs room as any packet does ([send queue](#send-queue)); one the bucket
cannot fit yet goes the moment the bucket will. One whose node has
stopped listening by the time its hand-over would start waits for the
next beat. A failed delivery counts no failure against anyone: the
node is not a neighbour it routes through.

Nodes in one train or one basement go dark together, so the queue holds
what arrives in a TTL for every node a holder can keep, its heard
table's 256 ([heard table](#heard-table)), at the 23 texts a day each that the network
carries ([capacity and sizing](#capacity-and-sizing)): 256 × 23 / 144 ten-minute TTLs in a day
= 40.9, so 41 messages, 10.6 KiB. A node takes at most what its holder
holds in one window, so on its beats at most 41 messages a period.

The owner remembers 41 delivered message IDs ([packet formats](#packet-formats)) and ACKs
a repeat without delivering it twice. When it needs the place, it
forgets the one delivered or last repeated longest ago; time alone
forgets nothing. 41 is the delivery queue's size: what a holder delivers
in one TTL for a full heard table.

The queue plus the sender's own retry covers a node that has moved to
another anchor; the old holder does not forward to the new one.

### A sleepy node's own message

A sleepy node with an anchor hands its own message to the anchor while
the anchor is live, with count 0 ([hop count](#hop-count)), and otherwise to its next hop
as any node does ([next hop](#next-hop)); one without an anchor sends as a relay does
([sleepy nodes alone](#sleepy-nodes-alone)). The node that takes a message with count 0 is
its origin ([routing](#routing)): it takes it only as fresh work, into its
regular room and only while not every neighbour of its is banned
([receiving DATA](#receiving-data)), and counts the sender as tried, so the search never
returns to a node that sleeps. Overload then stops a new message where
it enters (roadmap D31, D34). Handed to the neighbour nearest its
destination instead, the first hop was often a faint, far relay: in the
simulated capacity domain at twice the texting rate of
[capacity and sizing](#capacity-and-sizing) the relays reached their duty cap and 62 and 36 % of
texts were delivered, against 90 % with the anchor (roadmap D46). Sent
with count 1, as a relay's own message is, it could take hop place 1 at
its first hop, which overload must not give to fresh work.

A refused message waits as a relay's own does ([a node's own message](#a-nodes-own-message)).
One whose hand-over goes unanswered is tried again a fade later, 60 s,
or when the anchor's ban ends if that is later: a fade lasts about a
minute (the scenarios' correlation time), and the three attempts of one
hand-over, a few hundred milliseconds apart, all fall inside it, where
the ban's ladder would offer it every few seconds to a node whose
channel is full. A message that finds no live neighbour at all waits
for one, routed again every 60 s, until its time runs out
([receiving DATA](#receiving-data)): a carried node loses its neighbours by going dark
and finds them again. The application is not told ([addresses](#addresses)).

### Sleepy nodes alone

A sleepy node that hears no relay has no anchor, so it does not sleep:
it listens all the time and acts as a relay. It heartbeats at a relay's
interval on the rotating channel ([heartbeat](#heartbeat)), with the sleepy type so
that no other sleepy node takes it as its anchor, sweeps, chooses and
moves its home channel by the busy estimate, and forwards. Its address
is its own position, and it owns it. Nodes route through sleepy
neighbours only where no relay is live ([next hop](#next-hop)), so those of a place
without relays route among themselves. Kept on one channel, the lowest
of the quiet ones, sixty of them over a square kilometre with relay
heartbeats overloaded it: split copies of messages circulated until
their time ran out, thousands of frames for each text (roadmap D46).

---

## Phone link

BLE 5.0 GATT service on the nRF52840 (TrouBLE, roadmap
D5): characteristics for send, receive-notify, node status, position set,
sleepy set. The app owns identity and end-to-end encryption; the node sees
opaque payloads. The sender's address, position and node ID, travels inside the
encrypted payload so that only the recipient learns where the sender is.
Encryption scheme: open item O1.

A message has no ID field: the network identifies it by a hash of its
destination and payload ([packet formats](#packet-formats)). Two messages to the same destination with
identical payloads are therefore one message to every relay that still
remembers the first, which a relay does until newer records evict its
record ([message records](#message-records), [bounds](#bounds)), and the second is not delivered. The app must make every payload unique;
end-to-end encryption with a fresh nonce per message does that.

**What the network does not defend against.** Nothing on the air is
authenticated. Anyone with a 433 MHz LoRa radio can send heartbeats with
any position, attract traffic to it and drop it, send packets in another
node's name, or replay what it has heard. The app's end-to-end
encryption keeps payloads private and authentic; it does nothing for
routing. This is the scope stated at the top: infrastructure loss, not an
adversary on the air.

---

## What the protocol omits

Common mesh mechanisms left out, and why.

| Omitted | Instead | Why |
|---|---|---|
| Neighbour lists in heartbeats and a network-wide node table | 9-byte heartbeat of ID, short position and channel; routing uses only direct neighbours | spreading tables through the network does not scale to thousands of relays at LoRa rates; addresses are positions, so there is nothing to look up |
| Sequence numbers in heartbeats | none | they exist to age shared tables; without those they carry nothing |
| Time slots on a shared schedule | always listen, transmit as soon as listen-before-talk clears | no time sync exists, and slots add latency |
| Cooperative position estimation | every relay knows its position from GPS or survey | [position and time](#position-and-time) |
| Heartbeats on one fixed channel | rotating channel, random 5–15 s interval | [heartbeat](#heartbeat) |
| Addressing by ID with a lookup | position + node ID; the owner resolves the ID locally | [forwarding](#forwarding) |

---

## Capacity and sizing

Rationale, not specification.

Kyiv model: ≈840 km², ≈400 km² built-up, ≈3 M people. Indoor coverage
needs ≈9.6 relays/km² (≈3,800 relays); outdoor-only ≈1.9/km² (≈750).
Rooftop relays hear ≈3 km ([physical layer](#physical-layer)), so a collision domain, the area within which
transmissions interfere, is ≈28 km² with ≈270 relays, ≈14 domains across
the built-up city.

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
uncertain by 2×. The city centre is the hot spot, and the per-relay 10%
cap ([duty cycle cap](#duty-cycle-cap)) binds there first.

Sleepy nodes' beats cost what [beats](#beats) lists, 212 ms an hour each, and
relays nothing for them. Summed over the domain's 20 000 sleepy nodes
they take 1.18 s of airtime a second, 9 % of one channel's time over the
13, where every relay hears every other.

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
- **Sleepy nodes heartbeating as relays do, every 10 s:** collisions
  happen at the shared relay whatever the sleepy node's range; 72% of a
  channel at 400 sleepy nodes per relay.

---

## Packet formats

A proposal, to be finalised in open item O2.

The first byte of every packet is `type:4 | aux:4`; `aux` is the home
channel in heartbeats and type-specific flags elsewhere. Positions are
7 bytes: 25-bit latitude, 26-bit longitude, 5 spare bits; the heartbeat
carries the 4-byte short position ([position and time](#position-and-time)). A node ID is 4
bytes ([node identity](#node-identity)). The LoRa hardware CRC covers everything.

```
HEARTBEAT 0x1/0x2/0x8: first(1) id(4) short_pos(4)                         = 9
          0x1 relay, surveyed position; 0x2 relay, GPS-derived position;
          0x8 sleepy node; aux = home channel
ACK       0x3: first(1) id(4) from(4) to(4)                                 = 13
          the answer to a DATA, taken or refused; sent on the channel in
          the answered packet's aux; aux bit 0 = refused
          from, to = the IDs of the answering and the answered node
DATA      0x5: first(1) dst_hop(4) src_hop(4) dst_pos(7) dst_id(4) hops(1)
               time_left(1) payload                                         = 22 + payload
          dst_hop, src_hop = IDs of the receiving and the sending node;
          aux = sender's home channel, so the receiver knows where to ACK;
          time_left = the time the message has left, in 2.5 s, rounded down,
          stamped at each transmission ([receiving DATA](#receiving-data)); 240 is the 600 s TTL
Types 0x4, 0x6 and 0x7 are unused; nodes reject them like any unknown type.
Type 0xF is reserved for the bench frames of the
firmware's measurement console (`bark-fw/README.md`); nodes reject it like any
unknown type.
```

The **message ID** is a 32-bit FNV-1a hash of
`dst_pos || dst_id || payload`, computed
by every node that handles the message; it is the same on every hop
because none of these fields changes. DATA does not carry it; the ACK
does, to name what it acknowledges. The app must keep payloads unique
([phone link](#phone-link)).

`dst_hop` lets every node on a channel discard packets not addressed to it
after 5 bytes. `src_hop` names the node that sent the packet, and `aux`
its home channel, which is where the ACK goes; routing uses them to know
where a message came from ([routing](#routing)). The payload is at most
255 − 22 = 233 bytes; the sender's address is inside it, encrypted
([phone link](#phone-link)).

---

## Implementation notes

**Rust stack:** `embassy-nrf` (nRF52840), `lora-phy` from git (SF5, CAD,
RSSI; roadmap D4), TrouBLE with the Nordic controller for BLE (D5), the
stock UF2 bootloader for flashing (D6). The protocol core does no I/O of
its own and owns its routing and tables (D1, D3; `core-interface.md`).
Crate layout is D7.

**RAM budget (256 KiB):** neighbour table 64 × 56 B = 3.5 KiB; heard table
256 × 32 B = 8.0 KiB; delivery queue 41 × 264 B = 10.6 KiB; message records
([message records](#message-records)) 1855 × 32 B = 58.0 KiB; send queue (40 + 254) × 280 B = 80.4 KiB;
delivered window 41 × 16 B = 0.6 KiB; busy estimates and duty budget under
1 KiB; 162.3 KiB in all, measured, for every node, sleepy or not. BLE controller ≈5 KiB; Embassy and stacks ≈16 KiB. The
application has 255 KiB; 1 KiB at the bottom is kept for the boot record and
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
5. Sleepy node: anchor, beats, delivery in the window, queue, and the
   radio asleep between windows, which the firmware does not do yet.
   **This is the loop every user feels; prototype it before anything
   clever.**
6. BLE GATT + minimal phone app.
7. Field test: a dozen relays on windowsills across a few blocks.

**Test rig:** at least three T-Echo 433, SMA attenuators, one interferer
source (a fourth board is fine), a USB logic/UART sniffer for timing.

---

## Open items

| # | Item | Owner/when |
|---|---|---|
| O1 | End-to-end encryption scheme in the app (key exchange, address-in-payload format) | before step 6 |
| O2 | Finalise packet formats ([packet formats](#packet-formats)) | step 4 |
| O3 | Exact thresholds: busy threshold margin (6 dB), move hysteresis (0.05 / 10 min), demote count, ACK timeout | tune in steps 3–4 |
| O4 | Neighbour-table eviction for rooftop relays that hear hundreds: whether a set spread around the relay beats D12's rule ([neighbour table](#neighbour-table)) | simulator |
| O5 | Bench item C1: adjacent-channel rejection at 125 kHz spacing; fallback 11 × 150 kHz | step 1 |
| O7 | Sending to an area (address = position + radius, node ID = broadcast) | later |
| O8 | Per-packet transmit power control (prefer short hops, cut rooftop hearing radius): ≈2× capacity | resolved: measured 2026-10-15/16 (`power-research-2026-10-15.md`), not adopted (roadmap D40) |
| O9 | Repository/crate layout | resolved: roadmap D7 |
| O11 | Relay-side RSSI noise-floor calibration method and drift | step 3 |
| O12 | Confirm RAK WisMesh Repeater 433 availability for rooftop solar units | procurement |
| O13 | Packet for the owner's one-round question to its neighbours: do you hear this node ID? ([owner](#owner)) | step 5 |
| O14 | Hidden terminals: the 0–50 ms retry backoff was shorter than a full frame (139 ms), so two senders that cannot hear each other and collided once collided again on every retry | resolved: roadmap D22, [retry wait](#retry-wait) |
| O15 | A relay at its duty-cycle cap refuses only when its send queue is full, and its queue then drains at the bucket's rate: a packet can wait ≈23 s there (with three frames a message, 72 s median in the three-hour test) where the sender could have chosen another next hop at once. Whether to bound the queue by its drain time at the bucket's rate, and how | resolved: roadmap D29 rule 1, [receiving DATA](#receiving-data), [send queue](#send-queue) |

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

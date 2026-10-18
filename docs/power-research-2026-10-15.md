# Transmit power control

**Date:** 2026-10-15 and 16. Open item O8. Spec references are to
`protocol.md`. Builds on `capacity-research-2026-10-15.md` (S4, one
domain of 28 km²), D39 and D38. Research
only: the prototype is behind `Config::power`, off by default, and gives
byte-identical results with it off. Nothing here changes the spec.

## Summary

"Lower my power until at most N peers hear me" is not the right rule
for this network, and the measurements say so twice over.

- **Where the relays' 10 % duty cycle binds, which is S4, the design's
  domain, it costs capacity.** Shorter hops cost the same airtime per
  hop, and there are more of them. At twice the spec "Capacity and sizing" texting rate the
  degree rule delivers 37 to 59 % where full power delivers 94 %. It also
  splits the network: with N = 8 one relay is cut off in each seed, and N
  had to be 12 before the S4 relays stayed connected.
- **Per-receiver power alone buys nothing measurable once D38 and D39 are
  in.** At twice the spec "Capacity and sizing" rate it delivers 94.6 % against 94.4 % (seed 1)
  and 94.5 against 94.0 % (seed 2). It takes a third off the area each
  frame disturbs, but the channels were never what bound.
- **Where the channel binds, power control is the fix, but it must lower
  the heartbeats too.** In the domain where every relay is on a roof
  and all hear each other, spec "Capacity and sizing"'s own picture, full power delivers 7.9 %
  at the spec "Capacity and sizing" rate. Per-receiver power for data raises that only to
  12.9 %: heartbeats at full power keep every relay in every table and
  routes stay long. Heartbeats at the lowest power that reaches the
  relay's 8 to 16 nearest neighbours, with one rotation in three at full
  power, and data at per-receiver power (rule c) deliver 90.5 to 90.8 %.
- **That same rule costs the mixed domain at twice the spec "Capacity and sizing" rate,** where
  the domain is close to its limit: with N of 12 or 16 it delivers 92.7
  to 94.4 % in seed 1 but 77.5 to 81 % in seed 2, against 94 %; with N = 8
  52 to 91 %. Rooftop relays there carry the long hops, and whispering
  shortens them.
- **In overload both lose.** In a 2 × 2 tiling of domains whose texts
  cross the whole 112 km², at the spec "Capacity and sizing" rate, full power carries 4.7 texts
  a second, per-receiver power 2.1 and rule (c) 1.6.

So no rule wins everywhere. What decides is which limit binds at the
relay: its duty budget, which power cannot change, or the channel, which
it can. The recommendation is to keep full power, keep the five spare
heartbeat bits for the sender's power (spec "Heartbeat"), which costs nothing, and
hold rule (c) as the measured remedy for rooftop-dense areas, to be
switched on by a relay's own measured channel load once a gate for it
has been tested. Spec "Capacity and sizing"'s "+ per-packet power control ≈ 2× capacity"
assumes the channel binds; in S4 it does not, and the doubling spec "Capacity and sizing"
expected came from D38 and D39 instead.

## The question

A relay lowers its transmit power until at most N peers hear it, so that
collision domains shrink, each channel is reused more often across the
city, and rooftop relays stop hearing the whole domain. The counter-
argument: the neighbour count is the right goal but a poor control
signal, because lowering my power changes who hears me, not whom I hear.

## Physics

### What a relay can know about who hears it

A path between two antennas on one frequency is reciprocal: the loss from
a to b equals the loss from b to a, antenna gains of both ends included.
So a relay that hears a heartbeat at RSSI R, sent at power P, knows the
path loss L = P − R, and that the sender would hear it at any power p
with p − L at or above the sender's sensitivity. It needs P. The
heartbeat has five spare bits after its position (spec "Heartbeat"), enough to state
it. The prototype uses four for the sender's operating power in 2 dB
steps below +10 dBm, down to −8 dBm, the lowest even step at or above the
SX1262's −9 dBm (SX1261/2 datasheet §13.4.4, SetTxParams: −9 to +22 dBm
in 1 dB steps with the high-power amplifier), and one to say that this
heartbeat itself went at +10 dBm whatever the operating power. All zero
is +10 dBm, so heartbeats are unchanged on the wire with power control
off.

What reciprocity does not cover:

- **Receivers differ.** Whether b hears a depends on the noise at b. A
  rooftop relay away from street-level noise has a lower floor than a
  windowsill relay beside a busy street. The loss is shared; the
  sensitivity is not. The prototype assumes equal floors and leaves the
  difference to the margin.
- **Settings and readings are not exact.** The radio's output differs
  from its setting, and its RSSI from the true power, by amounts the
  datasheet does not bound; it gives the 1 dB setting step and the 0.5 dB
  RSSI step (§13.5.3, GetPacketStatus). Both enter the loss one for one.
- **RSSI includes noise.** The packet RSSI is signal plus noise: a signal
  6 dB above the floor reads 1 dB high, one at the floor 3 dB high. With
  a margin of 8 dB this never changes the chosen power, because a link
  weak enough to be misread needs full power anyway.
- **Fading is per channel.** A relay hears a neighbour's heartbeat on its
  own home channel and sends to the neighbour on the neighbour's.
  Shadowing (6 dB in the scenarios, fixed per pair) is common to both
  channels and both directions and is what the loss estimate measures.
  Multipath fading (4 dB, 60 s) differs between them. Heartbeats reach a
  given channel 130 s apart, so successive samples are nearly
  independent, and the D38 average, which counts each new frame half, has
  σ/√3 = 2.3 dB of one sample's 4 dB.

**The margin.** A frame sent at the power the averaged loss needs fails
when the fade on its own channel and the estimate's error together
exceed the margin: √(4² + 2.3²) = 4.6 dB, so 95 % of frames get through
with 1.645 × 4.6 = 7.6 dB, and the 1 dB setting step makes 8 dB. Errors
of calibration and reciprocity come on top; the bench measures them.
Every rule below uses 8 dB over a sensitivity of −119 dBm.

### Who hears whom when powers differ

A hand-over needs both directions: the DATA must reach the neighbour and
its ACK must reach back (spec "Hand-over"). With heartbeats stating their power, a
relay knows its own operating power, the neighbour's, and the loss
between them. The rule the prototype applies when relays lower their
operating power: a neighbour is a next hop only if

    L + margin ≤ min(p_me, p_neighbour) − sensitivity.

Without it a relay that whispers still lists every neighbour whose louder
heartbeat it hears, and greedy forwarding prefers exactly the far ones it
no longer reaches.

Under per-receiver power the ACK goes at the power the receiver's own
estimate of the loss to the sender needs, and at full power to a sender
it does not know. A leaf sends at full power, so a relay knows the loss
to a leaf from its last frame and polls and delivers to it at that loss
plus the margin.

### Hearing, interference and reuse

With the scenarios' path-loss exponent of 3.6, the distance at which a
frame arrives at a given strength scales as 10^(ΔP/36): each dB less
power takes 6.2 % off the distance and 12 % off the area. At −8 dBm,
18 dB down, distance falls to 0.32 and area to 0.10. A rooftop pair that
hears each other at 3 km at +10 dBm hears at 950 m at −8 dBm; a
windowsill pair's 750 m becomes 240 m. The floor is a hardware limit:
rooftop relays that all hear each other cannot whisper below about
1 km.

A frame too weak to decode still adds to the noise. The simulator
weights interference 7.5 dB above noise (the capture threshold over the
demodulator's required SNR, `bark-sim/src/phy.rs`): an interferer at
−124 dBm, 5 dB under sensitivity, doubles a receiver's effective noise.
The area a frame disturbs is 10^(10/36) = 1.9 times the area that
decodes it, and shrinks with power in the same ratio.

What a text costs is two things:

- **Channel time over area.** Each hop occupies its channel over the area
  it disturbs. For hops of length r over a distance D there are D/r of
  them, each disturbing an area ∝ r²: the cost per text ∝ D·r. Shorter
  hops cost less, and the channel is reused more. This is spec "Capacity and sizing"'s argument
  for power control.
- **Airtime at relays.** Each hop costs the sending relay one frame of
  its duty budget whatever its power: the cost per text ∝ D/r. Shorter
  hops cost more.

Which binds decides which way a shorter hop goes. A rule that shortens
hops (the degree rule) gains only where the channel binds and loses
where the duty cycle does. A rule that keeps the hops and lowers each
frame to what its hop needs (per-receiver power) leaves the duty cost
alone and lowers the channel cost: it cannot lose airtime, and gains
only as much as interference was costing retries. The regulation gives
nothing back for lower power: ERC/REC 70-03 band g1 limits duty cycle
in time at any power up to 10 mW.

The static analysis of the next section puts numbers on this for the S4
relays.

## The rules compared

| Rule | Heartbeats | DATA, ACK, ATTACHED, DELIVER | Next hop |
|---|---|---|---|
| Fixed | +10 dBm | +10 dBm | as D38 |
| (a) Degree | operating power | operating power | both ends reach each other at their operating powers |
| (b) Per receiver | +10 dBm | what the receiver's loss needs | as D38 |
| (c) Both | operating power | what the receiver's loss needs | as D38 |

The **operating power** is the lowest 2 dB step at which the relay's
N-th nearest neighbour by loss, and with sectors the nearest in each
occupied sector, would hear a full frame with the margin; full power if
it knows fewer than N. It rises at once and falls only by two steps or
more, so that the averaging noise of the losses (2.3 dB) does not move
it; it is recomputed after each heartbeat.

**Discovery.** Under (a) and (c) one rotation of heartbeats in k goes at
full power, so that every relay in reach is still heard and measured and
a newcomer finds a relay that whispers. k is at most 3: an entry expires
three rotations after its last heartbeat (spec "Neighbour table"), so a relay heard only
at full power must be heard at least that often. With the D38 average,
the loss to such a relay is then at most 6.5 minutes old. A heartbeat at
full power adds its sender to the table only if a full frame at full
power would reach it with the margin; it updates a relay already there
in any case. The receiver tells the two kinds of heartbeat apart by the
flag bit.

**Sectors.** Six sectors of 60° derive from cone-based topology control
(Li, Halpern, Bahl, Wang and Wattenhofer, PODC 2001): if every cone of
5π/6 (150°) around each node holds a neighbour, the graph stays as
connected as at full power. With fixed sectors of width w, any cone of
2w contains a whole sector, so six sectors of 60° put a neighbour in
every cone of 120°, inside the bound; four of 90° only in every cone of
180°, outside it. The proof assumes a disk model; shadowing breaks it,
as the table below shows. A sector counts only if a relay in reach at
full power has been heard there, through a discovery heartbeat if not
otherwise; an empty one, at the edge of the deployment or over a river,
never raises the power, and nothing ever goes above full power.

### Static topology

The S4 relays, seeds 1 and 2, and the all-on-roofs variant, from the
simulator's link table (mean losses, no fading), with the margin of 8 dB.
"Disturbed" counts relays at which a frame raises the noise by 3 dB or
more, per hop and summed over a text's hops; "greedy" is plain greedy
forwarding without the search, over 6000 random pairs, so it shows gaps;
"hidden" is the share of the relays a receiver hears that its sender
cannot detect. Script: `topo.py` in the research scratch directory.

| Seed 1 | Mean power, dBm | Links per relay | Cut off | Hops, shortest mean / max | Greedy reaches | Greedy hops / worst | Disturbed per hop | Disturbed per text | Hidden |
|---|---|---|---|---|---|---|---|---|---|
| Fixed | 10 | 10.3 | 0 | 3.4 / 7 | 86 % | 4.3 / 14 | 47 | 204 | 48 % |
| (b) per receiver | 4.4 | 10.3 | 0 | 3.4 / 7 | 86 % | 4.3 / 14 | 30 | 131 | 57 % |
| (a) N = 4 | 2.1 | 3.1 | 7, 11 parts | 12.6 / 30 | 11 % | 4.3 / 15 | 19 | 80 | 48 % |
| (a) N = 6 | 5.6 | 4.8 | 1 | 6.5 / 14 | 39 % | 6.4 / 20 | 28 | 177 | 48 % |
| (a) N = 8 | 7.4 | 6.2 | 1 | 5.3 / 13 | 59 % | 6.0 / 20 | 33 | 197 | 49 % |
| (a) N = 12 | 8.5 | 7.8 | 0 | 4.3 / 10 | 72 % | 5.2 / 16 | 39 | 203 | 49 % |
| (a) six sectors only | 6.8 | 6.2 | 0 | 4.6 / 10 | 66 % | 5.6 / 17 | 34 | 189 | 49 % |
| (a) four sectors only | 5.2 | 4.6 | 1, 3 parts | 5.8 / 13 | 49 % | 6.5 / 19 | 28 | 182 | 49 % |
| (a) N = 8, six sectors | 8.0 | 7.1 | 0 | 4.5 / 9 | 69 % | 5.3 / 15 | 37 | 199 | 49 % |
| (c) N = 8, no discovery | 4.3 | 9.7 | 0 | 3.5 / 8 | 85 % | 4.5 / 14 | 29 | 132 | 56 % |

Seed 2 agrees, except that six sectors alone leave 2 relays cut off and
N = 8 with six sectors 1; N = 12 is again the least N that keeps every
relay. All on roofs (seed 1): fixed power gives 79 links per relay and
1.8 greedy hops, 371 relays disturbed per text; per-receiver power 238;
the degree rule sits at the −8 dBm floor for N up to 8 with 11 links per
relay, 5.1 hops and 263 to 268 disturbed per text; (c) without discovery
2.8 hops and 227.

What this says before any traffic:

- **The degree rule does not reduce interference per text.** Per hop it
  disturbs fewer relays, but there are more hops: 197 to 203 relays per
  text against 204 at full power, for 20 to 40 % more hops. Only the
  per-receiver power, which keeps the hops, disturbs fewer per text, 36 %
  fewer.
- **Connectivity needs N = 12.** The literature puts the number of
  nearest neighbours that keeps a random planar network connected at
  Θ(log n) (Xue and Kumar, Wireless Networks 2004: below 0.074 log n
  disconnected, above 5.1774 log n connected; Balister, Bollobás, Sarkar
  and Walters, Adv. Appl. Prob. 2005: between 0.3043 log n and
  0.5139 log n), 1.7 to 2.9 for n = 269. Here a link must work both ways
  with 8 dB to spare, shadowing makes "nearest" a matter of loss, and the
  domain has edges, so more are needed: N = 6 or 8 leaves one or two
  relays alone in both seeds, N = 12 none. The symmetric k-neighbour
  protocol of Blough, Leoncini, Resta and Santi (MobiHoc 2003) sets its k
  by simulation for the same reason.
- **Six sectors help connectivity, four do not**, as the cone argument
  says; with shadowing, six still miss a relay in seed 2. Spread buys
  shorter worst cases (greedy worst 15 hops at N = 8 with six sectors
  against 20 without) at 0.6 dB more mean power.
- **Hidden terminals grow under per-receiver power,** from 48 to 57 % of
  what a receiver hears: a quieter sender is detected by fewer of the
  others its receiver hears.

## Results

### Setup

`scenarios/s4-capacity.toml` and its variants as in the capacity note:
the texting rate times 1 and 2, seeds 1 and 2, and all relays on roofs at
0.1 and 1 times the rate; fading on. Delivery is over messages sent from
900 s to 3900 s. Airtime per text counts every frame that carried or
answered a delivered text. "Relay dBm" is the airtime-weighted mean
transmit power of relays; leaves always send at +10 dBm.

Main moved twice during this work, and each move changed the baseline
more than any power rule. Every comparison below is paired against the
baseline of the same commit:

- **2097d3a** (D38 and D39): the result. Rows marked n- in the scratch
  directory.
- 1592331 (D39 without D38) and af0cb7a (before both), in brief after.

### On 2097d3a: the mixed domain (S4)

| Rule | ×1 seed 1 | ×1 seed 2 | ×2 seed 1 | ×2 seed 2 | ×2 s1 carried, texts/s | ×2 s1 air per text, s | ×2 s1 hops | ×2 s1 centre at cap | ×2 s1 busy at relays | ×2 s1 relay dBm |
|---|---|---|---|---|---|---|---|---|---|---|
| Fixed | 95.1 % | 94.7 % | 94.4 % | 94.0 % | 9.15 | 0.96 | 4.6 | 2.2 % | 3.4 % | 10.0 |
| (b) per receiver | 95.4 % | 94.9 % | 94.6 % | 94.5 % | 9.18 | 0.97 | 4.5 | 1.3 % | 2.6 % | 5.8 |
| (a) N = 12 | 95.2 % | | 58.7 % | | 5.65 | 2.22 | 10.2 | 62 % | 5.4 % | 8.8 |
| (a) N = 8, six sectors | 95.1 % | 94.3 % | 45.6 % | 36.9 % | 4.37 | 2.74 | 12.1 | 70 % | 5.9 % | 8.4 |
| (c) N = 8 | 95.2 % | | 82.2 % | 70.1 % | 7.95 | 1.69 | 6.3 | 42 % | 3.8 % | 5.2 |
| (c) N = 8, six sectors | 95.5 % | 95.0 % | 90.9 % | 52.0 % | 8.80 | 1.36 | 5.4 | 26 % | 3.4 % | 5.5 |
| (c) N = 8, six sectors, no discovery | 95.4 % | | 85.9 % | | 8.31 | 1.54 | 6.0 | 38 % | 3.7 % | 5.2 |
| (c) N = 12 | 95.5 % | | 94.4 % | 77.5 % | 9.15 | 1.06 | 4.6 | 5.3 % | 2.9 % | 5.6 |
| (c) N = 12, six sectors | 95.7 % | | 92.7 % | 80.3 % | 8.98 | 1.30 | 5.3 | 20 % | 3.3 % | 5.5 |
| (c) N = 16 | 95.5 % | | 92.7 % | 81.0 % | 8.99 | 1.24 | 5.1 | 21 % | 3.5 % | 5.6 |

At the spec "Capacity and sizing" rate every rule delivers what the dark phones allow; the
domain has room. At twice the rate:

- **Per-receiver power changes nothing that matters.** It lowers the
  relays' mean power by 4 dB and cuts the channel busy time at relays by
  a quarter, but the channels were 3.4 % busy. The cost per text and the
  hops stay.
- **The degree rule doubles the hops and collapses.** With 10 to 12
  hops per text, the centre relays spend 62 to 70 % of the time at the
  duty cap, and delivery falls to 37 to 59 %.
- **Rule (c) costs between 0 and 42 points**, more in seed 2 and more
  at small N. The windowsill relays keep +8 to +10 dBm of operating
  power, so their routes barely change; the rooftop relays fall to +1 to
  +4 dBm at N = 8, +5 at N = 12 and +7.7 at N = 16, and the domain loses
  part of the 3 km rooftop hops that carried its long routes. At twice
  the rate the domain is near the knee of the capacity note, where a
  small loss of capacity shows as a large fall in delivery, so the
  spread between seeds is wide. Sectors and the discovery rotation do
  not change this consistently.

### On 2097d3a: all relays on roofs

| Rule | ×0.1 | ×1 | ×1 carried, texts/s | ×1 air per text, s | ×1 DATA frames per hand-over | ×1 busy at relays | ×1 leaves without relay | ×1 operating dBm |
|---|---|---|---|---|---|---|---|---|
| Fixed | 95.0 % | 7.9 % | 0.38 | 28.9 | 19.2 | 41.4 % | 9130 | 10 |
| (b) per receiver | 95.7 % | 12.9 % | 0.62 | 9.7 | 22.1 | 27.9 % | 3808 | 10 |
| (a) N = 12 | | 58.3 % | 2.24 | 1.27 | 1.7 | 3.2 % | 6833 | −5.4 |
| (a) N = 8, six sectors | | 61.5 % | 2.47 | 1.21 | 1.7 | 3.7 % | 5891 | −4.9 |
| (c) N = 8 | | 90.7 % | 4.36 | 0.77 | 2.1 | 4.6 % | 698 | −6.5 |
| (c) N = 8, six sectors | 95.5 % | 90.7 % | 4.37 | 0.75 | 2.1 | 4.7 % | 664 | −4.8 |
| (c) N = 8, six sectors, no discovery | | 68.2 % | 3.08 | 0.80 | 2.6 | 3.5 % | 4460 | −5.2 |
| (c) N = 12 | | 90.5 % | 4.35 | 0.76 | 2.1 | 4.7 % | 657 | −5.3 |
| (c) N = 12, six sectors | | 83.7 % | 4.03 | 1.25 | 6.2 | 8.7 % | 2850 | −4.0 |
| (c) N = 16 | | 90.8 % | 4.36 | 0.71 | 2.1 | 4.6 % | 656 | −3.3 |

Here the channel binds: at full power every relay hears every other's
heartbeats and hand-overs, 41 % of each channel is busy, and 19 DATA
frames go out per hand-over that succeeds (found when D39 was measured).

- **Per-receiver power for data is not enough.** Heartbeats at full
  power keep all 269 relays in reach of each other and the tables full;
  greedy forwarding still picks the relay 3 km away, which needs full
  power, and the channel stays 28 % busy.
- **Lowering the heartbeats too is what works.** Under (c) the relays
  settle at −3 to −7 dBm, the channel at relays falls to 4.7 % busy and
  delivery rises from 7.9 to 90.7 %, all the texts offered but those of
  dark phones and leaves left without a relay.
- **Discovery is needed.** Without the full-power rotation (c) delivers
  68 %, and 4460 leaves end without a relay against 664: a leaf, and a
  newcomer relay, must be able to hear relays that whisper.
- **The degree rule (a) is half the cure.** It lowers the interference
  as much, but whispering DATA to leaves and the both-ends rule cost
  leaves (5900 to 6800 without a relay) and longer routes.
- **N matters little between 8 and 16, and sectors do not help.** The
  relays sit within 5 dB of the −8 dBm floor whatever N, so the floor,
  more than N, sets the hearing distance, about 1 to 1.4 km.

### Stability

Under (a) and (c) on roofs a relay's operating power changed 3 to 10
times an hour, one change every 6 to 20 minutes; in the mixed domain 0.6
to 4.5 times, fewer at larger N. The rule raises at once and lowers only
by 4 dB, and each relay recomputes after its own heartbeat, 5 to 15 s
apart at random, so relays do not adjust together; what one relay
changes reaches the others' tables only through its next heartbeats, a
rotation later. A new relay starts at full power, and its table fills
from the others' discovery rotations within three rotations
(6.5 minutes). The mean operating powers stayed where the static
analysis puts them, and by construction none went below the floor or
above full power. The control is slow and coarse on purpose: what it
controls, who can be reached, changes with the deployment, not with the
fading.

### Earlier baselines, in brief

On **1592331** (D39 without D38), at twice the rate: full power 34.0 %;
a hard filter that keeps out neighbours a full frame at full power does
not reach with 8 dB (the prototype's `reach_filter`) 80.0 %; that filter
and per-receiver power 94.3 %, as much as D38's ranking gives alone;
per-receiver power without the filter 28.5 %. On roofs at the spec "Capacity and sizing" rate
(c) N = 8 with the filter delivered 90.2 % against 7.2 %. So per-receiver power helped
only while far, faint neighbours were being tried first.

On **af0cb7a** (before D38 and D39), keepalive ACKs were the largest load
on every channel. There per-receiver power on top of the filter added 9
to 18 points at the spec "Capacity and sizing" rate (92.1 against 83.4 % in seed 1, 80.0
against 61.9 % in seed 2), mostly by sending the keepalive ACKs quieter.
**This conclusion changed with D39**: with keepalives gone, the same rule
adds nothing. The all-on-roofs domain failed under every rule (0 to 50 %)
because of the keepalive ACKs.

### Leaves and losses

Leaves without a relay at the end, of 20 000: 474 at full power at twice
the rate (seed 1), 449 under (b), 658 under (c) N = 8, 877 to 978 under
(a); on roofs at the spec "Capacity and sizing" rate 9130 at full power (the relays are too busy
to poll), 664 under (c), 5900 to 6800 under (a). A leaf chooses its
relay by heartbeat RSSI (spec "Relay choice"), and under (a) a relay's polls and
deliveries go at its whispering power, so a leaf that heard a discovery
heartbeat may attach to a relay that cannot reach it. Under (c) polls
and deliveries go at the power the leaf's own loss needs.

Nothing was lost without a trace in any run but one: under (a) N = 8 with
six sectors, twice the rate, seed 2, one message (tag 28629, originated
at 3416 s) ended unaccounted. A trace shows it queued for its leaf at its
destination relay at 3485 s; the leaf answered that relay's poll at
3710 s, and no DELIVER, delivery or expiry of the message followed to the
end of the run, although the relay delivered and expired others. Not
found. One candidate is two messages sharing a 32-bit message ID: the
run originated 39 000 messages, which makes a shared ID likely
(39 000² / 2³³ ≈ 0.18), and the simulator follows messages by ID. It
should be found whatever the power rule; the run reproduces it.

### The city

A 28 km² domain with shrunken hearing is part of a larger city: a frame
that no longer disturbs the far side of its own domain no longer
disturbs the next domain either, so whatever a rule does to the channel
inside a domain it does across borders too. The per-domain figures are
therefore the city's per-area figures as long as traffic stays local.
Traffic that crosses the city does not, and the run that shows it is a
2 × 2 tiling: 112 km², the same densities (1075 relays, 80 000 leaves),
the spec "Capacity and sizing" texting rate, destinations uniform over the whole area, seed 1,
on 2097d3a. Each run took 47 to 51 minutes and 1.3 GB.

| Rule | Offered, texts/s | Delivered | Carried, texts/s | Air per text, s | Hops | Busy at relays | Relay dBm |
|---|---|---|---|---|---|---|---|
| Fixed | 19.2 | 24.4 % | 4.69 | 5.1 | 11.3 | 8.1 % | 10.0 |
| (b) per receiver | 19.2 | 10.7 % | 2.06 | 7.6 | 13.6 | 6.4 % | 5.5 |
| (c) N = 12 | 19.2 | 8.4 % | 1.61 | 9.2 | 14.7 | 6.2 % | 5.1 |

- **Four domains carry no more than one.** Texts go twice as far (5.5 km
  on average), every relay carries traffic passing through, and the
  relays sit at the duty cap 49 % of the time on average (none in one
  domain at this rate): 4.7 texts a second for 80 000 users, against 4.6
  for 20 000 in one domain. Scaling one
  domain's figure by the number of domains, as the capacity note did,
  overstates a city whose texts cross it.
- **Power control is worse in overload.** Both rules lose more than half
  of what full power carries. A frame sent 8 dB above sensitivity is
  more easily lost to the interference of an overloaded domain than one
  sent at full power, each loss costs retries, and the retries cost the
  duty budget that binds. One confound: with power control on, a
  neighbour's DATA and ACKs no longer feed the D38 average (their power
  is not stated), so D38 judges margin from heartbeats only.

## What the table and the wire need

For rule (c), and for any later one:

- **Heartbeat:** the five spare position bits state the sender's power:
  four bits of operating power in 2 dB steps below +10 dBm, and one for
  "this one at full power". All zero means +10 dBm, today's heartbeat.
- **Neighbour table:** no new measurement. D38 already keeps an averaged
  RSSI per neighbour; it becomes the RSSI as if at full power, by adding
  back what a heartbeat states it was below full power, and the loss is
  full power less it. Frames that state no power (DATA, ACK) count only
  while every frame goes at full power. One more byte per entry: the
  neighbour's operating power, for the both-ends rule of (a). An entry
  stays 48 bytes.
- **Leaf table:** the loss to the leaf from its last ATTACH, one byte in
  whole dB; an entry stays 16 bytes.
- **Frame:** carries its power, which the driver applies under its
  ceiling and the simulator adds to its link table.

## What the bench must measure first

1. Output power against setting, −9 to +10 dBm, on several boards, at
   433.17 and 434.67 MHz: the loss estimate takes the setting as the
   power.
2. RSSI against a known input level, −120 to −60 dBm, per board: an
   offset between two boards enters the loss one for one.
3. Reciprocity between two boards with their own antennas: the loss
   measured each way, on several channels, over hours. The difference is
   what the 8 dB margin must also cover.
4. The noise floor of a rooftop and a windowsill site: how far "b hears
   a" differs from "a hears b" at the same loss.
5. The PA ramp: SetTxParams' ramp time (the driver uses 40 µs) at low
   power, against the adjacent-channel splatter of bench item C1.

## What this does not show

- Fading and shadowing are the scenarios' unmeasured 4 and 6 dB.
- One domain with edges and uniform destinations; the tiling (above) is
  one seed at one rate.
- Under (b) and (c) the D38 average takes heartbeats only, because DATA
  and ACK do not state their power. Stating it in DATA and ACK (no spare
  bits there) would remove the confound.
- The gate that would switch (c) on only where the channel binds is not
  built or tested. A relay could use its own busy estimate (spec "Busy estimate"), which
  in these runs was 2 to 7 % at relays in the mixed domain and 41 % on
  the roofs.
- O4, a rooftop table spread around the relay instead of D12's
  incumbents, is not measured. Under (c) the rooftop tables stay full (63
  of 64 entries on average) because discovery heartbeats admit every
  relay in reach; whether evicting by spread would let a whispering
  rooftop relay keep its useful neighbours is the follow-up.
- Leaves send at full power under every rule.

## Draft decision row

| D40 (draft) | **Transmit power stays at the ERP limit; heartbeats state it.** Heartbeats carry the sender's transmit power in their five spare position bits (all zero: +10 dBm), and every frame goes at +10 dBm. The heartbeat-and-data power rule (c), operating power for the 12th-nearest neighbour by loss with 8 dB of margin, one heartbeat rotation in three at full power, data at the power each receiver's loss needs, is the measured remedy where relays all hear each other, and is not adopted until a relay can tell, from its own channel load, that it is in such an area. | `power-research-2026-10-15.md`: in S4 the duty cycle binds; per-receiver power changes nothing (94.6 against 94.4 % at twice the spec "Capacity and sizing" rate), the degree rule costs 35 to 57 points, and (c) up to 17 points in seed 2. Where all relays are on roofs (c) takes delivery from 7.9 to 90.5 %. |

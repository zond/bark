# The neighbour table filled by sector

**Date:** 2026-10-21, roadmap D69. **Status:** measured; decided
2026-10-21 (roadmap D69): not adopted, the table of D12 with D68's place
for a relay within 50 m stays, the simulator's `--tables` kept, and spec
"Neighbour table" says why the first relays heard are the ones to keep.
Spec references are to `protocol.md`. The base is main
`fb3a0c7`, the table of roadmap D12 with D68's place for a relay within
50 m, run with this branch's simulator instruments, which repeat it to the
byte on S4 (`scenarios/s4-capacity.toml`).

The question: the neighbour table holds the first 64 relays heard and
replaces only demoted, stale or, for a relay within 50 m, far entries.
That is arrival order, not value. The rule measured, which
the branch's spec "Neighbour table" gave as a proposal, gives the 64 places by sector: 8
sectors of 45° around the node, 8 places each; in a sector every relay
within 50 m, then the closest until 4 are held so, then the farthest;
relays with margin before those without, demoted and silent entries
last, sleepy nodes with an anchor only in places no relay takes. A newcomer takes the place of an
entry it outranks, or is turned away.

## Summary

- **At the spec rate the rule changes nothing measurable.** Delivered of
  the texts sent in the settled window, seeds 1 and 2: S4 94.20 / 93.89 %
  on main against 93.97 / 94.10 %; 1300 relays 94.95 / 95.45 % against
  94.75 / 95.25 %; 5317 relays 97.43 / 97.50 % against 97.39 / 97.53 %;
  20 269 relays 99.59 / 99.52 % against 99.66 / 99.51 %. Texts claimed by
  the wrong owner stay as few as with D68's place for the near relay: 5 /
  10 against 8 / 4 at 5317 relays, 16 / 22 against 8 / 16 at 20 269.
- **At three times S4's rate it loses 6 and 9 points:** 73.40 / 70.64 %
  on main against 67.47 / 61.32 %. Routes grow longer, median hop count 6
  → 7 and p90 12 / 13 → 15 / 18, and a delivered text takes 3.0 / 3.5 s
  of airtime against 2.5 / 2.7 s. Every form of the rule tried loses
  alike, 60 to 70 %: fill to 0, 2, 4, 6 or 8 closest, the variants below,
  the sector cap alone with incumbents kept as D12 keeps them, and the
  ranking alone without sectors.
- **Among dense relays it costs airtime and latency, from one heartbeat
  judged as margin.** At 5317 relays failed hand-overs rise from 22 to 25
  and 26 % of all, airtime per delivered text 1.82 / 1.76 → 1.89 / 1.92 s,
  latency p50 2.1 → 3.5 / 3.7 s; at 20 269 relays failed hand-overs 24 →
  29 %, airtime 1.75 → 2.10 s, latency p50 4.1 → 4.8 / 5.0 s. The far places
  go to the farthest relays with margin, and a newcomer's margin is one
  heartbeat: those placed far were mostly heard at the top of a fade.
  Ranking a relay heard once with margin only when it is the 4 dB fade
  above it (variant A) takes this back and more: failed hand-overs 22.0 /
  22.7 %, airtime 1.74 s at 5317 relays and 1.66 / 1.68 s at 20 269,
  latency as main, delivered 97.31 / 97.50 % and 99.69 / 99.74 %. It does
  not help at three times S4's rate (67.98 / 60.78 %).
- **Long hops stay where load is low:** median hop count 8 → 7 at 5317
  relays and 9 → 8 at 20 269, p90 14 / 15 → 13 and 17 / 16 → 14; at S4's
  rate 4 / 5 both. Under load they do not: see above.
- **Anchors are not harmed.** At 5317 relays a sleepy node's anchor is the
  strongest relay it hears 50.3 / 49.9 % of the time on main and 49.6 /
  49.7 % with sectors, 8 dB or more below it 24.2 / 24.9 % against 24.3 /
  24.9 %; anchor changes 0.557 / 0.541 against 0.569 / 0.546 a node an
  hour; current 0.536 / 0.531 against 0.536 / 0.533 mA. S4 and 1300 relays
  alike. No fix is needed for anchoring.
- **The sparse indoor relays keep tables.** At 1300 relays 501 / 531 of
  them average fewer than 4 entries with margin on main, 501 / 543 with
  sectors; none has an empty table, the fewest averaging 5 to 7 entries.
  Texts to those relays arrive 97.9 / 97.5 % on main against 96.0 / 95.5
  %, to their sleepy nodes 91.3 / 92.6 % against 90.0 / 91.1 %.
- **Fill to 4 does not matter.** Fill to 2 and fill to 6 are within the
  spread of fill to 4 in every run, and fill to 0 and 8 at three times S4's
  rate too.
- **Recommendation:** do not adopt the rule. Keep D12 with D68's place for
  a relay within 50 m, which gives the owner rule the same table. If a
  spread table is wanted later, it must first win at three times S4's
  rate; variant A is the form to start from.

## What was changed

Kept on the branch:

| Commit | What |
|---|---|
| Simulator: what the neighbour tables hold, and whether a sleepy node's anchor is the strongest it hears | with `--tables`, once a minute after the settle time each running relay's entries and entries with margin, entries by sector; every five minutes, of the sleepy nodes that do not move, whether the anchor is the strongest relay they hear (base link above sensitivity, before fading), how far below it, and whether the strongest is in their table. A newcomer turned away or placed is counted whether or not the table is full |
| Candidate D69: the neighbour table filled by sector | the rule (`tables/neighbours.rs` `sector`, `keep`, `rank`), replacing D12's eviction and D68's `near_victim`; spec "Neighbour table" as a proposal; unit tests for the sectors, the near and far places, the owner radius, margin first, stale and demoted last, the sleepy node, the pinned anchor, a relay that moves and a node that moves |
| Simulator: who keeps whom, how far DATA reaches, and what makes way | with `--tables`: how many relays keep each relay, entries with margin by distance, DATA between relays by margin and distance, sent and received, and the entries that leave a table as a newcomer is heard |

S4's JSON and messages CSV are unchanged to the byte by both simulator
commits.

Measured and reverted on top:

| Candidate | What |
|---|---|
| A: one heartbeat a fade above the margin | a relay heard over fewer than two heartbeats ranks with margin only when its RSSI is at least the margin and the 4 dB fade of spec "Neighbour table" (`FADE_DEPTH_DB`) |

Measured as builds of the simulator only, to find what costs at three
times S4's rate, not committed:

| Form | What |
|---|---|
| fill to 0, 2, 6, 8 | the closest places other than 4 (`NEAR_PLACES`); 0 is the relays within 50 m and the farthest, 8 the closest only |
| B | far places only for relays with margin; the others the closest |
| one sector | the same ranking over one sector of 64 places, 32 near |
| cap only | 8 places a sector, but a newcomer takes only a demoted or stale entry's place, or, within 50 m, one beyond it: D12 inside each sector |
| sleepy kept | a sleepy node with an anchor keeps its place until it is stale, as under D12 |
| D12 again | the rewritten table with D12's admission: S4 repeats main to the byte, a check that the losses are the rule's and not the code's |

## What was run

S4 at its spec rate and at three times it, S4 with 5 % of the people's
nodes as relays (1300 relays), `scenarios/dense.toml` (5317) and every
person's node a relay (20 269), as roadmap D68 ran them. Every number is
of the texts sent in the settled window, from 2700 s to the end, those
still on their way at the end counted as not delivered; seeds 1 and 2
are given as "1 / 2". S4, its three times and the densities to 5317 ran
6300 s; 20 269 relays ran 4500 s. Up to eight runs at once on 32 cores, 3.6
to 3.8 GB each: S4 5 min, three times S4 16 min, 1300 relays 16 min,
5317 relays 25 min, 20 269 relays 41 to 44 min.

## Results

Main is fb3a0c7; sectors is the rule, filling to 4; fill 2 and fill 6
are the same rule with 2 and 6 closest places; A is the rule with
candidate A.

### S4, spec rate

| | main | sectors | fill 2 | fill 6 | A |
|---|---|---|---|---|---|
| delivered | 94.20 / 93.89 % | 93.97 / 94.10 % | 94.19 / 94.13 % | 94.15 / 94.30 % | 94.05 / 94.26 % |
| latency p50 / p90 | 1.5 / 184 s | 1.5 / 183 s | 1.5 / 184 s | 1.5 / 184 s | 1.5 / 184 s |
| hops, median (p90) | 4 / 5 (7) | 4 / 5 (7) | 5 (7) | 4 / 5 (7) | 5 (7) |
| airtime per delivered text | 1.32 / 1.42 s | 1.32 / 1.44 s | 1.34 / 1.43 s | 1.32 / 1.39 s | 1.33 / 1.41 s |
| entries; relays with margin | 63.9; 17.7 | 60.2; 17.6 | 60.2; 17.6 | 60.1; 17.6 | 60.2; 17.1 |
| newcomers turned away / placed, a relay an hour | 900 / 111 | 946 / 117 | 936 / 118 | 936 / 117 | 963 / 115 |
| relays no other keeps with margin | 0 | 0 | 0 | 0 | 0 |
| anchor the strongest relay heard; 8 dB or more below it | 74.1 %; 7.6 % | 74.4 %; 7.6 % | 74.4 %; 7.5 % | 74.0 %; 7.7 % | 74.3 %; 7.6 % |
| anchor changes a sleepy node an hour | 0.457 | 0.457 | 0.459 | 0.461 | 0.460 |
| sleepy node's current | 0.631 mA | 0.634 mA | 0.633 mA | 0.632 mA | 0.634 mA |

No text goes to a relay in S4, so none is claimed by the wrong owner.

### S4, three times the rate

| | main | sectors | fill 2 | fill 6 | A |
|---|---|---|---|---|---|
| delivered | 73.40 / 70.64 % | 67.47 / 61.32 % | 66.91 % | 69.91 % | 67.98 / 60.78 % |
| latency p50 / p90 | 114 / 569 s | 164 / 629 s | 166 / 637 s | 154 / 617 s | 156 / 619 s |
| hops, median (p90) | 6 (12 / 13) | 7 (15 / 18) | 7 (15) | 7 (15) | 7 (15 / 18) |
| airtime per delivered text | 2.51 / 2.73 s | 3.01 / 3.47 s | 3.03 s | 2.83 s | 2.95 / 3.53 s |
| failed hand-overs, of all | 18.9 / 17.8 % | 17.1 / 16.7 % | 17.0 % | 16.8 % | 16.7 / 16.8 % |
| entries; relays with margin | 63.9; 15.2 | 59.5; 15.8 | 59.4; 15.7 | 59.5; 15.8 | 59.4; 15.6 |
| newcomers turned away / placed, a relay an hour | 650 / 128 | 666 / 112 | 651 / 112 | 657 / 111 | 661 / 111 |
| anchor the strongest; anchor changes an hour; current | 74.6 %; 0.517; 0.712 mA | 74.2 %; 0.517; 0.708 mA | 74.3 %; 0.510; 0.711 mA | 74.7 %; 0.516; 0.712 mA | 74.4 %; 0.505; 0.707 mA |

What costs there, seed 1 / 2, delivered and airtime per delivered text:

| form | delivered | airtime |
|---|---|---|
| main | 73.40 / 70.64 % | 2.51 / 2.73 s |
| D12 again | 73.40 % | 2.51 s |
| sectors | 67.47 / 61.32 % | 3.01 / 3.47 s |
| fill 0 | 65.13 / 63.31 % | 3.20 / 3.33 s |
| fill 8 | 68.67 / 62.73 % | 2.92 / 3.35 s |
| B | 66.98 / 62.39 % | 3.05 / 3.39 s |
| one sector | 67.04 / 62.26 % | 3.03 / 3.37 s |
| cap only | 67.21 / 60.32 % | 3.02 / 3.58 s |
| sleepy kept | 69.00 / 62.37 % | 2.87 / 3.42 s |

With sectors, seed 1, against main: DATA between relays +7 % of airtime,
copies of split hand-overs +17 %, offers the origin's node refused for
overload (spec "Overload waits") +35 %. Fewer relays with margin left the
tables as newcomers came (5604 against 15 226), so the set does not churn
more; the DATA sent to relays less than 500 m away rose 24 %.

### 1300 relays

| | main | sectors | fill 2 | fill 6 | A |
|---|---|---|---|---|---|
| delivered | 94.95 / 95.45 % | 94.75 / 95.25 % | 94.68 % | 95.02 % | 94.75 / 94.81 % |
| latency p50 / p90 | 4.2 / 158 s | 4.3 / 169 s | 4.4 / 176 s | 4.3 / 167 s | 4.4 / 170 s |
| hops, median (p90) | 6 (12) | 6 (13) | 6 (14) | 6 (13) | 6 (14 / 13) |
| airtime per delivered text | 5.41 / 5.21 s | 5.47 / 5.75 s | 5.91 s | 5.70 s | 5.71 / 5.78 s |
| texts claimed by the wrong owner | 1 / 0 | 1 / 0 | 0 | 0 | 0 / 0 |
| relays no other keeps with margin | 19 / 29 | 18 / 30 | 20 | 19 | 23 / 32 |
| entries; relays with margin: S4's relays | 63.9; 25.3 | 60.7; 25.6 | 60.7; 25.5 | 60.7; 25.6 | 60.6; 24.3 |
| the same, the people's relays | 25.2; 4.2 | 24.1; 4.2 | 24.4; 4.1 | 24.2; 4.2 | 24.1; 4.2 |
| relays with fewer than 4 relays with margin | 501 / 531 | 501 / 543 | 510 | 505 | 501 / 540 |
| texts to them delivered | 97.9 / 97.5 % | 96.0 / 95.5 % | 97.9 % | 97.6 % | 96.7 / 94.7 % |
| texts to their sleepy nodes delivered | 91.3 / 92.6 % | 90.0 / 91.1 % | 90.1 % | 91.2 % | 90.3 / 90.5 % |
| newcomers turned away / placed, a relay an hour | 161 / 47 | 169 / 51 | 164 / 51 | 164 / 51 | 165 / 50 |
| anchor the strongest; anchor changes an hour; current | 63.6 %; 0.485; 0.578 mA | 63.3 %; 0.489; 0.580 mA | 62.8 %; 0.481; 0.580 mA | 63.3 %; 0.489; 0.577 mA | 63.3 %; 0.474; 0.577 mA |

### 5317 relays

| | main | sectors | fill 2 | fill 6 | A |
|---|---|---|---|---|---|
| delivered | 97.43 / 97.50 % | 97.39 / 97.53 % | 97.47 / 97.39 % | 97.46 / 97.48 % | 97.31 / 97.50 % |
| latency p50 / p90 | 2.1 / 12 s | 3.5 / 14 s | 3.4 / 13 s | 3.5 / 14 s | 2.0 / 12 s |
| hops, median (p90) | 8 (14 / 15) | 7 (13) | 7 (13) | 7 / 8 (14) | 7 / 8 (14) |
| airtime per delivered text | 1.82 / 1.76 s | 1.89 / 1.92 s | 1.96 / 1.98 s | 1.95 / 1.95 s | 1.74 / 1.74 s |
| failed hand-overs, of all | 21.9 / 22.7 % | 24.7 / 26.4 % | 24.6 / 26.1 % | 24.7 / 25.7 % | 22.0 / 22.7 % |
| texts claimed by the wrong owner | 5 / 10 | 8 / 4 | 5 / 13 | 8 / 7 | 6 / 6 |
| relays no other keeps with margin | 1 / 3 | 1 / 0 | 1 / 1 | 1 / 0 | 1 / 0 |
| entries; relays with margin: S4's relays | 64.0; 41.8 | 62.0; 52.3 | 61.9; 52.2 | 61.9; 52.3 | 61.9; 45.3 |
| the same, the people's relays | 39.8; 10.9 | 38.2; 10.9 | 38.1; 10.9 | 38.2; 10.9 | 38.2; 10.9 |
| newcomers turned away / placed, a relay an hour | 100 / 12 | 97 / 41 | 96 / 42 | 98 / 40 | 104 / 36 |
| anchor the strongest; 8 dB or more below it | 50.3 / 49.9 %; 24.2 / 24.9 % | 49.6 / 49.7 %; 24.3 / 24.9 % | 50.4 / 49.9 %; 24.0 / 25.0 % | 50.0 / 50.1 %; 24.4 / 25.0 % | 50.0 / 50.0 %; 24.4 / 25.1 % |
| anchor changes a sleepy node an hour | 0.557 / 0.541 | 0.569 / 0.546 | 0.543 / 0.532 | 0.578 / 0.558 | 0.570 / 0.555 |
| sleepy node's current | 0.536 / 0.531 mA | 0.536 / 0.533 mA | 0.535 / 0.532 mA | 0.536 / 0.533 mA | 0.534 / 0.532 mA |

### 20 269 relays

| | main | sectors | fill 2 | fill 6 | A |
|---|---|---|---|---|---|
| delivered | 99.59 / 99.52 % | 99.66 / 99.51 % | 99.62 % | 99.59 % | 99.69 / 99.74 % |
| latency p50 / p90 | 4.1 / 11 s | 4.8 / 14 s | 4.7 / 14 s | 5.0 / 15 s | 3.9 / 11 s |
| hops, median (p90) | 9 (17 / 16) | 8 (14) | 8 (14) | 9 (14) | 9 (15) |
| airtime per delivered text | 1.75 / 1.75 s | 2.10 / 2.10 s | 2.07 s | 2.17 s | 1.66 / 1.68 s |
| failed hand-overs, of all | 23.5 / 24.4 % | 28.8 / 29.8 % | 29.1 % | 28.8 % | 24.0 / 24.8 % |
| texts claimed by the wrong owner | 16 / 22 | 8 / 16 | 12 | 7 | 11 / 4 |
| relays no other keeps with margin | 0 | 0 | 0 | 0 | 0 |
| entries; relays with margin, a relay | 63.7; 32.4 | 61.5; 35.7 | 61.5; 35.7 | 61.5; 35.5 | 61.5; 31.1 |
| newcomers turned away / placed, a relay an hour | 359 / 189 | 321 / 329 | 321 / 335 | 324 / 307 | 357 / 320 |

Every sector holds about the same: entries with margin, relays and sleepy
nodes together, differ by 0.3 or less between the fullest and the
emptiest sector in every run, on main too.

## The checks

**Anchor choice.** A sleepy node takes the strongest relay it hears from
its own table (spec "Relay choice"), which follows the same rule, so a
strong relay through a window could fall out of a sector's places behind
nearer ones. It does not show: the share of sleepy nodes whose anchor is
the strongest relay they hear, how far below it the others are, the
anchor changes and the current are within the seeds' spread of main in
every run. Half the nodes at 5317 relays have an anchor other than the
strongest on main as well; that comes from choosing by heartbeats under
fading and switching only 8 dB up, not from the table. No fix was
needed, so none was measured.

**Long hops.** Where load is low the hop counts fall a little, the far
places doing what they are for. The airtime per text rises nonetheless,
by failed hand-overs to far relays placed on one lucky heartbeat;
candidate A removes that. Under load at three times S4's rate the hop
counts rise, in every form.

**The sparse indoor case.** At 1300 relays about 40 % of the relays
average fewer than 4 relays with margin, on main and with sectors alike,
and none has an empty table: margin ranks first, but a relay without
margin still takes a place no relay with margin claims. Texts to those
relays lose about 2 points with sectors, to their sleepy nodes 1.3 and
1.5.

**Candidate 4 of D68** is gone on the branch: the rule's first places are the relays
within 50 m, and the branch's spec "Neighbour table" and module documentation
describe only the sector rule.

## Why

**The spec rate.** Delivery at S4's rate and among dense relays is
limited by the duty budget where routes meet and by marginal links, not
by which 64 relays a table holds. At S4's 9.6 relays per km² a relay
keeps nearly every relay with margin it hears under either rule (17.7
and 17.6 on average); among dense relays any 64 hold relays in every
direction, and the relay next door is held by D68's place already.

**Dense relays.** The rule puts the farthest relays with margin in a
sector's far places, so every relay with margin heard from farther than
the current farthest takes a place. A newcomer's margin is one heartbeat,
which under 4 dB of fading is as likely the top of a fade as its mean;
the farther the relay, the more of those placed far were heard so. Their
average then falls below the margin, their hand-overs fail, they rank
below the next such newcomer and make way: placements a relay an hour
rise 3.4 times at 5317 relays and 1.7 times at 20 269. Asking a
heartbeat heard once to be a fade above the margin (candidate A) stops
it.

**Three times S4's rate.** Here relays at the duty cap refuse and are
banned (spec "Refusal"), and a message goes to the next best neighbour
toward its destination. With sectors, routes are longer and spend more
airtime, and every form measured loses, the sector cap alone and the
ranking alone as much as both. What these runs show is that the table
D12 keeps does better under load; why is not shown. A reading consistent
with the longer routes: the first 64 relays heard lie where the relays
are, mostly toward the domain's centre, where most destinations are too,
and a fixed 8 places a direction, given to the nearest and farthest,
leave fewer neighbours there to turn to when the best is banned.

## What this does not show

- One domain. Whether sectors help where a relay has no relay in some
  direction within its first 64, a city's edge or a corridor, was not run;
  S4's relays have neighbours all round.
- The reason for the loss under load (above); and with it, a rule that
  spreads the table and wins there. Places in proportion to the relays
  heard in each direction is the obvious next form; it was not built.
- Two seeds for every comparison but fill 2 and fill 6 at three times
  S4's rate, 1300 and 20 269 relays, which ran one.
- The anchor check samples sleepy nodes that do not move, every five
  minutes; it does not look at the moment of choice.

## Recommendation

1. **Do not adopt D69 as ruled.** It is neutral at the spec rate, costs
   airtime and latency among dense relays, and loses 6 to 9 points at
   three times S4's rate.
2. **Keep D12 with D68's place for a relay within 50 m.** It holds the
   relay next door for the owner rule as well as the sector rule does:
   texts claimed by the wrong owner 5 / 10 against 8 / 4 at 5317 relays,
   16 / 22 against 8 / 16 at 20 269.
3. **If a spread table is taken up again,** start from candidate A, which
   is the better form among dense relays (airtime per text 1.74 s against
   1.82 at 5317 relays, 1.66 against 1.75 at 20 269), and require it to
   win at three times S4's rate before anything else.

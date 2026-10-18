# Two small items: random IDs and DATA to sleeping nodes; a beat after following the anchor

Research note, 2026-10-18, roadmap D57. Measured against main `8704790`
(D52). Spec references are to `protocol.md`.

## Summary

- **Random IDs do not raise the DATA sent to sleeping nodes.** Nearly all
  of it, 99.9 % of 4 011 such frames over twelve runs, goes from a node
  without an anchor to a neighbour it still keeps as anchorless, which
  has since found an anchor and sleeps: the stale entry that D51 accepted.
  How many a run depends on a handful of pockets at the edge of relay
  coverage, 0 to 846 a run in one configuration, and switching to random
  IDs changes it by +16 ± 49 over six seeds on main. D52's doubling was two
  draws from that spread: on D52's own code the same two seeds repeat it,
  and the next four go the other way, +136 ± 200 over the six.
- **Forgetting a stale anchorless neighbour after two failed hand-overs**
  cuts these frames by two thirds where they matter, a village beside a
  district whose relays come on later, but loses 2.3 points of its
  delivery: the entry is often the only way out of the village. In S4 it
  saves 2.4 % of the airtime and loses 0.19 ± 0.22 points. Not built.
- **A beat after following the anchor (G) gains nothing in S4 as it now
  is**, −0.04 ± 0.06 points over four seeds, for 53 s more beat airtime a
  run (+0.16 %). D53's gain of half a point came from the simultaneous
  boot, whose relays choose their first channels badly and move often in
  the measured hour: there G gains +0.32 ± 0.13 now. Switched on over a
  window, relays choose among relays already running and move a third as
  often, 75 and 85 times in the measured hour against 216 and 355. With a
  tenth of the relays removed, which sets their neighbours moving, 353
  times, it gains +1.01 ± 0.39 points (four seeds, every one), and
  +0.88 ± 0.42 with the removal later (two seeds), for 0.3 to 0.4 % more
  airtime. The D42 suites deliver as main within their spread. Built, as
  its own commits.

## 1. Random IDs and DATA to sleeping nodes

### What D52 saw

D52 counts, after the settle time, every DATA sent to a sleepy node while
its radio sleeps, by whether the sender is the node's anchor. With
switch-on over a window (its model B) the nodes that are not the anchor
sent 486 and 377 a run (S4 at the spec "Capacity and sizing" rate, seeds
1 and 2); with random IDs added (model I), 860 and 1 224. The senders were
not traced.

### Who sends, through what entry, how old

The simulator, built with a line per such frame (not committed), names
the sender, the sender's neighbour table entry for the node, the frame's
preamble and whether the node is the frame's destination or a hop on its
way. Run on main with the scenario of D52's models B and I (switch-on over
a window, true clocks, nobody walking), seeds 1 to 6 of each, 4 011
frames:

| | Share |
|---|---|
| A hop on the way, not the frame's destination | 100 % |
| The sender keeps the node as an anchorless neighbour | 99.9 % |
| Sent with the long preamble | 99.9 % |
| The sender is a sleepy node without an anchor | 95.8 % |
| The sender is the node's previous anchor | 0.4 % |
| The sender holds the node in its heard table | 0 |
| The node had slept 5 s or more | 91 % |

The entry was last renewed, by a heartbeat or an ACK from the node, 1 035 s
before the frame at the median, 440 s at the 10th percentile and 1 492 s
at the 90th: within the 1 591 s an anchorless entry is kept (spec
"Neighbour table"). So the frames are hand-overs from nodes in anchorless
mode, which route as relays, to a neighbour that was anchorless when they
last heard it and has since taken an anchor. It now beats only on its
anchor's channel, and the anchorless nodes, receiving on their own
channels, never hear it; their entry for it lives out its time, and as it
is often the only neighbour on the way out of their pocket they keep
trying it, each hand-over three long frames and a ban after it. This is
the cost D51 named when it set the expiry at two of an anchorless node's
rotations: "a stale entry, of a node that has since found an anchor, costs
the attempts of a hand-over and a ban".

With a further line each time a sleepy node enters or leaves anchorless
mode (I, seed 2), each of the 21 sleeping nodes these frames went to had
spent one long spell in the mode, from 10 minutes to an hour, before it
found an anchor; 51 of the 63 renewals of the entries were its beats in
that spell, the rest its ACKs when it happened to listen.

It comes from a few places: up to 56 sender and node pairs a run,
up to 21 sleeping nodes, and the count follows the number of nodes that never
find an anchor (correlation 0.65 over the twelve runs), the edge of relay
coverage.

### Random IDs do not cause it

| Seed | 1 | 2 | 3 | 4 | 5 | 6 |
|---|---|---|---|---|---|---|
| B, the ID sequence | 328 | 676 | 318 | 436 | 194 | 6 |
| I, random IDs | 351 | 846 | 439 | 276 | 141 | 0 |

Paired over the six seeds, random IDs change the count by +16 ± 49 (one
standard error); the twelve runs spread from 0 to 846, a standard
deviation of 249. Changing the IDs changes every relay's heartbeat
interval, and with it which nodes hear which relay first, so a run with
random IDs is a new draw of the few pockets the count comes from, not the
same run with something added. The full S4 of main, with drifting clocks
and walkers, gives 338, 1 004, 779 and 291 on seeds 1 to 4.

On D52's own code (`36a5fc7`, random IDs added on top of the switch-on
window), the same instrument gives what D52 reported on seeds 1 and 2,
through the same entries, and the opposite on three of the next four:

| Seed, D52's code | 1 | 2 | 3 | 4 | 5 | 6 |
|---|---|---|---|---|---|---|
| B, the ID sequence | 450 | 374 | 459 | 570 | 340 | 0 |
| I, random IDs | 839 | 1 263 | 740 | 137 | 20 | 12 |

+136 ± 200 over the six seeds. The two seeds D52 ran both happened to
rise. Between that code and main the anchor watch opens three drifts
early (D52's separate commit), which changes when sleeping nodes check
their anchors and so every run, as the IDs do; nothing in it reads an ID.

Nothing in the core orders or spaces nodes by ID beyond the heartbeat
interval, and the simulator keys its maps by the whole ID, so no ID
collides in the low byte to any effect: next hops tie-break by ID only
between neighbours at the same distance, and the heard and neighbour
tables search by the whole ID.

### What it costs, and a fix measured and not built

Each such frame is a text's DATA with the long preamble, about 425 ms on
air (1 396 symbols of preamble and 122 bytes): 140 s a run at the mean of
the twelve runs, 0.4 % of the domain's airtime, 360 s in the worst, 1.1 %.
It is spent by the anchorless nodes at the edge, on texts that were
leaving a pocket without a relay. These are the frames that reach a node
asleep; DATA with the long preamble altogether takes 1 474 and 2 503 s
in the full S4, seeds 1 and 2, 4.6 and 7.2 % of the airtime.

**X: forget an anchorless neighbour once failed hand-overs demote it** (two
in a row), on the reasoning that an anchorless neighbour that does not
answer twice most likely sleeps now. In the village beside a district of
D51 (sixty sleepy nodes and no relay, two relays switched on at its edge
at 1800 s), ten seeds: the frames fall from 389 to 131 a run and the
airtime by 16 % (−352 ± 91 s), but delivered falls 2.25 ± 0.74 points,
86.9 → 84.6 %: an anchorless neighbour that fails twice is often a live
one at the end of a faint link and the only way out, and once forgotten it
is heard again only when its beat, which visits a new channel each minute,
comes round to this one, about thirteen minutes. The village alone and the
D42 suites deliver as main (S2 −0.08 ± 0.12 points). In the full S4,
seeds 1 and 2, the frames fall from 671 to 94 a run on average, DATA with
the long preamble halves (−765 and −1 280 s) and all airtime falls by
782 ± 134 s, 2.4 %, while delivered falls 0.19 ± 0.22 points, within its
spread but on both seeds. Not built: it saves airtime at the edge and
loses texts there.

## 2. A beat after following the anchor to a new channel

### The rule

A sleeping node follows its anchor to a new home channel at the anchor's
next heartbeat it wakes for (spec "Relay choice"), and its holder learns
of it from its next regular beat, 150 to 300 s later; until then the holder
has no window to send in. D53 measured two ways to shorten that and built
neither: F, beat at once and again soon; G, beat once on the new channel at
a random time within a heartbeat interval, 0 to 15 s, since every node of
the anchor follows on the same heartbeat. G is five lines in
`reconsider_anchor`: when it follows the anchor to a new channel it brings
its next beat forward to a random time in the next 15 s if that is sooner.
F was not measured again: it gained the same as G in D53 for five times
the airtime, with all of a relay's nodes beating in the same instant.

### What was built

| Commit | Story |
|---|---|
| `5cbf2cf` | When a node follows its anchor to a new home channel it brings its next beat forward to a random time within a heartbeat interval, `heartbeat_max`, if that is sooner. The follow test in `tests/sleepy.rs` now also checks that the holder learns the new channel within that interval; it fails without the rule. |
| `e2a7d56` | The spec says so ("Channel moves", "Beats"). |

No wire change and no new constant.

### Results

S4 as it now is (switch-on over a window, random IDs, drifting clocks,
walkers), delivered of the reachable texts sent from the settle time to a
delivery TTL before the end, paired by seed:

| | Main | G | G − main |
|---|---|---|---|
| ×1 delivered, seeds 1–4 | 90.75 % | 90.71 % | −0.04 ± 0.06 points |
| ×1 latency p50, p90 | 138.5, 345.1 s | 137.3, 340.5 s | −1.2 ± 0.6, −4.7 ± 3.6 s |
| ×1 airtime of sleepy beats | 6 419 s | 6 472 s | +53 ± 6 s (+0.16 % of all airtime) |
| ×1 airtime, all | 33 139 s | 33 200 s | +61 ± 289 s |
| ×1 DATA to sleeping nodes, anchor / others | 14 / 603 | 17 / 647 | +2.5 ± 3.2 / +44 ± 104 |
| ×1 sleepy node's radio current | 0.397 mA | 0.398 mA | +0.001 mA |
| A tenth of the relays removed at 1800 s, delivered, seeds 1–4 | 84.43 % | 85.45 % | +1.01 ± 0.39 points, every seed |
| ... latency p50, p90 | 155.7, 410.3 s | 151.0, 396.9 s | −4.7 ± 1.5, −13.4 ± 5.2 s |
| ... mail of the removed relays' sleepy nodes | 72.7 % | 73.8 % | +1.1 ± 0.9 points |
| ... airtime of sleepy beats | | | +187 ± 11 s (+0.37 % of all airtime) |
| ... sleepy node's radio current | 0.433 mA | 0.434 mA | +0.001 mA |
| A tenth removed at 3600 s, delivered, seeds 1–2 | 84.79 % | 85.68 % | +0.88 ± 0.42 points |
| ... mail of the removed relays' sleepy nodes | 59.5 % | 60.6 % | +1.1 ± 0.3 points |
| ... those nodes heard by a new anchor, p50 | 510 s | 510 s | |
| ... airtime of sleepy beats | | | +142 ± 3 s (+0.30 %) |
| ... sleepy node's radio current | 0.462 mA | 0.465 mA | +0.003 mA |
| Simultaneous boot, ID sequence, true clocks, nobody walking (S4 before D52), seeds 1–2 | 91.52 % | 91.84 % | +0.32 ± 0.13 points |

The D42 suites, the standard ones (S1 and the two meshes 10 seeds, S1b,
S2 and the corridor 60, the ring 10) and the loaded ones (the corridor at
1 s and 0.5 s, the ring at 1 s and 2 s, the burst mesh at 64 times its
rate, 30 seeds each), deliver as main within their spread: identical in
S1, S1b, S2, the corridor and the burst mesh; the ring at 2 s +0.60 ±
0.43 points, at 1 s +1.07 ± 1.19, the burst mesh ×64 +0.26 ± 0.28, the
corridors at 1 s and 0.5 s +0.00.

### Why D53's gain is gone

G acts only when a relay moves its home by the busy estimate (spec
"Channel moves"). Switched on together, as before D52, the relays choose
their first channels in the same second from the same few heartbeats and
correct them by moving from 795 s on: 216 and 355 moves in seeds 1 and 2,
all of them in the measured hour from 900 s, each relay holding about 115
sleepy nodes. Switched on over ten minutes, each relay chooses among
relays already running, and the domain moves less: 90 and 105 moves in
the whole run, 75 and 85 in the measured hour from 2700 s, about one a
minute across 269 relays, with 123 sleepy nodes each. With a third of the
moves G has a third of the work, and what it gains is within the spread.
Removing a tenth of the relays changes what their neighbours hear, and
ten minutes later they move: 353 moves after the settle time with the
removal at 1800 s, 130 of them in the ten minutes from 2400 s, and 264
with it at 3600 s, 126 of them from 4200 s (seed 1), against 75 without.
There G gains a point.

## What was run

- S4 with an extra line per DATA to a sleeping node from a node that is
  not its anchor, models B and I of D52 (switch-on over a window, true
  clocks, nobody walking, the ID sequence or random IDs) on main, seeds 1
  to 6; the same on D52's random-ID commit `36a5fc7`, seeds 1 to 6; the
  full S4 on main, seed 1, both ways; I on main, seed 2, with a line each
  time a sleepy node enters or leaves anchorless mode.
- X on the village and the village beside a district of D51, 10 seeds
  each, the D42 suites and S4 ×1 seeds 1 and 2.
- G against main: S4 ×1, seeds 1 to 4; with a tenth of the relays (every
  tenth) removed at 1800 s, seeds 1 to 4, and at 3600 s, seeds 1 and 2;
  S4 as it was before D52, seeds 1 and 2, with a line per relay move to
  count them, as in S4 now; the D42 suites.

No more than two S4 runs at once. Scripts and runs are in the scratchpad
(`d57/`): `runall.py`, `an.py`, `toasleep.py`, `bi.py`, `refresh.py`,
`beats.py`, `moves.py`, the edits `ed_g.py`, `ed_x.py` and the
instrument's patches.

## What it does not show

- Which nodes end in pockets without a relay is set by placement and by
  who hears whom at boot; six seeds a configuration show the count's
  spread, not its mean to better than a fifth.
- The removals happen before the settle time when at 1800 s, since S4 now
  settles at 2700 s; the time for the removed relays' sleepy nodes to be
  heard again is sampled from the settle time only and reads 900 s for
  every one of them. Their mail is counted from the removal.
- G was measured against relay moves at the rate S4 makes them; a channel
  that becomes busy at once, from an interferer, moves every relay that
  hears it together, and there G would act on every sleepy node at once.

## Recommendation

Item 1: nothing to change in the simulator or the protocol for it. The
DATA to sleeping nodes from others is the stale anchorless entry that D51
chose to pay for, about 0.4 % of airtime, and forgetting the entry after
failures loses texts at the edge. D52's "doubling" with random IDs is the
spread of a count that a few edge pockets make. What X showed on the way
deserves its own look: DATA with the long preamble takes 5 to 7 % of S4's
airtime, half of it to anchorless neighbours that failed twice in a row,
spent by the few nodes at the edge that can least afford it.

Item 2: adopt G. It acts only when relays move, costs 0.16 % of airtime
where they move little and changes nothing measurable there, and gains a
point of delivery where they move a lot, after relays are lost; nowhere
did it lose. The gain D53 measured in steady running was the simultaneous
boot's wave of channel moves.

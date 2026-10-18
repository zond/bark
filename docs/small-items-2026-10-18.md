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

## What was run

- S4 with an extra line per DATA to a sleeping node from a node that is
  not its anchor, models B and I of D52 (switch-on over a window, true
  clocks, nobody walking, the ID sequence or random IDs) on main, seeds 1
  to 6; the same on D52's random-ID commit `36a5fc7`, seeds 1 to 6; the
  full S4 on main, seed 1, both ways; I on main, seed 2, with a line each
  time a sleepy node enters or leaves anchorless mode.
- X on the village and the village beside a district of D51, 10 seeds
  each, the D42 suites and S4 ×1 seeds 1 and 2.

No more than two S4 runs at once. Scripts and runs are in the scratchpad
(`d57/`): `runall.py`, `an.py`, `toasleep.py`, `bi.py`, `refresh.py`,
the edit `ed_x.py` and the instrument's patches.

## What it does not show

- Which nodes end in pockets without a relay is set by placement and by
  who hears whom at boot; six seeds a configuration show the count's
  spread, not its mean to better than a fifth.

## Recommendation

Item 1: nothing to change in the simulator or the protocol for it. The
DATA to sleeping nodes from others is the stale anchorless entry that D51
chose to pay for, about 0.4 % of airtime, and forgetting the entry after
failures loses texts at the edge. D52's "doubling" with random IDs is the
spread of a count that a few edge pockets make. What X showed on the way
deserves its own look: DATA with the long preamble takes 5 to 7 % of S4's
airtime, half of it to anchorless neighbours that failed twice in a row,
spent by the few nodes at the edge that can least afford it.

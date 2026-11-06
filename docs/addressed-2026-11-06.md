# Handing a message to the node it is addressed to

**Status:** research note, 2026-11-06. Adds the first rule of the order in
spec "Next hop" (roadmap D116a), alone and with the triangle rule of
`triangle-2026-11-05.md`. Spec references are to `protocol.md` by section
name.

## Summary

The texts that end at the hop cap are mostly to carried nodes
(`triangle-2026-11-05.md`, "The finding"): the address is where the node
was, and the search walks the city until the cap ends it or it meets the
node by chance. Very often a relay on the way has the node itself in its
neighbour table, and does not use it: the next hop was chosen by
distance to the position alone.

With the addressed node first in the order wherever it is a candidate,
stale or not, the city loses a quarter to a tenth as many texts at the
cap. Of the
texts accepted, 98.27, 98.57 and 97.50 % arrive at ×1, ×3 and ×4,
against 97.55, 97.28 and 95.34 % on main; of those to carried nodes,
97.7 to 98.8 % against 89.5 to 93.5 %. The lost texts' share of their
window's air falls from 10 to 16 % to 1.6 to 2.7 %, and latency falls,
most at ×4 (p50 36.5 → 22.0 s).

It recovers all the triangle rule cost carried nodes and more. With both,
the delivered share is that of the ID rule alone within 0.3 points, and
a little less air goes to lost texts. Only a stale walker in the order
makes the difference: the same rule for fresh entries only gains little.

## The rule

As spec "Next hop" now states it, the order of the candidates starts
with the addressed node itself, before margin, demotion and distance:

- **Who.** The live neighbour-table entry whose ID is the message's
  `dst_id`, if it is a candidate: not in the tried set, not the parent.
  The neighbour table is only read; nothing is looked up, and the heard
  table is not consulted.
- **Last resorts.** A carried relay not heard for its heartbeat
  interval, a last resort for other messages, is first all the same. A
  sleepy node with an anchor stays a last resort: its anchor owns its
  address and delivers to it in its window ("The anchored sleepy
  destination" below). Where no other candidate is left, it is first
  among the last resorts, sent to with the long preamble.
- **Answered seen.** A node that has had the message answers seen as any
  other (spec "The ACK"), and the search goes on.

`Relay::choose` ranks `n.id == dst_id` first and lets it past the last
resort tier unless it is sleepy with an anchor. The record and `Relay`
are unchanged.

## How it was measured

As in `triangle-2026-11-05.md`, "How it was measured": the same seeds,
scenario files and windows, release builds. Builds:

| build | what |
|---|---|
| main | `e2e2761` |
| ID | main with the rule |
| ID, fresh only | main with the rule for entries that are no last resort; a last resort addressed stays one |
| ID, no anchored sleepy | main with the rule, a stale carried relay first, a sleepy node with an anchor still a last resort |
| triangle | main with the triangle rule (`ba527e4`) |
| triangle + ID | the branch as committed |
| variant, variant + ID | the triangle note's variant, without and with the rule |

The air of lost texts is given two ways: as a share of the air of the
texts written in the window, and as a share of all air in the run, as
in the triangle note. The second also holds the texts written before
the window, while the city starts; at ×1 most of the lost air with the
rule is there.

## Results

Seed ranges in brackets where there are several seeds.

| run | build | delivered of accepted % | lost at the hop cap / by the triangle, of accepted % | air of lost texts, of the window's texts' air / of all air in the run % | to carried nodes: delivered %, lost at the cap / triangle %, median / p90 hops | latency p50 / p90 / p99, s |
|---|---|---|---|---|---|---|
| city ×1 | main | 97.55 | 0.55 / 0.00 | 10.3 / 14.7 | 93.54, 3.21 / 0.00, 9 / 17 | 5.1 / 62 / 528 |
| city ×1 | ID | 98.27 | 0.13 / 0.00 | 2.7 / 12.1 | 98.66, 0.09 / 0.00, 8 / 13 | 4.2 / 25 / 499 |
| city ×1 | ID, fresh only | 97.69 | 0.41 / 0.00 | 7.8 / 14.0 | 94.92, 2.09 / 0.00, 9 / 17 | 5.7 / 57 / 490 |
| city ×1 | ID, no anchored sleepy | 98.29 | 0.18 / 0.00 | 4.2 / 12.7 | 98.71, 0.09 / 0.00, 7 / 14 | 3.9 / 24 / 509 |
| city ×1 | triangle | 97.28 | 0.10 / 0.74 | 4.5 / 8.7 | 91.76, 0.58 / 4.41, 9 / 16 | 4.9 / 59 / 505 |
| city ×1 | triangle + ID | 98.31 | 0.02 / 0.17 | 1.4 / 8.9 | 98.62, 0.04 / 0.13, 8 / 14 | 4.4 / 25 / 512 |
| city ×1 | variant | 97.39 | 0.15 / 0.59 | 6.0 / 10.3 | 92.29, 1.02 / 3.25, 9 / 17 | 4.9 / 59 / 497 |
| city ×1 | variant + ID | 98.21 | 0.01 / 0.20 | 1.1 / 7.9 | 98.35, 0.00 / 0.13, 8 / 13 | 4.1 / 25 / 500 |
| city ×3 | main | 97.28 | 1.00 / 0.00 | 16.1 / 14.0 | 92.10, 4.77 / 0.00, 10 / 21 | 9.9 / 92 / 569 |
| city ×3 | ID | 98.57 | 0.09 / 0.00 | 2.0 / 7.5 | 98.75, 0.08 / 0.00, 8 / 15 | 8.1 / 37 / 539 |
| city ×3 | triangle | 96.95 | 0.32 / 1.01 | 10.3 / 9.7 | 90.61, 1.56 / 4.94, 10 / 19 | 9.6 / 91 / 561 |
| city ×3 | triangle + ID | 98.53 | 0.01 / 0.08 | 0.9 / 5.5 | 98.73, 0.02 / 0.07, 8 / 15 | 8.3 / 37 / 546 |
| city ×3 | variant | 97.14 | 0.42 / 0.75 | 11.8 / 10.6 | 91.41, 2.06 / 3.61, 10 / 19 | 9.6 / 90 / 567 |
| city ×3 | variant + ID | 98.55 | 0.02 / 0.09 | 1.0 / 5.8 | 98.73, 0.00 / 0.10, 8 / 15 | 8.0 / 35 / 568 |
| city ×4 | main | 95.34 | 0.92 / 0.00 | 10.8 / 11.4 | 89.52, 4.28 / 0.00, 13 / 27 | 36.5 / 193 / 710 |
| city ×4 | ID | 97.50 | 0.09 / 0.00 | 1.6 / 6.4 | 97.73, 0.06 / 0.00, 10 / 19 | 22.0 / 128 / 644 |
| city ×4 | triangle | 95.27 | 0.33 / 1.04 | 8.1 / 8.3 | 87.88, 1.56 / 4.95, 13 / 24 | 31.2 / 171 / 692 |
| city ×4 | triangle + ID | 97.22 | 0.03 / 0.08 | 0.9 / 4.3 | 97.31, 0.02 / 0.08, 10 / 19 | 21.5 / 129 / 640 |
| city ×4 | variant | 95.35 | 0.44 / 0.74 | 9.0 / 9.1 | 88.79, 2.13 / 3.43, 13 / 25 | 30.9 / 177 / 733 |
| city ×4 | variant + ID | 97.43 | 0.02 / 0.06 | 0.8 / 4.6 | 97.59, 0.03 / 0.08, 10 / 19 | 20.7 / 129 / 644 |
| district ×32, seeds 1 to 3 | main | 82.12 (79.53–83.72) | 0.16 / 0.00 | 3.6 / 5.1 | 62.22, 1.35 / 0.00, 8 / 15 | 38.1 / 231 / 550 |
| district ×32, seeds 1 to 3 | ID | 84.28 (82.32–86.12) | 0.04 / 0.00 | 1.0 / 1.5 | 82.69, 0.06 / 0.00, 6 / 11 | 35.6 / 213 / 541 |
| district ×32, seeds 1 to 3 | ID, fresh only | 81.07 (77.91–82.76) | 0.12 / 0.00 | 3.1 / 4.7 | 65.85, 0.54 / 0.00, 8 / 15 | 38.5 / 233 / 558 |
| district ×32, seeds 1 to 3 | ID, no anchored sleepy | 82.73 (79.97–84.12) | 0.10 / 0.00 | 2.7 / 4.2 | 80.81, 0.00 / 0.00, 6 / 11 | 37.5 / 226 / 537 |
| district ×32, seeds 1 to 3 | triangle | 82.88 (80.86–83.96) | 0.06 / 0.55 | 3.7 / 3.8 | 62.53, 0.29 / 3.98, 8 / 13 | 32.9 / 209 / 520 |
| district ×32, seeds 1 to 3 | triangle + ID | 84.13 (81.70–86.47) | 0.01 / 0.15 | 1.0 / 1.1 | 84.66, 0.00 / 0.12, 6 / 11 | 33.4 / 214 / 535 |
| district ×32, seeds 1 to 3 | variant | 83.68 (79.99–86.59) | 0.06 / 0.42 | 3.5 / 4.8 | 65.09, 0.78 / 2.63, 8 / 14 | 37.2 / 222 / 533 |
| district ×32, seeds 1 to 3 | variant + ID | 84.26 (82.43–85.50) | 0.01 / 0.15 | 1.4 / 1.3 | 81.46, 0.00 / 0.12, 6 / 11 | 35.3 / 215 / 543 |
| S4 | main | 94.48 (93.71–95.35) | 0.12 / 0.00 | 4.8 / 3.4 | – | 1.4 / 193 / 991 |
| S4 | ID | 94.53 (93.78–95.35) | 0.12 / 0.00 | 3.9 / 3.1 | – | 1.4 / 192 / 998 |
| S4 | ID, fresh only | 94.44 (93.64–95.37) | 0.12 / 0.00 | 3.6 / 3.0 | – | 1.4 / 191 / 994 |
| S4 | ID, no anchored sleepy | 94.44 (93.64–95.37) | 0.12 / 0.00 | 3.6 / 3.0 | – | 1.4 / 191 / 994 |
| S4 | triangle | 94.50 (93.62–95.39) | 0.12 / 0.01 | 4.0 / 2.8 | – | 1.4 / 192 / 1006 |
| S4 | triangle + ID | 94.42 (93.80–95.30) | 0.10 / 0.01 | 3.3 / 2.5 | – | 1.4 / 191 / 990 |
| S4 | variant | 94.46 (93.62–95.39) | 0.12 / 0.01 | 4.2 / 2.9 | – | 1.4 / 193 / 1001 |
| S4 | variant + ID | 94.38 (93.76–95.30) | 0.11 / 0.01 | 3.6 / 2.7 | – | 1.4 / 192 / 990 |
| mesh burst | main | 99.00 (94.00–100.00) | 0.00 / 0.00 | 13.6 / 4.8 | – | 2.6 / 6 / 12 |
| mesh burst | ID | 99.60 (96.00–100.00) | 0.00 / 0.00 | 2.9 / 1.3 | – | 3.1 / 14 / 37 |
| mesh burst | ID, fresh only | 99.00 (94.00–100.00) | 0.00 / 0.00 | 13.6 / 4.8 | – | 2.6 / 6 / 12 |
| mesh burst | ID, no anchored sleepy | 99.00 (94.00–100.00) | 0.00 / 0.00 | 13.6 / 4.8 | – | 2.6 / 6 / 12 |
| mesh burst | triangle | 99.00 (94.00–100.00) | 0.00 / 0.00 | 13.6 / 4.8 | – | 2.6 / 6 / 12 |
| mesh burst | triangle + ID | 99.60 (96.00–100.00) | 0.00 / 0.00 | 2.9 / 1.3 | – | 3.1 / 14 / 37 |
| mesh burst | variant | 99.00 (94.00–100.00) | 0.00 / 0.00 | 13.6 / 4.8 | – | 2.6 / 6 / 12 |
| mesh burst | variant + ID | 99.60 (96.00–100.00) | 0.00 / 0.00 | 2.9 / 1.3 | – | 3.1 / 14 / 37 |

Ring, corridor and spread mesh are identical in every build. S4 is
within its seed spread in every build.

The district's spread between seeds is wider than any difference here,
so main, the rule alone and triangle + ID were also run on seeds 4 to 6.
Seed by seed, 1 to 6, main → ID: 82.98 → 86.12, 83.72 → 84.32, 79.53 →
82.32, 82.61 → 83.51, 76.63 → 77.86, 84.86 → 85.55 %: higher in each, by
0.6 to 3.1 points; triangle + ID gives 86.47, 84.09, 81.70, 84.38, 78.54,
85.97 %. The means are 81.72, 83.28 and 83.53 %.

## What it does on main alone

Of the texts to carried nodes, by city rate, main → ID:

| run | delivered % | lost at the cap % | hops, median / p90 |
|---|---|---|---|
| city ×1 | 93.54 → 98.66 | 3.21 → 0.09 | 9 / 17 → 8 / 13 |
| city ×3 | 92.10 → 98.75 | 4.77 → 0.08 | 10 / 21 → 8 / 15 |
| city ×4 | 89.52 → 97.73 | 4.28 → 0.06 | 13 / 27 → 10 / 19 |
| district ×32 | 62.22 → 82.69 | 1.35 → 0.06 | 8 / 15 → 6 / 11 |

A walker is mostly still within reach of the relays around the place it
last stood, and the search reaches those first, since it goes toward
that position; one of them has it in its table. This is inferred from
the outcome: the runs do not record which relay handed the message to
the node. The texts it saves are those that took hundreds of hand-overs or
the cap on main, so the hops of texts to carried nodes fall, most at the
tail. The texts to sleepy and fixed nodes are unchanged at ×1 and ×3, and
gain 0.6 to 0.8 points at ×4 from the air no longer spent.

## Why a stale walker must be first

The same rule for fresh entries only, a stale walker staying a last
resort, delivers 94.92 % of the texts to carried nodes at ×1 against
98.66 % with it, and 65.85 against 82.69 % in the district. A walker is
stale in the table of the relays it has walked past, which are where the
search arrives: its heartbeat comes back to their channel once a rotation
(spec "Next hop", on why a carried relay goes stale). Three unanswered
DATA frames to it cost well under a second of air, a hand-over taking
about 0.2 s in these runs (2.0 to 2.4 s per delivered text over its 8 to
10 hops); on main the search it would have ended cost 41 to 51 s.

## With the triangle rule

The triangle rule's cost was the texts to carried nodes that main's long
walk meets by chance (`triangle-2026-11-05.md`, "The island cost"). With
the ID rule those texts are handed to the node before the walk, and the
triangle ends only what has no node near its address:

| run | delivered %: ID, triangle + ID | lost by the triangle % | air of lost texts, window / run %: ID, triangle + ID |
|---|---|---|---|
| city ×1 | 98.27, 98.31 | 0.17 | 2.7 / 12.1, 1.4 / 8.9 |
| city ×3 | 98.57, 98.53 | 0.08 | 2.0 / 7.5, 0.9 / 5.5 |
| city ×4 | 97.50, 97.22 | 0.08 | 1.6 / 6.4, 0.9 / 4.3 |
| district ×32 | 84.28, 84.13 | 0.15 | 1.0 / 1.5, 1.0 / 1.1 |

So the ID rule recovers the carried-node loss the triangle caused and
more: in the city, texts to carried nodes arrive 97.3 to 98.7 % of the
time with both, against 87.9 to 91.8 % with the triangle alone. In the
city the triangle on top saves about half of the little air still lost
in the window. At ×4 it
delivers 0.28 points less, about the run-to-run spread at ×1.

The triangle note's variant with the ID rule (variant + ID) delivers
98.21, 98.55 and 97.43 % in the city and 84.26 % in the district, within
0.2 points of triangle + ID, with about as much air lost: once the
addressed node goes first, the variant's extra test seldom differs.

## The anchored sleepy destination

In mesh burst, where every leaf is a sleepy node with an anchor 15 m
away, the ID rule changes the run: a relay that hears a leaf hands the
message to it with the long preamble, rather than to its anchor, which
owns its position and would deliver it from its queue. Delivered rises
99.00 → 99.60 %, but the air per delivered text rises 0.89 → 1.13 s and
latency p90 6 → 14 s, p99 12 → 37 s. Keeping a sleepy node with an anchor
a last resort for its own messages (ID, no anchored sleepy) leaves mesh
burst as on main, gives the same city at ×1 (98.29 %, carried 98.71 %),
and in the district 82.73 % against 84.28 %, inside the seed spread,
with the carried nodes' share as with the rule (80.81 against 82.69 %).

## What it shows

- The addressed node, in the neighbour table of a relay the search
  reaches, ends the search for a carried node that would otherwise run
  to the hop cap; texts lost at the cap fall to a quarter at ×1 and a
  tenth at ×3 and ×4.
- It must be first while stale: the fresh-only form gains a fifth to a
  quarter of it.
- It delivers more texts at every city rate, cuts the air of lost texts
  to a quarter to an eighth of what it was in the window, and lowers
  latency.
- With it the triangle rule costs about nothing in delivery and saves a
  little more air.
- Sent to a sleepy node with an anchor, it costs air and latency where a
  relay hears another relay's leaf.

## What it does not show

- One seed per city rate, as in the triangle note.
- The district at ×32 is overloaded, a sixth of its texts in flight at
  the end, and its builds differ mostly in what is in flight.
- How often a real walker stays within reach of the relays near where it
  last stood is the simulator's walk model.

## Verdict

Left to the user. The ID rule is the larger gain and has no cost in the
city; it is worth having with or without the triangle rule. The choices
left are whether to add the triangle rule on top, for a little less air
at no clear cost in delivery, and whether a sleepy node with an anchor
should stay a last resort for its own messages, which keeps mesh burst's
air and latency as on main.

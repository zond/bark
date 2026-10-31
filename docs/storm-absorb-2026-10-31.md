# A copy from elsewhere is absorbed

**Date:** 2026-10-31. **Status:** measured; roadmap D89, the user
decides. Spec references are to `protocol.md`. Part 1 of the four-part
candidate of `docs/storm-2026-10-30.md`, built alone and measured against
main `bdfd090`, whose outputs are those of `e75bd97` to the byte; all
release builds, the same scenario files, the branch's binary from the
commit that carries the rule.

## Summary

- **The rule.** A relay that has handled a message and holds no copy of
  it, when another copy comes from a node it did not hand the message to,
  ACKs the copy and drops it. The sender's hand-over succeeded, so the
  sender is done with the message; the relay does not take the copy into
  its send queue and does not search again. A return, from a node that
  has the message from this relay, goes on with the relay's own search as
  before.
- **It ends the storm.** `dense-power.toml` seed 1, settled hour: 90.3 →
  97.9 % delivered, the run's airtime 1 134 454 → 202 693 s, latency p90
  288 → 42 s. Seed 7, which did not storm: 98.2 → 98.1 %, airtime
  276 300 → 200 781 s. S4 at three times the rate, seeds 1 / 2 / 3: 59.7 /
  56.8 / 63.8 → 72.8 / 71.8 / 83.6 %, airtime 15 to 19 % less. At the
  design rate 93.4 → 93.5 %; the ring 0.924 → 0.921; the walkers 96.5 →
  95.4 %.
- **It ends single searches too.** A message with one copy that comes
  back to a relay it has crossed, by a link that is not a return, ends
  there: the sender was done when the ACK came, and the relay does not
  go on. On the random mesh of 25 relays, `mesh-burst.toml` seed 1, 100 →
  86 % of a burst delivered, every loss an absorbed copy, and the
  scenario test `mesh_routes_bursts_and_a_restart` fails its bound of
  0.9. The spec's argument that the search visits every reachable relay
  ("Why the search finds a path") no longer holds; the routing tests that
  described it are rewritten to what the rule does.
- **Recommendation: not as stated.** What ends the storm is that a copy
  cannot start the search again at a relay the search has been; what
  loses the mesh's texts is that the sender is done when its copy is
  absorbed. A form in which the absorbing relay's answer leaves the
  sender searching, the relay marked tried, would keep the first without
  the second; it is a change to the ACK and to the sender's side, outside
  part 1, and is the thing to build and measure next.

## The rule

Spec "Receiving DATA" tests a DATA packet of a message the relay has a
record of, holds no copy of, that is not a retransmission and has not
arrived, in two cases:

- **A return** (case 5): the sender is in the record's `handed` set, the
  neighbours that took the message from this relay by ACKing a hand-over
  of it, or it is the neighbour the relay is still handing the message
  to. The relay takes it and goes on with its own search from its
  record, as before.
- **A copy from elsewhere** (case 6): any other sender. If the relay owns
  the destination now, it delivers, as in case 5. Otherwise it owes the
  ACK that takes the packet, from the reply reserve like the other
  no-work cases, and drops the copy.

Three readings were needed where the rule as written in the storm note
met the code, each the narrowest that keeps its meaning:

- **The parent that took a return is in `handed`.** The spec defined the
  set as every neighbour that ACKed a hand-over, which a return is; the
  code set the bit for forwards only, since the parent was excluded from
  the candidates by its ID anyway. Set on a return too, a parent's
  re-offer after a split, when its own hand-over to this relay had every
  ACK lost and it waited out the ban, is a return and goes on, as the
  test `a_split_heals_at_a_sender_that_waits` requires; without the bit
  the relay absorbed it and the test failed. A parent not in the
  neighbour table when the return went, sent to on the record's channel,
  gets no bit.
- **The relay that owns the destination now delivers.** Case 5 checked
  ownership again because a closer neighbour may have been demoted since
  the record was made; a copy from elsewhere for a position this relay
  owns now is delivered the same way, not absorbed, so that the rule
  does not end a message at the very relay that holds for its node.
- **The neighbour being handed to has the message.** A copy from the
  neighbour whose hand-over is under way, frame on air and ACK not yet
  come, ended that hand-over before and still does; it is treated as a
  return, since that neighbour has the message from this relay.

What the rule does not tell apart, since the record does not: a child
whose every ACK was lost is not in `handed`, so its return is absorbed
(the tests of roadmap D35 A and C); a child whose neighbour-table slot
changed hands has its bit cleared, so its return is absorbed too (D35
B). Both were searches that went on before.

The simulator counts absorbed copies (`absorbed`, beside `copies` and
`merged` in each node's core counters) and reports them as drops with
their own reason, `Absorbed`, so that a text whose last copy was
absorbed is accounted for.

## Why

From `docs/storm-2026-10-30.md`: a hand-over that fails after three
unanswered transmissions, with the receiver having taken the message,
leaves two searchers. Each copy that reached a relay that had handled
the message reopened the search there, case 5 as it was, so every split
started the depth-first search again from every relay it reached, and
each search split in its turn; in the storm's settled hour 3 060 986
copies reopened a search against 442 031 merged. Absorbed, a copy can go
on only to relays that have not seen the message, so a text has at most
as many holders as relays its first copy has not reached, and the chain
ends where the search has been.

## What it costs, and why

The depth-first search of spec "Routing" has one token: a relay that
hands the message over is done with it until the receiver returns it.
The token comes back to a relay it has crossed in two ways: as a return
from a child, or by a link from a relay deeper in the search that is not
the child's. Before this rule the second was case 5: the relay took the
token and went on from where its own search stood, and the sender, done
with it, stayed one of its candidates and got the token back in turn. So
the traversal visited every relay it could reach. Under the rule the
second way ends the token: the sender is done, the relay does not go on,
and the relays above them never get the message back.

On a dense grid, where the greedy next hop goes toward the destination
and is stopped mostly by bans, this is rare: at the design rate 595
copies were absorbed in 24 659 texts and 66 texts were lost to it, while
28 fewer were lost at the hop cap, the time to live and for want of a
route, and the settled hour delivered four texts more. On a sparse
random mesh, where a search often has to go round and comes back on
itself, it is common: 17 copies absorbed in 100 texts and 14 lost.

## How it was measured

Every run is the scenario file at the seed named, with `--json` and
`--messages`; the figures are the JSON summary's: delivered of the texts
sent in the settled hour and of all texts, latency, the run's airtime,
the cores' counters summed over the nodes, drops and texts lost by
reason, the relays whose load sample ever found the bucket under its
reserve, and the largest settled airtime of a node. Main's figures are
the reference outputs of `e75bd97` for seed 1 of each scenario and the
runs of main at the board's costs for S4 seeds 2 and 3; the mesh runs
are main's binary and the branch's on the same day. The three-times-rate
S4 is `s4-capacity.toml` with `per_leaf_per_hour` 2.875.

The two random-mesh runs were added when the scenario test failed on
the branch; they are not in the storm note's list.

## The table

Main → the branch. "Copies taken" are those from no neighbour the node
had handed the message to, cases 4 to 6; "returns" those from one it
had. A text is "lost" to a reason when its last copy ended with it.

| | dense-power seed 1 | dense-power seed 7 | S4 ×1 seed 1 | S4 ×3 seed 1 | S4 ×3 seed 2 | S4 ×3 seed 3 | ring seed 1 | dense-movers seed 1 |
|---|---|---|---|---|---|---|---|---|
| texts sent in the settled hour | 18 202 → 18 234 | 18 190 → 18 190 | 17 459 → 17 453 | 48 526 → 50 700 | 48 110 → 50 727 | 49 340 → 52 206 | 2 918 → 2 867 | 17 894 → 17 894 |
| delivered, settled hour | 90.3 → 97.9 % | 98.2 → 98.1 % | 93.4 → 93.5 % | 59.7 → 72.8 % | 56.8 → 71.8 % | 63.8 → 83.6 % | 92.4 → 92.1 % | 96.5 → 95.4 % |
| delivered, whole run | 91.2 → 95.9 % | 96.9 → 96.4 % | 93.4 → 93.6 % | 65.1 → 76.0 % | 62.6 → 75.6 % | 68.3 → 84.8 % | 92.6 → 92.5 % | 95.7 → 94.1 % |
| latency p50 | 20.2 → 9.6 s | 10.3 → 9.6 s | 1.6 → 1.6 s | 142.6 → 61.4 s | 160.3 → 67.4 s | 151.2 → 42.7 s | 60.7 → 53.3 s | 5.5 → 5.3 s |
| latency p90 | 287.7 → 41.8 s | 44.9 → 41.0 s | 191.5 → 191.4 s | 654.0 → 509.7 s | 685.2 → 516.4 s | 645.0 → 450.4 s | 152.7 → 157.9 s | 48.6 → 34.1 s |
| airtime, whole run | 1 134 454 → 202 693 s | 276 300 → 200 781 s | 49 656 → 49 173 s | 169 431 → 144 371 s | 173 508 → 143 620 s | 181 241 → 146 908 s | 1 808 → 1 802 s | 175 315 → 117 234 s |
| hand-overs taken | 3 261 455 → 304 274 | 638 893 → 302 998 | 124 609 → 114 940 | 438 792 → 397 269 | 449 168 → 404 547 | 464 236 → 414 490 | | 504 200 → 230 515 |
| hand-overs failed | 2 620 398 → 283 020 | 440 818 → 279 315 | 20 124 → 18 460 | 170 984 → 127 843 | 173 051 → 123 715 | 190 398 → 126 680 | | 236 598 → 119 896 |
| failed, the receiver had it | 767 407 → 30 766 | 98 492 → 30 840 | 4 417 → 3 320 | 43 477 → 31 854 | 43 887 → 31 020 | 55 812 → 40 081 | | 119 686 → 24 357 |
| returns taken from a child | 382 239 → 40 | 75 294 → 56 | 2 781 → 53 | 12 295 → 813 | 17 635 → 767 | 15 369 → 795 | | 67 757 → 82 |
| copies taken | 1 440 358 → 10 292 | 185 314 → 10 214 | 4 860 → 617 | 34 591 → 9 732 | 46 979 → 10 872 | 43 011 → 9 379 | | 153 295 → 5 275 |
| of those merged (case 4) | 175 211 → 275 | 9 808 → 277 | 165 → 22 | 2 963 → 970 | 3 426 → 946 | 4 053 → 976 | | 7 778 → 108 |
| of those absorbed (case 6) | 0 → 10 010 | 0 → 9 934 | 0 → 595 | 0 → 8 757 | 0 → 9 922 | 0 → 8 399 | 0 → 2 | 0 → 5 165 |
| hand-overs held back by the bucket | 1 760 186 → 4 460 | 9 619 → 5 147 | 147 → 65 | 864 004 → 440 900 | 907 780 → 448 484 | 861 766 → 252 856 | | 3 872 → 68 |
| neighbours demoted | 1 042 233 → 127 180 | 188 196 → 125 433 | 7 340 → 6 852 | 49 368 → 36 312 | 49 559 → 34 947 | 55 477 → 36 024 | | 97 737 → 54 354 |
| copies dropped at the hop cap / at the TTL | 22 474 / 73 956 → 0 / 19 | 4 059 / 531 → 0 / 8 | 53 / 368 → 0 / 342 | 151 / 6 004 → 0 / 3 710 | 134 / 6 512 → 0 / 3 167 | 155 / 6 022 → 0 / 2 415 | 0 / 0 → 0 / 0 | 4 355 / 424 → 0 / 38 |
| texts lost at the hop cap / at the TTL / absorbed | 254 / 683 / 0 → 0 / 19 / 624 | 236 / 78 / 0 → 0 / 7 / 577 | 15 / 338 / 0 → 0 / 326 / 66 | 7 / 5 602 / 0 → 0 / 3 601 / 3 711 | 17 / 5 883 / 0 → 0 / 3 061 / 4 761 | 14 / 5 357 / 0 → 0 / 2 294 / 2 324 | 0 / 0 / 0 → 0 / 0 / 1 | 178 / 79 / 0 → 0 / 31 / 772 |
| texts lost for want of a route / at a failed return | 3 / 13 → 4 / 6 | 5 / 5 → 4 / 2 | 1 / 0 → 0 / 0 | 8 / 1 → 13 / 0 | 6 / 0 → 8 / 0 | 11 / 0 → 12 / 0 | 28 / 1 → 25 / 1 | 2 / 1 → 0 / 0 |
| texts that waited at their origin | 2 362 → 51 | 49 → 37 | 194 → 198 | 212 730 → 109 295 | 225 537 → 118 098 | 209 319 → 59 114 | 2 407 → 2 375 | 90 → 56 |
| relays ever at their cap | 1 616 → 7 | 44 → 7 | 0 → 0 | 179 → 134 | 176 → 130 | 204 → 154 | | 62 → 7 |
| a node's largest settled airtime | 8.07 → 4.70 % | 5.35 → 4.33 % | 6.21 → 5.91 % | 9.91 → 9.91 % | 9.97 → 9.96 % | 9.99 → 9.97 % | 9.88 → 10.14 % | 5.46 → 4.77 % |
| heartbeats received | 71.1 → 86.4 % | 85.3 → 86.6 % | 94.3 → 94.3 % | 88.2 → 89.2 % | 88.2 → 89.2 % | 87.0 → 88.5 % | 87.8 → 88.8 % | 89.3 → 90.0 % |

The ring's summary carries no per-node counters; its two absorbed
copies are from its drops.

The random mesh, 25 relays and 25 leaves, every leaf sending across it
once, all at once (`mesh-burst.toml`) or each at its own time with the
middle relay restarting (`mesh-spread.toml`); at seed 5 a relay is out
of everyone's reach and its texts count as unreachable on both builds:

| | burst seed 1 | burst seed 5 | spread seed 1 | spread seed 5 |
|---|---|---|---|---|
| texts | 100 | 100, 16 unreachable | 90 | 90, 14 unreachable |
| delivered of the reachable | 100 → 86 % | 100 → 100 % | 100 → 95.6 % | 100 → 100 % |
| copies absorbed / texts lost to it | 0 / 0 → 17 / 14 | 0 / 0 → 9 / 8 | 0 / 0 → 4 / 4 | 0 / 0 → 8 / 7 |
| latency p50 / p90 | 3.9 / 8.4 → 3.5 / 6.0 s | 2.5 / 5.9 → 2.2 / 4.7 s | 0.7 / 1.0 → 0.7 / 0.9 s | 0.7 / 3.0 → 0.7 / 0.9 s |
| airtime | 240 → 208 s | 263 → 201 s | 194 → 181 s | 194 → 179 s |

The texts lost at seed 5 are the unreachable ones, whose searches end by
an absorbed copy instead of running out at the origin.

## What it shows

- **The storm is the chain of reopened searches, and absorbing ends it.**
  Seed 1 of `dense-power.toml` at the board's costs, which stormed on
  every build since the costs were measured (D86, D88), delivers 97.9 %
  with 202 693 s of airtime, less than the 295 000 s of the old costs'
  run without a storm, and the seven relays ever at their cap against
  1 616. Seed 7's airtime falls by a quarter with delivery unchanged:
  the copies were there too, below the edge.
- **Under load it is worth 13 to 20 points.** S4 at three times the rate
  rises on every seed, with a third fewer texts lost at the time to live,
  a sixth less airtime, half the hand-overs held back by the bucket and
  half the texts waiting at their origin. The relays still sit at their
  cap, 9.9 %: the load is the offered rate's, not the copies'.
- **At the design rate it is neutral on the grid and costs a point on
  the walkers.** S4 ×1 is unchanged to the tenth of a point; the walkers
  lose 1.1 points, 772 texts to absorbed copies against 257 at the hop
  cap and the time to live before, for a third less airtime. The ring
  moves 0.3 points, within its seeds.
- **It ends the search where it crosses itself.** The spec's search no
  longer visits every reachable relay, and a text whose one copy comes
  back on its own path is lost: 14 of 100 on the mesh burst. The
  returns taken from a child fall from tens of thousands to under a
  thousand on every scenario: a search under the rule hardly ever
  backtracks, since it meets a crossed relay first.
- **The absorbed losses are the splits' too.** On `dense-power.toml`
  seed 7, 577 texts were lost to absorbed copies where 314 were lost at
  the hop cap and the time to live before, and delivery is the same: a
  message whose copies all end at crossed relays, or whose one copy does,
  is lost either way; what the time to live and the hop cap ended after
  minutes of airtime now ends at the first crossed relay.

## What it does not show

- **Whether a form that keeps the sender searching keeps the gain.** The
  storm's copies and the mesh's single searches are told apart by nothing
  the absorbing relay knows; the difference is on the sender's side,
  which is done with the message when its hand-over is taken. A reply
  that says "seen, not taken", after which the sender marks the relay
  tried and goes on to its next candidate, is the depth-first search's
  own answer to a cross link; it changes spec "The ACK" and "Hand-over",
  outside part 1, and was not built.
- **Parts 2 to 4 of the candidate**, alone or with this one.
- **Other seeds of `dense-power.toml`**, and the mesh at more than two
  seeds; and a sparse topology at scale, where the mesh's loss would be
  the rule's cost rather than the grid's neutrality.
- **The board.** Nothing here was run on hardware.

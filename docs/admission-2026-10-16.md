# Taking fresh work only while keeping up: measured

**Date:** 2026-10-16. Builds on `collapse-research-2026-10-16.md`, which
found that at three times the spec "Capacity and sizing" texting rate the
hand-overs into the centre of the domain fail and nothing slows the
edge, and on `backoff-2026-10-16.md` (D42), whose throw-away whole-relay
hold did better at four times than D42 alone. The question here: can a
relay's admission of fresh work, the texts of its leaves and of its own
application, reflect its own evidence of trouble on the way on, so that
overload pushes back on the application at the edge before the network
breaks down? Measured against main `8cd70a4`, paired by seed. Spec
references are to `protocol.md`.

## What was built

Fresh work is a packet with count 0, from a leaf or the relay's own
application. Today a relay refuses it for want of room in the send
queue or the bucket (D29), or while every live neighbour is banned
(D34). Three further gates were built behind a switch in a scratch copy
of the core, each refusing fresh work with the refused ACK of today; a
refused leaf holds its text and offers it again after the ladder of
spec "Attach and polls", one ban doubling up to 61 s, and drops it
after 600 s (`NoRelay`). Forwarding is unchanged in G1 and G2.

- **G1, backlog gate:** no fresh work while a forward or a return in the
  send queue waits for a ban, after a refusal or after silence (D42), or
  holds a hop place, which it took because the regular room or the
  bucket was full. A message queued in the regular room does not count:
  it waits only for the hand-overs ahead of it, which is normal flow,
  and admission already counts its airtime. A DELIVER does not count
  either: it waits for its leaf, not for the network. The relay's own
  message on its refusal ladder is a forward waiting for a ban, so it
  counts.
- **G2, failure gate:** no fresh work for `hold(k)`, one ban doubled
  k − 1 times up to 61 s, after the k-th failed or refused hand-over in
  a row to any neighbour; an ACK that takes ends the run.
- **H, whole-relay hold:** the throw-away experiment of the collapse and
  backoff notes, ported: after the k-th failed hand-over in a row no
  hand-over at all and no fresh work for `hold(k)`.

None of them adds a constant. G1 is the one kept (commit `382e712`, spec
commit `6612d9c`); its refusal is traced as `DropReason::Behind`, and
D34's case stays as `Banned`. The switch, G2 and H are not on the
branch. `Relay` stays 169 160 B and the leaf 776 B; the relay image
stays 309 KiB of flash and 173 KiB of RAM.

G2 does not subsume G1 or the reverse. With D42 a failed forward waits
for its neighbour's ban, so after a failure G1 closes for that ban as G2
does; but G2 also closes after refusals whose message went on at once,
and G1 also closes for a hop place held, which needs no failure.

## What was run

S4 (`scenarios/s4-capacity.toml`) at the texting rate times 1, 2, 3 and
4, 13 channels of 125 kHz, seeds 1 and 2. "Delivered" is the share of
reachable texts sent from 900 to 3900 s that arrived. The standard
suite: S1, mesh-burst, mesh-spread and the ring at 10 seeds, S1b, S2
and the corridor at 60. The loaded suite: the corridor at one text per
leaf every 1 s and 0.5 s, the ring at 1 s and 2 s, mesh-burst at 64
times, 30 seeds each. Differences are paired by seed, ± one standard
error; with two seeds the S4 errors say little. A message's **life** is
from its origin's core taking it to its delivery or drop; "in the
network" leaves out the texts a leaf held for 600 s and dropped
(`NoRelay`), which never entered it. A binary built from the switch at
its old value repeated main to the frame on the runs checked (S1, the
corridor, the ring).

## S4

Delivered, mean of seeds 1 and 2:

| Rate | main | G1 | G2 | H |
|---|---|---|---|---|
| ×1 | 96.65 % | 96.71 % | 96.66 % | 96.71 % |
| ×2 | 95.96 % | 95.95 % | 95.87 % | 96.02 % |
| ×3 | 36.5 % | 37.5 % | 35.0 % | **41.2 %** |
| ×4 | 10.0 % | 10.5 % | 10.4 % | 10.3 % |

Paired, against main: G1 ×3 +1.1 ± 2.9 points, ×4 +0.5 ± 1.3; G2 −1.4
± 1.9 and +0.4 ± 1.1; H +4.7 ± 1.1 and +0.2 ± 0.8. At ×1 and ×2 all
three are within 0.1 point.

**The cost at the spec "Capacity and sizing" rate and twice it.** G1:
DATA frames per text 6.39 → 6.46 (+0.07 ± 0.02) at ×1 and 9.74 → 9.32
at ×2, airtime +93 ± 12 s (+0.4 %) at ×1 and −1102 s at ×2, latency
p50 / p90 1.0 / 65.3 → 1.0 / 64.9 s at ×1 and 2.7 / 84.4 → 2.8 / 85.0 s
at ×2. G1 is invisible at the design rate. H costs latency there: p50
/ p90 +0.3 ± 0.0 / +4.0 ± 1.5 s at ×1, +2.7 ± 0.1 / +12.2 ± 1.6 s at
×2, for 1.6 fewer DATA frames per text at ×2. G2's changes at ×1 and ×2
are within the spread.

Delivered per 600 s bin at ×1 and ×2 stays at 94 to 98 % in every bin
for main and G1 alike.

Past the cliff, mean of the two seeds:

| | carried a second | DATA frames per text | relay time at the cap | hand-overs per delivered text | airtime, s | latency p50 / p90, s |
|---|---|---|---|---|---|---|
| ×3 main | 5.14 | 17.0 | 50 % | 8.9 | 144 924 | 58 / 632 |
| ×3 G1 | 5.29 | 17.0 | 50 % | 9.0 | 144 963 | 63 / 627 |
| ×3 G2 | 4.93 | 16.9 | 49 % | 9.1 | 146 773 | 63 / 624 |
| ×3 H | 5.84 | 13.3 | 22 % | 6.7 | 120 940 | 60 / 608 |
| ×4 main | 1.77 | 15.6 | 73 % | 13.6 | 201 796 | 352 / 1158 |
| ×4 G1 | 1.86 | 15.6 | 73 % | 14.0 | 202 465 | 390 / 1142 |
| ×4 G2 | 1.84 | 15.5 | 71 % | 13.8 | 202 844 | 367 / 1146 |
| ×4 H | 1.84 | 10.6 | 26 % | 7.5 | 159 754 | 214 / 951 |

What became of the reachable texts sent in the window, both seeds:

| | never admitted (`NoRelay`) | delivered | expired in a delivery queue | in flight at the end |
|---|---|---|---|---|
| ×3 main | 44.9 % | 36.5 % | 11.9 % | 6.8 % |
| ×3 G1 | 44.5 % | 37.6 % | 11.5 % | 6.5 % |
| ×3 G2 | 46.8 % | 35.0 % | 11.5 % | 6.6 % |
| ×3 H | 35.2 % | 41.2 % | 6.8 % | 16.8 % |
| ×4 main | 72.1 % | 10.0 % | 9.9 % | 8.0 % |
| ×4 G1 | 71.9 % | 10.5 % | 9.6 % | 7.9 % |
| ×4 G2 | 72.0 % | 10.4 % | 9.6 % | 7.9 % |
| ×4 H | 62.6 % | 10.3 % | 4.5 % | 22.6 % |

No message was dropped in the network for any other reason, and none
was lost without a trace except one in H at ×3, seed 1, not followed
(the backoff note found every such case to be two texts with the same
message ID in the simulator). H keeps more texts in flight at the end:
it stops hand-overs, so its queues hold what they would have sent.

Delivered per 600 s bin, by send time, seed 1 (seed 2):

| Bin starting, s | 600 | 1200 | 1800 | 2400 | 3000 | 3600 |
|---|---|---|---|---|---|---|
| ×3 main | 95 % (96 %) | 67 % (63 %) | 32 % (29 %) | 21 % (19 %) | 15 % (12 %) | 8 % (6 %) |
| ×3 G1 | 96 % (95 %) | 70 % (60 %) | 41 % (27 %) | 26 % (17 %) | 17 % (12 %) | 8 % (6 %) |
| ×3 G2 | 96 % (93 %) | 67 % (55 %) | 34 % (26 %) | 21 % (17 %) | 15 % (11 %) | 8 % (6 %) |
| ×3 H | 96 % (97 %) | 85 % (81 %) | 48 % (38 %) | 24 % (18 %) | 9 % (6 %) | 3 % (2 %) |
| ×4 main | 51 % (48 %) | 16 % (14 %) | 7 % | 6 % (5 %) | 4 % | 2 % |
| ×4 G1 | 55 % (42 %) | 19 % (13 %) | 9 % (7 %) | 6 % (5 %) | 4 % | 2 % |
| ×4 H | 67 % (63 %) | 21 % (15 %) | 6 % (3 %) | 2 % (1 %) | 1 % | 0 |

H holds the load a few minutes longer and then falls lower than main.

**Why the gates do so little.** The edge relays are not where the
evidence is. At ×3 the relays refuse 408 000 and 421 000 offers a run
for their bucket (seeds 1 and 2); G1 adds 2400 refusals a run and G2
16 000. Relays whose buckets are dry already refuse their leaves, and a
relay at the edge, whose hand-overs to its neighbours succeed, has no
message waiting and no hop place held, so it admits everything. By the
sending leaf's distance from the centre of the domain, main, ×3, seed 1:

| Leaf's distance from the centre | 0 to 600 m | 600 to 1200 m | 1200 to 1800 m | 1800 to 2400 m | 2400 m and more |
|---|---|---|---|---|---|
| Texts never admitted | 82 % | 76 % | 63 % | 45 % | 18 % (G1 15 %) |
| Texts delivered | 12 % | 18 % | 27 % | 38 % | 53 % (G1 58 %) |
| Of the admitted, living past 600 s | 11 % | 18 % | 23 % | 31 % | 41 % (G1 36 %) |

The pushback reaches the applications near the centre, whose relays
are full. The texts that clog the network are admitted at the edge, and
each relay on their way takes them, into a hop place if need be,
whatever its bucket holds (spec "Send queue"), so nothing at the edge
sees the centre's trouble. A gate that reads only the relay's own
queue cannot change that; H changes it only by slowing every relay's
own sending.

## Message lifetimes and the TTL

The TTL byte of draft D44 (branch `worktree-agent-a99f07dbac7d97a8c`)
was merged into scratch copies of main and of G1 for this measurement
only. In the network, messages sent in the window for p50, p90 and max;
messages of the whole run for the count past 600 s and the oldest in
flight at the end; both seeds:

| | life p50 / p90 / max, s | admitted, living past 600 s | oldest in flight at the end |
|---|---|---|---|
| ×3 main | 164 / 970 / 2916 | 16 880 of 73 606 (22.9 %) | 3141 s |
| ×3 G1 | 168 / 957 / 2747 | 16 247 of 73 944 (22.0 %) | 3233 s |
| ×3 H | 90 / 904 / 2750 | 21 658 of 82 292 (26.3 %) | 3316 s |
| ×3 TTL | 246 / 773 / 1192 | 11 513 of 78 107 (14.7 %) | 1137 s |
| ×3 G1 and TTL | 289 / 779 / 1192 | 11 462 of 76 968 (14.9 %) | 1166 s |
| ×4 main | 745 / 1541 / 3476 | 19 466 of 60 723 (32.1 %) | 3796 s |
| ×4 G1 | 751 / 1519 / 3548 | 19 590 of 60 744 (32.3 %) | 3859 s |
| ×4 H | 467 / 1495 / 3485 | 28 165 of 72 459 (38.9 %) | 4075 s |
| ×4 TTL | 575 / 910 / 1193 | 14 060 of 72 758 (19.3 %) | 1307 s |
| ×4 G1 and TTL | 576 / 910 / 1196 | 13 444 of 71 658 (18.8 %) | 1165 s |

**With the gate alone messages still live past their TTL**: at ×3 the
longest life in the network is 46 minutes and 22 % of the admitted pass
600 s (9915 of them expired in a delivery queue after arriving, 3148
were delivered late, 3184 were still in flight at the end, the oldest
54 minutes old); at ×4 59 minutes and 32 %. With the TTL no life passes
two TTLs, the one in transit and the delivery queue's own, counted from
arrival (spec "Delivery queue"); what still passes 600 s had arrived at
its owner.

Gate and TTL against the TTL alone, delivered: ×3 39.3 against 40.9 %,
×4 16.2 against 16.8 % (main 36.5 and 10.0 %). The gate adds nothing on
top of the TTL in S4. The TTL is needed whatever the gate.

## A signal carried back: throw-away experiments

If the evidence is in the centre, it has to travel to the edge. The ACK
has spare `aux` bits (spec "Packet formats"), so a throw-away copy of G1
set one on every ACK that takes a DATA while the answering relay takes
no fresh work, and a relay whose hand-over was taken by such an ACK took
no fresh work for a while. Four forms:

- **P:** for `hold(k)` after the k-th such ACK in a row, the hold
  running out even if a plain ACK comes; a relay sets the bit while its
  own gate is closed for any reason, the signal included, so the signal
  travels as far as the traffic.
- **P1:** as P, for one ban each time, without doubling.
- **P2:** as P, and a plain ACK that takes reopens at once.
- **P3:** as P2, but a relay sets the bit only on its own evidence,
  G1's: the signal goes one hop.

| | S4 ×1 | ×2 | ×3 | ×4 | corridor at 1 s, carried a second | corridor at 0.5 s | ring at 1 s | ring at 2 s | mesh-burst ×64, delivered |
|---|---|---|---|---|---|---|---|---|---|
| main | 96.65 % | 95.96 % | 36.5 % | 10.0 % | 1.364 | 1.342 | 2.506 | 2.752 | 0.946 |
| G1 | 96.71 % | 95.95 % | 37.5 % | 10.5 % | 1.364 | 1.342 | 2.626 | 2.813 | 0.970 |
| P | 96.64 % | 92.40 % | 35.8 % | 17.5 % | 0.209 | 0.201 | 0.205 | 0.213 | 0.972 |
| P1 | 96.70 % | 96.00 % | 38.0 % | 10.7 % | 1.136 | | 2.727 | | 0.987 |
| P2 | 96.71 % | 95.62 % | **46.8 %** | **21.3 %** | 0.311 | 0.292 | 0.294 | 0.337 | 0.992 |
| P3 | 96.73 % | 95.92 % | 37.9 % | 11.8 % | 1.402 | 1.432 | 2.749 | 2.843 | 0.974 |

The transitive forms do what the gates could not in S4. With P2 at ×3
relays are at the cap 5 % of the time against 50 %, DATA frames per
text fall from 17.0 to 7.7, the longest life in the network is 29
minutes and 4.7 % of the admitted pass 600 s; at ×4 delivered doubles,
better than the TTL alone. The texts are refused at their leaves
instead: never admitted 47.7 % at ×3 and 73.2 % at ×4, but of the
admitted 89 % and 80 % arrive, against 66 % and 36 % with main.

But they cannot be kept as they are. In the loaded corridor and ring
the network carries between a quarter and an eighth of what it carries
with main, and P costs 3.6 points at twice the design rate. A relay
closed only because a neighbour said so sets the bit too, so around a
loop of relays, or back and forth along a line, the signal most likely
keeps itself up with nothing behind it. P3, which sets it only on its
own evidence, has no such loss and carries as much as main or more
everywhere in the loaded suite, but one hop is not enough for S4: ×3
+1.4 ± 3.7, ×4 +1.8 ± 0.4 points. A signal that reaches the edge without feeding
itself is the open design question; none was measured here.

## Standard and loaded suites

G1 against main, paired.

| Scenario | Delivered | Carried a second | Airtime, s a run | Hand-overs per text | Latency p50 / p90, s |
|---|---|---|---|---|---|
| S1, 10 seeds | 1.000 → 1.000 | same | 13 → 13 | 2.00 → 2.00 | 0.2 / 0.2 both |
| S1b, 60 seeds | 1.000 → 1.000 | same | 29 → 29 | 2.00 → 2.00 | 0.6 / 1.2 both |
| S2, 60 seeds | 0.952 → 0.952 | same | 74 → 74 | 1.26 → 1.26 | 0.3 / 91.5 both |
| corridor, 60 seeds | 1.000 → 1.000 | same | 42 → 42 | 4.00 → 4.00 | 0.7 / 1.3 both |
| ring, 10 seeds | 0.9987 → 0.9971 (−0.0016 ± 0.0015) | +0.044 ± 0.020 | 1396 → 1385 | 3.16 → 3.07 | 1.5 / 6.1 → 1.4 / 5.3 |
| mesh-burst, 10 seeds | 1.000 → 1.000 | same | 138 → 138 | 4.61 → 4.61 | 2.1 / 4.5 both |
| mesh-spread, 10 seeds | 1.000 → 1.000 | same | 141 → 140 | 5.85 → 5.90 | 0.3 / 1.0 → 0.3 / 1.1 |
| corridor at 1 s, 30 seeds | 0.9963 → 0.9964 | 1.364 → 1.364 | 779 → 779 | 4.00 → 4.00 | 2.3 / 8.5 both |
| corridor at 0.5 s, 30 seeds | 0.9944 → 0.9944 | 1.342 → 1.342 | 834 → 834 | 4.00 → 4.00 | 5.5 / 21.2 both |
| ring at 1 s, 30 seeds | 0.9953 → 0.9956 | 2.506 → **2.626** (+4.8 %) | 1443 → 1448 | 3.42 → 3.28 | 4.0 / 17.8 → 3.5 / 14.5 |
| ring at 2 s, 30 seeds | 0.9982 → 0.9978 | 2.752 → **2.813** (+2.2 %) | 1401 → 1398 | 3.16 → 3.07 | 1.5 / 6.2 → 1.4 / 5.2 |
| mesh-burst ×64, 30 seeds | 0.946 → **0.970** (+0.024 ± 0.011) | 2.121 → 2.146 | 3608 → 3476 (−132 ± 50) | 5.76 → 5.75 | 5.6 / 55.7 → 4.4 / 42.2 |

In the corridors G1 changes one run of 120, and the results are main's.
In the rings a relay with a message waiting for one neighbour no longer
adds its leaves' texts behind it, and the ring carries more with fewer
hand-overs per text. In mesh-burst ×64 the texts that expired in
delivery queues fall from 2074 to 1001 over the 30 seeds. The one row
worse, the ring at 10 seeds, is seed 2 (0.984), where texts for three
leaves were still in flight at the end of the run after a congestion in
its last 90 s; at 30 seeds and either rate the ring is better. In S1
the leaf of the relay whose only neighbour is removed is refused while
the relay waits, as under D34, a little longer: its application skips
18 more texts over the ten seeds, none of which main delivered. Nothing
was dropped for want of a route, and nothing was lost without a trace,
in any G1 or main run. G2 and H were run on the same suites. G2
carries more in the rings too, 2.506 → 2.661 and 2.752 → 2.815 a
second, and lifts mesh-burst ×64 by 0.9 ± 0.5 points; elsewhere it is
within the spread. H carries 2.4 % and 0.9 %
more in the rings, with latency longer by up to 0.8 s at p50 and 3.4 s
at p90 (mesh-burst, the corridor at 0.5 s), the ring at 1 s 0.15 ±
0.05 points worse and mesh-burst ×64 0.6 ± 0.9 points.

**A cost the suites do not show.** G1 is coarse: one message waiting
for one neighbour closes the relay to fresh work for every
destination. A relay whose only way to one leaf's relay is refused for
that leaf's full delivery queue holds that message for as long as the
queue is full, up to ten minutes, and its own leaves wait meanwhile even
for destinations the other way (the routing test
`a_relay_with_only_banned_neighbours_waits_for_the_ban`). With D34 a
relay with another way open took them.

## Recommendation

1. **G1, as on the branch, awaiting the decision.** It is free at the
   spec "Capacity and sizing" rate and twice it, carries 2 to 5 % more
   in the loaded rings and lifts mesh-burst ×64 by 2.4 points, and it
   states D34 as the special case of a wider rule: take fresh work only
   while what the relay holds keeps moving. It does not answer the
   question of this note: in S4 it moves nothing measurable, because
   the edge relays it would have to stop see no trouble. It has the
   coarse cost above. G2 does as well or better in the rings and less in
   mesh-burst ×64, and needs a count and a timer where G1 reads the
   queue the relay already has; it is not kept. H buys its S4 gain at ×3
   with latency at the design rate and is not kept.
2. **The TTL (D44) is needed whatever the gate.** With G1 alone 22 % of
   the admitted messages at ×3 and 32 % at ×4 live past 600 s, the
   longest 46 and 59 minutes; G1 adds nothing on top of the TTL.
3. **Pushback that reaches the edge needs a signal carried back**,
   which P2 shows can bound lives and double what ×4 delivers; the
   form measured feeds itself around loops and must not be adopted.

## Decision

Not adopted (roadmap D45, 2026-10-16). G1 was measured on a research
branch and not merged; the commit hashes above are not in the
repository. Its gain in the loaded rings does not pay for closing a
relay to every destination behind one stuck message, and in S4 it does
nothing. The TTL of D44 was not adopted either. What remains open is a
signal carried back from the centre that cannot keep itself up around a
loop.

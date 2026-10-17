# A sleepy node that sleeps: measured

**Date:** 2026-10-17. Roadmap D47, measured, awaiting the decision.
Branch `research/sleep-window`, based on main `4397e5f` (D46) and
measured against it.

D46 measured one node type with sleepy nodes that listened all the time:
the simulator never slept a node, and holders sending only after a beat
was all that made a node a sleeper. This note asks what a node that
really sleeps sees, what its window after a beat should be, and how it
keeps track of its anchor while asleep.

## Summary

- **The core now says when the radio may sleep** (`Action::Sleep`,
  `core-interface.md` §3 rule 10), and the simulator makes a sleeping
  node deaf. Asleep, the D46 design loses its anchor: the neighbour table
  drops it after 390 s without a heartbeat, which a node that listens
  only after its beats rarely hears. In the capacity domain of spec
  "Capacity and sizing" at its texting rate, the D46 design asleep
  delivers 87.3 % against 96.2 % for main's listening nodes, and a
  sleepy node listens 698 s an hour, 1.1 mA at the 5.8 mA of listening,
  mostly to choose its anchor again.
- **The window the user asked for works for ordinary traffic and costs
  loaded flows to one node.** With deliveries first, a "more follows"
  bit in DATA, and a node that sleeps after the last message, a sleepy
  node takes what its holder holds on each beat and listens 95 s an hour
  instead of 187 s (both keeping the anchor until a hand-over to it
  fails). But flows of a text a second or two to one node, the rings and
  corridors of the loaded suites, deliver 57 to 79 % against 90 to 100 %
  with D46's 9.25 s window renewed by every message, which, at those
  rates, kept the node awake.
- **Keeping the anchor until a hand-over to it fails does not work.**
  Delivered falls to 83.5 %; 12 to 15 % of sleeping nodes beat where
  their anchor does not hear them, mostly because it moved channel; with
  a tenth of the relays removed, 60 % of their sleepy nodes were never
  heard by a new anchor before the run ended, and 6 % of the mail for
  them arrived.
- **Waking for the anchor's heartbeat does.** With each node's heartbeat
  interval fixed by its ID, a sleeping node knows when its anchor's
  heartbeat next comes to its channel and listens for it, under a second
  a rotation. It follows the anchor's channel moves, notices a dead
  anchor within two rotations and chooses again: delivered 92.4 %
  (−3.8 ± 0.1 points against listening nodes, +5.1 against D46 asleep),
  latency 138 s at the median and 335 s at the 90th percentile, nodes of
  removed relays heard by a new anchor after 480 s at the median, 97 % of
  them by the end. A sleepy node listens 106 s an hour at the median and
  171 s on average, 0.28 mA, most of it checking its anchor after it has
  been out of reach.
- **Fixed heartbeat intervals cost nothing measurable in collisions**:
  0.84 % of heartbeats were lost at a listening receiver with another
  heartbeat on the air there, against 0.80 % with random intervals.
- **Presence and hidden terminals do not change**: beats take 480 ms of
  airtime an hour per sleepy node over a run whose nodes all boot
  together (467 ms in D46), and 8.6 % of the DATA lost at the relay it was
  sent to had a sleepy beat on the air there, as for listening nodes.

The spec is edited for the window, the "more follows" bit, deliveries
first, fixed heartbeat intervals and the anchor watch.

## What was built

| Commit | Story |
|---|---|
| `1a063b1` | The core says when the radio may sleep; the simulator and the test harness make a sleeping node deaf and count its sleep. A sleeping node keeps its anchor's neighbour entry and checks it, listening a rotation, when a hand-over of its own message to it fails or it has moved too far. |
| `7245514` | Simulator: the sleepy nodes of a relay a scenario removes or restarts, how long until an anchor hears them again, and their mail meanwhile. |
| `a2d6cf7` | Deliveries go first; the window after a beat is the attempt the holder may have under way and one hand-over, 3.37 s; DATA's "more follows" bit; the node listens one hand-over, 3.13 s, after answering a DATA that says so, and sleeps after answering one that does not. |
| `896be56` | Every node heartbeats on a fixed interval from its ID; the rotation steps every slot; the first heartbeat at a random phase. |
| `cf385b9` | A sleeping node wakes for its anchor's heartbeat; two rotations without it start a check; the window after a beat shrinks to the attempt under way and one more, 477 ms; a failed hand-over no longer starts a check. |
| `f7ea3e2` | A check ends when the anchor is heard; one that hears nothing keeps the anchor. |
| `b9908a4` | The watch keeps the earliest time the anchor's heartbeat came, and allows the lag of a busy anchor. |
| `bbbc4aa` | After a check that heard nothing, the node checks again only once it hears something. |

**Free bits for "more follows".** DATA's `aux` holds the sender's home
channel, 0 to 12, in all four of its bits, and the time left takes its
byte up to 240. The only free bits in DATA are the five spare bits of
`dst_pos`; "more follows" is the lowest of them. The message ID covers
`dst_pos` with its spare bits zero, so the bit leaves it unchanged.

**The windows, derived.** The window after a beat must let the holder
bring one message that goes first. In `a2d6cf7` that is the attempt the
holder may have under way, a full frame and the ACK timeout, 238.56 ms,
and the message's whole hand-over, three attempts of 738.56 ms and the
retry waits of 305.92 and 611.84 ms (spec "Hand-over", "Retry wait"):
3372.00 ms. In `cf385b9`, once the anchor is tracked by its heartbeat,
it is the attempt under way and one more: 2 × 238.56 = 477.12 ms. A
DATA that says more follow renews the window by one hand-over, 3133.44 ms:
the holder has just had the ACK, so nothing is under way ahead of the
next message. A hand-over waiting between its attempts steps aside for a
delivery; one whose frame is on the air finishes its attempt first.

**The fixed interval, derived.** T = 5 s + 10 s × b / 256 from the last
byte b of the ID. 256 steps of 39.06 ms: two intervals that differ slide
past each other by more than two heartbeats' airtime, 23.68 ms, at every
heartbeat, so two heartbeats meet at most once in a row; 422 steps would
do, and 256 is one byte. The rotation steps every slot, sent or skipped,
so that a neighbour heard once on channel c at t is next on c + j at
t + j × T. A heartbeat may lag its slot by a hand-over attempt it finds
under way, 238.56 ms, and two listen-before-talk backoffs, 400 ms: 638.56
ms. Clocks are taken to be good to ±50 ppm, a common 32.768 kHz watch
crystal, so two drift apart by up to 19.5 ms over a rotation at the
longest interval. The watch opens the drift before the expected time and
closes the drift, the lag and the airtime after it.

## What was run

Six simulators: **M**, main `4397e5f` with its sleepy nodes listening
(`sleepy_listens`), the comparison the brief names; **O0**, the sleep
model with D46's 9.25 s window and its neighbour table dropping the
anchor (`1a063b1` with the keeping of the anchor undone); **Oa**, the
sleep model with the 9.25 s window, keeping the anchor until a hand-over
to it fails (`1a063b1`); **A**, the new window and "more follows" with the
same anchor rule (`a2d6cf7`); **F** and **Fr**, `896be56` with fixed and
with random heartbeat intervals, otherwise A; and **H4**, the heartbeat
watch (`bbbc4aa`). H, H2 and H3 are its steps on the way.

S4 (`scenarios/s4-capacity.toml`) at one and two times the spec
"Capacity and sizing" rate, seeds 1 and 2; S4 at the rate with every
tenth relay (27) restarting at 1800 s, and with the same relays removed
then; the standard and loaded suites of D42. Delivered counts reachable
texts originated from the settle time to the end less a delivery TTL.
Listening counts the time a sleepy node's radio was not asleep after
the settle time, 900 s in S4; in the suites, whose settle time comes
before most sleepy nodes have chosen an anchor, it includes the choice
and is not comparable with S4. No more than eight S4 runs went at once:
each takes 5 GB.

## S4

| | M | O0 | Oa | A | H4 |
|---|---|---|---|---|---|
| ×1, delivered, seeds 1, 2 | 96.2, 96.2 % | 87.5, 87.0 % | 87.7, 83.7 % | 85.2, 81.7 % | 92.4, 92.5 % |
| ×1, − M, points | | −8.9 ± 0.2 | −10.5 ± 2.0 | −12.8 ± 1.7 | −3.8 ± 0.1 |
| ×1, latency p50, p90 | 1.0 s, 122 s | 151 s, 392 s | 138 s, 363 s | 146 s, 364 s | 138 s, 335 s |
| ×1, airtime a run | 28 909 s | 29 508 s | 28 014 s | 28 054 s | 27 674 s |
| ×1, not delivered | QueueExpired 1025, Expired 72 | QueueExpired 3437, Expired 203 | QueueExpired 4006, Expired 118 | QueueExpired 4632, Expired 131 | QueueExpired 2055, Expired 140 |
| ×1, listening an hour, mean, median, p90 | 3600 s | 698, 614, 1086 s | 187, 148, 328 s | 95, 54, 245 s | 171, 106, 404 s |
| ×1, current of listening, mean | 5.8 mA | 1.13 mA | 0.30 mA | 0.15 mA | 0.28 mA |
| ×1, anchor changes a run | 8 124 | 15 368 | 515 | 506 | 3 916 |
| ×1, checks of the anchor a run | | | 2 800 | 3 234 | 21 640 |
| ×1, lost DATA with a beat on the air | 8.6 % | 8.1 % | 7.8 % | 7.3 % | 8.6 % |
| ×1, presence airtime per sleepy node | 467 ms/h | 474 ms/h | 464 ms/h | 464 ms/h | 480 ms/h |
| ×1, back from the dark, p50, p90 | 103 s, 249 s | 101 s, 279 s | 107 s, 291 s | 106 s, 276 s | 102 s, 254 s |
| ×2, delivered, seeds 1, 2 | 94.1, 90.6 % | 74.8, 69.6 % | 67.2, 57.7 % | 61.3, 48.9 % | 88.5, 69.9 % |
| ×2, − M, points | | −20.1 ± 0.8 | −29.9 ± 3.0 | −37.3 ± 4.4 | −13.2 ± 7.5 |
| ×2, latency p50, p90 | 4.8 s, 215 s | 189 s, 493 s | 179 s, 476 s | 185 s, 475 s | 171 s, 441 s |
| ×2, listening an hour, mean | 3600 s | 685 s | 295 s | 223 s | 253 s |
| Restart, delivered | 96.0 % | 86.1 % | 81.0 % | 76.2 % | 91.2 % |

Presence airtime is that of a run whose 20 000 sleepy nodes boot
together and heartbeat as relays until they have an anchor (D46); a
sleepy node's beats in the steady state take 212 ms an hour. Presence
in M counts its beats too, as main has no other presence frame.

**Where the mail is lost.** DATA sent to sleepy nodes, and messages
whose delivery TTL ran out at a holder, ×1 seed 1:

| | O0 | Oa | A | H4 |
|---|---|---|---|---|
| DATA to a sleepy node received | 16 434 | 15 624 | 15 130 | 16 274 |
| lost, the node asleep | 11 688 | 5 025 | 5 373 | 1 281 |
| lost otherwise | 2 898 | 1 463 | 1 518 | 921 |
| TTL out at the anchor, node in reach | 668 | 1 424 | 1 784 | 581 |
| the same, node dark | 334 | 370 | 406 | 338 |
| at a node no longer its anchor | 870 | 56 | 68 | 307 |
| sleeping nodes whose anchor does not hear them, sampled | 8.2 % | 10.3 % | 12.2 % | 5.3 % |
| of those, on another channel than the anchor's | 2.8 % | 6.4 % | 9.0 % | 0.8 % |

DATA sent to a node asleep is mostly not a holder's: in A, 85 % of it
went from nodes that were not the sleeping node's anchor, most more than
a minute into its sleep: forwards to a sleepy neighbour by nodes that
route through sleepy nodes, and sleepy nodes without an anchor that act
as relays. A sleeping node cannot be a next hop, and a beat does not say
that its sender sleeps; that is left open. The rest went from the anchor
within 5 s of the node falling asleep: a retry whose first ACK was lost
after the node answered a DATA without "more follows".

With rule (a), a node whose anchor moves channel keeps beating on the old
one until its own message fails, and the anchor holds its mail until the
TTL runs out: the share on the wrong channel grows from 2.8 to 9 % and
the messages expiring at the anchor with the node in reach from 668 to
1784. The anchor watch brings it to 0.8 %. In O0 the anchor drops out of
the table, the node chooses again, often another, and mail for the old
address expires there (870).

## The window and "more follows"

A against Oa is the window alone, both keeping the anchor until a
hand-over to it fails. At the S4 rate A delivers 2.2 points less and
listens half as long, 95 against 187 s an hour. Bursts were cut by the
holder's bucket, the node asleep before the next came, 1 to 3 times in
about 600 DATA saying more follow a run at the rate, and 13 to 20 in
about 1 500 to 2 500 at twice it: the bucket alone bounds a burst.

The loaded suites send a text every 0.5 to 2 s to one node:

| Scenario | M | O0 | Oa | A | H4 |
|---|---|---|---|---|---|
| Ring | 100 % | 91.0 % | 92.7 % | 65.4 % | 68.7 % |
| Ring at 1 s | 100 % | 89.5 % | 90.4 % | 59.3 % | 57.4 % |
| Ring at 2 s | 100 % | 94.4 % | 93.6 % | 64.7 % | 67.9 % |
| Corridor at 1 s | 100 % | 99.6 % | 99.6 % | 81.0 % | 78.6 % |
| Corridor at 0.5 s | 100 % | 100 % | 100 % | 79.5 % | 74.6 % |
| Mesh, burst ×64 | 94.4 % | 79.1 % | 79.0 % | 74.1 % | 74.7 % |

With the 9.25 s window renewed by every message, a node fed a text a
second never sleeps: the old window listens 1940 to 2610 s an hour in
these scenarios. With "more follows" it sleeps when its holder's queue
is empty, which at these rates is a second before the next arrives;
the next waits for the next beat, the holder's delivery queue of 41
fills, refuses, and the relays behind it hold the flow until its time
runs out (traced in the corridor at 1 s: `Expired` at the relay before
the holder). The standard suites deliver what Oa does: S1, S1b, the
corridor, the meshes 99.7 to 100 %, S2 91.2 % against 91.8 %.

A phone does not receive a text a second. If such flows matter, the node
could listen one hand-over after every message it takes, as the 9.25 s
window did, and use the bit only to stay longer; that was not measured.

## Keeping track of the anchor

| | O0, table drops it | (a) keep until a hand-over fails | heartbeat watch, H4 |
|---|---|---|---|
| ×1, delivered | 87.3 % | 83.5 % (A) | 92.4 % |
| ×1, listening an hour, mean | 698 s | 95 s (A) | 171 s |
| Relays removed: nodes heard again by an anchor | 1 940 of 1 980 | 800 of 2 020 (A) | 1 967 of 2 036 |
| the same, after p50, p90 | 600 s, 1260 s | 1320 s, 2400 s | 480 s, 1320 s |
| mail for them delivered | 58 % | 6 % | 65 % |
| Relays restarted: nodes heard again | 1 976 of 1 980 | 1 160 of 2 020 (A) | 2 001 of 2 036 |
| the same, after p50, p90 | 480 s, 840 s | 1200 s, 2340 s | 420 s, 780 s |
| mail for them delivered | 86 % | 33 % | 89 % |

"Heard again" is sampled once a minute: the node's anchor is alive and
has it in its heard table on its channel. A restarted relay boots on a
new channel; under (a) its sleepy nodes beat on the old one until a
hand-over of their own message fails, about once an hour in S4.

**Following a channel move.** `tests/sleepy.rs`
`a_sleeping_node_follows_its_anchor_to_a_new_channel`: a carrier on the
anchor's home a third of the time moves it; the sleeping node hears the
anchor's next heartbeat on the old channel, which names the new home, and
retunes within one rotation. A carrier there all the time keeps the
anchor's heartbeats off the old channel too (listen-before-talk), so the
node loses it and checks: a move the node can follow needs the old
channel usable at the anchor.

**What the checks cost.** The watch alone costs little: 477 ms after each
of 16 beats an hour, 7.6 s, and the watch window, open the drift before
and closed as soon as the heartbeat is heard. What remains is the checks:
in S4 a node is out of reach five minutes in an hour, and once two of its
anchor's rotations pass in the dark it listens a rotation, 195 s,
hearing nothing. The steps measured, ×1:

| | delivered | checks a run | listening, mean, median | presence |
|---|---|---|---|---|
| H, check after two missed rotations; one that hears nothing leaves the node without an anchor | 92.9 % | 19 781 | 357, 211 s | 607 ms/h |
| H2, the check ends when the anchor is heard; one that hears nothing keeps it | 92.0 % | 33 628 | 282, 118 s | 480 ms/h |
| H3, the watch from the earliest heartbeat and with a busy anchor's lag | 91.9 % | 35 204 | 278, 114 s | 481 ms/h |
| H4, after a check that heard nothing, wait to hear something | 92.4 % | 21 640 | 171, 106 s | 480 ms/h |

H left nodes in the dark without an anchor, so they heartbeat as relays
and listened all the time until they heard one. The remaining checks,
about one a node an hour, are mostly the first in each dark spell longer
than two rotations; the node cannot tell the dark from a dead anchor
until it has listened.

**Rule (b)**, a periodic listen, was not built: the watch and the check
cover what it would.

**The holder answering each beat** was built first (an ACK when nothing
waits, the first DATA otherwise, retried beats, two unanswered periods
starting a check) and measured on the suites only before the direction
changed: it delivered as A or a little better (ring at 1 s 63.2 %,
mesh-burst ×64 82.6 %) and listened 55 to 550 s an hour there; the
answers added 330 ms an hour of presence per sleepy node in S2. It was dropped for the heartbeat watch,
which needs no frame from the holder.

## Fixed heartbeat intervals

| S4 ×1, seeds 1, 2 | random (Fr) | fixed (F) |
|---|---|---|
| heartbeats at a listening receiver above sensitivity | 437 905 | 480 262 |
| received | 89.9 % | 90.0 % |
| lost with another heartbeat on the air | 0.80 % | 0.84 % |

Fixed intervals send 10 % more heartbeats: a random interval runs from
the end of the last heartbeat, after its listen-before-talk, a fixed one
from its slot. Two nodes in reach share an interval with probability
1/256, 68 of S4's 17 469 pairs; the simulator's IDs, `(i + 1) × 0x9E3779B1`,
give consecutive nodes different last bytes, so only 3 pairs share one
there and the case is not measured. A shared or 2:1 interval matters
only for a pair in step within 23.7 ms, 0.24 % of such pairs, about 0.16
pairs a domain; clocks drifting apart by up to 100 ppm unlock them within
four minutes. The simulator's clocks do not drift, so a locked pair stays
locked there; the random first phase keeps nodes that boot together from
starting in step.

## What this does not show

- No node moves in the simulator: the check after a move farther than
  0.67 of the distance to the anchor is tested (`tests/sleepy.rs`) but
  not measured.
- Clocks do not drift in the simulator; the watch's margins for drift
  are untested there. The ±50 ppm is a crystal's tolerance, to be
  measured on the boards.
- The firmware does not sleep its radio yet (`bark-driver` ignores
  `Action::Sleep`).
- Sleeping nodes are offered DATA as next hops by nodes that route
  through sleepy nodes; a sleepy node's beat does not say whether it
  sleeps.
- Neighbours keeping quiet around a heartbeat they know is coming, which
  fixed intervals make possible, is the next measurement and was not
  built.
- Two seeds of S4 at one and two times the rate, one of the restart and
  one of the removal; at twice the rate H4's two seeds differ by 19
  points.

## Recommendation

Adopt the sleep signal, deliveries first, "more follows", fixed
heartbeat intervals and the heartbeat watch (H4): asleep, the network
delivers 92.4 % at the rate against 96.2 % with nodes that listen all
the time, for 106 s of listening an hour at the median. Keeping the anchor
until a hand-over fails loses nodes for good when their anchor moves or
dies. The open costs are the loaded flows to one node, which lose a
quarter to a third of what they carry against the 9.25 s window, and the
listening of nodes in the dark.

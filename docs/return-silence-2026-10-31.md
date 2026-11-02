# A neighbour silent past its demotion is tried: the search goes back

**Date:** 2026-10-31, the dense-power runs and variant (b)
2026-11-02. **Status:** built and measured; roadmap D95, **not adopted
as built**: it brings the dense-power storm back, since a neighbour
that silently took the message is never offered it again and the
split's copies no longer merge. Variant (b), the mark counting only
while the neighbour is demoted, measured on a later base and **not
adopted** either (the last section). Spec references are to `protocol.md`. Measured against main
`09b7d73`, with D90 in, release builds of both, the same scenario files
and seeds.

## Summary

- **The trap.** The D90 note's trace (`storm-seen-2026-10-31.md`,
  "what the trace revealed") found a relay whose last untried neighbour
  was heard at −112 dBm but never received a full frame. Each hand-over
  to it went unanswered; the neighbour was banned, the ban doubled, and
  since a banned neighbour that is not tried keeps a message waiting
  (roadmap D42), and a neighbour whose heartbeats arrive never leaves
  the table, the message waited for every ban of the ladder, up to 61 s
  each, until its time to live ended. Four relays of that burst spent 11
  to 13 failed hand-overs each this way. The rule was main's; the
  depth-first backtrack of D90 reaches it more often.
- **The rule.** A hand-over that fails to a demoted neighbour, or whose
  failure demotes it, makes the neighbour *silent* for that message: it
  joins the message's tried set in the relay's record, so the message is
  offered to it no more there and does not wait for its ban. With no
  candidate left the message returns to its parent, or ends at the
  origin as no route. A refusal is pushback as before, and so is the
  first silence: the message waits one ban and tries the neighbour
  again. The silent set is per message, a bit per neighbour slot in the
  record, 8 bytes a record.
- **What it gains.** The mesh burst's texts that sat on a dead link
  until the time to live no longer do: at seed 5 the six texts main lost
  to the time to live are now returned, and one more text is delivered
  on the spread run. S4 at three times the rate, seed 1: 51.3 → 55.2 %
  delivered in the settled hour, latency p50 158 → 135 s, airtime 4 %
  less, 13 000 fewer failed hand-overs. At the design rate 93.4 →
  93.7 %, the walkers 96.4 → 96.5 %. Most of the gain under load is not
  the returns the rule counts, 60 in that run, but the offers it stops:
  a search that comes back through a relay no longer tries a demoted
  neighbour again.
- **What it costs: the storm.** `dense-power.toml` seed 1, the storming
  seed, 97.8 → 88.2 % in the settled hour, the run's airtime 299 754 →
  1 269 227 s, latency p90 46 → 321 s; seed 7, 98.0 → 94.6 %, airtime
  305 040 → 428 401 s. The offers the rule stops are the ones that
  healed a split: a hand-over whose ACKs were lost twice leaves the
  receiver holding the message and demoted, silent for it at the
  sender, and when the sender's copy comes back it is no longer offered
  to the receiver, where it merged with the copy held there (spec
  "Receiving DATA" case 4, roadmap D36, D42). Both copies go on and
  split again: hand-overs that failed with the receiver holding the
  message 125 885 → 870 599, seen ACKs 302 116 → 2 075 371, relays ever
  at their cap 108 → 1 799. Smaller costs: on the loaded ring three
  settled texts end as no route where main delivered them late, since
  silence there is congestion; returns fail more where the parent is
  faint too; and on a line whose only way on is a faint link the rule
  gives the message two hand-overs, six frames, where main gave it up
  to 33, so a link that carries one frame in ten gets the text through
  47 % of the time instead of 97 %.
- **Recommendation: not adopted as built.** The rule tells a faint
  neighbour from a busy one no better than the ladder did, and the
  split it stops healing costs far more than the trap it ends. What to
  build next is at the end: silence past the demotion as a reason not
  to *wait*, while the neighbour stays a candidate, or a silent mark
  that lifts with the neighbour's demotion.

## The rule

Spec "Neighbour table" (demotion), "Hand-over" step 6, "Bans",
"Overload waits", "Message records", "Next hop", "Return to parent",
"Why the search finds a path" and "Bounds"; `tables/records.rs`
`Record::silent`, `silent_to`, `tried`, `silent_among`;
`relay/forward.rs` `handover_failed`, `hop`, `others_open`, `ban_ends`,
`route`.

1. A hand-over of a message to a neighbour fails: three attempts
   unanswered (spec "Hand-over" step 6). The failure is counted and the
   neighbour banned, one ban doubled for each failure in a row, as
   before (roadmap D42).
2. If the neighbour is not demoted after the failure, the message waits
   in its place for the ban and is offered to the same neighbour again
   with fresh attempts, as before.
3. If the neighbour is demoted after the failure, whether this failure
   demoted it or it was demoted already, the neighbour is **silent for
   the message**: its bit is set in the record's `silent` mask. It is in
   the message's tried set from then on: no candidate for the message
   at this relay, and its ban is not waited for. The message chooses
   its next hop at once.
4. With no candidate left, and no neighbour kept out by its ban alone,
   the message returns to its parent (spec "Return to parent"), or at
   the origin is dropped for want of a route; the application is told
   nothing, as before (roadmap D10).
5. A refusal is unchanged: the refuser is set aside for the choice and
   banned for the wait it asks, and a message whose remaining
   candidates are banned by refusals, or by a silence that has not yet
   demoted them, waits (spec "Overload waits").
6. The mask is cleared with `handed` when a neighbour-table slot
   changes hands. The next message's record starts empty: it tries the
   neighbour once more, after the ban, demoted ones last as before.

**Per message, not per neighbour.** The table already knows a demoted
neighbour; the rule could have read that alone: "a demoted neighbour's
ban is not waited for". That costs no RAM and returns a message at once
when it arrives at a relay whose only way on is demoted and banned,
without ever offering it. The choice here is per message, with the
cost of 8 bytes a record, 1571 records, 12.3 KiB a relay (`Record` 40
→ 48 bytes, `Relay` 170 480 → 183 032 bytes, 55 KiB of the board's RAM
left), because each message then gets one hand-over of its own at the
neighbour, after its ban: the demotion was earned by other messages'
frames, which are lost to collisions far more often than to a dead link
(roadmap D34), and a line's only way on should not be closed to every
message by two lost frames (spec "Next hop", on why a demoted neighbour
is still tried). The cost of that one try is bounded, one ban of at
most 61 s and three frames per message per neighbour, where the trap's
was the time to live. A count of the message's silent attempts, as
the question was put, would need more than a bit and buy nothing: one
failure past the demotion is the whole fact.

The mask lives in the record, not with the held message, so that a copy
that comes back to the relay, returned by a child or offered by a split
and answered seen, does not pay the neighbour's attempts again. This is
what the search bound in spec "Bounds" now says: a link that does not
answer costs a message at most two hand-overs at a relay, however often
the search comes back through it.

**When the neighbour comes back.** A fade lifts, or the neighbour
answers another message's hand-over, and its demotion ends. The message
that gave it up is elsewhere: with its parent, which goes on with its
own candidates, or ended at the origin. Its record at the relay keeps
the neighbour silent, so even a copy that comes back there does not try
it; the path through that neighbour is lost to this message. The next
message finds it: its own hand-over, after the neighbour's ban, and the
neighbour's first ACK ends the demotion and the ladder.

**The line.** A relay whose only way on is a faint neighbour, heard but
rarely reached. Before: the message waited for the ladder, 11 failed
hand-overs and 33 frames before the entry expired, or the time to live,
600 s, if the heartbeats kept the entry alive. Now: two hand-overs, six
frames, one ban between, and the message goes back, to be dropped at
the origin a few seconds after it was sent. What that costs is the
chance that a frame would have got through later. With each frame
getting through independently with probability p:

| p | two hand-overs, 6 frames | the ladder, 33 frames |
|---|---|---|
| 0.02 | 11 % | 49 % |
| 0.05 | 26 % | 82 % |
| 0.10 | 47 % | 97 % |
| 0.20 | 74 % | 100 % |
| 0.30 | 88 % | 100 % |
| 0.50 | 98 % | 100 % |

A link that carries one frame in five loses a quarter of its texts to
the rule; one in ten, half. The traced link carried none of 33 frames
in 537 s. What the rule buys on the line is that the text ends in
seconds rather than minutes, and that the relay and the link are free
for the next text, which gets its own two hand-overs. A link that
works but was demoted by two collisions costs the next message a wait
for the ban, at most 61 s, and nothing else: its first ACK clears the
demotion (`a_demoted_neighbour_is_the_last_resort_for_the_next_message`).

Fading in the simulated links has a standard deviation of 4 dB and a
correlation time of 60 s, so a link a few dB below the full-frame level
is the one the table covers.

## Why

A message with a way forward goes forward; one with none goes back.
That is the depth-first search, and spec "Return to parent" says it.
Roadmap D42 added one exception: a neighbour banned for silence is not
tried, so a message with no other candidate waits for the ban rather
than go the long way round on channels that are full. Measured, that
wait was what lifted S4 at three times the rate from 16 to 33 % and
250 kHz from 14 to 83 %; going to the next candidate at once undid it.

The wait was meant for overload, and overload answers: a refusal names
the wait, and the message honours it. Silence does not say what it is.
Once, it is most likely a collision, and the message waits one ban and
tries again, which D42 measured and this rule keeps. Twice in a row,
the table demotes the neighbour; the spec's reason for still trying a
demoted neighbour is that it may be the only way on. It is still tried
by each message, once. What this rule ends is the third, fourth and
seventh offer of the same message to a neighbour that has not answered
it twice, each after a longer wait, on the strength of a ban ladder
built for a bucket that refills: a dead or one-way link does not refill.

The trace made the cost plain: a search that had crossed 21 relays in
13 s and had one neighbour left at its last relay sat there 537 s and
ended at the time to live. Nothing in the network was overloaded.

## How it was measured

Every run is the scenario file at the seed named, with `--json` and
`--messages`; the figures are the JSON summary's: delivered of the
texts sent in the settled hour and of all texts, latency, the run's
airtime, the cores' counters summed over the nodes, drops and texts
lost by reason, the relays whose load sample ever found the bucket
under its reserve, and the largest settled airtime of a node. Main is
`09b7d73`, D90 adopted, built and run here; the branch is this one at
the commit that carries the simulator's counter. The three-times-rate
S4 is `s4-capacity.toml` with `per_leaf_per_hour` 2.875. "Ended past a
silent neighbour" counts the searches that returned or were dropped at
the origin with a live neighbour silent for the message: those that
would have waited before.

## The tables

Main / this rule.

| | dp1 | dp7 | s4x1 | s4x3-s1 | ring | movers |
|---|---|---|---|---|---|---|
| texts sent in the settled hour | 18234 / 18194 | 18189 / 18189 | 17459 / 17457 | 47738 / 48055 | 2919 / 2856 | 17894 / 17892 |
| delivered, settled hour % | 97.8 / 88.2 | 98.0 / 94.6 | 93.4 / 93.7 | 51.3 / 55.2 | 91.4 / 91.8 | 96.4 / 96.5 |
| delivered, whole run % | 96.5 / 89.8 | 96.9 / 94.4 | 93.4 / 93.7 | 58.5 / 62.3 | 91.7 / 91.5 | 95.5 / 95.6 |
| latency p50 s | 10.4 / 23.6 | 10.5 / 11.2 | 1.4 / 1.4 | 158.5 / 134.8 | 58.5 / 53.5 | 5.5 / 5.4 |
| latency p90 s | 46.0 / 320.6 | 45.1 / 66.1 | 187.9 / 189.1 | 693.2 / 662.6 | 145.8 / 158.6 | 57.3 / 50.5 |
| airtime, whole run s | 299754 / 1269227 | 305040 / 428401 | 44213 / 44142 | 169524 / 163152 | 1817 / 1792 | 184940 / 187599 |
| hand-overs taken | 438601 / 1993393 | 449952 / 605225 | 116275 / 116452 | 391541 / 392218 | - / - | 322312 / 328185 |
| hand-overs failed | 525536 / 3009061 | 544701 / 886971 | 19377 / 19128 | 184900 / 171766 | - / - | 287084 / 271150 |
| failed, receiver had it | 125885 / 870599 | 128604 / 230498 | 4009 / 3989 | 44738 / 41126 | - / - | 154968 / 137520 |
| returns taken from a child | 3631 / 26881 | 3812 / 8062 | 75 / 106 | 1020 / 955 | - / - | 3340 / 3871 |
| copies answered seen | 267568 / 1811154 | 270206 / 443009 | 7860 / 7061 | 54562 / 50968 | - / - | 210500 / 218474 |
| seen ACKs sent : acted on | 302116 : 255524 / 2075371 : 1658876 | 305720 : 257736 / 499728 : 409022 | 8470 : 7764 / 7599 : 6973 | 60927 : 53031 / 56618 : 49630 | 0 : 0 / 0 : 0 | 240184 : 200376 / 247263 : 208589 |
| ended past a silent neighbour: returned | 0 / 3682 | 0 / 2614 | 0 / 9 | 0 / 59 | 0 / 29 | 0 / 2136 |
| ended past a silent neighbour: dropped at the origin | 0 / 4 | 0 / 7 | 0 / 0 | 0 / 1 | 0 / 1 | 0 / 2 |
| hand-overs held back by the bucket | 24678 / 1941594 | 20288 / 120806 | 64 / 44 | 1007236 / 911330 | - / - | 9030 / 5380 |
| neighbours demoted | 217639 / 1240082 | 226798 / 383607 | 7164 / 7240 | 53889 / 49969 | - / - | 110218 / 117691 |
| copies dropped: hop cap | 1488 / 2214 | 1584 / 1441 | 37 / 32 | 26 / 34 | 0 / 0 | 1550 / 1555 |
| copies dropped: TTL | 792 / 79750 | 741 / 231 | 340 / 342 | 7174 / 6238 | 0 / 0 | 970 / 225 |
| copies dropped: no route | 141 / 347 | 155 / 161 | 265 / 242 | 1015 / 1058 | 26 / 40 | 114 / 111 |
| copies dropped: return failed | 49 / 849 | 48 / 476 | 0 / 5 | 3 / 22 | 3 / 5 | 36 / 494 |
| copies dropped: return seen | 0 / 21 | 1 / 16 | 0 / 0 | 0 / 1 | 0 / 0 | 3 / 20 |
| texts lost: hop cap | 237 / 262 | 219 / 258 | 30 / 26 | 16 / 24 | 0 / 0 | 161 / 212 |
| texts lost: TTL | 98 / 921 | 83 / 54 | 322 / 318 | 6603 / 5858 | 0 / 0 | 114 / 59 |
| texts lost: no route | 6 / 24 | 8 / 21 | 13 / 10 | 95 / 92 | 26 / 36 | 7 / 6 |
| texts lost: return failed | 5 / 39 | 5 / 24 | 0 / 1 | 2 / 11 | 2 / 1 | 1 / 21 |
| texts lost: return seen | 0 / 1 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 |
| texts that waited at their origin | 50 / 2812 | 65 / 164 | 180 / 191 | 242780 / 222222 | 2474 / 2150 | 129 / 84 |
| relays ever at their cap | 108 / 1799 | 106 / 488 | 0 / 0 | 188 / 175 | - / - | 171 / 115 |
| largest settled airtime of a node % | 4.94 / 8.79 | 4.74 / 5.48 | 5.65 / 5.33 | 9.98 / 9.97 | 9.98 / 9.84 | 5.11 / 5.08 |
| heartbeats received % | 84.9 / 68.8 | 84.7 / 82.8 | 94.4 / 94.4 | 87.7 / 88.1 | 90.6 / 87.4 | 89.4 / 89.4 |

The random mesh, 25 relays and 25 leaves, every leaf sending across it
once, all at once (`mesh-burst.toml`) or each at its own time with the
middle relay restarting (`mesh-spread.toml`); at seed 5 the scenario
counts the texts of one relay as unreachable:

| | burst-s1 | burst-s5 | spread-s1 | spread-s5 | ring |
|---|---|---|---|---|---|
| texts sent in the settled hour | 100 / 100 | 100 / 100 | 90 / 90 | 90 / 90 | 2919 / 2856 |
| delivered, settled hour % | 100.0 / 100.0 | 92.0 / 92.0 | 100.0 / 100.0 | 92.2 / 93.3 | 91.4 / 91.8 |
| delivered, whole run % | 100.0 / 100.0 | 92.0 / 92.0 | 100.0 / 100.0 | 92.2 / 93.3 | 91.7 / 91.5 |
| latency p50 s | 3.7 / 3.5 | 1.9 / 2.1 | 0.6 / 0.6 | 0.6 / 0.6 | 58.5 / 53.5 |
| latency p90 s | 6.7 / 7.3 | 4.9 / 4.3 | 0.8 / 0.8 | 0.8 / 0.9 | 145.8 / 158.6 |
| airtime, whole run s | 232 / 232 | 263 / 279 | 183 / 183 | 228 / 225 | 1817 / 1792 |
| hand-overs taken | 629 / 622 | 565 / 601 | 469 / 469 | 504 / 530 | - / - |
| hand-overs failed | 78 / 69 | 165 / 117 | 0 / 0 | 134 / 58 | - / - |
| failed, receiver had it | 40 / 25 | 113 / 41 | 0 / 0 | 101 / 36 | - / - |
| returns taken from a child | 2 / 2 | 22 / 31 | 0 / 0 | 25 / 41 | - / - |
| copies answered seen | 168 / 183 | 356 / 379 | 72 / 72 | 319 / 324 | - / - |
| seen ACKs sent : acted on | 173 : 166 / 188 : 182 | 373 : 349 / 398 : 379 | 72 : 72 / 72 : 72 | 323 : 318 / 327 : 324 | 0 : 0 / 0 : 0 |
| ended past a silent neighbour: returned | 0 / 0 | 0 / 10 | 0 / 0 | 0 / 10 | 0 / 29 |
| ended past a silent neighbour: dropped at the origin | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 1 |
| hand-overs held back by the bucket | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | - / - |
| neighbours demoted | 15 / 17 | 29 / 35 | 0 / 0 | 22 / 20 | - / - |
| copies dropped: hop cap | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 |
| copies dropped: TTL | 1 / 0 | 6 / 0 | 0 / 0 | 3 / 0 | 0 / 0 |
| copies dropped: no route | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 26 / 40 |
| copies dropped: return failed | 0 / 1 | 2 / 9 | 0 / 0 | 2 / 7 | 3 / 5 |
| copies dropped: return seen | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 |
| texts lost: hop cap | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 |
| texts lost: TTL | 0 / 0 | 6 / 0 | 0 / 0 | 3 / 0 | 0 / 0 |
| texts lost: no route | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 26 / 36 |
| texts lost: return failed | 0 / 0 | 1 / 8 | 0 / 0 | 2 / 6 | 2 / 1 |
| texts lost: return seen | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 |
| texts that waited at their origin | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 2474 / 2150 |
| relays ever at their cap | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | - / - |
| largest settled airtime of a node % | 0.52 / 0.51 | 0.90 / 0.65 | 0.41 / 0.41 | 0.73 / 0.56 | 9.98 / 9.84 |
| heartbeats received % | 99.6 / 99.6 | 99.0 / 99.3 | 99.6 / 99.6 | 99.3 / 99.2 | 90.6 / 87.4 |

## What it shows

- **The storm is back.** On `dense-power.toml` seed 1 main with D90
  delivers 97.8 % of the settled hour's texts in 299 754 s of airtime;
  with this rule 88.2 % in 1 269 227 s, with 2 075 371 seen ACKs sent
  against 302 116 and 870 599 failed hand-overs whose receiver held the
  message against 125 885: the D89 note's chain reaction of split
  copies. Seed 7, which did not storm on main, loses 3.4 points and
  40 % more airtime. The cause is in the rule's own definition. A
  hand-over whose frames reach the receiver and whose ACKs are lost
  looks the same as one whose frames are lost: silent. Twice, and the
  receiver is demoted and silent for the message at the sender, which
  moves on; so far as before. Before, when the sender's copy came back,
  returned by a child or routed again after a seen answer, the receiver
  was a candidate again, and the offer found it holding the copy: ACK
  only, the copies merged, one search went on (spec "Receiving DATA"
  case 4). That offer is what roadmap D36 and D42 called the one round
  trip a split costs, and in a dense network, where a quarter of the
  failed hand-overs are lost ACKs, it was the heal. Now the receiver is
  tried, the offer never comes, both copies search on, and each splits
  again. 3 682 searches ended past a silent neighbour in the run; what
  it cost was the 745 000 offers that no longer merged anything.
- **Under load elsewhere the gain is the offers the rule stops, not the
  returns it counts.** S4 at three times the rate, seed 1: 51.3 → 55.2 % in the
  settled hour, 2 034 more texts delivered, 971 fewer refused at their
  origin, 745 fewer ended at the time to live; latency p50 158 → 135 s;
  airtime 4 % less. Only 60 searches ended past a silent neighbour: a
  loaded relay seldom has one candidate left. What changed for the
  rest is that a search coming back through a relay, 955 returns and
  49 630 seen answers acted on, no longer offers the message again to a
  neighbour that was silent for it twice already: 13 134 fewer failed
  hand-overs, 3 920 fewer demotions, a tenth fewer hand-overs held back
  by the bucket. Under D90 a search comes back through its relays far
  more than it did, and each time paid those frames.
- **At the design rate and on the walkers it is neutral in delivery.**
  S4 93.4 → 93.7 %; the walkers 96.4 → 96.5 %, latency p90 57 → 50 s,
  airtime 1.4 % more. On the walkers 2 136 searches ended past a silent
  neighbour, and the losses move: 55 fewer texts at the time to live,
  51 more at the hop cap, 20 more as returns that failed. A neighbour
  that walks out of reach is silent twice and the message goes back
  rather than waiting for it; a parent that has walked too fails the
  return (spec "Return to parent"), and a search that goes back and on
  spends its hops.
- **On the ring three settled texts are lost that main delivered
  late.** 91.4 → 91.8 % in the settled hour, 91.7 → 91.5 % over the
  run; latency p50 58 → 53 s, p90 146 → 159 s. The ring runs at its cap
  and silence there is congestion: 29 searches went back past a silent
  neighbour, and three texts of 1 900, after 16 to 24 hops, reached the
  origin with nothing left and were dropped for want of a route, where
  main's copies waited and got through. The ring is one seed.
- **The mesh loses nothing and ends its stuck texts sooner.** The burst
  at seed 1 is 100 % on both; at seed 5 both deliver every reachable
  text, and the six that main held until the time to live are now
  returned and end as failed returns: the relay whose texts are
  unreachable is faint both ways. The spread run at seed 5 delivers one
  text more, the one that sat in the trap.
- **The trap itself is rare**, 9 to 60 searches in the big runs, and
  2 136 on the walkers; what it cost, where it bit, was the whole time
  to live.

## What it does not show

- **More seeds.** S4 at three times the rate and the ring are one seed
  each; the spread between seeds of the former was seven points in the
  D90 note. The storm's sign and size on dense-power need no more
  seeds; the S4 gain's size is not settled.
- **The variants below.** Variant (a) is not built; (b) is measured
  in the last section.
- **A line with a faint link that sometimes works.** The table above is
  arithmetic; no scenario here has such a link as the only way on.
- **The board.** Nothing here was run on hardware.

## Recommendation

Not adopted as built. The rule does what it was asked: the trap is
gone on every run, the mesh keeps every reachable text, and where the
search backtracks most it saves frames. But "silent for the message"
as a mark that lasts for the message's life at the relay also ends the
offer that healed a split, and the dense-power runs show that the
offers it stops there are mostly those: a storm of 1.27 M s of airtime
for a trap that bit 3 682 searches. The two silences the rule reads as
"this link does not carry the message" are, in a dense network, as
often "this link carries the message and loses the ACK", and nothing
in the sender's record tells them apart.

Two ways to keep the trap's cure without the split's cost:

- **(a) Silence past the demotion as a reason not to wait, not a tried
  mark.** The silent bit keeps a message from waiting for that
  neighbour's ban, and from nothing else: when the message is routed
  again and the neighbour is live and not banned, it is offered again,
  demoted ones last as before, and the split heals when the offer
  finds the copy. In the trap the message returns as soon as its last
  candidate is demoted and banned, which is the moment it is demoted;
  a copy that comes back pays three frames more at the faint
  neighbour, then returns again. What it loses against main is the
  heal of a split whose receiver's ban has not lapsed when the copy
  comes back, which in a storm, where the ladder climbs by other
  messages' failures, may be most of them. `ban_ends` alone reads the
  bit; `hop` and `others_open` go back to `handed`.
- **(b) A silent mark that lifts with the demotion.** The bit stays per
  message but counts only while the neighbour is demoted: an ACK from
  the neighbour to any hand-over ends its demotion
  (spec "Neighbour table") and with it the mark, so a neighbour that is
  alive and merely lost two ACKs is a candidate again as soon as it
  answers anyone, and a faint one that answers nobody stays silent until its
  demotion lapses at 390 s. This keeps the heal wherever the receiver
  is alive, which a split's receiver is, and keeps the trap's return
  wherever it is not. `Record::tried` takes the table's demotion as a
  second argument; the rest stands.

The second keeps more of D42 and is the one to measure first, on
`dense-power.toml` seeds 1 and 7 before anything else.

## Variant (b), measured

**Date:** 2026-11-02. Variant (b) above, built on main `8ff3a74`, which
has D93 (`storm-ack-2026-10-31.md`) but not D96 or D102. The code is on
the branch `worktree-agent-a7dcf1e57960c8617` at `e09b541`; it is not
on main.

### The rule as built

The first form's rule, steps 1 to 6 of "The rule" above, with one
change: the silent mark counts only while the neighbour table keeps
the neighbour demoted. A neighbour is in the message's tried set if it
has the message from this relay, or if it was silent for the message
and is demoted now. Its demotion ends when it takes or answers any
hand-over from this relay, of any message, or 390 s after the failure
that demoted it (spec "Neighbour table"); a heartbeat or a refusal does
not end it. From then on it is a candidate for the message again, in
its usual place (spec "Next hop"), and if it is banned the message
waits for the ban as for any candidate's (spec "Overload waits"). A
copy of the message that this relay holds or takes back is offered to
it again, and one that holds a copy answers with an ACK alone and the
copies merge (spec "Receiving DATA" case 4). The mark itself stays
until the slot changes hands, so a neighbour demoted again later is
silent for the message at once. "Ended past a silent neighbour" counts
the searches that returned or were dropped at the origin while the mark
counted.

### How it was measured

As for the first form, on main `8ff3a74` and the branch at `e09b541`,
release builds of both, the same scenario files and seeds. The city at
twice the rate is `dense-power.toml` with twice the texts per node, as
in `storm-busy-2026-10-31.md`; S4 at three times the rate is as above.
"Merged" and "seen" split the copies a relay answered without taking
them into those merged with a copy it held (case 4) and those it had
finished with (case 6).

### The tables

Main / variant (b).

| | dp1 | dp2 | dp7 | dp×2 s1 | dp×2 s7 | s4x1 | s4x3-s1 | movers |
|---|---|---|---|---|---|---|---|---|
| texts sent in the settled hour | 18234 / 18234 | 18199 / 18199 | 18189 / 18189 | 36559 / 36535 | 36595 / 36588 | 17457 / 17457 | 48831 / 48761 | 17893 / 17891 |
| delivered, settled hour % | 97.8 / 97.8 | 98.1 / 98.2 | 98.1 / 98.1 | 88.8 / 88.5 | 89.1 / 88.9 | 93.6 / 93.5 | 63.6 / 63.7 | 96.5 / 96.6 |
| latency p50 s | 10.2 / 9.9 | 10.0 / 9.9 | 10.0 / 9.8 | 46.5 / 46.4 | 49.1 / 46.1 | 1.4 / 1.4 | 83.0 / 95.2 | 5.4 / 5.4 |
| latency p90 s | 47.4 / 43.6 | 43.2 / 41.0 | 45.8 / 44.1 | 349.6 / 349.2 | 349.5 / 345.2 | 187.4 / 186.7 | 582.0 / 595.4 | 52.8 / 51.5 |
| airtime, whole run s | 283128 / 282972 | 283210 / 275716 | 271385 / 264486 | 1118451 / 1109701 | 1036475 / 1024802 | 43498 / 42880 | 150125 / 154267 | 184045 / 181642 |
| hand-overs failed | 480395 / 462775 | 477520 / 440722 | 453585 / 421259 | 2777351 / 2723834 | 2540036 / 2485526 | 18614 / 18248 | 139390 / 143904 | 286420 / 259801 |
| failed, receiver had it | 108676 / 102065 | 114376 / 97014 | 101670 / 85889 | 559603 / 536987 | 496462 / 481546 | 3406 / 3224 | 28661 / 29116 | 154020 / 128642 |
| copies merged (case 4) | 2108 / 1985 | 2216 / 1828 | 1823 / 1565 | 36174 / 36178 | 32295 / 31670 | 26 / 16 | 1139 / 1150 | 1639 / 1389 |
| copies answered seen (case 6) | 244127 / 247276 | 250721 / 233720 | 212914 / 199248 | 1001196 / 999905 | 901277 / 887368 | 7766 / 6977 | 41910 / 44816 | 211976 / 207935 |
| returns taken from a child | 3193 / 4629 | 3510 / 4327 | 2880 / 3684 | 10038 / 11720 | 8775 / 11207 | 41 / 41 | 340 / 377 | 3312 / 3397 |
| ended past a silent neighbour: returned | 0 / 2739 | 0 / 2508 | 0 / 2146 | 0 / 3852 | 0 / 4593 | 0 / 6 | 0 / 64 | 0 / 1903 |
| ended past a silent neighbour: dropped at the origin | 0 / 5 | 0 / 5 | 0 / 8 | 0 / 16 | 0 / 15 | 0 / 1 | 0 / 1 | 0 / 5 |
| hand-overs held back by the bucket | 18179 / 8326 | 18324 / 7948 | 17495 / 6882 | 1902851 / 1850628 | 1804117 / 1744647 | 46 / 87 | 710046 / 743311 | 9898 / 3275 |
| texts lost: hop cap | 261 / 283 | 267 / 312 | 222 / 272 | 386 / 474 | 426 / 491 | 32 / 29 | 29 / 30 | 168 / 213 |
| texts lost: TTL | 78 / 41 | 90 / 36 | 80 / 34 | 1741 / 1738 | 1599 / 1594 | 304 / 303 | 4582 / 4813 | 117 / 57 |
| texts lost: no route | 4 / 7 | 4 / 9 | 5 / 12 | 14 / 26 | 19 / 35 | 3 / 7 | 15 / 26 | 3 / 6 |
| texts lost: return failed | 3 / 22 | 3 / 31 | 2 / 27 | 48 / 81 | 35 / 103 | 0 / 1 | 0 / 6 | 2 / 25 |
| texts lost: return seen | 0 / 0 | 0 / 1 | 0 / 0 | 0 / 1 | 0 / 3 | 0 / 0 | 0 / 0 | 0 / 1 |
| texts that waited at their origin | 80 / 55 | 87 / 49 | 48 / 42 | 8430 / 8823 | 7970 / 8182 | 154 / 177 | 186140 / 194501 | 190 / 86 |
| relays ever at their cap | 73 / 56 | 86 / 55 | 73 / 43 | 813 / 702 | 726 / 666 | 0 / 0 | 148 / 152 | 179 / 93 |

The mesh and the ring:

| | burst-s1 | burst-s5 | spread-s5 | ring |
|---|---|---|---|---|
| texts sent in the settled hour | 100 / 100 | 100 / 100 | 90 / 90 | 2964 / 2843 |
| delivered, settled hour % | 99.0 / 100.0 | 92.0 / 93.0 | 92.2 / 96.7 | 92.6 / 94.3 |
| latency p50 s | 3.5 / 3.5 | 1.9 / 1.9 | 0.6 / 0.6 | 41.5 / 59.4 |
| latency p90 s | 8.9 / 8.9 | 3.8 / 4.0 | 1.0 / 4.1 | 159.2 / 156.7 |
| airtime, whole run s | 232 / 256 | 274 / 282 | 242 / 260 | 1807 / 1829 |
| hand-overs failed | 82 / 100 | 205 / 114 | 191 / 109 | - / - |
| failed, receiver had it | 45 / 57 | 156 / 55 | 160 / 77 | - / - |
| returns taken from a child | 1 / 37 | 29 / 47 | 49 / 102 | - / - |
| texts lost: TTL | 0 / 0 | 3 / 0 | 3 / 0 | 0 / 0 |
| texts lost: no route | 0 / 0 | 0 / 0 | 0 / 0 | 12 / 12 |
| texts lost: return failed | 0 / 0 | 4 / 7 | 3 / 3 | 2 / 3 |

### What it shows

- **No storm, and no gain in delivery.** Every dense-city run is within
  0.3 points of main in the settled hour: 97.8 / 98.1 / 98.1 → 97.8 /
  98.2 / 98.1 % at the design rate, 88.8 / 89.1 → 88.5 / 88.9 % at twice
  the rate, the two slightly under. S4 is level at both rates and the
  walkers too. The spread between seeds is about ±0.3 points on the
  city and ±2 on S4 (roadmap D96), so none of these is a result.
- **It costs less, where the channel is not overloaded.** At the
  design rate airtime 0 to 2.6 % less, hand-overs failed with the
  receiver holding the message 6 to 16 % fewer, hand-overs held back by
  the bucket less than half, and relays ever at their cap a fifth to
  two fifths fewer; the walkers the same, 16 % fewer of those failures
  and half the relays at their cap. These are the offers to a neighbour
  that was silent for the message twice that a copy coming back
  through the relay no longer makes, as in the first form's S4 run.
  Fewer copies merge, 6 to 18 % on the city: there are fewer split
  copies to merge, not fewer heals, since a split's receiver that
  answers anyone is offered the copy again.
- **The losses move; they do not shrink.** On the city the texts lost
  at the time to live halve, 78 / 90 / 80 → 41 / 36 / 34, the ones the
  trap held; the texts lost at the hop cap, as failed returns and as no
  route rise by as many or more, 268 / 274 / 229 → 312 / 353 / 311. A
  message that goes back rather than waiting spends hops on the way
  round, and its parent is as often faint too. At twice the rate the
  same, with no fall at the time to live, which is overload there, not
  the trap.
- **Under the bucket it is slower.** S4 at three times the rate,
  latency p50 83 → 95 s, airtime 3 % more, 231 more texts at the time
  to live and 8 361 more texts that waited at their origin: silence there is
  the bucket, a neighbour that will answer once it has refilled, and a
  message that goes back instead of waiting for it is carried the long
  way.
- **The mesh gains the trapped texts.** At seed 5 main held three
  texts in each run until the time to live; the variant holds none. The
  burst delivers one more and fails three more returns, the spread run
  delivers four more, 83 → 87 of 90. The burst at seed 1 delivers its last text. The ring,
  92.6 → 94.3 %, is within what its order of hand-overs moves between
  builds (roadmap D102).
- **What this does not show.** The first form was measured on
  `09b7d73`, before D93; this variant on `8ff3a74`, after it. The base
  with D93 no longer storms on any of these seeds, so whether the lift
  is what keeps the storm away, or D93's fewer splits would have kept
  the first form from storming too, is not measured.

### After D102

Main now has D96 and D102, and D102 changes the step this variant
hangs on. A message whose hand-over fails holds on to the neighbour,
demoted or not, waits for the ban its failure gave and is offered to it
again, and moves on only after three failures in a row, the
neighbour's or its own (spec "Hand-over" step 6). The variant set the
mark at a failure that found the neighbour demoted, which is the
second; on main that failure keeps the message with the neighbour, so
the mark as built would contradict the hold. Ported, it would be set
when the message moves on.

What it would then change on main is one thing, and D102 does not
touch it. A message that has moved on and has no other candidate left
waits for the neighbour's ban and is offered to it again, one hand-over
a time, the neighbour's count past three moving it on at once, then
waits for the next, doubled ban, up to 61 s, until its time to live
(spec "Overload waits": a banned candidate is waited for). This is the
trap above. The D102 note names it as the tail of its holds, p90
181.8 s and a longest of 597 s on S4 at three times the rate, the same
at main's count of two, and leaves it as it was (`hold-2026-11-02.md`,
"What it revealed"). A mark set at the move-on, counting while the
neighbour stays demoted, would end that wait: the message would go
back to its parent, or end at the origin, once its hold is over. That
is what "a reason not to wait" still means after D102, and all it
means.

The other effect the variant had here, the offers a returning copy no
longer made to a neighbour silent for it, is mostly D102's already: a
message now spends its three offers on the neighbour in one hold, and
the split it leaves behind fell by two thirds (roadmap D102). What
remains is the offer after the hold, which the mark would stop while
the neighbour is demoted and main makes as a last resort.

### Recommendation

Not adopted. On its own base it is level with main on every run that
carries texts, a little cheaper on the city at the design rate and on
the walkers, a little slower under the bucket, and it moves losses from
the time to live to the hop cap and to failed returns rather than
removing them. It does not bring the storm back, but the base no longer
storms anyway. The code stays on its branch.

A re-measurement on main `f94a7cd` would add one thing D102 left open:
whether ending the trap's tail, the message that waits for a demoted
last candidate's ban ladder after its hold, gains anything. It would
need the port above, a different rule from the one measured here, and
should be judged on S4 at three times the rate, seeds 1 and 2, where
D102 measured the tail, and on `mesh-burst.toml` and `mesh-spread.toml`
at seed 5, where the trapped texts are. The measurement here says what
to expect: the trapped texts in the mesh delivered, and on the loaded
runs the losses moved, not removed. It is not worth a run unless the
tail is wanted gone for its own sake: a relay freed sooner, and a text
that ends in seconds rather than minutes. How a text ends is not told
to the sender's application (roadmap D10: no failure notices), so a
user sees only the texts delivered, a few in the mesh.

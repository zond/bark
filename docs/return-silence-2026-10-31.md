# A neighbour silent past its demotion is tried: the search goes back

**Date:** 2026-10-31. **Status:** built and measured; roadmap D95. Spec
references are to `protocol.md`. Measured against main `09b7d73`, with
D90 in, release builds of both, the same scenario files and seeds.

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
  93.7 %. Most of the gain under load is not the returns the rule
  counts, 60 in that run, but the offers it stops: a search that comes
  back through a relay no longer tries a demoted neighbour again.
- **What it costs.** On the loaded ring three settled texts end as no
  route where main delivered them late: silence there is congestion,
  and the message gives the link up after two hand-overs where main
  waited. Returns that fail rise where the parent is faint too (the
  mesh at seed 5: 1 → 8, every one unreachable; S4 ×3: 2 → 11 texts). On
  a line whose only way on is a faint link the rule gives the message
  two hand-overs, six frames, where main gave it up to 33: a link that
  carries one frame in ten gets the text through 47 % of the time
  instead of 97 %.
- **Recommendation: adopt.** See the end.

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

| | s4x1 | s4x3-s1 | ring | movers |
|---|---|---|---|---|
| texts sent in the settled hour | 17459 / 17457 | 47738 / 48055 | 2919 / 2856 | 17894 / 17892 |
| delivered, settled hour % | 93.4 / 93.7 | 51.3 / 55.2 | 91.4 / 91.8 | 96.4 / 96.5 |
| delivered, whole run % | 93.4 / 93.7 | 58.5 / 62.3 | 91.7 / 91.5 | 95.5 / 95.6 |
| latency p50 s | 1.4 / 1.4 | 158.5 / 134.8 | 58.5 / 53.5 | 5.5 / 5.4 |
| latency p90 s | 187.9 / 189.1 | 693.2 / 662.6 | 145.8 / 158.6 | 57.3 / 50.5 |
| airtime, whole run s | 44213 / 44142 | 169524 / 163152 | 1817 / 1792 | 184940 / 187599 |
| hand-overs taken | 116275 / 116452 | 391541 / 392218 | - / - | 322312 / 328185 |
| hand-overs failed | 19377 / 19128 | 184900 / 171766 | - / - | 287084 / 271150 |
| failed, receiver had it | 4009 / 3989 | 44738 / 41126 | - / - | 154968 / 137520 |
| returns taken from a child | 75 / 106 | 1020 / 955 | - / - | 3340 / 3871 |
| copies answered seen | 7860 / 7061 | 54562 / 50968 | - / - | 210500 / 218474 |
| seen ACKs sent : acted on | 8470 : 7764 / 7599 : 6973 | 60927 : 53031 / 56618 : 49630 | 0 : 0 / 0 : 0 | 240184 : 200376 / 247263 : 208589 |
| ended past a silent neighbour: returned | 0 / 9 | 0 / 59 | 0 / 29 | 0 / 2136 |
| ended past a silent neighbour: dropped at the origin | 0 / 0 | 0 / 1 | 0 / 1 | 0 / 2 |
| hand-overs held back by the bucket | 64 / 44 | 1007236 / 911330 | - / - | 9030 / 5380 |
| neighbours demoted | 7164 / 7240 | 53889 / 49969 | - / - | 110218 / 117691 |
| copies dropped: hop cap | 37 / 32 | 26 / 34 | 0 / 0 | 1550 / 1555 |
| copies dropped: TTL | 340 / 342 | 7174 / 6238 | 0 / 0 | 970 / 225 |
| copies dropped: no route | 265 / 242 | 1015 / 1058 | 26 / 40 | 114 / 111 |
| copies dropped: return failed | 0 / 5 | 3 / 22 | 3 / 5 | 36 / 494 |
| copies dropped: return seen | 0 / 0 | 0 / 1 | 0 / 0 | 3 / 20 |
| texts lost: hop cap | 30 / 26 | 16 / 24 | 0 / 0 | 161 / 212 |
| texts lost: TTL | 322 / 318 | 6603 / 5858 | 0 / 0 | 114 / 59 |
| texts lost: no route | 13 / 10 | 95 / 92 | 26 / 36 | 7 / 6 |
| texts lost: return failed | 0 / 1 | 2 / 11 | 2 / 1 | 1 / 21 |
| texts lost: return seen | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 |
| texts that waited at their origin | 180 / 191 | 242780 / 222222 | 2474 / 2150 | 129 / 84 |
| relays ever at their cap | 0 / 0 | 188 / 175 | - / - | 171 / 115 |
| largest settled airtime of a node % | 5.65 / 5.33 | 9.98 / 9.97 | 9.98 / 9.84 | 5.11 / 5.08 |
| heartbeats received % | 94.4 / 94.4 | 87.7 / 88.1 | 90.6 / 87.4 | 89.4 / 89.4 |

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

- **Under load the gain is the offers the rule stops, not the returns
  it counts.** S4 at three times the rate, seed 1: 51.3 → 55.2 % in the
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
  D90 note. The sign is the same in every run here, and the mechanism
  that gives it, fewer offers to demoted neighbours, is counted; the
  size is not settled.
- **A line with a faint link that sometimes works.** The table above is
  arithmetic; no scenario here has such a link as the only way on.
- **The board.** Nothing here was run on hardware.

## Recommendation

Adopt. The rule takes out an unbounded wait, up to the time to live, on
a link that silence has shown twice not to carry the message, and keeps
what roadmap D42 measured: the wait for a refusal's pushback, and the
one wait for the first silence's ban. Measured, it loses no delivery on
any run and gains under load, where the search comes back through its
relays most. Its costs are the line whose only way on is a faint link
that sometimes works, where a message gets six frames instead of
thirty-three, three texts on the loaded ring, and a shift of the
walkers' losses from the time to live to failed returns and the hop
cap; each is bounded, and the first is arithmetic to revisit when Phase
2 has measured what a faint link carries.

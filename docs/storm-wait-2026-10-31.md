# A wait is never idle

**Date:** 2026-10-31; the runs finished the same day and the write-up
was finished 2026-11-02. **Status:** measured; roadmap D97, **not to be
adopted alone**: on the two dense-power seeds it starts the storm the
base does not have. Spec references are to `protocol.md`. Part 4 of the
storm candidate (`storm-2026-10-30.md`), built and measured alone on
main `09b7d73`, on which D90 is adopted; parts 2 and 3 are measured by
others on the same base at the same time, so what is measured here is
this rule alone. The branch's spec section "Waiting hand-overs"
describes the rule as built on the branch; main's spec is untouched
unless the branch is merged.

## Summary

- **The rule.** A hand-over that waits between its attempts, for its
  backoff, its retry wait, the bucket or an owed reply, keeps its place
  in the send queue and its own clock, and the radio serves the next
  packet meanwhile. One hand-over is on the radio at a time; at most
  four are under way. A packet to a channel that a waiting hand-over
  holds, for its backoff or its retry, waits for it. A delivery goes
  first, whatever waits.
- **What today's loop did.** A ban never held the queue: a packet
  whose candidates are all banned waits in its place, due when the
  earliest ban ends, and the next packet is served. Every other wait of
  a hand-over, the backoff after a busy check, the retry wait after an
  unanswered frame, the wait for the bucket and the hold for an owed
  reply, held the whole queue: the one hand-over the node ran was the
  current place, and nothing else was addressed until it ended.
- **Measured.** It starts the storm. `dense-power.toml` seed 1, which
  main `09b7d73` does not storm on (97.8 % of the settled hour's texts,
  airtime 299 754 s), falls to 87.4 % at 1 424 595 s, latency p90 46 →
  339 s, hand-overs failed 525 536 → 3 479 372, of them with the
  receiver holding the message 125 885 → 1 017 560 (24 → 29 % of the
  failures), relays ever at their cap 108 → 2 592; seed 7 the same, 98.0
  → 89.2 %, 305 040 → 1 162 170 s. Where the bucket binds already it is
  neutral: S4 at three times the rate 51.3 / 52.1 / 48.8 → 50.8 / 53.1 /
  47.1 %, p50 158 / 162 / 197 → 139 / 146 / 194 s, airtime −2 to +1 %,
  the split rate within 2 %; S4 at the design rate 93.4 → 93.4 %; the
  walkers 96.4 → 96.5 %, p90 57 → 54 s, splits +3 %. The ring gains, 91.4
  → 93.4 %, p50 58.5 → 49.9 s. The mesh burst at seed 1 loses three
  texts of a hundred with twice the splits (40 → 83); at seed 5 it gains
  one; the spread runs are main's to the byte.
- **Why.** The idle time between a hand-over's attempts was a brake. A
  relay that serves the next packet in its retry wait puts more frames
  on the air per second at the same offered load, and the splits that
  the storm note found starting the storm breed on frames: at seed 1
  DATA lost while the receiver was receiving another frame rose from
  9.4 to 16.9 % of the frames addressed to relays and lost while it was
  deaf from 3.3 to 9.1 %, ACKs received fell from 82.6 to 78.2 %, and
  the chain reaction that the base sat just below went off. The mesh
  burst showed the same in small: the cost is collisions from denser
  transmission, not the late ACKs the storm note named as the risk.
- **Recommendation: not adopted alone.** The rule recovers queue time
  only where a wait's cause is not the relay's own frames, the ring and
  the lightly loaded runs; under load it pays the recovered time in
  splits. It can pay only once a split cannot be made by the rules: part
  2, a take final only when its ACK has gone out, with the ACK sent
  without listen-before-talk, so that a receiver never holds a message
  its sender was not told of; and part 3, an attempt is a transmission
  and a busy channel never moves a message to another neighbour, so
  that a retry goes to the receiver that may hold it. With those in,
  the frames a never-idle radio adds are hand-overs that complete, not
  copies, and this rule is to be measured again on top of them.

## The question

The storm note found the relays at their cap spending 15.1 % of the
hour between a hand-over's attempts, in retry waits and backoffs, with
the radio idle while the send queue held hop places, returns and
deliveries for other channels. Part 4 of its candidate: whenever a
hand-over waits, the radio serves the next packet in the send queue
whose neighbour is on another channel, and the waiting hand-over keeps
its state and resumes when its wait ends. Two risks were named to
measure: a late ACK missed while the sender serves another channel,
which the retry wait catches today; and the tail latency of a packet
to a channel that stays busy, which no longer blocks the queue but may
wait behind it.

## What the loop did

The scheduler (`main-loop-2026-10-30.md`) ran one hand-over: the
send queue's current place, with `Outgoing::state` in it and one
`Flight` beside it, until when it waits and since when its checks found
the channel busy. `prepare` addressed the next due place only while no
place was in flight, so each `Wait` kind held the queue or did not:

| the wait | where | held the queue |
|---|---|---|
| a ban: every candidate banned, or the own message's hold | `Routed::Wait`: `not_before` set, the place stays `Queued` | no: `next_due` skips it and serves the next place |
| a ban after a failed hand-over, a refusal of a return or a delivery | `handover_failed`, `refused`: `not_before` set, `Queued` | no |
| `Wait::Turn`: its turn, once the radio is free | `Waiting` | yes, but only until the radio is free |
| `Wait::Backoff`: 0–50 ms after a busy check, for up to 500 ms | `Waiting` | yes |
| `Wait::Retry`: 0–311 ms, then 0–622 ms, after an unanswered frame | `Waiting` | yes |
| `Wait::Budget`: until the bucket fits the frame, up to 61 s | `Waiting` | yes, except that a delivery due made it step aside |
| `Wait::Reply`: an owed ACK goes first | `Waiting` | yes, for the reply's 100 ms at most |
| listening for its ACK, 100 ms | `AwaitAck` | yes, and must: the ACK comes home |

So the rule changes nothing for the bans, which were never idle, and
frees the queue from the four waits of the place itself. The ACK wait
stays exclusive: the radio listens at home for it, and another
hand-over's check or frame would take it away.

## The rule

Spec "Waiting hand-overs", in full there; the mechanism:

- **Flights.** The send queue keeps a flight for each hand-over under
  way (`tables/send_queue.rs` `Flight`): the place it belongs to, until
  when it waits or its ACK is due, since when its checks found the
  channel busy, and the neighbours its forward was routed past. A place
  has no room for them, 280 bytes being full; four flights are 160
  bytes. `HandOver` (`node/transfer.rs`) drives a place and its flight
  through the queue, begins the flight when the hand-over starts and
  ends it when the hand-over is answered, fails or is given up, so that
  a place is queued again with no flight.
- **The bound.** `FLIGHTS` is 4: the longest retry wait, 2 × 2 × 155.5
  = 622 ms, over the least a hand-over that transmits holds the radio,
  a full frame and the ACK timeout, 138.6 + 100 = 238.6 ms, is 2.6, so
  three hand-overs can be served during one retry wait, and with the
  waiter four. With four under way the next packet waits for one to
  end, as every packet did before. `IN_FLIGHT` stays 1: one hand-over
  on the radio, checking, transmitting or listening for its ACK.
- **The scheduler.** When the radio is free, no ACK is awaited, every
  hand-over under way waits and a flight is free, `prepare` addresses
  the next due place of the queue, in the queue's order, and starts it:
  one at a time, so that a forward is addressed as its neighbours are
  when it goes. `start_handover` serves the first hand-over whose wait
  has ended, a delivery first, else the one that began first, and
  defers past `now` those the bucket cannot fit, trying the next.
- **The channel hold.** A place whose route leads to a channel that a
  hand-over under way waits on, for its backoff or its retry, is not
  started: it is set `Waiting(Wait::Channel)`, with no flight, and is
  queued and addressed again once no hand-over waits on that channel.
  A delivery is started whatever is held.
- **A delivery goes first.** With no flight free, a hand-over that
  waits between its attempts steps aside for a delivery due and keeps
  its place, as before; with one free the delivery is simply next.
- **The counter.** The simulator counts the frames a hand-over sent
  while another hand-over under way waited between its attempts
  (`Trace::ServedWhileWaiting`, `served_while_waiting`).

*Why one at a time.* Started while others were under way, a forward
would be addressed up to a retry wait before it goes, and a ban or a
demotion learned meanwhile would not move it: the routing test of
messages waiting for a failure ban found exactly that, a forward sent
into a ban it was addressed before. Addressed when the radio can serve
it, a forward goes to its neighbours as they are.

*Why the channel is held.* A backoff says the channel was busy; a
second hand-over checking it before the backoff ends would only check
sooner than the backoff said, and the node is deaf at home for each
check. A retry wait leaves room for the sender this one collided with
at the receiver to go first; a frame of this relay's on that channel
meanwhile would be in its way. The storm note's wording, "whose
neighbour is on another channel", is this rule.

*What bounds it.* One hand-over on the radio, `IN_FLIGHT`; four under
way, `FLIGHTS`; the bucket, which every hand-over must fit before its
check, so that a packet the bucket cannot fit waits and a smaller one,
or a delivery, goes; and the reply reserve, which forwarding leaves in
the bucket as before.

*What ordering the queue keeps.* The queue's order is unchanged: a
delivery first, then of the places that waited for a ban the most
travelled, then arrival order; the hop places are places like any
other. A hand-over whose wait has ended goes before a new place is
addressed: a delivery first, else the one that began first.

**The structure alone.** The flights, `HandOver` driving the queue and
the scheduler over flights, with `FLIGHTS` set to one, reproduce main
`09b7d73` to the byte: the messages CSV and the summary without its
wall-time line on the ring, the four mesh runs and S4 at the design
rate. The rule is the bound raised to four with the channel hold.

**Unit tests** (`relay/tests.rs`): a place waiting for a ban lets the
places behind it go out; a hand-over in its retry wait keeps its own
clock while the next place, on another channel, is served, and a place
to its channel waits for it; and over six forwards on two channels at
most one hand-over is on the radio and at most four under way after
every event, with two under way at some point.

## How it was measured

Every run is the scenario file at the seed named, with `--json` and
`--messages`, release builds: main `09b7d73` and the branch at its D97
commit. The figures are the JSON summary's: delivered of the texts sent
in the settled hour and of all texts, latency, the run's airtime, the
cores' counters summed over the nodes, drops and texts lost by reason,
the relays whose load sample ever found the bucket under its reserve,
and the largest settled airtime of a node; and the fates of the frames
addressed to nodes that are not sleepy, received, lost while the
receiver listened and was free, while it received another frame, while
it was deaf, or on another channel. The scenarios: `dense-power.toml`
seeds 1 and 7, `s4-capacity.toml` seed 1 at the design rate and at
three times it (`per_leaf_per_hour` 2.875) seeds 1, 2 and 3, `ring.toml`,
`dense-movers.toml`, `mesh-burst.toml` and `mesh-spread.toml` seeds 1
and 5. Main's runs of the ring, the mesh and S4 at the design rate are
this note's own; its runs of the dense-power seeds, the walkers and S4
at three times the rate were made on the same day by another measurement
on the same base, whose binary reproduces this note's main to the byte
on the ring and on S4 at the design rate.

## The tables

Main `09b7d73` / D97. "Copies answered" are those from no neighbour
the node had handed the message to, cases 4 to 6; "returns" those from
one it had. A text is "lost" to a reason when its last copy ended with
it. "Frames sent while another hand-over waited" is the branch's new
count: a DATA that went to the radio while another hand-over under way
waited for its backoff, its retry or the bucket.

| | dp1 | dp7 | s4x1 | s4x3-s1 | s4x3-s2 | s4x3-s3 | ring | movers |
|---|---|---|---|---|---|---|---|---|
| texts sent in the settled hour | 18234 / 18176 | 18189 / 18144 | 17459 / 17457 | 47738 / 47645 | 47423 / 47471 | 47770 / 47458 | 2919 / 2885 | 17894 / 17893 |
| delivered, settled hour % | 97.8 / 87.4 | 98.0 / 89.2 | 93.4 / 93.4 | 51.3 / 50.8 | 52.1 / 53.1 | 48.8 / 47.1 | 91.4 / 93.4 | 96.4 / 96.5 |
| delivered, whole run % | 96.5 / 89.5 | 96.9 / 90.8 | 93.4 / 93.5 | 58.5 / 58.7 | 59.2 / 60.6 | 56.0 / 54.6 | 91.7 / 93.9 | 95.5 / 95.7 |
| latency p50 s | 10.4 / 26.5 | 10.5 / 20.0 | 1.4 / 1.3 | 158.5 / 138.5 | 161.8 / 145.9 | 197.4 / 194.4 | 58.5 / 49.9 | 5.5 / 5.3 |
| latency p90 s | 46.0 / 339.3 | 45.1 / 308.7 | 187.9 / 186.4 | 693.2 / 684.6 | 688.1 / 681.9 | 750.2 / 745.7 | 145.8 / 152.6 | 57.3 / 54.4 |
| airtime, whole run s | 299754 / 1424595 | 305040 / 1162170 | 44213 / 44464 | 169524 / 166610 | 167990 / 164997 | 187758 / 188947 | 1817 / 1812 | 184940 / 187940 |
| hand-overs taken | 438601 / 2201506 | 449952 / 1781471 | 116275 / 116740 | 391541 / 379862 | 383271 / 374644 | 393290 / 378052 | - / - | 322312 / 327141 |
| hand-overs failed | 525536 / 3479372 | 544701 / 2818897 | 19377 / 19624 | 184900 / 186340 | 176942 / 176636 | 220940 / 230974 | - / - | 287084 / 293592 |
| failed, receiver had it | 125885 / 1017560 | 128604 / 806913 | 4009 / 4346 | 44738 / 43835 | 43576 / 43634 | 58825 / 59745 | - / - | 154968 / 160186 |
| returns taken from a child | 3631 / 34319 | 3812 / 26531 | 75 / 99 | 1020 / 1003 | 1006 / 1147 | 1274 / 1378 | - / - | 3340 / 3552 |
| copies answered (not returns) | 270245 / 2146499 | 272955 / 1675901 | 7888 / 8947 | 57007 / 49384 | 71749 / 63410 | 71700 / 66415 | - / - | 212062 / 221679 |
| of those merged (case 4) | 2602 / 105995 | 2671 / 80931 | 28 / 30 | 2445 / 2319 | 2344 / 2408 | 3711 / 3670 | - / - | 1532 / 1618 |
| of those seen (case 6) | 267568 / 2040313 | 270206 / 1594790 | 7860 / 8917 | 54562 / 47065 | 69405 / 61002 | 67987 / 62743 | - / - | 210500 / 220023 |
| seen ACKs sent / acted on | 302116 / 255524 / 2334731 / 1856815 | 305720 / 257736 / 1821827 / 1455267 | 8470 / 7764 / 9674 / 8796 | 60927 / 53031 / 52557 / 45438 | 76821 / 67753 / 67533 / 59161 | 76340 / 65702 / 70485 / 59934 | 0 / 0 / 1 / 1 | 240184 / 200376 / 250992 / 209642 |
| hand-overs held back by the bucket | 24678 / 12335671 | 20288 / 9520286 | 64 / 58 | 1007236 / 3530214 | 984141 / 3324702 | 1165803 / 4280735 | - / - | 9030 / 18250 |
| frames sent while another hand-over waited | 0 / 4371263 | 0 / 3345493 | 0 / 4580 | 0 / 581963 | 0 / 546048 | 0 / 721023 | - / - | 0 / 42770 |
| neighbours demoted | 217639 / 1349311 | 226798 / 1103308 | 7164 / 7251 | 53889 / 53520 | 50871 / 50036 | 65648 / 67417 | - / - | 110218 / 112347 |
| copies dropped: hop cap | 1488 / 2464 | 1584 / 2220 | 37 / 44 | 26 / 22 | 59 / 53 | 51 / 40 | 0 / 0 | 1550 / 1662 |
| copies dropped: TTL | 792 / 98621 | 741 / 72042 | 340 / 338 | 7174 / 7080 | 6533 / 6283 | 8261 / 8606 | 0 / 0 | 970 / 885 |
| copies dropped: return seen | 0 / 2 | 1 / 1 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 3 / 1 |
| copies dropped: no route | 141 / 449 | 155 / 352 | 265 / 248 | 1015 / 1068 | 1058 / 1188 | 1233 / 1280 | 26 / 18 | 114 / 119 |
| copies dropped: return failed | 49 / 111 | 48 / 116 | 0 / 0 | 3 / 3 | 13 / 10 | 9 / 5 | 3 / 1 | 36 / 37 |
| texts lost: hop cap | 237 / 233 | 219 / 220 | 30 / 33 | 16 / 12 | 31 / 24 | 29 / 27 | 0 / 0 | 161 / 164 |
| texts lost: TTL | 98 / 1054 | 83 / 781 | 322 / 319 | 6603 / 6543 | 5965 / 5705 | 7431 / 7665 | 0 / 0 | 114 / 108 |
| texts lost: return seen | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 |
| texts lost: no route | 6 / 20 | 8 / 30 | 13 / 15 | 95 / 101 | 98 / 126 | 130 / 154 | 26 / 14 | 7 / 2 |
| texts lost: return failed | 5 / 11 | 5 / 18 | 0 / 0 | 2 / 2 | 8 / 8 | 4 / 2 | 2 / 1 | 1 / 1 |
| texts that waited at their origin | 50 / 4992 | 65 / 3278 | 180 / 183 | 242780 / 231987 | 247222 / 230737 | 277036 / 279564 | 2474 / 2400 | 129 / 145 |
| relays ever at their cap | 108 / 2592 | 106 / 2098 | 0 / 0 | 188 / 181 | 175 / 173 | 220 / 223 | - / - | 171 / 208 |
| largest settled airtime of a node % | 4.94 / 9.08 | 4.74 / 8.68 | 5.65 / 5.38 | 9.98 / 9.98 | 9.95 / 9.96 | 9.99 / 9.98 | 9.98 / 10.09 | 5.11 / 5.26 |
| heartbeats received % | 84.9 / 66.8 | 84.7 / 70.5 | 94.4 / 94.3 | 87.7 / 87.7 | 88.1 / 88.3 | 86.2 / 86.0 | 90.6 / 90.5 | 89.4 / 89.4 |

| | burst-s1 | burst-s5 | spread-s1 | spread-s5 |
|---|---|---|---|---|
| texts sent in the settled hour | 100 / 100 | 100 / 100 | 90 / 90 | 90 / 90 |
| delivered, settled hour % | 100.0 / 97.0 | 92.0 / 93.0 | 100.0 / 100.0 | 92.2 / 92.2 |
| delivered, whole run % | 100.0 / 97.0 | 92.0 / 93.0 | 100.0 / 100.0 | 92.2 / 92.2 |
| latency p50 s | 3.7 / 3.3 | 1.9 / 2.3 | 0.6 / 0.6 | 0.6 / 0.6 |
| latency p90 s | 6.7 / 9.8 | 4.9 / 4.7 | 0.8 / 0.8 | 0.8 / 0.8 |
| airtime, whole run s | 232 / 269 | 263 / 273 | 183 / 183 | 228 / 228 |
| hand-overs taken | 629 / 701 | 565 / 602 | 469 / 469 | 504 / 504 |
| hand-overs failed | 78 / 166 | 165 / 202 | 0 / 0 | 134 / 134 |
| failed, receiver had it | 40 / 83 | 113 / 151 | 0 / 0 | 101 / 101 |
| returns taken from a child | 2 / 17 | 22 / 40 | 0 / 0 | 25 / 25 |
| copies answered (not returns) | 168 / 268 | 359 / 364 | 72 / 72 | 321 / 321 |
| of those merged (case 4) | 0 / 2 | 3 / 4 | 0 / 0 | 2 / 2 |
| of those seen (case 6) | 168 / 266 | 356 / 360 | 72 / 72 | 319 / 319 |
| seen ACKs sent / acted on | 173 / 166 / 282 / 264 | 373 / 349 / 377 / 356 | 72 / 72 / 72 / 72 | 323 / 318 / 323 / 318 |
| hand-overs held back by the bucket | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 |
| frames sent while another hand-over waited | 0 / 189 | 0 / 124 | 0 / 0 | 0 / 0 |
| neighbours demoted | 15 / 30 | 29 / 33 | 0 / 0 | 22 / 22 |
| copies dropped: hop cap | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 |
| copies dropped: TTL | 1 / 2 | 6 / 3 | 0 / 0 | 3 / 3 |
| copies dropped: return seen | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 |
| copies dropped: no route | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 |
| copies dropped: return failed | 0 / 1 | 2 / 3 | 0 / 0 | 2 / 2 |
| texts lost: hop cap | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 |
| texts lost: TTL | 0 / 1 | 6 / 3 | 0 / 0 | 3 / 3 |
| texts lost: return seen | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 |
| texts lost: no route | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 |
| texts lost: return failed | 0 / 1 | 1 / 2 | 0 / 0 | 2 / 2 |
| texts that waited at their origin | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 |
| relays ever at their cap | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 |
| largest settled airtime of a node % | 0.52 / 0.64 | 0.90 / 0.81 | 0.41 / 0.41 | 0.73 / 0.73 |
| heartbeats received % | 99.6 / 99.4 | 99.0 / 99.1 | 99.6 / 99.6 | 99.3 / 99.3 |

**The fates of the frames addressed to nodes that are not sleepy**, by
what the receiver was doing when the frame started, and of the DATA
received how they were answered (an ACK that takes, one that refuses,
one started after the sender's timeout, none):

## dp1
| | received | lost free | lost receiving | lost deaf | other channel | DATA answered: taken / refused / late / none |
|---|---|---|---|---|---|---|
| main DATA | 300583 (43.9 %) | 288104 (42.1 %) | 64162 (9.4 %) | 22937 (3.3 %) | 8913 (1.3 %) | 293628 / 0 / 1280 / 0 |
| main ACK | 243590 (82.6 %) | 41751 (14.2 %) | 4002 (1.4 %) | 5561 (1.9 %) | 4 (0.0 %) |  |
| D97 DATA | 1625769 (39.8 %) | 1373702 (33.6 %) | 691707 (16.9 %) | 371147 (9.1 %) | 26528 (0.6 %) | 1438268 / 3454 / 26286 / 0 |
| D97 ACK | 1148669 (78.2 %) | 231417 (15.8 %) | 57277 (3.9 %) | 30616 (2.1 %) | 15 (0.0 %) |  |
## dp7
| | received | lost free | lost receiving | lost deaf | other channel | DATA answered: taken / refused / late / none |
|---|---|---|---|---|---|---|
| main DATA | 308505 (43.7 %) | 298928 (42.3 %) | 66314 (9.4 %) | 23680 (3.4 %) | 8547 (1.2 %) | 301160 / 0 / 1333 / 0 |
| main ACK | 249116 (82.4 %) | 43346 (14.3 %) | 4234 (1.4 %) | 5792 (1.9 %) | 4 (0.0 %) |  |
| D97 DATA | 1309829 (39.5 %) | 1141357 (34.4 %) | 552848 (16.7 %) | 287818 (8.7 %) | 24583 (0.7 %) | 1165096 / 3439 / 20553 / 0 |
| D97 ACK | 933003 (78.5 %) | 188284 (15.8 %) | 43650 (3.7 %) | 24133 (2.0 %) | 11 (0.0 %) |  |
## s4x3-s1
| | received | lost free | lost receiving | lost deaf | other channel | DATA answered: taken / refused / late / none |
|---|---|---|---|---|---|---|
| main DATA | 583860 (55.8 %) | 194490 (18.6 %) | 155787 (14.9 %) | 106892 (10.2 %) | 4740 (0.5 %) | 458325 / 79724 / 8850 / 0 |
| main ACK | 471131 (86.1 %) | 49325 (9.0 %) | 15689 (2.9 %) | 10741 (2.0 %) | 11 (0.0 %) |  |
| D97 DATA | 569890 (55.1 %) | 197509 (19.1 %) | 155990 (15.1 %) | 106131 (10.3 %) | 4893 (0.5 %) | 440937 / 85775 / 8094 / 0 |
| D97 ACK | 456650 (85.4 %) | 49969 (9.3 %) | 15670 (2.9 %) | 12510 (2.3 %) | 5 (0.0 %) |  |
## s4x3-s2
| | received | lost free | lost receiving | lost deaf | other channel | DATA answered: taken / refused / late / none |
|---|---|---|---|---|---|---|
| main DATA | 577942 (56.4 %) | 192109 (18.8 %) | 147265 (14.4 %) | 103138 (10.1 %) | 3666 (0.4 %) | 463058 / 73475 / 7823 / 0 |
| main ACK | 471057 (86.5 %) | 47710 (8.8 %) | 14800 (2.7 %) | 10786 (2.0 %) | 1 (0.0 %) |  |
| D97 DATA | 561302 (55.7 %) | 195063 (19.4 %) | 145551 (14.4 %) | 101453 (10.1 %) | 4413 (0.4 %) | 445878 / 77140 / 7239 / 0 |
| D97 ACK | 455453 (85.9 %) | 48266 (9.1 %) | 14249 (2.7 %) | 12279 (2.3 %) | 9 (0.0 %) |  |
## s4x3-s3
| | received | lost free | lost receiving | lost deaf | other channel | DATA answered: taken / refused / late / none |
|---|---|---|---|---|---|---|
| main DATA | 625803 (53.6 %) | 229073 (19.6 %) | 184980 (15.8 %) | 122224 (10.5 %) | 6242 (0.5 %) | 482359 / 84930 / 10783 / 0 |
| main ACK | 489757 (84.7 %) | 57232 (9.9 %) | 19952 (3.5 %) | 11124 (1.9 %) | 6 (0.0 %) |  |
| D97 DATA | 613888 (52.2 %) | 239159 (20.4 %) | 191984 (16.3 %) | 123615 (10.5 %) | 6512 (0.6 %) | 463880 / 93177 / 10556 / 0 |
| D97 ACK | 476704 (84.0 %) | 58171 (10.2 %) | 20103 (3.5 %) | 12622 (2.2 %) | 11 (0.0 %) |  |

## What it shows

- **The storm is reachable from this base by this rule alone.** The
  storm note found seed 1 on the edge: main at the old costs stormed by
  seed alone and at the measured costs by cost alone, and this base,
  three decisions on, sits below the edge on both seeds with the splits
  intact (below). The rule moves it over: on both seeds the airtime
  rises four to five times, the hand-overs that failed with the
  receiver holding the message eight times, the copies that expired at
  the time to live a hundred times (792 → 98 621), the relays ever at
  their cap twenty times, and delivery falls ten points with p90 seven
  times longer. The signature is the storm note's to the figure, now
  with 4.37 M frames sent while another hand-over waited, a fifth of the
  run's DATA.
- **At the cap the bucket binds, not the radio.** S4 at three times the
  rate has 175 to 220 relays at their cap on main, and the rule changes
  its delivery by ±2 points, its airtime by −2 to +1 %, its splits by
  −2 to +2 %, and its latency p50 by −20 s on two seeds and −3 on the
  third: the queue time it recovers goes to texts the bucket lets
  through, and the hand-overs held back by the bucket rise three and a
  half times, 1.0 M → 3.5 M, each a flight that found the bucket short
  and waited in its slot. At the design rate it is neutral in every
  figure, with 4 580 frames sent while another hand-over waited in the
  run.
- **Below load it gains.** The ring, where a wait is the channel's and
  not the relay's own frames, delivers two points more and its median
  text 9 s sooner; the walkers gain a tenth of a point and 3 s of p90
  for 3 % more splits; the mesh spread runs, one text at a time, are
  untouched.
- **Where load is a burst, the splits double.** The mesh burst at seed 1
  sends a hundred texts at once over 25 relays, and the rule doubles its
  failed hand-overs and its splits, and loses three texts; seed 5, where
  the texts spread over more relays, gains one. The frame fates say
  how: more DATA lost while the receiver listened and was free, more
  ACKs lost while the sender received another frame.
- **The risk the storm note named is not the cost.** ACKs started after
  the sender's timeout are 1 280 → 26 286 on seed 1, but that is the
  storm's own retries, 0.4 → 1.6 % of the DATA received; in the S4 runs
  they fall. What rises everywhere the rule costs is frames lost to
  other frames.

## What it revealed

**The base does not storm.** Main `09b7d73` delivers 97.8 % of the
settled hour's texts on `dense-power.toml` seed 1 with 299 754 s of
airtime, 525 536 hand-overs failed and 125 885 of them with the
receiver holding the message, where the D90 branch at `ab38727` had
89.3 %, 1 162 844 s, 2.77 M and 796 347 (`storm-seen-2026-10-31.md`),
and main `ab38727` 90.3 %. Nothing in D90's adoption touches the
hand-over; what came between, D91 (a sleepy node with an anchor runs
no GPS) and D92 (the receive cycle re-derived), changes radio events
and so the timing of every run, and seed 1 is a seed on the edge, as the
storm note found of D77's history: the same code storms or does not by
timing alone. Seed 7 is the same, 98.0 % at 305 040 s. On this base no
scenario shows the storm's airtime: the dense-power seeds are at the
295 000 s a healthy run of the old costs had. What every loaded
scenario keeps is the split rate the storm note named as what starts
it: of the hand-overs that fail, those whose receiver held the message
are 24 % on dense-power seeds 1 and 7, 24 to 27 % on S4 at three times
the rate, and 54 % on the walkers, where 287 084 hand-overs fail
against 322 312 taken. The storm is a chain reaction of these splits
above an edge; this base sits below the edge with the splits intact,
and the candidate's parts are measured on it as what they do to the
splits, not to a storm.

**A forward addressed early goes into a ban.** The first build of the
scheduler addressed a new place whenever a flight was free, during an
ACK wait included, so that up to four hand-overs sat addressed in
their turn while one was on the radio. The routing test of messages
waiting for a failure ban failed: a forward addressed to a neighbour
before that neighbour's ban was learned went to it when its turn came,
inside the ban. A hand-over's next hop is chosen when it starts (spec
"Hand-over" step 1 reads the address and the channel at each attempt,
not the neighbour), so a place must be addressed only when the radio
can serve it: one new flight at a time, when every other waits.

**The cost in the mesh is collisions, not late ACKs.** The storm note
named a late ACK missed while the sender serves another channel as the
risk to measure. In the mesh burst at seed 1 the simulator counts one
ACK started after the sender's timeout against none on main; what rose
was DATA lost while the receiver listened and was free, 157 → 338 of
about 1 200, and ACKs lost the same way, 19 → 44, with ACKs lost while
the sender received another frame 2 → 17. A relay whose radio is never
idle puts its frames closer together, and in a burst of a hundred texts
over 25 relays on a few channels that is more frames in the air at
once: hand-overs that failed with the receiver holding the message
doubled, 40 → 83, and the burst lost three texts.

## What it does not show

- Parts 2 and 3 of the candidate with this one; each is measured alone
  on the same base.
- Other seeds of `dense-power.toml` than 1 and 7.
- The board: nothing here was run on hardware.

## Recommendation

Not adopted alone, as the summary says. What the measurement decides:
the rule's gain is real where a relay's wait is not of its own making,
and its cost is that a relay at the storm's edge, one whose hand-overs
fail four times in ten with the receiver holding the message one time
in four, now makes those splits at the pace of a radio that never
rests. The brake was never a rule; it was the idle time this part
removes, and removing it shows the splits for what they are.

So the order of the candidate's parts matters more than the storm note
assumed. Parts 2 and 3 take the splits out of the rules: a receiver
that cannot answer in time keeps nothing, so that the sender's retry
takes the message afresh and there is one holder; a retry goes to the
receiver that may hold it, never to another on the channel's account.
With those adopted, a frame added by this rule is a hand-over that
completes, and the 15 % of a capped relay's hour the storm note found
idle is worth recovering. This rule should then be measured again on
that base, on both dense-power seeds first, with the split rate as the
figure that decides.

What should be kept whatever is decided: the structure, the flights in
the send queue and `HandOver` driving the queue, reproduces main to the
byte with the bound at one, carries no cost, and is what parts 2 and 3
were written to need (`main-loop-2026-10-30.md`); the two constants of
the tables, `ACK_TIMEOUT` and `HOP_ATTEMPTS`, replace literals; and
the count of frames sent while another hand-over waited is the figure
that says how much a never-idle radio adds.

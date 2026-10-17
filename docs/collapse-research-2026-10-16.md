# Why the capacity domain collapses

**Date:** 2026-10-16. Scenario S4 (`scenarios/s4-capacity.toml`, one domain
of 28 km², 269 relays, 20 000 indoor leaves). Builds on
`capacity-research-2026-10-15.md` and `leaf-polls-2026-10-15.md` (D39).
Spec references are to `protocol.md`. Protocol code: main `f1fa059`; the
simulator gains read-only instruments (commits `85cffb8` and `fe6ef32`) that change no
result. Nothing here changes the protocol. The experiments named below
were changed copies of the core, run once and thrown away.

## Summary

At the spec "Capacity and sizing" texting rate and at twice it the domain delivers 94 to 95 %.
At three times it delivers 11 to 22 %, and from about 25 minutes into
the run almost nothing gets through. The fall is not gradual: it takes
three minutes.

- **What breaks first is the hand-over itself, not the duty cap, a
  table, the polls or the leaves.** With 250 kHz channels, where no relay
  is near its cap, the share of hand-over attempts that get an ACK falls
  from 0.63 to 0.51 in two minutes and keeps falling: 0.42, then 0.31.
  The relays' DATA frames per minute quadruple in the two minutes after
  while the texts offered stay the same. Only then do relays reach the cap,
  refuse, skip polls and drop leaves, in that order.
- **The loop:** a lost attempt is sent again, up to three times, and a
  failed hand-over is sent at once to another neighbour, usually a longer
  way round. Each costs more frames. More frames keep the receivers
  busier receiving other frames and deaf more often, and lose more ACKs,
  so more attempts are lost. Above about three times the spec "Capacity and sizing" rate this
  feeds itself.
- **Why 250 kHz collapses at the same rate as 125 kHz:** both use the
  same spectrum. Six 250 kHz channels and thirteen 125 kHz channels give
  each message the same time on its channel; what is lost is lost to
  frames on the receiver's channel, not to any one relay's duty cap.
  Measured, the mean channel busy at the relays is 1.0 % at the spec "Capacity and sizing" rate
  with 125, 250 and 500 kHz alike.
- **What kicks it off in the runs measured:** the polls of D39. Every
  leaf took its slot when it attached, and the leaves attached together,
  so every relay polls most of its leaves in the same minute of each five.
  That minute adds 28 % to the domain's airtime. In both seeds at 250 kHz
  the collapse starts in such a minute. With the slots spread it starts
  six minutes later in one seed and at the same time in the other: the
  bursts are a trigger, not the cause. At 125 kHz the cap of the centre
  relays binds first, before any burst.
- **Why it stays at zero:** relays at the cap refuse fresh texts, as
  they should, and leaves hold them for the ten-minute TTL and then drop
  them (`NoRelay`, 30 196 of 58 477 texts). But the relays' whole budget
  goes on the texts already inside, which live up to 53 minutes and take
  a median of 1476 frames each when they end in a failed return, because nothing ends a message by time once a
  relay has it.
- **Suspects ruled out.** A `NoRelay` drop is not immediate: every one
  of the 30 196 was held exactly 600 s. Leaves keep their relay: attached
  leaves fall from 19 600 to 18 000. Tables do not fill first: no relay
  refused for want of a record, 948 refusals for a full send queue
  against 750 000 for the bucket, all after the onset. Polls are skipped
  only after the onset. Going to the next candidate after a refusal,
  demotion, and leaves' repeated offers each change nothing or only the
  timing.
- **A fix that moves the cliff:** treating an unanswered hand-over as
  pushback, the way a refusal is, raised delivery at three times the rate
  with 250 kHz from 15.5 % to 81.8 %, and at 125 kHz from 22.1 % to 34.2 %,
  with the ×2 runs unchanged. It does not remove the cliff: queues then
  fill with waiting messages and the domain collapses 30 minutes later.
  A draft decision is below; it needs an end to a message's life in the
  network as well.

## What was run

S4 as in the capacity note, the texting rate times 1, 2 and 3, seeds 1
and 2. Variants of the same code: **A**, main as it is (13 channels of
125 kHz); **B250**, 6 channels of 250 kHz, noise floor 3 dB higher;
**C500**, 3 channels of 500 kHz; **D1mW**, 1 mW and no duty cap with 3.6
times the relays, so that each relay has as many neighbours as at 10 mW
(run by the coordinator, read here). "Delivered" is the share of
reachable texts sent from 900 to 3900 s that arrived.

The new instruments (`bark-sim --series FILE`): each minute, every
relay's counters, bucket, queues and leaf table; the leaves' counters
summed; and the fate of every frame addressed to a relay, decided at the
receiver: received, or lost while it was listening and free, while it
was receiving another frame, while it was deaf on that channel (checking
the channel, transmitting, retuning), or while it was on another
channel; and for a received DATA, whether an ACK answered it within the
ACK timeout. The per-message table gains when the origin's core took a
text and when it was dropped.

| Run | ×2 | ×3 | First 5-minute slice under 80 % |
|---|---|---|---|
| A seed 1 | 94.4 % | 22.1 % | 1200 s |
| A seed 2 | | 10.7 % | 600 s |
| B250 seed 1 | 95.0 % | 15.5 % | 1200 s |
| B250 seed 2 | | 12.7 % | 1200 s |
| C500 seed 1 | | 94.5 % | none |
| C500 seed 2 | | 55.8 % | 2400 s |
| D1mW seed 1, ×2 and ×4 | 95.6 % | 25.4 % (×4) | 1500 s |
| D1mW seed 2, ×4 | | 21.0 % | 1200 s |

## The time series

B250, ×3, seed 1, per minute, summed over the relays unless said. "Answered"
is the share of relay-to-relay DATA attempts that drew an ACK. "Lost,
busy" and "lost, deaf" are the shares lost because the receiver was
receiving another frame or deaf on the channel when the DATA started.

| Minute ending, s | 1080 | 1140 | 1200 | 1260 | 1320 | 1380 | 1440 | 1500 | 1800 | 2400 |
|---|---|---|---|---|---|---|---|---|---|---|
| Texts offered | 865 | 892 | 849 | 826 | 913 | 901 | 847 | 899 | 830 | 823 |
| Texts delivered | 805 | 844 | 783 | 740 | 745 | 489 | 255 | 145 | 23 | 3 |
| Poll and answer frames (leaves' ATTACH) | 1337 | 1062 | **8279** | 5672 | 2478 | 1865 | 2070 | 9313 | 11 120 | 13 894 |
| Attempts answered | 0.63 | 0.57 | **0.51** | 0.52 | 0.42 | 0.31 | 0.29 | 0.34 | 0.36 | 0.30 |
| Lost, receiver free | 0.16 | 0.17 | 0.19 | 0.18 | 0.20 | 0.22 | 0.23 | 0.20 | 0.20 | 0.24 |
| Lost, receiver busy | 0.09 | 0.11 | 0.13 | 0.13 | 0.18 | 0.20 | 0.21 | 0.20 | 0.21 | 0.24 |
| Lost, receiver deaf | 0.02 | 0.04 | 0.04 | 0.04 | 0.08 | 0.12 | 0.13 | 0.13 | 0.10 | 0.09 |
| ACK lost | 0.09 | 0.10 | 0.13 | 0.13 | 0.18 | 0.22 | 0.23 | 0.21 | 0.20 | 0.23 |
| Relays' DATA frames | 5528 | 6691 | 8366 | 8216 | 15 252 | 33 439 | 45 378 | 46 139 | 39 081 | 39 169 |
| DATA frames per hand-over | 1.59 | 1.77 | 1.99 | 1.93 | 2.47 | 3.44 | 3.89 | 3.67 | 4.29 | 6.60 |
| Hand-overs that succeeded | 3478 | 3776 | 4198 | 4254 | 6172 | 9716 | 11 671 | 12 567 | 9120 | 5933 |
| Neighbours demoted | 108 | 130 | 235 | 221 | 574 | 1750 | 2264 | 1818 | 1380 | 1444 |
| Relays at the cap | 0 | 0 | 0 | 0 | 0 | 7 | 48 | 118 | 223 | 265 |
| Refusals received | 0 | 0 | 0 | 0 | 0 | 35 | 542 | 1791 | 4271 | 4860 |
| Polls skipped | 0 | 0 | 0 | 0 | 10 | 14 | 16 | 233 | 138 | 52 |
| Send queues, entries | 4 | 14 | 43 | 20 | 203 | 1115 | 2330 | 3518 | 6808 | 9627 |
| Leaves holding a text | 139 | 138 | 132 | 179 | 247 | 432 | 675 | 1085 | 3759 | 6943 |
| Leaves' DATA frames | 1341 | 1467 | 1429 | 1452 | 1795 | 2217 | 3226 | 5171 | 13 150 | 21 291 |
| Texts dropped, no relay took them | 2 | 2 | 4 | 1 | 1 | 0 | 1 | 2 | 11 | 577 |

Received DATA was answered: no ACK started within the timeout for at
most 1 % of attempts until 1320 s and 3 % after, and refusals rose to
2 % of attempts at 1440 s and 18 % only by 2400 s. So the attempts are lost on the air, at the receiver or on the
way back, not by a receiver that chose silence.

The minute ending at 1200 s is a poll minute: the airtime summed over the
domain rose from 6.9 to 9.6 s a second, 1.9 s of it ATTACHED and the
leaves' answers. Collisions heard at the 11 centre rooftop relays rose
from 448 to 1582 a minute each, and their hand-overs that succeeded fell
from 32.8 to 25.6 a minute while the centre windowsill relays' rose from
23.7 to 30.1: the long hops failed first and their work moved to short
ones. The poll minute 300 s earlier, at 900 s, raised the frames lost to
capture at relays too, from 17 000 to 44 000 a minute, and the domain
recovered; at 1200 s, from 22 000 to 70 000 on a slightly higher base
load, it did not.

At 1380 s, the first minute with relays at the cap, the seven relays
there are centre and inner windowsill relays that had used 3.2 to 5.2 %
airtime on average until then. By 1500 s 118 relays are at the cap, by
2400 s all but four.

**Seed 2** at 250 kHz: the same minute, 1200 s, the same order. DATA frames
per hand-over 1.88 → 2.91 → 4.21 and demotions 191 → 934 → 2447 a minute in the two
minutes after it, the first relay at the cap at 1320 s, polls skipped
from 1260 s.

**At 125 kHz** (A, seed 1) the cap comes first. Seven relays, six of them
in the centre, are at the cap at 720 s, in a poll minute, refusals start
in the same minute, and from there the same climb follows: DATA frames
per hand-over 1.75 → 3.0, hand-overs 3600 → 5700 a minute at the same
offered load, leaves holding texts 120 → 5000. With the poll slots spread
(below) the first relays reach the cap at 540 s instead: at three times
the spec "Capacity and sizing" rate the centre relays of A are over their duty cap without any
burst, as the capacity note found at the spec "Capacity and sizing" rate before D39.

**With no duty cap** (D1mW, ×4) the run shows the same shape: delivered
above 95 % until 1200 s, then hops per delivered text 5.8 → 15 → 38 and
frames per delivered text 26 → 400 → 1100, and 291 347 demotions against
8532 at ×2. No relay refuses for its bucket, so nothing reaches the
leaves; 46 672 texts end the run never forwarded by a relay, more than
the 20 000 leaves can hold, so most of them wait in their first relay's
send queue. Without a cap the regular room is ⌈60 s / 153 ms⌉ = 393 places
(spec "Hand-over", the bucket's depth over a hand-over), so admission never pushes
back. That the run collapses at all says the loop is in the hand-overs.

## Where the lost attempts go, below the cliff

At twice the spec "Capacity and sizing" rate, where delivery is still 94 %, the attempts already
fail often, as a diagnostic trace on another branch found (47 % of
ACK waits unanswered at 125 kHz):

| Share of relay-to-relay DATA attempts | A ×2 | B250 ×2 |
|---|---|---|
| Lost, receiver listening and free (weak link, fading, interference) | 0.15 to 0.17 | 0.18 |
| Lost, receiver receiving another frame | 0.07 to 0.11 | 0.06 to 0.08 |
| Lost, receiver deaf on the channel | 0.04 to 0.08 | 0.02 |
| Lost, receiver on another channel | 0.00 to 0.06 | 0.01 to 0.03 |
| Received, no ACK in time | 0.00 to 0.01 | 0.00 |
| Received and refused | 0.00 to 0.01 | 0.00 |
| Answered, after ACKs lost (0.10 to 0.12 of ACKs at A, 0.08 at B250) | 0.54 to 0.64 | 0.63 to 0.67 |

The part lost to a free receiver is the link and stays about the same
from ×1 to ×3. Everything that grows with load is the receiver being
busy with other frames, being deaf, and the ACK being lost: each grows
with the frames on the channel around it. In the collapse those three
reach 0.24, 0.13 and 0.23 of attempts.

## Why it stays down

From 1800 s on, the relays' DATA frames are 22.7 s of airtime a second
domain-wide, the refill of all 269 buckets. The texts sent after 1800 s
account for little of it: of the texts sent from 1200 to 1800 s, 827 ended
as `ReturnFailed` after a median life of 42 minutes and 1476 frames each,
1241 as `QueueExpired` (they reached the destination's relay, whose
bucket could no longer pay the DELIVER) after 15 minutes and 187 frames
at the median, 460 on average.
Their search goes on as long as hand-overs to someone succeed; the hop
cap was reached by none, because a lost attempt and a wait do not count
hops. Across the run, messages that ended `ReturnFailed` lived 37 minutes
at the median and 48 minutes at the 90th percentile, `QueueExpired` 12
and 32 minutes, `NoRoute` 7 and 33 minutes.

Leaves refused or unanswered hold their text, offer it again after the
hold (spec section "Attach and polls" (before D46)) and drop it at the TTL. Their offers are full DATA frames:
12 to 14 s of airtime a second in the collapse, a third of the total.
Making them wait the longest hold, 61 s, after the first refusal cut that
to 8 to 9 s a second and changed nothing else: delivery 15.8 % against
15.5 %, the same minute of onset.

## Why 250 kHz behaves like 125 kHz

What doubles at 250 kHz is the rate at which one channel carries bits;
what halves is the number of channels. A text costs one relay half the
airtime, so each relay is half as close to its duty cap: B250's busiest
relay uses 4.1 % airtime at ×2, 2.4 s a minute of the 5.9 s the bucket
refills.
But each channel now carries twice the texts, so the time each text keeps
its channel busy, which is what a receiver on that channel loses to it, is
the same. The spectrum is 13 × 125 = 1625 kHz against 6 × 250 = 1500 kHz.
Measured at the relays, the mean busy share of a channel is 1.0 % at the
spec "Capacity and sizing" rate with 125, 250 and 500 kHz alike, and 2.8, 2.3 and 2.0 % at
twice it. The attempt losses that grow with load are counted in frames
on the receiver's channel, and they are the same in A and B250 at the same
texting rate. The duty cap is a second limit at 125 kHz that happens to
bind at about the same rate.

Fixed times that do not shrink with the bandwidth add to this a little:
the 100 ms ACK timeout, the 0 to 50 ms listen-before-talk backoff and
the retune. The share of time a relay has a hand-over under way was 6 to
8 % before the onset at 250 kHz (10 to 15 % at 125 kHz), so one relay running one
hand-over at a time is not what binds.

## Experiments

Each changes one rule in a throw-away copy of the core; ×3 at the
seed named, against the same seed unchanged. Not merged, not to be.

| Change | B250 ×3 | A ×3 | ×2 |
|---|---|---|---|
| None | 15.5 % (seed 1), 12.7 % (seed 2) | 22.1 % (seed 1), 10.7 % (seed 2) | 94 to 95 % |
| A new leaf's poll slot drawn from its ID, so leaves that attach together are not polled together | 25.5 % (onset 1620 s instead of 1260 s), 15.6 % (same onset) | 21.7 %, 11.3 % | A: 94.5 % |
| A refused forward waits for the ban instead of going to the next candidate | 17.4 % | 20.2 % | |
| No demotion | 24.8 % (onset 1500 s) | 12.5 % | |
| No demotion, poll slots spread | 21.1 % | | |
| A refused leaf waits 61 s at once | 15.8 % | | |
| An unanswered hand-over is pushback: after the k-th failed forward or return in a row the relay starts no hand-over for one ban doubled k − 1 times, up to 61 s (the hold of spec "Hand-over"), and takes no fresh work meanwhile | **81.8 %** (94 % until 2400 s, then collapses) | **34.2 %** | 94.6 % (B250), 94.4 % (A) |

The last one is the only change that moves the cliff by much. With it
the B250 domain carries the ×3 load for 40 minutes; then send queues fill
with waiting texts (256 entries at 2400 s, 2209 at 3300 s), latency
grows, and the same collapse follows. One A run lost a text without a
trace with it; the diagnostic copy was not checked further.

## The loop, in order

1. The channels around the centre are loaded close to the point where
   receivers start losing frames to each other: at ×3, a fifth of all
   relay-to-relay attempts are already lost to a receiver busy or deaf,
   and a tenth of ACKs.
2. A burst of extra frames (a poll minute at 250 kHz) or the duty cap of
   the centre relays (at 125 kHz) pushes the loss up.
3. Each lost attempt is sent again, up to three times (spec "Hand-over"), and a
   hand-over whose three attempts fail goes at once to the next
   candidate (spec "Next hop"), usually a longer way, and twice in a row demotes
   the neighbour for three rotations (spec "Neighbour table"), sending later texts the
   longer way too. Nothing slows a relay whose hand-overs fail, and
   nothing tells the leaves.
4. More frames: the receivers are busier and deaf more, more ACKs are
   lost. DATA frames per text grow fourfold in three minutes.
5. Buckets run dry. Relays at the cap refuse fresh texts (spec "Receiving DATA", D34),
   which is right, and stay silent below the reply reserve (spec "Duty cycle cap"), which
   is one more unanswered attempt for their neighbours.
6. The texts already inside never end by time. They keep the buckets dry
   for up to 50 minutes. The leaves' texts wait their ten minutes and are
   dropped. Delivered: zero.

So the network does what the pushback rules ask of overload counted in
duty-cycle airtime and send-queue places, D29 to D36, and is blind to
overload counted in lost attempts.

## What the keepalive redesign must get right

These are measured here and bear on the build of the leaf-driven keepalive
on the other branch:

- **Spread the times.** Every leaf took the first free slot after it
  attached, and leaves attach together after a mass start, after a
  relay restarts or after an outage, so every relay's polls came in the
  same minute of five: 8000 to 9000 poll frames a minute against 1000 to
  2000 in the others, 28 % more airtime in the domain for that minute. In
  the simulator every relay's slot grid also starts at the same instant,
  which aligns the relays with each other; on boards whose clocks start
  at power-on that alignment is weaker, but the bunching per relay stays.
  A leaf's time should be spread over the period regardless of when it
  attached, for example drawn from its ID, as the experiment above did.
- **The ATTACH and the ATTACHED are small, but they are frames on the
  relay's channel and count against everything else there.** In the
  collapse they were 2.5 to 2.6 s of airtime a second domain-wide.
- **The period must not shorten with load.** A period of leaves × N
  clamped to 10 to 300 s gives a relay with 30 leaves an ATTACH every
  1.17 s on its channel, every 35 s per leaf: about nine times the
  frames of today's 300 s for the same leaves. Measured here, extra frames on the
  channel are what start the collapse.
- **A refused leaf should not offer its full text again and again.** If
  the ATTACHED carries the count of waiting messages, it could also carry
  whether the relay has room, and the leaf offer its DATA only then. The
  measured effect on the collapse is small, but each refused offer is a
  full frame sent for nothing (D32).
- **Keeping the relay is not what fails.** Leaves stayed attached. The
  `NoRelay` drop is a text held for its whole TTL by an attached leaf
  whose relay would not or could not take it.

## Draft decision (number to be assigned)

**An unanswered hand-over is pushback, and a message's life in the
network ends with its TTL.** (Spec "Neighbour table", "Next hop", "Receiving DATA", "Hand-over", "Duty cycle cap",
spec section "Attach and polls" (before D46), "Packet formats".)

(1) *Failure backs the sender off.* After the k-th hand-over in a row that
fails, three attempts unanswered (spec "Hand-over" step 6), forward or return, the
relay starts no hand-over for the hold of spec "Hand-over", one ban doubled k − 1
times, up to the bucket's refill of 61 s, and takes no fresh work while
it waits (D34's gate, which now also refuses while backing off). A
hand-over that succeeds ends the run. No new constant: the ban and its
doubling are those of refusals. An unanswered hand-over is the network's
sign that the channel or the receiver is full, as a refusal is the sign
that the receiver's bucket is; today it costs the sender nothing and
sends the message the long way at once.

(2) *A message ends when its TTL does.* DATA carries the time left to the
delivery TTL, in units of 10 s (one byte holds 600 s at that unit), set
by the origin and lowered by each relay by the time the message spent
there; a relay drops a message whose time is gone, as the delivery queue
and the leaf already do (spec section "Attach and polls" (before D46), "Delivery queue"). Today a message admitted to the
network lives as long as hand-overs to somebody succeed, measured up to
53 minutes, and keeps the relays at the cap long after its sender and
its destination's queue would have given up on it. This adds an end to
the list of D31; the time is the one the spec already gives the network
(spec section "Attach and polls" (before D46): "the time the network itself would keep it").

(3) *Polls and keepalives are spread over their period by the leaf's ID,*
whatever the time it attached (see above).

Not measured together. Rule 1 alone was measured above: from 15.5 to
81.8 % at 250 kHz and from 22.1 to 34.2 % at 125 kHz, ×2 unchanged, the
cliff later but still there, with queues filling. Rule 2 needs a wire
change and is the one that should stop the queues filling with messages
nobody waits for. Before deciding, both should be built together and
measured on S4 ×1 to ×4 at 125 and 250 kHz, two seeds, and on the standard
and loaded suites for no loss without a trace.

## What remains open

- The loss of 15 to 20 % of attempts to a receiver that was listening and
  free is the links: fading and weak links D38 already ranks by margin.
  It is a third of all losses at ×2 and is not what grows.
- Why the centre relays at 125 kHz reach the cap at ×3 at all after D39:
  their mean airtime at the onset was 5 to 6 %; the bucket empties in
  bursts. Not followed further.
- Whether a relay that answers its neighbours' DATA during its own ACK
  wait blinds itself often enough to matter (the same trace: 94 % of
  such waits unanswered, 4 % of all waits). Not split here.
- One seed of each experiment, two of the bursts experiment. The
  unchanged runs are deterministic and repeat to the frame.
- The C500 run that survives ×3 (seed 1) and the one that does not (seed
  2) were not followed in time.

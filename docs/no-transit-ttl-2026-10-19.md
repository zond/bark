# A message without an end by time on its way

**Date:** 2026-10-19 and 20, roadmap D67. **Status:** measured; decided
2026-10-20, the time left kept, the candidate not adopted. Spec references are to `protocol.md`. The base, named
"main" in every table, is main `f1d4e2c` (D66) with this branch's
read-only instruments, which repeat main to the frame (S4 at three times
the rate, seed 1, gives D66's 81.56 % delivered). The branch was then
rebased onto main `79b4313`, whose speed-ups change no output; the
candidate built on it gives the same message table to the byte.

The question: does a message still need the time left that DATA carries
(spec "Time to live", roadmap D44) now that overload is pushed back
to the application (roadmap D54, D66)? A text that ages out on its way
is lost with nobody told; refused at its origin, or delivered late, it
would not be.

## Summary

- **No: at the spec rate, on the strip and in the loaded suites, the
  time left ends nothing that matters.** S4 ×1 96.0 → 96.0 % and
  96.4 → 96.4 %, the strip 95.1 → 95.1 %, every suite within its spread.
- **Yes at three times the rate, by 2 to 9 points:** S4 ×3 81.6 → 78.5
  %, 77.5 → 75.2 %, 89.9 → 81.2 % (seeds 1, 2, 3); the loss is texts
  refused at their origin, +2.4 to +6.5 points, and aged there, −0.1 to
  +2.2 points.
- **The cause is one kind of text: those for a sleepy node with no
  anchor.** Its address is its own position, which no relay owns, and
  if no node hears it the search walks the domain: those texts are 0.1
  to 0.2 % of all and are almost never delivered (8 of 205 on main, seed
  1). Main ends them by time, after about 660 frames each at three times
  the rate; without the time left they run to the hop cap, 1100 to 1600
  frames each, and up to 35 of them are on their way at once, holding
  the highest hop places, which go first when a ban ends. With texts to
  such nodes not sent (a diagnostic, both binaries), the two are equal:
  81.72 → 81.66 %.
- **Nothing else needed the time left.** Of the texts main ages on their
  way, 78 to 100 % are for such nodes; the rest are delivered later or
  end elsewhere. No text hit a lost record because it lived longer,
  except those same texts at three times the rate (six of them,
  seed 3).
- **What the time left still does** is bound in time a search that
  cannot succeed. The hop cap bounds it in hand-overs, at about twice
  the cost under load and at every rate an expensive one: at the spec
  rate the texts for nodes without an anchor are 0.2 % of the texts and
  4 % of all airtime.
- **Proposed, unbuilt:** that no search start for a node that cannot be
  found, rather than ending it by time. Until then, keep the time left.
  The wire change that would follow (one byte, not two) saves no airtime
  on a 100-byte text.

## What was changed

The candidate (commit "Core and spec: a message the network has taken
has no end by time") keeps the wire as it is, stamps every DATA with the
time left as before, rounded down and never below zero, and changes the
places that ended a message when that time ran out:

| Place | On main | Candidate |
|---|---|---|
| `receive_data`: a packet to send on with no whole unit left | taken as no work, the message ends (`Expired`) | work like any other |
| `expire`: the send-queue sweep, every entry waiting for its turn, the bucket, a ban, in a hop place or as a return | dropped (`Expired`) | only the application's message at its origin, before a first hop takes it |
| `transmit_current`: an entry whose time is gone at its next attempt | dropped (`Expired`) | the same, only at its origin |
| `time_gone_at`: the wakeup for the sweep | every entry | the application's message only |
| The delivery queue (D66) | its own staleness, the time left carried to the node | unchanged |
| A sleepy node's own message, waiting for its anchor or for any neighbour | ages at the node after a TTL | unchanged: it is at its origin |
| The simulated application's patience on a given-back text | the delivery TTL | unchanged |

The origin keeps its end by time: a node holds its application's message
for at most the TTL until a first hop takes it, which is where D54
measures "aged at the origin". Spec text for the candidate was written as
a proposal in the same commit, and reverted with it (below).

## What was run

S4 (`scenarios/s4-capacity.toml`, two settled hours, 10 500 s, the
reachable texts the sleepy nodes sent from 2700 to 9900 s, as D54 and
D66) at the spec rate (×1), seeds 1 and 2, and at three times it (×3),
seeds 1, 2 and 3; the third seed was added when the first two
disagreed with the spec rate. The strip (`scenarios/strip.toml`) at the
spec rate, seed 1, its settled hour (2700 to 5700 s). The suites of D54,
paired by seed, ± one standard error: the corridor at one text per leaf
every 1 s and 0.5 s, the ring at 1 s and at its own 2 s, mesh-burst at 64
times (30 seeds each, the ring at 2 s 10), and S1, S1b, S2, the corridor,
mesh-burst and mesh-spread at their own settings. One simulation at a
time.

New read-only instruments (commit "Simulator: records a relay lost and
then needed"): each DATA a relay receives for a message it once kept a
record of and no longer does, and per relay in the series how many
records it keeps and, once the table is full, how long its least
recently active record has been idle.

## Results

Of the reachable texts sent from 2700 to 9900 s, main → candidate.
"Ended by the search" is `NoRoute` and `ReturnFailed`. "Still held" is a
text in a delivery queue (stale texts kept, D66) or on its way when the
run ends.

| S4, seed | ×1 s1 | ×1 s2 | ×3 s1 | ×3 s2 | ×3 s3 |
|---|---|---|---|---|---|
| Delivered | 95.99 → 96.01 % | 96.42 → 96.40 % | 81.56 → 78.52 % | 77.46 → 75.16 % | 89.91 → 81.24 % |
| Refused at the origin | 0 → 0 | 0 → 0 | 8.05 → 10.42 % | 11.77 → 14.19 % | 2.34 → 8.83 % |
| Aged at the origin | 1.25 → 1.20 % | 1.13 → 1.06 % | 5.90 → 6.55 % | 6.34 → 6.25 % | 3.19 → 5.38 % |
| Aged in transit | 0.03 → 0 | 0.03 → 0 | 0.09 → 0 | 0.16 → 0 | 0.16 → 0 |
| Expired or made way in a delivery queue | 0 → 0 | 0 → 0 | 0.99 → 1.09 % | 0.86 → 0.80 % | 1.44 → 1.33 % |
| Ended by the search | 0 → 0 | 0 → 0 | 0.01 → 0 | 0.01 → 0.01 % | 0.01 → 0.01 % |
| Ended at the hop cap | 0.09 → 0.10 % | 0.09 → 0.13 % | 0.01 → 0.05 % | 0.01 → 0.07 % | 0.01 → 0.10 % |
| Still held in a delivery queue | 2.62 → 2.67 % | 2.33 → 2.40 % | 3.20 → 3.16 % | 3.13 → 3.24 % | 2.89 → 2.93 % |
| Still held on its way | 0.01 → 0.01 % | 0 → 0 | 0.17 → 0.21 % | 0.27 → 0.28 % | 0.05 → 0.18 % |
| Latency p50, s | 1.5 → 1.5 | 1.6 → 1.6 | 109 → 130 | 158 → 165 | 54 → 92 |
| Latency p90, s | 213 → 211 | 228 → 229 | 595 → 623 | 656 → 663 | 462 → 577 |
| Latency max, s | 6817 → 7103 | 7351 → 7438 | 7342 → 7405 | 7620 → 7370 | 7233 → 7114 |
| Airtime, whole run, s | 90 358 → 87 906 | 90 257 → 90 794 | 269 748 → 271 361 | 282 786 → 282 043 | 256 778 → 279 927 |
| Texts at 255 hand-overs | 36 → 37 | 36 → 47 | 19 → 49 | 20 → 80 | 20 → 116 |
| Texts of 100 to 254 | 10 → 1 | 10 → 0 | 65 → 14 | 76 → 18 | 103 → 31 |
| Delivered, hand-overs p50, p90, p99 | 4, 7, 9 both | 5, 7, 10 both | 6, 11, 21 → 6, 12, 23 | 6, 13, 27 → 6, 14, 28 | 5, 9, 19 → 6, 11, 25 |
| On its way, p99, max, s | 9, 592 → 9, 1471 | 10, 592 → 9, 1941 | 151, 595 → 191, 3957 | 194, 595 → 271, 4000 | 113, 595 → 309, 5037 |
| On its way over 600 s | 0 → 5 | 0 → 6 | 0 → 45 | 0 → 126 | 0 → 298 |

"On its way" is from the text's first DATA from a node other than its
origin to its holder's queue, or to its end.

Every 20 minutes the gap at ×3 has the same sign; at seed 1 it is 3 to 5
points from 2400 to 6000 s and 1 to 2 points after, at seed 3 7 to 9
points throughout:

| Delivered by 20 minutes of sending, ×3 | 2400 | 3600 | 4800 | 6000 | 7200 | 8400 |
|---|---|---|---|---|---|---|
| Seed 1, main | 75.0 % | 79.5 % | 80.9 % | 83.1 % | 84.6 % | 84.3 % |
| Seed 1, candidate | 72.1 % | 74.9 % | 75.9 % | 80.3 % | 83.3 % | 82.3 % |
| Seed 2, main | 76.4 % | 76.3 % | 75.5 % | 78.7 % | 79.7 % | 79.5 % |
| Seed 2, candidate | 73.5 % | 74.3 % | 75.2 % | 75.8 % | 77.7 % | 75.5 % |
| Seed 3, main | 86.4 % | 88.5 % | 89.3 % | 89.2 % | 92.7 % | 92.5 % |
| Seed 3, candidate | 78.1 % | 79.7 % | 81.1 % | 80.8 % | 84.8 % | 83.0 % |

**The strip**, spec rate, seed 1, the settled hour: delivered 95.12 →
95.13 %, refused at the origin 0.04 → 0.02 %, aged there 1.44 → 1.42 %,
aged in transit 0.01 → 0, hop cap 0.04 → 0.05 %, latency p50, p90, max
5.5, 262, 3276 → 5.5, 266, 3218 s, airtime 196 971 → 200 616 s (+1.9
%), on its way over 600 s 0 → 6, longest 592 → 1325 s.

**The suites**, settled delivered of the reachable, main → candidate:

| Scenario, seeds | Delivered | Airtime | Latency p50, p90, max |
|---|---|---|---|
| Corridor at 1 s, 30 | 0.9662, identical | identical | identical |
| Corridor at 0.5 s, 30 | 0.9235, identical | identical | identical |
| Ring at 1 s, 30 | 0.8807 → 0.8805 (−0.02 ± 0.30) | 1532 → 1533 s | 62, 174, 753 → 61, 174, 761 s |
| Ring at 2 s, 10 | 0.9293, identical | identical | identical |
| Mesh-burst at 64 times, 30 | 0.9775 → 0.9797 (+0.26 ± 0.20) | 4903 → 4913 s | 11, 85, 655 → 12, 88, 1704 s |
| S2, 60 | 0.9356 → 0.9374 (+0.17 ± 0.13) | 136 → 156 s | 0.6, 204, 2363 → 0.6, 214, 2363 s |
| Mesh-spread, 10 | 0.9966 → 0.9989 (+0.22 ± 0.15) | 174 → 176 s | max 320 → 679 s |
| S1, S1b, the corridor, mesh-burst, 10 to 60 | identical | mesh-burst 189 → 195 s | identical |

In the loaded corridors and the ring at 2 s nothing ages on the way on
main either, so they are identical to the frame. Where something did
(the ring at 1 s 19 texts, mesh-burst at 64 times 290, S2 35), the
candidate delivers some of them late; S2 pays 15 % more airtime for
carrying them on.

**Where the texts that aged in transit went.** Matched by origin,
destination and time sent, the texts main ages on their way end in the
candidate as follows (the runs diverge, so a text's fate changes for
other reasons too):

| | Main aged in transit | Candidate: delivered | Hop cap | Still held | Elsewhere |
|---|---|---|---|---|---|
| ×1, seeds 1, 2 | 12, 9 | 3, 1 | 6, 7 | 3, 1 | 0, 0 |
| ×3, seed 1 | 91 | 28 | 24 | 20 | 19 |
| ×3, seed 2 | 156 | 60 | 36 | 20 | 40 |
| ×3, seed 3 | 159 | 42 | 56 | 14 | 47 |

Of the texts main ages on their way, 12 of 12 and 9 of 9 at ×1, and 78
of 96, 95 of 160 and 125 of 162 at ×3, are for a sleepy node without an
anchor. Every text either version ends at the hop cap, at both rates,
is for such a node, but one.

## Why three times the rate is worse

A sleepy node that hears no relay at its boot listen has no anchor and
gives out its own position as its address (spec "Sleepy nodes alone").
No relay owns that position (spec "Owner"), and unless a node on the way
hears the node itself, the search takes the relay nearest the position
for the edge of a gap and searches on, through every relay it can reach.
The domain has 269 relays, so the search does not finish within the hop
cap's 255 hand-overs. These texts are about 0.2 % of all texts at both
rates (59 to 88 in a run at ×1, 137 to 246 at ×3, whole runs), and few of
them are delivered (1 to 7 at ×1, 5 to 20 at ×3).

At the spec rate the hop cap ends them before their time would: about
900 to 1000 frames each, 3.7 and 4.6 % of all airtime on main. At three
times the rate the bans make each hand-over slow, and main ends them by
time, after about 660 frames each (seed 1); the candidate carries them
to the hop cap, 1100 to 1600 frames each, over 2000 to 4000 s. On their
way at any moment there are at most 16 of them on main, and in the
candidate 15 to 17, 19 to 24 and 22 to 34 through its busiest hours
(×3, seeds 1, 2, 3). Each holds a hop place numbered near its count, the
highest at every relay it waits at, and is offered first when a ban ends
(spec "Overload waits"), so the ordinary texts behind it wait longer: hop
places held in the whole domain, the mean over the two hours, 277 → 366,
380 → 483 and 116 → 398, the most at one relay 18 → 22, 22 → 39 and 23 →
48. A relay's regular room takes fresh work only if its bucket holds
everything queued, hop places included (spec "Receiving DATA"), so it
refuses more of its sleepy nodes' texts, for longer: refusals to them
257 641 → 295 171, 331 748 → 350 132 and 106 332 → 224 480, the waits told
of 32 bans or more 6.3 → 7.7, 7.4 → 13.5 and 3.4 → 12.1 % of them. The
regular room itself is unchanged (66 to 85 places held in the domain on
average in both). Seed 3 on main is the least loaded of the three and
settles at 92 % after two hours; the candidate's futile searches keep it at 83
%.

**The control.** With every text for a node without an anchor not sent
(a diagnostic switch in the simulator, in both binaries, not kept), ×3
seed 1: main 81.72 %, candidate 81.66 %; refused at the origin 8.20 →
8.16 %, aged there 5.51 → 5.67 %; hop places 264 → 270; airtime 256 646
→ 257 805 s; at most 7 texts on their way over 600 s at a time, all
delivered. Those texts are the whole difference.

## The records table

A message that lives longer could outlive its records (roadmap D31) and
then loop. Checked with the new instrument: every DATA a relay receives
for a message whose record it once kept and has since lost, and every
record table's least recently active record once the table is full.

At ×1 no table is full before 8400 s and no message meets a lost record.
At ×3 the tables fill from about 5000 s, 170 to 212 relays by the end,
and the least recently active record there has been idle 3100 to 3500 s,
in both versions. Main has 2, 2 and 8 packets for a lost record (2, 2 and
5 messages), all of them texts that waited in a delivery queue for over
an hour (D66) and were offered again; the candidate has 2, 3 and 27
packets: at seeds 1 and 2 three texts delivered late, as on main; at
seed 3 nine messages, six of them texts for nodes without an anchor, ended at the hop
cap 4500 to 6000 s after they were sent, longer than a record lasts, and
three texts delivered late. So it happens, only to texts that live longer
than about 55 minutes, which without the time left are the futile
searches; the hop cap still ends them. The strip and the suites have no such packet.

## What the time left still does

Nothing in the back pressure of D54 and D66 can refuse a text whose
destination cannot be found: its origin and every relay on the way take
it as ordinary work, since each has room for it. The only ends of such a
search are the search running out, which needs more hand-overs than the
hop cap gives in a domain of 269 relays, the hop cap, and the time left.
The time left is the only end in time: under load it ends these searches
at about half the cost, and keeps them below the life of a record.
Everything else it ends is either delivered without it (×3: 28 to 60
texts in a run) or ended elsewhere; at the spec rate it ends nothing the
hop cap would not.

What it does not do: it does not end the futile searches at the spec
rate, where they cost 4 % of all airtime for 0.2 % of the texts, and it
gives nobody a reason to wait instead of send.

## What would replace it

Not to start a search that cannot succeed. Two forms, neither built:

- **No address for a node without an anchor.** A node gives out an
  address only while it has an anchor; one without says so to its
  application, which can tell its contacts it cannot be reached. The
  control above is this in effect, at the sender, and loses nothing the
  time left saves. What it gives up: the texts the search does deliver,
  1 to 7 in a run at ×1, 7 to 20 at ×3, when a node on the way hears the
  node itself.
- **An address that says it is a node's own position.** A text to it
  would be held by the relay nearest the position, in its delivery queue
  under D66's rules, rather than search the domain; the node, if it ever
  comes in reach, collects it at a beat. This keeps the deliveries the
  search makes in reach of that relay and turns the rest into a held
  text, so that nothing the network takes is dropped for want of a search. It needs a bit in
  DATA or a reserved position form, and a rule for whom the nearest relay
  is, which the depth-first search does not know: "no neighbour closer to
  the position" is also how a gap looks (spec "Owner").

With either in place, the time left ends nothing this note could
measure, and could go.

## The wire change, not proposed

Without an end by time on the way, DATA would not need `time_left`. The
field is one byte (spec "Packet formats"; D44 chose one byte of 2.5 s),
not two. Removing it makes the header 21 bytes. By `radio::time_on_air`,
at SF5 a byte is 8 of the 20 bits in a block of five symbols, 1.28 ms,
so it saves a block on 94 of the 234 payload lengths and nothing on the
rest: a full frame 138.56 → 137.28 ms (−0.9 %), a 100-byte text 69.44 ms
either way, a DATA with no payload 18.24 ms either way; on average over
payload lengths 0.51 ms a DATA. The S4 texts would save nothing. It would
also free one instant per send-queue place (`Outgoing::expires`, 294 × 8
= 2352 B), less the one the origin needs for its own message, and it
would take from the destination the time its text was on its way, which
D66 gives it. Not proposed: the measurement says to keep the time left
until a futile search can no longer start, and the saving is small.

## What this does not show

- Three seeds at ×3, two at ×1, one strip. The runs repeat to the frame.
- Twice the rate was not run; at ×1 the futile searches end at the hop cap
  before their time, at ×3 by time, and where between the two the cost
  turns is not measured.
- Why the search for a node without an anchor is almost never answered
  by a node that hears it was not traced; the texts delivered show that
  it sometimes is.
- The simulated application does not react to a told wait or to a text
  that does not arrive.

## Recommendation

**Keep the time left.** Without it, S4 at three times the rate delivers
2 to 9 points less, all of it from the searches for sleepy nodes without
an anchor, which it is the only thing to end by time. Everything else
measured is unchanged or slightly better without it.

**Proposed, for the user to take or leave, unbuilt:** that a search that
cannot succeed does not start, by one of the two forms above; the second
fits the direction that nothing taken is dropped. Once one is in, the
time left can be measured again, and the byte removed if it then ends
nothing.

The candidate commit is reverted on the branch; the instruments stay.

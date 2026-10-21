# Inside the network at the edge of its capacity

Research note, 2026-10-18, roadmap D55; decided 2026-10-19, both rules adopted. Measured against main `4ccbaf4`
(D58), with read-only instruments added; the branch was then rebased onto
`c1732b2` (D59 to D64) without measuring again. A first round against `8704790`
(D52), before D57 and D58, found the same mechanism; its experiments are
reported where the new baseline was not rerun, and say so. Spec references
are to `protocol.md`.

## Summary

The capacity domain of `scenarios/s4-capacity.toml` at twice the spec
"Capacity and sizing" rate holds about 90 % for half an hour to an hour and
then slides, and the strip of three domains collapses at the spec rate
itself, its middle domain's relays at their duty cap 81 % of the time.
Traced per relay and per minute, what breaks first in both is not the duty
cap, a table or the delivery queues: it is the channel move rule, fed by a
busy estimate that read a relay's home channel differently from the
channels it compares it with.

- **The home channel was read differently.** The move rule compares the
  home channel's busy estimate with the quietest channel's
  (spec "Channel moves"). Other channels are read by RSSI alone; the home reading also
  counted the receive time of every packet decoded there, faint ones
  included, which no RSSI reading sees. At twice the rate the home channel
  read 7 to 12 % busy against 2 to 4 % for the quietest, half the relays
  were over the 5-point threshold at any time, and the domain's relays
  moved 1 191 and 1 161 times in two hours, most of them every 15 to 25
  minutes.
- **Each move costs the network, and the cost feeds the rule.** Senders on
  other channels send to the old one for up to a rotation; hand-overs fail;
  failures whose receiver had the message split it, and the copies search
  on. The extra frames make every channel busier and the home readings
  higher.
- **Read alike** (commit `aed03d0`), relays move 8 and 6 times in two hours
  and twice the rate delivers 92.7 and 93.1 % in every ten minutes of two
  settled hours, against 71.4 and 89.1 % on main, at 70 % of main's airtime
  for 13 % more texts taken in. The strip at the spec rate delivers 91.5 %
  against 24.6 %, its middle domain at the cap 5 % of the time instead of
  81 %, and the middle's own texts 88.6 % instead of 4.1 %. At the spec rate
  S4 delivers 93.9 and 93.9 % against 93.9 and 93.8 %, and the strip at half
  the rate 94.3 % against 94.0 %, with no moves. The D42 suites are within
  their spread.
- **At the cap a holder kept its own deliveries out.** A DATA to a sleepy
  node in its window goes ahead of the whole send queue, but entered it
  only when the bucket held everything queued as well. Let in on its own
  frame (commit `b00e929`), the domain at three times the rate delivers
  54.8 and 52.8 % instead of 33.4 and 31.2 %, 7.8 and 7.4 texts a second
  where twice the rate carries 9.1: past its capacity it carries close to
  its capacity, and what it cannot carry waits at the sleepy node that sent
  it. The loaded suites gain, the rings 5 points and mesh-burst at 64 times
  its load 18; below the cap it changes nothing.
- **The brief's other questions.** All 13 channels carry relays in the
  strip's centre, 12 to 37 each. The per-relay duty cap binds long before a
  channel's airtime: the centre's channels read 4 to 6 % busy where its
  relays are at their cap. At the cap the centre refuses its own users'
  texts first, by design (spec "Receiving DATA"), while transit passes in
  hop places: on main the centre delivers 4 % of its own texts and 28 % of
  those crossing it. Messages of more than 40 hand-overs are a few in ten
  thousand and take 5 to 10 % of the airtime; a lower hop cap would save
  that much at most. Relay density for capacity was not measured.

## What was built for the measurement

Read-only instruments, each its own commit, that change nothing the core
does: per relay, in the per-minute series of `bark-sim --series`, hand-overs
whose attempts all went unanswered (a new trace of the core,
`HandOverFailed`) and of those the ones whose receiver had the message by
then; copies of a message taken that were not retransmissions (a new trace,
`Copy`), merged or searching on; returns taken; deliveries made, failed and
late; the airtime of DATA to the node it is addressed to; frames in hop
places, waiting for a ban, and returns in the send queue; the home channel
and its busy estimate next to the quietest channel's. The per-message table
gains where the sender and the destination are.

## What was run

S4 at the spec rate (×1) and twice it (×2), seeds 1 and 2, for two settled
hours (9900 s, settled at 2700 s, texts counted from 2700 to 9300 s), and
three times it (×3) where said. The strip (`scenarios/strip.toml`) at the
spec rate and at half of it, seed 1 only, one settled hour as the scenario
has it. The suites of D42: standard, S1, mesh-burst, mesh-spread and the
ring at 10 seeds, S1b, S2 and the corridor at 60; loaded, the corridor at 1
and 0.5 s per text, the ring at 1 and 2 s, mesh-burst at 64 times, 30 seeds
each; paired by seed, ± one standard error. "Delivered" is the share of
reachable texts sent in the window that arrived. One S4 or strip run at a
time, 22 to 40 minutes and 4 GB for S4, an hour and 12 GB for the strip.
Runs are deterministic: a build that differs only in an instrument repeats
a run to the frame.

## What the traces show

### The slide

Main at ×2, delivered by ten minutes of sending:

| Sent from, s | 2700 | 3300 | 3900 | 4500 | 5100 | 5700 | 6300 | 6900 | 7500 | 8100 | 8700 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Seed 1 | 91 | 90 | 87 | 81 | 79 | 85 | 70 | 59 | 52 | 49 | 43 |
| Seed 2 | 91 | 90 | 91 | 90 | 89 | 90 | 87 | 88 | 88 | 88 | 86 |

Seed 1 slides from 4 000 s on and falls under 60 % after an hour of load;
seed 2 loses 5 points over the two hours. In both the losses that grow are
texts that reach their holder and expire in its delivery queue, then texts
that expire at their own sleepy origin, whose anchor at its cap takes no
fresh work.

### What grows with the load

The first half hour of the window, summed over the relays:

| | ×2 main, seed 1 | ×2 read alike, seed 1 | ×2 main, seed 2 | ×2 read alike, seed 2 |
|---|---|---|---|---|
| Texts entering the network | 17 281 | 17 463 | 17 058 | 17 196 |
| Hand-overs that succeeded | 100 727 | 88 302 | 109 361 | 80 888 |
| Hand-overs failed, attempts unanswered | 23 719 | 15 142 | 26 103 | 12 808 |
| Of those, the receiver had the message | 43 % | 45 % | 45 % | 39 % |
| Copies taken that were not retransmissions | 12 112 | 9 721 | 15 581 | 5 547 |
| Refusals for the bucket | 5 710 | 832 | 6 376 | 275 |
| Relays' airtime, s | 27 165 | 23 752 | 28 701 | 22 212 |
| Channel moves | 207 | 8 | 263 | 6 |

A hand-over whose ACKs are lost looks failed while its receiver has the
message; once the sender goes another way the message has two copies, and a
copy that reaches a relay that has already passed the message on is
searched on from there
(spec "Receiving DATA", "Why the search finds a path").

Where the frames addressed to relays went, per cent, the same half hour,
seed 1:

| | received | lost, receiver free | receiver busy | receiver deaf | receiver on another channel |
|---|---|---|---|---|---|
| main, relays' DATA | 58.0 | 18.1 | 11.5 | 8.2 | 4.2 |
| read alike, relays' DATA | 63.2 | 19.0 | 10.4 | 7.3 | 0.2 |
| main, sleepy nodes' DATA | 43.6 | 29.5 | 14.6 | 7.3 | 5.0 |
| read alike, sleepy nodes' DATA | 47.6 | 32.7 | 13.2 | 6.3 | 0.2 |

"Lost, receiver free" is the link. The frames sent to a channel the
receiver had left, 4 to 5 % of all, go with the moves.

### The moves

The rule moves a relay whose home is busier than the quietest channel by
more than 5 points for 10 minutes, after a random wait of up to 5 more
(spec "Channel moves"). The estimates averaged over the relays, main, ×2,
seed 1:

| s | 2400 | 3600 | 4800 | 6000 | 7200 | 8400 | 9600 |
|---|---|---|---|---|---|---|---|
| Home, as main reads it | 0.067 | 0.074 | 0.084 | 0.096 | 0.121 | 0.114 | 0.105 |
| Quietest channel | 0.020 | 0.029 | 0.031 | 0.034 | 0.032 | 0.036 | 0.037 |
| Relays over the threshold, of 269 | 120 | 100 | 123 | 154 | 167 | 165 | 134 |

Read alike, the home channel reads 0.029 to 0.034 and the quietest 0.017
to 0.019, and 0 to 6 relays are over the threshold.

The home reading was a 1 when the RSSI read above the threshold, else the
share of the last second spent receiving packets addressed to others (spec
"Busy estimate" as it stood, roadmap D24). A relay decodes packets down to
its sensitivity, about 8 dB below the busy threshold of 6 dB over the noise
floor, and where each relay hears dozens of others most of what it decodes
is faint. No reading of another channel counts them, so the home channel
read three to four times as busy as any other, the more so the more
traffic. Moving did not help: the new channel carried as much faint
traffic, and its reading was soon as high.

### The loop

1. Load raises every busy relay's home reading above the quietest
   channel's by more than the threshold; most relays move every 15 to 25
   minutes.
2. Each move sends its neighbours' frames to a channel it has left, for up
   to a rotation. Hand-overs fail; silence bans the neighbour, the message
   waits and is offered again, and after a second failure goes another way
   (spec "Bans").
3. Failures whose receiver had the message split it, and the copies search
   on. Frames per text grow.
4. More frames: receivers busy and deaf more often, more ACKs lost, more
   failures, and higher home readings. Back to 1.
5. The centre's relays reach their cap and take no fresh work; texts expire
   at their sleepy origins, and deliveries at capped holders miss their
   windows.

### The strip's centre

The strip at the spec rate, one settled hour, by domain, west, middle,
east:

| | main | read alike |
|---|---|---|
| Relays' time at the cap | 13, 81, 28 % | 0, 5, 0.3 % |
| Relays' duty cycle, mean | 3.8, 9.4, 5.0 % | 2.8, 4.9, 3.0 % |
| Channel moves in the hour | 431, 844, 574 | 2, 16, 2 |
| First hand-overs of texts born in the domain | 15 634, 5 638, 13 430 | 17 407, 21 225, 17 897 |
| Refusals for the bucket | 59 000, 409 000, 148 000 | 14, 18 000, 650 |

Texts by where they start and end, delivered:

| | main | read alike |
|---|---|---|
| Within the middle domain | 4.1 % | 88.6 % |
| From or to the middle domain | 13.5 % | 90.5 % |
| From one end to the other, across the middle | 28.1 % | 93.6 % |
| Within an end domain | 64.3 % | 93.8 % |

**Transit against local.** At its cap a relay takes no fresh work and
takes a message already in the network into a hop place whatever its
bucket holds (spec "Receiving DATA", "Send queue"): overload stops new
messages where they enter. In the centre that falls on the centre's own
users. On main the middle domain's relays started 5 638 texts of their
sleepy nodes in the hour against 21 225 read alike, and the centre's own
texts were delivered 4 % of the time while texts crossing it were
delivered 28 %. This is the rule doing what it says, and it was not what
broke: read alike, the centre's relays are at the cap 5 % of the time and
carry their own texts and the transit alike.

**The channels.** Every one of the 13 channels carries relays of the
middle domain, 12 to 37 on main and 15 to 27 read alike, the busiest read
alike 5.9 % busy against 3.6 % for the quietest. The channels are used.

**What binds.** The duty cap of a relay, not a channel's airtime: read
alike, the middle domain's channels are 4 to 6 % busy while 5 % of its
relay time is at the cap, and at twice the rate in S4 the relays at the cap
are one or two rooftop relays with full neighbour tables, the transit hubs
that greedy routing prefers, each handing over a message every two seconds.

## Ideas

### 1. The home channel read as any other (proposed)

**What it is.** A home reading is the same 0/1 reading as a reading of any
other channel, RSSI above the busy threshold. One taken while a packet that
follows the relay was on the air, a packet addressed to it or the reply to
its own hand-over, is set aside, as D24 has it. The receive-time count
goes. Commit `aed03d0`, with spec "Busy estimate" and a test,
`home_and_away_are_measured_alike`: four faint relays send a frame a
second, two on the home channel and two on another; before, the home
channel read 3.9 % and the other 0, now both 0. The test harness's RSSI
reading now reads the strongest frame on the air rather than a fixed level
(`57ddf0c`). No wire change, no new constant.

**What it changes.** S4, delivered by ten minutes of sending:

| | window | 2700 | 3300 | 3900 | 4500 | 5100 | 5700 | 6300 | 6900 | 7500 | 8100 | 8700 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ×2 main, seed 1 | 71.4 | 91 | 90 | 87 | 81 | 79 | 85 | 70 | 59 | 52 | 49 | 43 |
| ×2 read alike, seed 1 | 92.7 | 92 | 92 | 93 | 93 | 93 | 93 | 93 | 93 | 93 | 93 | 93 |
| ×2 main, seed 2 | 89.1 | 91 | 90 | 91 | 90 | 89 | 90 | 87 | 88 | 88 | 88 | 86 |
| ×2 read alike, seed 2 | 93.1 | 92 | 93 | 94 | 93 | 93 | 93 | 93 | 94 | 93 | 93 | 93 |
| ×1 main, seeds 1, 2 | 93.9, 93.8 | | | | | | | | | | | |
| ×1 read alike, seeds 1, 2 | 93.9, 93.9 | | | | | | | | | | | |

Moves in the two settled hours at ×2: 9 and 6 against 1 191 and 1 161.
Relays' airtime at ×2: 87 800 and 88 700 s against 125 700 and 119 400 s,
for 13 and 1 % more texts taken in. Latency at ×2, p50 and p90: 2.8 and
183 s against 11.8 and 340 s (seed 1). No message lost without a trace in
any run. At ×1, moves 0 and 0 against 131 and 88, airtime 41 600 and
42 900 s against 42 800 and 43 100 s, latency within a second. The strip:
above; at half the rate 94.3 % against 94.0 %, moves 2 against 408 in the
hour, nothing at the cap either way.

**What it cost.** The D42 suites, against main, paired:

| | main | read alike | difference | airtime, s a run |
|---|---|---|---|---|
| S1, S1b, S2, the corridor, mesh-burst, mesh-spread | | | identical | identical |
| the ring, 10 seeds | 0.8978 | 0.8769 | −0.022 ± 0.009 | 1428 → 1419 |
| the corridor at 1 s | 0.9861 | 0.9803 | −0.006 ± 0.004 | 854 → 859 |
| the corridor at 0.5 s | 0.9694 | 0.9668 | −0.003 ± 0.004 | 892 → 890 |
| the ring at 1 s | 0.8522 | 0.8506 | −0.003 ± 0.007 | 1390 → 1379 |
| the ring at 2 s, 30 seeds | 0.8981 | 0.8938 | −0.002 ± 0.011 | 1399 → 1398 |
| mesh-burst at 64 times | 0.8239 | 0.8243 | −0.001 ± 0.006 | 4968 → 4973 |

The ring at 10 seeds is its own first 10 of the 30 at 2 s, where the
difference is −0.2 ± 1.1 points: within the spread.

**What it does not show.** How many packets a real relay decodes below the
busy threshold is not measured; the simulator's links put many there. The
rule's purpose, leaving a channel for a foreign transmitter, is untouched:
one loud enough to read above the threshold counts on every channel alike.
No scenario has a foreign transmitter that starts after boot.

### 2. A delivery needs room for itself only (proposed)

**What it is.** In its node's window a message in the delivery queue
enters the send queue when the regular room has a place and the bucket
holds its own frame and the reply reserve, not the airtime of everything
queued: it goes ahead of all of that (spec "Delivery"). The forwards behind
it wait for the bucket instead. A delivery outside the window, with the
long preamble (D58), still needs the bucket to hold everything else.
Commit `b00e929`, with spec "Duty cycle cap", "Send queue" and
"Delivery queue" and a test, `a_delivery_goes_ahead_of_the_queue_in_the_bucket_too`:
ten forwards waiting for a ban outweigh the bucket, which holds the
delivery and the reserve; before, the delivery stayed out, now its
hand-over starts at once.

**Why.** Against `8704790`, at three times the rate with idea 1, a relay at
the cap half the time or more delivered 14 messages a half hour while 59
expired in its delivery queue, and a third of all texts reached their
holder and expired there.

**What it changes.** S4 at three times the rate, with idea 1:

| | delivered, seeds 1, 2 | texts delivered a second | expired in a delivery queue | expired at the sleepy origin | latency p50, p90, seed 1 |
|---|---|---|---|---|---|
| idea 1 | 33.4, 31.2 % | 4.7, 4.4 | 27 663, 27 037 | 31 929 | 63, 470 s |
| ideas 1 and 2 | 54.8, 52.8 % | 7.8, 7.4 | 8 444, 8 021 | 31 641 | 139, 519 s |

At ×2 92.9 % against 92.7 % (seed 1) and at ×1 94.1 % against 93.9 % (seed
1): below the cap it changes nothing. At ×3 half the relays are at their
cap half the time or more; such a relay delivers 104 messages an hour where
it delivered 25, and lets 27 expire in its delivery queue where it let 106.
The texts that now arrive are the ones that crossed the domain and then
waited, so the median latency rises with them. What still ends undelivered
is the third of the texts that the anchor at its cap never takes in, by the
rule that overload stops new messages where they enter
(spec "Receiving DATA"). One message was lost without a trace in idea 1's seed 2 at ×3, not
traced; the backoff note found every such case in S4 to be two texts with
one 32-bit message ID in the simulator.

**What it cost.** The D42 suites, both ideas against main, paired:

| | main | both | difference | airtime, s a run |
|---|---|---|---|---|
| S1, S1b, S2, the corridor, mesh-burst, mesh-spread | | | identical | identical |
| the ring, 10 seeds | 0.8978 | 0.9497 | +0.051 ± 0.012 | 1428 → 1448 |
| the corridor at 1 s | 0.9861 | 0.9855 | −0.001 ± 0.003 | 854 → 865 |
| the corridor at 0.5 s | 0.9694 | 0.9574 | −0.013 ± 0.006 | 892 → 887 |
| the ring at 1 s | 0.8522 | 0.9057 | +0.055 ± 0.010 | 1390 → 1469 |
| the ring at 2 s, 30 seeds | 0.8981 | 0.9470 | +0.052 ± 0.011 | 1399 → 1458 |
| mesh-burst at 64 times | 0.8239 | 0.9759 | +0.182 ± 0.027 | 4968 → 4826 |

The rings and the loaded mesh deliver to sleepy nodes from relays at their
cap. The corridor at 0.5 s loses 1.3 ± 0.6 points: in 11 of its 30 seeds
the two builds differ by more than two points, either way, and every loss
is a drop for want of a route at the corridor's end relays, the kind the
D58 note found main to have on other seeds; not traced further.

### 3. Measured on `8704790` and not proposed

These were measured on S4 ×2, seeds 1 and 2 where said, against the old
baseline, whose slide was deeper (63.7 and 42.5 %), and not rerun:

- **No moves at all** (the move threshold at 100 %): 89.9 and 89.9 %, as
  idea 1 there (89.9 and 90.0 %). It shows that the moves did the harm.
- **A move announced on every channel**: after a move the relay sends its
  heartbeat on all 13 channels, 154 ms, so every neighbour that listens
  hears the new channel at once. With moves nearly as frequent, 740 in two
  hours, seed 1 delivered 88.5 %, flat: moves made cheap stop the slide too,
  but sleeping nodes still missed the announcement then.
- **Sleepy nodes that listen** (`sleepy_listens`), with main's moves:
  91.1 %, seed 1, through 1 173 moves. Before D58 the moves did their harm
  through the nodes that slept.
- **A copy ends where the message has been**: a DATA of a message the
  relay has a record of, from a neighbour it did not hand it to, only
  ACKed. 86.1 and 85.8 % with main's moves, 90.0 % with idea 1 and 12 % less
  airtime, but 2 058 and 2 643 messages lost without a trace a run, 271 with
  idea 1: the copy ended is sometimes the last, as D35 found. Rejected.
- **A refused message waits for its refuser**, as a failed one does since
  D42, instead of going to its next candidate at once: 29.1 %, seed 1,
  falling to 5 % within half an hour. Going round a relay at its cap is
  what lets the others carry its load. Rejected.

### 4. Giving up on messages of many hand-overs (not built)

By the hand-overs a message made, at ×2, seed 2, the airtime of all
messages sent in the window:

| Hand-overs | main: texts, delivered, share of airtime | read alike |
|---|---|---|
| 0 to 10 | 59 929, 89 %, 77 % | 63 605, 93 %, 92 % |
| 11 to 40 | 3 901, 89 %, 12 % | 623, 93 %, 3 % |
| more than 40 | 434, 71 %, 10.5 % | 87, 25 %, 4.9 % |

Read alike, a few in ten thousand messages make more than 40 hand-overs;
they are searches with no reachable owner, and a quarter of them are
delivered. A hop cap of 40 would save at most 5 % of the airtime and lose
22 texts in two hours: not worth a smaller margin for searches around gaps
(spec "Hop count").

## What this does not show

- Two seeds for S4; one for the strip and for the measurements of idea 3.
- The share of packets a relay decodes below the busy threshold, and the
  threshold, are the simulator's; the bench measures neither yet (spec
  "Implementation notes").
- No foreign transmitter starts after boot in any scenario, so the moves
  the rule is for were not simulated, before or after.
- Relay density for capacity was not measured: read alike, the strip's
  centre carries the spec rate at 5 % of its relays' time at the cap, so
  the question moves to where the cap binds, the rooftop transit hubs, which
  more windowsill relays would not relieve.

## Recommendation

**Understood.** The slide at twice the rate in S4 and the collapse of the
strip at the spec rate are the channel move rule firing at nearly every
busy relay, because the home channel's busy estimate counted packets that
no reading of the other channels can see. The moves send frames to empty
channels, the failed hand-overs split messages, the copies search on, and
the frames raise the readings: a loop. Taking the moves out, by reading
home alike or by not moving at all, takes the slide out in every run
measured, on both baselines. Past the cap, at three times the rate, the
holders' admission kept their own deliveries out of their send queues
while the nodes listened.

**Fixed, if adopted.** Idea 1, the home channel read as any other: one rule,
no wire change, no constant, the D42 suites within their spread. Idea 2, a
delivery in its window needs room for itself only: the same, with the
loaded suites better and the corridor at 0.5 s 1.3 ± 0.6 points worse,
which the note does not explain. Take idea 1 first; it is the cause. Idea 2
is a separate decision about where the cap should fall.

**Not answered.** Relay density for capacity was not measured; read alike,
the cap binds first at the rooftop relays that carry the long hops. At
three times the rate a third of the texts wait at their sleepy origin and
are offered again and again to an anchor at its cap, each offer a full
frame on the busiest channels; that is a question for the edge. A lower
hop cap would save at most a twentieth of the airtime.

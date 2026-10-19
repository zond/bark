# Texts that expire waiting for a node the holder never hears

**Date:** 2026-10-19, roadmap D66. **Status:** measured, awaiting the
decision. Spec references are to `protocol.md`. Every run is S4
(`scenarios/s4-capacity.toml`, with its switch-on, random IDs, clocks,
walkers and dark spells) for two settled hours, 10 500 s, counting the
reachable texts the sleepy nodes sent from 2700 to 9900 s, as D54 did.
The base, named "main" in every table, is main `c0572cb` with this
branch's read-only instruments, which repeat main to the frame: they give
D54's figures, 77.7 % delivered at three times the rate on seed 1 and
72.6 % on seed 2.

D54 found that the largest loss left, and the only one that grows with
the texting rate, is texts that expire in a holder's delivery queue for a
sleepy node the holder did not hear while the text waited: 5.1 % of the
texts at the spec "Capacity and sizing" rate (×1), 8.8 % at three times
it (×3). It tied the rise to the anchors' reception of their nodes'
beats, 76 → 62 %, lost to the anchor's own load. This note traces those
texts one by one, confirms the load mechanism with a control, and
measures five changes.

## Summary

- **Three kinds of texts expire unheard,** by what became of every beat
  of their node while they waited (×1 → ×3, of all texts): the node's
  link to the holder is below sensitivity on average, 2.8 → 4.4 %; the
  node is dark the whole wait, 1.7 → 1.9 %; and the node's beats reach
  the holder's antenna but the holder is busy, receiving another frame,
  transmitting or on another channel, 0.4 → 2.3 %.
- **The anchors' load is the cause of the rise,** confirmed by a control:
  with the sleepy nodes texting at the spec rate and the relays made
  busier by their own applications, the anchors receive 68 % of the
  beats instead of 76 %, and 6.4 % of the texts expire unheard instead of
  5.1 %. Load raises both the busy-holder kind and the
  below-sensitivity kind.
- **The below-sensitivity kind is the edge of coverage, not a wrong
  anchor.** For its static nodes the link to the holder is −123 dBm at
  the median, 4 dB under sensitivity, the node's strongest relay is only
  1.5 dB better, and 5 % have any relay with margin. Just over half of
  these texts are to nodes that walk. Two rules of anchor choice, c and
  d below, change nothing for them and are not kept.
- **Proposed: a stale text stays queued until its place is needed
  (S).** Delivered ×1 93.3 → 96.0 and 93.3 → 96.4 %, ×3 77.7 → 81.6
  and 72.6 → 77.5 % (seeds 1 and 2). The rescued texts, 3 % of all at ×1
  and 6 % at ×3, reach their node about 14 minutes after they reached the
  holder at the median, half an hour at p90, two hours at most.
- **Beats on a fixed interval, with the holder sending in the windows of
  beats it missed (a),** recover most of the busy-holder kind (×3: 8.8 →
  6.4 and 8.9 → 6.2 % expire unheard), but the deliveries to nodes that
  are dark or out of reach cost the capped anchors as much: delivered
  −0.5 and −0.2 points at ×3, +0.1 at ×1. Not kept.

## What was run

Seeds 1 and 2 for main, S and a at ×3 and for main and S at ×1; seed 1
for the rest; one run at a time, 4 GB and 7 to 18 minutes each. Each
change is built on the instruments alone, so that each is measured
against main by itself:

| Run | What |
|---|---|
| main | `c0572cb` with the instruments (commits 1 and 7) |
| S | a stale text stays queued until its place is needed (commit 2) |
| a | beats on a fixed interval per ID, the holder sending in the windows of beats it missed (commit 3) |
| c | a sleeping node leaves an anchor it hears seldom (commit 4) |
| d | the first choice waits a rotation more for a relay with margin (commit 6) |
| c + d | both |
| busy | main; the sleepy nodes text at ×1, and every relay's own application sends a 100-byte text to a random relay every 25.3 s, twice as many texts as the sleepy nodes send |

**The instruments** (commits 1 and 7; runs repeat to the frame with and
without them). For each text, while it waits in its holder's delivery
queue, each beat of its node and what became of it at the holder:
received; the node dark, so the beat reached nobody; below sensitivity at
the holder on average, or no link at all; the holder resting on another
channel when the beat began; lost in a fade; lost to another frame
(capture, interference, or the holder demodulating a corrupted frame);
lost to the holder transmitting or turning its radio round; lost to the
holder checking a channel or retuning; skipped by the node's
listen-before-talk; or an anchorless beat, which opens no window. Also
whether another relay received the beats the holder missed, whether the
holder knew the node and was its anchor when the text arrived, the DATA
it sent with the long preamble, and, for a node that does not walk, its
link to the holder and to its strongest relay. A simulator bug found on
the way, a periodic flow after a Poisson source running the wrong flow,
is commit 5.

## The trace

Texts that expired in a queue with no beat of their node heard by the
holder while they waited, main, seed 1, of all texts:

| | ×1 | busy | ×3 |
|---|---|---|---|
| Delivered | 93.3 % | 91.1 % | 77.7 % |
| Expired unheard | 5.06 % | 6.44 % | 8.83 % |
| ... every beat below sensitivity at the holder | 2.78 % | 3.57 % | 4.38 % |
| ... the node dark the whole wait | 1.73 % | 1.77 % | 1.86 % |
| ... every beat lost to the holder's load | 0.40 % | 0.88 % | 2.28 % |
| ... mixed, or lost in fades | 0.15 % | 0.23 % | 0.31 % |
| Beats of all sleepy nodes received by their anchor | 76 % | 68 % | 62 % |
| ... lost while it received another frame | 5 % | 10 % | 14 % |
| ... lost while it was transmitting, checking, retuning | 2 % | 5 % | 7 % |
| ... lost with the anchor free: a weak link or out of reach | 13 % | 13 % | 13 % |
| ... lost on another channel | 3 % | 3 % | 4 % |
| Relays' mean duty cycle | 2.1 % | 4.7 % | 7.2 % |
| Anchor changes per sleepy node-hour | 0.36 | 0.38 | 0.41 |

Seed 2 gives the same total within 0.1 point at ×1 (5.06 % expired
unheard) and ×3 (8.88 %).

The beats of the expired texts' nodes, ×1 (×3): 3.4 (3.2) a text while
it waited; 58 (54) % below sensitivity at the holder, 34 (26) % sent
while the node was dark, 5 (4) % with the holder on another channel, 1
(11) % lost to another frame, 0 (3) % to the holder transmitting, 1 (1) %
in a fade, none skipped by listen-before-talk. The holder was the node's
anchor for 98 (93) % of these texts and knew the node when the text
arrived for 67 (70) %. It tried the long preamble at once for 64 (26) %:
less at ×3, where the bucket seldom has room for it.

**The busy holder.** Every beat of the text's node reached the holder's
antenna and every one was lost to the holder's own work: 0.40 % of the
texts at ×1, 2.28 % at ×3, 0.88 % in the busy control. The holder knew
82 to 88 % of these nodes. This is the loss D54's arithmetic predicted, a
text waiting about three beats at 62 % received.

**The edge of coverage.** Every beat arrived at the holder below
sensitivity on average: 2.78 % of the texts at ×1, 4.38 % at ×3. The
holder was the node's anchor in 96 and 87 %, and another relay received
one of the missed beats for 63 and 55 % of these texts. Just over half
are to nodes that walk: at ×1, 545 of 979, 155 walking when the text was
sent and the rest at a stop. For the others, static, the link to the
holder is −127, −123 and −120 dBm at p10, p50 and p90, against the −119
dBm sensitivity; the node's strongest relay is 1.5 dB stronger than the
holder at the median, and 5 % of these nodes have any relay with margin,
4.1 dB over the noise floor (spec "Next hop", roadmap D38). These nodes
have no good relay: their anchor hears a beat in a fade, and the other
relays that hear one are as weak. That this kind also grows with load
(2.8 → 3.6 % in the busy control) follows: a link that gets a beat
through only in a fade loses that beat as well when the holder is busy,
and for these nodes the few beats that could be caught are all there
is.

**The dark node.** No beat of the node reached anyone: 1.7 to 1.9 % at
every load, from the five minutes an hour each node is dark in S4. A
spell that starts while a text waits outlasts the queue's ten minutes.

**The mechanism, confirmed.** In the busy control the sleepy nodes,
their walks, their dark spells and their texts are those of ×1; only the
relays carry more, 4.7 % duty cycle against 2.1 %. The anchors receive
68 % of the beats against 76 %, the difference lost while receiving other
frames (10 % against 5 %) and transmitting (5 % against 2 %), and 6.44 %
of the texts expire unheard against 5.06 %. The anchors' load alone
raises the loss.

## The changes

All against main. Delivered, and D54's breakdown as shares of the
reachable texts: refused at the origin and given back to the
application; aged at the origin; aged in transit; expired in a queue,
the node unheard or heard; still queued or on the way at the end of the
run. Seed in brackets.

| | Delivered | Refused at origin | Aged at origin | Aged in transit | Queue, unheard | Queue, heard | Still held | Latency p50, p90 | Airtime |
|---|---|---|---|---|---|---|---|---|---|
| main ×1 (1) | 93.28 % | 0 | 1.26 % | 0.09 % | 5.06 % | 0.30 % | 0.01 % | 1, 170 s | 87 400 s |
| main ×1 (2) | 93.30 % | 0 | 1.21 % | 0.12 % | 5.06 % | 0.30 % | 0.01 % | 2, 172 s | 90 400 s |
| S ×1 (1) | 95.99 % | 0 | 1.25 % | 0.12 % | 0 | 0 | 2.63 % | 2, 213 s | 90 400 s |
| S ×1 (2) | 96.42 % | 0 | 1.13 % | 0.12 % | 0 | 0 | 2.33 % | 2, 228 s | 90 300 s |
| a ×1 (1) | 93.42 % | 0 | 1.30 % | 0.10 % | 4.91 % | 0.26 % | 0.02 % | 1, 168 s | 88 200 s |
| c ×1 (1) | 93.10 % | 0 | 1.27 % | 0.11 % | 5.16 % | 0.33 % | 0.03 % | 1, 159 s | 88 200 s |
| d ×1 (1) | 93.23 % | 0 | 1.27 % | 0.09 % | 5.09 % | 0.28 % | 0.03 % | 1, 167 s | 85 900 s |
| c + d ×1 (1) | 93.25 % | 0 | 1.17 % | 0.09 % | 5.12 % | 0.36 % | 0.02 % | 1, 162 s | 85 900 s |
| main ×3 (1) | 77.72 % | 7.29 % | 5.37 % | 0.10 % | 8.83 % | 0.40 % | 0.29 % | 81, 438 s | 265 500 s |
| main ×3 (2) | 72.62 % | 11.26 % | 6.22 % | 0.16 % | 8.88 % | 0.36 % | 0.49 % | 134, 498 s | 280 000 s |
| S ×3 (1) | 81.56 % | 8.05 % | 5.90 % | 0.12 % | 0.92 % | 0.07 % | 3.37 % | 109, 595 s | 269 700 s |
| S ×3 (2) | 77.46 % | 11.77 % | 6.34 % | 0.18 % | 0.79 % | 0.07 % | 3.40 % | 158, 656 s | 282 800 s |
| a ×3 (1) | 77.25 % | 9.52 % | 6.10 % | 0.10 % | 6.44 % | 0.27 % | 0.31 % | 88, 397 s | 270 900 s |
| a ×3 (2) | 72.38 % | 13.60 % | 6.95 % | 0.18 % | 6.22 % | 0.21 % | 0.46 % | 125, 449 s | 284 700 s |
| c ×3 (1) | 76.01 % | 8.49 % | 5.76 % | 0.09 % | 9.00 % | 0.36 % | 0.29 % | 89, 449 s | 266 100 s |
| c ×3 (2) | 72.41 % | 11.41 % | 6.04 % | 0.16 % | 9.13 % | 0.35 % | 0.49 % | 128, 495 s | 279 600 s |
| d ×3 (1) | 74.29 % | 9.67 % | 6.19 % | 0.13 % | 8.94 % | 0.38 % | 0.40 % | 96, 456 s | 271 500 s |
| d ×3 (2) | 73.28 % | 11.16 % | 5.88 % | 0.17 % | 8.68 % | 0.41 % | 0.43 % | 124, 489 s | 279 500 s |
| c + d ×3 (1) | 76.25 % | 8.53 % | 5.44 % | 0.10 % | 8.95 % | 0.40 % | 0.34 % | 88, 446 s | 266 700 s |

Past the cap any change moves delivered at ×3 by a few points either
way: d, which acts only at a node's first choice, gives −3.4 points on
seed 1 and +0.7 on seed 2. A change at ×3 counts here only where both
seeds agree and the mechanism it acts on moves.

### S: a stale text stays queued until its place is needed

**Rule** (commit 2; spec "Delivery queue", and the sentences on a
message's life in "Receiving DATA", "Owner", "Bounds", "Sleepy nodes",
"Relay choice"). A text queued the delivery TTL ago or longer is stale;
it stays, and goes to its node at the node's next beat as any other.
When a text arrives at a full queue, the oldest stale text makes way for
it. A queue full of texts that are not stale refuses with a wait, as
now, so that pushback is unchanged.

| | ×1 (1) | ×1 (2) | ×3 (1) | ×3 (2) |
|---|---|---|---|---|
| Delivered past the TTL in the queue, of all texts | 2.79 % | 3.05 % | 5.52 % | 5.72 % |
| ... time in the queue at collection, p50, p90, max | 871, 1977, 6514 s | 872, 1988, 7286 s | 860, 1695, 7278 s | 850, 1636, 7571 s |
| Made way for a newer text | 0 | 0 | 0.99 % | 0.86 % |
| Still queued at the end of the run | 2.63 % | 2.33 % | 3.37 % | 3.40 % |
| Texts queued, all relays, every 10 minutes (main) | 747–1637 (281–338) | 728–1533 (276–345) | 3402–5815 (1718–2031) | 3290–5704 (1798–1961) |
| Most at one relay (main) | 11–24 (5–9) | 13–24 (5–9) | 41 (30–41) | 39–41 (30–41) |
| Refusals for a full queue in the run (main) | 0 (0) | 0 (0) | 29 (57) | 134 (160) |
| ... at the relay that refused most (main) | 0 (0) | 0 (0) | 13 (28) | 87 (124) |

At ×1 no queue fills and nothing makes way; the queues hold four to five
times what they held and still grow at the end of the run toward the 41
places. At ×3 the busiest relays' queues are full of stale texts and each
arrival evicts the oldest; refusals for a full queue fall, since a full
queue now refuses only when it holds no stale text. Stale texts never
crowd out fresh ones: by the rule a text that is not stale is refused
only when no stale text is there to make way, and the relay that
refused most refused less than on main. The cost: airtime +3.4 and
−0.2 % at ×1, +1.6 and +1.0 % at ×3, the deliveries of the rescued texts;
at ×3 texts refused at the origin +0.8 and +0.5 points and aged there
+0.5 and +0.1, the holders' deliveries taking the place of fresh work;
latency p90 +40 to +160 s, which is the rescued texts arriving late
instead of not at all.

The rescued texts are for nodes back from the dark, walkers back in
reach of their old holder, and nodes at the edge that get a beat through
in a fade. Half are collected within 14 minutes of reaching the holder,
a tenth after more than half an hour.

**What the DATA carries.** The DATA to the node carries `time_left` as
the text had it when it reached the holder; the queue's wait already
does not count against it (spec "Delivery"), so it cannot go below zero,
stale or not. The destination's application learns from it how long the
text took on its way, not how long it waited for the node, and cannot
tell a stale text from a fresh one. If the person should see that a text
is old, the sending application can put its send time in the payload,
which is the application's business (roadmap D10); the rule needs
nothing on the wire.

D44's bound, a whole life within two TTLs, no longer holds under this
rule: place bounds a text's stay in the queue, not time.

### a: beats on a fixed interval, and delivery in the windows of missed beats

**Rule as built** (commit 3, reverted by commit 10). The brief assumed a
sleepy node's beat time is fixed per ID; on main it is not: each beat
comes a random half to one beat period after the last. The rule made it
fixed, a protocol change: a sleepy node with an anchor beats on a fixed
interval from the last byte of its ID, P/2 + P/2 × b/256 with P = 300 s,
counted from each slot, and listens its window after every slot whether
the beat went, went late or was skipped. The 256 intervals are 0.59 s
apart, more than two beats' airtime, so two nodes whose beats met once
slide apart, as heartbeats do. A holder that has heard one beat knows
the node's later windows: from each slot, a beat's airtime and the drift
of two ±50 ppm clocks since, to the window's length, 3.37 s. It sends in
them as in a window it heard, while it keeps the node; an unanswered
delivery closes that window only. The heard table kept when the last
beat heard started, 40 bytes an entry instead of 32, 2 KiB more. No wire
change; the node listened no more than before (0.633 against 0.628 mA
at ×1).

**What it did.** At ×3 the busy-holder kind fell from 2.28 to 1.01 %, and
all texts expiring unheard from 8.83 to 6.44 % (seed 2: 8.88 to 6.22 %).
But the holders also send in the windows of nodes that are dark or out
of reach: DATA to sleepy nodes that went unanswered rose from 26 300 to
50 200 in the run (seed 2: 24 500 to 47 800), and 7 500 went while the
node was not listening, its schedule moved by a beat the holder missed
after an anchor change or a channel move. That airtime falls on anchors
already at their cap and comes back as texts refused at the origin, +2.2
and +2.3 points, and aged there, +0.7: delivered 77.72 → 77.25 and 72.62
→ 72.38 %, airtime +2.0 and +1.7 %. At ×1 +0.14 point for +0.9 %
airtime.

It moves loss from the queue, where an accepted text is lost, to the
origin, where it is refused with a wait, which is the direction D54
took; but it carries fewer texts, and needs a protocol change and 2 KiB.
Not kept.

**Not built: holding the holder's frames off a predicted beat.** D60
measured that for heartbeats and found nothing. Here the beats lost to
the holder transmitting are 3 % at ×3 and those lost to another frame,
which the holder cannot keep off, 11 %; the windows of a cover both.

### b: the long preamble after missed beats

Not built. A holder that missed a node's beats can reach it at any time
with the long preamble (spec "Duty-cycled receive and the long preamble"),
at 3.6 times a full frame's airtime, three attempts a hand-over. It needs
what a needs, the node in the heard table with its channel, and main
already tries it once, when the text arrives and the bucket has room: for
64 % of the expired texts at ×1 and 26 % at ×3. Lever a reaches the same
nodes with the short preamble when they listen; where a costs as much as
it gains, b costs more.

### c: a sleeping node leaves an anchor it hears seldom

**Rule as built** (commit 4, reverted by commit 9). A sleeping node kept
the last eight watches of its anchor's heartbeat while in reach. With
fewer than half heard it checked the anchor, and that check did not end
on hearing it: at its end the node took the strongest other relay heard
that had margin, else kept the anchor and judged it afresh eight watches
on. The averaged RSSI counts only the frames that arrive, the fades up,
and cannot show a link at sensitivity. An anchor heard nine times in ten
is judged so once in 2300 judgements, one at sensitivity a third of the
time; eight watches are a byte, about 17 minutes.

| | Queue, unheard | ... below sensitivity | ... holder's load | Anchor changes per node-hour | Anchor checks per node-hour | Sleepy-node current |
|---|---|---|---|---|---|---|
| main ×1 (1) | 5.06 % | 2.78 % | 0.40 % | 0.36 | 1.47 | 0.628 mA |
| c ×1 (1) | 5.16 % | 2.87 % | 0.40 % | 0.38 | 1.37 | 0.642 mA |
| main ×3 (1), (2) | 8.83, 8.88 % | 4.38 % | 2.28 % | 0.41, 0.40 | 1.75, 1.63 | 0.711, 0.708 mA |
| c ×3 (1), (2) | 9.00, 9.13 % | 4.35 % | 2.60 % | 0.48, 0.46 | 1.67, 1.68 | 0.740, 0.737 mA |

It does not reach the edge group, which has no relay with margin to go
to, and at ×3 it does harm: an anchor at its cap delays and skips
heartbeats, so its well-reached nodes judge it seldom heard and move,
18 and 16 % more anchor changes, each leaving the mail already sent to
the old address to wait there, for −1.7 and −0.2 points delivered and
4 % more current. The relays did not move more (74 and 70 channel moves
in the run against 58 and 89): no move storm. Not kept.

Two first builds reset or trimmed the count at each check that heard
nothing; a node at the edge hears nothing in most checks, so neither ever
judged its anchor, and both measured as main at ×1 (93.12, 93.41 %).

### d: the first choice waits a rotation more for a relay with margin

**Rule as built** (commit 6, reverted by commit 8). At the end of the
rotation after its first anchor a sleepy node took the strongest relay it
heard only if it heard it over two heartbeats and its averaged RSSI had
margin, 4.1 dB over the noise floor, where a full frame arrives through
the 4 dB fade 99 times in 100 (roadmap D38); else it listened a rotation
more, 195 s, and then took the strongest it heard, margin or not, so that
a node no relay reaches with margin still has an anchor. Asked was that
the node be anchorless meanwhile; it kept its first anchor instead,
since a node without one beats with the long preamble on every channel
and draws answers from every relay that hears it (spec "Sleepy nodes alone").

| | Below sensitivity | Anchor changes per node-hour | Anchorless after the settle, s a node | Sleepy-node current |
|---|---|---|---|---|
| main ×1 (1) | 2.78 % | 0.36 | 3.0 | 0.628 mA |
| d ×1 (1) | 2.96 % | 0.36 | 2.9 | 0.629 mA |
| c + d ×1 (1) | 2.95 % | 0.37 | 2.8 | 0.642 mA |
| main ×3 (1), (2) | 4.38 % | 0.41, 0.40 | 3.7, 4.5 | 0.711, 0.708 mA |
| d ×3 (1), (2) | 4.34 % | 0.41, 0.39 | 3.1, 5.3 | 0.708, 0.706 mA |
| c + d ×3 (1) | 4.31 % | 0.47 | 3.0 | 0.735 mA |

It changes nothing measurable: the nodes it is for have no relay with
margin, and it acts once, at switch-on. The extra rotation costs a node
195 s of listening once, about 0.3 mAh. Not kept, for want of an effect.

### The relay's side

Asked: should a relay require the same, a link with margin, before it
takes mail for a node? The spec leaves it no choice to make. The address
is the anchor's position and the node's ID (spec "Addresses"); a text for
the node goes to the owner of that position (spec "Owner"), which was the
node's anchor for 87 to 98 % of the expired texts here, and is queued
there (spec "Delivery queue"). Refusing it would lose it earlier, at the
sender with a wait, for a node no other relay reaches better: 95 % of
the static edge nodes have no relay with margin. Nor can the relay tell
the node: it sends a sleepy node nothing but deliveries and the ACKs to
its DATA, and its heartbeat is for every node.

What the relay knows and the node does not is its own noise floor under
load: the node judges a relay's margin by its own floor, and a relay
whose floor is raised by the frames around it hears the node worse than
the node hears it. A relay could tell a node whose beats arrive under its
margin, with a bit in the ACK to the node's own DATA, so that the node
checks its anchor. The cost is a bit of the ACK, if one is spare, and the
node would learn it only when it sends, a few times an hour. Not built:
the edge nodes' links are under sensitivity both ways, and the rules of
choice that would act on such a bit, c and d, change nothing for them.

## What this does not show

- Two seeds for main, S and a, one for the rest; at ×3 a change moves
  delivered by a few points either way (d above).
- The loaded suites were not run for S.
- Static links are symmetric in the simulator; a relay's raised noise
  floor under load is modelled only as the frames it receives.
- The walkers' share of the edge group was split from the static nodes
  by whether the node has a static link row, not traced walk by walk.
- The application never retries a refused text and never replies, as in
  D54, and nothing measures what a person makes of a text two hours late.
- At ×1 the queues under S were still filling at the end of the run; a
  longer run would show the steady state, with some queues full of stale
  texts as at ×3.

## Recommendation

**Understood.** The rise of the texts that expire unheard with load is
the anchors' own load, confirmed by the control: their beat reception
falls with their duty cycle whatever the sleepy nodes do. What expires
unheard is, at every load, for three kinds of node: those at the edge of
coverage, which no rule of anchor choice helps, those dark the whole
wait, and those whose beats the busy holder misses.

**Proposed** (commit 2, for the user to take or leave): **S, a stale text
stays queued until its place is needed.** +2.7 and +3.1 points at ×1,
+3.8 and +4.8 at ×3, no wire change, pushback unchanged, fewer refusals
for a full queue. Its costs: some rescued texts arrive very late (p90
half an hour, up to two hours in the queue), with nothing on the wire to
say so; a little airtime; and at ×3 slightly more refused at the origin.

**Not proposed:** a, fixed beats and delivery in missed windows (fewer
delivered at ×3 on both seeds, a protocol change and 2 KiB); b, the long
preamble after missed beats (not built, dominated by a); c, leaving an
anchor heard seldom (anchor churn at ×3, nothing at the edge); d, waiting
a rotation for a relay with margin (no effect); a relay-side margin bit
(not built). Their code is reverted (commits 8 to 10).

**Left:** the edge of coverage is a question of where relays stand, not
of the protocol; the dark node's mail is what S keeps.

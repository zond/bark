# What slides a loaded domain into collapse, from the sender's end

**Date:** 2026-10-18 and 19, roadmap D54. Spec references are to
`protocol.md`. Three baselines, named in every table: main `8704790`
(D52), on which the ideas were first built and traced one at a time;
main `4ccbaf4` (D58), on which ideas 1 to 3 were measured together
against main; and main `1419f25` (D55, D56), this branch's base, on which
the refusal's wait and the sender's side were measured at three times
the texting rate, where the cap still binds. D55 and D56, adopted while
this note was written, are ideas 1 and 2 below found independently; the
branch carries them as main does, and its own proposals are ideas 3 and
5.

The question was the sender's and the application's end of the collapse:
what a node and its application should do at the edge of the network's
capacity. The traces led first to the busy estimate of the home channel,
then to the last hand-over of a message, and then back to the sender,
where the user's direction is that a text the network cannot carry is
refused at its origin, at once and with a time to try again, so that
nothing accepted is later dropped by age.

## Summary

- **The slide of S4 at twice the texting rate, and the strip's collapse
  at the spec rate, were the channel-move rule**, fed by a home busy
  estimate that read its own channel about three times as busy as any
  other: under load every relay saw a quieter channel elsewhere, moved
  every twenty to thirty minutes, and stranded its sleepy nodes and
  neighbours each time. Read as every other channel is (idea 1, now D55),
  relays stop moving, S4 at twice the rate holds 90 % and more for two
  hours, and the strip at the spec rate carries 92 % from every domain
  against 25 %.
- **Almost nothing is lost in transit; the losses are at the two ends.**
  Texts expire in a holder's delivery queue (8 % at the spec rate, 31 to
  36 % in the collapse) and at their sender (up to a third at three
  times the rate), and a text aged at its sender cost 27 frames of
  offers first: 29 % of all the texts' airtime at three times the rate on
  `1419f25`.
- **At three times the rate**, on main `1419f25`: 55.3 % delivered,
  33.4 % aged at their sender, 1.4 % aged in transit, 9.2 % in delivery
  queues. A sleepy node that doubles its wait after each silence of its
  anchor (idea 3) delivers 66.7 %. A refusal that says how long to wait,
  with the refused senders in line and the refused text given back to
  its application (idea 5, the user's design in the form that works),
  together with idea 3: 77.3 % delivered, 7.5 % refused at the origin
  and given up by the application, 5.6 % aged at the sender (after
  silence, not refusal), 0.1 % aged in transit, 9.3 % in delivery
  queues; latency p50 85 s against 174 s; airtime −18 %.
- **The pushback propagated hop by hop, refusing forwards too, stalls
  the network**: 3.2 % delivered at three times the rate, and in the
  loaded ring 34 points less; the refusals keep each other up around
  loops of relays, the herd D45 found. Refusing only fresh work for the
  way on, where the origin's anchor learns the pushback from its next
  hop, works.
- **The wait needs a byte, not three bits**: told waits at three times
  the rate run to 157 s, a quarter are from 40 to 77.5 s, part of them
  past the 61 s three bits on the ban's doubling scale reach, and the
  places in line, one ban apart, need a linear scale to stay apart. A
  NACK of 14 bytes costs the ACK's 14.4 ms at SF5 and carries a byte
  that counts bans of 1.56 s, 398 s in all; measured first in units of
  2.5 s, the unit of `time_left`, it now counts bans so that no two
  places in line are told the same wait (see the follow-up below).
- **The age signal** DATA carries to the destination's application leads
  the slide by 20 to 40 minutes over the whole domain, but a node sees
  two texts an hour and needs about 50 to tell the plateau from the
  run-up. **Fairness**: senders differ by cost, not rate; a tenth of them,
  far from their anchor, make half the sleepy nodes' offers.

Understood: the slide at twice the rate, the collapse of the strip, and
where the airtime goes past the cap. Fixed in the simulator: the slide
and the strip (D55, D56, on main); past the cap, ideas 3 and 5 take
three times the rate from 55 to 77 %, nearly all of what is not
delivered being refused or aged at its origin, not in transit. Not
fixed: texts still age at their sender after silence (5.6 %), refused
texts cost their senders about 20 offers each before the application
gives up, and the delivery queues still lose 9 % to destinations their
holder does not hear.

## What was run

S4 (`scenarios/s4-capacity.toml`, with D52's switch-on, IDs, clocks and
walkers) at the spec "Capacity and sizing" rate (×1) and twice it (×2),
seeds 1 and 2, run for two settled hours instead of one: 10 500 s,
counting the reachable texts sent from 2700 to 9900 s. Three and four
times the rate, seed 1; on `1419f25` three times the rate is where the
ideas of the sender's side were compared. The strip (`scenarios/strip.toml`) at the spec
rate and half of it, one seed each, its usual settled hour (2700 to 5700
s). The standard and loaded suites of D42 (S1, mesh-burst, mesh-spread
and the ring at 10 seeds; S1b, S2 and the corridor at 60; the corridor at
one text per leaf every 1 s and 0.5 s, the ring at 1 s, mesh-burst at 64
times, 30 seeds), paired by seed, ± one standard error. At most two S4
runs or one strip at a time, later one: an S4 run takes 4 GB and 12 to 25
minutes at ×1 and ×2, 45 minutes at ×3 and ×4; a strip 13 GB and an hour.

New read-only instruments (commit 1): the airtime of each message per
minute, split into its origin's DATA, other DATA, ACKs that take it and
ACKs that refuse it; per message when it first left its origin, the time
left its last DATA to the destination carried, and the beats of the
destination its holder heard while it was queued; the fate of each
sleepy node's beat at its anchor; the sleep counters per minute. Runs
repeat their messages to the frame with and without them.

## What the traces show

Main `8704790`, S4 at twice the rate, delivered by ten minutes of
origination and the fate of the rest:

| From, s | 1800 | 2400 | 3000 | 3600 | 4200 | 4800 | 5400 | 6600 | 7800 | 9000 |
|---|---|---|---|---|---|---|---|---|---|---|
| Seed 1 delivered | 86 % | 84 % | 83 % | 82 % | 80 % | 75 % | 71 % | 54 % | 40 % | 35 % |
| Seed 2 delivered | 84 % | 84 % | 79 % | 70 % | 54 % | 42 % | 36 % | 24 % | 25 % | 31 % |
| Seed 2 expired in a delivery queue | 15 % | 15 % | 20 % | 25 % | 33 % | 36 % | 35 % | 33 % | 35 % | 31 % |
| Seed 2 expired at their sender | 1 % | 1 % | 1 % | 5 % | 11 % | 20 % | 25 % | 35 % | 35 % | 30 % |
| Seed 2 expired in transit | 0 | 0 | 0 | 1 % | 2 % | 3 % | 4 % | 8 % | 6 % | 5 % |

On main `4ccbaf4` seed 1 runs at 89 to 91 % for an hour, then slides
(84, 79, 82, 80, 62, 56, 50, 47, 44 % in successive ten minutes); seed 2 runs at 87 to 91 % for the two
hours, drifting down by 4 points.

Airtime a second in the whole domain, `8704790`, seed 2, on the plateau
(2700 to 3900 s) and in the collapse (from 6000 s), against the same run
with relays forbidden to move (`move_threshold_percent = 100`, which
carried 90 % throughout):

| | Plateau | Collapse | No moves |
|---|---|---|---|
| Relays' DATA | 8 to 13 s | 16 to 20 s | 5.2 to 6.0 s |
| Relays' ACKs | 1.1 to 1.6 s | 2.2 to 2.8 s | 0.8 to 0.9 s |
| Sleepy nodes' DATA, their own texts | 2.2 to 3.3 s | 6.5 to 10 s | 1.3 to 2.6 s |
| Sleepy nodes' beats | 1.1 s | 1.1 s | 1.1 s |
| Relays at their cap | 0 to 40 | 90 to 150 | 0 |
| Relay home moves, per ten minutes | 70 to 160 | 70 to 200 | 0 |
| Relays' DATA received by the addressee | 54 to 62 % | 56 to 58 % | 66 to 70 % |
| Sleepy nodes' DATA received by the anchor | 38 to 45 % | 36 to 39 % | 51 to 54 % |
| Sleepy nodes' beats received by the anchor | 60 to 68 % | 51 to 55 % | 75 % |
| Anchored nodes listening off their anchor's channel | 5 to 9 % | 7 to 13 % | 0 |
| Anchored nodes their anchor does not hear | 9 to 13 % | 14 to 17 % | 6 % |

The first thing to change, before any relay is at its cap, is the moves:
none until 30 minutes in, when the busy estimates have run their ten
minutes after switch-on, then 40 to 80 every five minutes. From then
every row of the first column drifts the same way for 20 to 60 minutes,
with no single event, until relays reach their cap and refuse, the sleepy
nodes offer their texts again and again on their anchors' channels, and
those channels lose more beats. The relays at their cap at the onset are
windowsill relays 1.0 to 1.5 km from the centre with 110 to 160 sleepy
nodes in their heard tables.

**The home busy estimate.** The median relay's estimate of its own
channel, and moves per relay and hour from the settle time:

| | Home busy, median | Moves per relay-hour |
|---|---|---|
| ×1, main `8704790` | 3.6 to 3.7 % | 0.25 |
| ×2, main `8704790` | 8.7 to 10.1 % | 2.6 to 2.9 |
| ×2, `8704790`, no moves | 6.7 to 7.3 % | 0 |
| ×2, `8704790`, home read as any channel | 2.0 to 2.1 % | 0.00 to 0.01 |
| ×1, main `4ccbaf4` | 3.7 % | 0.23 |
| ×2, main `4ccbaf4` | 7.8 to 8.1 % | 2.1 to 2.2 |
| ×1, `4ccbaf4`, ideas 1 to 3 | 1.2 % | 0 |
| ×2, `4ccbaf4`, ideas 1 to 3 | 2.2 % | 0.01 |
| ×3, `4ccbaf4`, ideas 1 to 3 | 4.8 % | 0.18 |

Without moves and with the same traffic the old estimate reads 7.0 % at
the median where an RSSI reading reads 2.2 %. A home reading was 1 when
RSSI is above the threshold, as on every channel, and otherwise the share
of the last second spent receiving packets for other nodes. Frames above
the threshold were counted both ways, those below it, which at SF5 the
radio decodes under the noise (spec "Physical layer"), only at home. A
relay at 7 % at home sees channels at 2 % elsewhere, five points apart,
the move threshold (spec "Channel moves"); at the spec rate the readings
are half that, and few relays cross it. D24 tested the home estimate with
a relay's own traffic, which it rightly leaves out, and not with other
nodes' traffic at load.

**Where the loss is** (`8704790`). Texts that reach their owner and
expire in its delivery queue are the largest loss at every rate: 8 % at
the spec rate, 15 to 16 % on the plateau at twice it, 31 to 36 % in the
collapse. Of those on the plateau (seed 1, 2700 to 4800 s), 85 % expired
without their holder hearing one beat of their node in the ten minutes,
and they are concentrated: the 1 % of destinations with the most received
86 % of their texts this way. Those nodes are 455 m from their anchor at
the median against 204 m for nodes that lost none, changed anchor 4.4
times against 1.6, and their own texts took twice the airtime to hand
over: nodes at the edge of an anchor's reach, whose beats the anchor
rarely hears. In the collapse 53 % of the texts that expired had a beat
heard and no delivery tried: the holder was at its cap. Per text, seed 1:

| | Texts | Their sender's DATA frames | Airtime per text | Share of the texts' airtime |
|---|---|---|---|---|
| Plateau, delivered | 82 % | 2.6 | 1.33 s | 75 % |
| Plateau, expired in a delivery queue | 16 % | 2.8 | 1.27 s | 14 % |
| Plateau, expired at their sender | 1.4 % | 34 | 4.4 s | 4 % |
| Plateau, ended at the hop cap | 0.1 % | 2 | 81 s | 5 % |
| Collapse, delivered | 40 % | 5.0 | 2.68 s | 39 % |
| Collapse, expired in a delivery queue | 34 % | 5.1 | 3.06 s | 37 % |
| Collapse, expired at their sender | 23 % | 28 | 2.1 s | 17 % |
| Collapse, expired in transit | 4 % | 18 | 4.8 s | 6 % |

A text that dies at its sender costs its sender 28 to 34 frames: its
anchor refuses it or does not answer, and it offers again every minute
for ten. Nineteen texts in the plateau's 35 minutes ended at the hop cap
and took as much airtime as all the texts that expired at their sender,
81 s each: a search that wanders, at every rate, not studied here.

**The strip** (`8704790`, spec rate, seed 1): 25.5 % delivered over the
settled hour. Its relays move 2.4 times an hour each, the middle domain's
3.8 times in the run against 2.1 and 2.6 at the ends; the middle's relays
use 9.4 % of their duty cycle on average, the ends' 3.7 and 4.8 %. By the
sender's domain: from the middle, 63 % of the texts expire at their
sender and 13 % are delivered; from the ends, 28 and 38 % are delivered
and a quarter expire on the way, mostly in the middle, where 17 % of all
texts end in transit. The beats the anchors hear fall from 74 to 57 %,
and the anchored nodes their anchor does not hear rise to 15 %, as in S4
at twice the rate.

**Fairness** (`8704790`). By rate there is nothing to share: a sleepy
node offers one text at a time and texts at the rate every other does.
By cost, on the plateau at twice the rate: the 1 % of senders whose texts
cost most take 17 % of all the texts' airtime, the costliest 10 % take
40 %; of the sleepy nodes' own offers the costliest 10 % of senders make
49 %, and their texts are delivered 70 % of the time against 80 %
overall. A share per sender at the first hop would cap the nodes at the
edge of coverage, which pay most for each text because their link is
weak, not because they send more. Not built.

**The age signal** (`8704790`). The age a delivered text carries to its
destination, the TTL less its time left, is about 2.5 s a hand-over when
nothing waits, 12.5 s at the median on the plateau. Its mean rises from
25 s to 40 to 50 s in the 20 to 40 minutes before the slide, and the
share aged over a minute from 10 % to 20 to 33 %; in the collapse it is 55
to 70 %. So the signal leads the slide over the whole domain. But each
application sees only its own texts, at twice the rate 1.9 an hour.
Telling a share of 10 % from one of 20 % at two standard errors takes
about 4 × 0.15 × 0.85 / 0.1² ≈ 50 texts, a day, and one aged text says
nothing, as one in ten is aged on the plateau. An application cannot
throttle on the age in time unless it hears far more than its own texts,
which the network does not offer. Not built.

## Idea 1: read the home channel as every other channel (now D55)

**What:** a home reading is the same 0/1 RSSI reading as on every other
channel, set aside when a packet for the relay was on the air at the
time; the time spent receiving other nodes' packets is no longer added
(spec "Busy estimate"; `tables/busy.rs`, `node/relay.rs`; on main as D55). The
rule of D24 that leaves out the relay's own traffic stays.

**What it changes**, S4 on `8704790`, delivered over the two settled
hours, seeds 1 and 2:

| | ×1 | ×2 | ×2 airtime | ×2 latency p50, p90 |
|---|---|---|---|---|
| Main | 91.0, 91.0 % | 61.2, 41.9 % | 196 000, 243 000 s | 203–251, 519–594 s |
| No moves at all | | 89.9, 89.9 % | 92 000, 99 000 s | 144, 370 s |
| Home read as any channel | 91.1, 91.2 % | 90.0, 90.0 % | 89 000, 98 000 s | 142–144, 369–373 s |

At twice the rate delivery stays at 88 to 91 % in every ten minutes of
both seeds for two hours, with 7 moves among the 269 relays in the two
runs. At three times the rate the domain is at its cap and settles at 37
to 43 % in every ten minutes: a level, not a slide.

**What it costs:** nothing measured. On `8704790` the standard suite is
identical but for the ring (+0.8 ± 1.1 points) and the loaded suite moves
within one standard error. On `4ccbaf4` the ring at 10 seeds read −2.2 ±
0.9 points; at 40 seeds −0.3 ± 0.9, with its moves 940 → 879: none. The
corridor at 1 s −0.6 ± 0.4, the rest within ± 0.3.
`a_relay_does_not_flee_its_own_traffic` and
`a_busy_home_moves_and_announces_on_the_old_channel` still pass.

A foreign transmitter is still left: in `scenarios/s3-interferer.toml`
(D59; a handheld on channel 6 a third of the time from 1800 s), ten
seeds on main `c1732b2` against this branch, the relays make 8 moves in
all on both and 3 of 130 end on the transmitter's channel on both;
delivered 94.3 → 93.6 %, the cost of idea 3 seen in S2.

**What it does not show:** a foreign transmitter at load, where every
channel reads busier. Frames below the RSSI threshold do take a
receiver's time and are now counted on no channel, where before they
counted on home only.

## Idea 2: a delivery waits only for its own frame in the bucket (now D56)

**What:** a message in a delivery queue enters the send queue in its
node's window, where it goes first, when the regular room has a place
and the bucket holds its own frame and the reply reserve, not also the
airtime of everything queued behind it (spec "Delivery queue";
`relay/delivery.rs`; on main as D56). D58's delivery outside the window, with
the long preamble when the bucket has room, is unchanged.

**Why:** a holder at its cap that heard its node's beat could not feed
the delivery to the send queue until its bucket held the whole queue as
well, which at the cap it rarely does; the window after the beat passed,
the message waited a beat period, twice, and expired. It is the last
hand-over of a message that has paid for all the others.

**What it changes**, S4 on `8704790`:

| | ×2 main | ×2 main with idea 2 | ×3 idea 1 | ×3 ideas 1, 2 | ×4 idea 1 | ×4 ideas 1, 2 |
|---|---|---|---|---|---|---|
| Delivered | 61.2, 41.9 % | 71.0, 53.4 % | 39.9 % | 61.1 % | 12.0 % | 30.7 % |
| Expired in a delivery queue | 24.8, 30.6 % | 15.5, 14.2 % | 30.4 % | 11.3 % | 26.7 % | 7.7 % |
| Expired at their sender | 11.0, 22.3 % | 11.6, 26.5 % | 26.9 % | 25.4 % | 54.7 % | 55.3 % |
| Airtime | 196 000, 243 000 s | 195 000, 252 000 s | 264 000 s | 257 000 s | 421 000 s | 425 000 s |

Past its capacity the domain with ideas 1 and 2 loses its texts at their
senders instead of after carrying them across. Alone on main at twice
the rate it does not stop the slide, but the collapse holds 59 to 73 %
(seed 1) and 38 to 51 % (seed 2) where main holds 35 to 43 and 24 to 31 %.

**What it costs:** a forward already queued may wait longer for the
bucket behind a delivery that went first, which is what the delivery's
place at the front of the queue already meant. On `4ccbaf4`, against idea
1 alone: standard suite identical but the ring (+7.3 ± 1.2 points);
loaded suite: mesh-burst at 64 times 82 → 98 % (+18.3 ± 2.7 points), the
ring at 1 s +5.8 ± 0.9, the corridor at 1 s +0.5 ± 0.5, at 0.5 s −1.0 ±
0.4. S4 at the spec rate 94.1 % with ideas 1 and 2 against 93.9 % on
main (seed 1).

## Idea 3: a sleepy node doubles its wait after each silence

**What:** a sleepy node whose own message goes unanswered tries its
anchor again a fade (60 s) later doubled for each unanswered hand-over to
it in a row, instead of every fade; the ban after silence already
doubles the same way (spec "A sleepy node's own message",
"Silence is pushback"; `relay/forward.rs`, commit 2).

**What it changes.** On main `8704790` at twice the rate it did not stop
the slide: seed 2 held 84 to 85 % until 60 minutes and slid at 110
instead of 60 (55.8 % against 41.9 % over the two hours), seed 1 slid as
main (58.5 % against 61.2 %). The moves drove that slide; the senders'
offers were one loop of it. With ideas 1 and 2, where the senders' offers
are what remains:

| | ×3, ideas 1, 2 | ×3, with idea 3 | ×4, ideas 1, 2 | ×4, with idea 3 |
|---|---|---|---|---|
| Delivered, `8704790` | 61.1 % | 72.3 % | 30.7 % | 37.2 % |
| Delivered, `4ccbaf4` | 55.0 % | 65.1 % | | 39.0 % |
| Delivered, `1419f25` (main has ideas 1, 2) | 55.3 % | 66.7 % | | |
| Expired at their sender, `4ccbaf4` | 33.8 % | 24.7 % | | 50.1 % |
| Their sender's frames for a text never taken, `4ccbaf4` | 27 | 19 | | 19 |
| Share of the texts' airtime those take, `4ccbaf4` | 29 % | 19 % | | 34 % |
| Airtime, `4ccbaf4` | 330 000 s | 278 000 s | | 379 000 s |

Each silence in a row keeps a sender away twice as long, so a relay at
its cap is offered less the longer it stays there.

**What it costs:** a text whose anchor is silent for want of link, not
of room, waits longer too, and the doubling cannot tell the two apart.
On `4ccbaf4`, against ideas 1 and 2: S4 at the spec rate 94.1 → 93.2 %
(seed 1), at twice it 93.0 → 92.2 %, the texts expiring at their sender
0.4 → 1.2 % and 0.6 → 1.4 %, latency p90 154 → 165 s; S2 −1.0 ± 0.4
points, the rest of the suites within ± 0.8.

## Idea 4: a refusal says how long to wait

**What (the user's idea):** the ACK that refuses carries in its three
spare `aux` bits a step on the doubling scale the ladder already uses,
one ban (1.56 s) doubled up to the bucket's refill (61 s), seven steps of
eight, so the encoding adds no constant. The refuser chooses it from its
own state: for want of room, the time until its bucket holds everything
queued, the frame and its ACK with the reply reserve left, if nothing
else comes; and no sooner than the end of the bans its own neighbours set
it while they hold up everything it would send, which is how a wait told
at the centre passes outward. The refused sender bans the refuser for
that long instead of one ban, and a return or a delivery waits that
long. Built three ways and measured; not on the branch.

| | Baseline | ×3 | ×4 | Corridor at 1 s |
|---|---|---|---|---|
| Blind ladder, as now, idea 1 | `8704790` | 39.9 % | 12.0 % | |
| A: told wait in place of the ladder, idea 1 | `8704790` | 12.2 % | | |
| Blind ladder, ideas 1, 2 | `8704790` | 61.1 % | 30.7 % | 92.5 % |
| B: told wait as a floor under the ladder, ideas 1, 2 | `8704790` | 68.6 % | 34.7 % | 88.2 % (−4.3 ± 1.2) |
| Blind ladder, ideas 1 to 3 | `786b514` | 78.9 % | 37.7 % | 92.5 % |
| C: B, told to a sleepy node's own text only, ideas 1 to 3 | `786b514` | 76.8 % | 38.2 % | 92.3 % |
| Blind ladder, ideas 1 to 3 | `4ccbaf4` | 65.1 % | 39.0 % | 98.6 % |
| C, ideas 1 to 3 | `4ccbaf4` | 68.4 % | 38.7 % | 98.4 % |

A: the sleepy nodes' own DATA rose from 10 to 30 s of airtime a second.
The refuser's estimate is for one sender; every sleepy node of a relay at
its cap was told the same short wait, came back, and found the room
taken by the others. The ladder, which a sender climbs with each refusal
in a row, counts the competition the refuser cannot see. Senders coming
back in step is this, more than any want of jitter: they are refused at
different moments. B: the told wait helps where the ladder is short, but
banning a relay for the told time sends forwards in the loaded corridor
round it. C keeps the told wait from the network and gives it to the
senders only; on `4ccbaf4` a second seed at three times the rate gave
65.7 % against 68.5 % (seed 1: 68.4 against 65.1), so on three bits the
told wait adds nothing to idea 3, which already keeps senders away from a
silent anchor. Idea 5 is the form that works.

## Idea 5: the pushback, told back to the application

**What (the user's design):** a refusal carries a pushback time; a node
sets the pushback it sends from its own load and from the pushback its
next hops told it, so a full centre's wait goes back toward the origin,
where the anchor refuses the text with that wait and the application
holds it; nothing accepted then ages in transit. Per neighbour, so a full
direction does not stall the others, and decaying, so a relay that has
recovered is tried again. Built on main `1419f25` (commit 4), measured
in four forms at three times the rate, seed 1:

- **The wait.** A refusal is a NACK, a new type 0x7 of 14 bytes, the
  ACK's 13 and a byte. At SF5 the 14th byte fits in the 13th's block of
  symbols, so the NACK costs the ACK's 14.4 ms (an assert in `wire.rs`).
  The byte counted whole units of 2.5 s in the runs of this section,
  the unit of `time_left`, so 255 units hold 637 s; it now counts bans
  (follow-up below). Rounding down makes a wait passed from relay to
  relay only shrink. Linear, not geometric: the waits told to senders
  standing in line differ by one ban, 1.56 s, and must stay apart at a
  minute as at a second.
- **The refuser's wait.** For want of room: until its bucket holds
  everything queued, the frame and its ACK with the reply reserve left.
  Each sender refused takes a place in line one ban after the last, the
  refill of what it will bring back. For fresh work whose way on is
  banned: the rest of that ban.
- **What is learned, per neighbour.** The sender bans the refuser for
  the wait it was told: the existing ban, now as long as the refuser
  asks, which ends by itself.
- **The origin.** A sleepy node whose own text is refused gives it back
  to its application with the wait; the simulated application offers it
  again then, and gives it up once it is older than the TTL, or at once
  when the wait would take it past that.

| Form, `1419f25`, ×3 | Delivered | Refused at the origin | Aged at the origin | Aged in transit | Aged in a delivery queue | Airtime of refused and aged texts, share, per text | Latency p50, p90 | Airtime |
|---|---|---|---|---|---|---|---|---|
| Main | 55.3 % | 0 | 33.4 % | 1.4 % | 9.2 % | 29.4 %, 2.03 s | 174, 545 s | 327 000 s |
| PB: three bits, no line, forwards refused too | 18.7 % | 63.8 % | 6.5 % | 4.5 % | 5.0 % | 51.4 %, 3.42 s | 573, 977 s | 549 000 s |
| PB3a: a byte, a line, forwards refused too | 3.2 % | 80.3 % | 4.5 % | 10.6 % | 0.6 % | 34.2 %, 1.39 s | 544, 978 s | 381 000 s |
| PB3b: a byte, a line, only fresh work refused for the way on | 67.6 % | 19.3 % | 2.3 % | 0.2 % | 10.2 % | 19.7 %, 2.11 s | 168, 542 s | 312 000 s |
| PB3b with idea 3 | 77.3 % | 7.5 % | 5.6 % | 0.1 % | 9.3 % | 10.0 %, 1.38 s | 85, 443 s | 266 000 s |
| Idea 3 alone | 66.7 % | 0 | 23.4 % | 0.7 % | 8.8 % | 18.0 %, 1.44 s | 109, 479 s | 274 000 s |

Shares are of the reachable texts the applications had; the
application skipped 3 785 texts due while it held one on main, 4 586
with PB3b and idea 3, of about 132 000. Delivered in number: 56 465 on
main, 78 458 with PB3b and idea 3.

The waits told, as a share of the refusals:

| Wait | ≤ 1 ban | 2.5 s | 5–7.5 s | 10–17.5 s | 20–37.5 s | 40–77.5 s | 80–157.5 s | ≥ 160 s |
|---|---|---|---|---|---|---|---|---|
| PB3a | 0.4 % | 0.8 % | 1.7 % | 3.7 % | 11.5 % | 51.8 % | 30.1 % | 0.7 % |
| PB3b | 4 % | 6 % | 11 % | 19 % | 36 % | 23 % | 0.6 % | 0 |
| PB3b with idea 3 | 7 % | 10 % | 17 % | 21 % | 32 % | 13 % | 0.1 % | 0 |

PB, on three bits, told 1.6 s 9 % of the time, 3.1 s 12 %, 6.2 s 27 %,
12.5 s 45 %, 25 s 5 % and 50 s 2 %.

**What each form shows.** PB told the refuser's own estimate, a few
seconds; every sleepy node of a relay at its cap came back at once, and
the application, offering again after each short wait, sent its text
fifty times. The line spreads them. PB3a refused a forward as well as
fresh work while the forward's way on was banned, so that the wait
travels hop by hop: refused, a forward is set aside and goes round the
pushback, to a neighbour that soon pushes back too, and the refusals
keep each other up. The domain stalls. In the loaded ring at 1 s (30
seeds) a form whose line also grew with each pushback passed on, rounded
up, lost 33 ± 7 points, PB3a 57 ± 4, the ring's relays refusing each
other in turn; PB3b 1.7 ± 0.6. PB3b refuses only fresh work
for the way on: the origin's anchor learns the pushback from its next
hop, which is as far as it travels, and the relays between learn it only
when their own room runs out. Then nothing accepted ages in transit (0.2
%), and refused texts no longer age at their sender; what does age
there, with idea 3 5.6 %, are texts whose anchor did not answer, not
refusals. Idea 3 and PB3b each take ten points, and together twenty-two:
the one quiets the silent anchors, the other the full ones.

**How close to the ideal**, that a text the network cannot carry is
refused at once at its origin and everything accepted arrives. Of what
the network accepted with PB3b and idea 3, 0.1 % aged in transit and 9.3
% in a delivery queue, the second at destinations whose holder does not
hear them (see the traces), a mailbox, not load. Not at once: a refused
text cost its sender 1.4 s of airtime, about 20 offers and their NACKs,
before the application gave it up, because it is told to come back as
soon as its place in line comes, and comes back, at three times the rate,
to find the line longer. Aged at the origin, 5.6 %: silence tells no
time. Those two, 10 % of the texts' airtime, are what is left to the
ideal.

**What it costs**, against main `1419f25`, with idea 3: the standard
suite identical but S2 −0.8 ± 0.3 points (idea 3's) and the ring +0.5
± 0.8; loaded: the corridor at 1 s −1.2 ± 0.5, at 0.5 s −1.3 ± 0.9, the
ring at 1 s −1.2 ± 0.6, mesh-burst at 64 times +0.2 ± 0.2. A sender
refused in the corridors waits its place in line where the ladder let it
try again in 1.6 s. S4 at the spec rate 94.0 → 93.2 % and at twice it
93.0 → 92.0 % (seed 1), the loss idea 3's, texts aged at their sender
0.5 → 1.2 % and 0.6 → 1.5 %, latency unchanged; at those rates the NACK
asks for a wait of a ban or two, and no text is given back for good. The
strip at the spec rate, which D55 and D56 took off its cap: main 92.5 %,
PB3b alone 92.7 %, with idea 3 92.4 %, latency p50 7, 7 and 5 s, airtime
209 000, 209 000 and 197 000 s.

**What it does not show:** what people do with a told wait; the
simulated application offers again at once and gives up at the TTL. A
firmware must pass the give-back to the phone, which the core interface
now names (`DropReason::Pushback`) but no firmware does yet.

### Follow-up: one function, a unit of one ban, and the wait carried by forwards

Three changes after review, each its own commit, measured on `1419f25`.

- **One choice of hop** (commit 6). The way a message would go, for the
  pushback, and the way it goes were two copies of the same choice; now
  one function with a flag. Runs repeat to the frame.
- **Correction.** The binaries measured above as PB3b also told a forward
  refused for want of room no less than the ban on its way on; the
  commit that carried them (commit 4) left that out. Measured again as
  committed (H, units of 2.5 s): 77.2 % at three times the rate, seed 1,
  against 77.3 % with it. The numbers above stand for both forms.
- **The unit** (commit 8). The places in a refuser's line are one ban
  apart, and nothing in the protocol acts sooner than a ban, so the NACK
  counts whole bans, rounded down: 255 bans are 398 s, and a longer wait
  is told as that. Units of 2.5 s merged neighbouring places about a
  third of the time. The range a sender can use is bounded by the
  delivery TTL, the resolution by the ban; a geometric scale in the byte
  would buy range that cannot be used and lose the places the line exists
  for. The waits told hardly reach the end of the byte: at three times
  the rate 39 of 450 000 NACKs asked for 64 bans (100 s) or more (UM,
  seed 2), none in the other runs.
- **The wait carried by forwards** (commit 9, the middle form). A relay
  refuses a forward only for want of room, as before, and tells it no
  less than the rest of the ban on the way that forward would go on; it
  does not refuse it for that ban. The relay before then bans this one
  as long, its queue holds the message as long, and its own refusals tell
  longer waits.

S4 at three times the rate, on `1419f25`, all with idea 3:

| | Seed | Delivered | Refused at the origin | Aged at the origin | Aged in transit | Aged in a delivery queue | Airtime of refused and aged texts, share, per text | Latency p50, p90 | Airtime |
|---|---|---|---|---|---|---|---|---|---|
| Main (no idea 3) | 1 | 55.3 % | 0 | 33.4 % | 1.4 % | 9.2 % | 29.4 %, 2.03 s | 174, 545 s | 327 000 s |
| Main (no idea 3) | 2 | 54.5 % | 0 | 34.5 % | 1.6 % | 8.6 % | 29.9 %, 2.03 s | 191, 565 s | 331 000 s |
| H: units of 2.5 s | 1 | 77.2 % | 7.6 % | 5.6 % | 0.1 % | 9.2 % | 10.1 %, 1.38 s | 85, 444 s | 264 000 s |
| U: units of a ban | 1 | 77.7 % | 7.3 % | 5.4 % | 0.1 % | 9.2 % | 9.7 %, 1.37 s | 81, 438 s | 266 000 s |
| U | 2 | 72.6 % | 11.3 % | 6.2 % | 0.2 % | 9.2 % | 13.2 %, 1.47 s | 134, 498 s | 280 000 s |
| UM: U and the wait carried by forwards | 1 | 76.7 % | 8.0 % | 5.6 % | 0.1 % | 9.3 % | 10.4 %, 1.39 s | 92, 446 s | 268 000 s |
| UM | 2 | 70.8 % | 12.6 % | 6.5 % | 0.2 % | 9.4 % | 14.4 %, 1.50 s | 139, 506 s | 286 000 s |

The waits told, in bans, by the count of the refused DATA (UM, seed 1;
U is within two points in every cell):

| Refused DATA | NACKs | 1 | 2–3 | 4–7 | 8–15 | 16–31 | 32–63 bans |
|---|---|---|---|---|---|---|---|
| A sleepy node's own (count 0) | 268 000 | 11 % | 13 % | 18 % | 23 % | 29 % | 7 % |
| Count 1 | 36 000 | 6 % | 14 % | 20 % | 25 % | 29 % | 6 % |
| Count 2–3 | 37 000 | 2 % | 12 % | 22 % | 28 % | 31 % | 6 % |
| Count 4 and more | 15 000 | 0 | 6 % | 19 % | 29 % | 39 % | 8 % |

**What the middle form shows.** The waits told grow with the count, a
little: the farther from the origin a forward is refused, the longer it
is told to wait. But forwards are refused rarely, a quarter of all
NACKs, because the hop places take a message already in the network
whatever the bucket holds; and a refused forward is set aside and goes
to its next candidate rather than waiting out the ban it learned. So
little of the wait reaches the outer relays, and what does holds their
queues longer: 1.0 and 1.8 points less than U at three times the rate,
texts refused at the origin up 0.7 and 1.3 points, airtime up 1 to 2 %.
It does not herd: in the loaded ring and corridor it is within one
standard error of U (ring at 1 s +0.6 ± 0.8, corridor at 1 s −0.1 ±
0.6, at 0.5 s +0.1 ± 0.8, the ring at 10 seeds +1.8 ± 0.9). The strip
at the spec rate 92.0 % against 92.5 % on main and 92.4 % with the wait
carried in units of 2.5 s. **Not kept**; commit 10 reverts it.

**The unit alone** (U against H): 77.7 against 77.2 % at three times the
rate, seed 1; at the spec rate 93.3 % (H with idea 3: 93.2 %, main 94.0
%) and at twice it 92.2 % (92.0, 93.0 %); in the suites within one
standard error (the corridors −0.8 and −0.9, the ring at 1 s −0.9 ±
0.8). Kept.

The whole of D54 against main `1419f25` in the suites (idea 3, idea 5 in
units of a ban, UM's numbers; U's differ by under a point): S2 −0.8 ±
0.3, the corridor at 1 s −2.2 ± 0.5, at 0.5 s −3.1 ± 1.1, the ring at 1
s −2.0 ± 0.7, mesh-burst at 64 times +0.4 ± 0.3; the rest unchanged.

### Follow-up: the delivery queue's expiry

Asked: let a holder keep a text a full delivery TTL from its arrival,
not count its travel time, so that the queue fills, refuses with a wait
the sender's application hears, and delivery is delayed instead of lost.
That is already the rule (spec "Delivery queue"; `tables/delivery.rs`
expires a text `delivery_ttl` after it was queued, and the DATA to the
sleepy node carries the `time_left` it arrived with), so there is
nothing to build. What the queues do under U, on `1419f25`, seed 1 (seed
2 for ×3 in brackets):

| | ×1 | ×3 |
|---|---|---|
| Expired in a delivery queue | 5.4 % | 9.2 (9.2) % |
| ... the node heard by its holder while the text waited | 0.3 % | 0.4 (0.4) % |
| ... never heard while it waited | 5.1 % | 8.8 (8.9) % |
| Refusals for a full delivery queue, whole run | 0 | 57 (160) |
| Texts queued, all relays, every 20 minutes | 290 to 340 | 1 780 to 2 030 (1 830 to 1 960) |
| Most at one relay, of 41 places | 5 to 9 | 32 to 40 (32 to 39) |
| Time a delivered text waited in the queue, p50, p90, max | 0, 60, 595 s | 0, 294, 596 s (14, 324, 597 s) |

Main `1419f25` at ×3, seed 1: 9.2 %, of it 8.9 % never heard; 5 refusals
for a full queue; 0 to 35 per relay; 37, 350, 595 s.

What expires in a delivery queue is, nineteen times in twenty, a text for
a node its holder never heard while it waited: a node gone, dark or at
the edge of the holder's reach, the edge nodes the traces found, not a
mailbox too small. A longer stay would hold a place for a node that does
not collect. The queues fill at a few relays at three times the rate, a
full one refuses a few dozen to a hundred and sixty texts in two hours,
and every refusal asks for one ban: the line does not apply to a full
delivery queue, which empties when a node collects, not at the bucket's
rate. Not measured further.

Why the share for nodes never heard rises from 5.1 % at the spec rate
to 8.8 % at three times it (U, `1419f25`, seed 1). The instruments count
beats at their anchors by fate over the whole run, not per text, so the
split below is of all beats, not of the beats of the texts that expired;
whether another relay heard those nodes was not recorded.

| Sleepy nodes' beats at their anchor, settled run | ×1 | ×3 |
|---|---|---|
| Received | 76 % | 62 % |
| Lost with the anchor listening and free (weak link, or the node out of reach) | 13 % | 13 % |
| Lost while the anchor received another frame | 5 % | 14 % |
| Lost while the anchor was deaf: transmitting, checking or retuning | 2 % | 7 % |
| Lost while the anchor listened on another channel | 3 % | 4 % |
| Relays' duty cycle, mean | 2.1 % | 7.2 % |
| Expired in a queue, node never heard while the text waited, of all texts | 5.1 % | 8.8 % |
| ... of those, its destination walking when the text was sent | 10 % | 11 % |
| ... its node anchored to the holder at the end of the run | 43 % | 50 % |
| ... its node's distance to its anchor, median | 483 m | 410 m |

The part of beats lost to a weak link or a node out of reach does not
change with load (13 %); what grows is the anchor's own load, beats lost
while it receives other frames (5 → 14 %) or transmits (2 → 7 %), as its
duty cycle goes from 2 to 7 %. A text waits about 2.7 beats of its node
in the ten minutes; at 76 % of beats heard the chance that none is heard
is 0.24^2.7 ≈ 2 %, at 62 % it is 0.38^2.7 ≈ 7 %. That accounts for the
rise from the spec rate to three times it: the nodes at the edge of
reach are lost at every rate, and load adds nodes that are not at the
edge but whose few beats all meet the anchor busy. Half of them are
still anchored to the holder at the end, so a different anchor would
not have helped those. To split the expired texts themselves by what
became of their nodes' beats, and to see whether another relay heard
them, needs a per-text record of beats and a run with it.

## Ideas 1 to 3 together, against main, on `4ccbaf4`

| | Main ×1 | Ideas 1–3 ×1 | Main ×2 | Ideas 1–3 ×2 | Main ×3 | Ideas 1–3 ×3 | Ideas 1–3 ×4 |
|---|---|---|---|---|---|---|---|
| Delivered, seeds 1, 2 | 93.9, 93.8 % | 93.2, 93.4 % | 69.3, 88.7 % | 92.2, 92.2 % | 14.4 % | 65.1 % | 39.0 % |
| Expired at their sender | 0.5, 0.4 % | 1.2, 1.1 % | 11.0, 1.2 % | 1.5, 1.4 % | 53.1 % | 24.7 % | 50.1 % |
| Expired in a delivery queue | 5.5, 5.6 % | 5.5, 5.4 % | 18.0, 9.8 % | 6.2, 6.2 % | 25.4 % | 8.9 % | 7.7 % |
| Latency p50, p90 | 2, 150–152 s | 1–2, 165–174 s | 15–22, 323–384 s | 2–3, 183–184 s | 284, 708 s | 116, 484 s | 266, 633 s |
| Airtime | 90 400, 94 700 s | 83 400, 89 500 s | 224 000, 209 000 s | 152 000, 160 000 s | 396 000 s | 278 000 s | 379 000 s |
| Relay moves per relay-hour | 0.23 | 0 | 2.1 to 2.2 | 0.01 | 3.0 | 0.18 | |

Texts carried, as a multiple of what the spec rate sends: 0.93 at ×1,
1.84 at ×2, 1.95 at ×3, 1.56 at ×4; main carries 1.39 and 1.77 at ×2,
and less after its slide, and 0.43 at ×3.

The strip, seed 1, the settled hour:

| | Spec rate | Half the rate |
|---|---|---|
| Main `4ccbaf4` | 24.6 % | 94.0 % |
| Idea 1 | 91.5 % | |
| Ideas 1–3 | 91.7 % | 93.8 % |

At the spec rate the strip with the three changes delivers 92.1, 91.0 and
92.3 % of the texts sent from its west, middle and east domains, where
main delivers 39, 10 and 28 %: from the middle, main lets 72 % of the
texts expire at their sender. Its relays moved 33 times in the run
against 1849 on main, and the middle domain's relays use 4.7 % of their
duty cycle on average against 9.4 %. Latency p50, p90 6, 196 s against
170, 553 s; airtime 203 000 s against 332 000 s. What is still lost is
lost in delivery queues, 6.5 % of the texts, as at the edge of coverage
in S4. The strip did not collapse for want of capacity in its centre: the
moves made the centre's relays reach their cap, and idea 1 alone carries
the strip as well as all three (91.5 %, its losses 1.0 % at the sender
and 7.4 % in delivery queues). At half the rate the strip carries 94 %
either way.

## What this does not show

- One seed at three and four times the rate and of each strip run, two
  at the spec rate and twice it on the earlier baselines and one on
  `1419f25`. The S4 runs repeat to the frame.
- No foreign transmitter at load, the one thing the move rule is for.
- The simulated application never retries, never replies, and texts at
  a fixed rate whatever it sees; what people do when their texts are
  slow is not modelled, so nothing here measures an application that
  throttles, on the age of its texts or on a told wait.
- The nodes at the edge of a relay's reach, whose beats their anchor
  rarely hears, lose most of their mail at every rate; this note found
  them and did not study them. Nor the searches that end at the hop cap.

## Recommendation

**Understood.**

1. The slide at twice the rate, in every version measured since D24, and
   the strip's collapse at the spec rate, were the move rule fed by a
   home reading that counted others' frames twice. Main has fixed both
   (D55, D56); this note's measurements of the same two rules agree.
2. Past the cap nothing much is lost in transit. Texts age at their
   sender, offered again every minute to an anchor that refuses or does
   not answer, and at a holder that could not deliver them; each costs
   about what a delivered text costs.
3. A wait told by the refuser from its own state is optimistic: every
   sender refused at once comes back at once. It works when the refused
   senders stand in line, one ban apart, which needs the byte.
4. A pushback passed on hop by hop, by refusing forwards, keeps itself up
   around loops of relays and stalls the network. Carried by the
   refusals of forwards that happen anyway, for want of room, it does
   not herd, but it reaches the outer relays little, since forwards are
   refused rarely and go round, and costs a point or two. Passed from the
   origin's anchor's next hop to the anchor, and from there to the
   application, it works.
5. The age at delivery leads the slide over the whole domain but reaches
   each application too rarely to act on.

**Proposed** (commits 2, 4 and 8, for the user to take or leave):

- **Idea 3, the doubling after silence.** At three times the rate on
  `1419f25` +11.4 points and −16 % airtime; its cost, about a point at the
  spec rate and twice it on `4ccbaf4` and S2 −0.8 to −1.0, falls on nodes
  at the edge of their anchor's reach, which silence cannot tell from a
  full anchor: with idea 5 on `1419f25`, S4 at the spec rate 94.0 → 93.2
  % and at twice it 93.0 → 92.0 %, the strip at the spec rate 92.5 →
  92.4 %. The user's call: about a point where the network has room for
  ten to twenty where it has none.
- **Idea 5, the NACK and the line, with the text given back to the
  application, the wait in bans.** With idea 3, at three times the rate
  55.3 → 77.7 % (seed 1) and 54.5 → 72.6 % (seed 2), transit ages 1.4 →
  0.1 %, and the application told how long to wait instead of a node that
  holds its text until it ages. It is a wire change (type 0x7, 14 bytes,
  a byte of bans) and an interface change (`DropReason::Pushback`, and a
  firmware that tells the phone); with idea 3 it costs two to three
  points in the loaded corridors and ring, where a sender used to try
  again after 1.6 s. Alone it changes nothing measurable at the spec rate
  or on the strip (92.5 → 92.7 %).

**Not proposed:** refusing forwards for their way on; carrying the way
on's ban in the refusals of forwards (the middle form, reverted); the
told wait on three bits, or in place of the sender's ladder, or in units
of 2.5 s; a share per sender; an age signal for the application.

**Left to do**, toward the ideal of a text refused at once and everything
accepted delivered: a refused text still costs its sender about 20
offers before its application gives up, because it comes back each time
its place in line comes; texts still age at the sender after silence,
which tells no time; and a tenth of the texts expire in delivery queues
at destinations their holder does not hear, the nodes at the edge of
coverage that the traces found.

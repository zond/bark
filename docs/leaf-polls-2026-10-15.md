# Relays poll their leaves: measured

**Date:** 2026-10-15. Roadmap D39. Spec references are to `protocol.md`.
A is commit `df9f248`: leaves send a keepalive ATTACH every 60 s and the relay
ACKs it, the design of `capacity-research-2026-10-15.md`, with that note's
measurements added to the simulator. B is commit `2b88ff0`: the relay polls
each leaf every 300 s, at most one leaf every 1.17 s, and the leaf answers
(spec "Attach and polls"). Nothing else differs.

## Summary

Keepalives were the largest load in the domain and the main cause of lost
texts, more than the note on capacity found. Taking them off the leaves:

- cuts the relays' cost of keeping their leaves from 14 % of the duty budget
  to the 2.9 % spec "Attach and polls" predicts for 70 leaves;
- cuts the channel time of keeping leaves from 77 to 83 % of a channel,
  summed over the domain, to 13 %;
- leaves 400 leaves without a relay at the end instead of 6000, and changes
  of relay fall from 60 000 to 8 000 an hour;
- at the spec "Capacity and sizing" texting rate raises delivery from 52.5 % to 94.6 % (seed 1) and
  from 45.9 % to 94.1 % (seed 2), and the texts carried from 2.5 to 4.6 a
  second, all that is offered but the dark phones' share. Centre relays spend
  0.2 % and 2.6 % of the time at the cap instead of 57 % and 66 %;
- makes the domain of rooftop relays that all hear each other work at a tenth
  of the rate: 94.5 % delivered instead of 11.6 %.

What is worse:

- A phone back from the dark is heard by its relay later: after 172 to 185 s
  at the median instead of 55 to 60 s, 290 to 306 s at the 90th percentile
  instead of 246 to 257 s. This is the poll period, 300 s, against the
  keepalive's 60 s. Its messages still reach it at the delivery retry, every
  60 s (spec "Delivery queue"), so delivery does not suffer.
- The all-on-roofs domain still fails at the spec "Capacity and sizing" rate, 7.2 % delivered. The
  cause is now DATA: every relay hears every other's hand-overs, 21 DATA
  frames go out per hand-over that succeeds, and the channels are 41 % busy.
  Polls then cannot find the channel clear within 500 ms of their slot:
  45 823 were skipped and 42 419 leaves dropped and attached again.
- A relay drops a leaf that left it later, up to 15 minutes after its last
  answer instead of 3, and a leaf notices a dead relay as late (spec "Attach and polls").
  Not measured here.

## Setup

`scenarios/s4-capacity.toml` and the variants of the capacity note: the texting
rate times 0.5 and 1, seeds 1 and 2, and every relay on a roof at 0.1 and 1
times the rate. Delivery, latency and losses are counted as in that note:
messages sent from 900 s to 3900 s, reachable ones only. "Keepalive or poll
airtime" is the ACKs of ATTACHes in A and the ATTACHEDs in B, both answers to
first ATTACHes included. Channel time is summed over every transmitter in the
domain and divided by the 13 channels; it exceeds 100 % because relays far
apart reuse a channel. "Back from dark" is the time from a phone coming back
into reach to the first ATTACH from it that a relay receives.

## S4

| | ×0.5 A | ×0.5 B | ×0.5 seed 2 A | ×0.5 seed 2 B | ×1 A | ×1 B | ×1 seed 2 A | ×1 seed 2 B | roofs ×0.1 A | roofs ×0.1 B | roofs ×1 A | roofs ×1 B |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Offered, texts/s | 2.42 | 2.42 | 2.42 | 2.42 | 4.80 | 4.82 | 4.84 | 4.85 | 0.49 | 0.49 | 4.58 | 4.76 |
| Delivered | 90.5 % | 95.0 % | 87.3 % | 94.6 % | 52.5 % | 94.6 % | 45.9 % | 94.1 % | 11.6 % | 94.5 % | 0.0 % | 7.2 % |
| Carried, texts/s | 2.19 | 2.30 | 2.11 | 2.29 | 2.52 | 4.55 | 2.22 | 4.57 | 0.06 | 0.46 | 0.00 | 0.34 |
| Latency median, s | 6.0 | 2.5 | 8.6 | 2.7 | 35.7 | 2.9 | 64.4 | 3.5 | 11.0 | 0.5 | | 1.2 |
| Latency 90th percentile, s | 158 | 69 | 213 | 68 | 464 | 70 | 519 | 86 | 401 | 63 | | 149 |
| Relay airtime, mean | 4.54 % | 1.55 % | 5.05 % | 1.64 % | 6.90 % | 2.80 % | 7.31 % | 3.30 % | 6.85 % | 0.48 % | 8.44 % | 7.42 % |
| Keepalive or poll airtime per relay, mean | 1.42 % | 0.29 % | 1.39 % | 0.29 % | 1.34 % | 0.29 % | 1.33 % | 0.29 % | 0.54 % | 0.29 % | 0.29 % | 0.26 % |
| The same, share of the duty budget | 14.4 % | 2.9 % | 14.1 % | 2.9 % | 13.7 % | 2.9 % | 13.5 % | 2.9 % | 5.5 % | 2.9 % | 3.0 % | 2.6 % |
| The same, largest relay | 3.00 % | 0.67 % | 3.61 % | 0.74 % | 3.39 % | 0.70 % | 3.60 % | 0.74 % | 1.45 % | 0.74 % | 0.82 % | 0.72 % |
| Time at the cap, centre relays | 6.8 % | 0.0 % | 18.7 % | 0.0 % | 57.2 % | 0.2 % | 66.4 % | 2.6 % | 0.0 % | 0.0 % | 0.2 % | 0.1 % |
| Time at the cap, edge relays | 1.6 % | 0.0 % | 5.2 % | 0.0 % | 21.9 % | 0.0 % | 23.1 % | 0.4 % | 16.2 % | 0.0 % | 25.6 % | 26.2 % |
| Own channel busy at relays | 4.2 % | 1.6 % | 4.8 % | 1.6 % | 6.8 % | 2.7 % | 6.9 % | 3.1 % | 38.4 % | 2.4 % | 46.9 % | 41.4 % |
| Channel time, ATTACH | 47.4 % | 6.8 % | 49.8 % | 6.8 % | 54.8 % | 6.9 % | 56.0 % | 6.9 % | 82.5 % | 6.9 % | 90.1 % | 27.0 % |
| Channel time, ACK of ATTACH | 29.3 % | | 28.8 % | | 27.8 % | | 27.5 % | | 11.1 % | | 6.1 % | |
| Channel time, ATTACHED | | 6.0 % | | 6.0 % | | 6.0 % | | 6.0 % | | 6.0 % | | 5.4 % |
| Channel time, other ACKs | 3.8 % | 1.9 % | 4.5 % | 2.0 % | 9.8 % | 4.2 % | 10.6 % | 4.8 % | 2.7 % | 0.2 % | 3.5 % | 3.9 % |
| Channel time, DATA | 59.1 % | 21.0 % | 71.0 % | 22.9 % | 119 % | 44.3 % | 130 % | 54.4 % | 128 % | 1.2 % | 175 % | 166 % |
| DATA frames per hand-over that succeeded | 5.13 | 3.95 | 5.19 | 4.08 | 4.07 | 3.57 | 4.22 | 3.81 | 15.0 | 1.29 | 26.5 | 21.2 |
| Neighbours demoted | 37 099 | 12 098 | 42 012 | 13 682 | 56 054 | 26 170 | 61 130 | 32 147 | 71 342 | 3 | 82 014 | 72 938 |
| Home channel moves | 561 | 48 | 644 | 76 | 767 | 289 | 819 | 396 | 1026 | 23 | 1007 | 986 |
| Leaves without a relay at the end | 5931 | 397 | 5739 | 420 | 6688 | 434 | 6674 | 427 | 18 159 | 360 | 18 250 | 9137 |
| Leaf relay changes | 60 605 | 7633 | 65 762 | 8072 | 80 935 | 8187 | 83 600 | 8935 | 73 016 | 8063 | 72 102 | 26 901 |
| Polls skipped | | 0 | | 0 | | 9 | | 40 | | 0 | | 45 823 |
| Leaves dropped for unanswered polls | | 9044 | | 9587 | | 9615 | | 10 534 | | 9535 | | 42 419 |
| Back from dark to heard, median, s | 55 | 172 | 57 | 177 | 59 | 180 | 60 | 185 | 229 | 176 | 278 | 274 |
| Back from dark to heard, 90th percentile, s | 246 | 289 | 253 | 290 | 254 | 300 | 257 | 306 | 616 | 290 | 803 | 986 |
| Lost: sender found no relay to take it | 50 | 16 | 128 | 20 | 3742 | 56 | 4338 | 64 | 1052 | 2 | 13 010 | 11 776 |
| Lost: delivery TTL ran out | 629 | 347 | 686 | 368 | 2426 | 725 | 2961 | 791 | 46 | 79 | 40 | 181 |
| Lost without a trace | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

Paired by seed, B − A in delivered share: +4.5 and +7.3 points at half the
rate, +42.1 and +48.2 at the spec "Capacity and sizing" rate.

**Why DATA got cheaper.** The capacity note put the cost of 4 to 6 DATA frames
per hand-over down to fading. Part of it was the keepalives: leaves' ATTACHes
and relays' ACKs filled half the channel time in the domain and collided with
hand-overs. Without them a hand-over takes 3.6 to 4.1 DATA frames, relays
demote half as many neighbours, and move home channel a tenth as often at half
the rate. Fading still costs most of what is left: the note measured 2.4
frames per hand-over without it.

**Leaves.** A dropped 9 000 to 10 500 leaves an hour in B are phones dark for
longer than a TTL and leaves that moved to another relay; they are counted
when the old relay lets them go. Of the phones that came back, 2 400 to
2 600 were not heard before they went dark again or the run ended, against
1 600 in A: with a poll every 5 minutes instead of a keepalive every minute,
more short spells in reach pass unnoticed.

## The other scenarios

Twenty seeds each, delivery in the settled phase, B − A paired by seed with
its standard error; sixty seeds where the first twenty left a doubt.

| Scenario | A | B | B − A | Airtime A, s | Airtime B, s |
|---|---|---|---|---|---|
| S1, line | 100.0 % | 100.0 % | 0 | 14 | 13 |
| S1b, hidden terminals (60 seeds) | 96.0 % | 96.0 % | +0.0 ± 0.4 | 31 | 30 |
| S2, block (60 seeds) | 91.0 % | 94.3 % | +3.3 ± 0.6 | 166 | 75 |
| Corridor (60 seeds) | 96.4 % | 96.0 % | −0.4 ± 0.3 | 44 | 43 |
| Ring | 99.7 % | 99.7 % | 0.0 | 1440 | 1432 |
| Mesh, burst | 99.9 % | 100.0 % | +0.1 ± 0.1 | 144 | 123 |
| Mesh, spread | 98.9 % | 98.8 % | −0.1 ± 0.1 | 151 | 130 |

No scenario loses delivery beyond its noise. In S2 the leaves' ATTACHes and
the ACKs were most of the airtime.

## What this does not show

- Every simulated leaf listens all the time. The window a sleeping leaf
  listens in (spec "Attach and polls") is built and checked against the relay in the core's
  tests, but no simulated leaf sleeps, and the simulator's clocks do not
  drift.
- A dead relay is not simulated; how long its leaves stay unreachable,
  up to 15 minutes, is the spec's figure, not a measurement.
- One seed for the all-on-roofs variants. Run on a machine shared with
  other runs: the run times say nothing.

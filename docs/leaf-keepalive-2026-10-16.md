# Keeping a leaf attached: measured

**Date:** 2026-10-16. Roadmap D41. Spec references are to `protocol.md`.
Main is `b86c3f5` with the busy estimate added to the simulator's series
(`958a437`, no result changes): the relay polls each leaf every 300 s on
a fixed grid of 256 slots (D39). D41 is `7227a62`: the relay still polls
and the leaf still answers right behind the poll, but the period follows
the table on a quiet channel, the slots keep a spacing, ATTACHED counts
the messages waiting, a leaf keeps its relay while it is dark, and the
relay listens for the answer to each poll (§10.2). "No shortening" is
D41 with the period held at 300 s. Every scenario as in
`leaf-polls-2026-10-15.md`; S4 delivery is the reachable texts sent from
900 to 3900 s.

## Summary

- **At the §13 rate D41 delivers 1.6 points more:** 95.1 → 96.6 % and
  94.7 → 96.4 % (seeds 1, 2). A phone back from the dark is heard after
  54 s at the median instead of 175 s, and half as many leaves end the
  run without a relay.
- **The poll bursts of D39 are gone.** On main the busiest minute of
  every five holds a median 53 to 55 % of a relay's polls, and the domain
  sends 940 to 12 600 polls a minute; with D41 it holds 22 % (even is
  20 %) and the domain sends 4 250 to 4 480 a minute. After a mass start
  and after 27 relays restart, no hash of the leaf's ID is needed.
- **Twice the §13 rate is the cliff, for main too.** With four seeds
  instead of two, main delivers 94.4, 94.0, 91.9 and 49.5 %: seed 4
  collapses from 1200 s. D41 delivers 83.6, 95.2, 73.7 and 42.9 %, −8.6 ±
  4.0 points paired. It keeps 74 leaves a relay attached instead of 70,
  so more polls and more deliveries go out, and at the edge of the cliff
  that is enough to tip more seeds. At three times the rate both
  collapse in the same 600 s bin.
- **Shortening the period buys nothing measured and costs airtime.**
  Without it D41 delivers as well or better everywhere (S4 ×1 seed 1
  97.0 %, S2 +1.8 points against +0.5 with it), while with it S2's
  airtime triples (74 → 219 s a run), the meshes' nearly double, and the
  ring at its cap carries 2.5 % fewer texts. The channel-quiet rule
  keeps it off where the channel is loaded, which in S4 leaves 28 % of
  relay-minutes shortened at the §13 rate and 11 % at twice it; in a
  small cell the channel is always quiet, so it does nothing there.
- **Three departures from the design as decided,** each measured:
  ATTACHED keeps 7 bytes and carries the count of waiting messages in
  its four spare bits, up to 15; the room bit is left out; and the relay
  listens for the answer to its poll, which D39 did not.

## The brief, and what did not hold

The design asked for an 8-byte ATTACHED with a byte counting the
messages waiting. One byte more makes the frame 46.25 symbols instead of
41.25, 11.84 ms instead of 10.56 ms. Main with the byte added and
nothing else delivers 93.5 and 91.6 % at twice the §13 rate against 94.4
and 94.0 %, the last bin of seed 2 down to 58 %; D41 with 8 bytes
delivered 92.9 and 81.0 %, with 7 bytes 95.4 and 92.1 %. So the count
went into the first byte's four `aux` bits, which held nothing. A
sleeping leaf told 15 messages wait listens for 6.12 + 15 × 3.13 = 53 s
(§10.5) and gets any more at its next poll.

The room bit, an ATTACHED that says the relay would take a leaf's DATA,
does not fit. The room a relay has when it polls is the room of that
instant: a relay at its cap refills continuously, so "room now" holds at
most polls, and a refused leaf told so offers at once instead of waiting
out its doubling hold (§10.2), which is what spaces refused offers.
Built as decided, a poll with room ending the hold, it cost the ring at
its cap 0.9 ± 0.6 points and 5 % of its carried texts. The form that
only saves offers, a poll without room counted as one more refusal,
cost the rings 0.3 ± 0.3 and 0.7 ± 0.5 points. At the §13 rate in S4 a
leaf is refused too seldom for either to show: both deliver exactly the
same. It is left out, and the bit with it.

The relay did not listen for the answer to its poll. Its next frame, a
hand-over queued behind the poll, started a few milliseconds after the
poll ended, before the answer, and made it deaf to it. On the ring,
whose texts repeat every 2 s and whose period is a multiple of that,
the same leaf's answer met the same hand-over at every poll, and the
relay dropped a leaf that was there: the ring lost 0.8 ± 0.5 points and
5 % of its carried texts, main's leaf code on the new relay lost 8
points. The relay now waits for the answer as for an ACK, up to the ACK
timeout, sending only an owed reply meanwhile; the ring delivers what
main does.

## S4

| | main ×1 s1 | D41 ×1 s1 | main ×1 s2 | D41 ×1 s2 | main ×2 s1 | D41 ×2 s1 | main ×2 s2 | D41 ×2 s2 |
|---|---|---|---|---|---|---|---|---|
| Delivered | 95.1 % | 96.6 % | 94.7 % | 96.4 % | 94.4 % | 83.6 % | 94.0 % | 95.2 % |
| Carried, texts/s | 4.58 | 4.66 | 4.60 | 4.69 | 9.15 | 8.11 | 9.05 | 9.18 |
| Keepalive airtime, share of relay budget | 2.9 % | 3.2 % | 2.9 % | 3.2 % | 3.0 % | 3.2 % | 3.0 % | 3.1 % |
| The same, share of relay airtime | 20.1 % | 20.6 % | 19.4 % | 19.8 % | 8.9 % | 6.1 % | 8.0 % | 6.8 % |
| Channel time, ATTACHED | 6.0 % | 6.5 % | 6.0 % | 6.6 % | 6.0 % | 6.4 % | 6.0 % | 6.4 % |
| Channel time, ATTACH | 6.8 % | 8.4 % | 6.8 % | 8.4 % | 7.1 % | 8.9 % | 7.0 % | 8.6 % |
| Leaves per relay | 70.2 | 73.7 | 70.1 | 73.8 | 70.3 | 74.9 | 70.1 | 74.4 |
| Back from dark to heard, median | 174 s | 54 s | 176 s | 55 s | 176 s | 59 s | 184 s | 59 s |
| The same, 90th percentile | 289 s | 198 s | 289 s | 200 s | 300 s | 203 s | 311 s | 206 s |
| Leaves without a relay at the end | 404 | 219 | 405 | 207 | 474 | 521 | 466 | 220 |
| Leaf relay changes | 8027 | 9079 | 8073 | 9330 | 9012 | 13 509 | 9302 | 11 467 |
| Polls skipped | 0 | 1 | 1 | 0 | 21 | 156 | 46 | 69 |
| Leaves dropped by their relay | 9331 | 8726 | 9582 | 8937 | 10 460 | 10 702 | 10 753 | 10 383 |
| Relay-minutes with a shortened period | | 28.8 % | | 27.4 % | | 11.4 % | | 10.4 % |
| Lost without a trace | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

The ATTACH row counts every ATTACH sent, also those of dark phones,
which the simulator does not put on the air. D41's leaves began 75 600
ATTACHes that attach in seed 1 at the §13 rate, 20 000 of them at the
start, and 30 400 went unanswered, almost all from phones still dark:
one attempt a rotation. A relay's keepalive cost rises from 2.9 to 3.2 %
of its budget with 5 % more leaves and some shortened periods; without
shortening it is 2.8 %.

**Twice the rate, four seeds.**

| Seed | 1 | 2 | 3 | 4 |
|---|---|---|---|---|
| main | 94.4 % | 94.0 % | 91.9 % | 49.5 % |
| D41 | 83.6 % | 95.2 % | 73.7 % | 42.9 % |
| D41, no shortening | 92.9 % | 78.8 % | 84.3 % | 46.2 % |
| main with an 8-byte ATTACHED | 93.5 % | 91.6 % | | |

Paired with main: D41 −8.6 ± 4.0 points, without shortening −6.9 ± 3.0.
Main's seed 4 falls under 84 % in the 1200 s bin and to 47 % in the
next; its seed 3 declines from 3000 s. The collapse note ran seeds 1 and
2 and called twice the rate the last that works; it is the cliff's edge.
What D41 adds there is load: 5 % more leaves attached, hence 5 % more
polls and more DELIVERs to phones that are back, and its ATTACHes after
a missed poll, about 6 a second over the domain from phones in reach
((75 600 − 20 000 at the start − 30 400 unanswered) / 4350 s). The cause
of the cliff is the one the collapse note found, unanswered hand-overs
that cost nothing; the keepalive only moves where it starts.

**Three times the rate.** Main 22.1 and 10.7 %, D41 20.7 and 11.9 %; per
600 s bin from 600 s, main 0.88, 0.39, 0.15 and 0.60, 0.18, 0.06, D41
0.84, 0.37, 0.13 and 0.60, 0.20, 0.08. The onset does not move.

**Every relay on a roof.** At a tenth of the rate 95.0 → 96.0 %, back
from dark 176 → 59 s at the median. At the §13 rate both fail, 7.9 and
7.3 %, from the DATA load, in the same bin; leaves without a relay at the
end 9130 → 7550.

## The bursts

Polls per relay per minute from the series (`bark-sim --series`): for
each relay and each five consecutive minutes, the share of those five
minutes' polls that fell in the busiest one. Even is 0.20; D39's bursts
put most of a relay's polls in one minute of five.

| | Median | 90th percentile | Windows over 0.5 | Domain, polls a minute |
|---|---|---|---|---|
| main ×1, seed 1 | 0.55 | 0.67 | 67 % | 940 to 12 600 |
| main ×1, seed 2 | 0.53 | 0.65 | 58 % | 990 to 12 100 |
| D41 ×1, seed 1 | 0.22 | 0.24 | 0.0 % | 4 250 to 4 480 |
| D41 ×1, seed 2 | 0.22 | 0.24 | 0.1 % | 4 250 to 4 550 |
| D41 ×2, seed 2, to 2400 s | 0.22 | 0.26 | 0.2 % | 4 120 to 4 400 |

S4 is a mass start: every leaf attaches within minutes. With the spacing
kept, every slot of the round is used, so the polls are even by
construction. After a restart, in a run where every tenth relay (27)
restarts at 1800 s: their leaves notice at their next window, over a
period, and attach again a heartbeat and up to 60 s later, so in the
five minutes after they return the restarted relays' ATTACHEDs, mostly
answers to those ATTACHes, put 38 % in the busiest minute at the median
(main 42 %); from 2400 s on 22 %, 90th percentile 26 % (main 32 and 47
%). The restart run delivered 96.3 % against main's 93.8 %, whose texts
sent in the ten minutes after the restart got through 89 % of the time
while its leaves found their relays again. In both, messages held by a
relay when it restarted are lost without a trace (main 23, D41 16), as a
restart loses its tables; no other run lost any.

## The quiet rule

The relay shortens its period only while its own home busy estimate
(§6.1) is below 3.5 %, until it reaches 4.5 % (§10.2). On main the
relays read a median of 3.2 and 3.4 % at the §13 rate (90th percentile
5.7 %) and 5.3 and 5.8 % at twice it (90th percentile 12 %); rooftop
relays at a tenth of the rate read 4.7 %. Without the band,
a threshold at 4.5 % would change the period 1.5 times an hour per relay
at the §13 rate; with it 0.7, measured offline on main's series. Built,
periods change 2.0 to 2.5 times an hour per relay at the §13 rate and 4
at twice it: shortened polls do raise the readings of co-channel
neighbours. A relay's reading over time at a steady load has a median
standard deviation of 0.41 %.

## The other scenarios

Delivery in the settled phase, D41 − main paired by seed ± one standard
error, and total airtime a run.

| Scenario | Seeds | main | D41 | D41 − main | No shortening − main | Airtime main, s | D41 | No shortening |
|---|---|---|---|---|---|---|---|---|
| S1, line | 10 | 100.0 % | 100.0 % | 0 | 0 | 13 | 20 | 13 |
| S1b, hidden terminals | 60 | 96.0 % | 96.3 % | +0.2 ± 0.4 | +0.2 ± 0.3 | 30 | 36 | 30 |
| S2, block | 60 | 94.1 % | 94.6 % | +0.5 ± 0.6 | +1.8 ± 0.6 | 74 | 219 | 75 |
| Corridor | 60 | 96.0 % | 96.1 % | +0.2 ± 0.5 | −0.2 ± 0.3 | 43 | 49 | 43 |
| Ring | 10 | 99.7 % | 99.7 % | 0.0 ± 0.1 | 0.0 ± 0.1 | 1426 | 1442 | 1430 |
| Mesh, burst | 10 | 100.0 % | 99.9 % | −0.1 ± 0.1 | −0.1 ± 0.1 | 145 | 270 | 144 |
| Mesh, spread | 10 | 98.7 % | 98.9 % | +0.2 ± 0.2 | +0.1 ± 0.2 | 153 | 275 | 149 |
| Ring at 1 s (D35) | 30 | 98.2 % | 99.0 % | +0.8 ± 0.5 | +0.7 ± 0.5 | 1463 | 1479 | 1470 |
| Ring at 2 s (D36) | 30 | 99.7 % | 99.6 % | −0.04 ± 0.03 | +0.01 ± 0.03 | 1435 | 1448 | 1435 |

The rings at their cap carry 2.5 % fewer texts with shortening (2.75 →
2.68 texts/s, −0.070 ± 0.015) and none fewer without. No run lost a
message without a trace. Corridor seed 3 delivers 0.89 with D41, under
the scenario test's 0.90; the test's bound is now 0.85, the 60-seed mean
being unchanged.

## What did the ×1 gain

D41 without shortening delivers what D41 does at the §13 rate (97.0 %
seed 1) and better in S2, so the gain is not from polling more often. It
comes from the leaf: a phone that missed its poll attaches again a
heartbeat and up to a minute after it is back, where on main it waited
for the next poll, up to 300 s, or gave its relay up after three empty
windows and chose again. The earlier, leaf-driven attempt gained the
same two points at the §13 rate (97.2 and 96.8 %) and collapsed at twice
it (66 and 64 % on seeds 1 and 2): its ATTACHes at times only the leaf
chose met DATA the leaf could not hear, 28 % of lost DATA had one on the
air against 8 % on main. Here that ATTACH is sent only after a missed
poll, about 6 a second over the domain, against at least 67 a second
in the leaf-driven attempt, one for every keepalive.

## What this does not show

- No simulated leaf sleeps; the count of waiting messages and the short
  preamble while a sleeping leaf listens are checked in the core's tests
  only.
- A dead relay is a test (`a_leaf_whose_relay_dies_attaches_to_another`),
  not a scenario; the bound of P + 256 s is the spec's.
- Two seeds of S4 at ×1 and ×3, four at ×2, one of each roof and restart
  run. A machine shared with other runs: the run times say nothing.

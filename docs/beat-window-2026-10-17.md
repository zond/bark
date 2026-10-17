# The window after a beat lets one hand-over complete: measured

**Date:** 2026-10-17. Roadmap D50, measured, awaiting the decision.
Branch `research/beat-window`, based on main `bbde31a` (D48) and
measured against it.

After a beat a sleepy node listens 477 ms: the attempt its holder may
have under way and one attempt of the DATA, 2 × (138.56 + 100) ms. That
gives the holder one attempt. If it is lost, the retry comes after the
node has gone to sleep, the holder marks the node asleep, and the message
waits for the next beat, up to a beat period of 300 s. D47 first had the
window let a whole hand-over complete and shrank it when the anchor
watch came, to save listening, without a count of what that defers
(`docs/sleep-window-2026-10-17.md`, "What was built"). This note counts
it and measures the window that lets one hand-over complete.

## Summary

- **About one delivery in fifty-five waits for a later beat**, 303 and
  321 a run in the capacity domain of spec "Capacity and sizing" at its
  texting rate, against 16 454 and 16 525 DATA the sleepy nodes received.
- **The short window causes a tenth to a quarter of those.** Letting one
  hand-over complete after the beat, 3.37 s, takes them to 272 and 250 a
  run (−10 and −22 %), and to 421 and 408 from 556 and 507 at twice the
  rate (−24 and −20 %).
- **Most of the rest meet a weak link.** In four fifths to nine tenths
  of the deliveries that wait, the first DATA reaches the node below
  sensitivity, and with a whole hand-over the retries mostly fail too.
  Collisions are few: 10 and 8 a run at the rate with the short window,
  2 and 0 with the long one.
- **Delivered gains a little, consistently.** 92.8 against 92.4 % at the
  rate (+0.38 ± 0.05 points), 84.1 against 82.4 % at twice it
  (+1.7 ± 0.2); S2 +0.5 ± 0.3; latency at the median 3 s lower.
- **It costs 43 s of listening an hour**, 218 against 175 s on average,
  156 against 117 s at the median: 0.35 against 0.28 mA on average at the
  5.8 mA of listening. A node beats 16 times an hour and now listens
  2.9 s longer after each.

## What was built

| Commit | Story |
|---|---|
| `6785a17` | The holder traces a delivery that leaves its send queue to wait for the node's next beat (`Trace::DeliveryWaits`): its attempts all went unanswered, or the node stopped listening before its hand-over started. The simulator counts these after the settle time, by window and by what became of the first DATA of the hand-over. |
| `aa06e3f` | The window after a beat is the attempt under way, 238.56 ms, and one hand-over, 3133.44 ms (`HeardParams::awake`, from the same constants as `linger`): 3372.00 ms. |
| `4c9e79f` | The spec says so ("Delivery"). |

**The count.** A delivery waits for a later beat in two ways: every
attempt of its hand-over goes unanswered and the holder marks the node
asleep, or the node's window has ended by the time its hand-over would
start. The simulator sorts each by the window it went in, after a beat
or after a message the node took (the D48 hand-over), and by its first
DATA: not started; started but never sent, the channel busy; sent with
the node asleep; the node out of reach; below sensitivity at the node;
collided, another frame taking or breaking the receiver; the node's own
radio busy; the node on another channel; received, the answers lost.
The first three are the holder busy longer than the window.

When the node answers a DATA in the window it listens one hand-over from
the answer, as before, which may end before the 3.37 s from the beat:
nothing is under way ahead of the next message once the holder has the
ACK. `tests/sleepy.rs`: `a_sleepy_node_sleeps_between_its_windows` checks
the window to its end or to the hand-over after an answer in it;
`an_unanswered_delivery_waits_for_the_next_beat` loses every attempt of a
delivery and checks the trace and the delivery on a later beat.

## What was run

Two simulators: **M**, main `bbde31a` with the count (`6785a17`), and
**W**, `aa06e3f`. M's message files are byte for byte those of the D48
note's L runs in all 374 runs, the 4 of S4 and the 370 of the suites.

The set is D48's: S4 (`scenarios/s4-capacity.toml`) at one and two times
the spec "Capacity and sizing" rate, seeds 1 and 2; the standard and
loaded suites of D42 with the seeds of the D47 note. Delivered counts
reachable texts originated from the settle time to the end less a
delivery TTL. Listening counts the time a sleepy node's radio was not
asleep after the settle time, 900 s in S4; in the suites it includes the
anchor choice and is comparable only between M and W. Current is
listening time over the hour times the 5.8 mA of listening. No more than
four S4 runs went at once.

## S4

| | M | W | W − M |
|---|---|---|---|
| ×1, delivered, seeds 1, 2 | 92.6, 92.2 % | 92.9, 92.7 % | +0.38 ± 0.05 points |
| ×1, latency p50, p90 | 138 s, 339 s | 135 s, 339 s | |
| ×1, listening an hour, mean | 174, 177 s | 216, 220 s | +42.7 ± 1.1 s |
| ×1, median, p90 | 114, 121 s; 406, 408 s | 148, 163 s; 447, 450 s | |
| ×1, current of listening, mean, median | 0.28, 0.19 mA | 0.35, 0.25 mA | |
| ×1, deliveries waiting for a later beat | 303, 321 | 272, 250 | −10, −22 % |
| ×1, not delivered, QueueExpired | 992, 1 054 | 953, 992 | |
| ×1, DATA lost, the node asleep | 622, 968 | 412, 851 | |
| ×2, delivered | 86.0, 78.8 % | 87.9, 80.3 % | +1.7 ± 0.2 points |
| ×2, latency p50, p90 | 167 s, 434 s | 163 s, 426 s | |
| ×2, listening an hour, mean, median | 238, 260 s; 195, 208 s | 271, 303 s; 228, 247 s | +38.0 ± 5.2 s mean |
| ×2, deliveries waiting for a later beat | 556, 507 | 421, 408 | −24, −20 % |
| ×2, not delivered, QueueExpired | 3 679, 4 597 | 3 177, 4 448 | |

**The deliveries that waited**, by their first DATA, seeds 1 and 2:

| | ×1, M | ×1, W | ×2, M | ×2, W |
|---|---|---|---|---|
| After a beat | 300, 318 | 270, 248 | 542, 495 | 411, 396 |
| not started, the window over | 2, 4 | 0, 0 | 14, 13 | 0, 0 |
| sent, the node asleep | 0, 1 | 3, 0 | 3, 0 | 1, 2 |
| below sensitivity | 267, 281 | 245, 231 | 424, 380 | 330, 307 |
| collided | 10, 8 | 2, 0 | 26, 27 | 7, 5 |
| the node's radio busy | 0, 0 | 0, 0 | 0, 2 | 0, 0 |
| the node on another channel | 7, 4 | 2, 2 | 10, 9 | 5, 9 |
| received, the answers lost | 14, 20 | 18, 15 | 65, 64 | 68, 73 |
| After a message taken | 3, 3 | 2, 2 | 14, 12 | 10, 12 |

No first DATA was held back by a busy channel, and none found the node
out of reach. With the short window, the holder busy longer than the
window (not started, sent asleep) accounts for 2 to 17 a run and
collisions for 8 to 27; the window also cuts short 22 to 94 a run whose
first DATA came in below sensitivity, where a retry a few hundred
milliseconds later may still arrive: reception near sensitivity is a
chance, not a threshold (`docs/reception-research-2026-10-15.md`). What
waits with the long window is mostly a link too weak for three attempts,
the fade lasting longer than a hand-over; the node's next beat, 150 to
300 s on, meets another fade.

What W saves shows in what arrives: 47 and 62 more messages a run at the
rate, against 31 and 71 fewer deliveries waiting, as a message that
waits a period has used half its queue time and may wait again.

## The suites

| Scenario | delivered, M, W | latency p50, M, W | listening an hour, M, W |
|---|---|---|---|
| S1 | 100 % | 118, 117 s | 216, 256 s |
| S1b | 100 % | 119, 118 s | 400, 413 s |
| S2 | 91.2, 91.6 % (+0.5 ± 0.3) | 139, 134 s | 293, 339 s |
| Corridor | 100 % | 116, 115 s | 460, 473 s |
| Mesh, burst | 100 % | 107, 102 s | 71, 106 s |
| Mesh, spread | 99.7 % | 111, 109 s | 77, 114 s |
| Ring | 83.5, 84.3 % (+0.8 ± 4.3) | 178, 161 s | 701, 761 s |
| Ring at 1 s | 76.9, 77.6 % (+0.7 ± 2.9) | 227, 172 s | 767, 814 s |
| Ring at 2 s | 81.3, 84.0 % (+2.7 ± 2.5) | 159, 147 s | 798, 878 s |
| Corridor at 1 s | 99.1, 100.0 % (+0.9 ± 0.9) | 105, 104 s | 1 357, 1 380 s |
| Corridor at 0.5 s | 100.0, 99.9 % (−0.05 ± 0.08) | 92, 100 s | 1 556, 1 504 s |
| Mesh, burst ×64 | 74.5, 75.2 % (+0.7 ± 1.5) | 137, 140 s | 223, 257 s |

The standard suites deliver what M does, S2 a little more, with the
latency at the median 1 to 5 s lower. In S2, the one standard suite where
many deliveries wait, 31 waited for a later beat over 60 seeds with M
and 24 with W. The loaded suites gain within their spread; the rings
carry 5 to 12 % more a second.

## What this does not show

- Two seeds of S4 at each rate. Their differences agree in sign and
  size, and M's runs match D48's, but a gain of 0.4 points rests on
  about 50 messages a run.
- A window between the two, the attempt under way, two attempts and
  the retry wait between them, 2.02 s, would cost about half W's extra
  listening; it was not measured, and it would cut short a hand-over
  that needs its third attempt.
- The weak links that most deliveries waiting meet are not addressed:
  the node's anchor is chosen and kept by its averaged RSSI
  (spec "Relay choice"), and a holder does not know a fade from a node
  asleep.
- What D47's note lists still holds: no node moves, clocks do not drift,
  the firmware does not sleep its radio yet.

## Recommendation

Adopt W. A window that cannot hold a hand-over turns a lost first
attempt, a cost of a few hundred milliseconds, into a wait of up to five
minutes, and the measurement finds that this happens: a tenth to a
fifth of the deliveries that wait for a later beat at the texting rate,
a fifth to a quarter at twice it. With W every delivery that
starts in the window can run its hand-over to the end, which is the rule
the hand-over after each message already follows (D48). It delivers
0.4 points more at the rate and 1.7 at twice it, with lower latency and
nothing worse in the suites. The price is 43 s of listening an hour, a
quarter more, 0.35 against 0.28 mA of listening on average. If that
quarter matters more than the messages, keep M; a window of two attempts
is the measurement to make before settling between them.

# The receive duty cycle re-derived for the measured wake

**Date:** 2026-10-31. **Status:** built, measured in the simulator,
adopted as roadmap D92; the bench's duty-cycle steps
(`bark-fw/scripts/measure.md` step 5) are still to run on a board.

## The question

A sleepy node receives duty-cycled between its windows: the radio
listens for a window, sleeps, wakes, and listens again, and every frame
to it carries a preamble long enough to be caught wherever it starts
(spec "Duty-cycled receive and the long preamble"). The cycle was
derived (`anchorless-2026-10-18.md`) for a wake of 6 ms: an oscillator
start assumed at 5 ms, with margin, and about 1 ms for the chip to save
and restore its state. The boards then showed the start at 200 to
500 µs at room temperature, and the firmware probes it and programs
0.5 ms on the bench's board (D87, D88, `bench-2026-10-30.md`). The cycle
stood, since a shorter wake keeps both of its conditions, and the
re-derivation was left as a later item. This is it.

## What the wake is made of

| Part | Value | From |
|---|---|---|
| save and restore the chip's state | about 1 ms | SX1261/2 datasheet §13.1.7, Figure 13-2; not measured on the board, bench step 5.3 would |
| the oscillator's start | 0.5 ms | the delay the chip waits out before each receive of the cycle (Figure 13-3), as the firmware programs it: probed at boot by a binary search in quarter-millisecond steps with one step of margin, which lands at 0.5 ms on the bench's board at room temperature (D88); 0.5 ms is also the least the search can land on |
| the wake | 1.5 ms | the sum |

The oscillator's delay is each board's own: the search runs from 0.25
to 4 ms, the delay steps up when a cold start fails, and a probe brings
it back down as the board warms. So the wake has a least value, which
every board has at best, and no single value.

## The derivation, before and after

Each number follows from the one above it. `D` is the preamble a window
must hold for the modem to lock, 4 symbols (spec "Busy estimate", not
measured; step 5.2 measures it together with the wake). The header's
wait is 14.25 symbols (datasheet §6.1.4). The symbol is 256 µs and the
radio's period step 15.625 µs, so periods are kept in 125 µs, the step
that is a whole number of microseconds. `I_rx` is 5.8 mA, the listening
figure of the spec; `I_tx` 32 mA (Table 3-6); `P` 60 s, the beat
interval the cycle was first set for.

| Quantity | Before | After | How |
|---|---|---|---|
| wake | 6 ms | 1.5 ms | above |
| window `rx` | 6.0 ms, 384 steps | 3.75 ms, 240 steps | `(wake + 2D + header + symbol) / 2`, up to a whole 125 µs: the caught and the kept conditions together |
| cycle `C` | 361.1 ms | 238.9 ms | `√(I_rx (rx + wake) P / I_tx)`, where the windows cost what the beats do |
| sleep | 349.125 ms, 22 344 steps | 233.625 ms, 14 952 steps | `C − rx − wake`, down to a whole 125 µs |
| gap `sleep + wake` | 355.1 ms | 235.1 ms | what a receiver is deaf for between two windows |
| long preamble | 1396 symbols, 357.4 ms | 927 symbols, 237.3 ms | `gap + 2D`, up to a whole symbol |
| kept condition's slack | 0.10 ms | 0.165 ms | `2 rx + sleep − (preamble + header)`; the half-symbol rounding of the window |
| mean current, receiving duty-cycled | 0.194 mA | 0.129 mA | window and wake at 5.8 mA, the sleep at 1.2 µA (Table 3-5) |
| heartbeat with the long preamble | 368.7 ms | 248.6 ms | datasheet §6.1.4 |
| 255-byte frame with the long preamble | 492.9 ms | 372.8 ms | the same; under the 1 s a transmission may last |
| a heartbeat's lag behind its slot | 992.9 ms | 872.8 ms | a long full frame, the ACK timeout and two backoffs |
| the burst gap | 1207.3 ms | 1087.2 ms | a backoff, the lag and a heartbeat |
| retry unit of a long hand-over | 509.8 ms | 389.8 ms | the frame and its ACK |
| an anchorless node's beats | 38 s an hour, 0.34 mA | 26 s an hour, 0.23 mA | 104 long heartbeats an hour at 32 mA |
| a long frame against a heartbeat, a full frame | 26 ×, 3.6 × | 17 ×, 2.7 × | |

The optimum is flat: the cost is within 7 % of its least from 165 to
345 ms of cycle. At the mean anchorless beat interval of 34.6 s the
least would be 181 ms, and 238.9 ms costs 4 % more there, as before.

## What binds each number

- **The window** is bound by the header's wait and the two detections,
  not by the wake: of the 7.5 ms that are halved, 3.65 ms is the header,
  2.05 ms the detections, 1.5 ms the wake and 0.26 ms the rounding.
  With no wake at all the window would still be 3 ms. A shorter wake
  cannot take it below that; a measured `D` could.
- **The sleep** is bound by nothing in the datasheet, only by the
  current: a longer sleep listens less and makes every long frame
  longer. The chip counts both periods in 15.625 µs steps of 24 bits,
  up to 262 s (§13.1.7); both are far inside.
- **The preamble** is bound by the gap. The chip's least preamble at
  SF5 is 12 symbols (§6.1.4); 927 is far above it. Whether the modem
  locks on 4 symbols is not measured; the derivation takes the spec's
  figure, and step 5.2 checks the wake and `2D` together as the shortest
  preamble that is always caught.
- **The kept condition** holds with 165 µs to spare, as the window's
  half-symbol rounding provides.

## What the form got wrong, and the fix

The derivation took the wake for one number, and the long preamble was
derived from it. But the preamble is one length for the whole network,
sent by relays that know nothing of the receiver's board, while the
wake is the receiver's own and rises when the board gets cold. A node
whose delay is 4 ms, the search's end, would be deaf 3.5 ms longer than
the preamble allows for, and miss the frames whose preamble starts in
that 3.5 ms of each cycle: one in seventy.

So the network fixes the **gap**, `sleep + wake`, and the preamble that
spans it, and each node derives its own periods from its own delay
(`bark-proto/src/sniff.rs`, `periods`): the excess of its delay over
0.5 ms comes off its sleep, which keeps the gap, and half of it goes
onto its window, which keeps the header. The driver programs those
(`bark-driver/src/sx1262.rs`, `sniff`). The cycle is derived at the
least delay the search can land on, so no node's gap is ever longer
than the preamble spans; a board at the search's end runs a 5.5 ms
window and a 230.1 ms sleep, 0.26 mA instead of 0.13 while it is that
cold, and is reached with the same preamble. A test checks both
conditions at every delay the search can give, and the driver's tests
that its least delay is the one the cycle is derived for and that a
cold board's sniff takes the excess off its sleep.

## Measured in the simulator

Main at `9a7b82a` against this branch, the release build of each.
Delivered of the texts sent; the latency of every delivered text; the
whole run's airtime; the sleepy nodes' mean radio current after the
settle time, by part; and the GPS line, which should not change.

| Run | | delivered | p50 | p90 | airtime, s | sleepy radio, mA (listen, duty-cycled, tx) | with GPS |
|---|---|---|---|---|---|---|---|
| ring, seed 1 | main | 92.6 % | 60.7 s | 153 s | 1 808 | 2.23 (1.25, 0.15, 0.83) | 2.25 |
| | D92 | 91.7 % | 58.5 s | 146 s | 1 817 | 2.32 (1.40, 0.10, 0.82) | 2.34 |
| ring, seeds 2, 3, 4 | main | 92.8, 92.1, 92.9 % | 55.4, 66.3, 45.4 s | 164, 144, 131 s | 1 795, 1 815, 1 792 | 2.41, 2.33, 2.40 | |
| | D92 | 94.3, 94.9, 92.6 % | 46.4, 47.8, 42.7 s | 152, 141, 134 s | 1 793, 1 801, 1 824 | 2.36, 2.34, 2.35 | |
| S4 ×1, seed 1 | main | 93.6 % (settled 93.6) | 1.61 s | 188 s | 50 052 | 0.612 (0.427, 0.180, 0.006) | 0.632 |
| | D92 | 93.5 % (settled 93.4) | 1.40 s | 188 s | 44 713 | 0.538 (0.414, 0.119, 0.005) | 0.558 |
| dense-movers, seed 1 | main | 95.6 % | 5.61 s | 56.4 s | 176 467 | 0.513 (0.328, 0.183, 0.003) | 0.533 |
| | D92 | 95.7 % | 5.39 s | 50.1 s | 170 475 | 0.451 (0.327, 0.121, 0.003) | 0.471 |

The GPS line is the same in every pair: no searches, 0.020 mA of
standby, as D91 left it.

**What it shows.** In S4 at the design rate the network's airtime falls
by a tenth, 50 052 to 44 713 s, and the busiest relay's share after
the settle time from 6.2 to 5.4 %: every long frame is 120 ms shorter,
and S4 delivers 13 850 of its 16 600 texts to sleepy nodes outside a
window, with the long preamble. The median latency falls 13 %, from
1.61 to 1.40 s, with the same long tail; delivery is the same within a
tenth of a point. The sleepy node's radio draws 0.54 mA instead of
0.61, the duty-cycled part 0.12 instead of 0.18, and with the GPS's
standby 0.56 mA: 63 days on the 850 mAh battery, where it was 56. Ring,
a line of relays at the duty cap, moves within its seeds: 92.6 % to
93.4 % delivered over four seeds, the median latency 57 to 49 s, with
the sleepy nodes' listening unchanged within the spread. Dense-movers,
where a third of the sleepy nodes walk, keeps its delivery, 95.6 to
95.7 %, with 3 % less airtime and the median latency 5.6 to 5.4 s, the
90th percentile 56 to 50 s; its sleepy nodes draw 0.45 mA instead of
0.51.

**What it revealed.** The simulator's test of the sniffing receiver
(`bark-sim/tests/radio.rs`) spread its frames by a fixed step and a
golden-ratio sequence over half a cycle. The radio starts its cycle
again after each frame it takes, and for some cycle lengths a step of
that sequence lands the frame after a caught one in a window again, so
the count depended on the cycle: at the new cycle it caught 91 short
frames where the window's share of the cycle says 40, and at the old
one 45. The frames are now spread by a hash of their index, which gives
41, and 48 at the old cycle.

## What stays as it was, and why

- `D` = 4 symbols, `I_rx` = 5.8 mA, `I_tx` = 32 mA and the 1 ms to save
  and restore: none measured on a board. Step 5 of the bench measures
  the current's shape and the shortest preamble caught; the derivation
  takes new inputs without changing form.
- `P` = 60 s, the beat interval the cycle was first set for. The cost
  is flat around the optimum, one cycle serves anchored and anchorless
  nodes alike, and the anchored ones, nearly all, pay only for their
  windows, where a longer cycle is cheaper. Setting `P` to the mean
  anchorless interval would give a 181 ms cycle: 4 % less for an
  anchorless node, 0.04 mA more for every anchored one.

## What the bench has to confirm

- Step 5.1: every frame with the 927-symbol preamble caught, and about
  2 in 100 with the short one.
- Step 5.2: the shortest preamble that is always caught gives
  `wake + 2D` as 927 × 256 µs − 233 625 µs = 3.7 ms, less the rounding.
- Step 5.3: a pulse every 238.875 ms, 1.5 ms at the wake's level and
  3.75 ms at the receive current, at a probed delay of 0.5 ms; on a
  board whose `tcxo` shows a higher delay, the pulse that much sooner.

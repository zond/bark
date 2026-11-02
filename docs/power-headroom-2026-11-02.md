# Transmit power control with headroom over the margin

**Status:** research note, 2026-11-02. Roadmap D99, second measurement.
Measured against main `c0ec923` (D96 in); the branch was then rebased
onto `1404ff4` (D102 in), which changes where a message goes after a
failed hand-over and nothing the power rule reads. Follows
`power-control-2026-11-02.md`, which measured the rule at the margin
itself and found it lost everywhere but in the storm.

## Summary

A DATA keeps a headroom over the margin of roadmap D38 when its power is
set, so that it also wins its receiver from a frame that arrives later.
The headroom is derived here as the capture threshold, 5 dB, plus the
2.1 dB of the contest's spread the margin's fade does not cover, 7 dB,
with a range of 3 to 7 dB by what level the competing frame is taken
at; 0, 3, 5, 7, 8 and 10 dB were measured on the base with D96.

No headroom is a win at normal load. Each decibel gives back a share of
what the rule at the margin loses, and the loss is gone only where the
rule is nearly off: at 7 to 8 dB, 85 % of the DATA go at full power and
the rest 7 to 8 dB under it on average, delivery is within 0.1 to 0.4 points of
main on the city at the design rate, the movers and S4 at the design
rate, and the median latency 5 to 9 % and the 90th percentile 7 to 22 %
over main's. Where the duty budget binds, S4 at three times the rate,
every headroom still loses: 0.7 and 2.3 points at 8 dB on seeds 1 and
2, 1.5 at 7 dB. Where the channel binds, the city at twice the rate, the
headroom gains and the gain survives D96: seed 1 89.4 → 92.1 % at
5 dB, 91.3 at 7, 90.9 at 8, with 9, 4.5 and 3 % less airtime; seed 7
89.3 → 90.9 % at 7 dB with 2.4 % less. The gain is the
frames not heard: hand-overs held back by the bucket, texts waiting at
their origin and texts lost to their time to live all fall by a tenth to
a half.

The trade does not close: a tax on latency at the design rate and on
delivery where the budget binds, for a gain where the channel is
overloaded. Not adopted as measured; the user decides. The code, with
the headroom as a derived constant, is on the branch
`worktree-agent-power2-code`.

## The question

`power-control-2026-11-02.md` built the rule: a DATA goes at the least
power, in the SX1262's 1 dB steps down to −9 dBm, at which its receiver
still has the margin of roadmap D38 for a full frame through a fade;
heartbeats and ACKs stay at full power. At the margin it lost at every
normal load and won only where main stormed, because the margin is
derived against noise and a fade, not against other frames: a DATA that
arrives at the margin loses the receiver to any later frame within the
capture threshold of it. Five decibels of headroom over the margin, a
guess, gave the loss back on two runs and kept half the gain.

This note derives the headroom instead of guessing it, measures values
spanning the derivation and beyond it on the whole suite, on the base
with D96, and decides whether the form is a win and at what value.

## The headroom, derived

A DATA at headroom `H` is sent so that its receiver's averaged RSSI of
it would read the margin plus `H`. What the frame then has to beat, and
what the margin already covers:

**The capture threshold.** A LoRa receiver keeps the frame it is
receiving against a later frame of the same spreading factor only while
the first is stronger by the capture threshold, `C`, and a later frame
within `C` below it costs it what noise at sensitivity does
(`bark-sim/src/phy.rs`, `capture_db`). The simulator takes `C` as 5 dB.
The datasheet gives no same-SF figure: its co-channel rejection
(Table 3-8, `CCR_L`, 5 dB at SF7, 19 dB at SF12, none at SF5) is
measured with a single tone, referenced to sensitivity +3 dB, and says
how far a tone may sit *above* the wanted signal, the spreading's
rejection of a tone. It is not the margin one same-SF frame needs over
another, which no document we hold gives for this chip. The 5 dB is a
stand-in of the right order until the bench measures it at SF5
(`bark-fw/scripts/measure.md`, `capture_db`). The headroom needs `C`:
5 dB, give or take one.

**The fade the margin does not cover.** D38's margin puts the signal one
standard deviation of the fade, 4 dB, above the level where a full
frame is lost once in a hundred, and covers the DATA's own fade on the
receiver's channel to that standard. A competing frame fades too,
independently: the difference of the two fades has a standard deviation
of 4 × √2 = 5.7 dB, so holding the contest to the same one-standard-
deviation standard takes 1.7 dB more than the margin holds.

**The sender's own uncertainty.** The sender sets the power from the
neighbour's averaged RSSI, `rssi_ema`, each new frame counting half
(spec "Neighbour table"), of heartbeats and ACKs the neighbour sent at
full power on the sender's channel at other times; it takes the link as
symmetric and its own noise floor as the neighbour's. The average of a
half-weight filter under a 4 dB fade has a spread of 4 / √3 = 2.3 dB
(roadmap D68 used the same figure). The two ends' noise floors can
differ, by how much nobody has measured; the simulator's are equal. The
power step, 1 dB, and the reading's 0.5 dB are rounded toward more
power and err safe, and the packet RSSI being signal and noise together
errs the same way (`power-control-2026-11-02.md`). Taking the average's
2.3 dB with the two fades, the contest's spread is
√(4² + 4² + 2.3²) = 6.1 dB, 2.1 dB over the 4 dB the margin holds.

**Against what level.** Which competing frames the headroom must beat
is not in the datasheet. The DATA at the margin arrives, on average,
4.4 dB above sensitivity (1.9 dB above the floor, which is 2.5 dB
above sensitivity at SF5). Against the faintest frame a receiver can
hear, one at sensitivity, it needs `C` − 4.4 ≈ 0.6 dB plus the 2.1 dB of
spread: about 3 dB. Against a frame that arrives at the margin level
itself, another node's controlled DATA to the same receiver or a
heartbeat from the edge of reach, it needs `C` plus the spread: about
7 dB. Against a frame from a nearer node, which arrives higher, no
finite headroom helps; with the path-loss exponent of the scenarios,
3.6, and nodes spread evenly, the share of the frames a receiver can
hear that arrive within `X` dB of sensitivity is 1 − 10^(−X / 18): 32 %
at 3 dB, 47 % at 5, 64 % at 8, 72 % at 10, 85 % at 15. Each decibel of
headroom takes 12 % off the competitors a frame can still lose to, and
gives back a twelfth of the power the rule turned down.

So the derivation gives a range, 3 to 7 dB, with 7 dB its own value,
and the measurement takes 3, 5, 7 and 8 dB, 10 dB on S4 at three times
the rate, and the rule at the margin, 0 dB, again on S4 at three times
the rate and the movers, where it lost most.

## How it was measured

Release simulators of main `c0ec923` and the branch, which is main with
the mechanism of `power-control-2026-11-02.md` brought over (the frame's
power, the drivers that honour it, the simulator's level offset and
count of the powers used), the ACK's research switch removed, and the
headroom a scenario knob for the measurement only. The same scenarios
and seeds as the first measurement, two runs at a time. Main's ring run
matched the baseline runs of another agent at `c0ec923` byte for byte
(the messages CSV), so the main columns are those runs where they
existed, and were run here where they did not (the movers, S4 at three
times the rate seed 2).

| run | scenario | seed |
|---|---|---|
| dp1, dp2, dp7 | `scenarios/dense-power.toml`: 28 km² of city, 7223 of the people's nodes switching between USB and battery, a fifth of the people walking, one settled hour after 2700 s | 1, 2, 7 |
| dpx2-s1, dpx2-s7 | the same at twice the texts per node | 1, 7 |
| s4x1 | `scenarios/s4-capacity.toml` at the design rate | 1 |
| s4x3-s1, s4x3-s2 | S4 at three times the rate | 1, 2 |
| ring | `scenarios/ring.toml` | 1 |
| movers | `scenarios/dense-movers.toml` | 1 |
| mesh-burst-s1 | `scenarios/mesh-burst.toml` | 1 |

The figures are those of the first measurement. The frame fates are of
frames addressed to nodes that are not sleepy, after the settle time:
received; lost while the receiver listened and was free of any frame
when it started, a loss to noise, to a fade or to a stronger frame that
came later; lost while the receiver was receiving another frame; lost
while it was deaf; or sent while it listened elsewhere. The power
histogram counts DATA frames after the settle time by their steps under
full power, +10 dBm, step 19 the amplifier's floor, −9 dBm.

## The tables

Each cell is main / 0 / 3 / 5 / 7 / 8 / 10 dB of headroom, '-' where a
value was not run. S4 at three times the rate, the movers and S4 at the
design rate:

| | s4x3-s1 | s4x3-s2 | movers | s4x1 |
|---|---|---|---|---|
| texts sent in the settled hour | 49047 / 48313 / 48979 / 48936 / 49157 / 49193 / 48915 | 48449 / - / 48124 / 48256 / - / 48333 / - | 17893 / 17893 / 17893 / 17893 / 17893 / 17894 / - | 17459 / - / 17454 / 17457 / - / 17458 / - |
| delivered, settled hour % | 65.7 / 53.4 / 62.1 / 62.7 / 64.2 / 65.0 / 63.6 | 62.4 / - / 55.7 / 58.7 / - / 60.1 / - | 96.4 / 94.9 / 95.6 / 95.8 / 96.0 / 96.0 / - | 93.6 / - / 93.1 / 93.1 / - / 93.4 / - |
| delivered, whole run % | 70.7 / 60.0 / 68.4 / 68.9 / 70.7 / 70.9 / 69.3 | 69.7 / - / 64.4 / 66.6 / - / 68.3 / - | 95.7 / 94.3 / 94.8 / 95.1 / 95.2 / 95.2 / - | 93.6 / - / 93.2 / 93.1 / - / 93.5 / - |
| latency p50 s | 86.2 / 173.4 / 102.3 / 99.7 / 79.1 / 83.6 / 96.6 | 91.4 / - / 124.6 / 109.9 / - / 98.5 / - | 5.4 / 9.0 / 6.5 / 5.7 / 5.7 / 5.7 / - | 1.4 / - / 1.5 / 1.4 / - / 1.4 / - |
| latency p90 s | 584.3 / 674.9 / 593.8 / 592.3 / 565.6 / 580.9 / 596.0 | 594.7 / - / 644.4 / 627.7 / - / 612.8 / - | 51.4 / 88.7 / 65.8 / 62.1 / 62.6 / 61.7 / - | 189.0 / - / 194.7 / 190.3 / - / 188.9 / - |
| airtime, whole run s | 149057 / 172465 / 154173 / 152992 / 148691 / 149105 / 151901 | 150087 / - / 160886 / 156178 / - / 154087 / - | 181789 / 199574 / 185799 / 184291 / 188225 / 182325 / - | 43665 / - / 44366 / 43142 / - / 42901 / - |
| hand-overs taken | 391749 / 400870 / 395120 / 396356 / 394427 / 395827 / 398115 | 389980 / - / 389370 / 391055 / - / 393861 / - | 321202 / 331401 / 321734 / 320498 / 321561 / 319741 / - | 116455 / - / 117352 / 116639 / - / 116485 / - |
| hand-overs failed | 137746 / 196042 / 149734 / 144233 / 135304 / 133809 / 139520 | 134523 / - / 156746 / 145214 / - / 136240 / - | 278323 / 355035 / 297946 / 290279 / 301686 / 283750 / - | 19432 / - / 20085 / 18651 / - / 18216 / - |
| failed, receiver had it | 28397 / 27567 / 25861 / 26466 / 25840 / 26058 / 28009 | 28863 / - / 28658 / 28614 / - / 28495 / - | 147279 / 165643 / 151715 / 151067 / 163912 / 147579 / - | 3944 / - / 3640 / 3553 / - / 3513 / - |
| returns taken from a child | 337 / 243 / 294 / 288 / 253 / 299 / 341 | 403 / - / 378 / 386 / - / 393 / - | 3273 / 2806 / 2930 / 3007 / 3549 / 3085 / - | 63 / - / 36 / 49 / - / 24 / - |
| copies answered (not returns) | 40748 / 46860 / 40914 / 43346 / 41081 / 40408 / 43909 | 57722 / - / 62204 / 63609 / - / 62982 / - | 209594 / 188984 / 199887 / 205124 / 219807 / 203004 / - | 8753 / - / 7300 / 6655 / - / 6118 / - |
| of those merged (case 4) | 974 / 1044 / 877 / 974 / 915 / 886 / 1076 | 1224 / - / 1261 / 1214 / - / 1203 / - | 1514 / 1515 / 1428 / 1497 / 1577 / 1462 / - | 34 / - / 19 / 21 / - / 11 / - |
| of those seen (case 6) | 39774 / 45816 / 40037 / 42372 / 40166 / 39522 / 42833 | 56498 / - / 60943 / 62394 / - / 61779 / - | 208047 / 187442 / 198429 / 203605 / 218201 / 201508 / - | 8719 / - / 7281 / 6634 / - / 6107 / - |
| seen ACKs sent / acted on | 45113 / 38916 / 49654 / 45084 / 44434 / 39382 / 47353 / 41603 / 44933 / 39423 / 44373 / 38819 / 48406 / 41956 | 63334 / 55474 / - / 67171 / 59953 / 69149 / 61319 / - / 68666 / 60720 / - | 236830 / 198608 / 213216 / 178778 / 226307 / 189142 / 232675 / 194252 / 250431 / 207627 / 229915 / 191940 / - | 9519 / 8600 / - / 7863 / 7201 / 7217 / 6528 / - / 6546 / 6043 / - |
| hand-overs held back by the bucket | 704513 / 991775 / 759417 / 749618 / 693560 / 694945 / 745674 | 721530 / - / 863976 / 803353 / - / 771360 / - | 7829 / 11276 / 11285 / 9134 / 12358 / 9439 / - | 594 / - / 114 / 117 / - / 90 / - |
| neighbours demoted | 40129 / 60341 / 43637 / 41497 / 38554 / 38289 / 40006 | 38796 / - / 45359 / 41362 / - / 38648 / - | 107642 / 142140 / 117175 / 112860 / 116331 / 111194 / - | 7267 / - / 7563 / 6989 / - / 6850 / - |
| copies dropped: hop cap | 40 / 23 / 28 / 40 / 42 / 41 / 43 | 70 / - / 60 / 70 / - / 69 / - | 1541 / 1180 / 1483 / 1462 / 1747 / 1544 / - | 41 / - / 36 / 33 / - / 29 / - |
| copies dropped: TTL | 4505 / 6924 / 5051 / 4863 / 4535 / 4377 / 4747 | 4422 / - / 5469 / 4944 / - / 4583 / - | 821 / 1141 / 855 / 976 / 932 / 858 / - | 333 / - / 352 / 362 / - / 339 / - |
| copies dropped: return seen | 0 / 0 / 0 / 0 / 0 / 0 / 0 | 0 / - / 0 / 0 / - / 0 / - | 0 / 1 / 1 / 0 / 0 / 0 / - | 0 / - / 0 / 0 / - / 0 / - |
| copies dropped: no route | 164 / 103 / 164 / 168 / 181 / 170 / 162 | 185 / - / 157 / 190 / - / 199 / - | 16 / 15 / 18 / 26 / 20 / 15 / - | 101 / - / 102 / 103 / - / 93 / - |
| copies dropped: return failed | 0 / 2 / 2 / 3 / 3 / 1 / 2 | 4 / - / 10 / 3 / - / 7 / - | 38 / 31 / 21 / 30 / 51 / 39 / - | 0 / - / 0 / 0 / - / 0 / - |
| texts lost: hop cap | 26 / 17 / 18 / 31 / 38 / 32 / 36 | 51 / - / 46 / 54 / - / 52 / - | 159 / 137 / 166 / 161 / 153 / 153 / - | 33 / - / 30 / 25 / - / 23 / - |
| texts lost: TTL | 4349 / 6653 / 4891 / 4673 / 4376 / 4208 / 4553 | 4155 / - / 5131 / 4643 / - / 4294 / - | 115 / 215 / 177 / 157 / 156 / 135 / - | 310 / - / 333 / 347 / - / 319 / - |
| texts lost: return seen | 0 / 0 / 0 / 0 / 0 / 0 / 0 | 0 / - / 0 / 0 / - / 0 / - | 0 / 0 / 0 / 0 / 0 / 0 / - | 0 / - / 0 / 0 / - / 0 / - |
| texts lost: no route | 11 / 11 / 14 / 20 / 13 / 12 / 16 | 12 / - / 17 / 10 / - / 12 / - | 3 / 3 / 3 / 2 / 2 / 3 / - | 5 / - / 6 / 6 / - / 2 / - |
| texts lost: return failed | 0 / 1 / 1 / 1 / 3 / 1 / 1 | 4 / - / 8 / 3 / - / 4 / - | 0 / 1 / 1 / 2 / 1 / 1 / - | 0 / - / 0 / 0 / - / 0 / - |
| texts that waited at their origin | 185826 / 253805 / 195366 / 192272 / 179054 / 181612 / 192935 | 191976 / - / 225176 / 210041 / - / 204762 / - | 119 / 145 / 122 / 148 / 119 / 109 / - | 328 / - / 181 / 189 / - / 189 / - |
| relays ever at their cap | 153 / 192 / 158 / 164 / 152 / 143 / 155 | 155 / - / 166 / 163 / - / 157 / - | 143 / 201 / 195 / 174 / 235 / 178 / - | 3 / - / 0 / 0 / - / 0 / - |
| largest settled airtime of a node % | 9.98 / 9.96 / 9.96 / 9.98 / 9.99 / 9.98 / 9.97 | 9.98 / - / 9.97 / 9.98 / - / 9.99 / - | 5.05 / 5.38 / 5.31 / 4.48 / 5.14 / 4.74 / - | 5.44 / - / 5.42 / 5.50 / - / 5.35 / - |
| heartbeats received % | 88.7 / 88.8 / 89.1 / 88.9 / 89.0 / 89.0 / 88.7 | 88.8 / - / 88.9 / 89.0 / - / 88.9 / - | 89.5 / 89.6 / 89.6 / 89.7 / 89.5 / 89.5 / - | 94.3 / - / 94.5 / 94.5 / - / 94.5 / - |
| DATA mean power dBm / at full % | - / 3.2 / 18.4 / 5.4 / 44.3 / 6.4 / 55.9 / 7.2 / 65.3 / 7.5 / 69.2 / 8.0 / 75.5 | - / - / 5.4 / 44.5 / 6.4 / 56.3 / - / 7.5 / 69.1 / - | - / 6.1 / 27.9 / 7.8 / 66.2 / 8.4 / 77.1 / 8.9 / 84.5 / 9.0 / 86.4 / - | - / - / 6.8 / 58.0 / 7.6 / 68.9 / - / 8.3 / 79.1 / - |
| ACK mean power dBm / at full % | - / 10.0 / 100.0 / 10.0 / 100.0 / 10.0 / 100.0 / 10.0 / 100.0 / 10.0 / 100.0 / 10.0 / 100.0 | - / - / 10.0 / 100.0 / 10.0 / 100.0 / - / 10.0 / 100.0 / - | - / 10.0 / 100.0 / 10.0 / 100.0 / 10.0 / 100.0 / 10.0 / 100.0 / 10.0 / 100.0 / - | - / - / 10.0 / 100.0 / 10.0 / 100.0 / - / 10.0 / 100.0 / - |
| a sleepy node's radio current mA (mean / median) | 0.614 / 0.542 / 0.594 / 0.529 / 0.604 / 0.538 / 0.601 / 0.535 / 0.608 / 0.541 / 0.609 / 0.544 / 0.609 / 0.540 | 0.615 / 0.546 / - / 0.607 / 0.541 / 0.611 / 0.543 / - / 0.608 / 0.541 / - | 0.450 / 0.382 / 0.448 / 0.378 / 0.450 / 0.380 / 0.449 / 0.380 / 0.451 / 0.382 / 0.451 / 0.383 / - | 0.536 / 0.465 / - / 0.530 / 0.458 / 0.530 / 0.459 / - / 0.532 / 0.462 / - |
| a relay's radio current mA | 7.720 / 7.934 / 7.775 / 7.766 / 7.731 / 7.717 / 7.745 | 7.717 / - / 7.806 / 7.762 / - / 7.751 / - | 5.921 / 5.936 / 5.928 / 5.922 / 5.929 / 5.926 / - | 6.304 / - / 6.304 / 6.297 / - / 6.293 / - |

The city at the design rate and at twice it:

| | dp1 | dp2 | dp7 | dpx2-s1 | dpx2-s7 |
|---|---|---|---|---|---|
| texts sent in the settled hour | 18234 / - / 18234 / 18234 / 18233 / 18234 / - | 18199 / - / - / - / 18199 / - / - | 18190 / - / - / - / 18190 / - / - | 36542 / - / - / 36588 / 36578 / 36572 / - | 36581 / - / - / - / 36633 / - / - |
| delivered, settled hour % | 97.9 / - / 97.8 / 97.8 / 97.8 / 97.8 / - | 98.2 / - / - / - / 98.1 / - / - | 98.1 / - / - / - / 98.1 / - / - | 89.4 / - / - / 92.1 / 91.3 / 90.9 / - | 89.3 / - / - / - / 90.9 / - / - |
| delivered, whole run % | 96.5 / - / 96.4 / 96.4 / 96.4 / 96.4 / - | 96.7 / - / - / - / 96.5 / - / - | 96.9 / - / - / - / 96.9 / - / - | 90.9 / - / - / 92.7 / 92.1 / 91.8 / - | 90.5 / - / - / - / 91.7 / - / - |
| latency p50 s | 10.0 / - / 11.9 / 11.1 / 10.8 / 10.9 / - | 10.1 / - / - / - / 10.8 / - / - | 9.7 / - / - / - / 10.6 / - / - | 46.3 / - / - / 38.2 / 41.0 / 40.6 / - | 48.9 / - / - / - / 44.1 / - / - |
| latency p90 s | 44.6 / - / 56.9 / 54.0 / 47.7 / 47.6 / - | 42.9 / - / - / - / 44.8 / - / - | 43.6 / - / - / - / 50.6 / - / - | 346.1 / - / - / 305.8 / 318.4 / 316.2 / - | 343.0 / - / - / - / 321.0 / - / - |
| airtime, whole run s | 281452 / - / 284825 / 291773 / 288004 / 286378 / - | 279551 / - / - / - / 285805 / - / - | 268776 / - / - / - / 275492 / - / - | 1099444 / - / - / 996899 / 1049725 / 1062253 / - | 1027860 / - / - / - / 1003329 / - / - |
| hand-overs taken | 411933 / - / 410760 / 418731 / 419404 / 417727 / - | 418550 / - / - / - / 422058 / - / - | 402054 / - / - / - / 406895 / - / - | 1888673 / - / - / 1678889 / 1783914 / 1800205 / - | 1801163 / - / - / - / 1743191 / - / - |
| hand-overs failed | 475022 / - / 503264 / 513668 / 500263 / 497254 / - | 466899 / - / - / - / 494942 / - / - | 445409 / - / - / - / 471360 / - / - | 2745283 / - / - / 2523448 / 2648478 / 2672972 / - | 2539614 / - / - / - / 2506212 / - / - |
| failed, receiver had it | 111672 / - / 104783 / 116963 / 113637 / 109960 / - | 109841 / - / - / - / 111554 / - / - | 100963 / - / - / - / 101530 / - / - | 559962 / - / - / 482163 / 522781 / 536339 / - | 496196 / - / - / - / 479263 / - / - |
| returns taken from a child | 3502 / - / 2847 / 3760 / 3283 / 3400 / - | 3390 / - / - / - / 3448 / - / - | 2966 / - / - / - / 2957 / - / - | 10035 / - / - / 8104 / 8789 / 9478 / - | 8571 / - / - / - / 7950 / - / - |
| copies answered (not returns) | 247724 / - / 213294 / 250391 / 243257 / 237148 / - | 244375 / - / - / - / 236866 / - / - | 211608 / - / - / - / 210900 / - / - | 1035073 / - / - / 864893 / 946483 / 988439 / - | 927456 / - / - / - / 878911 / - / - |
| of those merged (case 4) | 2326 / - / 1842 / 2366 / 2166 / 2248 / - | 2013 / - / - / - / 2089 / - / - | 1874 / - / - / - / 1848 / - / - | 34848 / - / - / 22977 / 27257 / 28006 / - | 30786 / - / - / - / 25249 / - / - |
| of those seen (case 6) | 245313 / - / 211385 / 247932 / 241014 / 234834 / - | 242291 / - / - / - / 234720 / - / - | 209665 / - / - / - / 208982 / - / - | 1000020 / - / - / 841736 / 919035 / 960260 / - | 896462 / - / - / - / 853494 / - / - |
| seen ACKs sent / acted on | 277615 / 234741 / - / 238194 / 202513 / 280316 / 237034 / 272752 / 230468 / 265907 / 224348 / - | 273928 / 232114 / - / - / - / 266340 / 224413 / - / - | 237232 / 200743 / - / - / - / 236640 / 199989 / - / - | 1159423 / 948266 / - / - / 965002 / 801116 / 1059814 / 873112 / 1107570 / 912426 / - | 1034533 / 852239 / - / - / - / 981584 / 812540 / - / - |
| hand-overs held back by the bucket | 17680 / - / 17838 / 21000 / 17283 / 18102 / - | 18741 / - / - / - / 17199 / - / - | 15377 / - / - / - / 17898 / - / - | 1772894 / - / - / 1443893 / 1574012 / 1589815 / - | 1738903 / - / - / - / 1567467 / - / - |
| neighbours demoted | 198076 / - / 213418 / 214921 / 210176 / 209167 / - | 193762 / - / - / - / 207469 / - / - | 186739 / - / - / - / 199164 / - / - | 1172432 / - / - / 1088680 / 1139702 / 1149185 / - | 1087883 / - / - / - / 1077910 / - / - |
| copies dropped: hop cap | 1435 / - / 1235 / 1487 / 1345 / 1469 / - | 1586 / - / - / - / 1560 / - / - | 1312 / - / - / - / 1283 / - / - | 2754 / - / - / 2483 / 2537 / 2521 / - | 2537 / - / - / - / 2559 / - / - |
| copies dropped: TTL | 755 / - / 768 / 818 / 867 / 723 / - | 793 / - / - / - / 727 / - / - | 651 / - / - / - / 657 / - / - | 34682 / - / - / 19944 / 24320 / 25159 / - | 30481 / - / - / - / 22933 / - / - |
| copies dropped: return seen | 0 / - / 0 / 0 / 1 / 2 / - | 0 / - / - / - / 2 / - / - | 1 / - / - / - / 0 / - / - | 2 / - / - / 0 / 2 / 0 / - | 2 / - / - / - / 3 / - / - |
| copies dropped: no route | 19 / - / 19 / 22 / 21 / 17 / - | 18 / - / - / - / 12 / - / - | 22 / - / - / - / 25 / - / - | 67 / - / - / 60 / 58 / 59 / - | 48 / - / - / - / 58 / - / - |
| copies dropped: return failed | 49 / - / 58 / 58 / 64 / 53 / - | 39 / - / - / - / 46 / - / - | 44 / - / - / - / 33 / - / - | 208 / - / - / 203 / 211 / 201 / - | 218 / - / - / - / 196 / - / - |
| texts lost: hop cap | 258 / - / 225 / 234 / 241 / 261 / - | 255 / - / - / - / 252 / - / - | 230 / - / - / - / 211 / - / - | 374 / - / - / 385 / 376 / 384 / - | 428 / - / - / - / 422 / - / - |
| texts lost: TTL | 78 / - / 85 / 83 / 94 / 89 / - | 98 / - / - / - / 111 / - / - | 78 / - / - / - / 89 / - / - | 1576 / - / - / 1074 / 1266 / 1277 / - | 1557 / - / - / - / 1236 / - / - |
| texts lost: return seen | 0 / - / 0 / 0 / 0 / 0 / - | 0 / - / - / - / 0 / - / - | 0 / - / - / - / 0 / - / - | 1 / - / - / 0 / 0 / 0 / - | 0 / - / - / - / 0 / - / - |
| texts lost: no route | 3 / - / 4 / 3 / 5 / 4 / - | 4 / - / - / - / 4 / - / - | 6 / - / - / - / 4 / - / - | 17 / - / - / 13 / 12 / 14 / - | 18 / - / - / - / 15 / - / - |
| texts lost: return failed | 6 / - / 4 / 4 / 5 / 2 / - | 2 / - / - / - / 4 / - / - | 5 / - / - / - / 4 / - / - | 40 / - / - / 52 / 39 / 40 / - | 54 / - / - / - / 39 / - / - |
| texts that waited at their origin | 92 / - / 126 / 72 / 63 / 66 / - | 79 / - / - / - / 90 / - / - | 58 / - / - / - / 95 / - / - | 7850 / - / - / 2935 / 3219 / 3816 / - | 8431 / - / - / - / 3493 / - / - |
| relays ever at their cap | 103 / - / 76 / 108 / 78 / 80 / - | 77 / - / - / - / 90 / - / - | 70 / - / - / - / 76 / - / - | 813 / - / - / 672 / 698 / 772 / - | 689 / - / - / - / 675 / - / - |
| largest settled airtime of a node % | 4.45 / - / 4.70 / 4.40 / 4.72 / 4.47 / - | 3.97 / - / - / - / 4.07 / - / - | 4.10 / - / - / - / 4.07 / - / - | 9.98 / - / - / 9.97 / 9.97 / 9.99 / - | 9.99 / - / - / - / 9.94 / - / - |
| heartbeats received % | 85.6 / - / 85.9 / 85.7 / 85.7 / 85.4 / - | 85.9 / - / - / - / 85.9 / - / - | 85.9 / - / - / - / 85.9 / - / - | 68.1 / - / - / 70.8 / 69.8 / 69.4 / - | 69.9 / - / - / - / 71.2 / - / - |
| DATA mean power dBm / at full % | - / - / 7.9 / 64.3 / 8.5 / 77.0 / 9.0 / 84.8 / 9.1 / 87.3 / - | - / - / - / - / 9.0 / 85.1 / - / - | - / - / - / - / 9.0 / 85.2 / - / - | - / - / - / 8.4 / 74.0 / 8.9 / 82.4 / 9.1 / 85.3 / - | - / - / - / - / 8.9 / 82.5 / - / - |
| ACK mean power dBm / at full % | - / - / 10.0 / 100.0 / 10.0 / 100.0 / 10.0 / 100.0 / 10.0 / 100.0 / - | - / - / - / - / 10.0 / 100.0 / - / - | - / - / - / - / 10.0 / 100.0 / - / - | - / - / - / 10.0 / 100.0 / 10.0 / 100.0 / 10.0 / 100.0 / - | - / - / - / - / 10.0 / 100.0 / - / - |
| a sleepy node's radio current mA (mean / median) | 2.324 / 0.543 / - / 2.325 / 0.542 / 2.326 / 0.543 / 2.326 / 0.543 / 2.327 / 0.543 / - | 2.320 / 0.543 / - / - / - / 2.323 / 0.543 / - / - | 2.288 / 0.543 / - / - / - / 2.289 / 0.542 / - / - | 2.616 / 0.745 / - / - / 2.572 / 0.681 / 2.592 / 0.703 / 2.598 / 0.718 / - | 2.552 / 0.711 / - / - / - / 2.533 / 0.677 / - / - |
| a relay's radio current mA | 5.914 / - / 5.918 / 5.919 / 5.918 / 5.920 / - | 5.912 / - / - / - / 5.917 / - / - | 5.911 / - / - / - / 5.916 / - / - | 6.411 / - / - / 6.351 / 6.380 / 6.391 / - | 6.355 / - / - / - / 6.339 / - / - |

The ring and the mesh:

| | ring | mesh-burst-s1 |
|---|---|---|
| texts sent in the settled hour | 2964 / - / 3050 / 2842 / 3025 / 3025 / - | 100 / - / 100 / 100 / 100 / 100 / - |
| delivered, settled hour % | 92.6 / - / 90.6 / 96.5 / 92.1 / 92.1 / - | 99.0 / - / 99.0 / 98.0 / 99.0 / 99.0 / - |
| delivered, whole run % | 93.3 / - / 92.3 / 96.3 / 92.8 / 92.8 / - | 99.0 / - / 99.0 / 98.0 / 99.0 / 99.0 / - |
| latency p50 s | 41.5 / - / 37.0 / 55.9 / 51.9 / 51.9 / - | 3.9 / - / 3.0 / 3.6 / 2.7 / 2.7 / - |
| latency p90 s | 159.2 / - / 151.7 / 160.1 / 144.9 / 144.9 / - | 10.2 / - / 7.9 / 8.0 / 5.8 / 5.8 / - |
| airtime, whole run s | 1807 / - / 1825 / 1786 / 1815 / 1815 / - | 253 / - / 227 / 252 / 230 / 230 / - |
| hand-overs taken | - / - / - / - / - / - / - | 688 / - / 592 / 670 / 542 / 542 / - |
| hand-overs failed | - / - / - / - / - / - / - | 92 / - / 71 / 140 / 50 / 50 / - |
| failed, receiver had it | - / - / - / - / - / - / - | 51 / - / 34 / 92 / 5 / 5 / - |
| returns taken from a child | - / - / - / - / - / - / - | 6 / - / 2 / 7 / 1 / 1 / - |
| copies answered (not returns) | - / - / - / - / - / - / - | 245 / - / 157 / 205 / 103 / 103 / - |
| of those merged (case 4) | - / - / - / - / - / - / - | 1 / - / 1 / 0 / 0 / 0 / - |
| of those seen (case 6) | - / - / - / - / - / - / - | 244 / - / 156 / 205 / 103 / 103 / - |
| seen ACKs sent / acted on | 0 / 0 / - / 0 / 0 / 0 / 0 / 1 / 1 / 1 / 1 / - | 248 / 243 / - / 157 / 155 / 215 / 202 / 103 / 103 / 103 / 103 / - |
| hand-overs held back by the bucket | - / - / - / - / - / - / - | 0 / - / 0 / 0 / 5 / 5 / - |
| neighbours demoted | - / - / - / - / - / - / - | 16 / - / 12 / 23 / 8 / 8 / - |
| copies dropped: hop cap | 0 / - / 0 / 0 / 0 / 0 / - | 0 / - / 0 / 0 / 0 / 0 / - |
| copies dropped: TTL | 0 / - / 0 / 0 / 0 / 0 / - | 1 / - / 0 / 2 / 0 / 0 / - |
| copies dropped: return seen | 0 / - / 0 / 0 / 1 / 1 / - | 0 / - / 0 / 0 / 0 / 0 / - |
| copies dropped: no route | 12 / - / 12 / 12 / 12 / 12 / - | 0 / - / 0 / 0 / 0 / 0 / - |
| copies dropped: return failed | 2 / - / 0 / 0 / 0 / 0 / - | 0 / - / 0 / 0 / 1 / 1 / - |
| texts lost: hop cap | 0 / - / 0 / 0 / 0 / 0 / - | 0 / - / 0 / 0 / 0 / 0 / - |
| texts lost: TTL | 0 / - / 0 / 0 / 0 / 0 / - | 1 / - / 0 / 1 / 0 / 0 / - |
| texts lost: return seen | 0 / - / 0 / 0 / 0 / 0 / - | 0 / - / 0 / 0 / 0 / 0 / - |
| texts lost: no route | 12 / - / 12 / 12 / 12 / 12 / - | 0 / - / 0 / 0 / 0 / 0 / - |
| texts lost: return failed | 2 / - / 0 / 0 / 0 / 0 / - | 0 / - / 0 / 0 / 1 / 1 / - |
| texts that waited at their origin | 2269 / - / 2281 / 2356 / 2425 / 2425 / - | 0 / - / 0 / 0 / 0 / 0 / - |
| relays ever at their cap | - / - / - / - / - / - / - | 0 / - / 0 / 0 / 0 / 0 / - |
| largest settled airtime of a node % | 9.89 / - / 9.94 / 10.08 / 10.02 / 10.02 / - | 0.66 / - / 0.53 / 0.57 / 0.55 / 0.55 / - |
| heartbeats received % | 91.0 / - / 89.3 / 87.7 / 92.0 / 92.0 / - | 99.5 / - / 99.4 / 99.6 / 99.7 / 99.7 / - |
| DATA mean power dBm / at full % | - / - / 1.1 / 0.0 / 2.5 / 0.0 / 3.0 / 63.4 / 3.0 / 63.4 / - | - / - / 4.7 / 52.3 / 6.2 / 64.9 / 5.4 / 64.1 / 5.6 / 66.5 / - |
| ACK mean power dBm / at full % | - / - / 10.0 / 100.0 / 10.0 / 100.0 / 10.0 / 100.0 / 10.0 / 100.0 / - | - / - / 10.0 / 100.0 / 10.0 / 100.0 / 10.0 / 100.0 / 10.0 / 100.0 / - |
| a sleepy node's radio current mA (mean / median) | 2.364 / 2.378 / - / 2.465 / 2.274 / 2.164 / 2.213 / 2.362 / 2.211 / 2.362 / 2.211 / - | 0.257 / 0.255 / - / 0.256 / 0.254 / 0.256 / 0.255 / 0.270 / 0.254 / 0.270 / 0.254 / - |
| a relay's radio current mA | 8.354 / - / 8.375 / 8.332 / 8.381 / 8.381 / - | 5.900 / - / 5.888 / 5.899 / 5.882 / 5.882 / - |

**Frame fates**, of frames addressed to nodes that are not sleepy, after
the settle time: received; lost while the receiver listened and was
free when the frame started; lost while it was receiving another frame;
lost while it was deaf; sent while it listened on another channel. The
last column is how the DATA received were answered.

**s4x3-s1**

| | received | lost free | lost receiving | lost deaf | other channel | DATA answered: taken / refused / late / none |
|---|---|---|---|---|---|---|
| main DATA | 499820 (56.2 %) | 176392 (19.8 %) | 126649 (14.2 %) | 82370 (9.3 %) | 3786 (0.4 %) | 434841 / 64590 / 1 / 0 |
| main ACK | 429895 (86.1 %) | 67269 (13.5 %) | 2263 (0.5 %) | 0 (0.0 %) | 3 (0.0 %) |  |
| 0dB DATA | 521869 (49.0 %) | 291589 (27.4 %) | 147581 (13.9 %) | 100742 (9.5 %) | 2670 (0.3 %) | 442194 / 79492 / 3 / 0 |
| 0dB ACK | 468689 (89.8 %) | 51180 (9.8 %) | 1813 (0.3 %) | 1 (0.0 %) | 4 (0.0 %) |  |
| 3dB DATA | 497243 (53.8 %) | 208748 (22.6 %) | 128518 (13.9 %) | 85726 (9.3 %) | 3271 (0.4 %) | 430644 / 66296 / 1 / 0 |
| 3dB ACK | 438425 (88.2 %) | 56642 (11.4 %) | 1869 (0.4 %) | 2 (0.0 %) | 3 (0.0 %) |  |
| 5dB DATA | 504859 (55.3 %) | 191326 (21.0 %) | 127156 (13.9 %) | 84818 (9.3 %) | 4036 (0.4 %) | 437574 / 66987 / 2 / 0 |
| 5dB ACK | 441637 (87.5 %) | 60803 (12.1 %) | 2122 (0.4 %) | 0 (0.0 %) | 1 (0.0 %) |  |
| 7dB DATA | 495668 (56.3 %) | 175989 (20.0 %) | 123733 (14.1 %) | 81196 (9.2 %) | 3959 (0.4 %) | 432853 / 62471 / 2 / 0 |
| 7dB ACK | 432046 (87.2 %) | 61138 (12.3 %) | 2134 (0.4 %) | 2 (0.0 %) | 3 (0.0 %) |  |
| 8dB DATA | 497558 (56.7 %) | 174446 (19.9 %) | 121531 (13.8 %) | 81208 (9.2 %) | 3447 (0.4 %) | 434214 / 63030 / 3 / 0 |
| 8dB ACK | 432721 (87.0 %) | 62315 (12.5 %) | 2205 (0.4 %) | 1 (0.0 %) | 4 (0.0 %) |  |
| 10dB DATA | 512328 (56.5 %) | 176287 (19.4 %) | 128640 (14.2 %) | 84952 (9.4 %) | 4264 (0.5 %) | 444609 / 67400 / 3 / 0 |
| 10dB ACK | 443248 (86.6 %) | 66587 (13.0 %) | 2173 (0.4 %) | 0 (0.0 %) | 3 (0.0 %) |  |
**movers**

| | received | lost free | lost receiving | lost deaf | other channel | DATA answered: taken / refused / late / none |
|---|---|---|---|---|---|---|
| main DATA | 594873 (41.6 %) | 689720 (48.2 %) | 92707 (6.5 %) | 39289 (2.7 %) | 14821 (1.0 %) | 594825 / 0 / 2 / 0 |
| main ACK | 499605 (84.0 %) | 94395 (15.9 %) | 824 (0.1 %) | 0 (0.0 %) | 2 (0.0 %) |  |
| 0dB DATA | 577298 (34.7 %) | 927146 (55.7 %) | 101485 (6.1 %) | 47082 (2.8 %) | 12096 (0.7 %) | 577273 / 0 / 0 / 0 |
| 0dB ACK | 490472 (85.0 %) | 86075 (14.9 %) | 724 (0.1 %) | 0 (0.0 %) | 1 (0.0 %) |  |
| 3dB DATA | 583172 (39.3 %) | 757343 (51.0 %) | 90761 (6.1 %) | 41189 (2.8 %) | 11398 (0.8 %) | 583136 / 0 / 0 / 0 |
| 3dB ACK | 491023 (84.2 %) | 91386 (15.7 %) | 724 (0.1 %) | 0 (0.0 %) | 2 (0.0 %) |  |
| 5dB DATA | 589340 (40.3 %) | 730962 (49.9 %) | 90842 (6.2 %) | 40594 (2.8 %) | 12182 (0.8 %) | 589307 / 0 / 1 / 0 |
| 5dB ACK | 494628 (83.9 %) | 93867 (15.9 %) | 810 (0.1 %) | 0 (0.0 %) | 3 (0.0 %) |  |
| 7dB DATA | 609693 (40.3 %) | 755537 (50.0 %) | 93338 (6.2 %) | 41319 (2.7 %) | 12587 (0.8 %) | 609648 / 0 / 0 / 0 |
| 7dB ACK | 509079 (83.5 %) | 99760 (16.4 %) | 805 (0.1 %) | 0 (0.0 %) | 2 (0.0 %) |  |
| 8dB DATA | 586573 (40.8 %) | 711330 (49.5 %) | 89695 (6.2 %) | 39634 (2.8 %) | 11159 (0.8 %) | 586539 / 0 / 1 / 0 |
| 8dB ACK | 491354 (83.8 %) | 94390 (16.1 %) | 792 (0.1 %) | 0 (0.0 %) | 2 (0.0 %) |  |
**dp1**

| | received | lost free | lost receiving | lost deaf | other channel | DATA answered: taken / refused / late / none |
|---|---|---|---|---|---|---|
| main DATA | 272610 (43.6 %) | 270298 (43.3 %) | 55740 (8.9 %) | 18695 (3.0 %) | 7524 (1.2 %) | 272595 / 0 / 0 / 0 |
| main ACK | 228405 (83.8 %) | 43577 (16.0 %) | 611 (0.2 %) | 0 (0.0 %) | 1 (0.0 %) |  |
| 3dB DATA | 259588 (41.4 %) | 287172 (45.8 %) | 54785 (8.7 %) | 18858 (3.0 %) | 5951 (1.0 %) | 259575 / 0 / 0 / 0 |
| 3dB ACK | 219049 (84.4 %) | 39951 (15.4 %) | 572 (0.2 %) | 0 (0.0 %) | 3 (0.0 %) |  |
| 5dB DATA | 278187 (42.2 %) | 294888 (44.7 %) | 58429 (8.9 %) | 20575 (3.1 %) | 7615 (1.2 %) | 278175 / 0 / 0 / 0 |
| 5dB ACK | 233312 (83.9 %) | 44267 (15.9 %) | 595 (0.2 %) | 0 (0.0 %) | 1 (0.0 %) |  |
| 7dB DATA | 276615 (43.0 %) | 282071 (43.9 %) | 56588 (8.8 %) | 20145 (3.1 %) | 7706 (1.2 %) | 276598 / 0 / 0 / 0 |
| 7dB ACK | 231567 (83.7 %) | 44400 (16.1 %) | 630 (0.2 %) | 0 (0.0 %) | 1 (0.0 %) |  |
| 8dB DATA | 271333 (43.2 %) | 274206 (43.7 %) | 55014 (8.8 %) | 18949 (3.0 %) | 8013 (1.3 %) | 271328 / 0 / 0 / 0 |
| 8dB ACK | 227490 (83.8 %) | 43199 (15.9 %) | 637 (0.2 %) | 0 (0.0 %) | 1 (0.0 %) |  |
**dpx2-s1**

| | received | lost free | lost receiving | lost deaf | other channel | DATA answered: taken / refused / late / none |
|---|---|---|---|---|---|---|
| main DATA | 1136091 (37.0 %) | 1192704 (38.8 %) | 506188 (16.5 %) | 210421 (6.8 %) | 28013 (0.9 %) | 1113935 / 22063 / 0 / 0 |
| main ACK | 873418 (76.9 %) | 254786 (22.4 %) | 7782 (0.7 %) | 0 (0.0 %) | 9 (0.0 %) |  |
| 5dB DATA | 980108 (35.9 %) | 1120077 (41.0 %) | 430612 (15.8 %) | 176696 (6.5 %) | 23510 (0.9 %) | 966619 / 13415 / 0 / 0 |
| 5dB ACK | 769514 (78.5 %) | 204965 (20.9 %) | 5549 (0.6 %) | 0 (0.0 %) | 3 (0.0 %) |  |
| 7dB DATA | 1055601 (36.4 %) | 1159258 (40.0 %) | 464473 (16.0 %) | 192893 (6.7 %) | 25825 (0.9 %) | 1039658 / 15875 / 1 / 0 |
| 7dB ACK | 822884 (78.0 %) | 226252 (21.4 %) | 6384 (0.6 %) | 0 (0.0 %) | 9 (0.0 %) |  |
| 8dB DATA | 1076764 (36.6 %) | 1166344 (39.7 %) | 473401 (16.1 %) | 195534 (6.7 %) | 28268 (1.0 %) | 1060544 / 16154 / 2 / 0 |
| 8dB ACK | 837742 (77.8 %) | 232329 (21.6 %) | 6612 (0.6 %) | 0 (0.0 %) | 13 (0.0 %) |  |
| 10dB | - |

**The powers used**, DATA frames after the settle time by their steps
under full power, +10 dBm, step 19 the amplifier's floor, −9 dBm:

- s4x3-s1, 0 dB: 0:18.4% 1:9.1% 2:7.8% 3:6.8% 4:6.0% 5:5.3% 6:4.8% 7:4.2% 8:3.8% 9:3.4% 10:3.1% 11:2.8% 12:2.6% 13:2.3% 14:2.1% 15:1.9% 16:1.7% 17:1.5% 18:1.4% 19:10.9%
- s4x3-s1, 3 dB: 0:44.3% 1:6.0% 2:5.3% 3:4.7% 4:4.2% 5:3.7% 6:3.4% 7:3.0% 8:2.7% 9:2.4% 10:2.2% 11:2.0% 12:1.8% 13:1.6% 14:1.4% 15:1.3% 16:1.1% 17:1.0% 18:0.9% 19:7.0%
- s4x3-s1, 5 dB: 0:55.9% 1:4.6% 2:4.1% 3:3.7% 4:3.3% 5:3.0% 6:2.7% 7:2.4% 8:2.2% 9:2.0% 10:1.8% 11:1.6% 12:1.4% 13:1.3% 14:1.1% 15:1.0% 16:0.9% 17:0.8% 18:0.7% 19:5.3%
- s4x3-s1, 7 dB: 0:65.3% 1:3.7% 2:3.3% 3:3.0% 4:2.7% 5:2.4% 6:2.2% 7:1.9% 8:1.7% 9:1.5% 10:1.4% 11:1.2% 12:1.1% 13:1.0% 14:0.9% 15:0.8% 16:0.7% 17:0.6% 18:0.5% 19:4.1%
- s4x3-s1, 8 dB: 0:69.2% 1:3.2% 2:2.9% 3:2.7% 4:2.4% 5:2.1% 6:1.9% 7:1.7% 8:1.5% 9:1.3% 10:1.2% 11:1.1% 12:1.0% 13:0.9% 14:0.8% 15:0.7% 16:0.6% 17:0.6% 18:0.5% 19:3.6%
- s4x3-s1, 10 dB: 0:75.5% 1:2.6% 2:2.4% 3:2.1% 4:1.9% 5:1.7% 6:1.5% 7:1.4% 8:1.2% 9:1.1% 10:1.0% 11:0.9% 12:0.8% 13:0.7% 14:0.6% 15:0.5% 16:0.5% 17:0.4% 18:0.4% 19:2.8%
- movers, 0 dB: 0:27.9% 1:16.2% 2:11.5% 3:8.4% 4:6.5% 5:5.0% 6:4.0% 7:3.2% 8:2.6% 9:2.1% 10:1.8% 11:1.5% 12:1.2% 13:1.1% 14:0.9% 15:0.8% 16:0.7% 17:0.6% 18:0.5% 19:3.5%
- movers, 3 dB: 0:66.2% 1:5.9% 2:4.6% 3:3.7% 4:2.9% 5:2.4% 6:2.0% 7:1.6% 8:1.4% 9:1.2% 10:1.0% 11:0.9% 12:0.8% 13:0.7% 14:0.6% 15:0.5% 16:0.5% 17:0.4% 18:0.4% 19:2.4%
- movers, 5 dB: 0:77.1% 1:3.5% 2:2.9% 3:2.3% 4:1.9% 5:1.6% 6:1.3% 7:1.2% 8:1.0% 9:0.9% 10:0.8% 11:0.7% 12:0.6% 13:0.5% 14:0.4% 15:0.4% 16:0.3% 17:0.3% 18:0.3% 19:2.0%
- movers, 7 dB: 0:84.5% 1:2.3% 2:1.8% 3:1.5% 4:1.3% 5:1.1% 6:0.9% 7:0.8% 8:0.7% 9:0.6% 10:0.5% 11:0.5% 12:0.4% 13:0.4% 14:0.3% 15:0.3% 16:0.2% 17:0.2% 18:0.2% 19:1.4%
- movers, 8 dB: 0:86.4% 1:1.9% 2:1.6% 3:1.3% 4:1.1% 5:0.9% 6:0.8% 7:0.7% 8:0.6% 9:0.5% 10:0.5% 11:0.4% 12:0.4% 13:0.3% 14:0.3% 15:0.3% 16:0.2% 17:0.2% 18:0.2% 19:1.3%
- dp1, 3 dB: 0:64.3% 1:6.8% 2:5.2% 3:4.2% 4:3.2% 5:2.6% 6:2.1% 7:1.7% 8:1.4% 9:1.2% 10:1.0% 11:0.8% 12:0.7% 13:0.6% 14:0.5% 15:0.5% 16:0.4% 17:0.4% 18:0.3% 19:2.2%
- dp1, 5 dB: 0:77.0% 1:4.0% 2:3.1% 3:2.5% 4:2.0% 5:1.7% 6:1.4% 7:1.1% 8:1.0% 9:0.8% 10:0.7% 11:0.6% 12:0.5% 13:0.5% 14:0.4% 15:0.3% 16:0.3% 17:0.3% 18:0.2% 19:1.7%
- dp1, 7 dB: 0:84.8% 1:2.3% 2:1.9% 3:1.6% 4:1.3% 5:1.1% 6:0.9% 7:0.7% 8:0.7% 9:0.6% 10:0.5% 11:0.4% 12:0.4% 13:0.3% 14:0.3% 15:0.2% 16:0.2% 17:0.2% 18:0.2% 19:1.3%
- dp1, 8 dB: 0:87.3% 1:1.9% 2:1.6% 3:1.3% 4:1.0% 5:0.9% 6:0.8% 7:0.7% 8:0.5% 9:0.5% 10:0.4% 11:0.4% 12:0.3% 13:0.3% 14:0.2% 15:0.2% 16:0.2% 17:0.2% 18:0.2% 19:1.2%
- dpx2-s1, 5 dB: 0:74.0% 1:4.4% 2:3.6% 3:2.9% 4:2.4% 5:2.0% 6:1.7% 7:1.4% 8:1.1% 9:0.9% 10:0.8% 11:0.7% 12:0.6% 13:0.5% 14:0.4% 15:0.4% 16:0.3% 17:0.3% 18:0.2% 19:1.3%
- dpx2-s1, 7 dB: 0:82.4% 1:2.9% 2:2.4% 3:2.0% 4:1.6% 5:1.4% 6:1.1% 7:0.9% 8:0.8% 9:0.7% 10:0.6% 11:0.5% 12:0.4% 13:0.3% 14:0.3% 15:0.2% 16:0.2% 17:0.2% 18:0.2% 19:1.0%
- dpx2-s1, 8 dB: 0:85.3% 1:2.4% 2:2.0% 3:1.6% 4:1.3% 5:1.1% 6:0.9% 7:0.8% 8:0.7% 9:0.6% 10:0.5% 11:0.4% 12:0.3% 13:0.3% 14:0.2% 15:0.2% 16:0.2% 17:0.2% 18:0.1% 19:0.9%

## What it revealed

**Each decibel of headroom buys back a share of the loss, and the loss
is gone only when the rule is nearly off.** On S4 at three times the
rate, seed 1, the rule at the margin loses 12 points on this base as it
did on the last (65.7 → 53.4 %); 3 dB gives back 9 of them (62.1), 5 dB
a little more (62.7), 8 dB all but 0.7 (65.0), with airtime, failed
hand-overs and the relays at their cap back at main's. The movers go the
same way: 96.4 → 94.9 at the margin, 95.6, 95.8 and 96.0 at 3, 5 and
8 dB, latency at the 90th percentile 51 → 89 → 66 → 62 → 62 s. At the
design rate S4 is within its seed spread at every headroom, 93.6 →
93.1, 93.1 and 93.4, with 1 to 2 % less airtime at 5 and 8 dB and 4 to
6 % fewer failed hand-overs: the one place a headroom shows a gain, and
a small one.

What the headroom does is in the fates and the powers. At the margin
the DATA lost at a receiver that was free when they started rise from
20 to 27 % of those sent in S4 at three times the rate and from 48 to
56 % on the movers; 3 dB brings them to 23 and 51 %, 5 dB to 21 and
50 %, 8 dB to 20 and 49.5 %, main's. The share of DATA at full power
rises with the headroom: 18, 44, 56 and 69 % in S4 at three times the
rate, 28, 66, 77 and 86 % on the movers, since most forwards go to the
neighbour nearest the destination, which is the farthest in reach with
little to give (`power-control-2026-11-02.md`). At 8 dB the rule turns
down one DATA in seven on the movers and one in three in S4, by 7 to
8 dB on average, and those are the hops to a near neighbour, which were
heard everywhere at full power and still are at 2 or 3 dBm.

**What is noise.** S4 at three times the rate moves about 2 points
between runs: 10 dB on seed 1, which turns down less than 8 dB, delivers
63.6 against 8 dB's 65.0 and main's 65.7; seed 2 at 3, 5 and 8 dB gives
55.7, 58.7 and 60.1 against main's 62.4. Every S4 run at every headroom
is under main, by 0.7 to 3.7 points at 5 dB and above, which is the
sign and the order, not a value. The ring's 3000 texts move 2 to 4
points (90.6, 96.5, 92.1 against 92.6 at 3, 5 and 7 dB) and the mesh's
100 texts one text; neither says anything. The dense city at the design
rate moves 0.1 point between seeds, and its latency and airtime moved
the same way on all three seeds at 7 dB: p50 +7 to +9 %, p90 +4 to
+16 %, airtime +2.2 to +2.5 %.

**The gain under overload is not only the storm's.** On the base with
D96 the city at twice the rate, seed 1, no longer storms (main 89.4 %,
1.10 million seconds of airtime against 1.12 million on the base before
D96), and the headroom still gains there: 92.1 % at 5 dB with 9 % less
airtime, 91.3 % at 7 dB with 4.5 % less, 90.9 % at 8 dB with 3 % less;
hand-overs held back by the bucket −19, −11 and −10 %, texts waiting at
their origin 7850 → 2935, 3219 and 3816, texts lost to their time to
live 1576 → 1074, 1266 and 1277, heartbeats received 68.1 → 70.8, 69.8
and 69.4 %. Each DATA turned down is one fewer frame at every node in
reach but its receiver, and in the loaded city what the frames cost is
each other: at 5 dB the ACKs lost at a free receiver fall 22.4 → 20.9 %,
the DATA lost while the receiver received another frame 16.5 → 15.8 %.
The gain falls with the headroom as the share turned down does, 74 →
82 → 85 % at full power.

## Battery

The simulator charges one transmit current, 32 mA, at every power
(`bark_proto::sniff::TX_CURRENT_UA`): the least the datasheet lists for
the SX1262 at 434 MHz, +14 dBm with the +14 dBm settings (Table 3-6; 42
at +17, 65 at +20, 107 at +22 dBm). The datasheet gives the SX1262
nothing below +14 dBm: its low-power figures (+10 dBm at 14.5 mA at
434 MHz) are the SX1261's, a different amplifier, and no bench note has
measured the board's current at any power. So the current columns of
the tables move only with airtime, and what a DATA at reduced power
saves is bounded by hand.

A sleepy node's radio current is its listening: in the tables the
transmitting part is 0.003 to 0.05 mA of 0.45 to 2.3 mA, one to two
hundredths, at 32 mA for every frame. Of that part, the beats and the
ACKs stay at full power: an anchorless node's beats, 249 ms with the
long preamble every 34.6 s, 26 s an hour, are nearly all of its airtime;
a node with an anchor beats 14.4 ms every 225 s, 0.23 s an hour, and
sends its own texts at 68 ms per attempt, under two an hour in the dense
city and under one in S4, so there the DATA can be half of a transmit
part that is a hundredth of the whole. If a DATA at −9 dBm drew nothing
at all, the saving would be under one hundredth of a sleepy node's radio
current, under 0.03 mA in the dense city and under 0.01 mA in S4:
nothing a battery would notice. The relays are on external power. If
the amplifier's current at reduced power is ever wanted, the bench
measures it (the console's power command, `bark-fw/scripts/measure.md`
step 5).

## Recommendation

Do not adopt. The first measurement's reading holds on the base with
D96: what a DATA at reduced power loses is the contest with later frames,
and headroom only trades that loss against the saving, decibel by
decibel, until at 7 to 8 dB there is little of either. What remains at
7 dB is a tax at normal load, 5 to 9 % on the median latency and 7 to
22 % on the 90th percentile, 1.5 points of delivery in S4 at three times
the rate, against 1.9 points in the city at twice the rate with 4.5 %
less airtime. The network is designed for its rate, and the
overloaded city is the one place the rule pays; a rule that loads the
normal case to ease the overloaded one is the wrong way round, and
roadmap D29 to D36 already put the overload where it belongs, at the
origin's admission.

Two things would change the reading, and both are the bench's: the
capture threshold at SF5, on which the whole mechanism stands and which
the simulator takes from a tone figure at SF7; and the fade, 4 dB
assumed, which sets both the margin and the headroom. If the measured
capture threshold is well under 5 dB, the headroom shrinks, more DATA
are turned down and the trade may close; if it is well over, the rule
as built cannot pay anywhere. The amplifier's
current at reduced power is not a reason either way: a sleepy node's
transmitting is a hundredth of its radio current and its beats stay at
full power.

If the user adopts it nonetheless, 7 dB is the value: the derivation's
own, within the noise of 8 dB on every run and under it on none, and
the spec's "Hand-over" step 3 and "Neighbour table" (`rssi_ema` from
heartbeats and ACKs only) need the rule written in; the branch above
holds the code and this note.

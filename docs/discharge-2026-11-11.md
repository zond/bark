# The T-Echo cell's first measured discharge, and the curve fitted to it

**Date:** 2026-11-11. **Status:** adopted as roadmap D210. Spec references
are to `protocol.md`.

## The run

| | |
|---|---|
| board | f4f5, a LILYGO T-Echo with its own 850 mAh cell |
| image | an open node image built with `BARK_DISCHARGE_BENCH=1` (roadmap D190): a relay at any battery level, its GPS tracking from boot as the steady load |
| current | 35.4 mA as the core's currents count it, a relay's 6.41 mA and the GPS's 29 mA; constant, one mode throughout |
| temperature | room temperature, indoors |
| samples | one every 600 s, 131 from the one where USB went (seq 49) to the last before the board stopped (seq 179) |
| length | 21.7 h, 767 mAh at 35.4 mA, 90 % of the cell's nominal charge |
| readings | 4153 mV at the start to 2606 mV when the board stopped; uncalibrated, as no scale was kept |
| data | `docs/data/discharge-f4f5-2026-11-11.csv`, every sample of the log; the console's printing beside it, `.txt` |

At a constant current the charge left falls evenly in time, so a sample
at time t of a run from t₀ to t₁ has 100 (t₁ − t) / (t₁ − t₀) % left; the
samples are 600 to 601 s apart, so this is 100 (1 − i / 130) % for the
i-th. The 767 mAh are the core's currents times the run's length; the
microcontroller and the display are not in those currents, so the cell
gave somewhat more, unmeasured. The core counts its drains in the same
currents (spec "Sleepy nodes", How long the battery lasts), so in its
units the cell is 767 mAh.

`bark-fw/scripts/discharge.py docs/data/discharge-f4f5-2026-11-11.csv`
reproduces every figure below.

## Reading the log

The console printed the log one line every 3 ms, one millisecond per
64 bytes of its longest line, a guess at how fast the host reads. The
host got the 181 lines within 0.6 s, cut and spliced, without the end
line; the script found no end, and spliced lines that still matched its
pattern gave a first fit of "30 h, 125 % of the cell". The samples were
recovered by reading the log 30 times and taking each sample by
majority. Since then the console prints each line only once the log's
buffer has room for the longest line, and each sample's line ends in the
CRC-16 of its text, which the script checks; it also says where sequence
numbers are missing and where samples lie further apart than their
interval (`bark-fw/README.md`, "Discharge log").

## The fit

The charge left against the reading has one bend that matters, near
full, and the knee at the end, where the reading falls from 3.20 V to
2.61 V in the last 40 minutes, 3 % of the charge. A least-squares
quadratic over the 127 samples at 3 % and above, u the volts above
3.300 V:

    percent = 5.11 + 60.04 u + 65.74 u²

reads every sample of the run within 1.02 % rms and 3.08 % at most, the
largest at the knee, where it gives 0 % for 3.20 V, 3 % in truth.
Straight lines through the measured voltage at every 5 %, 21 points, do
0.67 % and 2.80 %;
the converter's step is 1.76 mV, 0.1 to 0.3 % of the charge, so neither
is limited by the readings. The quadratic is one formula with three
numbers, and its inverse is closed-form.

Rounded to `5 + 60 u + 66 u²` it is within 1.02 % rms and 3.08 % at most,
and with the whole percent the core shows, 1.07 % and 3.08 %: the
rounding costs nothing. The core computes it in integers, in millionths
of a percent against millivolts, `5 000 000 + 60 000 u + 66 u²` with u in
mV (`bark_proto::node::cell_percent`), and its inverse with an integer
square root (`cell_mv`), both `const`, so the levels are computed at
build time. Below the parabola's lowest point, 2.85 V, it would rise
again; it is negative there and reads 0.

| left | measured | curve | Maxim AN4189 | the old mapping at the measured voltage |
|---:|---:|---:|---:|---:|
| 100 % | 4153 mV | 4128 mV | 4177 mV | 96 % |
| 75 % | 3980 | 3971 | 3975 | 66 % |
| 50 % | 3795 | 3788 | 3821 | 33 % |
| 30 % | 3601 | 3611 | 3775 | low |
| 20 % | 3512 | 3504 | 3741 | low |
| 10 % | 3380 | 3377 | 3687 | low |
| 5 % | 3296 | 3300 | 3675 | low |

"Measured" is the run's readings, a median of five, interpolated; "Maxim
AN4189" is the typical cell of Maxim Integrated's Application Note 4189,
Table 1, open-circuit voltages after an hour's rest, which the core's
levels came from; "the old mapping" is the core's straight lines through
that table's 20, 30 and 100 %. The cell's curve lies far below the
typical one from 50 % down: at the old low level, 3.741 V, this cell has
44 % left, so a relay turned sleepy with nearly half its charge unused,
and the percent it showed was low by up to 25 points, `20%` with 45 %
left.

## The levels

They keep their meaning (spec "Sleepy nodes"; roadmap D81: low at about
20 %, a relay again at about 30 %) and take their voltages from the
curve's inverse:

| | old | new |
|---|---:|---:|
| low, 20 % | 3741 mV | 3504 mV |
| high, 30 % | 3775 mV | 3611 mV |
| full, what a calibrated full cell reads | 4177 mV | 4128 mV |

The full voltage moves too. The calibration scales a board's readings so
that its first reading an hour after a full charge reads as full. The
curve is in f4f5's readings, so a full cell must read as the curve's own
100 %; at AN4189's 4177 mV every calibrated board would read 1.2 % high,
some 40 mV at the low level, 3.5 % of the charge. A relay calibrates at
its own load, lighter than the bench's, and so reads its full cell some
9 to 20 mV higher than the curve's 99 %, the charge an hour at its drain
leaves (below); the scale takes 0.1 to 0.4 % off, 4 to 14 mV at the low
level, so that about half of the lighter load's higher reading there is
scaled away.

A node at 3741 mV showed `LOW` and turned sleepy; it now shows 44 % and
relays.

## The load

The run's first hours, on the same board before the bench build, switched
the GPS on and off every few minutes (`.csv`, seq 2 to 43): the reading
rose 16 to 24 mV within a minute of the GPS stopping and fell as much
when it started. Its 29 mA is the L76K's datasheet figure, not measured
on the board, so this puts the cell's resistance, with whatever relaxes
within a minute, at about 0.7 Ω, against the 0.3 Ω assumed for D190, and
the bench's readings some 20 mV below a relay's at the same charge,
rather than 9. At the low level, where 1 % is 11.5 mV, that is under
2 %, inside the curve's own error.

After the charger the readings fell 54 mV in the first 40 minutes,
where the charge used, 3.1 %, explains some 19: the charger's lift, some
35 mV, was gone within 40 minutes. From 40 to 60 minutes the fall, 11 mV,
is the charge's alone. The calibration's hour of rest covers it (spec
"Sleepy nodes").

## The cold

Nothing here was measured cold; what follows is an estimate from
published figures.

- *The voltage at rest.* Each electrode's potential moves by about
  1 mV a kelvin, and the two nearly cancel in the cell (Royal Society of
  Chemistry, Physical Chemistry Chemical Physics, 2019,
  https://pubs.rsc.org/en/content/articlelanding/2019/cp/c8cp06638h, as
  reported): of the order of 0.1 to 1 mV a kelvin for the cell, either
  way with the charge. At −20 °C, 40 K below the run, that is a few
  millivolts to some tens, a few percent at most either way.
- *The resistance.* A lithium cell's internal resistance is about 1.4 to
  3 times its room value at 0 °C and 2 to 5 times at −20 °C
  (https://technick.net/guides/electronics/bpw/c09_03/, as reported);
  the charge-transfer part alone 2 to 4 times at −20 °C
  (https://www.mdpi.com/2075-1702/13/12/1114, as reported). At a relay's
  6.4 mA through 0.3 to 0.7 Ω the drop is 2 to 4.5 mV at room temperature
  and 4 to 22 mV at −20 °C: up to some 18 mV more, 1.6 % of the charge at
  the low level. At the bench's 35.4 mA it is 11 to 25 mV, and 22 to
  125 mV at −20 °C, tens of millivolts, up to some 9 % of the charge. A
  sleepy node's 0.6 mA moves nothing.
- *The capacity.* A 1000 mAh polymer cell of this kind gives at 0.2 C,
  200 mA, 80 % of its capacity at 0 °C, 60 % at −10 °C and 95 % at 55 °C
  (LP402764 datasheet,
  https://e2e.ti.com/cfs-file/__key/communityserver-discussions-components-files/196/DataSheet_2D00_LP402764_2D00_1000ZHL.pdf,
  as reported). That is five times the bench's rate relative to its
  cell and some 25 times a relay's; at a node's small current the loss
  is smaller, by how much is not known.

So a cold node mostly reads low, and calls its battery low before 20 %
is left: by a percent or two as a relay, which errs toward keeping it
alive. Where the cold takes charge away, the curve's percent are of more
charge than the cell can give; the time left does not depend on that, as
the node learns the percent it sees fall each hour (roadmap D169) and so
follows the cold by itself, over its average of 83.7 h. A correction
would need the cell's resistance by temperature, which is not measured;
the nRF52840's temperature sensor reads the chip, which the board's own
current warms, not the cell. The curve has no term for temperature, and
a run in the cold is an open item (spec "Open items", O24).

## What follows from the curve

- *The drain* (roadmap D169). Its percent are the curve's, so its cell is
  the run's 767 mAh: a relay's first drain is 0.836 % an hour, a sleepy
  node's 0.0772; a sample is 13 h, 1 % at a sleepy node's drain, 13.0 h,
  rounded up; the average is over 83.7 h, a relay's time from full to
  30 %. A full relay shows 3 days, 96 h rounded down, and a full sleepy
  node 54 days.
- *What a node keeps.* The stored record's form does not change, so its
  layout number does not. The scale a board calibrated against 4177 mV
  is 1.2 % high on the new curve; the board replaces it at its next full
  charge, 3 h on USB and an hour on battery, by the calibration's own
  rule. The drains are kept as steps from their first values, a ratio;
  the old mapping's percent were off by a factor of 0.4 to 1.6 along the
  curve, the errors cancelling over a whole discharge, so a kept ratio
  is of the right order and the moving average takes the rest, 63 % of
  the way in one 83.7 h, 95 % in three. They are not reset: only bench
  boards hold any, and a reset would only lose what they learned of
  their current.
- *The simulator* reads each cell off the same curve, its capacity the
  core's 767 mAh (`scenarios/grid-down.toml`); the results are in roadmap
  D210.

## Open

- A run in the cold, and one at a relay's current, against this curve
  (O24).
- More cells: this is one cell of one board.
- The microcontroller's and the display's current, which neither the
  run's 767 mAh nor the drains count.

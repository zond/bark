# The radio driven with only the commands that change

**Date:** 2026-10-30. **Status:** built, measured in the simulator and
run on board 1, adopted as roadmap D88; one regression found on the
board and fixed, its retest open. The bench note `bench-2026-10-30.md`
has the measurements this rests on.

## The question

A listen-before-talk cost 9.2 ms at the measured costs (spec "Heartbeat"
before D88), paid by every heartbeat, hand-over and answer, where the
protocol had assumed 2.8 ms. Four things in the driver's path made it so,
and the bench found each unnecessary:

| In the driver | On the bench |
|---|---|
| every setting sent again on every operation | modulation, packet parameters, the symbol timeout and the receiver gain do not change between operations: a retune is three commands, 355 µs, where five cost 461 |
| the bus driven through the interrupt path | driven blocking inside the asynchronous device, a command loses 35 µs, a third of it: a read 155 → 49 µs |
| a return to listening between the CAD and the reading | the reading is right four symbol periods into the CAD, as in receive |
| the oscillator restarted with a 2 ms wait after every CAD | restarted within a millisecond of stopping it needs no wait; the start itself is 200 to 500 µs at room temperature, the flag flickering below |

Could a driver that sends the chip only what changes bring the
listen-before-talk to the CAD's preparation and sampling, about 2.5 ms,
and the restart after it to about 100 µs, without losing the safety the
generic driver buys by repeating itself?

## What was built

**One excursion.** The core asks for `Action::Lbt(channel)` and gets
`Event::LbtDone` with the busy result and the reading; the channel is
clear if neither says busy. The heartbeat, the hand-over and the reply
lose a step each; the sweep's reading stays `Action::Rssi`
(`core-interface.md` §3 rule 5).

**A sequencer that tracks the chip** (`bark-driver/src/sx1262.rs`), in
the host workspace and tested there against a chip that records the
commands it is sent. It keeps what the chip holds, its mode, frequency,
packet parameters, oscillator delay and transmit power and whether it
holds the configuration, and sends an operation only what differs:

| Operation | Commands |
|---|---|
| the configuration, once | regulator, RF switch, packet type, sync word, buffer base, retention list, fallback to STDBY_XOSC, modulation with its errata write, the IQ errata write, one interrupt mask for receive, CAD and transmit, symbol timeout, receiver gain |
| retune | standby, frequency, receive |
| listen-before-talk | standby, frequency, the CAD's parameters, CAD; `GetRssiInst` four symbol periods in; the result read and cleared; then frequency and receive from the standby the CAD ends in |
| transmit | standby, frequency and packet parameters if they differ, the payload, transmit; then the receiver's packet parameters and receive |
| duty-cycled receive | standby RC, frequency and packet parameters if they differ, the probed delay, the cycle |

After a reset, a sleep (every duty-cycled receive sleeps) or an error
the whole configuration goes again, since the chip may not hold it; an
error resets the chip first. The firmware (`bark-fw/src/radio.rs`) is the
chip one command at a time, with the bus driven blocking and the
encoding of each command from the vendored `lora-phy`, which gained
one-command methods for it.

**The reading inside the CAD, guarded.** The datasheet documents
`GetRssiInst` for receive only. At boot the radio takes five pairs of
readings on its home channel, one inside a CAD and one while listening,
and keeps the median difference as the offset it takes off every reading
inside a CAD; if a pair has no reading inside, or the offset is beyond
12 dB, twice the largest the bench saw, every listen-before-talk reads
after the CAD instead, with the receiver started on the channel and
settled, 1.1 ms more. The choice is logged at boot and shown by the
console's `tcxo`.

**The oscillator's start, probed** (`bark-driver/src/tcxo.rs`). Two
delays: 100 µs for the restart after a CAD, twice the 50 µs that never
failed within a millisecond of stopping on the bench, since zero hangs
the chip; and a probed one for every cold start. The probed delay is
found at boot by a binary search over 0.25 to 4 ms in 0.25 ms steps, the
bench's resolution and wider than the band in which the flag flickers;
each probe is a standby with the oscillator off for 2 ms, the delay set,
the errors cleared, a receive and the error word read; four more probes
confirm the step found, since at the edge about half fail; one step of
margin goes on top. On the bench's board that lands at 0.5 ms. In use
the error flag is read after every cold start and the delay stepped up
at once when it is set, which covers a board getting cold; a probe two
steps below the delay, which lowers it by one if it passes, comes when
the processor's die temperature has moved 3 °C since the delay settled
and not within an hour of it, which recovers the margin as the board
warms. The die sensor is uncalibrated and only its change is used; no
bench number sets the 3 °C, which only paces a recovery the step-up
makes safe, and it is the one number here chosen rather than derived.

**The console** keeps its commands and gains `lbt <k>`, which prints the
phases and the reading, `tcxo`, which prints the delay in use and the
last probe, and `probe`, which forces one. The node's 10 s `driver` line
carries the delay, its errors and the cold starts.

## Measured on the board

Board 1, the branch merged with main, console image 6, medians at room
temperature (2026-10-30, after the simulator runs below):

| | |
|---|---|
| boot | radio up in 73.5 ms; the delay 500 µs after 8 probes, the lowest confirmed 250; the self-check's five pairs −1, 1, 1, 2, 3 dB, the offset 1 |
| retune, `ch` to another channel | 386 to 389 µs; to the same channel nothing is sent, 10 µs |
| a read | 51 µs; `rssi 5 1024`: retune 389, dwell 1071, read 51 |
| `lbt 5`, quiet | prepare 359, CAD 2023 (one in five 2279), the read inside 51, back 505 to 515: 2.90 to 2.92 ms home to home; −78 dBm outside the CAD, −77 inside |
| `tx 3 9` | prepare 547, 161 over the airtime, 369 back; 255 bytes: prepare 794, 165 over |
| `sweep`, no dwell | 5.81 ms |
| under a beacon on channel 5 at 10 % duty, −33 to −36 dBm | `lbt 5` 300 times: busy 37, each with the reading inside at −33 to −35 and the receive-mode reading at −33 to −36 |

**A regression, fixed.** The same beacon test with the CAD flag alone
(`cad 5`, 200 times) found it busy once, where the old driver, which
sent the CAD's parameters before every CAD, found the beacon 10 % of
the time, its duty. With the parameters sent once in the configuration
the CAD kept its length, 8 symbols, but not its detection thresholds,
so only a preamble, 3 ms of a 139 ms frame, set the flag. The
parameters now go before every CAD, one command, 51 µs; `cad <k>` runs
the CAD alone with nothing read inside it, so that the flag can be
counted against `lbt`. Retest: `cad 5` under the beacon, busy at about
the beacon's duty.

## The simulator's costs

The simulator's cost model carries the board's numbers as defaults,
each field saying which measurement it is:

| Field | µs | From |
|---|---|---|
| `rx_to_fs_us` + `fs_to_rx_us`, the retune | 388 | measured |
| `rssi_read_us` | 51 | measured |
| `lbt_prepare_us` | 430 | measured with the CAD's parameters sent before each CAD; 359 without them |
| the CAD's sampling | 2176 | derived, 8.5 symbols; measured 2.02 to 2.28 ms |
| `lbt_return_us` | 510 | measured |
| `rx_to_tx_us` | 765 | measured, 602 to prepare 64 bytes between the 547 of 9 and the 794 of 255, and 163 over the airtime |
| `stby_to_rx_us` | 369 | measured |

A listen-before-talk is then 3.1 ms from listening to listening where
it charged 9.2, and a reading of another channel 1.9 ms where it charged
2.5.

## Measured in the simulator

Main at `1bce3d6`, with the costs D86 set from the bench, against this
branch at the board's costs above: `s4-capacity.toml` at the design
rate and at three times it (`per_leaf_per_hour` 2.875),
`dense-power.toml` seed 1. Delivered of the texts sent in the settled
hour, the whole run's airtime, and the latency of every delivered text.

| Run | | delivered | airtime, s | p50 | p90 |
|---|---|---|---|---|---|
| S4 ×1, seed 1 | main | 93.5 % | 51 677 | 1.79 s | 195 s |
| | D88 | 93.5 % | 51 190 | 1.63 s | 192 s |
| S4 ×3, seed 1 | main | 44.5 % | 188 418 | 211 s | 783 s |
| | D88 | 56.2 % | 172 688 | 151 s | 678 s |
| S4 ×3, seed 2 | main | 45.3 % | 187 107 | 209 s | 771 s |
| | D88 | 56.8 % | 171 690 | 154 s | 668 s |
| S4 ×3, seed 3 | main | 43.7 % | 211 415 | 270 s | 850 s |
| | D88 | 58.9 % | 188 141 | 173 s | 690 s |
| dense-power, seed 1 | main | 81.9 % | 1 932 955 | 60.6 s | 412 s |
| | D88 | 89.5 % | 1 368 119 | 33.8 s | 330 s |

At the design rate nothing changes: the network is far from its limit.
At three times the rate the lean listen-before-talk gives back 12 points
of the 18 the measured costs took (D86 measured 62 % before them), with
less airtime, since fewer hand-overs are retried, and shorter waits.
`dense-power` seed 1, which storms at the measured costs, still storms
with four and a half times the airtime of a run without one (295 000 s,
D86), but at a third less than before; its cause is a later item. The
same runs at the note's first defaults, from the bench's experiment
before the board ran this driver (the retune 355, the read 49,
`lbt_prepare_us` 464, `lbt_return_us` 350, `rx_to_tx_us` 1356,
`stby_to_rx_us` 840), came out the same within the seeds: S4 at three
times the rate 58.0 / 57.8 / 57.1 %, `dense-power` 90.2 % with
1 190 000 s of airtime.

## What the bench still has to confirm

- Confirmed 2026-10-30 evening: with the parameters before each CAD,
  the CAD flag alone is busy 40 and 38 of 400 under the beacon, its
  duty; `lbt`'s `prepare_us` is 430 µs and the whole 2.98 ms.
- The probed delay at boot on each board, the error count in the
  `driver` line after a day, and `probe` after a night outside: whether
  a cold board's search lands higher and the hourly probe brings it
  back down.
- That the chip accepts the configuration sent once: a transmit, a
  listen-before-talk and a duty-cycled receive in turn, with frames
  received after each. The sequence after a sleep sends everything
  again, which the host tests check and the board must bear out.
- A transmit from the standby a CAD ends in, which the driver loop does
  not yet do (it returns to listening before it delivers the result):
  what it would save is the receive and the standby between, under half
  a millisecond, so it waits for the bench to show the rest.

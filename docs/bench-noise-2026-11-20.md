# Noise on the bench: USB, battery and a solar supply

**Date:** 2026-11-20. **Status:** measurements, three boards, no decision.
The noise a board reads depends on how it is powered and where it sits,
by more than 10 dB. Every bench figure before this date was taken on one
desk, over USB, with the port being read.

## Why

On 2026-11-19 two boards' noise floors fell about 6 dB within minutes of
the host no longer reading their logs, and the readings returned when
the host read again (roadmap D233). This asks whether reading a board
over USB raises the noise it hears, and what a solar supply does.

## Method

Three boards ran the survey build (`bark-fw/README.md`, Survey build).
Each reads all 13 channels once a minute, 100 readings a channel, and
keeps the median in RAM for every minute, read or not. Each also
beacons on its own channel, 0, 6 and 12, so the figures below are the
median over channels 2, 3, 4, 8, 9 and 10, and over the minutes of a
condition. A minute that falls under a neighbour's beacon reads about
−77 dBm on several channels; the median over minutes leaves those out.
The two minutes after each change are left out.

| Board | Role |
|---|---|
| f4f5 | on USB; the host reads the port for 30 minutes, typing a status command every second, then closes it for 30 minutes; 12 times over, 21:30 to 09:30 |
| 3112 | on USB, port never opened until the readout at 09:33 |
| c367 | on its own cell from 21:50, and through the steps below the next morning |
| bac7 | in its bootloader, silent |

All four sat on the desk beside the laptop, a hand's width or two apart.

## Reading the port

| Board | Condition | Noise, dBm |
|---|---|---|
| c367 | on its cell | −98 |
| 3112 | on USB, port never opened | −95 |
| f4f5 | on USB, port closed | −89 |
| f4f5 | on USB, port read | −88 |

Each figure is the mean of 12 half-hour medians, which did not vary by
more than 2 dB through the night. Reading the port costs 1 dB. f4f5 was
6 dB above 3112 all night, read or not; the two differ in place on the
desk and in f4f5's port having been opened, and this does not say which
matters. 3112 held −95 dBm from 21:30 to 09:15, so the room did not
change between night and morning.

## The solar supply

c367 the next morning, the other two boards untouched beside it. The
supply is a panel with its own cell and a 5 V output, indoors in shade.

| Time | c367 | c367, dBm | 3112, dBm | f4f5, dBm |
|---|---|---|---|---|
| 08:50–09:19 | on its cell, on the desk | −90 | −94 | −91 |
| 09:19–09:41 | on the supply, on the desk; the supply 1 dm from 3112, its cable beside f4f5's antenna | −87.5 | −90.5 | −86 |
| 09:41–09:57 | on the supply, 5 dm from it and from the desk's boards; the supply 2 to 3 dm from them | −100 | −93 | −86 |
| 09:57–10:20 | on its cell, same place; the supply idle, same place | −100 | −94.5 | −88 |
| 10:20 on | on the laptop's USB again | −87 | −94 | −88 |

- **Power from the supply adds no noise to the board it powers:** −100 dBm
  on the supply and on the cell, in the same place.
- **The supply radiates while it delivers power, at close range:** 3112
  rose 4.5 dB with it 1 dm away, 2 dB at 2 to 3 dm, nothing with it
  idle; c367, 5 dm from it, shows nothing. f4f5 rose 5 dB and did not
  fall back with distance, so its figures do not fit one cause.
- **The desk is the noisy place:** 5 dm from it c367 read −100 dBm; on
  it, −98 at night and −90 the next morning on its cell, and −87 on the
  laptop's USB.

## What follows

- A node's antenna belongs half a metre or more from its solar supply.
- Bench figures taken on the desk over USB were read against a noise
  8 to 13 dB above what a board on its cell or a solar supply hears
  half a metre away. Ranges and margins worked out from them are too
  small by that much at most.
- The busy rule no longer depends on which of these holds: a channel's
  floor follows the readings within a noise half-life either way
  (spec "Busy estimate", roadmap D233).

## Not explained

- c367 on its cell in one place on the desk: −98 dBm at night, −90 the
  next morning. The laptop was idle at night and in use in the morning.
- f4f5 against 3112, 6 dB all night.
- c367, plugged in after the night on its cell, left USB twice when its
  port was opened within half a minute, and the host could not reach it
  until it was plugged in again. Opened 20 minutes after a third
  plugging in, it answered, with its uptime and every minute of the
  night kept. Its cell read 4.03 V.

## Data

The minutes of each board, the times of each step and the two scripts
are in `bark-fw/target/bench/runs/noise-20261120/` on the bench host,
not in the repository.

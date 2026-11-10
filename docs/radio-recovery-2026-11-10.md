# A radio that reads every channel busy

2026-11-10. Board bac7, one of the four T-Echo boards on the bench, ran
image 23 from 09:22 for 5.8 hours. By 14:25 it skipped nearly every
heartbeat because every channel read busy to its listen-before-talk, and
it received fewer and fewer whole frames. Moving it away from the
computer and the other radios changed nothing. A reflash at 15:10 cured
it at once. This note works out what the logs say happened, what the
chip's error means, what the driver did and did not restore after a
reset, and what changed (roadmap D192). The logs are the USB captures of
the four boards; the 10 s line and its counters are described in
`crc-failures-2026-11-10.md`, "What the counters count", which also
explains the 40 to 60 % failed share every board shows on this bench.

## The long boot

The board was on USB until 09:24:35 and again from 14:25:31, when it
dumped the record log it keeps on the board. That log had dropped 21 849
records, so uptime 240 s to 18 199 s (09:26 to 14:25) is missing.

| Uptime | Time | What |
|---:|---|---|
| 0.06 s | 09:22 | oscillator delay 500 µs after 8 probes; reading inside a CAD less listening +1 dB |
| 3.2 s | | a heartbeat skipped on channel 2, then `radio: Chip(OpError(170))` |
| 12 s | | `radio_errors=1 resets=1 configured=2 cold_starts=4`, delay 500 µs |
| 0–172 s | | 6 frames, 6 failed, 1 heartbeat skipped |
| 240–18 199 s | | not logged; the second error, reset and configuration and the fifth cold start happened here |
| 18 202 s | 14:25 | `frames=983 failed=1053 radio_errors=2 resets=2 configured=3 cold_starts=5`, delay 500 µs |
| 18 305 s | 14:27 | starts gathering image 25 from a neighbour that streams it; blocks 0 and 1 by 18 832 s |
| 19 269 s | 14:43 | `tcxo: start error at 500 us, now 750 us`; `cold_starts=6 tcxo_errors=1` |
| 20 887 s | 15:10 | reflashed |

Per ten minutes after the dump (the first bin starts at 18 199 s):

| Uptime | Frames | Failed | Failed share | Heartbeats skipped |
|---|---:|---:|---:|---:|
| 18 199–18 600 | 40 | 40 | 50 % | 34 |
| 18 600–19 200 | 116 | 204 | 64 % | 47 |
| 19 200–19 800 | 26 | 256 | 91 % | 46 |
| 19 800–20 400 | 28 | 122 | 81 % | 47 |
| 20 400–20 882 | 22 | 231 | 91 % | 36 |

The 211 skipped heartbeats fell on all 13 channels alike, 15 to 20 on
each. No more blocks of the image arrived after 18 832 s, while 3112, a
few metres away, took the same stream normally. No error, reset or
reconfiguration came in these 45 minutes; the only radio event was the
start error.

After the reflash, image 25 booted at 15:10:47: in its first 373 s, 23
frames, 23 failed (16 header, 7 CRC, 0 timeout), one heartbeat skipped,
no reset, no cold start, delay 500 µs.

So the state was entered while running and a boot left it. Every reset
the board had was over before the dump, and none of them came with it;
the logs cannot say when in the missing five hours the busy readings
began.

## The other boards

bac7 is not the only board with these errors:

- `radio: Chip(OpError(170))` also on 3112 (image 19, uptime 5 853 s, a
  second after a start error 1 750 → 2 000 µs) and on c367 (image 19,
  6 804 s).
- Start errors on every board, and on 3112 they did not stop: in one
  boot of image 19 the delay went from 500 to 3 250 µs in eleven steps
  over 4.3 hours. A step of 250 µs ends errors that the delay causes, so
  repeated steps say the errors came from elsewhere (below).
- `radio: NotDone(4)`, `NotDone(20)` and `NotDone(36)` on c367, f4f5
  and 3112: a CAD or a transmission whose interrupt came without its
  done flag, which the driver counts as an error and answers with a
  reset. 4 is a preamble detected, 20 a preamble and a valid header, 36
  a preamble and a header error: the receiver's flags.
- Long stretches of skipped heartbeats on every board in some boots
  (up to 170 an hour on 3112, 100 on c367), some after resets and some
  with none.

The live logs at 15:39 show the same on today's images. c367 (version
25, booted 14:27, streaming update pieces) had four errors and 3112
(version 24, booted 13:20) six:

| Board | Uptime | Line | One to two ms before it |
|---|---:|---|---|
| c367 | 704 s | `NotDone(20)` | `update: serving` a piece |
| c367 | 887 s | `NotDone(4)` | `update: serving` a piece |
| c367 | 2 198 s | `NotDone(4)` | `update: serving` a piece |
| c367 | 2 694 s | `NotDone(4)` | |
| 3112 | 5 185 s | `Chip(OpError(170))` | `HeartbeatSkipped` 47 ms before |
| 3112 | 5 756–5 773 s | `NotDone(22)` twice, `NotDone(20)` three times | `update: serving` each of five pieces in a row, 4.3 s apart |

Every `NotDone` came at the transmission of a served piece, which
leaves straight from receive: a frame arriving at that moment (22 is a
preamble, a valid header and a packet received) held DIO1 high, the
transmission was taken for failed, and the reset that followed cut the
piece off on the air. Every `OpError(170)` on any board, four in all
(bac7 at 3.2 s, 3112 at 5 185 s and in image 19 at 5 853 s, c367 in
image 19 at 6 804 s), came within a second after a skipped heartbeat,
that is at the return to receive after a busy check, the short restart.
Over 3 800 s today the cold delay of 3112 stepped from 500 to 2 250 µs.
Reception and skipped heartbeats in the ten minutes after each error
were as in the ten before (c367: 19 to 36 frames and no skip either
side; 3112: 134 to 150 frames, 2 to 4 skips). No change merged today
touches the radio sequencer, and image 19 showed the same errors
yesterday. So these errors are provoked by the driver on every board,
not by bac7's hardware; what is particular to bac7 is the busy state,
which none of these errors brought on any other board.

## The error decoded

`OpError(170)` is the status byte 0xAA that the chip returned with a
`GetIrqStatus`, `GetRssiInst`, `GetRxBufferStatus` or `GetPacketStatus`
(the vendored driver checks the status of those four). By the
datasheet's status byte (SX1261/2 DS §13.5.1, `GetStatus`):

| Bits | Value | Meaning |
|---|---|---|
| 7 | 1 | reserved |
| 6:4 | 0b010 | chip mode STBY_RC |
| 3:1 | 0b101 | command status 0x5, failure to execute command: the command was taken, but the chip could not enter the mode it asked for |
| 0 | 0 | reserved |

So the chip was in STDBY_RC, with its oscillator off, when the driver
believed it was receiving: the last mode change had failed. On every
board it came after a heartbeat's last check, at the first
`GetIrqStatus` after the return to receive, which is the restart within
a millisecond of a CAD that the driver programs with the short 100 µs
delay (`tcxo.rs`, `SHORT`). A start fails like this when the oscillator
is not running at the end of its delay (`GetDeviceErrors` bit 5,
`XOSC_START_ERR`) or the PLL does not lock on it (bit 6, `PLL_LOCK_ERR`,
DS Table 13-84). The driver did not read the device errors before the
reset, which clears them, so which of the two it was is not known; it
reads and logs them now.

The recovery does clear the device errors: the reset is followed by
`ClearDeviceErrors`, and after the calibration they are read and cleared
again. A reset is the full one the datasheet offers short of a power
cycle (NRESET, DS §9.1): it returns the chip to its power-up state.

## What a reset restored

What the boot sends and measures, against what the reset path did:

| | Boot | After a reset, before | Now |
|---|---|---|---|
| NRESET, device errors cleared | yes | yes | yes |
| TCXO voltage, delay for the calibrations | 4 000 µs, the longest | the delay probed for the oscillator's start, 500 µs on bac7 | 4 000 µs |
| full calibration, its errors read | yes | yes | yes |
| regulator, RF switch, packet type, sync word, buffer base, retention list, fallback mode, modulation with its errata write, IQ errata write, interrupt mask, symbol timeout, receiver gain | sent | sent (the same `configure`) | sent |
| image calibration for 430–440 MHz | yes, at 4 000 µs | yes, at the probed delay | yes, at 4 000 µs |
| frequency, packet parameters, transmit power | sent when first needed | the same | the same |
| CAD parameters | before every CAD | the same | the same |
| oscillator delay | searched | kept, with every step up since | searched again |
| offset of the reading inside a CAD | measured | kept | measured again |
| core's noise floor and busy threshold | from the first readings | not the radio's: kept | kept |

The defect is the second and sixth rows, with the two measurements. Every
boot calibrates the chip 4 ms after powering the TCXO; bac7's recovery at
3 s calibrated it 500 µs after, one step over the shortest wait at
which its oscillator started without an error. A start says nothing of
its frequency having settled, and the PLL and image calibrations run on
that frequency. The offset and the delay were kept from a chip that had
since been reset.

A raised oscillator delay changes only cold starts: the receive, the CAD
and the transmission start from STDBY_XOSC, or within a millisecond of
STDBY_RC, at every other time, and bac7 made six cold starts in 5.8
hours. So the step at 19 269 s cannot have made the listen-before-talk
or the sweep read late. A chip that failed to enter its mode says so in
its status, as above, and the driver resets it, so a reading in a wrong
mode is an error rather than a busy channel. What does read busy without an error is a reading
inside the CAD at or below −127 dBm, the receiver not up yet: the
listen-before-talk then reports the top of the scale, so as not to send
on a check it never got.

Three faults of the driver turned up on the way, on every board:

1. **Receive flags ended a CAD or a transmission.** A preamble the
   receiver flagged just before the driver left it held DIO1 high, so
   the wait for the CAD's or the transmission's interrupt ended at once,
   without a done flag: the `NotDone` errors, each a reset, and for a
   served piece the piece cut off.
2. **The short restart fails, and its errors were blamed on the cold
   delay.** The restart 100 µs after a CAD is where every
   `OpError(170)` came. The error flag stays set until read, and the
   driver read it only after a cold start; a flag a short restart had
   left was taken for the cold start's own and stepped the cold delay
   up. On 3112 that walked the delay to 3 250 µs.
3. **No recovery from a radio that answers but reads wrong.** The driver
   reset the chip only on an error.

## What is the board's and what is ours

The board's: like every board's, its oscillator does not always restart
within 100 µs of stopping. The bench of 30 October saw no failure in 90
restarts at 50 µs; in use, with thousands of restarts a day, all four
boards fail one now and then. That the driver relied on it is ours.

Ours: the state after recovery was not the state after boot, the three
faults above, and nothing that noticed a radio reading every channel
busy. Whether the calibration at 500 µs is what put bac7 into its state
cannot be shown from these logs: the hours in which it began are
missing. The candidates, and what tells them apart:

| | Cause | Cured by a radio reset | Shows in the new log line as |
|---|---|---|---|
| A | the chip, calibrated 500 µs after its oscillator was powered | yes, now that the reset calibrates at 4 ms | preamble seen at few checks, readings above the floor; the busy readings stop after the line |
| B | the reading inside a CAD drifted from the offset measured at boot | yes: the offset is measured again | the same, and the offset in the `radio: reading inside a CAD less listening` line after the reset differs from the boot's |
| C | no reading inside the CAD, the receiver not up | yes, if a reset cures the chip | readings up to 0 dBm, the scale's top |
| D | the CAD sees preambles that are not there | yes, if a reset cures the chip | a preamble seen at most checks |
| E | the core's noise floor estimate, which no radio reset touches | no; a reboot does | the line repeats every few minutes, the skipped heartbeats go on |
| F | the board's oscillator or front end failing | no, nor a reboot for long | as E, and the boot's own measurements look wrong |

## What changed (roadmap D192)

- A reset calibrates at the longest oscillator delay, as the boot does,
  and the boot's measurements, the delay search and the check of the
  reading inside a CAD, run again before the radio next listens; their
  usual log lines follow. Before the reset the device errors are read
  and logged: `radio: device errors 0x.. before the reset`.
- The error word is read before every cold start. A flag found there
  came from a short restart: `tcxo: a restart within 1000 us of stopping
  failed at 100 us; every start now takes N us`, and from then on every
  start takes the probed delay. The same follows a reset whose error
  word, read before it, shows the oscillator or the PLL failed after a
  short restart, the `OpError(170)` case. The cold delay steps up only
  for its own errors.
- After a receive, the receiver's flags are cleared before a CAD or a
  transmission.
- A heartbeat skipped on every channel with no check found clear in
  between resets the radio as after an error (spec "Heartbeat" gives
  the threshold), with the line
  `radio: a heartbeat skipped on every channel, N checks busy in a row,
  M with a preamble seen, readings L to H dBm; resetting the radio`.
  The 10 s line counts these in `recoveries`.
- The console's `recover` does the same reset by hand.

## What the bench should do with bac7

1. Run it on an image with D192 and keep its USB log, the others beside
   it as before. Each `resetting the radio` line names the kind of busy
   (table above, last column), and the lines after it say whether the
   reset cured it: the boot's two measurement lines with the new delay
   and offset, then the skipped heartbeats per ten minutes.
2. If the lines repeat every few minutes and the heartbeats stay
   skipped, the cause is E or F. A reboot (the console's `reboot`, not a
   reflash) then tells them apart: cured by it is E, not cured is F.
3. With a console image, once it is in the state: `lbt k` on a few
   channels (the CAD flag, the reading, the reading as read inside),
   `noise k 100` on the same channels for the floor while listening,
   `tcxo` and `status`; then `recover` and the same again. A reading
   inside the CAD that sits above the floor before `recover` and not
   after is A or B.
4. If start errors and `OpError(170)` keep coming on bac7 and not on the
   others at the same rate, its oscillator is the weaker one: the
   `device errors` line before each reset says whether the oscillator
   did not start (0x20) or the PLL did not lock (0x40).

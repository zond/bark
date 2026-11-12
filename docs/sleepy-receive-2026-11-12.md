# A sleepy node's radio after a reception, 2026-11-12

A T-Echo built sleepy on USB (`BARK_ON_BATTERY=1`) logged `WARN radio: Chip(Busy)` after every
frame it received in duty-cycled receive, then reset its radio. This note gives what the bench
showed, the cause, the fix (roadmap D211) and what is left to measure. "DS" is the SX1261/2
datasheet, Rev. 1.2.

## What the bench showed

Two boards on a quiet desk: the sleepy board S, and a console board K sending a 36-byte
`beacon` on S's home channel every 5 s, first 206 with the short preamble, then 103 with the
long one. S caught 3 of the short and 102 of the long, as designed. Its 10 s line at the end:
`frames=106 radio_errors=106 resets=110 configured=282 tcxo_errors=178 sniffs=179`. The log
has a `radio: Chip(Busy)` line after every caught frame, and no `radio: device errors ...
before the reset` line at all.

With a relay three metres away as its anchor, streaming an update S was gathering, S received
2 frames in 510 s, reported `AnchorWatch { heard: false, ... }` and gathered no block. The
evening before, a sleepy board on battery answered about one status ask in five and had none
of its own six answered.

A long frame, on S's clock (K's clock is 40.56 s ahead, within about 10 ms, from the shortest
USB delay of each board's lines):

| S's clock, s | what |
|---|---|
| 1615.49 | K starts a beacon, 261.7 ms with the long preamble |
| 1615.716 | a step ends, the header seen, `frames=5` |
| ≈1615.75 | the frame ends; S reads it and the core takes it |
| 1615.789 | `tcxo: a cold start after None us off failed at 750 us, cold starts now take 1000 us` |
| 1615.806 | `radio: Chip(Busy)`: a command waited out the 15 ms bound on BUSY |
| 1616.868 | `tcxo: cold delay 750 us after 63 probes`: after the loop's 1 s pause, the reset and the boot's measurements |
| 1616.919 | the reading check done, the radio sniffing again |

So after each frame S could neither receive nor transmit for about 1.17 s. Every beacon after
it followed the same course.

Before any long frame, on the same board: every sniff that the core ended showed the
oscillator's error flag, `tcxo_errors` kept pace with `sniffs`, and each one stepped the cold
delay up: 750, 1000, ... up to 4000 µs, where the line read `failed at 4000 us, cold starts now
take 4000 us`. Every reset brought it back to the 750 µs the boot's search finds on this board.
Four of about forty such ends were also followed by `Chip(Busy)`.

## The cause

The driver took the chip's state after a duty-cycled receive from the datasheet's line that
a packet ends the cycle "and returns to STBY_RC" (DS §13.1.7). After a packet it therefore sent
no SetStandby, read the error word, sent the whole configuration and then
`SetDIO3AsTCXOCtrl`, which is valid in STDBY_RC only (DS §13.3.6), and `SetRxDutyCycle`. The
chip was not in STDBY_RC: BUSY rose and stayed high, through the 1 s pause, since the read of
the error word before the reset failed every time, until the reset. Where the chip was the logs
cannot say: in STDBY_XOSC, the fallback the configuration sets for receive and transmit, or
going on with its cycle. A sniff the core ended was handled better, with a wake and a
SetStandby, but that SetStandby was sent without waiting for BUSY low, which the datasheet
requires before every command (DS §8.3.1, §10), and was now and then lost, hence the four.

The oscillator's flag was a second fault. A wake from sleep with a TCXO raises
`XOSC_START_ERR` whatever the delay (DS §13.3.6: "an expected behaviour ... simply clear this
flag"); each window of the cycle is such a wake. The driver read the flag after every sniff as
a cold start that failed, stepped the cold delay up, and logged it with `None` for the time
off, which it did not keep. The bench proves the flag says nothing of the delay: it stayed at
4000 µs, eight times what the board needs, while the cycle caught 102 of 103 long frames.

## The fix

- However a sniff ends, the driver takes the chip out of the cycle the same way, before any
  other command: a wake (a falling NSS edge, sent without waiting for BUSY, which a sleeping
  chip holds high, DS §8.2.2), a SetStandby to STDBY_RC (DS §13.1.7), and the error word read
  with the status byte. The status must show STDBY_RC, else the chip is reset
  (`Error::NotStandby`).
- On a packet, received or failed, in a sniff, the frame is read first, at once, since a chip
  back asleep has cleared its buffer (DS §7.4); the interrupt's handling begins with a wake, in
  case the chip went to sleep since its interrupt.
- `XOSC_START_ERR` after a cycle is cleared and blamed on nothing; `PLL_LOCK_ERR` still steps
  the cold delay up. A sniff leaves no start to blame before a reset.
- The firmware waits for BUSY low before every command but the wake (`bark-fw/src/radio.rs`,
  `Hardware::idle`).

No change to the core, the spec or the core's interface.

## The other ends of a sniff

All take the same path, so each is covered:

- The core's next action (a check, a transmission, a reading, a listen): the chip may be in a
  window, saving its state, asleep or waking. A wake that meets it going to sleep is ignored
  (DS §13.1.1) and BUSY stays high for the sleep; the wake is then sent once more, as before.
- A header that fails, a payload whose CRC fails, a timeout: the chip goes on with its cycle
  (DS §13.1.7: no packet received), and the driver, which used to take each as the cycle's end
  in STDBY_RC, takes it out.
- A preamble and then nothing: the chip goes on with its cycle; the driver waits out the
  longest frame, as before, then starts the cycle again, through the same path.

## The oscillator's delays

D193's search and steps are sound when a reset follows another fault: each reset runs the
boot's search again from scratch, which on this board landed at 750 µs every time, so a delay
stepped up just before a reset is undone. What was wrong is what counted as a failure: the
flag a cycle's wakes raise. Fixed, a failure caused by this fault no longer counts: the cycle
blames no start, and the reset's own read blames only a start whose word was not read. By the
boot's lines on this board the delays settle at 750 µs cold (500 µs the lowest confirmed, one
step of margin) and 100 µs for a restart (50 µs and one step); the datasheet gives no TCXO
start, which belongs to the TCXO (DS §13.3.6), and board 1 found 500 µs at room temperature
(D88).

## What it cost a sleepy node before the fix

From the code, after each frame caught in a sniff:

- Nothing owed, such as a heartbeat or a piece: the restart of the cycle failed, the step
  returned the error, the loop paused 1 s, the radio was reset and measured again (the boot
  takes 0.11 s): about 1.17 s deaf and unable to send. A relay streaming an update, each piece
  caught, kept it there most of the time.
- A DATA addressed to it: the core asks for the ACK at once (spec "The ACK"). The transmission
  met the wedged chip and failed, which the core takes as sent; the reset followed. No ACK went
  out, so the sender's 100 ms ACK timeout ran out at every attempt; each attempt it caught
  failed the same way. Texts to sleepy nodes, status asks and the answers to its own asks were
  received and never acknowledged; whatever got through did so between failures.
- Its anchor's heartbeat falls in a window the core listens for; deaf then, it missed it.
- The cold delay climbed with every sniff the core ended, to 4 ms, which takes the excess off
  the sleep and half of it onto each window (`sniff::periods`), so the cycle cost more current.

Relays never sniff, and their radio took none of these paths: the relay's log has no error.

## Tested on the host

The fake chip in `bark-driver/src/sx1262.rs` now holds BUSY high while asleep in the cycle,
while saving its state (500 µs, ignoring the bus) and while waking (500 µs and the delay); a
wake edge in the sleep brings it to STDBY_RC in 340 µs (DS Table 8-2); each wake from the
sleep raises the flag; each sleep clears the buffer; `SetDIO3AsTCXOCtrl` outside STDBY_RC
holds BUSY high until a reset, as on the bench. A packet leaves it in STDBY_RC as the datasheet
says, in STDBY_XOSC, or going on with its cycle. Against main, with the frame then the cycle
started again or an ACK sent: in STDBY_RC the cold delay climbs twice; in STDBY_XOSC the
restart fails with `Chip("busy")`, the bench's error; going on with the cycle, the
configuration reaches a chip in its cycle. Fixed, all three read the frame and go on with no
error, no reset and the delays unchanged
(`a_frame_caught_in_a_sniff_is_read_and_the_radio_goes_on_without_an_error`), as do the other
ends of a sniff (`every_end_of_a_sniff_leaves_the_chip_in_standby_without_an_error`).

## To measure

- The quiet run again, long and short preamble: frames caught as before, and `radio_errors`,
  `resets` and `tcxo_errors` staying at 0, with no `radio:` or `tcxo: a cold start` line.
- With the relay streaming: S gathers blocks, hears its anchor, and answers asks.
- Where a packet leaves the chip, which would say whether the datasheet's line holds: the
  status byte before the SetStandby, not logged yet.

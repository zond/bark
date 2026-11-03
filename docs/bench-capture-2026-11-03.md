# Bench: two frames on one channel at SF5

**Status:** bench note, 2026-11-03. Three T-Echo 433 boards, the console
build of main `a9460bc` (image version 12) on the two senders, the
receiver 1.5 m from them on a desk. Channel 5, 433.795 MHz, SF5,
125 kHz, 12-symbol preamble, the console's `beacon`. The question: what
the SX1262 does when a second frame arrives while it is receiving one,
which the simulator models with a capture threshold
(`bark-sim/src/radio.rs`, spec "Physical layer").

## Method

One sender, the long one, beacons 255-byte frames every 1 390 ms, the
console's 10 % duty limit, 139.3 ms on the air. The other, the short
one, beacons 8-byte frames every 150 ms, 12.6 ms on the air, each with a
sequence number. The two beacon on their own clocks and do not defer to
each other (the long sender's period as the receiver saw it was 1 402.8
ms every time, the console's loop, never a skipped frame), so a short
frame starts at a random point of the long one about one time in ten.
The receiver logs every frame it decodes or fails with the receiver's
own timestamp, length, packet RSSI and CRC result.

From the receiver's timestamps of the long frames, decoded or failing
their CRC alike, each long frame's span on the air is known; a long
frame the receiver never saw at all is placed in the middle of a gap of
two periods. A short frame whose span meets a long frame's is an
overlap. The lost short frames are the missing sequence numbers, their
start times interpolated between the decoded neighbours. Before each
step each sender beacons alone for 6 to 8 s, and the median packet RSSI
of those frames is its level at the receiver; the difference, short
minus long, is the level column, resolved to 1 dB. Each step ran 120 s,
about 86 overlaps. Four sweeps covered the span: the long sender at 10,
0 and −9 dBm, the short one stepped from −9 to 10 dBm, with the boards
swapped between the roles once so that the short frame was both the
weaker and the stronger.

The scripts were run from this session's scratch space and are not in
the repository; the method above is complete.

## Result

Of the overlaps at each level, how many long frames were decoded, how
many short, and how many neither. Levels at the receiver were −56 to
−22 dBm for the short frames and −37 to −28 dBm for the long ones, all
far above the floor.

| short minus long, dB | overlaps | long decoded | short decoded | neither |
|---|---|---|---|---|
| −20 | 87 | 84 | 3 | 3 |
| −12 to −10 (4 steps) | 85 to 87 | 81 to 85 | 0 to 3 | 2 to 5 |
| −9 to −7 (4 steps) | 85 to 87 | 82 to 85 | 1 to 3 | 2 to 4 |
| −4 (5 steps) | 85 to 86 | 81 to 84 | 1 to 3 | 2 to 5 |
| −3 | 87 | 3 | 11 | 75 |
| −2 (2 steps) | 85 to 86 | 42, 19 | 2 | 44, 64 |
| 0 to +2 (3 steps) | 85 to 86 | 0 to 3 | 10 | 75 |
| +4 to +8 (5 steps) | 85 to 86 | 0 to 4 | 10 to 12 | 74 to 75 |
| +10 to +14 (5 steps) | 85 to 87 | 1 to 4 | 10 to 13 | 73 to 77 |

Short frames that did not meet a long one were decoded every time in
every step, and long frames that met no short one likewise.

Three things follow.

1. **A frame being received survives a later frame that is about 3 dB
   or more weaker, and is lost to one within 3 dB or stronger.** The
   long frames were decoded 81 to 85 of 86 at −4 dB and below, 42 and
   19 of 86 at −2 dB, and 0 to 4 of 86 from −3 dB up. With the levels
   resolved to 1 dB and each sender's level a median over 6 s, the
   transition is between −4 and −2 dB; 3 dB is taken.
2. **A later frame is never decoded once the receiver is past the
   preamble, however strong.** From 0 to +14 dB the short frames
   decoded in an overlap stayed at 10 to 13 of 86, and that count is
   the short frames that started *before* the long one, up to 12.6 ms
   before its start (about 7 of the 791 a step), plus those that
   started during its 3.1 ms preamble (2 to 3). The receiver does not
   leave a frame it has locked on for a stronger one.
3. **A collision within 3 dB costs both frames.** From −3 dB up, 73 to
   77 of 86 overlaps decoded neither frame.

## What the simulator does, and what it should do

`bark-sim/src/radio.rs` lets a later packet's preamble steal the lock
when it is stronger by `capture_db`, 5 dB, and loses the later packet
otherwise, the earlier one surviving either way. Measured here:

- A packet arriving while the receiver is in another's payload is lost;
  the one being received is lost too unless the arrival is 3 dB or more
  weaker.
- A packet arriving during another's preamble: the stronger is decoded.
  The few such overlaps a step here (2 to 3) show it happening but do
  not resolve by how much the stronger must win; the 5 dB stays a
  stand-in for that case.

The change is a simulator decision (the protocol core is not involved)
and is recorded in `roadmap.md` when made. It raises the cost of
collisions in the loaded city, where a fifth of the DATA between relays
is lost to other DATA (`docs/pushback-2026-11-03.md`): each such loss
now takes the other frame with it unless 3 dB apart.

## Not measured

- The preamble case's threshold: needs the two senders started within
  a few symbols of each other.
- Levels near the floor: both frames here were 60 to 90 dB above it;
  the 3 dB may not hold at the sensitivity limit.
- Other spreading factors and frames of equal length.
- The same for the receiver's own transmissions: a frame arriving while
  the board transmits is lost in any model.

## Found on the way

- A board queues console commands it receives during a receive window
  and runs them afterwards, in order. A listener script that answered
  every corrupt frame with another `rx 5 60000` left the receiver with
  hours of queued windows; nothing sent over the console gets ahead of
  them. The reset button (one press of the top-left button) clears it.
- The console's packet RSSI of 8-byte frames read 9 to 17 dB lower than
  255-byte frames from the same board at the same power in the first
  sweep, but not in the later ones; the senders' levels were taken
  from short frames alone at each step, so the difference is as the
  receiver saw the short frames.
- The two senders differed by 4 to 13 dB at the receiver at the same
  power setting, with their orientation and cables on the desk.

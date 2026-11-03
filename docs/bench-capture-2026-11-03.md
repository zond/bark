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
otherwise; the earlier one then faces the later as interference on the
error curve, which loses it mostly when the later is within about 2 dB.
Measured here:

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
- What a frame less than 3 dB weaker does to one held when it was
  already on the air at lock or arrived during the preamble.
- Other spreading factors.
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

## Two long frames

The same afternoon, the two senders both sent long frames: one 255-byte
frames every 1 390 ms, the other 200-byte frames every 1 250 ms, the
lengths telling the streams apart even when the CRC fails. Each overlap
lasts most of the shorter frame, 111 ms, and is of either order, so
one run gives both the held frame surviving a weaker arrival over a
long overlap and a strong long frame arriving late. Six minutes a step,
about 50 overlaps, half of each order. The 255-byte sender was at
−32 dBm at the receiver in the first six steps and −28.5 dBm in the
last two.

| 200-byte minus 255-byte, dB | 255-byte first: n, decoded | 200-byte first: n, decoded, 255-byte decoded |
|---|---|---|
| −16 | 29, 29 | 22, 1, 1 |
| −10 | 28, 28 | 22, 0, 1 |
| −6 | 28, 28 | 22, 0, 1 |
| −4 (2 steps) | 28, 28 and 28, 28 | 23, 0, 1 and 23, 0, 0 |
| −3 (2 steps) | 28, 28 and 29, 25 | 22, 0, 1 and 22, 0, 0 |
| −2 | 28, 16 | 23, 2, 2 |

Frames that met no other were decoded every time in every step.

- **A frame 3 dB or more weaker does no harm over a whole overlap.**
  The held 255-byte frame was decoded 28 of 28 at −3 to −16 dB, 25 of
  29 at the second −3 dB step, and 16 of 28 at −2 dB: the same edge as
  the short frames found, now over 111 ms instead of 12.6 ms. The
  simulator's error curve, which also charges a weaker arrival as
  interference, predicted about a third lost at −4 dB over an overlap
  this long; nothing was.
- **A later frame never takes the lock, whatever its length.** With the
  weaker 200-byte frame held, the stronger 255-byte one arriving 3 to
  16 dB above it was decoded 0 to 2 of 22, the preamble cases, and the
  held one was lost every time.

## In the simulator

Since roadmap D107 the simulator follows the result: a packet arriving
after the held one's preamble is lost, and the held one with it unless
the arrival is at least 3 dB weaker (`harmless_below_db`); a weaker one
still counts as interference on the error curve, which the two-long-frames
run above shows it should not below 3 dB. The error curve already
lost most such packets, so the change mostly moves losses from symbol
errors to the new count, and adds 3 to 9 % to the receptions lost to a
frame after locking. On main `e4b20eb`, seed 1, main → this model, it
changes delivery, airtime and the loss breakdown by no more than one
seed's spread:

| | city | city, twice the rate | S4 | walkers |
|---|---|---|---|---|
| delivered of the settled texts % | 97.5 → 97.5 | 97.7 → 97.6 | 93.4 → 93.5 | 96.1 → 96.2 |
| latency p50 / p90 s | 7.5 / 60.8 → 7.5 / 61.5 | 8.2 / 64.2 → 8.3 / 64.0 | 1.4 / 185.0 → 1.4 / 183.7 | 2.1 / 37.6 → 2.3 / 40.0 |
| airtime per delivered text s | 2.04 → 2.05 | 2.31 → 2.33 | 1.08 → 1.08 | 1.79 → 1.84 |
| hand-overs failed | 161 010 → 161 144 | 324 962 → 322 730 | 20 638 → 20 065 | 125 904 → 134 639 |
| DATA between relays received % | 56.2 → 56.3 | 54.2 → 54.8 | 65.4 → 65.6 | 54.0 → 53.3 |
| lost to a DATA to the same receiver / to another % | 1.3 / 4.0 → 1.3 / 4.0 | 2.4 / 7.1 → 2.3 / 7.0 | 1.3 / 4.2 → 1.2 / 4.1 | 1.0 / 2.8 → 1.1 / 3.1 |
| lost to an ACK, a heartbeat or a beat % | 5.4 → 5.5 | 6.2 → 6.3 | 4.3 → 4.5 | 3.2 → 3.4 |
| of the lost, by a later frame within 3 dB % | – → 4.2 | – → 5.5 | – → 3.7 | – → 2.7 |
| receptions lost to a frame after locking, thousands | 1 691 → 1 757 | 4 043 → 4 197 | 219 → 225 | 621 → 678 |

The walkers' larger moves are the one seed's runs parting: they send 5 %
more DATA between relays with the same delivery.

Since roadmap D108 a frame of ours at least 3 dB weaker than the held
one is no interference on its error curve, whenever it arrived, as the
two-long-frames run shows; the curve still charges interferer bursts,
other channels' leakage, noise, and frames less weaker that were on the
air at lock or arrived during the preamble. Against the D107 model, with
main `411398d`'s protocol, seed 1, the losses on the curve after locking
fall by a seventh to a quarter, while the 3 dB rule's own count and the
losses to noise do not move. That takes 2 to 4 % of the DATA between
relays and 3 to 6 % of the failed hand-overs off in every scenario, with
delivery flat; each figure alone is within one seed's spread, but all
four runs move the same way.

| | city | city, twice the rate | S4 | walkers |
|---|---|---|---|---|
| delivered of the settled texts % | 97.5 → 97.7 | 97.6 → 97.7 | 93.5 → 93.6 | 96.2 → 96.0 |
| latency p50 / p90 s | 7.5 / 61.5 → 6.9 / 61.9 | 8.3 / 64.0 → 7.9 / 65.2 | 1.4 / 183.7 → 1.4 / 184.5 | 2.3 / 40.0 → 2.1 / 35.5 |
| airtime per delivered text s | 2.05 → 2.04 | 2.33 → 2.23 | 1.08 → 1.07 | 1.84 → 1.78 |
| hand-overs failed | 161 144 → 152 355 | 322 730 → 306 372 | 20 065 → 19 391 | 134 639 → 127 262 |
| DATA between relays sent, thousands | 430 → 414 | 887 → 866 | 162 → 158 | 864 → 831 |
| DATA between relays received % | 56.3 → 57.3 | 54.8 → 55.6 | 65.6 → 65.8 | 53.3 → 54.5 |
| lost to noise alone % | 27.1 → 27.1 | 22.1 → 22.2 | 21.2 → 21.3 | 33.0 → 33.0 |
| lost to a DATA to the same receiver / to another % | 1.3 / 4.0 → 1.2 / 3.6 | 2.3 / 7.0 → 2.3 / 6.6 | 1.2 / 4.1 → 1.2 / 3.9 | 1.1 / 3.1 → 1.0 / 2.7 |
| lost to an ACK, a heartbeat or a beat % | 5.5 → 5.2 | 6.3 → 6.0 | 4.5 → 4.4 | 3.4 → 3.1 |
| of the lost, by a later frame within 3 dB % | 4.2 → 4.2 | 5.5 → 5.6 | 3.7 → 3.7 | 2.7 → 2.6 |
| receptions lost to a frame after locking, thousands | 1 757 → 1 575 | 4 197 → 3 591 | 225 → 205 | 678 → 587 |
| of them on the error curve | 929 → 738 | 2 443 → 1 837 | 136 → 117 | 366 → 284 |

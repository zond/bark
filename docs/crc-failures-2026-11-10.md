# Failed receptions on the bench

2026-11-10. Node 3112 counted about 500 failed receptions to 3 000 frames
over five hours on image 19 this morning, and about as many failed as
frames since it rebooted into image 23 at 09:32. This note finds what
changed and where the failures come from, from the four bench boards'
USB logs (3112, c367, f4f5 and bac7, within a few metres of each other,
at 10 dBm).

## What the counters count

Every 10 s the driver logs `frames` and `failed`, counted since boot
(`bark-driver`, `Stats`). `frames` are packets whose CRC passed, whether
or not they were meant for this node. `failed` are packets the radio
began and ended without a frame: the header CRC failed, the payload CRC
failed, or the receive timed out (`Sx1262::service`, the HeaderErr,
CrcErr and Timeout bits of the datasheet's interrupt status). The
running node does not say which; the console build's `rx` prints
`header_crc=error` and `crc=error` apart, with each frame's RSSI and SNR.

Below, the failed share is failed ÷ (frames + failed): of the packets
the radio began, the share that gave nothing.

## The figures

Per board and image, every boot of that image added up:

| Board | Image | Hours | Frames | Failed | Failed share | Frames/h | Failed/h |
|---|---|---:|---:|---:|---:|---:|---:|
| 3112 | 0 | 33.7 | 2 441 | 2 081 | 46 % | 72 | 62 |
| 3112 | 8 | 11.0 | 1 451 | 1 087 | 43 % | 131 | 98 |
| 3112 | 9 | 1.0 | 2 464 | 143 | 5 % | 2 442 | 142 |
| 3112 | 10 | 2.7 | 1 991 | 316 | 14 % | 730 | 116 |
| 3112 | 15 | 2.5 | 291 | 934 | 76 % | 118 | 380 |
| 3112 | 19 | 12.5 | 7 240 | 4 690 | 39 % | 577 | 374 |
| 3112 | 23 | 5.1 | 619 | 529 | 46 % | 122 | 104 |
| c367 | 0 | 19.3 | 1 852 | 2 372 | 56 % | 96 | 123 |
| c367 | 8 | 15.3 | 2 670 | 3 993 | 60 % | 174 | 260 |
| c367 | 15–18 | 3.8 | 782 | 3 968 | 84 % | 206 | 1 044 |
| c367 | 19 | 3.4 | 2 087 | 455 | 18 % | 612 | 133 |
| c367 | 23 | 10.4 | 1 680 | 2 495 | 60 % | 162 | 240 |
| f4f5 | 0 | 33.7 | 4 465 | 4 501 | 50 % | 133 | 134 |
| f4f5 | 8 | 4.4 | 645 | 457 | 41 % | 148 | 105 |
| f4f5 | 15 | 2.4 | 605 | 1 814 | 75 % | 250 | 749 |
| f4f5 | 19 | 5.3 | 3 450 | 4 019 | 54 % | 648 | 755 |
| f4f5 | 23 | 9.5 | 1 770 | 2 141 | 55 % | 185 | 224 |
| bac7 | 8 | 2.1 | 431 | 767 | 64 % | 207 | 369 |
| bac7 | 14 | 17.3 | 9 760 | 5 395 | 36 % | 564 | 312 |

(bac7's long boot from 13:41 yesterday shows image 14 in its banner.
Its 5 395 failed to 9 760 frames is 55 % of the frames and 36 % of all
packets begun.)

The failed share has been 40 to 60 % on every board since image 0, the
first image these logs hold. It falls only when frames rise, and rises
above 60 % only when failures rise while frames do not.

## What changed at 09:32

Nothing in the failures. 3112's boot into image 19 at 04:02 found its
neighbours holding image 23 and gathered it from their stream:

| 3112, image 19, from 04:02 | Hours | Frames | Failed | Failed share |
|---|---:|---:|---:|---:|
| gathering image 23, to 06:22 | 2.3 | 2 780 | 288 | 9 % |
| image 23 staged, to 09:30 | 3.1 | 319 | 227 | 42 % |
| image 23 running, from 09:32 | 1.8 | 245 | 218 | 47 % |

The update's pieces are long frames sent on 3112's home channel, so 3112
heard 2 500 more whole frames while it gathered; its failures went on at
70 to 125 an hour throughout. Its earlier low shares (images 9
and 10, 5 and 14 %) and c367's 18 % on image 19 overnight are the same:
each of those boots was gathering an image. Once the stream stops the
share returns to 40 to 50 %.

The same holds the other way: the share rises above 75 % when a node's
neighbours stream on other channels. Yesterday from 14:10 to 17:10 f4f5
gathered images 15 to 19 while c367 and 3112 served them, and c367's
failures rose to 1 000 to 1 600 an hour while its frames stayed near 200.

No change between images 19 and 23 touched the radio's receive
settings, the oscillator delay, the sweep, the listen-before-talk or the
channel plan: the radio's files (`bark-driver`'s `sx1262.rs` and
`tcxo.rs`, `bark-fw/src/radio.rs`, `bark-proto/src/radio.rs` and
`sniff.rs`) last changed on 7 November, for the external flash, a bench
filter and the app's neighbour list, and the share was the same before. The oscillator delays differ per board (750 µs on c367,
500 µs on the others), and the failed shares do not follow them.

## Where the failures come from

The console capture on f4f5 from 01:58 to 06:54 on 9 November listened on
channel 0 for 4.9 hours while the other three ran image 8. It printed 782
receptions: 498 frames, 95 payload CRC errors and 189 header errors. A
heartbeat moves one channel on at every slot of its sender's interval T
(spec "Heartbeat"), so the time from a sender's heartbeat heard on
channel 0 to a later reception, divided by its T, gives the channel the
later one was sent on:

| Sent by | Sent on | Level | Frames | CRC errors | Header errors |
|---|---|---|---:|---:|---:|
| 3112 | channel 0 | −20 dBm, SNR 9 | 242 | 0 | 0 |
| 3112 | channel 1 | about −50 dBm, SNR 0 | 0 | 37 | 105 |
| c367 | channel 0 | −36 dBm, SNR 8 | 153 | 0 | 0 |
| c367 | channel 1 | about −60 dBm, SNR −1 | 0 | 18 | 30 |
| c367 | channel 2 | about −73 dBm, SNR −1 | 80 | 11 | 1 |
| not matched | | | 21 | 29 | 50 |

So no frame sent on the listened channel failed, and 202 of the 284
failures were the neighbours' heartbeats on the one or two channels
next to it, heard 30 to 40 dB down. 3112's heartbeat on channel 1 was
caught 142 times of the about 240 it sent. c367's heartbeat two channels
away even arrived whole 80 times, and was counted as a frame heard on
channel 0. Of the 79 failures not matched, most are likely the same
from bac7 or from DATA, ACKs and pieces, which keep no schedule; at most
16 an hour, then, are noise that triggered the preamble detector.

This fits the running nodes' rates. Three neighbours at heartbeat
intervals of 5.7 to 14.6 s send about 150 heartbeats in ten minutes,
and up to four thirteenths of them, about 47, fall on the two channels
either side of the listened one. A third to a half of those caught, as
above, is the 15 to 35 failures a node counts in ten minutes when
nothing is streaming.

*Why a neighbouring channel is heard at all.* Channels are 125 kHz apart
and 125 kHz wide (spec "Channel plan"), so a packet one channel away
starts where this one ends. The datasheet's receive specifications give
adjacent-channel rejection only at 1.5 bandwidths off (60 dB at SF7) and
none at one bandwidth, the spacing used here; that is open item O5. At
a few metres a neighbour arrives at −20 to −40 dBm, 80 to 100 dB above
the floor, so what passes the filter a channel away, 30 to 40 dB down, is
still far above sensitivity, and the receiver detects its preamble and
tries its header. The bench note of 30 October saw the same leak with
the channel activity detection: at 20 cm a transmission was read one to
three channels away (`bench-2026-10-30.md`).

*Not overload of the listened channel.* Frames sent on the listened
channel at −20 to −37 dBm decoded with an SNR of 8 to 9 dB and none
failed, here or in the night of 3 November with everything on one channel
(0.08 % CRC errors, `bench-fading-2026-11-04.md`). The closeness matters
because it raises the leak above sensitivity, not because it breaks
reception on the channel itself.

## What this means

The cause is the bench, boards a few metres apart, together with a
channel spacing equal to the bandwidth; it is not in the code. In the
field a neighbour heard at −90 dBm leaks at about −125 dBm a channel
away, below sensitivity, so these failures need neighbours closer than
the field puts them, such as relays a few tens of metres apart.

What it costs where it happens:

- A leaked packet that passes its header holds the receiver for its
  length, so a frame on the listened channel that starts meanwhile is
  missed: for a heartbeat 14 ms, for a piece of an update up to 139 ms.
- A leaked frame that arrives whole is reported as heard on the listened
  channel, and its RSSI, 30 to 40 dB below the sender's real level, is
  averaged into the neighbour's (`Neighbours::frame_heard`, `heard`). On
  the bench the average stays far above the margin; it does skew the
  levels shown in the app.
- A preamble seen holds a listen-before-talk or a sweep reading until the
  packet ends, at most one longest frame (`held` in the driver's line).

## Telling the explanations apart on the bench

1. **Distance.** Put one board in another room, 10 m or more and a wall
   away, and log a night. If the leak is the cause, the failed share of
   the far board falls well below 40 %, and that of the near ones by
   about a third.
2. **One sender, one receiver.** With console builds: a receiver on
   channel k and a sender beaconing on k+1, then k+2, at 10 dBm and then
   at −9 dBm. The header and CRC errors per beacon give the detection
   rate against level and channel distance, the number O5 asks for.
3. **The counter split.** Counting header errors, payload CRC errors and
   timeouts apart in the driver's line would show the mix in normal
   running without a console build: noise that triggers the preamble
   detector fails at the header, a leak shows both.
4. **All on one channel.** A night with every node held on one channel
   should show almost no failures, as on 3 November; any left are noise.

## Not covered

The home channel moves seen on the bench (about every 20 minutes per
node) were not checked against the leak. Whether the busy estimate reads
the neighbours' leak as a busy channel and moves home because of it is a
separate question for the bench.

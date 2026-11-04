# Bench: a still indoor pair over a night

2026-11-04. Two T-Echo boards beaconing to a third on one desk for twenty
hours, to see how much the level of a link that nobody moves wanders. The
bench list in `roadmap.md` ("Fading and shadowing") asks for this over
24 h for three kinds of link; this is the first and easiest, and it was
cut short at 20 h when the morning's data were in.

## Method

The console build of main (`bark-fw --features console`) on all three
boards, on channel 5 at 10 dBm. Board c367, about 2.5 m from the receiver,
sent an 8-byte frame every 5 s; board f4f5, about 1.5 m from it, every 7 s.
The receiver 3112 listened in ten-minute windows from 16:40 on 3 November
to 13:00 on 4 November and printed each frame's RSSI, SNR, CRC result and
bytes. The two senders are told apart by their sequence numbers, which
each count up by one; a gap in a sender's sequence is a frame the
receiver did not decode. The script and the log are in the session's
scratch directory (`board/fading.py`, `board/fading_analyse.py`).

## Result

| | c367, 2.5 m | f4f5, 1.5 m |
|---|---:|---:|
| frames decoded | 14 537 | 10 436 |
| missed, by sequence gaps | 29 (0.20 %) | 36 (0.34 %) |
| RSSI over the night, mean | −24.8 to −26.1 dBm | −26.7 to −26.9 dBm |
| RSSI within an hour, min to max | 2 to 6 dB | 2 to 6 dB |
| step in the morning, 06:40 to 07:40 | −4 dB, to −28.7 | +1 dB, to −25.7 |
| SNR, mean | 8 dB, every hour | 8 dB, every hour |
| CRC errors, both | 19 of 24 992 (0.08 %) | |

Through the night the hourly means of both links held within 1 dB, and
within an hour the level stayed in a 2 dB band with a few readings 4 to
6 dB below it. One step came in the morning when people were up: c367's
link fell 4 dB and stayed there for the next seven hours; f4f5's rose
1 dB. The SNR reading sits at 8 dB whatever the level: the most the chip
reports at this spreading factor, so it says nothing about the link here.

The two senders' periods of 5 and 7 s put their frames on the air
together once in 35 s when aligned; the clocks drift apart, so the
overlaps are brief, and the losses above include them. The receiver
restarted its listening window every ten minutes, which can drop a frame
at the join; 122 joins, 65 frames missed in all, so most joins lost
nothing.

## What it says for the simulator

The scenarios give each pair 6 dB of shadowing fixed for the run and
4 dB of fading that changes over 60 s (`scenarios/*.toml`,
`capacity-research-2026-10-15.md`). A still pair across a room moves far
less than that: under 1 dB from hour to hour, 2 dB within an hour. The
scenario figures are for street links of hundreds of metres with people
and traffic in between, which this pair is not, so they stand
unmeasured. What the night does show is that the 20 dB swing seen between
two days on the same desk (the bench list in `roadmap.md`) is not
something a still link does on its own; a board or an object had moved.

## Not measured

- Indoor to street and rooftop to street, the links the capacity result
  hinges on. They need a board left outside and a day.
- A link at street range, where the 4 dB fading figure applies.

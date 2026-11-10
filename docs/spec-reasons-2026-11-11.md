# Reasons moved out of the spec, 2026-11-11

The reasons, measurements and history that `protocol.md` carried before it was cut to its rules on 2026-11-11 (roadmap D204), where no decision row holds them. Each section names the spec section the text came from; the text is as it stood then.

## Nodes

*Why nothing to set.* The nRF52840 detects USB power itself, so a
person's node relays on its charger and sleeps in a pocket.

The two combine in four ways, all of them real, and each flag means the
same whatever the other is:

| | not carried | carried |
|---|---|---|
| **not sleepy** | a node on USB at a window | a node on USB in a car |
| **sleepy** | a node on battery left somewhere | a node on battery in a pocket |

*Why either flag takes them away.* Mail is held only where a sleepy node
can count on finding it. A carried node would leave its sleepy nodes
behind, and as an anchor it would carry its sleepy nodes' addresses off
with it. It has no position at which anyone can address it for long, so
its senders learn where it is at the app layer
(spec "Addresses").

## Physical layer

*Why SF5.* It carries about 3× the bytes per unit of airtime of SF7
(the common LoRa default) for a quarter less range. LoRa brings an
existing driver, hardware error correction, CRC and whitening, CAD, and
~15 dB of processing gain against the key fobs and LPD433 handhelds that
share the band. Why not GFSK:
spec "What the protocol omits".

*Why network-wide.* An SX1262 receives one SF at a time, so a mixed-SF
network splits into parts that cannot hear each other. Starting at SF7
would force a network-wide switch later; the deployment is expected to
start at a density where SF5 range suffices
(spec "Capacity and sizing").

## Duty-cycled receive and the long preamble

Their cost is in spec "Sleepy nodes alone".

*Why the long preamble is that long.* The radio catches a preamble only
if a window holds `D` of it, the time the modem takes to lock, taken
here as 4 symbols (≈1 ms; to be measured in bring-up step 1,
`roadmap.md` Phase 2). A preamble that starts just too late for one window must reach
`D` into the next, so it is caught wherever it starts if it is at least
`gap + 2D` long.

*Why that window.* Once it has detected a preamble, the radio waits
`2 rx + sleep` for the header (datasheet §13.1.7: "Tpreamble + Theader ≤
2 × rxPeriod + sleepPeriod"), which ends 14.25 symbols after the
preamble. So the window must be at least `(wake + 2D + header) / 2`, and
half a symbol more to round the preamble up to whole symbols: `rx` =
3.75 ms, in the radio's steps of 15.625 µs. The header's wait and the
two detections set most of it: with no wake at all it would still be
3 ms.

*Why that sleep.* The sleep follows from the current. The node pays
`I_rx (rx + wake) / C` for its windows, at the 5.8 mA of listening, and
`I_tx C / P` for its own beats, each a preamble about `C` long every
`P`, at 32 mA (datasheet Table 3-6, the least listed for the SX1262 at
434 MHz). The sum is least where the two are equal,
`C = √(I_rx (rx + wake) P / I_tx)`: 238.9 ms at `P` = 60 s, and within
7 % of that from 165 to 345 ms.

Without an anchor a node's beats come every 34.6 s, where the least is
181 ms and 238.9 ms costs 4 % more; with one they come every 225 s
(spec "Beats"), where the windows dominate whatever the cycle.

*Why ordinary traffic keeps the shortest preamble.* A long frame costs
its sender and its channel 17 times a heartbeat's airtime and 2.7 times
a full frame's. Every node on a relay's channel would pay the long one
on every frame, and a relay forwards at the 10 % cap long before its
channel fills (spec "Duty cycle cap").

## Busy estimate

*Why not the dBm.* The average is of the 0/1 reading, not of the dBm:
what matters is the fraction of time listen-before-talk would refuse.

*Why foreign transmitters count.* What hurts a node is busy time at its
own receiver, which RSSI measures directly; counting neighbours per
channel would not, since two neighbours that hear each other share a
channel cleanly.

*Why one channel a second.* A reading of another channel deafens the
node for `g` = 1.9 ms. The modem needs a length `D` of preamble to lock
on, four symbols
(spec "Duty-cycled receive and the long preamble"),
so a gap loses a packet whose preamble starts too late before it for
the modem to have locked, or too early in it to leave `D` after it: a
window of `g + 2D − P`, with `P` the 12-symbol preamble, 3.1 ms
(spec "Physical layer"). That is 1.9 + 2.0 − 3.1 = 0.8 ms
a reading.

| | one channel a second | every channel each second |
|---|---|---|
| deaf | 1.9 ms a second, 0.19 % | 24 ms, 2.4 % |
| packet starts lost | 0.8 ms a second, 0.08 % | 11 ms, 1.1 % |
| samples a channel in a window | 138 in 30 minutes | 600 in 10 minutes |

Thirteen readings a second would cost every relay 1 % of what it hears;
one a second costs a thirteenth of that. The price is fewer samples: 138
a channel take 30 minutes, which only slows moves, and moves are meant
to be rare (spec "Channel moves"). 138 samples give ±2.6
points on a 10 % channel, half the difference the move rule acts on
(roadmap D86).

## Channel at boot

*Why a random channel.* Nodes that hear nothing, and nodes switched on
together after a power cut, then do not all start on channel 0.

*Why 15 readings suffice.* The first rotation reads each channel 15
times (spec "Busy estimate"), ±8 points on a 10 % channel:
enough to keep off a channel an interferer or a crowd of relays holds,
not to rank quiet channels against each other. Ranking them is the move
rule's, on its 30-minute window.

*Why it keeps a home among the quietest.* The move at the end is a move
like any other (spec "Channel moves"): neighbours that missed
its announcement send to the old channel for up to a rotation. A home
as quiet as the quietest gains nothing by it.

## Channel moves

*Why the hysteresis.* It keeps moves rare: without it, every rooftop
relay in range of the same interference sees the same ranking, and they
all jump together.

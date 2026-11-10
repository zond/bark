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

## Position

*Why no heartbeat before.* A heartbeat says where the node is, and
carries only the short position (spec "Positions on the wire"):
a node with no place can rebuild neither its neighbours' positions nor
its own beyond the short form's period, so any position or address it
gave would send its neighbours elsewhere. For the same reason its driver
checks its first fix against no neighbour.

*Why the first fix and not the fixed rule.* Measured from a position it
never had, the first fix is a far fix, and the node would be carried
for four hours, holding no mail, at the place it has just found.

*Why the app clears it.* A person set it, so a person ends it; a fix
cannot, since none is read while it is set.

*Why 72 s.* A walker crosses the allowance's whole width, 2 × 50 m, in
72 of the L76K's one-a-second fixes, so it is carried an owner area
beyond the radius. A still node's stray fix comes back inside; a
walker's keeps going. A sleepy node without an anchor fixes once a
delivery TTL (spec "Beats"), so for it 72 s is two far fixes at least
72 s apart.

*Why only time with fixes.* While the GPS runs and gives no fix, the
node may be anywhere; two far fixes with a minute without a fix between
are not a minute far. The driver says when the GPS, running, stops
giving fixes that pass the checks, and that time until the next fix
does not count. Its searches end at a fix, so a sleepy node's time
between them counts.

*Why the radio.* Moving changes every neighbour's signal: by
36 log10(1 + x/r) dB for a move x towards or away from a neighbour r
away under the path-loss exponent of 3.6, and more where the node steps
out of a building or past the walls that shadow it. A node that does
not move sees only fading.

*Why a map point rests it.* No fix moves a node with a map point
(spec "Nodes"), so the GPS has nothing to do.

*Why under a minute.* A clock that shows minutes needs no better, and
past that the node shows no time rather than a wrong one: on a crystal
taken at ±250 ppm, 2.7 days after the fix.

## Heartbeat

*Why weighted by the window.* The radio reads another channel once a
second (spec "Busy estimate") and sends its own heartbeats
and ACKs, so few windows pass with it listening throughout. A heartbeat
that came at some moment of the window was lost to the radio's absence
with the probability of the absence's share of the window, and with the
rest to the link; a frame the radio did not decode is not known to it,
so a collision it lost counts as the link's.

*Why a fixed cycle.* A fixed cycle reaches every neighbour once per
rotation; a random channel per heartbeat would need ≈41 heartbeats on
average to cover all 13.

*Why a heartbeat can be late.* It may find a hand-over attempt under
way, a full frame with the long preamble to a sleepy node outside its
window and the ACK timeout (spec "Hand-over"), and then back off
twice before its last check, 372.8 + 100 + 2 × 200 ms. A check held for
a reception in progress waits less, at most that frame.

*Why 1.1 s.* The answers to one heartbeat of the burst come within a
backoff of it, 200 ms, and may then wait as long behind their slot as
any heartbeat, 872.8 ms, and take a heartbeat's airtime, 14.4 ms: 1087.2
ms. The next heartbeat goes after them, so that the node is on its home
when they arrive.

*Why within a backoff.* Every relay on the channel hears the same
heartbeat; checking the channel in the same instant, they would all
find it free and send together.

*Why a node in its first rotation does not answer.* Nodes switched on
together learn each other from their bursts, which are heartbeats like
any other; answers would add to the busiest minute after a power cut.

**The 256 intervals** are 39.1 ms apart, more than two heartbeats'
airtime, 2 × 14.4 = 28.8 ms, so two nodes whose intervals differ slide
past each other by more than an overlap at every heartbeat and meet at
most once in a row, as random intervals would. 10 s / 28.8 ms = 347
steps would do, and 256 is the largest power of two below it, one byte.

*Why a shared interval does little harm.* Two nodes in reach share an
interval with probability 1/256, which matters only if they are also in
step, within 28.8 ms in a 10 s interval, 0.29 % of such pairs; their
heartbeats then meet each time they are on one channel, as do those of a
pair in step whose intervals stand 2:1 or 3:2.

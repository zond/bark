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

## Node identity

*Why six bytes.* An ID must be unique only among nodes close enough
that one relay hears them together; a pair that shares one there loses
about a quarter of its mail to each other (roadmap D59). At the density
of spec "Capacity and sizing", 714 nodes per km² in a
city of N = 300 000, a node has ρπr² others within a hearing range r
(spec "Physical layer"), and the city N ρπr² / 2 such pairs:

| | indoors, r = 300 m | street, r = 750 m |
|---|---|---|
| nodes within r of a node | 200 | 1 260 |
| pairs within r in the city | 3.0 × 10⁷ | 1.9 × 10⁸ |
| chance that one pair shares a 4-byte ID | 0.7 % | 4.4 % |
| chance that one pair shares a 6-byte ID | 1.1 × 10⁻⁷ | 6.7 × 10⁻⁷ |

So with four bytes a city at the design density has such a pair about
one time in fifty, and with six one time in millions. Eight would make
it rarer for nothing and cost as much again: each two bytes in the ID
fields add about 6 % to a text's channel time, 86.4 → 91.5 → 96.6 ms a
hop for four, six and eight bytes (spec "Capacity and sizing").

## Neighbour table

*Why that margin.* A 255-byte frame is then lost no more than one time
in a hundred, the datasheet's standard for sensitivity, after a fade of
4 dB. A packet arrives only if every symbol does: a 255-byte frame has
541 symbols, the datasheet's 64-byte reference packet 157, a heartbeat
56, so with the modulation's symbol error rate (32 symbols, detected
without phase) a full frame needs 0.43 dB more signal than the
reference for the same 1 % loss, and a heartbeat 0.39 dB less; half of
all heartbeats arrive even 3.1 dB below a full frame's level, so a
heartbeat says little about a full frame. Sensitivity is 2.5 dB below
the noise at SF5 (datasheet Table 6-1), so a full frame's level is
2.5 − 0.43 = 2.07 dB below the noise floor, and after the fade it needs
−2.07 + 4 = 1.93 dB above it. Packet RSSI is signal and noise together
(datasheet §13.5.3): 10 log10(1 + 10^0.193) = 4.1 dB above the floor.

The fade, 4 dB, is the standard deviation of fading over time and
channel in the simulated links, until Phase 2 measures it. The average
must cover it because a frame goes out on the neighbour's channel while
this relay hears the neighbour on its own, where the fading differs.

*Why any answer clears it, and nothing else.* A refusal or a seen ACK
shows, as a take does, that the neighbour heard the DATA and this relay
its reply. A DATA addressed to this relay shows nearly as much: the
neighbour chose this relay from its own table, so this relay's
heartbeats reach it, and it sends on this relay's channel now. A
heartbeat, or a frame to another node, shows only that the neighbour is
alive, not that it hears this relay; a link that works one way would
otherwise cost every message the failed attempts again.

*Why 39.* The neighbour's bucket sets its state: at its cap it takes a
full frame a ban, and its bucket refills from empty in 61 s
(spec "Duty cycle cap"), so 39 takes span one whole refill.
Fewer would follow the luck of single takes; more would remember a
bucket that has since refilled. It is kept in 32 bits, in 1/1024 of a
ban, a step of 1.5 ms; 16 bits would hold only 64 bans, 101 s.

*Why a sleepy node replaces none.* It is a last resort, and a relay
hears every sleepy node beating on its channel, often more than 64:
making way for them would push out the neighbours it routes through.

## Neighbour status

The app asks again a neighbour that has not answered within 2 s of its
round of asks, one round at a time, up to three asks of it in a look; a
refresh asks only the neighbours that have not answered within the last
10 s. On a quiet channel an answer comes about 100 ms after its ask;
under load each of the two frames may wait two backoffs and three
checks (spec "Heartbeat"), 409.3 ms, and to a sleepy node they
carry the long preamble, so the pair takes at most 1.33 s more, and 2 s
leaves the phone link room. 10 s is a choice: the mean heartbeat
interval, within which what a neighbour hears seldom changes.

*Why only on a look.* Said in every heartbeat, the same would cost
every node every rotation, for a map that is seldom open; asked, it
costs the network nothing between looks.

*Why the app asks again.* An ask and an answer are single frames on a
shared channel, lost to a collision or given up on a busy channel, the
more often the busier it is: on the bench, with two nodes streaming an
update, 4 of 12 asks were answered. The one who looks knows which
answer is missing, so it asks again, not the node; three asks bound
what a look costs, and a round at a time keeps the node's asks from
piling up behind each other.

*Why the image gathered.* During an update a person at the map sees
which neighbours still gather and how far they are, as each one's
screen shows it, without a console at the node; the last three fields
are its heartbeats' section's (spec "Updates over the mesh").
*Why its blocks.* The status then fills 13 blocks of symbols at SF5
exactly, so the byte costs no airtime, and the held blocks can be shown
against all.

## Duty cycle cap

*Why that rate.* Any interval of length L then carries at most
6 s + 9.83% × L on air, so any hour at most 360 s: 10%, the limit of
ERC/REC 70-03 band g1, which is measured over an hour (proof in
`tables/budget.rs`). The rate is below 10% by the bucket's depth over
the hour, so that a burst cannot carry an hour over the limit.

## Hand-over

*Why a byte in every packet.* Nodes are planted by anyone and updated by
hand, one at a time, so a new format must work beside nodes that never
get it: an updated node talks to its old neighbours in the old format,
and their heartbeats tell it, at no traffic, which speaks what. The byte
costs no airtime in the heartbeat and the ACK, whose 13 and 18 bytes
fill the same 6 and 8 blocks of symbols at SF5 as 11 and 17 would. In
DATA it adds a block, 1.28 ms, for two payload lengths in five: 0.51 ms
on average, under 1 % of a text's frame, and nothing for the 100-byte
text of spec "Capacity and sizing".

## Retry wait

*Why that unit.* 155.5 ms is how long one hand-over holds its receiver,
a full frame (138.6 ms) and its ACK (17.0 ms). A hand-over whose frame
has the long preamble holds its receiver longer by as much as its frame
is longer, 234.2 ms
(spec "Duty-cycled receive and the long preamble").

*Why a random wait that doubles.* Other than a weak link, the usual
cause of a missing ACK is a hidden terminal: a sender the first cannot
hear transmitted to the same receiver at the same time, and both miss
the ACK and retry. Their retries miss each other only if they start more
than one hand-over apart, which for full frames and a window of W units
happens with probability (1 − 1/W)²: never with one unit, 1/4 with two,
9/16 with four. Doubling per attempt also spreads more than two such
senders.

*Why a give-up waits the same window.* The backoff waits for a frame to
end, and a channel busy for 500 ms, longer than any frame, is held by
one hand-over after another: the wait is in their unit, and the senders
waiting for the same channel come apart as retrying ones do.

## The ACK

(roadmap D29). A second reply would go out only after the first and its
retunes, and taking a packet it may not answer in time would let its
sender hand the message to someone else as well.

## Bans

The senders a relay refuses stand in a **line**, each told to come back
at its own place, so that they return one after another as room frees,
not together to be refused again at the cost of a frame and an ACK
each. A place is rounded up to a code, so a sender comes back at most a
ban, or an eighth of a long wait, late; places are at least a ban apart,
or they would get the same code. The wait told is:

*Why silence doubles.* A refusal measures what one hand-over takes from
the receiver's bucket. Silence says only that the channel or the
receiver is full, or the link gone, not how full. Without the doubling,
a neighbour that stops answering under load is offered every other
message at once, each offer up to three more frames where frames are
already being lost.

## Next hop

message would only wander among that node's neighbours, each passing it
over in turn, to the hop cap. Refused by that node, it waits for it in
the delivery queue (spec "Delivery queue").

*Why a demotion ends the wait.* A neighbour whose heartbeats this relay
hears but which never hears it stays live, its heartbeats renewing it.
Waited for as the only candidate left, it would hold the message for as
long as it beats, and this relay's part of the search would never end.
Three hand-overs in a row unanswered are the test that it does not hear
this relay, so there the message lets it go. The cost falls on that one
message: where three collisions demote a working link that is the only
way on, it is returned or, at its origin, dropped; the next message
tries the link last.

*Why the next hop need not be closer.* After the message comes back, the
best neighbour left is usually farther from the destination than the
relay itself, and such a rule would turn every gap into a dead end. On a
first send the nearest neighbour is chosen anyway.

## Send queue

*What sets the 39.* The bucket holds at most 6 s
(spec "Duty cycle cap"), and a full frame and the ACK that
took it on cost 138.6 + 17.0 = 155.5 ms, so ⌈6 s / 155.5 ms⌉ = 39. With
the reply reserve left in, a full bucket takes at most 33 full frames,
so for full frames the bucket is the limit; shorter frames fill the 39
places first, and 39 hand-overs of them go out within a few seconds.

*Why route at each chance.* The table changes between one chance to
send and the next: a ban ends, a neighbour arrives, recovers or is
demoted. Routed only when its turn came, a message waiting for one ban
would miss a better way that opened meanwhile.

## Receiving DATA

Cases 1, 3, 4 and 6 end a packet without acting on it, and in each the
message lives on: it has arrived, the packet was answered when it first
came, another copy goes on, or, seen, the copy stays with its sender, so
no case ends the last copy of a message
(spec "Overload waits" lists what does).

Nothing is sent straight back, and no copy searches twice over the same
ground: a relay that has had the message goes on from where its own
search stands when the message returns to it, and a copy that reaches
it from elsewhere searches on from its sender's record, past this
relay. A return goes from a relay to its parent, and the parents, each
set when its record was made, lead back to the origin without a loop,
so a message cannot go back and forth between two relays that hold none
of it (roadmap D35).

## Return to parent

*Why hold it.* Within seconds of its boot a node has heard no relay
yet, though its burst brings it the relays in range within about 15 s
(spec "Heartbeat"): dropped then, the person's first text would
be lost to the boot alone.

## Hop count

*Why not start higher.* Sent with 1, the message could take hop
place 1 at its first hop, which overload must not give to fresh work
(spec "Send queue").

So along the path of a message the count only grows, and a relay sends
a count for a message once, but in retries: to send another packet of
it, it must have received one since its last hand-over that succeeded or
was answered seen, which spends the count the receiver recorded, or that
hand-over failed and the packet goes elsewhere. After a wait for a ban
it may also go again to the neighbour that did not answer; if that

*Why the cap.* A search that keeps its records ends on its own
(spec "Why the search finds a path"); the cap
ends one that has lost its way because a record was replaced or the
message split in two, and it ends the chain of hop places
(spec "Overload waits"). 255 is generous: the longest
straight route across the city of
spec "Capacity and sizing" (≈30 km at the ≈750 m
street range of spec "Physical layer") is about 40
hand-overs, leaving about 200 for routing around gaps.

## Why the search finds a path

Suppose that while a message is being routed the links do not change,
every hand-over to a neighbour that answers succeeds within its
attempts, and no record is replaced. Then:

- The message moves as a single packet: a relay sends only what it has
  just received or originated, and every ACK-only case of
  spec "Receiving DATA" sends nothing.
- Each stay of the message at a relay ends with a hand-over that
  succeeds, or at the origin with a drop (spec "Next hop",
  spec "Return to parent"), and there are finitely many of
  those: a relay hands the message to each neighbour at most once, since
  one that takes it is tried (spec "Message records"), and
  the parents lead to the origin without a loop. So the search ends.
- A neighbour that does not answer costs a bounded number of failed
  hand-overs (spec "Bounds"), and the hop cap bounds the stays
  (spec "Hop count").
- A send to a relay that already has the message and holds none of it
  is answered seen (receiving DATA, case 6): the sender marks it tried
  and goes on with its own candidates, and the relay's own search, if
  unfinished, goes on when the message returns to it from its child. So
  a relay met again is neither searched twice nor skipped: its
  neighbours are its own to try, and the message stays a single copy. A relay whose
  search is unfinished gets the message back through the parents, so
  none is left with neighbours untried.

So the message makes a depth-first traversal of the relays reachable
from the origin through neighbours that answer, nearest to the
destination first. A depth-first traversal visits every relay it can
reach; it stops at the first owner, or returns to the origin when there
is none. If a path to an owner exists, the message finds it, provided
the traversal fits in the 255 hand-overs of spec "Hop count".
The search cannot stall on a seen answer, since the sender keeps the
message and its next choice is its own; nor loop, since the message
moves only when a hand-over is taken, and each relay hands it to each
neighbour at most once.

What breaks it:

- **A lost record.** A message returning to a relay whose record has
  been replaced is new there; the relay searches again from itself, and
  the relays above it never get the message back. The search ends early,
  or wanders until the hop cap ends it.
- **A split.** A hand-over whose ACKs were all lost looks failed, so the
  sender moves on while the receiver also carries the message: two
  copies search at once. They share the relays' records: a copy that
  reaches a relay the other has crossed is answered seen and searches on
  past it, into relays neither has reached, or back up its own parents.
  A copy ends where it meets the other held at a relay (receiving DATA,
  case 4), where the message has arrived (case 1), at the origin with
  the search, at the hop cap, or as a return to a parent that gave the
  child up and answers seen (case 6). Only the last can end a copy that
  is the last one, and it takes every ACK of three hand-overs on one
  link lost.
- **Links that change** during the search, including a parent that moves
  to another home channel: parts of the graph go unsearched.

## Bounds

*What a neighbour that does not answer costs.* Each failed hand-over to
it bans it for every message, the ban doubling up to 61 s
(spec "Bans"), until it leaves the table 390 s after its last
heartbeat or answer: the bans 1.58, 3.16, 6.3, 12.7, 25.3 and 50.6 s add
up to 100 s, and 61 s bans fill the rest. After three failures it ranks
behind the others (spec "Neighbour table") and the message
that waited for it moves on, so the first message pays two bans and
three hand-overs, 14.2 s, and every later one a hand-over, 3.15 s, until
the demotion lapses and the count starts again. A hand-over is at most
three attempts of 738.6 ms, listen-before-talk until it gives up, a full
frame and the ACK timeout, and the retry waits, 0.93 s
(spec "Hand-over"). A channel that stays busy adds its wait to a
forward's or a return's hand-over, but no airtime (step 2). A neighbour
that answers this relay between failures is not demoted and holds a
message as long as its bans, with no bound by count: it hears this
relay, and each offer is another chance for the frame to get through;
if it refuses the message, the message goes round it where another
candidate is left (spec "Next hop").

*Why that many records.* A message sent on can come back, returned by a
child or offered by a split copy, and its record must still be here
then. On its way it may wait at each relay as long as a full send queue
takes to drain, each of the 103 places, 39 regular and 64 hop places,
waiting out one ban of 1.58 s: 162.9 s. A ban after silence lasts up to 61 s, so where every way on is
silent a message can wait longer; its own record is kept while it is
held, and this bound covers a wait of one ban per place behind
refusals only (roadmap D42); a refusal may ask for more
(spec "Bans"), and a message held longer than the records last can
come back to a relay that has replaced its record, where the hop cap
ends a search that loses its way (spec "Hop count").

Every message a relay handles costs at least an ACK (18 bytes, 17.0 ms)
and one DATA packet with no payload (28 bytes, 22.1 ms,
spec "Physical layer"), together 39.0 ms.

Any 162.9 s carries at most a full bucket and its refill,
6 s + 9.83% × 162.9 s = 22.0 s of airtime
(spec "Duty cycle cap"), so a relay handles at most 564
messages in that time. The records of held messages are never evicted
and may be older, so the table has room for the 103 a send queue holds
on top: 667.

## Sleepy nodes

*Why those levels.* At 20 % a relay has relayed through four fifths of
its cell and keeps the rest for its owner. The curve is flat there,
34 mV from 20 to 30 %; the gap is several times a reading's noise and
the load's drop across the cell, so the role does not flap, and a cell
rises without external power only on a solar box, which then relays
again.

*Why it calibrates.* The SAADC's gain error at gain 1/6 is up to ±3 %
(nRF52840 Product Specification, SAADC), which at 3.74 V spans 4 to 57 %
of a cell's charge. A cell that has finished charging and rested reads
the curve's 100 %: the T-Echo's 500 mA charger brings an empty 850 mAh
cell to its end voltage within 1.7 h, its constant-voltage end is
assumed to take under 1.3 h more, and AN4189's readings rest an hour.
Scaled so, a board is left with the charger's tolerance on its end
voltage, about 1 %, and turns between 14 and 32 %. A scale beyond the
gain error is of a cell that was not full. A relay unplugged from a full
charge calibrates within its first hour, days before it is low; only a
discharge begun short of full, before any calibration, turns anywhere
between 4 and 57 %.

*Why once a minute.* A reading a minute late is under a tenth of a
percent of the cell at the most a board draws, below a reading's noise,
and a reading costs a sample of microseconds.

*Why an hour.* A charger moved across the room then costs nobody
anything, and a node taken off its charger to be carried away is seen
carried within it (spec "Position").

*Why a TTL at most.* The flush goes through the duty budget as any
delivery, and where the channels are full it could wait for hours with
the node listening at full current. A TTL is what the network promises a
node it cannot reach.

*Why hand it on.* A message ends only as
spec "Overload waits" lists (roadmap D31), and a node that walks off or runs low is none of those.
Its address is still where the network can find the message's node: the
relay that owns it now, or the node itself as a neighbour, takes it on.

## Relay choice

*Why at once.* Its address is the anchor's position, which its senders
still use, and its holder may hold mail for it.

*Why that margin.* It is two standard deviations of the difference, so
that a relay no better takes the node on 2.3 % of comparisons. A relay's
heartbeats reach the node's channel about 130 s apart, longer than the
60 s the fading lasts, so they are nearly independent.

Under fading of σ = 4 dB (the scenarios' figure) a mean of two
heartbeats has σ/√2, and the difference of two such means
σ√(1/2 + 1/2) = 4 dB. The neighbour table's average lets each new
frame count half, which varies less than a mean of two (σ²/3 against
σ²/2), so 8 dB is a little more than two standard deviations there.

*Why it wakes.* Between its windows a sleepy node receives duty-cycled,
which catches no heartbeat with the ordinary preamble, so it hears its
anchor's rotating heartbeat only if it listens when the rotation brings
it to its channel. Hearing the heartbeat is how the node knows the
anchor is alive, and how it follows it to a new home channel: the
heartbeat names the anchor's home, and the next on the new channel comes
when the rotation brings it there.

## Beats

*Why that period.* It is half the delivery TTL, so that two beats are
never more than a TTL apart. A holder that keeps a node for a TTL then
loses it only after two lost beats, and keeps a node that has gone for
a TTL, as long as its messages are kept at least
(spec "Heard table").

*Why random.* The spread keeps two nodes whose beats met once from
meeting again. The interval is random, not fixed, since nobody wakes for
a beat: a holder hears it as it listens.

*Why two rotations.* A neighbour heard anchorless once may be switching
on, and will anchor within minutes. One still anchorless a rotation
later is in a pocket with no relay, whose nodes mostly know their way
out already, from the time both were anchorless, so waiting a rotation
costs them little.

## Heard table

*Why that size.* 256 is three times the ≈80 sleepy nodes per relay
expected at indoor density in Kyiv with 10 % adoption
(spec "Capacity and sizing"); at 50 %, ≈400, a full
table does no harm (above).

## Delivery queue

*Why age alone drops nothing.* A message dropped when no other needs its
place is lost for nothing, and its node may yet beat, back from a pocket
or a basement, or heard at last by a holder that missed its beats.

*Why that size.* Nodes in one train or one basement go dark together, so
the queue holds what arrives in a TTL for every node a holder can keep,
its heard table's 256 (spec "Heard table"), at the 23 texts a
day each that the network carries
(spec "Capacity and sizing"): 256 × 23 / 144 ten-minute
TTLs in a day = 40.9, so 41 messages.

*Why it waits.* A leaving node is there but not now, as a sleepy node is
between its beats, and is back within the drain's bound and its swap.
Sent round it, a message that can end only there would wander among its
neighbours to the hop cap.

## Delivery

*Why the flush.* The node is leaving: once it has gone its sleepy nodes'
beats no longer reach it, and the long preamble reaches them now,
whatever their windows. Each message is flushed once, so a still node
whose fixes stray past the carried radius again pays no more. The cost is
bounded: a full queue, 41 messages of 372.8 ms, is 15.3 s of airtime,
two and a half buckets, paced by the bucket.

## A sleepy node's own message

*Why a fade.* A fade lasts about a minute (the scenarios' correlation
time), and the three attempts of one hand-over, a few hundred
milliseconds apart, all fall inside it, where the doubling bans would
offer it every few seconds to a node whose channel is full.

## Sleepy nodes alone

*Why that period.* Beats at most a period apart visit every channel
within a TTL, so a relay in reach on any channel hears the node, and two
nodes in anchorless mode find each other, within the least time the
network keeps what it holds for them (spec "Sleepy nodes").

## Finding the node

*Why the name in the scan response.* A legacy advertisement holds 31
bytes (Vol 6 Part B §2.3.1): the flags take 3 and a 128-bit UUID 18,
which leaves 10, too few for the name's 17.

*Why 1022.5 ms.* A person who opens the app waits about a second for
the node to show, and an iPhone finds a node quickly only at one of the
intervals Apple's accessory guidelines list, of which this is the one
nearest a second (spec "Sources"). Faster costs a sleepy node more: an
advertising event on the three channels is about 2 ms of the radio at
4.8 mA, about 11 µA at this interval, near a tenth of its duty-cycled
receive (an estimate from the nRF52840 Product Specification's radio
currents, for the bench to measure).

## Pairing

*Why every paired phone is an owner.* The passkey shows only on the
node's screen, so a phone can pair only while its person is at the
node: being there is what a person can show without accounts or a
server. The node trusts everyone who pairs alike, each with every
owner's setting: being at the node is the only guard, for the second
person who pairs as for the first. A pairing adds and does not replace,
so a visitor who pairs with a café's node by the code on its screen
takes it from nobody, and its keepers see the visitor among the node's
phones and may remove them (spec "Phones"). Each phone sets or
clears only its own person's mailbox key (spec "Settings").

*Why eight.* A record has a bit of its own in each slot's mark in the
ring (spec "Mailbox", The ring), and the mark is a byte, so that
one person's reading marks nothing of another's; eight covers a
household of four to six with room. Nothing else bounds it lower: the
BLE host holds nine bonds, the eight and the one a pairing makes before
the node takes or refuses it; eight records of at most 128 bytes are
1 KiB, a quarter of one 4 KiB page of flash; and the links fit (below).

*Why end the link.* The node could instead refuse the key and keep the
link unencrypted (LE Long Term Key Request Negative Reply, Vol 4 Part E
§7.8.26), and the phone's first read would fail with insufficient
authentication. But a phone refused its key may start pairing by itself
and ask its person for a code before the app can say why. The BLE host
ends the link, and the node keeps that.

## The service

*Why both.* An Android app must ask for the MTU, and iOS chooses it
itself, which the app cannot raise; a notification has no long form.

So the app can show "listening, 3 relays heard, 1:40 to go", and the
address the person gives others to reach them. Sleepy and carried are
measured, not set (spec "Nodes"). The fields go from what the node is
to what it is doing: its flags, its address and its battery; whom it
hears; its first rotation and an install; and what waits for the phone.

*Why one value, with ready in it.* Ready and the count are the reading
phone's own, the rest the node's, but every field is something the app
shows or acts on as soon as it changes, and one value is one
notification and one read at a connection. Ready is the cue to write
Send, and a value of its own would be one more subscription for one
bit. The eight flags fill one byte; booting is told beside boot_left
because the end of the first rotation changes no other notified field,
and the countdown is not notified.

## Sending

*Why the value stays.* The phone may not be connected when the message
is given back, and the node takes no other message before the wait is
over, so the app learns of it whenever it next connects.

## Receiving

*Why read and confirmed, not notified.* The phone is often not
connected: the app is closed, the phone is out of range or off. A
notification is confirmed by nobody, an indication only by the phone's
Bluetooth stack, not by the app that must keep the message; the ID
written back is the app's own word that it has it.

*Why the ID it arrived with, kept.* An ID is a hash of the node's
address (spec "Packet formats"), which changes whenever the
node's position does, at every fix while it is carried
(spec "Position"); worked out again at each read, an item's ID
would change between the phone's read and its confirmation.

## The neighbour map

A link's **quality** is the margin of its averaged RSSI over the noise
floor, reckoned as the node's margin is (spec "Neighbour table"): fragile below that margin, 4.1 dB, where a
full frame is lost more than one time in a hundred in a fade of one
standard deviation, 4 dB, and the node sends through the neighbour only
when no other is left; good from 6.9 dB, where a full frame gets
through a fade of two, 8 dB, which comes one time in 44, the convention
of the switching margin (spec "Relay choice"); fair between.
The floor and the RSSI in tenths are there so that the app reads the
margin as the node does.

*Why pages from a slot.* The table holds 64 neighbours of 18 bytes, more
than one value holds, and every value fits one PDU
(spec "The service"). A neighbour keeps its slot while it
stays in the table, so when one leaves between two reads the others
stay where they were, and the next page skips none of them; counted
from the first live one, they would shift down by one, and one would be
skipped.

*Why a characteristic of its own.* The answer is notified, and a
notified value is the characteristic's own: Neighbours' is a page of
the table. A write of an ID to Neighbours would also change what the
app writes there, which only a new version may (spec "Versions").

## Updates from the phone

*Why without a response, seven at a time.* A write request is answered
in a later connection event than the one that carried it, so a phone
that waits for each answer sends one piece every two events; without
a response it sends as many as an event carries. The node's host
queues 8 ATT PDUs for a link and drops one more, so seven pieces and
the read fit; the read is answered only once the node has written the
pieces before it, so it is also the flow control.

*How long it takes.* An app asks Android for a short connection
interval for the transfer, 11.25 to 15 ms (spec "Sources"). A
packet of 251 bytes takes 2.12 ms on the 1 Mbit/s PHY, 2.5 ms with the
node's empty answer and the two 150 µs gaps between (Vol 6 Part B
§2.1, §4.1.1), so the node's connection event of 7.5 ms and its three
receive buffers take three pieces an event. Seven pieces take three
events at 15 ms; the node writes them meanwhile, each 240 bytes over its
external flash's bus at about 1 Mbit/s, 2 ms, two page programs on
average, 2.6 ms, and a sector erase every 17 pieces, 0.6 ms each: 36 ms.
The read's answer goes in the next event and the next pieces in the one
after: about five events, 75 ms, for 1680 bytes, 22 KB/s. An image of
400 KB takes about 18 s, and the commit's hash, a read of it over the
same bus, about 3 s more. At Android's default of 30 to 50 ms the node's
writing hides inside the three events, and the same five events take
twice as long or more, 40 to 60 s; with a write request for each piece,
two events a piece, it would be 50 s at 15 ms.

## Public nodes

*Why on Public.* It is the owners' setting of whom the node serves, and
the last unpairing ends at the node public by default, its end state; a
characteristic of its own would add to the table for one byte.

*Why the public node's UUID, not a flag beside the service's.* The 10
bytes the advertisement has left beside the flags and the service's
UUID hold one more AD structure (Core Specification Supplement, Part
A, Data types); those that carry a value of one's own name it by a
company identifier or a 16- or 32-bit UUID that the Bluetooth SIG
assigns, of which bark holds none, and the form with a 128-bit UUID,
service data, takes 19 bytes. A second UUID costs no byte, and both
phone systems filter a scan by service UUID in the controller, where a
flag would be read only by the app.

*Why one guest's link, and short.* A link costs 5.9 KB of static RAM
whether it is used or not (spec "Pairing"), and one held open buys
a guest nothing the ring does not: the ring already holds mail for a
phone that is away, as it does a paired phone's, so a guest needs a
link only for its errand.

*Why the ring, by tag.* The tag says whose a message is without opening
it, and the ring keeps it in flash until it wraps whether its phone is
there or not, across reboots; the guest's number says what it has read,
so the node keeps nothing per guest.

*Why Inbox, with a number written to Guest.* A guest's mail is its
inbox, read as a paired phone's is; it carries the item's number, which
the guest writes back to read on. A readable value of its own would add
a characteristic to the table, and 235 B of RAM, for what Inbox already
reads.

## Phones

*Why a code, and not the bond alone.* Every paired phone is trusted
alike, and the link reaches through walls, tens of metres: with the
bond alone, a phone paired once and gone, lost, stolen or a former
household's, could take a person's phone or home from anywhere in
reach. The code asks what pairing asks, being at the node. A phone that
unpairs itself needs none (spec "Public nodes").

*Why the day.* The node writes a record again when its phone's day
changes, at most once a day for each, so that phones coming and going
do not wear out the flash; a day tells a lost phone from one in use.

*Why 30 s, and one try.* 30 s is the time a pairing gives its person to
type the code, the Security Manager's timeout for each step (Vol 3 Part
H §3.4). A code is one in a million, and one try for each code drawn
makes guessing cost a code on the screen, where the people at the node
see it.

## The payload

*Why keyed with the identity, over the node.* Only someone who holds
the recipient's card can make the tag, and it differs from node to
node, so it ties together only the payloads for one person at one
node, which their destination address already groups by node.

*Why one byte.* A phone handed a payload that is not its own fails to
open it and drops it, so a wrong match costs a read and a failed open,
not a message. A node's ring holds the mail of the people paired with
it and of its guests, read by tag: a person's inbox passes over
another's item unless the tags match, and a guest reads, beside its
own, the items of anyone whose tag at the node is the same, each other
person's with odds of 1 in 256, 0.4 %; a second byte would make that
rarer at a byte on every message, and would tie a person's payloads at
a node together more surely.

sender: sealed to the sender's identity, it shows that the receiver
holds the sender's card. Waiting for a boxed one instead would leave two
people who hold each other's cards sending introductions for ever, as

*Why the time without a home.* Clearing the home changes the card too,
and a card that drops it must replace an older one only if it is later.

*Why unique.* Two messages to one destination with identical payloads
are one message to every relay that still keeps the first one's record
(spec "Message records", spec "Bounds"), and the second
is not delivered.

## Where a text goes

The app keeps a contact's home, from their card, apart from one place
they were last known at, and when: the address a message from them was
written from, at its `sent`, or at its arrival when `sent` is ahead of
the phone's clock, and the node on a card of theirs, at its link time;
the newer wins, and a message without an address keeps the place and
moves its time. While the place is under a minute old, a text goes
there; past it, with a home, the person chooses between where they were
and the home's mailbox, for each text, and without one it goes there,
as nothing else is, the app saying how old it is. With a home and no
place, or the place the home, it goes to the home (roadmap D132, D162).

*Why where they write from.* A reply goes straight to where the person
is, rather than to their home's mailbox to wait for a fetch, at
13 bytes on a message after a change.

*Why a minute.* In a minute a person on foot, at a brisk 1.5 m/s, moves
about 90 m, well within the ≈750 m a relay hears a handheld in the
street (spec "Physical layer"); a person driving has left
that reach by then, so a longer wait would not make the guess better.
Past it the app cannot tell whether they are still there, and the
person may know.

## Which node a phone uses

A phone may be paired with many nodes, each holding its bond. Of the
nodes it hears well, at −85 dBm or above, it uses its home when it is
paired with it, else the node it is paired with that it hears
strongest, and is the owner there. It uses the public node it hears
strongest, as a guest, while it is paired with none that is still its
own (spec "Pairing", the removed phone), while those are out of
reach, and while it hears them only weakly. A node is out of reach when
the app's scans, 15 s apart, have reached it in neither of the last two,
30 s; for a node that said it was installing, counted from 55 s after
it ended the link, the time a node is away for its swap
(spec "Updates over the mesh", the install), and
while it waits for that node it uses no other. A node
heard weakly is passed over for one further down the order heard well;
when the phone hears no node well, it uses the one it hears strongest,
of whatever kind. A scan that hears one of its own nodes well takes the
phone back to it at once.

On a node it is paired with that is not its home, the phone listens for
the home once a minute and moves to it once it hears it well and nothing
on the link is under way: no message written and not yet taken, none
read and not yet confirmed, no fetch awaiting its answer, no update. It
moves only up, from a public node to one it is paired with, from such a
node to the home; never between two of its own nodes nor two public
ones; and not from a node the person chose, until that link drops. A
phone joins a node over a bond only as its owner, never as a guest, as
the node takes a link encrypted with its bond as the owner's or ends
it: a bond with a node the app does not count as the person's it has
the phone remove as soon as it hears that node, before any link.

On a public node the phone is a guest like any other: its texts go from
that node, so replies come to it, and it reads them there by its
recipient tag; a fetch it sends there carries its tag at that node, so
the home's answer comes there too (spec "Mailbox", The fetch)
(roadmap D163, D166, D171, D172).

*Why the home first.* The home keeps the person's mail and its address
is on their card, so on it nothing waits elsewhere for them; on another
of their nodes, texts sent to the home wait there to be fetched.

*Why a weak node is passed over.* A link to a node at the edge of its
reach may not hold, while a node nearer would. −85 dBm is a choice, the
line below which the app calls a signal weak, as the phone hears an
advertisement before any link. Below it the order gives way to signal,
as of nodes that may all fail to hold a link the strongest is the
likeliest to hold. It is applied only when the phone takes a node: a
link once up is not left for its signal, so a node heard about the line
does not move the phone back and forth.

*Why two scans, and back at once.* A scan lasts four of the node's
advertising intervals, 4.1 s, and hears a node in reach while the
phone's screen is on; with the screen off the phone listens less and
may miss it in one scan, so one miss does not prove the node gone. A
node heard weakly was not missed, so the public node goes before it at
once. A scan every 15 s is two in 30 s, within the five Android lets an
app start in 30 s. A node the phone is paired with serves the person
whole, its inbox and its settings, so the phone goes back to one at
once. The two differ on purpose: at the edge of such a node's reach,
heard in some scans and not others, the phone stays with it rather than
going back and forth, and a text and the replies to it do not go by a
public node the phone then leaves.

*Why a minute, and only up.* On a standing link a scan serves only this
and costs the phone's battery: 4.1 s a minute listens 7 % of the time,
against 27 % every 15 s, for a home reached at most 45 s sooner. A
minute is also how long a person on foot stays within a relay's reach
(`bark-app/README.md`, "Where a text goes"). A guest's link lasts one
errand, so the next scan, at most 15 s on, takes a node the phone is
paired with. Between two nodes of one place in the order, heard in turn
at like signals, the phone would move back and forth. Nothing under way
is cut, as a message cut between its write and the node's answer may go
twice, and a fetch's answer comes to the node it was sent from. A node
the person chose they may be at for its settings or an update.

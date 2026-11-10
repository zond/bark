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

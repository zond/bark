# Collapse at the edge of capacity: channel moves, deliveries, and dropping

Research note, 2026-10-18, roadmap D56. Spec references are to
`protocol.md`. Builds on `collapse-research-2026-10-16.md`,
`backoff-2026-10-16.md`, `admission-2026-10-16.md`, `ttl-2026-10-16.md`,
`bandwidth-research-2026-10-16.md` and `realism-2026-10-18.md`.

**Baselines.** The ideas were found and measured against main `8704790`
(D53), and every table below says so unless it names another. The branch
was then rebased twice; the first two tables, and the strip, compare it
with main `4ccbaf4` (D58), on which every sleepy node receives
duty-cycled and a holder may deliver outside a node's window with the
long preamble, so the domain spends 70 % more airtime at the spec rate and
sits closer to its cap. A few tables against main `786b514` (D57) are
marked so.

## Summary

S4 at twice the spec "Capacity and sizing" rate holds 80 to 91 % for half
an hour to over an hour and then, on most seeds, slides into collapse; the
strip collapses at the rate itself. The question was how a hot spot at its
cap can shed load without taking its domain down. The answer found is
that at twice the rate the hot spot need not be at its cap: the slide puts
it there, and the slide is driven by the relays' home-channel moves. On
main `8704790` no relay comes near its cap at twice the rate once moves
stop; on main `4ccbaf4`, which spends more airtime, a few do, and the
domain still carries the rate at 93 %.

| S4, two hours of load, delivered, seeds 1, 2 | ×1 | ×2 | ×3 |
|---|---|---|---|
| main `4ccbaf4` | 93.9, 93.8 % | 71.4, 89.1 % | 14.5, 12.0 % |
| this branch | 94.0, 93.7 % | 93.0, 92.9 % | 51.5, 50.9 % |
| this branch, dropping the oldest when the room is full (not on the branch) | | | 82.4 % (seed 1) |
| this branch, 6 channels of 250 kHz (not on the branch) | | | 35.2 % (seed 1) |

| Strip, one settled hour, one seed | half the rate | the rate |
|---|---|---|
| main `4ccbaf4` | 94.0 % | 24.6 % |
| this branch | 94.3 % | 88.6 %, sliding in its last 20 minutes |

- **Moves drive the slide.** Under load the packets that neighbours send
  each other on a relay's channel make it look busier than another by
  more than the move threshold (spec "Busy estimate", spec "Channel moves").
  At twice the rate every relay moved about once in half an hour, ten
  times as often as at the rate, onto channels other moves had just
  filled. Each move leaves the relay's sleepy nodes on a channel nobody
  listens on until they follow, 6 to 9 % of their time against under 1 %
  at the rate: their beats go unheard, their mail expires at the holder,
  their own texts go to an anchor that is not there, and the retries and
  detours load the channels further. With moves off S4 carries twice the
  rate for two hours at the rate's delivery, with half the airtime, and
  its busiest relay at 7.5 % of its duty cycle (main `8704790`).
- **On the branch, commit `f3af05e`: a running node's home estimate counts
  no frame of the network,** only foreign energy. A move cannot leave the
  network's traffic behind; it can leave an interferer behind. In S4 it is
  the same as no moves, and relays still leave a channel an interferer
  appears on. It widens D24.
- **On the branch, commit `29adf20`: a delivery in a node's window needs
  the bucket for itself alone.** A holder at its cap let it into its send
  queue only when the bucket held it on top of everything queued, more
  than the bucket holds, so texts carried all the way expired at the
  holder with their node in reach. It is worth 24 points at three times
  the rate, 18 to 26 points in the loaded suite's mesh-burst ×64 and 4 to
  6 in its rings; it costs the overloaded corridor at 0.5 s a point, in
  texts still under way when the run ends.
- **At three times the rate the domain is past its capacity** however its
  relays behave, and what is left is what to drop. Waiting, the branch
  delivers about 51 %, steadily; dropping the oldest queued message when
  the regular room is full delivers 82 %, steadily, with two thirds of the
  airtime (85 % against main `8704790`). That reverses D31 and is not on
  the branch.
- **Wider channels** helped before D58 and hurt after it. On main
  `8704790` they ended the slide at twice the rate on their own and
  carried three times it at 90 % on top of the branch, by halving every
  frame; on main `4ccbaf4`, whose sleepy nodes' long beats crowd every
  channel, six channels instead of thirteen deliver 35 % at three times
  the rate against 51 %. They conflict with D43 either way.
- **Stopping reception of a frame for another node** after its first five
  bytes, if the radio allows it, is worth 8 points at three times the
  rate with moves off (one seed). A bench question.
- **Quiet slots around known heartbeats** could save at most 1.7 % of the
  relays' DATA attempts. Not built.

## What was run

S4 (`scenarios/s4-capacity.toml`, every model of D52 on) at the spec rate
(×1), twice it (×2) and three times it (×3), seeds 1 and 2, with two hours
of load after the settle time instead of one (`duration_s` 9900), since
one hour hides the slide. The strip (`scenarios/strip.toml`) at the spec
rate and half of it, one seed each, one settled hour as in the file.
"Delivered" counts reachable texts sent from the settle time to the end
less a delivery TTL, as in the earlier notes; "per ten minutes" bins them
by when they were sent. At most two S4 runs, or one strip, at a time, and
later one run of either; an S4 run at twice the rate took 23 minutes and
4.0 GB on main `8704790`, at three times 32 minutes.

| Name | Change | Where |
|---|---|---|
| main | none | `8704790`, `786b514` or `4ccbaf4`, as each table says |
| no moves | `move_threshold_percent = 100`: no home move after the boot choice | scenario only |
| foreign only | a running node's home estimate counts no frame of the network | commit `f3af05e` |
| deliveries first | a delivery in a node's window needs the bucket for itself alone | commit `29adf20` |
| both | the two commits: the branch | branch head |
| 250 kHz | 6 channels of 250 kHz from 433.175 MHz, the nominal noise floor 3 dB up, the table sizes derived from the frame time | scratch copy |
| discard | a receiver stops receiving a DATA addressed to another node once its first five bytes are in, and listens again | scratch copy of the simulator |
| TTL 300 | a message's time in the network is 300 s, half the delivery TTL | scratch copy of both |
| drop oldest | a packet to send on that the regular room refuses takes the place of the queued forward or return with the least time left, which is dropped | scratch copy of both |

Scratch copies are not on the branch; their measurements are kept here.

| Main `4ccbaf4`, ×2 | main | branch |
|---|---|---|
| Per ten minutes, seed 1 | 91 → 43 | 93 → 92 |
| Per ten minutes, seed 2 | 91 → 86 | 93 → 93 |
| Home moves after the boot choice, a run | 1 284, 1 271 | 0 |
| Sleepy nodes' time on a channel their anchor has left | 6.5, 6.0 % | 0 |
| Relays with under 1 s in the bucket, last ten minutes | 118, 43 of 269 | |
| Relays above 9 % of their duty cycle over the run | 52, 50 | 8, 11 |
| Expired at the holder with the node in reach | 7 745, 3 419 | 1 848, 1 956 |
| Latency p50, p90 | 19, 373 s; 14, 320 s | 3, 185 s |
| Airtime, s | 206 000, 195 000 | 147 000, 160 000 |
| ×3 expired on the way | 58.2, 62.2 % | 38.6, 39.8 % |

## Where the airtime goes as the domain slides

Main `8704790`, delivered per ten minutes of sending:

| | 2700 | 3300 | 3900 | 4500 | 5100 | 5700 | 6300 | 6900 | 7500 | 8100 | 8700 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| ×1 seed 1 | 92 | 92 | 91 | 90 | 91 | 91 | 90 | 90 | 91 | 91 | 91 |
| ×2 seed 1 | 84 | 83 | 81 | 78 | 72 | 75 | 67 | 45 | 42 | 37 | 36 |
| ×2 seed 2 | 83 | 74 | 64 | 47 | 37 | 34 | 26 | 26 | 25 | 24 | 27 |
| ×2 seed 1, main `786b514` | 85 | 88 | 87 | 86 | 85 | 86 | 86 | 86 | 73 | 54 | 54 |
| ×2 seed 2, main `786b514` | 87 | 83 | 79 | 54 | 32 | 25 | 21 | 24 | 27 | 25 | 23 |

| Main `8704790` | ×1 | ×2 |
|---|---|---|
| Delivered, seeds 1, 2 | 90.9, 91.0 % | 63.7, 42.5 % |
| Expired at the holder with the node in reach, texts | 1 066, 1 111 | 11 192, 15 192 |
| Expired on the way | 0.6, 0.5 % | 11.1, 25.9 % |
| DATA airtime per text, first and last ten minutes | 0.85 → 0.74 s | 1.42 → 2.77, 1.42 → 3.12 s |
| Of it on texts not delivered, last ten minutes | 14, 10 % | 67, 72 % |
| Relays' duty cycle, mean, busiest | 1.2, 3.4 % | 5.9 and 7.2, 9.9 % |
| Relays with under 1 s in the bucket, second hour | 0 | 100 to 150 of 269 |
| Home moves after the boot choice, a run | 150, 162 | 1 492, 1 660 |
| Sleepy nodes' time on a channel their anchor has left | 0.8, 0.9 % | 8.0, 8.7 % |
| Relays' DATA attempts lost: receiver free, receiving, deaf, on another channel | 22, 4, 3, 2 % | 16, 13, 9, 4 % |

At the spec rate nothing slides and almost every loss is a sleepy node
out of reach (spec "Delivery queue"). At twice the rate the texts that die
are not refused at the edge: they are carried to their holder and expire
there with their node in reach, or expire on the way, and by the end two
thirds of the airtime goes on them. Relays at the cap are the consequence,
not the cause: in the first half hour of load almost none is near it, and
the number grows with the slide (seed 2: 4 at 3000 s, 45 at 3600 s, 96 at
4800 s, 144 at 7200 s; of the first 45, 30 are among the 50 within 1.3 km
of the centre).

The moves start with the load, about 100 every ten minutes from 2400 s on
in every run at twice the rate, two an hour per relay, and stay there.
DATA lost to a receiver on another channel is a small share of it; the
larger cost is at the sleepy nodes. A sleeping node follows its anchor to
a new channel at the anchor's next heartbeat it wakes for
(spec "Relay choice", spec "Channel moves"); until then it beats where its anchor no longer
listens, so the holder misses its beats and its messages wait out the
TTL, and its own texts go to an anchor that is not there. D57, which has a
node that has followed beat soon on the new channel, shortens the second
half of that and softens the slide of seed 1 (79.1 % against 63.7 %); it
does not make the moves rarer, 1 309 and 1 626 a run.

## Idea 1: moves for foreign energy only

**What it is.** The home estimate decides when a relay moves
(spec "Busy estimate"). D24 left out of it the packets addressed to the node and the
ACKs of its own hand-overs, which would follow it to any channel. The
packets of others on its channel stayed in, as what would stay behind.
They do not stay behind for long: the channel a relay moves to carries
others' packets too, and under load every relay sees one channel busier
than another by more than the threshold, so they move in turn and fill
the channels they move to. Measured: once running, a node counts no frame
of the network at home, so a home reading is 1 only for energy that is not
one of its frames; booting, it counts every frame, so that its first
choice still spreads the relays over the channels. The sweep of the other
channels reads RSSI alone and is unchanged.

**What it changes.** S4 has no foreign energy, so no relay moves after its
boot choice: the run with the rule repeats the run with moves off to the
frame (×2, seed 1; seed 2 was stopped once that was seen).

| ×2, against main `8704790` | main | no moves | foreign only | both |
|---|---|---|---|---|
| Delivered, seeds 1, 2 | 63.7, 42.5 % | 89.9, 89.9 % | 89.9 % | 89.7, 89.8 % |
| Per ten minutes, seed 2 | 83 → 27 | 89 → 91 | | 89 → 90 |
| Airtime, s | 178 000, 225 000 | 86 800, 93 900 | 86 800 | 86 500, 94 100 |
| Relays' duty cycle, mean, busiest | 5.9 and 7.2, 9.9 % | 2.4 and 2.5, 7.5 % | | 2.4 and 2.5, 7.6 % |
| Expired at the holder with the node in reach | 11 192, 15 192 | 2 773, 2 808 | | 2 887, 2 832 |
| Sleepy nodes' radio current | 0.46, 0.50 mA | 0.40 mA | | 0.40 mA |

At the spec rate it delivers 91.1 and 90.8 % against 90.9 and 91.0 %,
with 3 % less airtime.

**Interferers.** S4 at the spec rate for two hours, without walkers (the
simulator places an interferer's reach once, so the two cannot be
combined), with seven interferers switched on at 3600 s in a settled
domain (`from_s`, built here and now on main from D59): five on channel 6 and two on channel
2, 10 dBm, on 2 s and off 5 s on average. Of the 72 relays within 600 m of
one, 7 were on its channel when it came on:

| | Relays within 600 m on the interferer's channel, at 3600 s → at the end | Moves after the boot choice | Delivered |
|---|---|---|---|
| main with deliveries first | 7 → 2 | 170 | 92.5 % |
| both | 7 → 2 | 5 | 92.8 % |
| no moves | 7 → 7 | 0 | 92.7 % |

The relays an interferer drove off its channel left it with the rule as
without it; the other 165 moves were load. The interferers cost S4 so
little that not moving costs nothing either: the scenario shows that the
rule still moves, not what moving is worth.

**What it does not show.** One seed with interferers. A relay whose
channel carries much more of the network's traffic than another, with no
interferer, now never moves: the boot choice, made on all energy, is the
only balancing, and a domain that grows after its relays booted is not
rebalanced; in the runs with interferers the relays per channel stay
between 15 and 27 either way. A frame of the network that fails its CRC
still counts as foreign; under load that is a share of the home estimate,
seen here only in the moves it makes, a few a run at ×3, none at ×2.

**Conflicts.** It widens D24 and keeps its reason; spec "Busy estimate"
changes its "Home channel" item and says why.

## Idea 2: deliveries first, in the bucket too

**What it is.** A delivery goes first in the send queue (spec "Delivery"),
but it entered the queue only when the bucket held it on top of everything
queued, as a packet to send on does (spec "Delivery queue"). A holder at
its cap holds more than its bucket in hop places, so the check failed for
as long as the node listened, and the message waited a beat period, two at
most before its TTL ran out. Measured: the delivery needs a place in the
regular room and its own frame and the reply reserve in the bucket; what is
queued does not count. A forward already taken may then wait for the
bucket, as it does after retries and replies (spec "Duty cycle cap").

**What it changes.** Against main `8704790`:

| | main | deliveries first | foreign only | both |
|---|---|---|---|---|
| ×1 delivered, seeds 1, 2 | 90.9, 91.0 % | 90.7, 91.0 % | 91.1, 90.8 % | 91.1, 90.9 % |
| ×2 delivered | 63.7, 42.5 % | 71.3, 53.6 % | 89.9, 89.9 % | 89.7, 89.8 % |
| ×2 expired at the holder with the node in reach | 11 192, 15 192 | 6 163, 6 020 | 2 773, 2 808 | 2 887, 2 832 |
| ×3 delivered | 9.5, 8.5 % | | 32.7, 27.6 % | 56.2, 52.4 % |
| ×3 expired at the holder with the node in reach | 21 465, 20 836 | | 25 028, 24 721 | 6 429, 5 917 |

With moves, it holds the second hour at 60 % instead of 36 % (seed 1) but
does not stop the slide. With the move rule nobody is at the cap at ×2 and
it changes nothing; at ×3 it is worth 24 points. The texts that still
expire at their holder with the node in reach, 2 800 a run at ×2 with
both, were not followed. Giving deliveries places of their own beyond the
regular room, so that a full regular room could not keep them out either,
delivered 69.3 % at ×2 against 71.3 % (seed 1, with moves) and was not
kept.

The loaded suite of D42, paired by seed, settled delivery:

| Scenario, seeds | main | deliveries first | difference |
|---|---|---|---|
| ring, 10 | 0.604 | 0.653 | +0.049 ± 0.034 |
| ring at 1 s, 30 | 0.532 | 0.574 | +0.042 ± 0.022 |
| ring at 2 s, 30 | 0.598 | 0.644 | +0.046 ± 0.018 |
| mesh-burst ×64, 30 | 0.648 | 0.909 | +0.260 ± 0.019 |
| corridor at 1 s, 30 | 0.916 | 0.918 | +0.002 ± 0.002 |

The rest of the standard and loaded suites (S1, S1b, S2, the corridor,
mesh-burst and mesh-spread, the corridor at 0.5 s) repeats main to the
frame. The move rule on top of it: within its spread everywhere, the
ring −0.014 ± 0.009, the ring at 1 s +0.026 ± 0.017. After the rebase the
branch against main `786b514`: the ring at 1 s +0.060 ± 0.026, at 2 s
+0.032 ± 0.022, mesh-burst ×64 +0.263 ± 0.019, the corridor at 0.5 s
−0.006 ± 0.008, the rest the same. Against main `4ccbaf4`: the ring
+0.048 ± 0.011, at 1 s +0.057 ± 0.009, at 2 s +0.052 ± 0.010, mesh-burst
×64 +0.182 ± 0.027, and the corridor at 0.5 s −0.013 ± 0.005, carried a
second 1.035 → 1.010; the delivery rule alone gives the corridor the same,
−0.012 ± 0.006. Its texts are not lost there: on every seed the
difference is texts still under way when the run ends, while the relay at
each end of the line delivers before it forwards. The rest repeats main
to the frame. No text was lost without a trace in any suite run.

**What it does not show.** It changes when a holder's forwards go, not
how many it takes: a holder that delivers more spends its bucket on the
last hop, and forwards wait longer behind it. That is the right order for
a message already carried all the way, but it was measured only as the
totals above.

## Idea 3: wider channels

**What it is.** Six channels of 250 kHz instead of thirteen of 125 kHz, as
`bandwidth-research-2026-10-16.md` built them: every frame takes half the
airtime, and sensitivity is 3 dB worse, which the simulator derives from
the bandwidth. D43 kept 125 kHz because the collapse then lay in the
channel, not the duty cycle, and wider channels did not move it.

**What it changes.** At twice the rate, on main `8704790`, it ends the
slide too: 90.3 % in both seeds, flat, the busiest relay at 3.2 to 3.4 %
of its duty cycle, a quarter of main's airtime (43 000 and 46 000 s
against 178 000 and 225 000). Relays still move, 840 and 1 050 times a
run, but the load that turns moves into a cascade is not there, and a
rotation of six channels brings a sleepy node after its anchor in half the
time. On top of both rules at three times the rate it delivers 89.9 and
89.6 %, flat, the busiest relay at 5.4 %.

**After D58 it goes the other way.** On main `4ccbaf4`, on top of both
rules at three times the rate, it delivers 35.2 % against 51.5 % (seed 1),
and makes 4.7 million relay DATA attempts against 1.9 million, a fifth
of them lost to a receiver busy with another frame against 15 %. D58 has
every sleepy node beat with the long preamble on the channels of the
rotation; half as many channels carry twice those beats each, so the
channel binds before the duty cycle. Not followed further.

**What it costs.** 3 dB on every link. In S4 that shows only at the edges:
anchor changes +20 %, spells without an anchor 679 against 478 a run. The
indoor wall loss, on which this turns, is not measured on our hardware
(Phase 2 bench), and S4's 9.6 relays a square kilometre hide a shorter
reach. Six channels leave the move rule five alternatives and cannot all
avoid 433.92 MHz.

**Conflicts.** D43. Its reason for waiting, until the collapse fix had set
where the cliff lies, now reads the other way: past the move cascade the
cliff is load, and halving every frame halves the load.

## Idea 4: stop receiving a frame addressed to another node

**What it is.** A relay that locks onto a DATA for another node on its
channel is deaf to its own frames until that one ends, 69 ms for a text.
`dst_hop` is in its first five bytes (spec "Packet formats"); a radio that
let the firmware read them during reception, stop, and listen again would
be free 64 ms sooner. The scratch simulator does this: a lock on a DATA
addressed to another node ends once a five-byte frame would have ended,
and a retune's time later the receiver listens again. Whether the SX1262
lets the buffer be read before the packet ends is not documented; it is a
bench question.

**What it changes.** At twice the rate on main it ends the slide too, 90.3
and 90.4 %, but mostly through the moves: relays moved 21 and 12 times a
run, since a dropped frame no longer counted toward the busy estimate.
With moves off it delivers 90.4 and 90.2 %, the same as moves off alone,
and receivers busy with another frame lose 4.6 % of the relays' attempts
instead of 6.8 %. At three times the rate with moves off it delivers
40.4 % against 32.7 % (one seed).

**What it does not show.** Whether the radio can do it. The firmware
would also have to count the rest of a dropped frame as busy time, which
it knows from the header; the scratch copy did not.

## Idea 5: quiet slots around known heartbeats

**What it is.** Since D47 every node knows when each neighbour's next
heartbeat comes and on which channel, so a sender could hold a DATA that
would overlap one at the receiver.

**What it would change.** The simulator counts the relays' DATA attempts
that were lost while a relay's heartbeat was on the air at the receiver:
at twice the rate on main, 26 500 of 1 532 000, 1.7 %. That bounds what
such slots could save, since a sender knows only its own neighbours'
heartbeats and some of these attempts would have been lost anyway. A
sleepy node's beat was on the air during another 2.8 % of all attempts
that were lost, but beats come at random times nobody knows (spec
"Beats"). Not built: the bound is below the spread between seeds. D60 built the
slots and measured the same: no gain.

## Idea 6: dropping, at three times the rate

The pushback rules never drop for want of room (D31): a message waits, in a
hop place if need be, until its time runs out (D44). Past the domain's
capacity that means relays spend their airtime on messages that will expire
before they arrive. Two ways of dropping were measured on top of both
rules at three times the rate, where the domain is past its capacity
whatever the relays do (relays at 7 % of their duty cycle on average):

- **TTL 300:** a message's time in the network is 300 s; the delivery queue
  keeps its own 600 s.
- **Drop oldest:** when the regular room cannot take a packet to send on,
  the queued forward or return with the least time left goes, dropped
  (traced as `SendQueueFull`), if it has less left than the newcomer; the
  newcomer then takes the room or a hop place as before. A delivery, the
  hand-over under way and the node's own application's message are never
  dropped.

| ×3, both, before the rebase | waiting | TTL 300 | drop oldest |
|---|---|---|---|
| Delivered, seeds 1, 2 | 56.2, 52.4 % | 76.0, 64.7 % | 85.5, 85.3 % |
| Per ten minutes, seed 2 | 70 → 49 | 74 → 59 | 85 → 84 |
| Expired on the way | 32.0, 36.5 % | 12.3, 24.3 % | 0.6, 0.6 % |
| Dropped for room | 0 | 0 | 3.0, 3.1 % |
| Airtime, s | 261 000, 262 000 | 170 000, 201 000 | 133 000, 144 000 |
| Relays' duty cycle, mean | 7.0 % | 5.2 % | 4.0 % |
| Of the DATA airtime, on texts not delivered | 45, 50 % | 21 to 40 % | 16 to 25 % |
| Latency p50, p90, s | 253, 591 | 176, 436 | 154, 401 |

On main `4ccbaf4` (seed 1) it delivers 82.4 % against 51.5 % with
waiting, flat at 81 to 84 % every ten minutes, dropping 9.6 % of the texts
for room and letting 1.0 % expire on the way against 38.6 %, with 208 000
s of airtime against 316 000 and a median latency of 7 s against 199 s.

Dropping the oldest when the room is full carries three times the rate at
85 %, steadily, against 52 to 57 % with waiting, and everything else
moves the right way: half the airtime, a third less latency, almost
nothing expires on the way. The 3 % it drops are messages near the end of
their time that would have spent more hops and then expired; in their
place messages with time left get through. A shorter TTL does part of the
same blindly, to every message whether its relay is full or not. So, to
the question asked: a steady 85 % is better than both main's swing
between 35 and 84 % and a steady 55 % with waiting.

**What it costs.** It reverses D31 ("overload never drops a message for
want of room") for messages already in the network, and with it the
argument of spec "Overload waits": hop places still exist, but a packet
can now take the room of an older one. A dropped message is dropped
silently, as every drop is (D10). It was not run on the suites, nor below
three times the rate, where with both rules the room is rarely full.
Which message to drop, and whether a relay should drop before its room is
full, were not explored.

## The strip

Three S4 domains side by side with a centre of twice the people in the
middle one and texts to anyone in them (`scenarios/strip.toml`), one seed,
one settled hour, against main `4ccbaf4`:

| | main, half the rate | branch, half the rate | main, the rate | branch, the rate |
|---|---|---|---|---|
| Delivered | 94.0 % | 94.3 % | 24.6 % | 88.6 % |
| Per ten minutes | 94 → 94 | 94 → 95 | 29 → 22 | 91 → 84 |
| Within one domain, across one border, across two | | | 38, 14, 28 % | 88, 87, 93 % |
| Middle domain's relays: duty cycle mean, time at the cap | | | 9.4 %, 54 % | 5.7 %, 11 % |
| Home moves after the boot choice | 500 | 0 | 2 200 | 10 |
| Expired on the way | 0.5 % | 0.4 % | 57.5 % | 4.0 % |
| Airtime, s | 115 000 | 114 000 | 332 000 | 227 000 |

Against main `786b514` before D58 the branch delivered 89.9 % at the rate,
flat, against 25.5 %, and 91.5 against 90.8 % at half of it.

At the rate the branch carries the strip that main loses, but not
comfortably: its middle domain's relays are at their cap a tenth of the
time and 59 of them above 9 % of their duty cycle, and the last twenty
minutes slide from 90 to 84 %. That is the hot spot of the question, now
truly at its cap: its own texts, the doubled centre and the transit
between the ends. Dropping the oldest, which carries S4 at three times the
rate at 82 %, was not run on the strip.

## What this does not show

- The rate at which the moves start a cascade depends on the move
  threshold, dwell and wait of spec "Channel moves", none of which was
  varied; the rule kept removes the dependence rather than tuning it.
- One strip per rate and variant; one seed with interferers; one seed of
  discard at three times the rate; the rule alone at twice the rate on one
  seed (the other seed was the no-moves run, to which it is identical).
- Interferers that come and go, or move, were not modelled; nor a domain
  that grows after its relays booted, where the rule would not rebalance.
- Dropping and wider channels were built as scratch copies on top of the
  branch, not as commits; drop oldest was not run on the suites.
- Five of 124 000 texts in a run at three times the rate had the ID of an
  earlier one (commit `c43abc5` counts them); one of them showed as a text
  lost without a trace. No other text was lost without a trace.
- The strip is three domains; the city has fourteen. On main `4ccbaf4`
  the branch's strip at the rate is still sliding at the end of its hour.
- Wider channels and drop oldest were run on main `4ccbaf4` on one seed;
  why wider channels lose there was not followed.

## Recommendation

**Understood.** The slide at twice the rate, and the strip's collapse at
the rate, are not the duty cap of a hot spot shedding load badly. They are
the move rule reacting to the network's own load: relays move every half
hour, each move strands sleepy nodes, and the failures it causes raise the
load that makes them move. With that removed the hot spot is not at its
cap at twice the rate. The holder's deliveries starving behind its own
queue is a second, smaller flaw that matters once a holder is at its cap.
Past the capacity, at three times the rate, waiting carries a steady
51 % (55 % before D58); the airtime goes on messages that expire on the
way. On main `4ccbaf4` the strip's middle domain is at its cap again at
the spec rate with both rules, so the edge is near there too.

**Fixed, on the branch, proposed:**

1. A running node's home estimate counts no frame of the network (commit
   `f3af05e`, spec "Busy estimate"). It widens D24.
2. A delivery needs the bucket for itself alone (commit `29adf20`, spec
   "Delivery queue").

Against main `4ccbaf4` they carry S4 at twice the rate for two hours at
93.0 and 92.9 % (main: 71.4 and 89.1 %, seed 1 sliding to 43 %), three
times it at 51 % (main: 12 to 15 %), and the strip at the rate at 88.6 %
(main: 24.6 %); at the spec rate S4 delivers 94.0 and 93.7 % against 93.9
and 93.8 %. In the suites they lift the rings by 5 to 6 points and
mesh-burst ×64 by 18, and cost the overloaded corridor at 0.5 s 1.3
points, all of it texts still under way at the end of the run.

**Measured, not on the branch, for the user to decide:**

3. Drop the oldest queued message when the regular room is full: 82 %
   at three times the rate against 51 % (85 against 55 % before D58). It
   conflicts with D31. It is the measured answer to what the network
   should do at the edge of its capacity, and the next thing to try on
   the strip.
4. Wider channels: not as the network now is. Before D58 they carried
   three times the rate at 90 %; after it, 35 % against 51 %. They
   conflict with D43 and would need the bench's wall loss as well.
5. Stopping reception of frames for others: a bench question first.

Not recommended: quiet slots around heartbeats (at most 1.7 % of attempts)
and a shorter TTL (drop oldest does the same where it is needed).

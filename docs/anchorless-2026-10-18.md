# Anchorless mode: duty-cycled receive and the long preamble

Research note, 2026-10-18, roadmap D51. Measured against main `494be01`
(D50).

## Summary

Until now a sleepy node that heard no relay listened all the time and
acted as a relay, at 5.8 mA. Here it stays a relay of a place without
relays, holding and forwarding with the ordinary time to live, but its
radio receives duty-cycled between its beats, and every frame to it
carries a long preamble that a duty-cycled receiver always catches. The
two heartbeat bits get one meaning each (spec "Packet formats"):

- **Sleepy**: reach me only in my window after my beat. That its anchor
  is who sends follows from its address, which points there.
- **Anchorless**: I am sleepy and know no node that is not; I send with
  long preambles and can be reached at any time with one; if you are like
  me, send my mail to me directly; if you are not sleepy, wake me with a
  long preamble so that I can leave the mode.

The numbers follow from the SX1262 datasheet: a 6 ms receive window every
361.1 ms, a preamble of 1396 symbols (357.4 ms), 0.194 mA while receiving
duty-cycled. A node in the mode beats once a minute with the long
preamble on the next channel each time; a relay that hears such a beat
answers on the node's channel with the long preamble, at once and a fade
later.

Measured: sixty sleepy nodes over a square kilometre with no relay
deliver every text, as listening did, at **0.59 mA a node instead of
5.85 mA**, with six times the airtime and half a second of latency at the
median. Two relays switched on at the edge of such a village bring its
nodes in 215 s after they are switched on at the median (their own 195 s
boot listen included), 686 s at the 90th percentile. In the capacity
domain, where everything boots together, one sleepy node in thirty hears
no relay during its boot listen and spends seven minutes at the median in
the mode; delivery is **1.3 points lower** (92.6 → 91.3 %) and airtime half
again higher, and S2 loses 2.0 ± 0.8 points for the same reason. The
other suites move within their spread. Removing a tenth of the relays
changes nothing for their orphans: a check that hears no relay keeps the
anchor, as before.

## The two conditions on the duty cycle

The SX1262's SetRxDutyCycle (datasheet §13.1.7) receives for `rx`, saves
its state (about 0.5 ms, Figure 13-2), sleeps for `sleep`, restores its
state (0.5 ms) and starts its TCXO (Figure 13-3 note: the start-up delay
is added between the sleep and the receive; 5 ms as `bark-fw` programs it,
an assumption with margin), and receives again. So the cycle is
`C = rx + sleep + wake`, `wake` = 6 ms.

- **Caught.** A receive window detects a preamble once `D` of it is inside
  the window, `D` = 4 symbols (1.024 ms), the time spec "Busy estimate"
  takes for the modem to lock. A preamble starting just too late for one
  window must still hold `D` in the next: `L ≥ C − rx + 2D = sleep + wake
  + 2D`.
- **Kept.** After it detects a preamble the radio waits `2 rx + sleep` for
  the header, then sleeps again (§13.1.7: "Tpreamble + Theader ≤ 2 *
  rxPeriod + sleepPeriod"). The header ends 6.25 + 8 = 14.25 symbols
  (3.648 ms) after the preamble (§6.1.4).

With `L` at its least the two give `rx ≥ (wake + 2D + header) / 2`
whatever the sleep, 5.848 ms; half a symbol more lets the preamble be
rounded up to whole symbols, 5.976 ms, and the period steps of 15.625 µs
that are whole microseconds are 125 µs: **`rx` = 6.000 ms**, 384 steps.

## The cycle, the preamble and the currents

A node in the mode pays for its windows, `I_rx (rx + wake) / C`, and for
its own beats, a long preamble of about `C` once a beat period `P`:
`I_tx C / P`. With `I_rx` = 5.8 mA, the listening figure of the spec and
every note (the datasheet's boosted receive is 5.3 mA, Table 3-5, and the
TCXO adds an unmeasured part), `I_tx` = 32 mA (Table 3-6, the least listed
for the SX1262 at 434 MHz, +14 dBm; at our +10 dBm less) and `P` = 60 s,
the sum is least at `C = √(I_rx (rx + wake) P / I_tx)` = 361.2 ms, where
the two are equal. It is flat: within 7 % from 250 to 520 ms, so the
working figure of 250 ms was close.

| Quantity | Value | Source |
|---|---|---|
| Receive window | 6.000 ms (384 steps) | the two conditions |
| Sleep | 349.125 ms (22 344 steps) | `C` less window and wake, in whole steps |
| Cycle | 361.125 ms | |
| Long preamble | 1396 symbols, 357.4 ms | sleep + wake + 2D, rounded up |
| Kept condition | 361.024 ≤ 361.125 ms | checked in `sniff.rs` |
| Mean receive current | 0.194 mA | windows and wakes at 5.8 mA, sleep at 1.2 µA (Table 3-5) |
| Own beats, once a minute | 0.195 mA | 366.1 ms at 32 mA every 60 s |
| Heartbeat, long preamble | 366.1 ms | datasheet §6.1.4 formula |
| 255-byte frame, long preamble | 492.9 ms | under the 1 s per transmission of spec "Regulatory limits" |

A short frame is caught by a duty-cycled receiver only if it starts within
`rx + 3.07 ms − 2D` of a window, 1.9 % of the cycle; the simulator checks
this (`bark-sim/tests/radio.rs`).

If the bench finds the TCXO starts in 1.5 ms (roadmap, bench table), the
wake is 2.5 ms, the window 4.1 ms, the optimal cycle 270 ms and the two
costs 0.29 mA together instead of 0.39.

**Airtime and the regulation.** A node in the mode spends 60 × 366 ms =
22 s an hour on its beats, 6 % of the 360 s an hour the duty cycle allows
(ERC/REC 70-03 band g1, NCEC row РІ 50-3-1). A relay's answer is two long
heartbeats, 0.73 s, per anchorless beat it hears. A DATA to a node in the
mode is 357 ms longer than the same DATA to anyone else. The ordinary
traffic keeps the short preamble: a long frame takes 30 times a
heartbeat's airtime, and a relay's 10 % runs out long before its channel
fills.

**The beat period.** Nothing in the radio fixes it; it is how long a node
may stay unheard: a relay on one channel hears a beat once a rotation of
13 beats. With the cycle chosen for each period, the two costs go as one
over its square root: 0.45 mA at 46 s, the period at which a rotation fits
in a delivery TTL; 0.39 mA at 60 s, a rotation of 13 minutes; 0.17 mA at
300 s, a sleepy node's period, with a rotation of an hour. The user's 60 s
is kept.

**Expiry.** A beat comes at a random time in the second half of each
period, so beats on one channel come at most 13.5 periods apart; two
rotations and a heartbeat's lag are 26.5 × 60 s + 0.99 s = 1591 s. One
rotation, as D49 had for awake anchorless nodes, would drop a live
neighbour with each lost beat, one time in ten at a listening receiver,
for 13 minutes, where it may be the only way on.

**Answers.** A relay answers each anchorless beat it hears with a
heartbeat on the channel the beat names, with the long preamble, at once
and a fade later, 60 s, while two beat periods last: two answers, so a
fade on the first does not cost a rotation.

## What was built

- `bark-proto/src/sniff.rs`: the constants above, derived in const
  functions and checked by tests; `detects`, whether a duty-cycled receiver
  detects a preamble starting at a given time.
- `Action::Sniff(until)`, core-interface §3 rule 11. Rule 4: frames carry
  the long preamble to a node in the mode. Rule 8: a check is held for at
  most a long full frame, 492.9 ms; at 138.6 ms, the relays' sweep cut
  every long frame short, and no relay ever heard an anchorless beat.
- The core: `Relay::anchorless()`; anchorless beats on the rotating
  channel with the long preamble, once a minute; the sniff between them;
  DATA to an anchorless neighbour with the long preamble, readdressed
  before each attempt; the airtime of a node's own DATA counted with the
  long preamble in admission; relay answers (`Beat::Answer`, one byte in
  each neighbour entry); anchorless entries expiring after two rotations.
  A retry after an unanswered long frame waits longer by as much as the
  frame is; a heartbeat may lag behind its slot by a long full frame, so
  a sleeping node's watch for its anchor grows from 670 to 1 025 ms. A
  sleepy node listens two rotations at boot.
- The simulator: duty-cycled reception from `sniff::detects`, the cycle
  starting again after each frame taken; time receiving duty-cycled,
  time receiving a caught frame, and the radio's current per sleepy node
  (5.8 mA listening, 0.194 mA duty-cycled, 32 mA transmitting); the
  busiest node's duty cycle after the settle time; spells without an
  anchor; airtime of anchorless beats, answers and long DATA; scripted
  `start` and `sniff` events; an anchorless node's address is its own
  position for the traffic model.
- `bark-driver` ignores `Sniff` as it ignores `Sleep`.

Tests: `sniff.rs` (the numbers, both conditions, catching at every phase,
short preambles), `answers_follow_each_anchorless_beat` and
`anchorless_entries_expire_after_two_rotations` (`neighbours.rs`),
`sleepy_nodes_alone_meet_and_route` and
`a_relay_answers_an_anchorless_beat_and_brings_the_node_in`
(`tests/sleepy.rs`),
`a_sniffing_receiver_takes_long_preambles_and_misses_most_short_ones`
(`bark-sim/tests/radio.rs`).

## What was run

Simulators: **M**, main `494be01`; **A**, this branch. For the edge
scenario M needs the scripted start and the own-position address of a node
without an anchor, both backported to it (**MS**). The village is the D46
one: 60 sleepy nodes outdoors over a square kilometre, no relay, the spec
"Capacity and sizing" rate, 4500 s, seeds 1 to 10. S4
(`scenarios/s4-capacity.toml`) at the same rate, seeds 1 and 2, and with
every tenth relay (27) removed at 1800 s, seeds 1 and 2. The edge
scenario: the village beside a district of 2 km² 750 m east of it, with 19
windowsill relays and 160 indoor sleepy nodes, and two relays switched
on at 1800 s at the village's east edge, seeds 1 to 10. The standard and
loaded suites of D42 with the seeds of the D50 note. Delivered counts
reachable texts originated from the settle time to the end less a
delivery TTL. Current is the radio's alone. No more than four S4 runs at
once.

## Results

**The village.**

| | M | A |
|---|---|---|
| Delivered | 100 % | 100 % |
| Latency p50, p90 | 0.07, 0.2 s | 0.5, 1.1 s |
| Radio current per node | 5.85 mA | 0.59 mA: listening 0.14, duty-cycled 0.19, transmitting 0.26 |
| Airtime a run | 344 s | 2 154 s (beats 1 961 s) |
| Busiest node's duty cycle | 0.2 % | 1.0 % |

Listening, 86 s an hour, is the time a node takes the long frames that
come to its channel, its own hand-overs and their ACKs. Messages sent
before the settle time lose a few to the owner rule: until a node has
heard its neighbours, a rotation of 13 minutes, the one nearest a
destination may take it for its own.

**S4 at the spec "Capacity and sizing" rate.**

| | M | A |
|---|---|---|
| Delivered, seeds 1, 2 | 92.5, 92.6 % | 91.9, 90.6 % |
| Latency p50, p90 | 136, 338 s | 144, 371 s |
| Airtime | 24 573 s | 37 994 s |
| Busiest node's duty cycle | 3.3 % | 5.9 % |
| Sleepy node's radio current, mean | 0.350 mA | 0.376 mA |
| Listening an hour | 214 s | 230 s |
| Never anchored | 1, 4 | 20, 21 |
| Spells without an anchor | – | 645 a run, 407 and 435 s at the median |

The loss is the price of the boot: everything starts at once, a relay
heartbeats only after its 195 s boot listen, and a sleepy node that has
not heard one of them in its 390 s goes into the mode, about 650 of 20 000.
Each waits for a relay on the channel of one of its beats, seven minutes at
the median, and meanwhile messages go to and from it with long frames:
anchorless beats 3 921 s and long DATA 3 467 s of the 13 000 s more
airtime, hop-cap drops 64 against 0. On main it would have heard the next
heartbeat while listening.

**S4 with a tenth of the relays removed at 1800 s.**

| | M | A |
|---|---|---|
| Delivered | 86.9, 84.2 % | 84.1, 83.7 % |
| Orphans heard by a new anchor | 1 974 / 2 019, 2 094 / 2 175 | 1 992 / 2 053, 2 109 / 2 177 |
| ... after, p50 and p90 | 480 and 1 140 s, 540 and 1 320 s | 540 and 1 320 s, 480 and 1 260 s |
| Their mail delivered | 638 / 1 002, 632 / 1 121 | 650 / 1 049, 653 / 1 109 |
| Sleepy node's radio current | 0.467 mA | 0.510 mA |

The orphans go as on main: a check that hears no relay keeps the anchor
(spec "Relay choice"), so no orphan enters the mode.

**The edge.** Ten seeds, sums of reachable texts in brackets:

| | MS | A |
|---|---|---|
| Delivered, all | 86.2 % (1 686) | 86.2 % (1 688) |
| village to village | 58.6 % (102) | 66.4 % (101) |
| between the village and the district | 76.7 % (653) | 75.5 % (656) |
| within the district | 94.9 % (931) | 95.1 % (931) |
| Village nodes anchored at the end | 599 / 600 | 593 / 600 |
| ... after the relays were switched on, p50, p90 | – | 215 s, 686 s |
| Village node's radio current | 1.00 mA | 0.93 mA |
| Airtime | 1 053 s | 2 031 s |
| DATA to a node asleep | 2 | 3 870 |

How long an answer takes to bring a node in: the relays hear beats during
their boot listen and answer as soon as they run, so the nodes heard then
come in within seconds of 1 995 s; the rest wait for a beat on a relay's
channel, at most a rotation, 13 minutes, which with two relays on two
channels is half that on average. Main's village listens until 1 800 s
and anchors at the first heartbeat it hears after the relays boot.
The DATA to sleeping nodes are long frames from nodes in the mode to
neighbours that have since anchored, whose entries live 26 minutes.

**Suites.** Delivered M, A (paired difference ± one standard error),
latency at the median, airtime a run, listening an hour:

| Scenario | Delivered | Latency | Airtime | Listening |
|---|---|---|---|---|
| S1 | 100, 100 % | 115, 107 s | 15, 15 s | 143, 174 s |
| S1b | 100, 100 % | 110, 109 s | 34, 34 s | 222, 209 s |
| S2 | 91.1, 89.1 % (−2.0 ± 0.8) | 129, 124 s | 78, 108 s | 278, 257 s |
| Corridor | 99.8, 99.8 % | 107, 109 s | 48, 48 s | 249, 249 s |
| Ring | 82.5, 77.6 % (−4.9 ± 3.7) | 321, 314 s | 1 108, 1 103 s | 390, 358 s |
| Mesh, burst | 100, 100 % | 109, 99 s | 145, 151 s | 79, 78 s |
| Mesh, spread | 99.2, 99.7 % (+0.5 ± 0.4) | 109, 116 s | 141, 140 s | 87, 86 s |
| Corridor at 1 s | 97.8, 98.2 % (+0.4 ± 0.6) | 105, 100 s | 675, 681 s | 1 182, 1 227 s |
| Corridor at 0.5 s | 98.1, 97.2 % (−0.9 ± 0.8) | 107, 116 s | 744, 724 s | 1 338, 1 330 s |
| Ring at 1 s | 71.8, 72.9 % (+1.1 ± 3.2) | 362, 355 s | 1 111, 1 136 s | 505, 499 s |
| Ring at 2 s | 78.4, 76.1 % (−2.3 ± 2.7) | 314, 327 s | 1 092, 1 095 s | 483, 475 s |
| Mesh, burst ×64 | 74.4, 74.9 % (+0.5 ± 1.3) | 143, 139 s | 4 583, 4 550 s | 227, 230 s |

S2 loses for the reason S4 does: about four of its 80 sleepy nodes a run
hear no relay in their boot listen, some of them dark then. The rings at
their cap jam on some seeds depending on when their flows start (D49
note); their differences are within twice their spread.

## Paths measured and not taken

Each was built, measured on the edge (ten seeds) and S4 seed 1, and
removed:

| Variant | Edge delivered | S4 delivered | S4 airtime |
|---|---|---|---|
| A (this branch) | 86.2 % | 91.9 % | 35 098 s |
| A, and a check that hears frames but no relay leaves the anchor | 85.6 % | 90.1 % | 54 455 s |
| that, and a node that takes an anchor beats once, sleepy and long, on the channel of each anchorless neighbour | 85.5 % | – | – |
| that, on all 13 channels | 85.1 % | 84.7 % | 171 111 s |
| A with the 13-channel beats | 85.5 % | 90.9 % | 129 316 s |
| A, anchorless neighbours after all others in next-hop order | 87.1 % | 91.6 % | 34 385 s |

Leaving the anchor after a check that hears only sleepy nodes fits the
anchorless bit's meaning, but nodes at the edge of a relay's reach then
swing between the two modes, and each swing leaves stale entries and
long frames. The beats that tell neighbours a node now sleeps halve the
DATA sent to sleeping nodes but cost more airtime than they save. Putting
anchorless neighbours last changes nothing measurable.

## What this does not show

- No board has received duty-cycled yet. The detection time `D`, the
  TCXO start and the currents are the datasheet's and the spec's
  assumptions; the bench table's current row and a long frame sent 100
  times to a board in the mode (spec "Implementation notes", step 5) test
  them.
- The simulator detects a preamble once a window holds four symbols of
  it, locks then as if from the start, and assumes the header comes in
  time, which the derivation guarantees; it does not model the radio
  cutting a reception short at `2 rx + sleep`.
- A relay's answer is a heartbeat outside its slot; a sleeping node that
  takes it for its anchor's may check its anchor once. Not counted.
- The window after a beat (D50) still allows for an attempt with the short
  preamble under way at the holder; one with the long preamble is 354 ms
  longer.
- Nodes do not move and clocks do not drift, as in the earlier notes.
- Two seeds of S4.

## Firmware

`bark-driver` ignores `Action::Sniff`, as it ignores `Action::Sleep`, and
the firmware keeps listening; the image grows to 308 KiB of flash
(`bark-fw/README.md`). To receive duty-cycled it needs: the receiver's
packet parameters programmed with the long preamble, so that a long frame
is received at all (datasheet §6.1.4: program the longest when it varies);
`SetRxDutyCycle(384, 22 344)` for `Sniff`, and `SetStandby` before any
other command; the Rx boosted register written again after each wake
(datasheet §9.6); and the driver's hold of a check for a reception already
allows a long frame (`MAX_AIRTIME`). Transmit already takes the frame's
preamble.

## Recommendation

Adopt it where it matters, and decide on the boot. Anchorless mode does
what it was for: a place without relays routes among itself at a tenth
of the current, 0.59 against 5.85 mA a node, and a relay that arrives
brings its nodes in within minutes. Where relays are, it costs: a node
that misses its relays at boot waits minutes in the mode instead of
seconds, and the capacity domain carries 1.3 points less and S2 2.0. That
cost is the simulator's power-on of every node at once; a sleepy node switched
on among running relays hears one within a rotation. If the user wants
none of it, the cheapest fix is a longer boot listen for sleepy nodes,
not measured here. The two heartbeat bits now say one thing each.

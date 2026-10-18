# Every sleepy node receives duty-cycled and is a last resort: built and measured

Research note, 2026-10-18, roadmap D58. Spec references are to `protocol.md`.
Measured against main as it was before D58 in code (`878dbdc`; main has
since moved to `14dc0ba` with spec commits only): the D58 spec with the old
behaviour in the code. Both simulators carry the same first commit, which
counts the DATA sent to a sleepy node that cannot hear it.

## Summary

The branch as adopted, D58 with the amendment below and the holder's
delivery outside the window, against the old code: S4 delivers 93.77 %
against 90.75 % (+3.02 points), at 1.6 s median latency against 137 s, a
sleepy node draws 0.638 mA against 0.399, and all airtime is 55 291 s
against 33 011. The rest of this summary is D58 alone, before the
amendment.

- **The pocket is fixed.** DATA sent into sleepy nodes that cannot hear it
  falls from 412 to 1 a run in the village beside a district, and from 741
  to 15 in S4; what is left is a holder's DATA at the very end of a
  window.
- **The current is as derived:** a sleepy node draws 0.194 mA more in S4,
  0.593 mA against 0.399, of which receiving duty-cycled is 0.180 mA
  (derived: 0.194 mA for a node that does nothing else).
- **The long beats to anchorless neighbours cost far more airtime than
  anything else in D58.** In S4 they are 26 319 and 25 930 s a run, 44 % of
  all airtime, which rises from 33 011 to 59 454 s. A quarter of S4's
  sleepy nodes, 4 570 and 4 724 of 20 000, send them at some time: each has
  caught the long beat of a node in anchorless mode on its own channel and
  keeps an anchorless entry for 1 201 s. Delivery does not suffer, +0.40 ±
  0.25 points in S4; this is airtime the channels carry.
- **The village of sixty with no relay** delivers every text as before; its
  shorter anchorless beat period, 46.2 s against 60 s, costs 0.113 mA more
  a node and 609 s more airtime a run, as derived.
- **The D42 suites deliver as main within their spread**, S2 better by
  1.4 ± 0.5 points, but for the corridor at 0.5 s, −1.4 ± 0.7, where a few
  seeds lose texts at an end relay that misses its neighbour's heartbeats
  through the boot, as main does on other seeds.
- **The sleepy firmware image needs 18.6 KiB less RAM**, 152 KiB instead of
  170 KiB, and 22 KiB less flash.

## What was built

| Commit | Story |
|---|---|
| `ce4975c` | The simulator counts DATA sent to a sleepy node that cannot hear it: asleep (the old code) or receiving duty-cycled with a short preamble. |
| `5c17fc8` | The D58 rules in the core, the driver, the simulator and the tests. |
| `27257e8` | Comments that still said duty-cycled receive was for a node without an anchor alone. |
| `bcc7ed9` | The firmware README's sizes. |
| `7fa31c2` | The spec where the implementation found it silent (below). |
| `d0aaa77` | The four points of the user's review of the spec (below). |
| `e3e7c55` | The spec's derived current of the anchorless beats, 0.34 mA, not 0.33. |
| `06848d6` | This note and the D58 row. |
| `f254460` | The amendment: long beats only to a neighbour that stays anchorless a rotation (below). |
| `de02bec` | The holder's delivery outside the window, its own decision (last section). |

In the core:

- The heartbeat codes are 0x8 relay, 0x9 sleepy with an anchor and 0xB
  sleepy without one; 0xA is rejected like any unknown type.
- `Action::Sleep` and `Relay::sleeps()` are gone. A sleepy node asks
  `Action::Sniff` whenever it has nothing under way and is not in a window
  after its beat or a message it took, nor listening for its anchor's
  heartbeat, nor listening to choose or check its anchor. A node without an
  anchor has no window, so it sniffs after a message it took as well.
- Every heartbeat fills the neighbour table, a sleepy one marked sleepy. A
  node that is not sleepy puts the beat of a sleepy node with an anchor in
  its heard table too. Entries of sleepy nodes with an anchor expire after
  two beat periods, 600 s; anchorless ones after 26 anchorless beat periods
  and a heartbeat's lag, 1 201 s.
- A sleepy neighbour with an anchor is a next hop only where the search
  would otherwise end here: no other neighbour live, untried and not the
  parent that is a candidate or only banned. One that listen-before-talk
  gave up on, set aside and not banned, does not hold it back. Every DATA
  to a sleepy neighbour carries the long preamble.
- A sleepy node owns only messages addressed to its ID, so it forwards
  everything else at once, and a sleepy neighbour nearer a destination does
  not keep a relay from owning it.
- A sleepy node with an anchor whose table holds anchorless neighbours
  sends each of its beats on each channel they listen on as well, with the
  long preamble, just before the beat itself; where one listens on its
  home, the beat there has the long preamble.
- The anchorless beat period is the delivery TTL over the 13 channels,
  46.2 s. The duty cycle stays set for the 60 s it was derived with, as
  the spec says.
- `Node<H, Q>` is the one node, with room for `H` sleepy nodes heard and
  `Q` messages for them. `Relay` is `Node<256, 41>`, 166 832 B, and
  `SleepyNode` is `Node<0, 0>`, 147 816 B; the firmware's sleepy build uses
  it. A `Relay` set up sleepy keeps its tables empty and sends exactly what
  a `SleepyNode` sends (a unit test).

Each rule has a test that fails without it, checked by undoing the rule:
`a_sleepy_node_is_a_last_resort`, `a_pocket_learns_its_way_out_from_long_beats`,
`a_sleepy_node_receives_duty_cycled_between_its_windows`
(`tests/sleepy.rs`); `a_sleepy_node_keeps_no_heard_table`,
`a_sleepy_node_owns_no_other_node_s_message`,
`a_sleepy_node_without_the_mailbox_is_a_sleepy_relay`
(`node/relay.rs`); `sleepy_entries_expire_after_two_beat_periods`,
`a_sleepy_newcomer_takes_only_a_free_slot`,
`anchorless_entries_expire_after_two_rotations` (`tables/neighbours.rs`);
the heartbeat codes in `wire.rs`; the cycle's cost at the mean anchorless
interval in `sniff.rs`.

## Where the spec was silent

The spec now says each of these ("Owner", "Neighbour table", "Heard
table").

1. A sleepy node "holds mail for nobody", but the owner rule would make
   one within 50 m of a gone relay's position the owner of its messages,
   with no queue to hold them. It now owns only what is addressed to it.
2. "Fills the neighbour table like any heartbeat", against eviction "when
   a relay not in the table is heard": a relay hears more sleepy nodes
   than it has entries. A sleepy node with an anchor takes only a free
   slot.
3. The heard table took "the sleepy nodes it hears beat"; a node without
   an anchor has no window to deliver in, so it takes those with one.

The derived current of the anchorless beats at their mean interval was
given as 0.33 mA: 38.08 s an hour at 32 mA is 0.3385 mA, 0.34.

## What was run

Simulators: **M**, the old code and the counting commit; **N**, the
branch at `e3e7c55`. S4 (`scenarios/s4-capacity.toml`, switch-on over a
window, random IDs, drifting clocks, walkers) at the rate of
spec "Capacity and sizing", seeds 1 and 2. The village beside a district of D51
(sixty sleepy nodes outdoors over a square kilometre, a district of 2 km²
750 m east with 19 windowsill relays and 160 indoor sleepy nodes, two
relays switched on at its edge at 1 800 s), seeds 1 to 10, and the village
alone, seeds 1 to 10. The D42 suites: S1 and the two meshes 10 seeds, S1b,
S2 and the corridor 60, the ring at 2 s 30 (its first 10 are the standard
suite), and the loaded ones, the corridor at 1 s and 0.5 s, the ring at
1 s and the burst mesh at 64 times its rate, 30 each. Delivered counts the
reachable texts originated from the settle time to the end less a
delivery TTL. Current is the radio's alone. The firmware sizes are
`llvm-size -A` of the release builds. No more than two S4 runs at once;
scripts and runs are in the scratchpad (`d58/`).

## Results

**S4**, means of seeds 1 and 2, the difference paired by seed:

| | M | N | N − M |
|---|---|---|---|
| Delivered | 90.75 % | 91.15 % | +0.40 ± 0.25 points |
| Latency p50, p90 | 136.7, 339.9 s | 137.1, 341.2 s | +0.4 ± 0.0, +1.3 ± 6.8 s |
| Sleepy node's radio current | 0.399 mA | 0.593 mA | +0.194 ± 0.001 mA |
| ... of it receiving duty-cycled | 0.000 mA | 0.180 mA | |
| Listening an hour | 245.6 s | 251.3 s | +5.7 ± 0.4 s |
| DATA to a sleepy node that cannot hear it, from its anchor | 21 | 15 | −6 ± 11 |
| ... from others | 720 | 0 | −720 ± 106 |
| Airtime with the long preamble | 4 585 s | 30 857 s | +26 272 ± 231 s |
| ... beats to anchorless neighbours | – | 26 125 s | |
| ... DATA | 1 940 s | 1 590 s | |
| ... beats of nodes without an anchor | 2 382 s | 2 863 s | |
| Airtime, all | 33 011 s | 59 454 s | +26 443 ± 74 s |
| Busiest node's duty cycle after the settle time, seeds 1, 2 | 4.7, 3.9 % | 3.3, 3.8 % | |

The DATA from its anchor that a node cannot hear goes in the last moments
of a window, within 5 s of the node leaving it, on both simulators.

**The long beats to anchorless neighbours.** 4 570 and 4 724 of S4's
20 000 sleepy nodes send them at some time in the run, eight and seven
times at the median, 152 and 256 at the most. In the village beside the
district 65 of 220 do, 74 times at the median over its 4 500 s, 1 665 s of
airtime a run. A node in anchorless mode beats on every channel in turn
with the long preamble, and every sleepy node with an anchor in reach that
receives duty-cycled on that channel catches it: in a dense domain many
do. Each keeps the entry for 1 201 s, though the anchorless node is
usually anchored again within minutes, and sends each of its beats on that
channel too until the entry expires. The spec's 5.9 s an hour per channel
is right per channel; it is multiplied by every node that heard and by
every channel its anchorless neighbours listen on. A first build that sent
them once a period, on a timer of their own, with the 60 s anchorless
period, spent 22 197 and 19 289 s on them in S4 and delivered 91.03 %.

**The village beside a district**, ten seeds:

| | M | N | N − M |
|---|---|---|---|
| Delivered, all | 86.89 % | 88.06 % | +1.17 ± 1.24 points |
| ... village to village | 70.8 % | 66.5 % | −4.4 ± 6.3 |
| ... between the village and the district | 77.7 % | 79.7 % | +2.0 ± 2.4 |
| ... within the district | 94.9 % | 95.3 % | +0.4 ± 0.7 |
| DATA to a sleepy node that cannot hear it, anchor, others | 10, 402 | 0, 1 | −10.5 ± 3.6, −401 ± 82 |
| Airtime | 2 167 s | 3 864 s | +1 697 ± 204 s |
| ... with the long preamble | 972 s | 2 706 s | +1 735 ± 165 s |
| ... of it beats to anchorless neighbours | – | 1 665 s | |
| ... of it DATA | 718 s | 721 s | |
| Sleepy node's radio current, all | 0.421 mA | 0.717 mA | +0.296 ± 0.007 mA |
| ... village nodes | 0.931 mA | 1.451 mA | +0.521 ± 0.025 mA |
| Village nodes that carry others' texts, a run | 15.6 | 27.3 | +11.7 ± 2.3 |
| ... their current | 1.363 mA | 1.379 mA | +0.016 ± 0.069 mA |

The nodes that carry others' texts are the village's sleepy nodes that
handed over more messages than they originated. Nearly twice as many do
now: nodes with an anchor are the way out of the village for those
without. Texts between two village nodes are about ten a seed, too few to
tell a change.

**The village of sixty with no relay**, ten seeds: delivered 100 % on
both, latency 0.5 s at the median on both. With the anchorless period
derived from the TTL, 46.2 s instead of 60 s, a node beats 92 times an
hour over the run, its boot listen included, 33.8 s of airtime, against 71
times and 26.1 s. Counted from the end of the boot listen, 390 s into the
4 500 s, that is 101 beats and 37 s an hour, within 3 % of the derived 104
and 38 s. The radio current
rises from 0.590 to 0.702 mA a node (+0.113 ± 0.001), transmitting 0.262
to 0.337 mA and listening 0.139 to 0.178 mA, and the airtime from 2 154
to 2 763 s a run (+609 ± 42).

**The D42 suites**, delivered M, N (paired difference ± one standard
error), and the sleepy node's current:

| Suite | M | N | N − M | Current, N − M |
|---|---|---|---|---|
| S1, 10 | 100 % | 100 % | 0 | +0.185 mA |
| Mesh spread, 10 | 99.69 % | 100 % | +0.31 ± 0.21 | +0.190 mA |
| Mesh burst, 10 | 100 % | 100 % | 0 | +0.190 mA |
| S1b, 60 | 100 % | 100 % | 0 | +0.183 mA |
| S2, 60 | 88.99 % | 90.43 % | +1.44 ± 0.48 | +0.191 mA |
| Corridor, 60 | 99.78 % | 99.78 % | 0 | +0.181 mA |
| Ring at 2 s, 30 | 77.67 % | 80.77 % | +3.10 ± 2.68 | +0.215 mA |
| Ring at 1 s, 30 | 71.93 % | 73.73 % | +1.80 ± 3.64 | +0.208 mA |
| Corridor at 1 s, 30 | 99.01 % | 98.59 % | −0.43 ± 0.33 | +0.069 mA |
| Corridor at 0.5 s, 30 | 97.84 % | 96.44 % | −1.39 ± 0.73 | +0.241 mA |
| Burst mesh ×64, 30 | 73.49 % | 74.61 % | +1.12 ± 1.45 | +0.213 mA |

The corridor at 0.5 s loses its texts on four seeds of thirty (6, 15, 22,
27), where an end relay, its channel full of its sleepy node's own texts,
hears none of its neighbour's heartbeats for minutes after the boot and
drops what its sleepy node gives it for want of a next hop. Main loses
texts the same way on seeds 4 and 20, which N repeats exactly. Which seeds
do it changes with any change that moves a frame.

**Firmware**, `bark-fw` release builds, `llvm-size -A`:

| Image | Flash (`.text`, `.rodata`, `.data`) | RAM (`.data`, `.bss`) |
|---|---|---|
| Sleepy, old code | 317 100 B | 174 372 B |
| Sleepy, N | 294 692 B | 155 372 B |
| Relay, N | 317 020 B | 174 388 B |

The sleepy image saves 19 000 B of RAM, the heard table and the delivery
queue, whose empty contents also start in flash.

## The amendment: long beats only to a neighbour that stays anchorless

Decided by the user after the results above: a sleepy node sends its
beats on an anchorless neighbour's channel only once it has heard that
neighbour's anchorless beat in two consecutive rotations of its beats.
Two beats heard on one channel come in consecutive rotations when they are
at least half a rotation apart, 13 beats at half an anchorless period
each, 300 s, and at most a rotation and a heartbeat's lag, 601 s; the
entry counts them in its count of beats, and a beat with the sleepy bit
alone, or the entry's expiry, starts it again. The reason: a neighbour
heard anchorless once may be switching on and anchor within minutes; a
pocket's nodes mostly know their bridge already from the time both were
anchorless. Measured against the branch before it, with the holder's
delivery outside the window in both (**E**, and **A** with the
amendment):

| | E | A | A − E |
|---|---|---|---|
| S4 delivered | 93.81 % | 93.77 % | −0.04 ± 0.20 points |
| S4 latency p50, p90 | 1.6, 162.1 s | 1.6, 149.8 s | 0.0, −12.2 ± 8.0 s |
| S4 long beats to anchorless neighbours | 25 835 s | 8 013 s | −69 % |
| S4 airtime with the long preamble | 43 949 s | 26 878 s | −17 071 ± 2 936 s |
| S4 airtime, all | 71 976 s | 55 291 s | −16 684 ± 2 826 s |
| S4 sleepy nodes sending them | 4 724 | 1 296 | |
| S4 sleepy node's current | 0.645 mA | 0.638 mA | −0.006 mA |
| Village beside a district, delivered | 90.78 % | 89.98 % | −0.80 ± 0.94 points |
| ... village to village | 75.5 % | 75.1 % | −0.4 ± 4.9 |
| ... between the village and the district | 83.8 % | 82.6 % | −1.2 ± 2.3 |
| ... long beats to anchorless neighbours | 1 716 s | 1 150 s | |
| ... airtime, all | 4 005 s | 3 470 s | −535 ± 136 s |
| ... village nodes' current | 1.448 mA | 1.369 mA | −0.079 ± 0.021 mA |

The pocket still finds its way out. In the village beside the district,
the texts the village's nodes send are delivered, over the ten seeds:

| Sent | E | A |
|---|---|---|
| Before the relays, 900–1 800 s | 78.5 % (135) | 80.6 % (134) |
| 0–300 s after the relays are switched on | 86.7 % (45) | 82.6 % (46) |
| 300–600 s after | 87.2 % (47) | 89.1 % (46) |
| 600–1 200 s after | 92.0 % (87) | 87.8 % (90) |
| 1 200–2 100 s after | 95.5 % (132) | 95.4 % (131) |

The differences are two to four texts in forty to ninety, within what
ten seeds can tell. 63 of the 220 sleepy nodes send the long beats there
against 69: the village's nodes heard each other anchorless for the half
hour before the relays came, so the bridges among them qualify at once
when they anchor. In S4 most of the anchorless spells are a node switching
on, and the long beats fall by two thirds. The village of sixty, which
has no anchor, and the D42 suites are identical, run for run: none has a
node in anchorless mode beside one with an anchor, but S2, +0.30 ± 0.31
points and 23.5 s less airtime.

## What it does not show

- Two seeds of S4; the gain in delivery there is within what two seeds
  can tell.
- The airtime of the long beats to anchorless neighbours depends on how
  many nodes go without an anchor and for how long, which the boot and the
  edge of relay coverage set; S4 starts everything within ten minutes.
- No test on hardware: the firmware still keeps the radio listening where
  the core asks for duty-cycled receive.

## Separately: a holder delivering outside the window with a long frame

Its own decision, in its own last commit; the user decided to keep it. A
holder sends a message for a sleepy node at once, with the long preamble,
while the node is outside its window, if its bucket holds that frame
beyond everything it already has to send: the send queue, the other
messages of its delivery queue at the short preamble in their windows, and
the reply reserve, which is the admission rule of the send queue's
regular room with the delivery queue's needs added. Otherwise, or once a
delivery to the node has gone unanswered, the message waits for the
node's beat as before. Once the node has taken it, the node lingers and
what follows goes with the short preamble. Measured against N with the
same scenarios and seeds (**E**):

| | N | E | E − N |
|---|---|---|---|
| S4 delivered | 91.15 % | 93.81 % | +2.67 ± 0.04 points |
| S4 latency p50, p90 | 137.1, 341.2 s | 1.6, 162.1 s | −135.5 ± 1.8, −179.1 ± 4.7 s |
| S4 DATA with the long preamble | 1 590 s | 14 886 s | |
| S4 airtime, all | 59 454 s | 71 976 s | +12 522 ± 2 476 s |
| S4 busiest node's duty cycle, seeds 1, 2 | 3.3, 3.8 % | 5.5, 5.9 % | |
| S4 sleepy node's current | 0.593 mA | 0.645 mA | +0.051 ± 0.001 mA |
| Village beside a district, delivered | 88.06 % | 90.78 % | +2.72 ± 0.72 points |
| ... latency p50, p90 | 140.7, 339.9 s | 0.8, 196.8 s | −139.9 ± 4.1, −143.1 ± 19.4 s |

The D42 suites, delivered and the sleepy node's current, E − N: S1, S1b,
the corridor and the burst mesh unchanged, the mesh spread −0.47 ± 0.24
points, S2 +2.49 ± 0.53, the ring at 2 s +18.2 ± 1.7 and at 1 s +24.7 ±
2.4, the corridor at 1 s +0.35 ± 0.45 and at 0.5 s +1.33 ± 0.68, the burst
mesh ×64 +9.5 ± 1.8. Latency at the median falls from 90 to 340 s to
1 to 100 s in every one. The current rises where messages to sleepy
nodes are many, since each one taken holds the node listening a hand-over
more: +0.58 mA in S1b and the corridor, +0.79 mA and +0.59 mA in the
rings, +2.6 mA in the corridor at 1 s, +0.91 mA in the burst mesh ×64.
In S4 the busiest node spends half again its airtime; the DATA lost
otherwise at sleepy nodes rises from 4 724 to 15 822 a run over the two
seeds, frames to nodes that are dark or gone, which a beat no longer
gates.

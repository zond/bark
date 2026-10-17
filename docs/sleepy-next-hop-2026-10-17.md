# A sleeping node is never a next hop: measured

**Date:** 2026-10-17. Roadmap D49, measured, awaiting the decision.
Branch `research/sleepy-next-hop`, based on main `bbde31a` (D48) and
measured against it.

A sleepy node with an anchor is deaf except in its window after a beat.
The D47 note found that most DATA sent to a sleeping node came from nodes
that were not its anchor, and left open why: a heartbeat did not say
whether its sender slept (`docs/sleep-window-2026-10-17.md`, "Where the
mail is lost"). This note traces where that DATA comes from and changes
three things, each measured: what a heartbeat says, when a sleepy node
takes its anchor, and how long others keep a node that has no anchor.

## Summary

- **Where it came from.** In main, 622 and 968 DATA a run start while
  their addressee sleeps, in the capacity domain of
  spec "Capacity and sizing" at its texting rate. 59 and 74 % come from
  sleepy nodes without an anchor, routing through sleepy neighbours
  because no relay was in reach; the rest from the anchor, a retry after
  the node's window had closed. The 85 % of the D47 note was of its
  variant A, before the heartbeat watch; the retry after a lost ACK it
  named is gone since D48.
- **Step 1, the heartbeat** (`52a5229`, `3e64a82`). It no longer says
  where its sender's position came from: nothing read it. It is type 0x8
  with two flags in its low bits. *Sleepy* (0x9) says the sender sleeps
  and may be sent to only in the window after its beat, by its anchor: it
  is never a next hop, and its beat keeps it out of every neighbour
  table. *Anchorless* (0xA) says the sender knows no node that is not
  sleepy: no anchor for another sleepy node; as D46 had it for sleepy
  nodes, a next hop only where no other is live, giving way in a full
  table. Against main: delivered +0.03 ± 0.05 points, DATA to a sleeping
  node 450 a run against 795.
- **Step 2, the anchor** (`9d041e6`). A sleepy node takes the first relay
  it hears as its anchor, at once, stays awake a worst-case rotation, and
  then takes the strongest it heard. It has an anchor after 211 s at the
  median against 429 s. At the texting rate it delivers the same, and
  DATA to a sleeping node falls to 301; its texts start 200 s sooner, and
  that start costs the loaded scenarios (below).
- **Step 3, anchorless entries** (`a3fca3e`). With step 2 a node is
  anchorless only while it hears no relay. Treated as any relay, with an
  entry that expires a rotation after its last heartbeat, it delivers
  the same and DATA to a sleeping node falls to 256. Without step 2 the
  same rule loses: phones are anchorless for 430 s at boot, fill the
  relays' tables and draw traffic (18 points at twice the rate). Step 3
  needs step 2, so the commits run 1, 2, 3, and the branch can stop
  after step 1 or after step 2.
- **The cost of step 2 is the earlier start.** Rings and corridors that
  carry a text a second to one node lose 7 to 1 482 messages over their
  10 to 60 seeds against main; started at the same time as main, they
  lose none or gain but for one jammed seed of a corridor, so the loss is
  the earlier start; no mail was stranded by the move at the end of the
  rotation. At twice the rate in S4 one seed did
  not recover from the earlier start (47 against 79 %).

## Where it came from

The simulator was given a classifier for every DATA that starts while its
addressee sleeps, after the settle time: whether the sender is the
node's anchor, how long the node had slept, what the sender is, whether
it had a relay live, and the age of its entry for the node against when
the node took its anchor. It is not on the branch; the branch has the
count by sender (`ea985d0`). S4 at the texting rate, seeds 1 and 2:

| | `2ace68e` (D47) | `bbde31a` (D48), main |
|---|---|---|
| all | 1 281, 1 591 | 622, 968 |
| from the anchor | 728, 789 | 258, 253 |
| of which a DATA the node already had | 468, 532 | 0, 1 |
| from others | 553, 802 | 364, 715 |
| of which from a sleepy node without an anchor and no relay live | 548, 796 | 362, 711 |
| of those, the entry refreshed by the sleeping node's beats | | 139, 299 |
| of those, the entry from before it had an anchor | | 223, 411 |
| of those, older than 390 s | | 164, 322 |
| from a relay delivering, not the anchor | 3, 6 | 0, 4 |

In main a sleepy node chooses its anchor a worst-case rotation after it
first hears a relay. About 550 of S4's 20 000 take it after the settle
time, and about 60 have none at any minute after it. While they have none
they forward, and route through sleepy neighbours because no relay is
live. Their tables held sleeping nodes by two routes: the sleeping node's
beats, which every node on its anchor's channel hears, and its entry
from before it had an anchor, kept 600 s, two beat periods.

**What routing through sleeping nodes delivered.** On `2ace68e`, 11 and
14 messages a run went a hop through a sleepy node from one with no relay
live; 8 and 7 arrived, at 33 to 228 frames each. The hop succeeds only if
it falls in the 477 ms after one of the sleeping node's beats.

**The anchor's share.** Each of the 256 a run is a DATA the node never
took: the attempt came after its window had closed. A first attempt fits
the window after a beat (spec "Delivery"); if it is lost, the retry, a
full frame, the ACK timeout and the first retry wait after the beat,
138.6 + 100 + 305.9 = 544.5 ms, finds the node asleep, and the message
waits for the next beat. This is 0.14 % of all DATA, about 35 s of the
27 700 s of airtime a run: noise, and not changed here.

## What was built

| Commit | Story |
|---|---|
| `ea985d0` | Simulator: DATA sent to a sleeping node, by its anchor or another. |
| `52a5229` | The heartbeat says nothing of where its position came from; type 0x8 with flags in its low bits, sleepy 0x9. No behaviour changes. |
| `3e64a82` | Sleepy means the sender sleeps; a beat keeps it out of the neighbour table; anchorless (0xA) is no anchor, and a next hop only where no other is live, giving way in a full table. |
| `b1d556d` | Simulator: when sleepy nodes first take an anchor, anchor changes after the settle time, anchorless entries in the relays' tables a minute at a time. |
| `9d041e6` | A sleepy node anchors at the first relay it hears, and a rotation later takes the strongest. The ring test starts its flows once every node has its anchor either way. |
| `a3fca3e` | An anchorless entry is any relay's, and expires a rotation and a heartbeat's lag after its last heartbeat. |

**The codes, derived.** The first byte is `type:4 | aux:4`, and a
heartbeat's `aux` holds its home channel, 0 to 12, in all four bits, so
the flags go in the type. Two flags need a base with both low bits clear
whose four codes are free of ACK 0x3, DATA 0x5 and the bench frames'
0xF. 0x0 would give 0x3, 0x4 would give 0x5, 0xC would give 0xF; 0x8
gives 0x8 to 0xB, all free. 0xB, both flags, is rejected: a node that
sleeps has an anchor. The old codes 0x1 and 0x2 said whether a position
was a map point or from GPS; nothing has read that since D46 stopped
freezing a relay's position, and the firmware set it for every board.
`wire.rs` asserts the codes at compile time.

**The first rotation, considered.** Should a switch in the first
rotation skip the margin? Moving at once to every relay heard stronger
than the anchor chases the fading: a single heartbeat of a relay no
better is above the anchor's average half the time, and in S2, seed 1,
its 80 sleepy nodes changed anchor 142 times in the run against 10.
Sleeping at once on the first relay heard keeps it however faint, since
a sleeping node hears other relays only by chance. Built: the node stays
awake the rotation and moves once, at its end, to the strongest it
heard, without the margin: the anchor main chose, reached 220 s sooner.
Anyone the node wrote to in that rotation has its first address; in the
loaded suites no mail was stranded by the move (below), so the margin was
not measured on it.

**The expiry, derived.** An anchorless node heartbeats as a relay, on
every channel once a rotation, at most 13 × 15 s = 195 s apart on any
one channel, and a heartbeat lags its slot by at most 638.56 ms (spec
"Heartbeat"). So 195.64 s without one on this channel says that it has
anchored and sleeps, unless its heartbeat was lost, which happens to one
in ten at a listening receiver (D47's measurement). An entry dropped
while the node is awake is back at its next heartbeat, and an anchorless
node is never an anchor, so a wrong expiry costs little; a stale one
draws DATA into a deaf node.

Tests: `a_sleeping_node_is_no_next_hop`,
`a_sleepy_node_anchors_at_the_first_relay_and_then_takes_the_strongest`,
`an_anchorless_node_is_no_anchor`, `sleepy_nodes_alone_meet_and_route`
(`tests/sleepy.rs`), `anchorless_entries_expire_after_a_rotation`
(`neighbours.rs`), the wire codes (`wire.rs`).

## What was run

S4 (`scenarios/s4-capacity.toml`) at one and two times the
spec "Capacity and sizing" rate, seeds 1 and 2; S4 at the rate with
every tenth relay (27) restarting at 1800 s, and with them removed then,
seed 1; the standard and loaded suites of D42; the village of D46, 60
sleepy nodes and no relays, seeds 1 to 10; the ring with its sleepy
nodes listening (`sleepy_listens`, as its test runs it), seeds 1 to 40.
Delivered counts reachable texts originated from the settle time to the
end less a delivery TTL. "DATA" is every DATA frame started after the
settle time, "lost" those its addressee did not receive, listening a
sleepy node's mean time an hour with its radio not asleep after the
settle time. No more than four S4 runs went at once.

Simulators: **B**, main with the count of `ea985d0`; **W**, step 1
(`3e64a82` with `b1d556d`); **A1**, step 2 (`9d041e6`); **Z**, step 3
(`a3fca3e`); **S13**, step 3 without step 2. Started late (**BT**,
**ZT**): B and Z with every text before 700 s dropped, a scratch change
to the simulator.

## Step 1 against main

| S4 | B, main | W, step 1 |
|---|---|---|
| ×1, delivered | 92.42 % | 92.45 % (+0.03 ± 0.05) |
| ×1, DATA to a sleeping node | 795 | 450 |
| ×1, of which not from the anchor | 540 | 202 |
| ×1, airtime; listening an hour | 27 658 s; 175 s | 27 557 s; 173 s |
| ×2, delivered | 82.40 % | 84.18 % (+1.8 ± 6.6) |
| ×2, DATA to a sleeping node | 2 582 | 648 |
| Restarted, delivered | 91.16 % | 91.02 % |
| Removed, delivered, seeds 1 to 3 | 85.70 % | 85.12 % (−0.58 ± 0.83) |

The suites deliver as main: S2 −0.08 ± 0.15 points, the mesh burst ×64
the same, every other run frame for frame the same. What still goes to
a sleeping node from others comes from anchorless nodes, through entries
from before the sleeping node took its anchor.

## Step 2: the first relay heard

| S4 | W, wait 195 s | A0, at once, asleep | A1, at once, a rotation awake |
|---|---|---|---|
| ×1, delivered | 92.45 % | 89.08 % (−3.37 ± 0.17) | 92.40 % (−0.05 ± 0.03) |
| ×1, first anchor p50, p90 | 429 s, 536 s | 211 s, 260 s | 211 s, 260 s |
| ×1, anchor changes a node an hour | 0.17 | 0.36 | 0.18 |
| ×1, listening an hour | 173 s | 220 s | 168 s |
| ×1, DATA to a sleeping node | 450 | 618 | 301 |
| ×1, anchorless nodes after the settle time | 61 | 15 | 15 |
| ×2, delivered | 84.18 % | 59.58 % | 48.12 % (−36.1 ± 7.9) |
| ×2, traffic from 450 s, delivered | 79.54 % | | 85.20 % (+5.7 ± 5.9) |
| Restarted, delivered | 91.02 % | 87.06 % | 90.99 % |
| Removed, delivered | 86.05 % | 82.78 % | 86.22 % |

Relays start heartbeating after their own boot listen, 195 s, so a
sleepy node's first anchor comes about 15 s after that. At twice the
rate, 6 550 texts are sent before the settle time against 4 270, as nodes
have addresses 220 s sooner; the relays' queues fill, and the domain stays
overloaded for the hour: settled, 45 and 37 % against 74 and 82 %.
Started at 450 s in both, the same traffic delivers as well or better
with A1. A0 keeps the first relay heard: it changes anchor twice as often
after the settle time and loses 3 to 20 points in S2 and the meshes; A1
loses none there.

## Step 3: anchorless entries

With step 2 in place, from A1:

| S4 | (b) last resort, giving way | (a) as any relay's | (c) expiring after a rotation |
|---|---|---|---|
| ×1, delivered | 92.40 % | 92.35 % (−0.05 ± 0.03) | 92.41 % (+0.01 ± 0.07) |
| ×1, DATA to a sleeping node (not from the anchor) | 301 (14) | 370 (116) | 253 (9) |
| ×2, delivered; DATA to a sleeping node | 48.1 %; 354 | 48.9 %; 508 | 43.3 %; 262 |
| Restarted: delivered; DATA to sleeping | 90.99 %; 249 | 90.88 %; 393 | 90.94 %; 269 |
| Removed: delivered; DATA to sleeping | 86.22 %; 258 | 86.55 %; 395 | 86.56 %; 241 |
| A relay's anchorless entries at boot, mean at the peak; most | 33.4; 57 | 34.3; 56 | 34.3; 56 |
| the same after the settle time | 0.05; 4 | 0.06; 4 | 0.02; 3 |

(c) was measured at 195 s; the commit adds the 0.64 s lag. The suites
deliver the same in all three but the rings: (a) and (c), identical
there, deliver 7.2 ± 3.6 points more than (b) on the ring and 4.8 ± 2.4
on the ring at 2 s, 3.5 ± 3.0 less at 1 s. The village delivers every
text in all three. So with step 2 the rule that kept an anchorless node
last is not needed, and the expiry saves the DATA it saved, and more.

**Step 3 without step 2.** On step 1, with the 195 s wait kept (S13):

| S4 | B, main | W, step 1 | S13, step 1 and (c) |
|---|---|---|---|
| ×1, delivered | 92.42 % | 92.45 % | 92.25 % (−0.17 ± 0.10 against main) |
| ×1, DATA to a sleeping node (not from the anchor) | 795 (540) | 450 (202) | 1 642 (1 370) |
| ×1, airtime | 27 658 s | 27 557 s | 31 195 s |
| ×2, delivered | 82.40 % | 84.18 % | 64.35 % (−18.1 ± 1.5) |
| Restarted, delivered; DATA to sleeping | 91.16 %; 600 | 91.02 %; 608 | 90.74 %; 2 346 |
| Removed, delivered; DATA to sleeping | 86.57 %; 677 | 86.05 %; 577 | 86.49 %; 2 413 |
| A relay's anchorless entries at boot, mean at the peak; after the settle time | | 52; 0.8 | 58 of 64; 0.4 |

With the wait every phone is anchorless for about 430 s at boot: its
entries take 58 of a relay's 64 places, and as next hops like any relay
they draw traffic into phones that then anchor and sleep. The suites
deliver as main (S2 +0.6 ± 0.3, the rings within their spread). So step
3 is right only with step 2, and the branch orders them so.

## The rings and corridors: the earlier start

Against main, Z loses on the loaded flows to one node. Split, as
messages not delivered in the measured window, summed over the seeds:
(i) the earlier start, the loss that goes when both start their texts at
700 s, after every node has its anchor either way; (ii) mail stranded at
the first address by the move at the end of the rotation, counted as
messages whose delivery TTL ran out at a node no longer their
addressee's anchor; (iii) the rest, Z against main both started at 700 s.

| Scenario | Z − main | (i) earlier start | (ii) stranded | (iii) the rest |
|---|---|---|---|---|
| Ring, seeds 1 to 10 | +196 | +267 | 0 | −71 |
| Ring at 1 s, 1 to 30 | +1 482 | +1 885 | 0 | −403 |
| Ring at 2 s, 1 to 30 | +517 | +1 925 | 0 | −1 408 |
| Corridor at 1 s, 1 to 30 | +447 | +314 | 0 | +133 |
| Corridor at 0.5 s, 1 to 30 | +533 | +533 | 0 | 0 |
| Corridor, 1 to 60 | +7 | +7 | 0 | 0 |

No anchor moved in these scenarios after any mail was sent to it, so no
mail was stranded, and the margin was not measured on the move. Started
together, Z delivers 2.2, 0.5 and 6.0 points more on the rings than main
with latency at the median 75 s less, 10 s more and 16 s less; the corridor at 1 s
loses its 133 in one seed of 30, which jammed (58 %).

**The ring test.** With its sleepy nodes listening, the ring at its cap
jams on some seeds, its backlog still in flight at the end:

| Ring, sleepy nodes listening, seeds 1 to 40 | seeds below 0.98 | worst | seeds 1, 2, 3 |
|---|---|---|---|
| main | 0 | 0.991 | 0.999, 0.999, 0.998 |
| main, texts from 700 s | 3 | 0.946 | 0.995, 0.999, 0.998 |
| S13, step 1 and (c) | 3 | 0.965 | 0.987, 0.998, 0.996 |
| A1, step 2 | 3 | 0.957 | 0.992, 0.993, 0.998 |
| (c) on step 2 | 2 | 0.899 | 0.899, 0.998, 0.998 |
| Z, texts from 700 s | 1 | 0.936 | 0.999, 0.997, 0.998 |

Whether it jams depends on when its flows start: main jams too once its
texts start later. In seed 1 with step 2 and (c) the run first differs
from A1 at 264 s, when a relay hands a text to a sleepy node nearer the
destination that is still anchorless, which hands it back; the two runs
then go different ways, and the jam starts at 1 400 s, after every
anchorless entry has expired. So (c) does not lose on this scenario;
which seeds jam moved. The test keeps its bound, 0.98 on seeds 1 to 3,
and starts its flows at 800 s, after every node has its anchor whichever
way it chose it, the latest at 727 s in the runs measured.

## All together

| | B, main | Z, all three |
|---|---|---|
| ×1, delivered, seeds 1, 2 | 92.61, 92.24 % | 92.43, 92.26 % |
| ×1, − B, points | | −0.08 ± 0.10 |
| ×1, latency p50, p90 | 138 s, 339 s | 139 s, 343 s |
| ×1, DATA a run | 176 698 | 189 902 |
| ×1, DATA to a sleeping node | 795, 0.45 % | 256, 0.13 % |
| ×1, of which from the anchor, from others | 256, 540 | 248, 7 |
| ×1, DATA lost | 54 477 | 60 036 |
| ×1, airtime a run | 27 658 s | 24 740 s |
| ×1, listening an hour | 175 s | 170 s |
| ×2, delivered, seeds 1, 2 | 85.98, 78.82 % | 85.28, 46.98 % |
| ×2, DATA to a sleeping node | 2 582 | 346 |
| Restarted, delivered | 91.16 % | 90.68 % |
| Restarted, their nodes heard again; mail delivered | 2 003 of 2 037; 901 of 1 021 | 1 962 of 2 024; 873 of 1 007 |
| Removed, delivered | 86.57 % | 85.27 % |
| Removed, their nodes heard again; mail delivered | 1 969 of 2 037; 679 of 1 021 | 1 946 of 2 024; 599 of 1 007 |

There are more DATA frames and less airtime: texts sent at boot by nodes
that now have an address count in neither, and the anchorless heartbeats
of 60 nodes an hour, at a relay's rate, are gone. The suites against
main: S1, S1b, S2, the meshes, the mesh burst ×64 and the village within
0.6 points; the rings 3.6 to 7.2 points less and the loaded corridors
1.4 to 1.8, the earlier start above.

## What this does not show

- At twice the rate, whether the branch or main is better once both
  start with traffic flowing: the two seeds of Z differ by 38 points,
  and the boot surge decides. How the network recovers from a surge is
  the collapse question of D44 and D45, not changed here.
- A node that hears no relay but hears sleeping nodes, a phone deep in a
  building beside one by a window, has no way out until it hears a relay;
  it had one by luck in main, 7 or 8 messages a run.
- The holder's retry after the window closed, 0.14 % of DATA.
- The move at the end of the first rotation with the 8 dB margin: no
  mail was stranded without it in the scenarios run.
- One seed of the restart and the removal for Z and S13; clocks that
  drift and nodes that move are not simulated.

## Recommendation

Adopt step 1: a sleeping node is no next hop and an anchorless one no
anchor, for the same delivery and 43 % less DATA into sleeping nodes,
with no cost found. Steps 2 and 3 go together: DATA into sleeping nodes
falls by two thirds for the same delivery at the texting rate, but nodes
reachable 220 s sooner send 200 s sooner, and a domain at twice the rate,
or a ring at its cap, takes that start worse than main's quiet one.
Started together the loaded scenarios do as well or better with them, so
adopting them asks for a look at how the network takes a boot surge.

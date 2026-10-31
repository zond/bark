# A copy from elsewhere is answered seen

**Date:** 2026-10-31. **Status:** measured; roadmap D90, adopted 2026-10-31 on the
user's reading: main's fourth case dropped a branch of the graph (a
relay that took a copy from a node it had not handed to kept its first
parent and returned there, and the sender, done by the ACK, never tried
its other candidates), so the sender must exhaust its candidates before
anyone searches on, whatever it costs against that shortcut while the
splits are unfixed; the splits are measured on this base from here on. Spec references are to `protocol.md`. The form the D89 note
(`storm-absorb-2026-10-31.md`) said to build next: the same copies D89
absorbed are answered "seen, not taken", and their sender searches on.
Measured against main `ab38727`, whose outputs are those of `e75bd97`
to the byte on the ring (checked), and against D89's branch at
`c7bdc7e`; all release builds, the same scenario files and seeds, the
branch's binary from the commit that carries the simulator's counters.

## Summary

- **The rule.** A relay that has a record of a message, holds no copy
  of it, and receives it from a node it did not hand the message to
  answers an ACK with a seen flag instead of taking the copy. It bans
  nothing, drops nothing it holds and takes on no work. The sender marks
  that relay tried, bans nothing, and goes on to its next candidate at
  once, as if the relay had returned the message the moment it got it.
  Arrival (case 1), a retransmission (case 3), a copy while holding one
  (case 4) and a return from a child (case 5) are as before.
- **It does not end the storm.** `dense-power.toml` seed 1, settled
  hour: 90.3 → 89.3 % delivered, the run's airtime 1 134 454 →
  1 162 844 s, latency p90 288 → 311 s; D89 had 97.9 % and 202 693 s.
  Seed 7: 98.2 → 98.1 %, airtime 276 300 → 298 467 s. The copies that
  D89 ended at crossed relays now live out their time to live, bouncing
  off them: 1.84 M seen ACKs in the run, 1.61 M copies answered seen,
  hand-overs failed 2.62 → 2.77 M while those taken fell 3.26 → 1.80 M,
  and the same frames per text as main (p50 143, p90 2 332).
- **The mesh is nearly whole again.** On the random mesh of 25 relays,
  where D89 lost 14 of 100 texts of a burst, this rule delivers 98 of
  100; the two it does not deliver in time stalled at a relay whose last
  untried neighbour is one it hears but cannot send to, a trap the
  search had before this rule and now reaches more often. The spread
  run delivers 100 %, as main does, at main's airtime. The burst costs
  18 % more airtime than on main: the search backtracks where main's
  copy jumped.
- **Under load it costs.** S4 at three times the rate, seeds 1 / 2 / 3:
  59.7 / 56.8 / 63.8 → 51.4 / 48.7 / 51.7 %, eight to twelve points
  below main where D89 was 13 to 20 above it; latency p50 143 / 160 /
  151 → 174 / 183 / 206 s; airtime 5 to 7 % more. At the design rate it
  is neutral: 93.4 → 93.6 %, airtime 2 % more. The walkers 96.5 →
  96.5 % at 12 % more airtime, p90 49 → 61 s; the ring unchanged.
- **Recommendation: not adopted.** What ended the storm under D89 was
  that a split's copies died where the search had been; what lost the
  mesh's texts was that a single search's copy died there too. Nothing
  in the records tells the two apart, and seen keeps both alive: the
  mesh is nearly whole again and the storm is whole again. The rule is
  the depth-first search's correct answer to a cross link, and it is
  the wrong answer to a split. The splits themselves are parts 2 to 4
  of the storm candidate, not yet built; with fewer of them, D89's
  pruning and this rule would both have less to do, and the mesh's
  crossing loss would be D89's whole cost.

## The rule

The ACK's first byte becomes `s 1 w w w w w w`: bit 6 the ACK kind as
before, bit 7 the seen flag, the six wait bits as before and 0 in a seen
ACK (spec "Packet formats"). The flag sits where a heartbeat has its
kind, so the kind `11` that was free is the seen ACK; an ACK is still 18
bytes and 17.0 ms. A seen ACK with a wait is rejected as of no kind.

Spec "Receiving DATA" tests a DATA packet of a message the relay has a
record of, holds no copy of, that is not a retransmission and has not
arrived, in two cases:

- **A return** (case 5): the sender is in the record's `handed` set, or
  it is the neighbour the relay is still handing the message to. The
  relay takes it and goes on with its own search from its record, as
  before. A copy for a destination the relay owns now is taken and
  delivered the same way.
- **A copy from elsewhere** (case 6): any other sender. The relay answers
  seen. It changes nothing but its record: the packet is noted as the
  last one, with the answer it got, and its sender is counted as tried.

The sender, on a seen ACK that answers its outstanding hand-over, goes
through the one step a taken ACK goes through: the receiver is tried in
the record's `handed` set and the count the receiver recorded is spent
(spec "Hop count"). Then, where a taken ACK takes the packet out of the
send queue, a seen one leaves it there, to be routed again from the
record when its turn comes: the same choice a return taken from a child
gets, by the same next-hop function. Nothing is banned and no failure is
counted. A return answered seen is dropped: the parent has gone on
without this copy, which it never knew the child had, and a return has
nowhere else to go.

Four readings were needed where the rule as agreed met the code:

- **A retransmission is answered as the first packet was.** The record
  remembers that its last packet was answered seen. Without that, a
  seen ACK that was lost would make the sender's retry a retransmission
  (case 3), answered with a plain ACK: the sender would take the relay
  to have the packet and be done with the message, which would end a
  copy at a relay that holds none, the D89 loss. With it, the retry is
  answered seen again, and the sender goes on.
- **The relay that answers seen counts the sender as tried.** The sender
  has the message and searches on from its own record. Were the relay
  later to offer the message to that sender, the sender would take it
  as a return from a node in its `handed` set, and a second copy would
  search from there while the first is with the sender's child. Marked
  on both sides, the link is crossed once.
- **The count is spent.** The receiver recorded the count it was sent;
  the sender's next frame goes one higher, so that a later offer to the
  same receiver, after its slot changed hands and its bit was cleared,
  is not taken for a retransmission.
- **The parent that took a return is in `handed`.** As D89 found: the
  spec defined the set as every neighbour that ACKed a hand-over, which
  a return is, and the code set the bit for forwards only.

The simulator counts the copies each node answered seen and the seen
ACKs each node acted on, prints "seen ACKs: N sent, M acted on", and
reports a return answered seen as a drop of its own reason,
`ReturnSeen`. Nothing else is dropped.

## Why

From `docs/storm-2026-10-30.md` and the D89 note: a hand-over that fails
after three unanswered transmissions, with the receiver having taken the
message, leaves two searchers, and each copy that reached a relay that
had handled the message reopened the search there, case 5 as it was.
D89 ended the storm by absorbing those copies, and lost 14 of 100 texts
on the random mesh: the sender of an absorbed copy was done with the
message, so a single search ended where it crossed its own path.

Seen keeps what ended the storm, that a copy cannot start the search
again at a relay the search has been, and gives up what lost the texts:
the copy stays with its sender, which has the one record that knows
what it has still to try. This is the depth-first search's own answer
to a cross link: the relay met again is neither searched twice nor
skipped. Its neighbours are its own to try, when the message returns to
it from its child, or were tried already; the sender marks the link
crossed and goes on.

**When the seen ACK is lost.** The sender retries, as for any lost ACK;
the relay answers seen again, since its record says the last packet
was; after three lost answers the sender counts a failure, bans the
relay and waits for the ban, as for any silence, then offers once more
and is answered seen. The seen answer only shortens the silence path.

**Where the relay cannot afford the reply.** It stays silent, as for any
answer it cannot afford, and the sender's attempts fail and move on as
for silence.

**Stalls and loops.** The search cannot stall on a seen answer: the
sender keeps the message, and its next choice is its own, from its
record; with no candidate left it returns to its parent as before. It
cannot loop: the message moves only when a hand-over is taken, a relay
hands it to each neighbour at most once per record, and a relay that
answers seen never takes the message from that sender again. A
depth-first search with handed sets still visits every reachable relay:
a relay with a record either awaits a child's return, and tries its
remaining neighbours then, or has returned, with none left. The one
place a copy ends at a relay that holds none is a return to a parent
that gave the child up (spec "Receiving DATA" case 6, roadmap D35 A): the
parent's two hand-overs to the child failed with every ACK lost, it
demoted the child and moved on, and the child's return finds it holding
nothing and not knowing the child had the message. Before this rule the
parent took the return and searched on; now the child drops it, and
what the child searched is in the relays' records for the parent's
other copy to meet as seen. The routing test
`a_return_to_a_parent_that_gave_up_on_the_child_ends_there` describes
it, with its cost: six ACKs lost in a row on one link while the other
copy comes back over it.

## How it was measured

Every run is the scenario file at the seed named, with `--json` and
`--messages`; the figures are the JSON summary's: delivered of the texts
sent in the settled hour and of all texts, latency, the run's airtime,
the cores' counters summed over the nodes, drops and texts lost by
reason, the relays whose load sample ever found the bucket under its
reserve, and the largest settled airtime of a node. Main's figures are
the reference outputs of `e75bd97` for seed 1 of each scenario, which
main `ab38727` reproduces to the byte on the ring, and runs of `ab38727`
for the mesh and for S4 at three times the rate at seeds 2 and 3; D89's
are its branch's outputs of 2026-10-31. The three-times-rate S4 is
`s4-capacity.toml` with `per_leaf_per_hour` 2.875. One mesh-burst run
was traced to follow its slowest texts.

## The table

Main / D89 / D90. "Copies answered" are those from no neighbour the node
had handed the message to, cases 4 to 6; "returns" those from one it
had. A text is "lost" to a reason when its last copy ended with it.

| | dp1 | dp7 | s4x1 | s4x3-s1 | s4x3-s2 | s4x3-s3 | ring | movers |
|---|---|---|---|---|---|---|---|---|
| texts sent in the settled hour | 18202 / 18234 / 18198 | 18190 / 18190 / 18190 | 17459 / 17453 / 17453 | 48526 / 50700 / 47815 | 48110 / 50727 / 47275 | 49340 / 52206 / 47997 | 2918 / 2867 / 2867 | 17894 / 17894 / 17893 |
| delivered, settled hour % | 90.3 / 97.9 / 89.3 | 98.2 / 98.1 / 98.1 | 93.4 / 93.5 / 93.6 | 59.7 / 72.8 / 51.4 | 56.8 / 71.8 / 48.7 | 63.8 / 83.6 / 51.7 | 92.4 / 92.1 / 92.1 | 96.5 / 95.4 / 96.5 |
| delivered, whole run % | 91.2 / 95.9 / 90.8 | 96.9 / 96.4 / 96.9 | 93.4 / 93.6 / 93.6 | 65.1 / 76.0 / 58.3 | 62.6 / 75.6 / 56.6 | 68.3 / 84.8 / 57.5 | 92.6 / 92.5 / 92.5 | 95.7 / 94.1 / 95.7 |
| latency p50 s | 20.2 / 9.6 / 20.4 | 10.3 / 9.6 / 10.6 | 1.6 / 1.6 / 1.6 | 142.6 / 61.4 / 173.6 | 160.3 / 67.4 / 182.8 | 151.2 / 42.7 / 205.7 | 60.7 / 53.3 / 53.3 | 5.5 / 5.3 / 5.7 |
| latency p90 s | 287.7 / 41.8 / 310.7 | 44.9 / 41.0 / 44.9 | 191.5 / 191.4 / 191.5 | 654.0 / 509.7 / 714.5 | 685.2 / 516.4 / 721.6 | 645.0 / 450.4 / 754.1 | 152.7 / 157.9 / 157.9 | 48.6 / 34.1 / 61.3 |
| airtime, whole run s | 1134454 / 202693 / 1162844 | 276300 / 200781 / 298467 | 49656 / 49173 / 50817 | 169431 / 144371 / 177762 | 173508 / 143620 / 178517 | 181241 / 146908 / 194569 | 1808 / 1802 / 1802 | 175315 / 117234 / 197178 |
| hand-overs taken | 3261455 / 304274 / 1797312 | 638893 / 302998 / 440279 | 124609 / 114940 / 117039 | 438792 / 397269 / 390934 | 449168 / 404547 / 383060 | 464236 / 414490 / 396998 | - / - / - | 504200 / 230515 / 331529 |
| hand-overs failed | 2620398 / 283020 / 2773239 | 440818 / 279315 / 519695 | 20124 / 18460 / 20384 | 170984 / 127843 / 186686 | 173051 / 123715 / 187101 | 190398 / 126680 / 217207 | - / - / - | 236598 / 119896 / 311473 |
| failed, receiver had it | 767407 / 30766 / 796347 | 98492 / 30840 / 121060 | 4417 / 3320 / 4709 | 43477 / 31854 / 45120 | 43887 / 31020 / 47070 | 55812 / 40081 / 57374 | - / - / - | 119686 / 24357 / 174071 |
| returns taken from a child | 382239 / 40 / 24885 | 75294 / 56 / 3603 | 2781 / 53 / 114 | 12295 / 813 / 1041 | 17635 / 767 / 1197 | 15369 / 795 / 1283 | - / - / - | 67757 / 82 / 3880 |
| copies answered (not returns) | 1440358 / 10292 / 1695384 | 185314 / 10214 / 253654 | 4860 / 617 / 8456 | 34591 / 9732 / 59338 | 46979 / 10872 / 74344 | 43011 / 9379 / 69666 | - / - / - | 153295 / 5275 / 232257 |
| of those merged (case 4) | 175211 / 275 / 84411 | 9808 / 277 / 2427 | 165 / 22 / 53 | 2963 / 970 / 2485 | 3426 / 946 / 2803 | 4053 / 976 / 3494 | - / - / - | 7778 / 108 / 1752 |
| of those absorbed (D89) / seen (D90) | 0 / 10010 / 1610819 | 0 / 9934 / 251159 | 0 / 595 / 8403 | 0 / 8757 / 56853 | 0 / 9922 / 71541 | 0 / 8399 / 66171 | - / - / - | 0 / 5165 / 230474 |
| seen ACKs sent:acted on | 0 / 0 / 1840553:1474842 | 0 / 0 / 283801:239607 | 0 / 0 / 9048:8295 | 0 / 0 / 63334:55254 | 0 / 0 / 79311:69645 | 0 / 0 / 74188:63961 | 0 / 0 / 1:1 | 0 / 0 / 263477:218988 |
| hand-overs held back by the bucket | 1760186 / 4460 / 1773973 | 9619 / 5147 / 18577 | 147 / 65 / 103 | 864004 / 440900 / 1030384 | 907780 / 448484 / 1037569 | 861766 / 252856 / 1171008 | - / - / - | 3872 / 68 / 15071 |
| neighbours demoted | 1042233 / 127180 / 1114604 | 188196 / 125433 / 216535 | 7340 / 6852 / 7474 | 49368 / 36312 / 53985 | 49559 / 34947 / 54146 | 55477 / 36024 / 64394 | - / - / - | 97737 / 54354 / 117815 |
| copies dropped: hop cap | 22474 / 0 / 2054 | 4059 / 0 / 1435 | 53 / 0 / 45 | 151 / 0 / 31 | 134 / 0 / 38 | 155 / 0 / 53 | 0 / 0 / 0 | 4355 / 0 / 1744 |
| copies dropped: TTL | 73956 / 19 / 68874 | 531 / 8 / 781 | 368 / 342 / 324 | 6004 / 3710 / 7345 | 6512 / 3167 / 7103 | 6022 / 2415 / 7952 | 0 / 0 / 0 | 424 / 38 / 1022 |
| copies dropped: absorbed (D89) / return seen (D90) | 0 / 10010 / 1 | 0 / 9934 / 2 | 0 / 595 / 0 | 0 / 8757 / 0 | 0 / 9922 / 0 | 0 / 8399 / 0 | 0 / 1 / 0 | 0 / 5165 / 1 |
| copies dropped: no route | 4 / 4 / 341 | 5 / 4 / 165 | 1 / 0 / 274 | 8 / 13 / 949 | 7 / 8 / 1099 | 12 / 12 / 1269 | 31 / 26 / 27 | 2 / 0 / 122 |
| copies dropped: return failed | 121 / 9 / 132 | 51 / 9 / 38 | 0 / 0 / 0 | 1 / 0 / 3 | 3 / 0 / 7 | 4 / 0 / 3 | 3 / 3 / 3 | 26 / 1 / 45 |
| texts lost: hop cap | 254 / 0 / 229 | 236 / 0 / 224 | 15 / 0 / 31 | 7 / 0 / 20 | 17 / 0 / 28 | 14 / 0 / 28 | 0 / 0 / 0 | 178 / 0 / 144 |
| texts lost: TTL | 683 / 19 / 787 | 78 / 7 / 83 | 338 / 326 / 306 | 5602 / 3601 / 6789 | 5883 / 3061 / 6474 | 5357 / 2294 / 7225 | 0 / 0 / 0 | 79 / 31 / 134 |
| texts lost: absorbed (D89) / return seen (D90) | 0 / 624 / 0 | 0 / 577 / 0 | 0 / 66 / 0 | 0 / 3711 / 0 | 0 / 4761 / 0 | 0 / 2324 / 0 | 0 / 1 / 0 | 0 / 772 / 0 |
| texts lost: no route | 3 / 4 / 23 | 5 / 4 / 6 | 1 / 0 / 6 | 8 / 13 / 97 | 6 / 8 / 119 | 11 / 12 / 134 | 28 / 25 / 26 | 2 / 0 / 6 |
| texts lost: return failed | 13 / 6 / 18 | 5 / 2 / 2 | 0 / 0 / 0 | 1 / 0 / 0 | 0 / 0 / 6 | 0 / 0 / 1 | 1 / 1 / 1 | 1 / 0 / 3 |
| texts that waited at their origin | 2362 / 51 / 2873 | 49 / 37 / 51 | 194 / 198 / 183 | 212730 / 109295 / 250776 | 225537 / 118098 / 257285 | 209319 / 59114 / 280351 | 2407 / 2375 / 2375 | 90 / 56 / 141 |
| relays ever at their cap | 1616 / 7 / 1771 | 44 / 7 / 83 | 0 / 0 / 2 | 179 / 134 / 199 | 176 / 130 / 191 | 204 / 154 / 223 | - / - / - | 62 / 7 / 251 |
| largest settled airtime of a node % | 8.07 / 4.70 / 8.51 | 5.35 / 4.33 / 4.61 | 6.21 / 5.91 / 5.90 | 9.91 / 9.91 / 9.95 | 9.97 / 9.96 / 9.97 | 9.99 / 9.97 / 9.91 | 9.88 / 10.14 / 10.14 | 5.46 / 4.77 / 4.98 |
| heartbeats received % | 71.1 / 86.4 / 70.5 | 85.3 / 86.6 / 84.9 | 94.3 / 94.3 / 94.2 | 88.2 / 89.2 / 87.7 | 88.2 / 89.2 / 87.7 | 87.0 / 88.5 / 86.3 | 87.8 / 88.8 / 88.8 | 89.3 / 90.0 / 89.2 |

The random mesh, 25 relays and 25 leaves, every leaf sending across it
once, all at once (`mesh-burst.toml`) or each at its own time with the
middle relay restarting (`mesh-spread.toml`); at seed 5 the scenario
counts the texts of one relay as unreachable, and every build delivers
all the reachable ones:

| | burst-s1 | burst-s5 | spread-s1 | spread-s5 |
|---|---|---|---|---|
| texts sent in the settled hour | 100 / 100 / 100 | 100 / 100 / 100 | 90 / 90 / 90 | 90 / 90 / 90 |
| delivered, settled hour % | 100.0 / 86.0 / 98.0 | 100.0 / 92.0 / 94.0 | 100.0 / 95.6 / 100.0 | 100.0 / 92.2 / 92.2 |
| delivered, whole run % | 100.0 / 86.0 / 98.0 | 100.0 / 92.0 / 94.0 | 100.0 / 95.6 / 100.0 | 100.0 / 92.2 / 92.2 |
| latency p50 s | 3.9 / 3.5 / 4.0 | 2.5 / 2.2 / 2.2 | 0.7 / 0.7 / 0.7 | 0.7 / 0.7 / 0.7 |
| latency p90 s | 8.4 / 6.0 / 8.1 | 5.9 / 4.7 / 5.1 | 1.0 / 0.9 / 1.0 | 3.0 / 0.9 / 1.2 |
| airtime, whole run s | 240 / 208 / 283 | 263 / 201 / 314 | 194 / 181 / 193 | 194 / 179 / 271 |
| hand-overs taken | 707 / 466 / 689 | 935 / 398 / 633 | 549 / 401 / 469 | 521 / 354 / 586 |
| hand-overs failed | 58 / 39 / 140 | 118 / 30 / 231 | 0 / 0 / 0 | 7 / 10 / 200 |
| failed, receiver had it | 22 / 11 / 89 | 90 / 7 / 186 | 0 / 0 / 0 | 1 / 2 / 175 |
| returns taken from a child | 59 / 0 / 24 | 187 / 0 / 62 | 40 / 0 / 0 | 63 / 0 / 74 |
| copies answered (not returns) | 85 / 17 / 236 | 257 / 9 / 427 | 52 / 4 / 72 | 70 / 8 / 394 |
| of those merged (case 4) | 0 / 0 / 2 | 5 / 0 / 8 | 0 / 0 / 0 | 0 / 0 / 1 |
| of those absorbed (D89) / seen (D90) | 0 / 17 / 234 | 0 / 9 / 419 | 0 / 4 / 72 | 0 / 8 / 393 |
| seen ACKs sent:acted on | 0 / 0 / 248:233 | 0 / 0 / 449:410 | 0 / 0 / 72:72 | 0 / 0 / 403:390 |
| hand-overs held back by the bucket | 0 / 0 / 1 | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 |
| neighbours demoted | 6 / 4 / 26 | 27 / 4 / 40 | 0 / 0 / 0 | 2 / 5 / 33 |
| copies dropped: hop cap | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 |
| copies dropped: TTL | 0 / 0 / 3 | 1 / 0 / 3 | 0 / 0 / 0 | 0 / 0 / 5 |
| copies dropped: absorbed (D89) / return seen (D90) | 0 / 17 / 0 | 0 / 9 / 0 | 0 / 4 / 0 | 0 / 8 / 0 |
| copies dropped: no route | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 |
| copies dropped: return failed | 0 / 0 / 1 | 0 / 0 / 4 | 0 / 0 / 0 | 0 / 0 / 1 |
| texts lost: hop cap | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 |
| texts lost: TTL | 0 / 0 / 1 | 0 / 0 / 3 | 0 / 0 / 0 | 0 / 0 / 5 |
| texts lost: absorbed (D89) / return seen (D90) | 0 / 14 / 0 | 0 / 8 / 0 | 0 / 4 / 0 | 0 / 7 / 0 |
| texts lost: no route | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 |
| texts lost: return failed | 0 / 0 / 0 | 0 / 0 / 2 | 0 / 0 / 0 | 0 / 0 / 1 |
| texts that waited at their origin | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 |
| relays ever at their cap | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 |
| largest settled airtime of a node % | 0.52 / 0.48 / 0.78 | 0.56 / 0.43 / 1.01 | 0.40 / 0.39 / 0.42 | 0.41 / 0.40 / 0.91 |
| heartbeats received % | 99.4 / 99.5 / 99.3 | 99.5 / 99.5 / 99.2 | 99.5 / 99.5 / 99.5 | 99.4 / 99.4 / 99.2 |

## What it shows

- **The storm is the copies' life, not their path.** On seed 1 the
  split copies are made at the same rate as on main (796 347 failed
  hand-overs whose receiver held the message, against 767 407), and
  under this rule each lives until its own search ends: back up its
  parents to the origin, 341 drops for want of a route against 4, or at
  the time to live, 68 874 against 73 956. On the way it meets a
  crossed relay 1.6 M times and is answered seen, a full DATA frame and
  an ACK each time: the DATA airtime of the run is 995 798 s against
  main's 967 223 s and D89's 100 248 s. Fewer copies reach the hop cap
  (2 054 against 22 474): a copy that backtracks spends counts slower
  than one that wanders. The result is main's storm with the copies
  moving less and transmitting as much.
- **Seed 7 is worse than main, not better.** D89 cut its airtime by a
  quarter by ending the copies that were there below the edge; seen
  keeps them, and the seen exchanges add 8 % to the airtime for the
  same delivery.
- **Under load the backtrack is paid in full.** S4 at three times the
  rate loses eight to twelve points against main. A search that is
  answered seen goes on through its own candidates, every one a
  hand-over on channels already full; main's copy, taken at the crossed
  relay, restarted nearest the destination and often found the owner
  from there. Hand-overs held back by the bucket rise a fifth, texts
  waiting at their origin a fifth to a third, and texts lost at the
  time to live a fifth.
- **At the design rate and on the walkers it is neutral in delivery
  and costs airtime.** The grid is unchanged to the tenth of a point;
  the walkers lose nothing and spend 12 % more airtime, 230 474 copies
  answered seen where D89 absorbed 5 165 and main took 153 295.
- **The mesh is nearly whole.** The burst delivers 98 of 100 where D89
  delivered 86: the search visits what it should. The two texts it did
  not deliver in time stalled on a faint neighbour (below), and the
  burst costs 18 % more airtime than main, with two and a half times
  the hand-overs that failed with the receiver holding the message,
  the stalls' retries. One text at a time, the spread run is main's to
  the frame.
- **The return to a parent that gave the child up** ended four copies
  in all the runs (`ReturnSeen`), none the last of its text.

## What it revealed

**A search that exhausts a relay's good neighbours sits on its faint
one.** The traced mesh burst's text #29 (a leaf of R4 to a leaf of
R2's) crossed 21 relays in 13 s, answered seen at every relay it met
again, and reached R2 with every other neighbour of R2 tried. R2's last
candidate was R4, heard at −112 dBm, a neighbour without margin (spec
"Next hop"), which never received a DATA frame from R2 while every one
of R2's heartbeats and ACKs reached it. R2 sent three frames, counted a
failure, banned R4, waited the ban, sent three more, demoted it, and
then, with no candidate but a banned one, waited for each ban in the
ladder and sent three frames more at its end, 1.58, 3.16, 6.3, 12.7,
25.3, 50.6 and 61 s apart, until the time to live ended the text at
1735 s, 537 s after it reached R2. It never returned the message to
R15, its parent, because a neighbour that is banned and not tried keeps
a message waiting (spec "Overload waits", roadmap D42), and R4's entry
never expired, since R2 heard it. Four relays of the burst spent 11 to
13 failed hand-overs each this way.

The rule is main's, and the trap is main's: the same relay with the
same faint neighbour sits the same way on main. What changes is how
often a search gets there. On main a copy that met a crossed relay was
taken and searched on from that relay's record, nearest the destination
first: a restart that often found the owner before any relay's good
neighbours ran out. Seen, the copy backtracks through its own
candidates, as the depth-first search says, and the backtrack passes
through "every good neighbour tried" at each relay on the way, where a
faint neighbour is the last resort. The spec's bound for a neighbour
that does not answer, 11 failed hand-overs before its entry expires at
390 s, assumes its heartbeats stop; a faint one's do not.

Two ways out, neither taken here, both the user's to decide: a relay
with a parent and no candidate left but a demoted one could return the
message instead of waiting for the ban, which spec "Next hop" argues
against for a line, where that demoted neighbour may be the only way on
and the demotion lapses; or a neighbour without margin could be given
fewer attempts. Both change roadmap D42, not this rule.

## What it does not show

- **Parts 2 to 4 of the storm candidate**, alone or with this one.
- **Other seeds of `dense-power.toml`**, and the mesh at more than two
  seeds.
- **The board.** Nothing here was run on hardware.

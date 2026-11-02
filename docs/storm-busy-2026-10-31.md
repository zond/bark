# A busy channel never moves a message

**Date:** 2026-10-31; the runs finished 2026-10-31 and 2026-11-02 and
the write-up on 2026-11-02. **Status:** measured; roadmap D96, the
user decides. Spec references are to `protocol.md`. Part 3 of the
four-part candidate of `docs/storm-2026-10-30.md`, built alone on main
`09b7d73` (D90 adopted, D91 and D92 merged) and measured against it;
all release builds, the D90 note's scenario files and seeds plus
three, main's binary carrying the simulator's new give-up counter and
nothing else (its outputs are main's to the byte but for the counter
on the ring and the four mesh runs, checked).

## Summary

- **The rule.** Once the checks of one attempt have found the
  receiver's channel busy for 500 ms, the hand-over keeps its
  receiver and its transmissions so far, waits the retry window of
  those transmissions, one unit before any, and listens again for a
  fresh 500 ms. Nothing is counted against the receiver and it is not
  set aside: what moves a message to another neighbour is an answer,
  a refusal or seen, or three transmissions unanswered. The time to
  live ends the wait, as it ends every wait; the hand-over never
  passes the neighbour over for a busy channel.
- **Main at this base does not storm on seed 1.** `dense-power.toml`
  seed 1 on `09b7d73` delivers 97.8 % of the settled hour with
  299 754 s of airtime, where main at the D90 base delivered 89.3 %
  with 1 162 844 s: D91 and D92 moved the seed off the edge. Below the
  edge a listen-before-talk give-up is rare, 64 in the run's two
  hours over 20 000 nodes, 38 in S4 at the design rate and 650 to
  2 100 at three times it, so the rule has almost nothing to act on
  there and changes those runs within their seed spread.
- **The branch storms on seed 1 and main does not:** 88.6 % delivered,
  1 388 224 s of airtime, 264 088 give-ups that waited 45 442 s in
  all, 0.17 s each. Seed 7 is unchanged, 98.0 → 98.2 %. The two
  seed-1 runs part at the first give-up, which draws a random wait
  main does not draw, and differ by minute 30; the branch tips at
  minute 50 to 55. Seeds 2 to 4, run to tell the edge from the rule:
  the edge, not the rule. Seed 2 storms on main and not on the branch (94.1 → 98.2 %, 465 201 → 309 490 s, 43 559 → 104 give-ups), seed 3 on neither (97.8 → 97.9 %); each build storms on one seed of three. Seed 4 was still running at the write-up.
- **At twice the design rate, where both builds storm, the rule is
  worth a point:** seeds 1 and 7, 75.5 / 77.5 → 76.4 / 78.4 %
  delivered in the settled hour, 2.6 % less airtime, 8 % fewer copies
  and 9 % fewer texts ended by the time to live, 8 % fewer hand-overs
  held back by the bucket, with 505 248 and 386 411 give-ups waiting
  86 708 and 66 272 s. The split rate, the hand-overs that fail with
  the receiver holding the message, does not fall anywhere: 24.0 →
  28.7 % on seed 1, 27.3 → 27.4 % at twice the rate. The splits are
  lost ACKs, not give-ups, as the storm note's trace said.
- **Elsewhere it is neutral, and the walkers pay 4 % airtime.** S4 at
  three times the rate, seeds 1 / 2 / 3: 51.3 / 52.1 / 48.8 → 53.7 /
  49.9 / 48.4 %, within the spread; the design rate 93.4 → 93.6 %; the
  ring and the four mesh runs identical or within a frame; the
  walkers 96.4 → 96.4 % at 184 940 → 192 694 s. The interferer
  scenario has no give-up on either build: the relays move off the
  carrier's channel before a sender waits 500 ms for it.
- **It fixes a drop.** On main a sleepy node whose anchor's channel is
  busy for 500 ms set its anchor aside, found no candidate and dropped
  its own message for want of a route half a second after the
  application gave it, with no word to the application; under the
  rule it waits (`a_sleepy_node_waits_for_its_anchor_s_channel`).
- **Recommendation:** adopt, on this base. The rule is the user's principle built as stated, costs nothing where give-ups are rare, is worth a point and 2.6 % of airtime where both builds storm, and removes a drop of a sleepy node's own message. It does not lower the split rate, which is lost ACKs, and it does not end a storm; nor does it start one: over three seeds at the design rate each build storms on one, main on seed 2 and the branch on seed 1, which is the edge. The queue a waiting hand-over holds is part 4's to free. The same measurement on the D93 base (main `8ff3a74`, the take final at the ACK) was queued at the write-up and decides the row; the branch is rebased onto it.

## The rule

The user's rule of 2026-10-30: a listen-before-talk that gives up is
not a lost ACK; it says nothing about the neighbour. Built as:

- **An attempt is a transmission.** Once the checks of one attempt
  have found the receiver's channel busy for 500 ms, the hand-over
  gives the radio up for a retry wait and then listens again for the
  same neighbour, with the transmissions it has made and a fresh
  500 ms. Nothing is counted against the neighbour, it is not set
  aside, and the message is not routed again. What moves a message to
  another neighbour is an answer, a refusal or seen, or three
  transmissions unanswered (spec "Hand-over" step 2 and the rule after
  step 6).
- **The wait** is the retry wait's window for the transmissions made
  so far, uniform in [0, 2^k × 155.5 ms] with k the transmissions, one
  unit before any: 0–155.5 ms, then 0–311 ms, then 0–622 ms; 389.8 ms
  the unit for a frame with the long preamble (spec "Retry wait"). The
  0–50 ms backoff waits for a frame to end; a channel busy for 500 ms,
  longer than any frame (372.8 ms with the long preamble), is held by
  one hand-over after another, so the wait is in their unit and grows
  with the attempts as the retry wait does, so that senders waiting for
  the same channel come apart.
- **What bounds the wait.** The time to live: the message is dropped
  where it waits when its time is gone, as it is for a ban or the
  bucket (spec "Time to live"). The duty cycle cap does not bound it,
  since a waiting message spends no airtime. The hand-over never passes
  the neighbour over for a busy channel, however long the channel
  stays busy: no number derived from the radio or the regulation says
  when a channel will not clear, the sender's own busy estimate of the
  channel is a 30-minute average and not a forecast, and a neighbour
  that hears the carrier too moves off its channel by the move rule
  (spec "Channel moves"), after which the hand-over follows it there,
  as every attempt reads the neighbour's home afresh. A channel busy
  at the sender and not at the neighbour, a carrier hidden from the
  neighbour, is the case the rule does not reach: messages for that
  neighbour wait at the sender until their time runs out, and the send
  queue behind them waits with them; part 4 of the candidate, a wait
  that is never idle, is what lifts the second.
- **The set-aside set** holds only neighbours that refused the message
  (spec "Bans") or that a failure demoted (step 6). A refusal still
  moves the message at once: it is an answer, and says the receiver
  does not take the packet.
- **Returns and deliveries** are as before: a give-up counts as one of
  their three attempts, since they have no other candidate. That a
  return's three give-ups on a busy channel still drop it (spec
  "Return to parent") is the same principle unapplied, and is left to
  the return rule, D95.

The simulator counts the give-ups per node and sums the waits they
cause (`lbt_gave_up`, `busy_wait_us`, from the core's new
`Trace::ChannelBusy`); on main the trace fires at the give-up with a
wait of zero, the next hop being chosen at once.

Tests: `a_busy_channel_keeps_the_hand_over_on_the_same_neighbour`,
`a_refusal_after_a_busy_channel_moves_the_message`,
`the_time_to_live_ends_a_message_waiting_for_a_channel`
(`bark-proto/tests/routing.rs`),
`a_sleepy_node_waits_for_its_anchor_s_channel` (`tests/sleepy.rs`), and
the hand-over's own
`a_busy_channel_keeps_the_receiver_and_waits_the_retry_window`
(`node/transfer.rs`).

## Why

From `docs/storm-2026-10-30.md`: the storm is a chain reaction of split
copies, and a split is made whenever a sender gives up on a receiver
that took the message and offers it to its next candidate. Spec
"Hand-over" step 2 made one such split by rule: a give-up on a busy
channel set the neighbour aside and chose the next hop at once, on a
first attempt and on a retry alike, so a retry's give-up after a lost
ACK offered the message to a second neighbour while the first held it,
exactly when the channel was fullest. In the storm's hottest 1 000 s
the give-up fired 90 877 times on a first attempt and 87 871 on a
retry, 20 924 of the retries with the receiver having ACKed.

The channel is heard at the sender and says nothing about the
neighbour: not that it lacks the message, which a transmission may
have brought it, nor that it is full or gone. Passed over before any
transmission, the message takes the longer way round on the same full
channels, which is what the ban after silence keeps it from (roadmap
D42); passed over after one, it is split. The same holds for the
sleepy node's own message, whose only candidate is its anchor: on main
a give-up on the anchor's channel set the anchor aside, the next-hop
choice found no candidate and the message was dropped for want of a
route half a second after the application gave it, with no word to the
application (`a_sleepy_node_waits_for_its_anchor_s_channel` fails on
main's `forward.rs` with that drop).

What the rule costs is the queue: a relay runs one hand-over at a
time, and a hand-over that waits for a channel holds the queue as a
retry wait does, and listens for that channel 500 ms at a time, deaf
to its home for each check. In a storm this is the cost to measure;
on a channel held by a foreign carrier the neighbour cannot hear, it
is the whole cost.

## How it was measured

Every run is the scenario file at the seed named, with `--json` and
`--messages`; the figures are the JSON summary's: delivered of the
texts sent in the settled hour and of all texts, latency, the run's
airtime, the cores' counters summed over the nodes, drops and texts
lost by reason, the relays whose load sample ever found the bucket
under its reserve, and the largest settled airtime of a node; the
minutes and the frames per text from the per-message file. Main is
`09b7d73` with the counter commit `be8d792` on top; the branch is
`3130ee0`, the rule, with the tests of `cc57bc5`. The three-times-rate
S4 is `s4-capacity.toml` with `per_leaf_per_hour` 2.875. Three runs
are beyond the D90 note's list: `s3-interferer.toml` seed 1, a
handheld transmitter at the block's centre on 20 s in 60 on channel 6
from 1800 s, for the case the rule does not reach; and
`dense-power.toml` at twice its rate, `per_leaf_per_hour` 1.9166667,
seeds 1 and 7, since main at this base no longer storms at the design
rate and the rule acts only where channels are full. Seeds 2 to 4 of
`dense-power.toml` were added when seed 1 stormed on the branch alone.
The outputs are under the scratchpad's `storm3/`.

## The tables

Main / the branch.

| | dp1 | dp7 | s4x1 | s4x3-s1 | s4x3-s2 | s4x3-s3 | ring | movers |
|---|---|---|---|---|---|---|---|---|
| texts sent in the settled hour | 18234 / 18188 | 18189 / 18190 | 17459 / 17457 | 47738 / 47964 | 47423 / 47227 | 47770 / 47714 | 2919 / 2919 | 17894 / 17893 |
| delivered, settled hour % | 97.8 / 88.6 | 98.0 / 98.2 | 93.4 / 93.6 | 51.3 / 53.7 | 52.1 / 49.9 | 48.8 / 48.4 | 91.4 / 91.4 | 96.4 / 96.4 |
| delivered, whole run % | 96.5 / 90.2 | 96.9 / 97.0 | 93.4 / 93.5 | 58.5 / 60.8 | 59.2 / 58.1 | 56.0 / 55.8 | 91.7 / 91.7 | 95.5 / 95.7 |
| latency p50 s | 10.4 / 37.7 | 10.5 / 10.4 | 1.4 / 1.4 | 158.5 / 144.1 | 161.8 / 165.7 | 197.4 / 198.7 | 58.5 / 58.5 | 5.5 / 5.4 |
| latency p90 s | 46.0 / 344.6 | 45.1 / 45.7 | 187.9 / 189.7 | 693.2 / 676.4 | 688.1 / 695.6 | 750.2 / 749.1 | 145.8 / 145.8 | 57.3 / 58.6 |
| airtime, whole run s | 299754 / 1388224 | 305040 / 296966 | 44213 / 43896 | 169524 / 165584 | 167990 / 169746 | 187758 / 186744 | 1817 / 1817 | 184940 / 192694 |
| hand-overs taken | 438601 / 2216342 | 449952 / 443622 | 116275 / 116324 | 391541 / 392143 | 383271 / 383479 | 393290 / 389338 | - / - | 322312 / 330815 |
| hand-overs failed | 525536 / 3408956 | 544701 / 521978 | 19377 / 19065 | 184900 / 178666 | 176942 / 182847 | 220940 / 221278 | - / - | 287084 / 306719 |
| failed, receiver had it | 125885 / 976696 | 128604 / 122216 | 4009 / 3772 | 44738 / 42656 | 43576 / 45082 | 58825 / 58234 | - / - | 154968 / 171624 |
| returns taken from a child | 3631 / 24862 | 3812 / 3489 | 75 / 67 | 1020 / 908 | 1006 / 1097 | 1274 / 1214 | - / - | 3340 / 3676 |
| copies answered (not returns) | 270245 / 2027488 | 272955 / 254930 | 7888 / 6769 | 57007 / 53916 | 71749 / 71823 | 71700 / 70178 | - / - | 212062 / 234873 |
| of those merged (case 4) | 2602 / 95797 | 2671 / 2462 | 28 / 24 | 2445 / 2168 | 2344 / 2513 | 3711 / 3502 | - / - | 1532 / 1776 |
| of those seen (D90) | 267568 / 1931492 | 270206 / 252385 | 7860 / 6745 | 54562 / 51745 | 69405 / 69310 | 67987 / 66673 | - / - | 210500 / 233070 |
| seen ACKs sent:acted on | 302116:255524 / 2225132:1776077 | 305720:257736 / 284827:241064 | 8470:7764 / 7172:6682 | 60927:53031 / 57823:50397 | 76821:67753 / 76890:67580 | 76340:65702 / 74741:64402 | 0:0 / 0:0 | 240184:200376 / 265159:221890 |
| listen-before-talk give-ups | 64 / 264088 | 104 / 94 | 38 / 38 | 773 / 883 | 648 / 782 | 2066 / 2043 | - / - | 23 / 18 |
| waits after give-ups, s | 0 / 45442 | 0 / 16 | 0 / 5 | 3 / 128 | 4 / 114 | 3 / 313 | - / - | 0 / 2 |
| split rate: failed with the receiver holding it, % of failed | 24.0 / 28.7 | 23.6 / 23.4 | 20.7 / 19.8 | 24.2 / 23.9 | 24.6 / 24.7 | 26.6 / 26.3 | - / - | 54.0 / 56.0 |
| hand-overs held back by the bucket | 24678 / 2027903 | 20288 / 20244 | 64 / 91 | 1007236 / 945136 | 984141 / 1009565 | 1165803 / 1165230 | - / - | 9030 / 12404 |
| neighbours demoted | 217639 / 1383108 | 226798 / 217425 | 7164 / 7110 | 53889 / 51785 | 50871 / 52878 | 65648 / 65611 | - / - | 110218 / 116892 |
| copies dropped: hop cap | 1488 / 2306 | 1584 / 1462 | 37 / 28 | 26 / 21 | 59 / 50 | 51 / 43 | 0 / 0 | 1550 / 1796 |
| copies dropped: TTL | 792 / 87759 | 741 / 683 | 340 / 329 | 7174 / 6826 | 6533 / 6831 | 8261 / 8221 | 0 / 0 | 970 / 1014 |
| copies dropped: no route | 141 / 393 | 155 / 126 | 265 / 240 | 1015 / 1044 | 1058 / 1089 | 1233 / 1255 | 26 / 26 | 114 / 106 |
| copies dropped: return failed | 49 / 186 | 48 / 41 | 0 / 0 | 3 / 6 | 13 / 5 | 9 / 4 | 3 / 3 | 36 / 37 |
| texts lost: hop cap | 237 / 213 | 219 / 213 | 30 / 26 | 16 / 16 | 31 / 30 | 29 / 26 | 0 / 0 | 161 / 148 |
| texts lost: TTL | 98 / 981 | 83 / 83 | 322 / 316 | 6603 / 6390 | 5965 / 6336 | 7431 / 7425 | 0 / 0 | 114 / 118 |
| texts lost: no route | 6 / 27 | 8 / 6 | 13 / 17 | 95 / 85 | 98 / 118 | 130 / 138 | 26 / 26 | 7 / 6 |
| texts lost: return failed | 5 / 15 | 5 / 5 | 0 / 0 | 2 / 4 | 8 / 4 | 4 / 1 | 2 / 2 | 1 / 1 |
| texts that waited at their origin | 50 / 1942 | 65 / 56 | 180 / 188 | 242780 / 229589 | 247222 / 249370 | 277036 / 277468 | 2474 / 2474 | 129 / 94 |
| relays ever at their cap | 108 / 1676 | 106 / 101 | 0 / 0 | 188 / 182 | 175 / 186 | 220 / 217 | - / - | 171 / 220 |
| largest settled airtime of a node % | 4.94 / 9.46 | 4.74 / 4.63 | 5.65 / 5.71 | 9.98 / 9.89 | 9.95 / 9.99 | 9.99 / 9.90 | 9.98 / 9.98 | 5.11 / 5.05 |
| heartbeats received % | 84.9 / 66.0 | 84.7 / 84.9 | 94.4 / 94.4 | 87.7 / 87.9 | 88.1 / 87.7 | 86.2 / 86.2 | 90.6 / 90.6 | 89.4 / 89.2 |

The random mesh at seeds 1 and 5 (`mesh-burst.toml`, `mesh-spread.toml`;
at seed 5 one relay is out of reach and its texts count as
unreachable), the interferer, and `dense-power.toml` at twice the rate:

| | burst-s1 | burst-s5 | spread-s1 | spread-s5 | s3 | dpx2-s1 | dpx2-s7 |
|---|---|---|---|---|---|---|---|
| texts sent in the settled hour | 100 / 100 | 100 / 100 | 90 / 90 | 90 / 90 | 57 / 57 | 36323 / 36365 | 36393 / 36408 |
| delivered, settled hour % | 100.0 / 100.0 | 92.0 / 92.0 | 100.0 / 100.0 | 92.2 / 92.2 | 100.0 / 100.0 | 75.5 / 76.4 | 77.5 / 78.4 |
| delivered, whole run % | 100.0 / 100.0 | 92.0 / 92.0 | 100.0 / 100.0 | 92.2 / 92.2 | 94.1 / 94.1 | 79.5 / 80.6 | 80.6 / 81.4 |
| latency p50 s | 3.7 / 3.4 | 1.9 / 1.9 | 0.6 / 0.6 | 0.6 / 0.6 | 0.5 / 0.5 | 121.5 / 116.6 | 108.9 / 105.5 |
| latency p90 s | 6.7 / 6.8 | 4.9 / 4.9 | 0.8 / 0.8 | 0.8 / 0.8 | 624.7 / 624.7 | 463.9 / 458.8 | 456.7 / 452.9 |
| airtime, whole run s | 232 / 234 | 263 / 263 | 183 / 183 | 228 / 228 | 274 / 274 | 2254044 / 2195672 | 2044160 / 1990824 |
| hand-overs taken | 629 / 630 | 565 / 565 | 469 / 469 | 504 / 504 | 131 / 131 | 4745825 / 4647878 | 4341377 / 4242423 |
| hand-overs failed | 78 / 81 | 165 / 165 | 0 / 0 | 134 / 134 | 19 / 19 | 5084750 / 5055096 | 4626425 / 4591138 |
| failed, receiver had it | 40 / 41 | 113 / 113 | 0 / 0 | 101 / 101 | 0 / 0 | 1388977 / 1383261 | 1244467 / 1232558 |
| returns taken from a child | 2 / 2 | 22 / 22 | 0 / 0 | 25 / 25 | 0 / 0 | 37843 / 32273 | 31715 / 27307 |
| copies answered (not returns) | 168 / 180 | 359 / 359 | 72 / 72 | 321 / 321 | 0 / 0 | 3030775 / 2888319 | 2639842 / 2520103 |
| of those merged (case 4) | 0 / 0 | 3 / 3 | 0 / 0 | 2 / 2 | 0 / 0 | 161583 / 147252 | 134814 / 123908 |
| of those seen (D90) | 168 / 180 | 356 / 356 | 72 / 72 | 319 / 319 | 0 / 0 | 2868775 / 2740633 | 2504629 / 2395827 |
| seen ACKs sent:acted on | 173:166 / 185:177 | 373:349 / 373:349 | 72:72 / 72:72 | 323:318 / 323:318 | 0:0 / 0:0 | 3275944:2662365 / 3152316:2553401 | 2852919:2331934 / 2742493:2238274 |
| listen-before-talk give-ups | 1 / 1 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 492656 / 505248 | 389217 / 386411 |
| waits after give-ups, s | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 57 / 86708 | 47 / 66272 |
| split rate: failed with the receiver holding it, % of failed | 51.3 / 50.6 | 68.5 / 68.5 | - / - | 75.4 / 75.4 | 0.0 / 0.0 | 27.3 / 27.4 | 26.9 / 26.8 |
| hand-overs held back by the bucket | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 5043717 / 4611268 | 4454484 / 4115600 |
| neighbours demoted | 15 / 17 | 29 / 29 | 0 / 0 | 22 / 22 | 6 / 6 | 1957881 / 1960549 | 1796770 / 1799207 |
| copies dropped: hop cap | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 2894 / 2851 | 2836 / 2837 |
| copies dropped: TTL | 1 / 1 | 6 / 6 | 0 / 0 | 3 / 3 | 3 / 3 | 219808 / 201774 | 190195 / 172763 |
| copies dropped: no route | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 869 / 851 | 874 / 884 |
| copies dropped: return failed | 0 / 0 | 2 / 2 | 0 / 0 | 2 / 2 | 0 / 0 | 224 / 232 | 256 / 231 |
| texts lost: hop cap | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 290 / 306 | 356 / 341 |
| texts lost: TTL | 0 / 0 | 6 / 6 | 0 / 0 | 3 / 3 | 3 / 3 | 6652 / 6062 | 6041 / 5686 |
| texts lost: no route | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 52 / 48 | 73 / 66 |
| texts lost: return failed | 0 / 0 | 1 / 1 | 0 / 0 | 2 / 2 | 0 / 0 | 23 / 30 | 38 / 38 |
| texts that waited at their origin | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 2 / 2 | 16996 / 17304 | 14863 / 15691 |
| relays ever at their cap | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 2348 / 2325 | 2027 / 1898 |
| largest settled airtime of a node % | 0.52 / 0.53 | 0.90 / 0.90 | 0.41 / 0.41 | 0.73 / 0.73 | 0.29 / 0.29 | 9.99 / 9.99 | 9.98 / 9.99 |
| heartbeats received % | 99.6 / 99.5 | 99.0 / 99.0 | 99.6 / 99.6 | 99.3 / 99.3 | 97.0 / 97.0 | 60.6 / 60.9 | 62.8 / 63.3 |

Seeds 2 and 3 of `dense-power.toml`, main / the branch (seed 4 was
still running at the write-up; its summaries are `storm3/main/dp4.json`
and `storm3/d96/dp4.json`):

| | dp2 | dp3 |
|---|---|---|
| delivered, settled hour % | 94.1 / 98.2 | 97.8 / 97.9 |
| airtime, whole run s | 465 201 / 309 490 | 283 426 / 279 774 |
| listen-before-talk give-ups | 43 559 / 104 | 44 / 38 |
| waits after give-ups, s | 5 / 18 | 0 / 5 |
| split rate, % of failed | 27.8 / 24.3 | 23.3 / 23.9 |

## What it shows

- **Below the edge the rule is idle.** A give-up needs a channel busy
  at every check for 500 ms; at 5 % busy, where `dense-power.toml`
  seed 1 runs on main, that happened 64 times in two hours, 104 at
  seed 7, 23 on the walkers, 38 in S4 at the design rate. With so few
  give-ups there is nothing for the rule to change, and the runs
  differ by the random draws that follow the first give-up: the ring
  and three mesh runs are identical to the frame, the burst at seed 1
  had one give-up and differs by a hand-over.
- **Seed 1 storms on the branch.** The branch's run is main's until
  its first give-up draws a wait; by minute 30 the two differ in a few
  deliveries, and from minute 50 the branch's frames per text go from
  86 to 362 to 2 247 while main's stay at 80 to 150. From there it is
  the storm of the storm note: 264 088 give-ups, 3.4 M failed
  hand-overs, 1 676 relays at their cap, heartbeats received 66 %,
  the median text 247 frames and the p90 3 059 against main's 50 and
  203. The give-ups' waits, 45 442 s in all over 20 000 nodes, are 0.17
  s each; what the storm costs is 1.1 M s of airtime. Whether the
  rule tipped it or the seed's edge did is what seeds 2 to 4 answer:
  the edge. On seed 2 it is main that storms, at half strength (94.1 %, 465 201 s, 43 559 give-ups) and the branch that does not (98.2 %, 309 490 s, 104 give-ups); on seed 3 neither does. The branch's run is main's until the first give-up draws a wait, so the two runs of a seed are two draws of one seed, and over seeds 1 to 3 each build storms once: at the design rate `dense-power.toml` sits on the edge, as D77's branch and main showed on 2026-10-30 and as D95 and D97 found on their branches, and which draw tips is the seed's, not the rule's.
- **Where both storm, the rule helps a little.** At twice the rate
  the branch delivers 0.9 points more on both seeds with 2.6 % less
  airtime, a tenth fewer texts and copies ended by the time to live
  and a twelfth fewer hand-overs held back by the bucket: a message
  that waits for a channel does not take the longer way round, so it
  spends less of the relays' buckets. Half a million give-ups waited
  87 000 s; the latency p50 fell 5 s.
- **The split rate does not fall.** On every run the share of failed
  hand-overs whose receiver held the message is main's: 24 → 29 % on
  seed 1's storm, 27.3 → 27.4 % at twice the rate, 24 % in S4 at
  three times it, 54 → 56 % on the walkers. These are ACKs lost on
  the sender's home channel after three transmissions, the bulk the
  storm note found (803 462 ACKed receivers among 4.45 M failures
  against 20 924 retry give-ups with an ACKed receiver in the hottest
  1 000 s). Part 3 closes the give-up's split; parts 2 and 4 and the
  lost ACKs are the rest.
- **Under load it is neutral; the walkers pay airtime.** S4 at three
  times the rate moves +2.4, −2.2 and −0.4 points over three seeds,
  with the hand-overs held back by the bucket −6 %, +3 %, 0 %: the
  spread of the seeds. The walkers deliver the same and spend 4 %
  more airtime, 7 % more hand-overs failing and 10 % more copies
  answered seen, with 18 give-ups in the run: not the rule's waits,
  but the different run that follows them.
- **The interferer does not reach the rule.** With a carrier on 20 s
  in 60 on one channel, the relays that hear it move off it and no
  sender ever found a channel busy for 500 ms: both builds are
  identical. The hidden-carrier case the rule does not reach, a
  carrier the sender hears and the neighbour does not, is not in any
  scenario.

## What it revealed

- **The storm's edge moved with D91 and D92, not with any storm
  rule.** The D90 note measured seed 1 at 89.3 % on `ab38727`; main
  `09b7d73`, two decisions later and with no change to the hand-over,
  delivers 97.8 %. The storm note said it of D77 already: seed 1 at
  the design rate is a seed on the edge, and the same code tips or
  not by its timing. A storm rule measured on seed 1 alone measures
  the edge.
- **Give-ups are a storm's symptom, not its cause.** 64 give-ups in a
  calm run against 264 088 in the storm, on the same seed and code
  but for the wait: the busy channel that makes a sender give up is
  the storm's own traffic. A rule on the give-up acts after the storm
  has begun, and the measurement at twice the rate says what it is
  worth then: a point of delivery and 2.6 % of airtime.
- **A sleepy node's own message was dropped by a busy channel.** The
  anchor is its only candidate, so the give-up's set-aside left it no
  route and the drop said nothing to the application. The rule removes
  the path; the test guards it.

## What it does not show

- **Parts 2 and 4 with this one.** The queue a waiting hand-over holds
  is part 4's to free; a take that is final only when its ACK has gone
  out is part 2's. Each was measured alone.
- **A carrier the neighbour cannot hear**, where a message waits at the
  sender for its whole time to live: no scenario has one.
- **The board.** Nothing here was run on hardware.

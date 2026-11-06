# Sending a sleepy node its message at once

**Status:** research note, 2026-11-06. Measures a relay sending a
message to the sleepy node with an anchor it is addressed to at once,
with the long preamble, where main leaves it to the node's anchor
(roadmap D134). Pending the user. Spec references are to
`protocol.md` by section name.

## Summary

The rule changes little where it was meant to help. In the city at ×1
and ×3 every figure is within what one seed can tell apart, for all
texts and for those to sleepy nodes with an anchor alike: on main the
anchor already sends such a text at once with the long preamble when its
bucket spares it (spec "Delivery", roadmap D112), so a relay that hears
the node saves only the hops to the anchor.

It helps where relays hear one another's sleepy nodes. In mesh burst and
spread mesh it delivers 99.70 and 99.89 % against 99.00 and 99.44 %, and
the lost texts' share of the window's air falls from 13.6 and 7.6 % to
2.6 and 1.0 %; in mesh burst latency p90 / p99 rises 6 / 12 → 7 / 16 s.
In the overloaded district it delivers about one point more, inside the
spread of three seeds.

The channel condition hurts the ring. The rule there never sends from a
relay that is not the anchor; the change is the condition applied to the
anchor's own sends outside the window, which keeps about a tenth of them off the
busiest channels. Delivered falls 97.73 → 96.99 %, by up to 5.2 points
on one seed, and p99 latency 303 → 431 s. Without the conditions, (d)
unconditional, the ring is as on main, and the district gains most.

## The rule

As spec "Next hop" states it on the branch: a sleepy neighbour with an
anchor is first for a message addressed to it while the relay **spares
the long frame**, and it is sent the message at once with the long
preamble:

- the relay is not sleepy (a sleepy node does not sweep, so it has no
  busy estimate);
- its bucket holds a full DATA frame with the long preamble, 372.8 ms,
  beyond its send queue, its delivery queue at the ordinary preamble and
  the reply reserve, 845 ms (spec "Duty cycle cap"): the condition the
  anchor already used for its sends outside the window (D112), with a
  full frame because the routing does not know the message's length;
- the node's home channel stays within the move threshold, 0.05, of the
  quietest channel in the relay's busy estimate, with that frame added
  (spec "Channel moves", "Busy estimate");
- no such hand-over to it has gone unanswered since it was last heard.

Otherwise it stays a last resort and the message goes on toward its
anchor's position. One hand-over unanswered in all its three attempts
marks the neighbour until its next beat; the message is routed again at
once, toward the anchor, and the failure counts as any other. The
anchor's own send outside the window (spec "Delivery") takes the same
two conditions; the owner of the position still delivers from its
delivery queue rather than hand over as a next hop.

`Relay::reach_now`, `Relay::spares_long`, `Relay::channel_spares`,
`Neighbour::unanswered`; tests `an_addressed_sleepy_node_is_sent_long_at_once`,
`a_busy_channel_leaves_a_sleepy_node_to_its_anchor`,
`a_spent_bucket_leaves_a_sleepy_node_to_its_anchor`,
`an_unanswered_sleepy_node_is_left_to_its_anchor`.

### The thresholds

The bucket condition is D112's, reused. The channel condition asks for
a threshold "at which the home-channel choice would leave a channel".
The only one is the move rule's, and it is relative: a relay moves when
its home is more than 0.05 busier than the quietest channel. It does
not compose with a frame's air as a load: the busy estimate is a
fraction of time over its window, 138 samples 13 s apart, 1794 s, so
one long frame adds 372.8 ms / 1794 s = 0.0002, a hundredth of a point,
and the condition is in effect "the node's channel is not one its
relays would leave". It refuses on a channel much busier than the
others, not on a band that is full everywhere. No absolute threshold
exists in the spec to use instead, and none was invented.

## How it was measured

As in `addressed-2026-11-06.md`, "How it was measured": the same
scenario files, seeds and windows, release builds. City ×4 was not run
for want of time. Builds:

| build | what |
|---|---|
| main | `389c30b` with the simulator's new counter (`7398dea`); it holds the owner's hand-over (`cb95095`), which the note's (c) figures did not, so main was run again |
| (d) | the branch, `24e9701` |
| (d) unconditional | (d) with both conditions taken out, at the relays and at the anchor: a relay sends to the addressed node at once unless it went unanswered since its last beat; the anchor sends outside the window as on main. Variant (a) of the addressed note for sleepy nodes with an anchor |

Every output was checked whole: the JSON parses with the scenario's
duration, every message row has every column, the run's resource line
is there. None was re-run; every run started after the root volume was
full that morning.

## Results

### All texts

| run | build | delivered of accepted % | lost at the hop cap, of accepted % | air of lost texts, of the window's texts' air / of all air in the run % | air per delivered text, s | to carried nodes: delivered %, lost at the cap %, median / p90 hops | latency p50 / p90 / p99, s |
|---|---|---|---|---|---|---|---|
| city ×1 | main | 98.38 | 0.14 | 3.1 / 12.8 | 1.95 | 98.89, 0.04, 8 / 14 | 3.9 / 22 / 511 |
| city ×1 | (d) | 98.41 | 0.13 | 2.7 / 12.4 | 1.96 | 99.11, 0.09, 8 / 14 | 4.1 / 21 / 482 |
| city ×1 | (d) unconditional | 98.32 | 0.13 | 2.9 / 12.4 | 1.94 | 98.71, 0.04, 8 / 14 | 4.0 / 21 / 521 |
| city ×3 | main | 98.66 | 0.08 | 2.0 / 8.4 | 2.14 | 99.23, 0.07, 8 / 15 | 8.0 / 30 / 549 |
| city ×3 | (d) | 98.54 | 0.07 | 1.7 / 7.8 | 2.15 | 99.09, 0.09, 8 / 15 | 7.8 / 29 / 546 |
| city ×3 | (d) unconditional | 98.66 | 0.08 | 1.9 / 7.7 | 2.14 | 99.18, 0.08, 8 / 15 | 7.9 / 31 / 562 |
| district ×32 | main | 83.30 (80.62–85.19) | 0.09 | 2.5 / 4.2 | 2.54 | 84.84, 0.00, 7 / 12 | 37.2 / 221 / 538 |
| district ×32 | (d) | 84.25 (80.07–86.82) | 0.03 | 0.9 / 2.1 | 2.43 | 84.42, 0.00, 6 / 11 | 36.6 / 218 / 523 |
| district ×32 | (d) unconditional | 85.21 (83.01–86.59) | 0.04 | 0.9 / 1.4 | 2.49 | 86.96, 0.00, 6 / 11 | 33.6 / 203 / 519 |
| S4 | main | 94.43 (93.73–95.37) | 0.12 | 4.0 / 3.1 | 1.12 | – | 1.4 / 192 / 992 |
| S4 | (d) | 94.42 (93.46–95.35) | 0.12 | 4.0 / 3.1 | 1.11 | – | 1.4 / 198 / 1003 |
| S4 | (d) unconditional | 94.46 (93.79–95.31) | 0.13 | 3.8 / 3.0 | 1.11 | – | 1.4 / 192 / 1000 |
| ring | main | 97.73 (95.93–99.80) | 0.00 | 0.0 / 1.4 | 0.43 | – | 26.4 / 121 / 303 |
| ring | (d) | 96.99 (94.56–99.30) | 0.00 | 0.0 / 1.1 | 0.42 | – | 27.0 / 129 / 431 |
| ring | (d) unconditional | 97.73 (95.93–99.80) | 0.00 | 0.0 / 1.4 | 0.43 | – | 26.4 / 121 / 303 |
| corridor | main | 100.00 (100.00–100.00) | 0.00 | 0.0 / 0.0 | 0.65 | – | 1.0 / 2 / 3 |
| corridor | (d) | 100.00 (100.00–100.00) | 0.00 | 0.0 / 0.0 | 0.65 | – | 1.0 / 2 / 3 |
| corridor | (d) unconditional | 100.00 (100.00–100.00) | 0.00 | 0.0 / 0.0 | 0.65 | – | 1.0 / 2 / 3 |
| mesh burst | main | 99.00 (94.00–100.00) | 0.00 | 13.6 / 4.8 | 0.89 | – | 2.6 / 6 / 12 |
| mesh burst | (d) | 99.70 (97.00–100.00) | 0.00 | 2.6 / 1.0 | 0.95 | – | 3.0 / 7 / 16 |
| mesh burst | (d) unconditional | 99.70 (97.00–100.00) | 0.00 | 2.6 / 1.0 | 0.95 | – | 3.0 / 7 / 16 |
| spread mesh | main | 99.44 (94.44–100.00) | 0.00 | 7.6 / 2.1 | 0.76 | – | 0.6 / 1 / 65 |
| spread mesh | (d) | 99.89 (98.89–100.00) | 0.00 | 1.0 / 0.3 | 0.71 | – | 0.6 / 1 / 15 |
| spread mesh | (d) unconditional | 99.89 (98.89–100.00) | 0.00 | 1.0 / 0.3 | 0.71 | – | 0.6 / 1 / 15 |

### Texts to sleepy nodes with an anchor

A text counts here if its destination was sleepy with an anchor when it
was written. Accepted texts, delivered, lost and latency are over the
whole run, not the window; the long DATA frames and the long preambles'
share of the air are a run's mean, the share over every channel and on
the channel where it is highest.

| run | build | accepted texts | delivered % | lost / in flight at the end % | latency p50 / p90 / p99, s | air per delivered text, s | long DATA to the node, a run: from its anchor, from other relays, frames (s) | long preambles, share of all air / of the busiest channel's % |
|---|---|---|---|---|---|---|---|---|
| city ×1 | main | 8905 | 97.25 | 0.27 / 2.48 | 4.0 / 31 / 690 | 2.07 | 13988 (4302), 151 (46) | 4.1 / 4.7 |
| city ×1 | (d) | 8905 | 97.23 | 0.22 / 2.55 | 4.0 / 30 / 673 | 2.08 | 13782 (4238), 1020 (314) | 4.2 / 4.7 |
| city ×1 | (d) unconditional | 8905 | 97.18 | 0.26 / 2.56 | 3.9 / 31 / 698 | 2.05 | 13758 (4231), 1156 (355) | 4.2 / 4.9 |
| city ×3 | main | 26332 | 97.78 | 0.11 / 2.10 | 8.1 / 57 / 751 | 2.30 | 104868 (32249), 958 (295) | 4.8 / 5.1 |
| city ×3 | (d) | 26326 | 97.62 | 0.11 / 2.27 | 7.9 / 63 / 762 | 2.27 | 101976 (31360), 5798 (1783) | 5.0 / 5.3 |
| city ×3 | (d) unconditional | 26329 | 97.80 | 0.13 / 2.08 | 7.9 / 63 / 773 | 2.27 | 103539 (31840), 5898 (1814) | 5.0 / 5.2 |
| district ×32 | main | 20784 | 78.35 | 0.31 / 21.34 | 45.3 / 268 / 601 | 2.61 | 10720 (3297), 1054 (324) | 5.1 / 7.2 |
| district ×32 | (d) | 20844 | 78.95 | 0.14 / 20.90 | 50.1 / 277 / 583 | 2.51 | 8860 (2725), 4906 (1509) | 6.5 / 8.7 |
| district ×32 | (d) unconditional | 20819 | 80.43 | 0.23 / 19.35 | 40.7 / 253 / 571 | 2.60 | 10152 (3122), 6976 (2145) | 7.9 / 10.0 |
| S4 | main | 174587 | 94.53 | 0.05 / 5.42 | 1.4 / 192 / 992 | 1.12 | 30473 (9371), 134 (41) | 35.2 / 61.2 |
| S4 | (d) | 174597 | 94.52 | 0.05 / 5.43 | 1.4 / 198 / 1003 | 1.11 | 30137 (9268), 356 (110) | 35.0 / 55.8 |
| S4 | (d) unconditional | 174589 | 94.56 | 0.05 / 5.38 | 1.4 / 192 / 1001 | 1.11 | 30418 (9354), 352 (108) | 34.8 / 57.4 |
| ring | main | 29169 | 97.73 | 0.04 / 2.22 | 26.4 / 121 / 303 | 0.43 | 307 (85), 0 (0) | 6.2 / 34.2 |
| ring | (d) | 28007 | 96.99 | 0.04 / 2.97 | 27.0 / 129 / 431 | 0.42 | 278 (77), 0 (0) | 5.7 / 15.5 |
| ring | (d) unconditional | 29169 | 97.73 | 0.04 / 2.22 | 26.4 / 121 / 303 | 0.43 | 307 (85), 0 (0) | 6.2 / 34.2 |
| corridor | main | 940 | 100.00 | 0.00 / 0.00 | 1.0 / 2 / 3 | 0.65 | 116 (32), 0 (0) | 34.9 / 65.0 |
| corridor | (d) | 940 | 100.00 | 0.00 / 0.00 | 1.0 / 2 / 3 | 0.65 | 116 (32), 0 (0) | 34.9 / 65.0 |
| corridor | (d) unconditional | 940 | 100.00 | 0.00 / 0.00 | 1.0 / 2 / 3 | 0.65 | 116 (32), 0 (0) | 34.9 / 65.0 |
| mesh burst | main | 1000 | 99.00 | 1.00 / 0.00 | 2.6 / 6 / 12 | 0.89 | 97 (28), 1 (0) | 15.8 / 46.2 |
| mesh burst | (d) | 1000 | 99.70 | 0.30 / 0.00 | 3.0 / 7 / 16 | 0.95 | 85 (24), 62 (18) | 20.2 / 45.3 |
| mesh burst | (d) unconditional | 1000 | 99.70 | 0.30 / 0.00 | 3.0 / 7 / 16 | 0.95 | 85 (24), 62 (18) | 20.2 / 45.3 |
| spread mesh | main | 890 | 99.44 | 0.45 / 0.11 | 0.6 / 1 / 65 | 0.77 | 86 (25), 0 (0) | 15.0 / 30.0 |
| spread mesh | (d) | 890 | 99.89 | 0.11 / 0.00 | 0.6 / 1 / 10 | 0.71 | 76 (22), 21 (6) | 16.0 / 42.4 |
| spread mesh | (d) unconditional | 890 | 99.89 | 0.11 / 0.00 | 0.6 / 1 / 10 | 0.71 | 76 (22), 21 (6) | 16.0 / 42.4 |

All air in the run, mean of a run, s:

| run | main | (d) | (d) unconditional |
|---|---|---|---|
| city ×1 | 185 714 | 184 984 | 186 014 |
| city ×3 | 808 661 | 800 506 | 796 215 |
| district ×32 | 71 727 | 67 058 | 67 981 |
| ring | 1 671 | 1 590 | 1 671 |
| mesh burst | 219 | 214 | 214 |

District ×32 and ring seed by seed, delivered of accepted %:

| run | main | (d) | (d) unconditional |
|---|---|---|---|
| district, seeds 1 to 3 | 84.02, 85.19, 80.62 | 86.82, 85.67, 80.07 | 85.99, 86.59, 83.01 |
| ring, seeds 1 to 10 | 96.60, 98.92, 97.69, 98.34, 97.26, 98.11, 95.93, 99.80, 97.84, 96.96 | 99.30, 98.92, 97.10, 98.97, 97.26, 94.93, 96.74, 94.56, 97.06, 95.54 | as main |

## What it shows

- **City.** Relays send a few more long frames to the node directly,
  1 020 against 151 a run at ×1 and 5 798 against 958 at ×3, and the
  anchor a few less; the long preambles' share of the air rises by
  0.1 to 0.2 points. Delivered, latency and air per text are within
  0.15 points, a few seconds and 0.03 s of main. One seed.
- **Meshes.** Where a relay hears another relay's sleepy nodes, the
  direct send delivers what main lost and cuts lost air by a factor of
  five; it costs mesh burst 0.06 s of air per delivered text and a few
  seconds at p90 and p99. The unanswered mark keeps the cost far below
  that of variant (a) of the addressed note (0.89 → 1.13 s, p90 6 → 14 s),
  which tried such a node again after every failure.
- **District ×32.** Overloaded, a fifth of its texts in flight at the
  end. (d) 84.25 % and (d) unconditional 85.21 % against 83.30 %,
  inside the seed spread; to sleepy nodes 78.95 and 80.43 against
  78.35 %. Fewer texts are lost at the cap, 0.09 → 0.03 %, and all air
  falls 6 to 7 %. The long preambles' share of the air rises 5.1 → 6.5
  and 7.9 %, on the busiest channel 7.2 → 8.7 and 10.0 %.
- **Ring.** No relay but the anchor hears a sleepy node, so only the
  anchor's sends change. The channel condition holds back about a tenth of them
  (278 against 307 a run) and halves the long preambles' share of the
  busiest channel's air, 34.2 → 15.5 %; those texts wait for the
  node's beat, and delivered falls 0.74 points with p99 latency 303 →
  431 s. Seeds differ by −5.2 to +2.7 points, so it is spread as well
  as loss, but the mean and the tail are worse.
- **S4, corridor.** As on main, within the seed spread.
- **The gates.** The runs do not count how often each condition
  refused; the difference between (d) and (d) unconditional, separate
  runs that drift apart, gives it roughly. At the relays, the conditions held back 12 % of the direct sends
  at ×1, 2 % at ×3 and 30 % in the district; in the ring and the meshes
  none. The channel condition is the one that acts in the ring, since
  the anchor's bucket condition is the same in all three builds.

## What it does not show

- City ×4, and more than one seed of the city.
- Which condition refused, bucket or channel, at a relay.
- The share of long preambles on the sleepy nodes' own channels
  alone: the share is of every channel, and of the busiest.
- What a long frame costs the sleepy node's battery when a relay
  other than its anchor sends it; the current is counted per node and
  was not compared.

## Verdict

Measured 2026-11-06, pending the user. In my view the rule does not
earn its place as written: the city, which it was meant for, does not
change, the meshes gain, and the channel condition costs the ring a
point and its tail. The condition is the weak part: the move threshold
is relative, so it refuses where one channel is busier than the others,
which in the ring is the channel where the anchor serves its own
nodes, and it never refuses a band full everywhere. (d) unconditional
does no worse than main beyond the seed spread anywhere measured and gains the meshes and the
district, at 2.8 points more of the district's air in long preambles.
If a form is adopted, that one is the better candidate, with the
bucket condition the anchor uses already; the city ×4 run should come
first.

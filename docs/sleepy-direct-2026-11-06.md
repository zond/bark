# Sending a sleepy node its message at once

**Status:** research note, 2026-11-06. Adds to spec "Next hop" a relay
sending a message to the sleepy node with an anchor it is addressed to
at once, with the long preamble, where it was left to the node's anchor
(roadmap D134). Adopted by the user on 2026-11-06 without a condition
on the node's channel; the form with one was measured and not adopted.
Spec references are to `protocol.md` by section name.

## Summary

On main, a relay that hears a sleepy node with an anchor sends a text
for it on toward its address, the anchor's position. The anchor sends it
at once with the long preamble when its bucket spares the frame (spec
"Delivery", roadmap D112), else in the node's window after its next
beat. With the rule the relay that hears the node sends it the text
itself, with the long preamble, under the same bucket condition.

Where relays hear one another's sleepy nodes it delivers what the search
lost: mesh burst 99.00 → 99.70 %, spread mesh 99.44 → 99.89 %, the lost
texts' share of the window's air 13.6 → 2.6 % and 7.6 → 1.0 %, at 0.06 s
more air per delivered text and p90 / p99 latency 6 / 12 → 7 / 16 s in
mesh burst. The overloaded district delivers 80.94 against 78.67 %,
more on each of three seeds. In the city at ×1 and ×4, one seed each,
delivered changes by +0.10 and −0.18 points, the tail of latency falls
at ×4 (p90 134 → 117 s, p99 690 → 640 s) and all air falls 2 and 4 %.
The ring, the corridor and S4 are as on main.

The long preambles' share of all air rises by 0.1 to 0.2 points in the
city and 5.1 → 7.4 % in the district.

## The rule

As spec "Next hop" states it: a sleepy neighbour with an anchor is first
for a message addressed to it while the relay **spares the long
frame**, and it is sent the message at once with the long preamble:

- the relay's bucket holds a full DATA frame with the long preamble,
  372.8 ms, beyond its send queue, the messages of its delivery queue
  at the ordinary preamble and the reply reserve, 845 ms
  (spec "Duty cycle cap"): the condition the anchor uses for its sends outside the
  window, with a full frame because the routing does not know the
  message's length;
- no such hand-over to the node has gone unanswered since it was last
  heard.

Otherwise it stays a last resort and the message goes on toward its
anchor's position. One hand-over unanswered in all its three attempts
marks the neighbour until it is heard again; the message is routed
again at once, toward the anchor, and the failure counts as any other.
The anchor's own sends are unchanged: the owner of the position still
delivers from its delivery queue (spec "Owner", "Delivery").

`Relay::reach_now`, `Relay::spares_long`, `Neighbour::unanswered`;
tests `an_addressed_sleepy_node_is_sent_long_at_once`,
`a_busy_channel_does_not_hold_a_sleepy_node_back`,
`a_spent_bucket_leaves_a_sleepy_node_to_its_anchor`,
`an_unanswered_sleepy_node_is_left_to_its_anchor`. The simulator
counts DATA with the long preamble to the node it is for, from its
anchor and from others (`sleep.long_to_node`).

## How it was measured

As in `addressed-2026-11-06.md`, "How it was measured": the same
scenario files, seeds and windows, release builds. City ×3 was not run
for the rule as adopted; the variants below were. Builds:

| build | what |
|---|---|
| main | `57afc9c` with the simulator's counter (`7398dea`) |
| the rule | the branch at `7f2e88e`, main `57afc9c` merged |

Main moved to `036d368` before the rule was merged; the move changed a
simulator test only. Every output was checked whole: the JSON parses
with the scenario's duration, every message row has every column, the
run's resource line is there.

## Results

### All texts

| run | build | delivered of accepted % | lost at the hop cap, of accepted % | air of lost texts, of the window's texts' air / of all air in the run % | air per delivered text, s | to carried nodes: delivered %, lost at the cap %, median / p90 hops | latency p50 / p90 / p99, s |
|---|---|---|---|---|---|---|---|
| city ×1 | main | 97.46 | 0.18 | 3.8 / 11.8 | 2.34 | 98.79, 0.05, 8 / 14 | 6.4 / 27 / 573 |
| city ×1 | the rule | 97.56 | 0.12 | 2.7 / 11.3 | 2.29 | 98.63, 0.05, 8 / 14 | 6.2 / 26 / 569 |
| city ×4 | main | 97.19 | 0.10 | 1.6 / 5.9 | 3.41 | 98.03, 0.05, 11 / 20 | 22.4 / 134 / 690 |
| city ×4 | the rule | 97.01 | 0.11 | 1.9 / 5.3 | 3.37 | 97.91, 0.07, 11 / 20 | 22.0 / 117 / 640 |
| district ×32 | main | 78.67 (76.62–79.96) | 0.11 | 2.4 / 3.6 | 2.59 | 78.64, 0.00, 6 / 12 | 43.3 / 270 / 588 |
| district ×32 | the rule | 80.94 (78.10–82.68) | 0.04 | 1.2 / 1.5 | 2.62 | 79.40, 0.00, 6 / 11 | 43.6 / 257 / 567 |
| S4 | main | 94.43 (93.73–95.37) | 0.12 | 4.0 / 3.1 | 1.12 | – | 1.4 / 192 / 992 |
| S4 | the rule | 94.47 (93.83–95.31) | 0.12 | 4.1 / 3.1 | 1.12 | – | 1.4 / 192 / 1003 |
| ring | main | 97.73 (95.93–99.80) | 0.00 | 0.0 / 1.4 | 0.43 | – | 26.4 / 121 / 303 |
| ring | the rule | 97.73 (95.93–99.80) | 0.00 | 0.0 / 1.4 | 0.43 | – | 26.4 / 121 / 303 |
| corridor | main | 100.00 (100.00–100.00) | 0.00 | 0.0 / 0.0 | 0.65 | – | 1.0 / 2 / 3 |
| corridor | the rule | 100.00 (100.00–100.00) | 0.00 | 0.0 / 0.0 | 0.65 | – | 1.0 / 2 / 3 |
| mesh burst | main | 99.00 (94.00–100.00) | 0.00 | 13.6 / 4.8 | 0.89 | – | 2.6 / 6 / 12 |
| mesh burst | the rule | 99.70 (97.00–100.00) | 0.00 | 2.6 / 1.0 | 0.95 | – | 3.0 / 7 / 16 |
| spread mesh | main | 99.44 (94.44–100.00) | 0.00 | 7.6 / 2.1 | 0.76 | – | 0.6 / 1 / 65 |
| spread mesh | the rule | 99.89 (98.89–100.00) | 0.00 | 1.0 / 0.3 | 0.71 | – | 0.6 / 1 / 15 |

District ×32 seed by seed, delivered of accepted %: main 79.96, 79.40,
76.62; the rule 81.91, 82.68, 78.10.

### Texts to sleepy nodes with an anchor

A text counts here if its destination was sleepy with an anchor when it
was written. Accepted texts, delivered, lost and latency are over the
whole run, not the window; the long DATA frames and the long preambles'
share of the air are a run's mean, the share over every channel and on
the channel where it is highest.

| run | build | accepted texts | delivered % | lost / in flight at the end % | latency p50 / p90 / p99, s | air per delivered text, s | long DATA to the node, a run: from its anchor, from other relays, frames (s) | long preambles, share of all air / of the busiest channel's % |
|---|---|---|---|---|---|---|---|---|
| city ×1 | main | 8905 | 96.08 | 0.38 / 3.54 | 6.4 / 42 / 703 | 2.46 | 13865 (4264), 240 (74) | 3.6 / 3.9 |
| city ×1 | the rule | 8905 | 96.33 | 0.24 / 3.44 | 6.3 / 41 / 691 | 2.45 | 13707 (4215), 1344 (413) | 3.7 / 4.3 |
| city ×4 | main | 34711 | 95.97 | 0.20 / 3.83 | 24.2 / 204 / 860 | 3.57 | 131113 (40320), 3840 (1181) | 2.8 / 3.0 |
| city ×4 | the rule | 34727 | 95.82 | 0.24 / 3.94 | 23.7 / 181 / 814 | 3.52 | 129619 (39860), 13502 (4152) | 3.0 / 3.2 |
| district ×32 | main | 19744 | 73.71 | 0.32 / 25.97 | 52.4 / 312 / 629 | 2.67 | 10221 (3143), 1151 (354) | 5.1 / 6.8 |
| district ×32 | the rule | 19978 | 76.37 | 0.16 / 23.47 | 52.3 / 298 / 624 | 2.69 | 9655 (2969), 6556 (2016) | 7.4 / 9.1 |
| S4 | main | 174587 | 94.53 | 0.05 / 5.42 | 1.4 / 192 / 992 | 1.12 | 30473 (9371), 134 (41) | 35.2 / 61.2 |
| S4 | the rule | 174590 | 94.57 | 0.05 / 5.38 | 1.4 / 192 / 1003 | 1.11 | 30412 (9352), 348 (107) | 35.1 / 56.6 |
| ring | main | 29169 | 97.73 | 0.04 / 2.22 | 26.4 / 121 / 303 | 0.43 | 307 (85), 0 (0) | 6.2 / 34.2 |
| ring | the rule | 29169 | 97.73 | 0.04 / 2.22 | 26.4 / 121 / 303 | 0.43 | 307 (85), 0 (0) | 6.2 / 34.2 |
| corridor | main | 940 | 100.00 | 0.00 / 0.00 | 1.0 / 2 / 3 | 0.65 | 116 (32), 0 (0) | 34.9 / 65.0 |
| corridor | the rule | 940 | 100.00 | 0.00 / 0.00 | 1.0 / 2 / 3 | 0.65 | 116 (32), 0 (0) | 34.9 / 65.0 |
| mesh burst | main | 1000 | 99.00 | 1.00 / 0.00 | 2.6 / 6 / 12 | 0.89 | 97 (28), 1 (0) | 15.8 / 46.2 |
| mesh burst | the rule | 1000 | 99.70 | 0.30 / 0.00 | 3.0 / 7 / 16 | 0.95 | 85 (24), 62 (18) | 20.2 / 45.3 |
| spread mesh | main | 890 | 99.44 | 0.45 / 0.11 | 0.6 / 1 / 65 | 0.77 | 86 (25), 0 (0) | 15.0 / 30.0 |
| spread mesh | the rule | 890 | 99.89 | 0.11 / 0.00 | 0.6 / 1 / 10 | 0.71 | 76 (22), 21 (6) | 16.0 / 42.4 |

All air in the run, mean of a run: city ×1 196 840 → 193 525 s, city ×4
1 732 844 → 1 670 006 s, district 69 956 → 68 515 s, mesh burst 219 →
214 s.

## The form with a channel condition

Measured first, on main `389c30b`: the rule with a third condition,
that the node's home channel stay within the move threshold of the
quietest channel, 0.05 (spec "Channel moves"), in the relay's busy
estimate with the frame added, applied to the anchor's sends outside
the window too. The move threshold was the only threshold for leaving a
channel, and it is relative; one long frame adds 372.8 ms over the
estimate's 1794 s, 0.0002, so the condition refused a channel much
busier than the others, never a band busy everywhere.

In the ring no relay but the anchor hears a sleepy node, so only the
anchor's sends changed. The condition held back about a tenth of them,
307 → 278 long frames a run, mostly on the busiest channel, where the
anchor serves its own nodes (the long preambles' share of that
channel's air 34.2 → 15.5 %), and those texts waited for the beat:
delivered 97.73 → 96.99 %, seeds from −5.2 to +2.7 points, p99 latency
303 → 431 s. Elsewhere it was within the spread of the form without it:
city ×1 98.41 against 98.32 %, ×3 98.54 against 98.66 %, district
84.25 against 85.21 %, the meshes the same. That form, without the
bucket condition at the relays too, delivered the same as main in the
ring. Not adopted.

## What it shows

- **Where relays hear one another's sleepy nodes**, as in the meshes,
  the relay sends the node its text and the search through them that
  lost texts no longer happens. The unanswered mark keeps the cost small
  against variant (a) of the addressed note (air per delivered text
  0.89 → 1.13 s, p90 6 → 14 s there, against 0.95 s and 7 s here),
  which tried such a node again after every failure.
- **District ×32.** Overloaded, a quarter of its texts to sleepy nodes
  in flight at the end. Delivered rises on each seed, by 1.5 to 3.3
  points; lost at the cap 0.11 → 0.04 %; texts to sleepy nodes 73.71 →
  76.37 %. Relays other than the anchor send 6 556 long frames a run
  against 1 151, and the long preambles' share of the busiest channel's
  air rises 6.8 → 9.1 %.
- **City.** Relays other than the anchor send 1 344 long frames against
  240 at ×1, and 13 502 against 3 840 at ×4, a tenth of the anchors'.
  Delivered is within 0.2 points of main, to sleepy nodes too; at ×4
  latency p90 / p99 falls 134 / 690 → 117 / 640 s, and all air falls 4 %.
- **Ring, corridor, S4.** As on main: in the ring and the corridor the
  same to the text, in S4 within the seed spread.

## What it does not show

- One seed of the city at each rate, and ×3 not for the rule as adopted.
- How often the bucket held a relay back; the runs do not count it.
- What a long frame from a relay other than its anchor costs the sleepy
  node's battery; it listens a hand-over after each text it takes,
  as after its anchor's.

## Verdict

Adopted by the user, 2026-11-06, without the channel condition (roadmap
D134).

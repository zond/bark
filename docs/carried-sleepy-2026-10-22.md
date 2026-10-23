# Sleepy and carried, measured apart

**Date:** 2026-10-22, roadmap D72. **Status:** measured, the decision is
the user's. Spec references are to `protocol.md`. The base is main
`dc7b031`, run as its own binary on the same scenario files.

The question, from the user's decision of 2026-10-22: there are no
relays as a kind of node. A node has two properties it measures of
itself, sleepy (on battery past the grace, D71) and carried (its GPS
says it moves, D70), each published as a heartbeat flag, and all four
combinations are real: a node on USB at a window, on USB in a car, on
battery left somewhere, on battery in a pocket. "Relay" stays as the word
for a node that is not sleepy, a state as "anchor" and "holder" are.
The flags are to describe the node, and the spec and the code are to
handle each the right way, with no new rules.

## Summary

- **Built:** every node measures whether it is carried by one rule, and
  its power changes only whether it is sleepy. A pocket node's beats
  carry 0xD, 0xF while it is anchorless. A carried node plugged in is a
  carried relay until four hours still; a fixed one is fixed where it is
  then, as before. A sleepy node's change of the flag heartbeats at once,
  as a relay's does, and opens its window.
- **Nothing measured moves beyond the seeds.** Delivered of the texts
  sent in the settled hour, main against the branch: S4 94.20 → 94.02 %
  and 93.89 → 94.47 %; `dense-power.toml` 98.09 → 98.03 % and 98.31 →
  98.29 %; `dense-movers.toml` (seed 1) 96.83 → 96.89 %. Texts to
  sleepy nodes 96.97 → 97.15 % and 97.10 → 97.12 % (`dense-power`),
  95.93 → 96.04 % (`dense-movers`).
- **What it costs is small:** a heartbeat at once for each sleepy node
  that turns carried, 1340 / 1296 frames and 16.6 / 15.3 s of airtime in
  S4's settled hour over the domain, 985 and 11.7 s in `dense-movers`.
  The beats that carry the flag are beats a node sends anyway.
- **What it changes where power switches:** a walker that turned sleepy
  in a long walk, or switched on mid-walk, stays carried when it is
  plugged in at its stop, and holds no mail there until it has stayed
  four hours: 41 / 54 such plugs in `dense-power`'s settled hour, no text
  queued at a carried node. Texts to walkers 98.45 → 98.15 % and 99.02
  → 98.99 %.

## What was built

| Commit | What |
|---|---|
| Simulator: count what sleepy nodes that move say, and whom texts go to | `research.carried` notes whether each node that turns carried or fixed is sleepy, and counts beats flagged sleepy and carried (`beats`), heartbeats for a sleepy node's change of the flag (`sleepy_changed`), nodes plugged in while carried (`plugged_in`) and texts queued at a carried node (`queued`); the messages CSV has `dst_sleepy` and `dst_carried` at the time of sending. A heartbeat for a change is followed at every node, not only under `[power]`, so that it never counts as a beat to take a fix at. |
| Core: sleepy and carried are measured apart, on every node | `relay/motion.rs`: one rule for every node, far fixes against the place the node stands (`still_at`), carried after 72 s of them with none near, fixed again after `fixed_after` within the allowance of one place; a sleepy or carried node takes each fix; only a node that holds mail flushes at a first far fix. `relay/power.rs`: turning sleepy keeps the motion; plugged in, a fixed node is fixed where it is (`fixed_here`). A map point that clears a sleepy node's carried flag heartbeats. |
| Core: a sleepy node's heartbeat for a change opens its window | On its anchor's channel the holder takes it for a beat and delivers at once, so the node listens its window after it. |
| Tests: the four combinations of sleepy and carried | `tests/carried_sleepy.rs`, and two tests of `tests/power.rs` (Checks). |
| Core: two comments | A node in a pocket, not a carried one, goes dark; an anchor is left when heard carried or sleepy. |
| Spec | "Nodes" rewritten around the two properties; "Position", "Heartbeat", "Neighbour table", "Sleepy nodes", "Sleepy nodes alone", "Phone link", "Packet formats"; `core-interface.md` §4. |

**How the simulator hands fixes,** checked before any run: every walker,
sleepy or not, is handed a fix at each heartbeat it sends, a relay's
every 5 to 15 s and a sleepy node's at each beat, at a stop as on a walk,
since the walker is a walker throughout (`note_beat_fix`, `Walks::walks`).
That is what the firmware does: a sleepy node refreshes its fix before
each beat (spec "Beats"). Nodes that do not walk are handed no fix, since
the error model is off in these scenarios and a true fix of a still node
changes nothing. Nothing about feeding had to change but one thing: under
no `[power]` a relay's heartbeat for a change was taken for a beat and
handed a fix too, which the first commit ends (it changes
`dense-movers.toml` against main; S4 and `dense-power.toml` repeat main to
the byte at that commit). Two things stay as they were: the fix is handed
right after the beat, not before it, so a beat carries the position of
the beat before; and the GPS error model (`[gps] error_m`) applies only to
nodes placed as relays.

With a fix a beat, a sleepy walker's 72 s of far fixes are its second
beat after it leaves the allowance: in S4 the run from the first far fix
to carried is p50 195 s, p90 278 s, against 76 / 82 s for a walking relay
with a fix a heartbeat.

## What was run

Main `dc7b031` (`bark-sim-main`), main with the simulator commit
(`9a5600a`, `bark-sim-base`, for the destination's role in the messages
table; S4 seed 1 and `dense-power.toml` seeds 1 and 2 repeat main to the
byte with it, the printed table but the new line and the wall time), and
the branch at `3b4d178` (`bark-sim-d72`), all `cargo build --release -p
bark-sim`. Each run 6300 s, the settled hour from 2700 s, delivered of
the texts sent in it, those still on their way at the end counted as not
delivered:

```
bark-sim SCENARIO --seed N --json OUT.json --messages OUT.csv > OUT.txt
```

The tables take the rows of `OUT.csv` with `t_s` from 2700 to 6300 s:
delivered is `fate` = `delivered`, by `dst_walking`, `dst_sleepy` and
`dst_carried`, and by whether `dst` is among `research.power.switchers`
of `OUT.json` (with its third field, a walker); airtime per delivered
text is the sum of `airtime_s` over those rows divided by the delivered;
latency is `latency_s` of the delivered. The rest is `research.carried`,
`research.power`, `airtime_s` and `sleep.current_ma` of `OUT.json`, and
the printed table.

| Run | Scenario | Seeds |
|---|---|---|
| main, branch | `scenarios/s4-capacity.toml` | 1, 2 |
| main (as base), branch | `scenarios/dense-power.toml`, grace 1 h as adopted | 1, 2 |
| base, branch | `scenarios/dense-movers.toml` | 1 |

## Results

### S4

S4's walkers are a fifth of the sleepy nodes. On main a sleepy node
never measured its motion; now each walker turns carried in its first
walk and, with stops of an hour, is not fixed again within the run.

| | main s1 | branch s1 | main s2 | branch s2 |
|---|---|---|---|---|
| delivered, all | 94.20 % (17 457) | 94.02 % (17 456) | 93.89 % (17 442) | 94.47 % (17 444) |
| to a node walking when sent | 82.17 % (544) | 82.57 % (545) | 80.42 % (475) | 82.11 % (475) |
| to a carried sleepy node | | 86.40 % (2353) | | 87.42 % (2131) |
| airtime per delivered text | 1.32 s | 1.35 s | 1.42 s | 1.39 s |
| latency p50 / p90 | 1.5 / 183.8 s | 1.5 / 183.2 s | 1.6 / 189.0 s | 1.6 / 186.6 s |
| all airtime over the run | 50 479 s | 52 098 s | 53 587 s | 51 464 s |
| sleepy node's radio current, mean (median, p90) | 0.631 (0.549, 1.127) mA | 0.630 (0.546, 1.126) mA | 0.637 (0.564, 1.133) mA | 0.634 (0.562, 1.126) mA |
| sleepy nodes turned carried, run (settled hour) | | 3230 (1340) | | 3060 (1296) |
| fixed again | | 0 | | 0 |
| beats flagged sleepy and carried, settled hour | | 54 649, 1401 s | | 51 294, 1210 s |
| heartbeats for a sleepy node's change of the flag | | 1340, 16.6 s | | 1296, 15.3 s |
| anchor changes after the settle time | 9140 | 9061 | 9123 | 8898 |

S4 is no longer the same to the byte as main: its walking sleepy nodes
now turn carried and say so with one heartbeat each, which opens a
window; from the first such frame the runs part. Nothing reads the
carried flag of a sleepy node's beat beyond what its sleepy flag says
already, so what changes is those frames and where the random draws fall
after them: one seed up 0.6 points, the other down 0.2, the total
airtime up 3 % and down 4 %.

### `dense-power.toml`

The D71 scenario at the adopted grace of an hour. Every walker's node is
on battery while it walks and on USB at its stops, so walkers are relays
but for the 67 / 66 whose walk outlasts the grace and those switched on
in a walk, which are sleepy from their boot (D71).

| | main s1 | branch s1 | main s2 | branch s2 |
|---|---|---|---|---|
| delivered, all | 98.09 % (18 234) | 98.03 % | 98.31 % (18 199) | 98.29 % |
| to a walker | 98.45 % (3730) | 98.15 % | 99.02 % (3568) | 98.99 % |
| to a switching node | 98.91 % (6601) | 98.59 % | 99.22 % (6565) | 99.16 % |
| to a sleepy node | 96.97 % (8921) | 97.15 % | 97.10 % (8888) | 97.12 % |
| to a relay | 99.15 % (9313) | 98.86 % | 99.47 % (9311) | 99.40 % |
| to a carried relay | 97.79 % (2446) | 97.53 % (2511) | 98.83 % (2220) | 98.69 % (2283) |
| airtime per delivered text | 4.45 s | 4.54 s | 4.55 s | 4.45 s |
| latency p50 / p90 | 9.3 / 34.0 s | 9.4 / 34.2 s | 9.6 / 31.8 s | 9.4 / 31.9 s |
| all airtime over the run | 225 966 s | 229 817 s | 230 994 s | 230 722 s |
| sleepy nodes turned carried, run (settled hour) | | 469 (0) | | 456 (0) |
| beats flagged sleepy and carried, settled hour | | 140, 1.7 s | | 293, 3.5 s |
| plugged in while carried, settled hour | | 41 | | 54 |
| texts queued at a carried node | | 0 | | 0 |
| heartbeats for a change, all nodes | 2024, 24.7 s | 1929, 23.5 s | 1990, 23.9 s | 1879, 22.6 s |
| graces ended; texts held then, dropped | 67; 3 | 67; 6 | 66; 5 | 66; 5 |
| flush frames | 64, 27.1 s | 83, 35.2 s | 49, 20.8 s | 59, 25.0 s |
| to a node as a relay | 1384, 95.5 s | 1341, 92.2 s | 1256, 86.4 s | 1204, 82.5 s |

"To a walker" is a text to a node whose power follows its walks, at any
time; "to a sleepy node", "to a relay" and "to a carried relay" go by the
destination's state when the text was sent. The sleepy nodes' radio
current in this scenario mixes nodes placed as leaves that are relays
for a while (D71) and is 2.35 mA in every run.

All 469 / 456 turns of a sleepy node to carried come in the boot half
hour, from walkers switched on in a walk; after the settle time no
sleepy node turns carried, and the 140 / 293 flagged beats are those of
walkers whose grace ended while they were carried. The 41 / 54 plugged in
while carried are walkers reaching a stop carried: main made them fixed
relays there at once, the branch keeps them carried relays, which hold
no mail and are nobody's anchor, for four hours. The heartbeats for a
change fall by about a hundred for the same reason: a walker that stays
carried at its stop does not turn carried again on its next walk.

### `dense-movers.toml`

`dense-movers.toml` has the walking relays of D70 and walking sleepy
nodes, as S4 has, and no power switching. The base is main with the
simulator commit, which here differs from main by the fix no longer
handed at a relay's heartbeat for a change.

| | base s1 | branch s1 |
|---|---|---|
| delivered, all | 96.83 % (17 894) | 96.89 % |
| to a node walking when sent | 75.74 % (577) | 73.83 % |
| to a sleepy node | 95.93 % (13 353) | 96.04 % |
| to a sleepy node walking when sent | 68.92 % (444) | 66.67 % |
| to a relay | 99.45 % (4541) | 99.41 % |
| to a carried sleepy node | | 85.81 % (1839) |
| airtime per delivered text | 3.04 s | 3.05 s |
| latency p50 / p90 | 5.1 / 28.8 s | 5.1 / 27.9 s |
| all airtime over the run | 155 654 s | 154 952 s |
| sleepy node's radio current, mean (median, p90) | 0.549 (0.462, 0.926) mA | 0.548 (0.461, 0.927) mA |
| relays turned carried, run (settled hour) | 826 (335) | 826 (335) |
| sleepy nodes turned carried, run (settled hour) | | 2421 (985) |
| beats flagged sleepy and carried, settled hour | | 42 286, 501 s |
| heartbeats for a sleepy node's change of the flag | | 985, 11.7 s |

The walking sleepy nodes' texts are the weak class, at 67 to 69 % on
either side: a sleepy node that walks leaves its anchor behind, and its
senders address the old one (D65). Being carried changes nothing of
that, since nothing reads the flag of a sleepy node's beat.

## The checks

- `cargo test --workspace`, `cargo clippy --workspace --all-targets`,
  `cargo fmt --check`, the core for `thumbv7em-none-eabihf`, and the
  firmware's clippy for its two images are clean. The timing test of
  `bark-sim/tests/scenarios.rs`, bounds unchanged, failed while four
  runs of the simulator shared the machine (debug 95 to 99 s against
  the 90 s bound); alone, at a load of about 2 from other work: debug
  57.9 s; release 19.67 and 20.21 s on the branch against main's
  19.95 s, both at the 20 s bound on this machine today. It is to be run
  again on an idle machine before a merge.
- `tests/carried_sleepy.rs`: each of the four combinations heartbeats its
  own type, 0x8, 0xC, 0x9, 0xD, and 0xF anchorless, and a neighbour keeps
  both flags; a sleepy node whose fixes walk for 72 s is carried, keeps its
  anchor, beats flagged at once on its home channel, and its holder
  delivers in the window after that heartbeat; a sleepy node is fixed
  again after staying, and says so; a carried sleepy node plugged in
  heartbeats 0xC, queues no text for a sleepy node it hears, is not
  chosen as an anchor by a sleepy node that hears it far the strongest,
  and once it has stayed is a fixed relay that queues; a still sleepy
  node plugged in is fixed at once where it is, with a run of far fixes
  begun or not; a map point fixes any node and says so; an anchorless
  node on the move says so with the long preamble. All seven fail on main.
- `tests/power.rs`: a relay carried in its grace turns sleepy carried,
  its beats flagged both, with no second flush; a change of the flag
  heartbeats at once on the home channel for a sleepy node as for a
  relay, and a carried node's fixes add none.

## Why

A heartbeat flag is how a node tells others what it is. D71 measured
keeping the carried state across a sleepy spell and left it out because
the carried flag would then appear in sleepy nodes' beats; but that is
the rule, not a cost: a node on battery in a pocket is sleepy and
carried, and a flag that is cleared by the other property no longer
describes the node. Measured again here, the difference is within the
seeds, as D71 found (98.03 against 98.09 % at an hour, seed 1, the same
figures this branch gives).

What the separation does: a carried relay unplugged stays carried while
it sleeps, and back on a charger in a car or at a stop is a relay that
routes but holds no mail until it has stayed four hours, which is D70's
reason for the slow way back. A sleepy node's carried flag is read by
nobody beyond what its sleepy flag says, so its cost is one heartbeat
when it changes. The window after that heartbeat is no new rule: the
heartbeat of a sleepy node with an anchor, on its anchor's channel, is a
beat to whoever hears it.

## What this does not show

- Where carried matters for a sleepy node: nothing reads it yet. If a
  later rule does, for instance a sender learning that its destination
  moves, it will be measured then.
- The fix after the beat rather than before it (above): a beat carries
  the position of the beat before, and a sleepy node turning carried
  sends its beat unflagged and the flagged heartbeat a moment after.
- GPS wander of a still sleepy node indoors: with a fix a beat, two far
  fixes a beat apart make it carried; the bench item O16 covers it.
- One seed for `dense-movers.toml`, two for the others.

## Recommendation

Adopt: it is the user's rule that each flag describes the node, it costs
a heartbeat a turn, and nothing measured moves beyond the seeds.

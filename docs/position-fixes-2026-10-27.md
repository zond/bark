# Three faults in where a node says it is

**Date:** 2026-10-27, finished 2026-10-30. **Status:** adopted
2026-10-30 as roadmap D83, D84 and D85. Spec references are to
`protocol.md`. Begun on main `7fff055` (D82), where the relay GPS work
(`relay-gps-2026-10-26.md`, "Found on the way") found all three.

## Summary

- **D83, a fresh board's first fix.** A node with no place takes its
  first accepted fix as its place, fixed, at once, and sends no heartbeat
  before it. Before, a board built without a place started at 0, 0, its
  first fix was a far fix, and it was carried for four hours. Reasoned;
  the simulator boots every node placed, so nothing there changes.
- **D84, the map point.** The app sets and clears it; no fix is taken
  while it is set; the next accepted fix after it is cleared is the
  place. Before, nothing could clear it. Reasoned; no scenario sets one.
- **D85, plug-in.** A fixed node plugged in takes its first accepted fix
  within a search's limit, 60 s, as its place. Before, it kept a sleepy
  node's last fix, which may be hours old, and turned carried at the
  place it had reached. Measured on `dense-power.toml`, seeds 1 to 7:
  delivery as main's within 0.2 points in five; the scenario's
  forwarding storm hits D85's seed 2 (95.56 % against main's 98.20 %)
  and main's seed 7 (91.94 % against D85's 98.24 %).

## 1. A fresh board's first fix (D83)

**Fault.** Spec "Position" says the place a fixed node stands is its
first accepted fix. The core had no "no place yet": it started at
`Config::position`, which the firmware sets from `BARK_LAT` and
`BARK_LON` at build time and otherwise to 0, 0. Measured from 0, 0 the
first fix is a far fix, so the node turned carried 72 s later and was
fixed again only after four hours, holding no mail and anchoring nobody
at the place it had just found.

**Rule.** A node has a place when it was given one where it was built,
has one stored, or the app sets a map point. One with none takes its
first accepted fix as its place, fixed, at once. Until then it sends no
heartbeat and stores nothing. A heartbeat says where the node is and
carries only the short position (spec "Positions on the wire"); a node
with no place can rebuild neither its neighbours' positions nor its own
beyond the short form's period, so any position it gave would send its
neighbours elsewhere. For the same reason its driver checks its first
fix against no neighbour. At the first fix it rebuilds the positions it
heard near its place, and a relay heartbeats its first rotation's burst,
as at boot (D73). A sleepy node may take an anchor before it is placed
but beats only once it is.

**Built.** `Config::placed` (the firmware sets it when built with a
place, the simulator for every node), `Node::placed`, the `placed` and
`first_fix` fields of the motion state (`relay/motion.rs`),
`Neighbours::rebuild`; the simulator's driver skips the neighbour check
for a node with no place. Tests in `bark-proto/tests/first_fix.rs`: a
relay with no place sends nothing until its first fix, then heartbeats
there, burst first, and later fixes within the allowance move nothing; a
map point places it as a fix would; a sleepy node with no place beats
only after its first fix, with its anchor's position rebuilt.

## 2. The map point (D84)

**Fault.** Spec "Packet formats" said a node clears the map point when
it takes a fix as its position; `core-interface.md` §4 said the driver
hands no fix while a map point is set. So no fix came, and nothing could
clear it: a person who set a wrong map point had no way back to GPS.

**Rule.** The app sets and clears it. While it is set the node takes no
fix, its driver hands none, a relay's GPS rests (D82) and a sleepy node
starts no search. Cleared, the node stays where it is, says so in a
heartbeat at once, and its next accepted fix is its place. A person set
it, so a person ends it.

**Built.** `AppCommand::ClearMapPoint`; the core ignores a fix while a
map point is set, and a sleepy node's search is held then as a relay's
GPS rests. The test `a_map_point_fixes_any_node` in
`tests/carried_sleepy.rs` now checks that a fix leaves the map point,
that clearing it is heartbeat at once, and that the next fix is the
place, not a far fix.

## 3. Plug-in (D85)

**Fault.** A sleepy node plugged in is a relay at once and is fixed
where it is then (`fixed_here`): its position is its last fix. Under D80
a sleepy node fixes only when the radio says it may have moved, so that
fix may be hours and kilometres old. On USB the relay's GPS runs (D82),
its first fix at the new place is a far fix, and 72 s later the node is
carried for four hours, holding no mail and anchoring nobody.

**The claim checked.** The proposal was that the first fix after plug-in
may be taken as the place, since nobody addresses a node at its own
position before it is a relay. True of the time before: as a sleepy node
its address was its anchor's position (spec "Addresses"). Not of the
time after: it heartbeats as a relay at once (spec "Sleepy nodes",
its paragraph on plug-in), and from that heartbeat sleepy nodes that
choose an anchor may take it (spec "Relay choice", its first choice) and
its own messages carry its new address. When its first fix moves it,
those addresses change with it, as at any change of a relay's position
(spec "Node identity"). So the rule is safe only if the first fix comes
before many hold the old place. On USB the GPS runs from the plug-in; a
hot start takes 2 s, a cold start under the open sky 30 s (L76K Hardware
Design V1.0, Table 2), and D80's search limit, 60 s, covers a cold start
with one frame of orbits lost. A node that has no fix by then does not
see the sky, and a fix it gets later, perhaps hours later at a window,
is as likely a stray as a move: taken as the place, a single stray would
move a relay that sleepy nodes and senders by then rely on. So the rule
is bounded by the search limit.

**Rule.** A fixed node plugged in takes its first accepted fix within a
search's limit, 60 s, as its place, fixed, at once; without one it keeps
the fix it has, and later fixes are checked as any other. A carried node
plugged in stays carried (D72), and one on a map point keeps it.

**Considered and not taken.** Taking the first fix only when it is far,
so that a near one moves nothing: the position the node has is itself
one fix, so a newer one is no less trustworthy and is where the node is
now; and the near case moves the address less than the allowance, which
the owner rule covers (spec "Owner"). One rule for all three faults,
"the next accepted fix is the place", reads more simply. Unbounded, the
form first built: the stray at a window above.

**Built.** The motion state's `first_fix` holds until when the next fix
is the place, in whole seconds as the GPS keeps its times, so that the
node stays within its size (`core-interface.md` §6): for good for D83
and D84, the search limit after a plug-in. The test
`a_sleepy_node_plugged_in_takes_its_first_fix_as_its_place` plugs a
sleepy node in 280 m from its last fix: a first fix 10 s later is its
place and it is never carried; one 70 s later is not, and a run of far
fixes makes it carried. `a_still_sleepy_node_plugged_in_is_fixed_at_once`
now expects the first fix as the place.

### How it was measured

`scenarios/dense-power.toml`, release builds, main `7fff055` against the
branch, the settled hour (2700 to 6300 s), seeds 1 to 7. Both builds
carried a temporary trace, switched on by an environment variable and
not committed, that logged each plug-in of a node that was not carried:
how far its kept position was from where it was, and its first fix
after. The main runs of seeds 2 to 4 repeat D82's row to the byte
(98.20 / 97.93 / 98.18 %). The branch build first measured was the
unbounded form; the bounded form repeats it to the byte on seed 2 (the
messages CSV is identical), since every first fix in the scenario comes
within 16 s of the plug-in.

### Where it fires

In `dense-power.toml` a walker's node is on battery while it walks and
on USB at its stops; within the hour's grace it stays a relay and turns
carried 72 s into a walk, so at its next stop it is plugged in carried,
which D85 does not touch. The plug-ins D85 does touch are of two kinds:

| | seed 1 | 2 | 3 | 4 | 5 |
|---|---|---|---|---|---|
| plug-ins of a node not carried | 1820 | 1855 | 1790 | 1801 | 1828 |
| of nodes that do not walk | 1619 | 1667 | 1584 | 1605 | 1636 |
| of walkers | 201 | 188 | 206 | 196 | 192 |
| walkers kept more than 50 m from where they were | 139 | 131 | 135 | 143 | 137 |
| walkers' distance, p50 / p90 / max, m | 98 / 568 / 2397 | 91 / 516 / 1687 | 109 / 516 / 1808 | 107 / 494 / 1632 | 108 / 450 / 1549 |
| the last walker's, at s | 2469 | 2170 | 2494 | 3277 | 2801 |

(Branch runs; main's are the same within a few.) The walkers are those
switched on in the middle of a walk, in the boot window: they boot on
battery, sleepy and fixed, fix only at anchor changes (D80), and are
plugged in at their next stop. The others are people's nodes that switch
power at random, plugged in where they always were; the simulator hands
a relay that does not walk no fix, so for them D85 changes nothing here.
On a real board each of them would move its place by its first fix's
error, within the allowance under the sky, and say so in one heartbeat.

On main the walkers kept far from where they are turn carried 72 s
after their stop begins and stay carried through it; under D85 they are
fixed relays there, as a walker that booted at its stop is. The cost
follows: at their next walk they leave fixed and turn carried 117 s in
(p50, D82), as every walker that boots at a stop does already.

### Results

| seed | delivered, main | delivered, D85 | airtime, s, main → D85 | turned carried | walks of fixed relays | walked fixed, h |
|---|---|---|---|---|---|---|
| 1 | 97.93 % | 97.93 % | 294 762 → 288 130 | 3271 → 3238 | 1298 → 1371 | 38.95 → 41.05 |
| 2 | 98.20 % | **95.56 %** | 287 972 → **384 184** | 3097 → 3055 | 1255 → 1315 | 37.11 → 39.00 |
| 3 | 97.93 % | 97.87 % | 274 348 → 273 096 | 3183 → 3140 | 1251 → 1301 | 36.96 → 38.52 |
| 4 | 98.18 % | 98.07 % | 275 809 → 269 564 | 3162 → 3130 | 1270 → 1341 | 37.77 → 39.99 |
| 5 | 98.08 % | 98.10 % | 268 794 → 268 274 | 3218 → 3189 | 1296 → 1364 | 38.56 → 40.55 |
| 6 | 98.08 % | 98.26 % | 268 269 → 270 073 | 3240 → 3202 | 1273 → 1346 | 37.51 → 39.79 |
| 7 | **91.94 %** | 98.24 % | **1 004 612** → 274 320 | 3165 → 3124 | 1330 → 1384 | 40.78 → 41.09 |

Without the storms below, seeds 1 and 3 to 6, D85 delivers 98.05 %
against main's 98.04 % on average, its turns to carried 29 to 43 fewer a
run and its walks of fixed relays 50 to 73 more, 1.6 to 2.3 hours more
walked fixed: the stops above.

Texts to the walkers D85 touched, in the settled hour, main → D85:
seed 2 125 of 127 → 99 of 102, seed 3 119 of 121 → 120 of 122, seed 4
143 of 144 → 143 of 145. A text to a relay goes to its ID wherever it is
(spec "Owner"), so whether it is carried or fixed at its stop does not
decide its own mail.

**Seed 2's storm.** Seed 2 on the branch delivers as main to the minute
for 3200 s of the settled hour (98.3 to 99.4 % in each ten minutes), then
falls: 95 % of the texts sent in its 99th minute delivered, 79 % in the
101st, 18 % in the last. The airtime rises by a third over the run. The
lost texts are spread across the whole square, every kilometre band
alike, not around the walkers D85 touched, the last of which was plugged
in 3700 s before. It has the shape of the storm D77 found on seed 1,
which a change of the DATA header's length set off and a probe showed
to follow any shift of timing. A run of the branch on seed 2 repeats it
to the byte. Main storms the same way on seed 7 from its second ten
settled minutes on (97.7 %, then 92.8 %, and 73 % in the last), with 3.7
times the branch's airtime, and the branch does not. One
storm on each side in seven seeds: the storm is the scenario's, and what
starts it is still D77's later item.

## What this does not show

- **Indoor fixes.** The simulator hands a relay that does not walk no
  fix, so the plug-ins of nodes that stay where they are, nine in ten,
  are untouched here, and the search limit's bound never acts: every
  walker's first fix comes within 16 s. Whether a window node's first
  fix lands within 60 s, and how far it strays, is the bench's (O16).
- **Many sleepy nodes anchored to a node in its first minute.** The
  scenario's plug-ins are spread over the day; nothing measured how many
  take the new relay as an anchor before its first fix.
- **Map points and boards without a place.** No scenario has either; D83
  and D84 rest on the tests and the reasoning above.

## What I am unsure of

- **The storm.** One storm on each side in seven seeds, on different
  seeds, domain-wide, and on the branch an hour after the last event D85
  changed: I read it as the scenario's fragility. Until D77's later item
  explains what starts a storm, no rule can be shown not to.
- **The bound.** 60 s is the search limit's derivation (D80), not a
  measurement of how long sleepy nodes take to anchor to a new relay.

# A sleepy node with an anchor runs no GPS

**Date:** 2026-10-31. **Status:** decided by the user 2026-10-31, built
and measured (roadmap D91). Spec references are to `protocol.md`. The
base is main `ab38727`, run as its own binary on the same scenario
files; the reference runs of main were checked byte for byte on
`ring.toml` first.

## The decision

A sleepy node with an anchor is addressed by its anchor's position, so
its own GPS fixes serve nothing the protocol needs while it has one: a
node that loses its anchor takes its position from the next anchor it
chooses, and needs no fix of its own to choose one. So it runs no
search of its own while it is sleepy and anchored: its GPS rests, as
every sleepy node's does between searches (D80). The roadmap had this
as "treating it as carried, GPS at rest". D80 had measured the variant
as "anchorless only" and chosen a search at each anchor change over it,
for the rebuilding of short positions after a long trip and a fresh
stored place; this note takes the other side, and says what that
costs.

## What a sleepy node with an anchor used its own fixes for

D80's note (`gps-schedule-2026-10-25.md`) listed nine uses of a sleepy
node's own position. With an anchor, under the rules on main
`ab38727`, its own fixes went into three of them:

| Use | Where | What replaces it |
|---|---|---|
| 1. The carried flag: 72 s of far fixes make it carried, four hours of near ones fixed again (spec "Position") | `relay/motion.rs` `fix` | nothing while it sleeps with an anchor: it keeps the flag it had when it took the anchor. Nothing reads a sleepy node's carried flag beyond its sleepy flag, since a sleepy node holds mail for nobody and is nobody's anchor whatever the flag (spec "Nodes"); the flag decides something only when the node is plugged in, and the relay's own GPS settles it then (below) |
| 2. The check of the anchor after a move of 0.67 of the way to it, at least 50 m (spec "Relay choice") | `relay.rs` `app`, `relay/anchor.rs` `move_bound_m` | the anchor missed twice, which checks it a rotation or two later. The check was dead already: since D80 the only fix an anchored node took was the one asked for at the choice, which reset the point it measured from, so no fix could ever lie beyond the bound. The rule and its code are removed |
| 3. The point it rebuilds short positions near, its new anchor's among them, after a trip (spec "Positions on the wire", "Beats") | `relay/heartbeat.rs` `heard`, the plane | the anchor's position, which is within 6 km of every node it hears: a sleepy node hears nothing beyond the hearing radius, 3 km, and its anchor is within it. The fix did the same within reach, and no better beyond; see "What it revealed" |

The user's belief was that only the first use remained. The second was
there in the spec and the code but could not fire; the third is the
one the belief misses, and it is why D80 kept a search at each anchor
change. It is lost in this decision in one case only, a trip of more
than half a short position's period while the node hears no relay, and
there the fix at the new anchor did not serve it either (below).

The other uses in D80's table need no fix with an anchor: the address
is the anchor's; the short position in its beats is now the anchor's
too, a last-resort hop its neighbours put where the node can be reached
from; the place kept across a reboot is the last the node had by its
own fixes, which a stored anchor overrides at boot; the position it
takes as a relay when plugged in is that place, as before.

## The rules now

Spec "Beats", under GPS:

- A sleepy node with an anchor works by the anchor's position: it is
  its address, the position its beats carry, the centre of its flat
  plane and the point it rebuilds short positions near. It takes no
  fix, runs no search and measures nothing of its motion: it keeps the
  carried flag it had when it took the anchor. Its own place, the last
  it had by its own fixes, stays in `Config::position` for when it is
  plugged in.
- Without an anchor it searches as D80 said: when it turns anchorless
  and once a delivery TTL while it stays so, 60 s at most, 8 h of
  back-off after one that found nothing. A search under way ends when
  the node takes an anchor, told to the driver by a `Locate` at that
  instant.
- A node with no place (D83) searches until it has one, anchor or not,
  from its boot and again at the end of each back-off: without a fix of
  its own it cannot rebuild its anchor's position, so the anchor's
  position cannot serve it.
- The search at boot and the search at a new anchor go, and the move
  check of spec "Relay choice" with them.

The transitions:

| Transition | What happens |
|---|---|
| anchored → anchor lost | the node keeps the position it had, its old anchor's, and no search starts; it listens a rotation and takes the strongest relay heard, or is anchorless. With the next anchor its position is that anchor's; anchorless, it searches once a delivery TTL and its address is its own fix (spec "Sleepy nodes alone") |
| anchorless → anchored | the search under way ends; the position is the anchor's; the carried flag stays what the anchorless fixes made it |
| sleepy → relay (plugged in, or the battery high again) | as before (D82, D85): the position is the node's own place, not its anchor's; the GPS runs on external power, and on battery through the grace, and a fixed node takes its first fix within a search's limit as its place, a carried one is fixed after four hours of fixes near one place; the carried flag it kept while sleepy is what it starts from |
| relay → sleepy (grace over, or battery low) | as before: it stays carried or fixed as it was, and takes an anchor at once among the relays it knows, with no search; with none it is anchorless and searches |
| boot sleepy with a stored anchor | the anchor at once, and no search |
| boot sleepy without one | the first rotation, no search; a search at its end only if no relay was heard |

Core-interface §4 says the driver hands no fix to a sleepy node with an
anchor, since the core asks for none, and that a `Locate` at or before
now ends a search. The firmware has no GPS driver yet and logs the
request; nothing in it changes.

## What it costs and what it saves

The L76K draws 29 mA searching and 20 µA in standby (spec "Beats"). Under
D80 an anchored node searched at each anchor change: a hot start, 2 s,
under the sky, 0.016 mAh; a failed search, 60 s, 0.48 mAh, then 8 h of
back-off. D80 measured the whole of a sleepy node's GPS at 0.023 mA in
S4 against 0.020 mA for standby alone, so the searches were 0.003 mA of
a 0.63 mA node, and "anchorless only" 0.020 mA: the saving the rule
can make is at most those 0.003 mA, half a percent of the node's
current, and nothing in battery life. That is the honest derived
number; the decision is taken for the rule's simplicity, a node that
sleeps with an anchor has one position, its anchor's, and for the
bench, where a node in a pocket now runs its GPS only when it hears no
relay.

What is given up:

- The carried flag of a sleepy node that walks. Under D80 a walker's
  fixes at its anchor changes made it carried while it slept, and the
  simulator counted them; now none does, and the flag it carries is the
  one it had when it last measured. Nothing reads it while it sleeps,
  and at plug-in the relay's GPS measures afresh within 60 s or four
  hours, so a node that walked here as "fixed" is a fixed relay at its
  old place until its first fix, as D85 already allowed, and a node that
  stood still for days as "carried" holds no mail for its first four
  hours on the charger. A carried sleepy node's anchor is not stored
  across a reboot; one that stays "carried" does a first rotation at
  each reboot where it might have resumed its anchor.
- The fix at a new anchor after a long trip (the third use above).

## Measurements

Main `ab38727` against this branch, each its own release binary on
the same scenario files; main's reference runs were first checked byte
for byte against a fresh run of `ring.toml`. Delivered is of the texts
sent in the settled hour, 2700–6300 s; "to walkers" is of those to a
node that walks at some point in the run, "walking" of those sent while
the node was in the street, both from the message log; latency is the
run's p50 and p90; the GPS current and the searches are the sleepy
nodes' after the settle time, as the summary prints them; "carried" is
sleepy nodes turned carried by their fixes.

| Scenario | delivered | to walkers | walking | latency p50 / p90 | sleepy GPS mA | searches /node/h | sleepy node mA | sleepy turned carried |
|---|---|---|---|---|---|---|---|---|
| `ring.toml` seed 1 | 92.6 → 92.6 % (identical) | — | — | 60.7 / 152.7 → same | 0.020 → 0.020 | 0 → 0 | 2.248 → 2.248 | 0 → 0 |
| S4 seed 1 | 93.42 → 93.57 % | 87.57 → 87.59 % | 78.90 → 78.31 % (545 texts) | 1.64 / 191.5 → 1.61 / 188.1 s | 0.023 → 0.020 | 0.01 → 0.00 | 0.634 → 0.632 | 320 → 0 |
| S4 seed 2 | 93.90 → 93.58 % | 88.32 → 87.62 % | 77.05 → 75.79 % (475) | 1.77 / 195.0 → 1.76 / 194.4 s | 0.023 → 0.020 | 0.01 → 0.00 | 0.643 → 0.637 | 328 → 0 |
| `dense-movers.toml` seed 1 | 96.54 → 96.35 % | 89.97 → 89.50 % | 71.92 → 68.28 % (577) | 5.54 / 48.6 → 5.61 / 56.4 s | 0.022 → 0.020 | 0.01 → 0.00 | 0.537 → 0.533 | 236 → 0 |
| `dense.toml` seed 1 | 96.93 → 96.88 % | 87.67 → 87.16 % | 65.99 → 61.49 % (444) | 2.48 / 17.2 → 2.52 / 18.3 s | 0.022 → 0.020 | 0.01 → 0.00 | 0.522 → 0.521 | 241 → 0 |
| `strip.toml` seed 1 | 40.68 → 40.05 % | 36.41 → 35.39 % | 20.62 → 19.93 % (1610) | 91.1 / 668 → 100.3 / 686 s | 0.023 → 0.020 | 0.01 → 0.00 | 0.664 → 0.662 | 1053 → 0 |

The ring has no walkers and its sleepy nodes never lose their anchors:
the runs differ only in the eight boot searches that no longer happen,
sixteen events fewer, and deliver the same messages at the same times.
`strip.toml` is overloaded at its rate, 40 % delivered, and is here for
its 13 000 walkers. The dense-power scenarios have no sleepy walkers and
were not run.

Reading:

- **Delivery overall is within the seeds.** S4 moves +0.15 and −0.32
  points over two seeds, the dense scenarios −0.05 and −0.19, the
  overloaded strip −0.63.
- **The sleepy nodes' GPS goes from 0.022–0.023 mA to the 0.020 mA of
  standby**, 0.002–0.003 mA on a 0.52–0.66 mA node: what D80 had
  measured, and no change in battery life.
- **Texts to walkers while they walk lose a little in every scenario:**
  −0.6, −1.3, −3.6, −4.5 and −0.7 points, on 444 to 1610 texts, so 3 to
  20 texts each; to walkers over the whole run −0.0 to −1.0 points. The
  same direction five times is more than chance, and the size is what
  D80 measured between its variants with and without a fix at the anchor
  change (to a walker walking 75.6–78.2 % across them in S4). The fix at
  each anchor change gave a walker its own position for a few minutes at
  a time; now its plane is centred on its anchor, up to a hearing radius
  from where it stands, which can reorder its next hops, and its beats
  put it at the anchor for the relays that keep it as a last resort.
  Which of the two it is was not measured.
- **No sleepy node turns carried any more**, 236–1053 in each scenario
  before. Nothing read the flag while they slept, and none was plugged
  in while carried in these runs, so nothing else moved with it.
- **No search was ended at an anchor** in any run: a sleepy node that
  turns anchorless in these scenarios takes an anchor again before its
  first rotation's end or after a search has ended, never during one.

## What it revealed

- **The move check was dead.** Spec "Relay choice" said a node checks
  its anchor after moving 0.67 of the way to it. Since D80 a node with
  an anchor took exactly one fix, at the choice, and that fix set the
  point the move was measured from, so no later fix existed to measure.
  The rule described nothing the code could do.
- **The long trip is not covered by a search either.** The reason D80
  kept a search at each anchor change was a node carried more than half
  a short position's period, 19.5 km north-south and 25 km × cos(lat)
  east-west, while it heard no relay: it rebuilds its next anchor's
  position a period off, and the fix at the new anchor was to put it
  right. But D74 drops a fix farther than the hearing radius from every
  relay the node knows, and the node now knows that anchor, a period
  off, so the true fix is dropped and the node stays wrong until it is
  plugged in and D85's first fix, dropped the same way, or a map point.
  With or without this decision the trip is unhandled; it is open item
  O19, and a remedy has to come from somewhere other than a search: a
  full position in a relay's answer to an anchorless beat, or a fix
  check that allows a whole period's error at a new anchor taken after
  the dark.
- **The simulator's sleepy GPS line could not tell a cut search from a
  failed one.** It now counts searches ended at an anchor apart.

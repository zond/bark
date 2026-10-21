# A node's role follows its power

**Date:** 2026-10-21, roadmap D71. **Status:** measured, awaiting the
decision. Spec references are to `protocol.md`. The base is main
`34ee00d`, run as its own binary on the same scenario files.

The question, from the user's ruling of 2026-10-21: a node's role is not
a setting. Every heartbeat flag is a measured property: sleepy means "I
am on battery", carried means "my GPS says I move" (D70), and a map point
from the app makes a node fixed. So the power is an input; a node plugged
in is a relay at once; one unplugged keeps relaying for a grace, then
flushes what it holds and turns sleepy; a change of flags or position
sends a heartbeat at once; every board runs one image and reads USB
power itself. Two values of the grace were to be measured and one
recommended.

## Summary

- **Built as asked**, with two parts measured and taken out again (below):
  the power is `AppCommand::Power`, reported by the driver after the boot
  wakeup and at each change; `Config::sleepy` is gone; a node plugged in
  relays at once; one unplugged changes nothing for its grace, then owns
  nothing, flushes, listens until each message has had its attempt, drops
  what is left and anchors; a change of a flag sends one heartbeat at once
  on the home channel; the firmware has one image and reads VBUS.
- **The grace: one hour is recommended.** On `dense-power.toml`
  (`dense-movers.toml` with every walker on battery while it walks and on
  USB at its stops, and a fifth of the other people's nodes switching at
  random), seeds 1 / 2, delivered of the texts sent in the settled hour:
  98.09 / 98.31 % at 1 h against 97.45 / 97.70 % at 10 min; to the
  switching nodes 98.91 / 99.22 % against 97.67 / 98.04 %; to sleepy
  nodes whose anchor switches 96.01 / 95.92 % against 93.96 / 93.71 %. Of
  4707 / 4605 unplugs after the settle time, 4335 / 4177 (92 / 91 %) end
  within an hour and change nothing, against 2489 / 2391 (53 / 52 %)
  within ten minutes; turns to sleepy 67 / 66 against 2214 / 2179, sleepy
  nodes moved off an anchor that turned sleepy 23 / 34 against 575 / 631.
  The hour costs a node 0.68 % of its battery at most, each unplug.
- **What it costs:** airtime per delivered text 4.45 / 4.55 s at 1 h,
  3.99 / 3.84 s at 10 min, 3.13 / 3.09 s on main without switching. The
  cost is the walkers: at an hour's grace a walker stays a relay, carried,
  through a walk of twelve minutes on average, and routes are made through
  moving relays; with only still nodes switching the airtime per text
  falls, 2.74 s at 1 h and 2.89 s at 10 min against 3.13 s, since more
  nodes are relays.
- **Against main** (no node switches there; the same texts to the same
  nodes): all 96.84 / 97.04 % → 98.09 / 98.31 % at 1 h; to the switching
  nodes 94.57 / 94.97 % → 98.91 / 99.22 %; texts to nodes that neither
  switch nor anchor to a switcher 98.35 / 98.49 % → 98.25 / 98.52 %,
  within the seeds.
- **Mail at the turn is little:** at the end of a grace a relay held 3
  (1 h, seed 1), 6 (seed 2) and 36 / 38 (10 min) texts; the flush delivered
  0 / 1 and 2 / 6 of them, none went at a beat before the turn, the rest
  were dropped at the turn. Most relays that leave are walkers, carried
  within 72 s of their first step, whose flush at the first far fix has
  already gone.
- **The heartbeat for a change of position, measured and taken out:** a
  carried node's position changes at each fix, and a heartbeat for it once
  an interval doubled every walking relay's heartbeats: 219 117 / 208 695
  frames, 2594 / 2471 s of airtime in the settled hour at 1 h, for
  delivered 98.18 / 98.31 % against 98.09 / 98.31 % without. Its slots
  carry the position anyway. What stays, a heartbeat for a change of a
  flag, is 2024 / 1990 frames and 24.7 / 23.9 s an hour at 1 h, 6817 /
  6600 and 95 / 92 s at 10 min.
- **Keeping a carried node carried while it sleeps, measured and not
  built:** within the seeds (97.50 / 97.65 % against 97.45 / 97.70 % at
  10 min, 98.03 % against 98.09 % at 1 h).
- **S4 and `dense.toml` seed 1 repeat main to the byte**: JSON summary,
  messages CSV and printed table but the wall time, at every simulator
  commit and at the tip (Checks).
- **Open: solar relays** (spec O17). A solar box runs from its battery and
  charges it by day; read as "external power" from its charger, it would
  turn sleepy every night.

## What was changed

| Commit | What |
|---|---|
| Core: a node's role is its power, reported at boot and at each change | `AppCommand::Power { external }`; `Config::sleepy` goes and every read of it reads the node's runtime state (`Node::sleepy()`, `relay/power.rs`). A `Relay` takes itself to be on external power until told, a `SleepyNode` stays sleepy. While it boots a report sets the role at once, the boot listen as long as that role's, counted from the boot. Running, plugged in: a relay at once, no anchor, its own position, slots at a random phase, the sweep again. A sleepy node leaves an anchor heard sleepy as one heard carried, and a check that hears no relay leaves one last heard so. The test harness reports the power at boot (`Harness::powered`), the simulator and the firmware likewise. |
| Core: a relay unplugged keeps relaying for a grace, then flushes and sleeps | `Config::grace`, an hour. Power back within it, nothing happened. After it, the node is leaving: it owns nothing, answers no anchorless beat, and flushes as at a first far fix; once each message has had its attempt and no delivery is queued it turns sleepy, dropping what is left (`DropReason::Unplugged`), and takes the strongest relay it knows, as at the end of a boot listen, or is anchorless. `Trace::GraceEnded`, `Trace::Role`. Tests in `tests/power.rs`. |
| Core: a change of flags or position sends a heartbeat at once | `Beat::Change` on the home channel, outside the slots, listen-before-talk as any heartbeat; one for any number of changes; a position change from a fix at most once an interval; a relay turning sleepy says so on its old home. `Trace::Changed`. |
| Firmware: one image, the role from USB power | The `sleepy` feature goes; every board runs `Relay` (167 008 B; the image 319 KiB of flash and 171 KiB of RAM, 84 KiB left for the stack, against 317 / 170 KiB for the old relay image). `bark-fw/src/power.rs` reads `USBREGSTATUS.VBUSDETECT` again at each POWER interrupt, bound beside the USB driver's handler; `bark-driver`'s `App::power()` is reported after the boot wakeup and whenever `App::ready` wakes for a change. The LED blinks and the 32 MHz crystal runs only on USB. `BARK_ON_BATTERY=1` keeps a board sleepy on the bench's USB. `bark-fw/README.md`, `scripts/measure.md` step 5, `core-interface.md` §1, §2, §4, §5, §6. |
| Simulator: power that switches, and what the switches do | `[power]` (`walkers`, `fraction`, `usb_s`, `battery_s`), `[protocol] grace_ms`, `research.power`, `scenarios/dense-power.toml`; a node on USB does not go dark; a sender learns a switching node's address from its core. |
| Core: no heartbeat for a position a fix changes (measured, not adopted) | The position part of the heartbeat on change goes, with its bound; a flag or the map point still sends one. |
| Spec proposal | Spec "Hardware", "Heartbeat", "Position and time", "Sleepy nodes", "Relay choice", "Delivery queue", "Packet formats", "Phone link", "Implementation notes", the summary's "Nodes" row, open item O17. README. |

Found on the way, before any run counted: the simulator kept a sleepy
node's anchor bookkeeping by its index, so a node placed as a relay that
turned sleepy was never followed, and the flush and the deliveries to a
node as a relay of a node placed as a leaf were not counted; both now go
by the core's state. Not built: the walkers' sleepy-node metrics
(`to_sleepy`, the beat fates of `SleepMetrics`) still go by placement, so
they leave out a node placed as a relay while it is sleepy, and the
current per sleepy node mixes nodes placed as leaves that are relays now.

## What was run

Main `34ee00d` and the branch, each run 6300 s with the settled hour from
2700 s, delivered of the texts sent in it, those still on their way at
the end counted as not delivered:

| Run | Scenario |
|---|---|
| main | `dense-movers.toml` seeds 1, 2 (nobody switches; the comparison for every row below) |
| branch, as first built | `dense-power.toml` at a grace of 1 h and of 10 min, seeds 1, 2 (with the heartbeat for a position change) |
| branch, the tip | the same, without it (the "flags-only" variant, built as `0a9d88c` then committed), seeds 1, 2 |
| branch, still nodes only | `dense-power.toml` with `walkers = false`, 1 h and 10 min, seed 1 (as first built) |
| branch, carried while sleepy | the tip with a node's carried state kept while it is sleepy (below), 1 h seed 1, 10 min seeds 1, 2 |
| main, branch | `s4-capacity.toml` and `dense.toml` seed 1, at the first commit, at the simulator's and at the tip |
| main, branch at both graces | four settled hours, seed 1, run 17 100 s |

`dense-power.toml` adds to `dense-movers.toml`: every walker's node,
20 % of the people, is on battery while it walks and on USB at its stops,
starting at a stop; and a fifth of the other people's nodes switch at
random, on USB for an exponential time of mean 2 h and on battery for one
of mean 30 min. Both means are assumptions. A walker's stop lasts an hour
and its walk twelve minutes on average, so about 4 000 more people's nodes
are relays at any time than in `dense-movers.toml`'s 5317.

"Switching nodes" are those `[power]` lets switch, 7223 / 7201 (4041 /
3924 walkers); on main the same nodes, which there keep their placement's
role. "Sleepy node of a switching anchor" is a text to a sleepy node whose
anchor, when the text was sent, is a switching node. "Back within the
grace" counts the reports of USB to a node that still relayed after it
had been told battery.

## Results

### The settled hour

| | main s1 | main s2 | 1 h s1 | 1 h s2 | 10 min s1 | 10 min s2 |
|---|---|---|---|---|---|---|
| delivered, all | 96.84 % (17 893) | 97.04 % (17 877) | 98.09 % (18 234) | 98.31 % (18 199) | 97.45 % (18 232) | 97.70 % (18 199) |
| to a switching node | 94.57 % (6487) | 94.97 % (6458) | 98.91 % (6601) | 99.22 % (6565) | 97.67 % (6600) | 98.04 % (6565) |
| of them walkers | 91.66 % | 92.50 % | 98.45 % | 99.02 % | 96.51 % | 96.78 % |
| of them at random | 98.37 % | 97.90 % | 99.51 % | 99.47 % | 99.16 % | 99.53 % |
| to a sleepy node of a switching anchor | 94.71 % (2172) | 94.45 % (2161) | 96.01 % (3184) | 95.92 % (3305) | 93.96 % (3162) | 93.71 % (3321) |
| to a sleepy node of another anchor | 96.19 % | 96.52 % | 97.54 % | 97.82 % | 97.09 % | 97.58 % |
| to neither | 98.35 % | 98.49 % | 98.25 % | 98.52 % | 98.23 % | 98.56 % |
| airtime per delivered text | 3.13 s | 3.09 s | 4.45 s | 4.55 s | 3.99 s | 3.84 s |
| latency p50 / p90 | 5.1 / 29.0 s | 5.1 / 29.6 s | 9.3 / 34.0 s | 9.6 / 31.8 s | 8.7 / 34.5 s | 8.8 / 34.2 s |
| all airtime over the run | 157 837 s | 152 732 s | 225 966 s | 230 994 s | 213 251 s | 214 586 s |
| channels busy | 2.9–3.3 % | | 4.0–4.7 % | | 3.4–4.2 % | |
| anchor changes after the settle time | 9015 | 8741 | 3335 | 3347 | 7666 | 7732 |

### What the switches did, the settled hour

| | 1 h s1 | 1 h s2 | 10 min s1 | 10 min s2 |
|---|---|---|---|---|
| reports of power (to battery) | 9639 (4707) | 9395 (4605) | 9639 (4707) | 9395 (4605) |
| USB back within the grace | 4335 | 4177 | 2489 | 2391 |
| USB back during the flush | 0 | 0 | 0 | 0 |
| graces ended, turns to sleepy | 67 | 66 | 2214 | 2179 |
| turns to relay | 597 | 613 | 2443 | 2399 |
| texts held at a grace's end: flush / at a beat / to the node as a relay / dropped | 0 / 0 / 0 / 3 | 1 / 0 / 0 / 5 | 2 / 0 / 0 / 34 | 6 / 0 / 0 / 32 |
| sleepy nodes moved off an anchor turned sleepy | 23 | 34 | 575 | 631 |
| heartbeats for a change of flag: frames, airtime | 2024, 24.7 s | 1990, 23.9 s | 6817, 95.2 s | 6600, 91.6 s |
| delivered to a node as a relay: frames, airtime | 1384, 95.5 s | 1256, 86.4 s | 1003, 68.8 s | 977, 66.9 s |
| flush frames, airtime | 64, 27.1 s | 49, 20.8 s | 129, 54.7 s | 113, 47.9 s |
| turns to carried (whole run) | 3230 | 3054 | 4469 | 4193 |

The turns to relay outnumber those to sleepy because a node placed as a
leaf that switches at random is sleepy from its boot and turns a relay at
its first plug, and at an hour's grace few turn back within the hour.

### Only still nodes switching

`walkers = false`: the fifth of the people's nodes that do not walk switch
at random, the walkers keep their placement's role. Seed 1, as first
built (with the heartbeat for a position change, which only the 1004
walking relays send here):

| | main | 1 h | 10 min |
|---|---|---|---|
| delivered, all | 96.84 % | 96.97 % | 96.75 % |
| to a switching node | 98.37 % (2820) | 99.40 % (2837) | 99.44 % (2837) |
| to a sleepy node of a switching anchor | 96.18 % (1494) | 94.08 % (2788) | 92.69 % (2587) |
| to a sleepy node of another anchor | 95.92 % | 96.12 % | 95.99 % |
| airtime per delivered text | 3.13 s | 2.74 s | 2.89 s |
| latency p50 / p90 | 5.1 / 29.0 s | 5.5 / 25.4 s | 5.5 / 26.7 s |
| unplugs after the settle time; back within the grace | | 1186; 857 | 1186; 330 |
| turns to sleepy | | 66 | 840 |
| sleepy nodes moved off an anchor turned sleepy | | 39 | 857 |

### The two measured and taken out

| | as first built, 1 h | flags only, 1 h | carried while sleepy, 1 h | flags only, 10 min | carried while sleepy, 10 min |
|---|---|---|---|---|---|
| delivered, all, s1 / s2 | 98.18 / 98.31 % | 98.09 / 98.31 % | 98.03 % | 97.45 / 97.70 % | 97.50 / 97.65 % |
| to a sleepy node of a switching anchor | 96.15 / 96.07 % | 96.01 / 95.92 % | 95.84 % | 93.96 / 93.71 % | 94.28 / 93.84 % |
| airtime per delivered text | 4.50 / 4.23 s | 4.45 / 4.55 s | 4.61 s | 3.99 / 3.84 s | 3.91 / 3.85 s |
| heartbeats for a position change, the settled hour | 219 117 / 208 695, 2594 / 2471 s | none | none | none | none |
| all airtime over the run | 230 254 / 232 204 s | 225 966 / 230 994 s | 231 316 s | 213 251 / 214 586 s | 214 864 / 215 483 s |

"Carried while sleepy": a node that turns sleepy keeps whether it is
carried, and its fixes keep deciding it, far from the place it stood when
it turned sleepy or from its first fix after; plugged in, a carried one
stays carried until four hours still, as D70 has a carried relay, where
the tip makes it fixed at once. The aim was the walker that turns sleepy
in a long walk at 10 min and, fixed on its charger, takes mail it strands
on its next walk. It moves nothing beyond the seeds, and it would put the
carried flag in sleepy nodes' beats, so it was not built.

### Four settled hours

`dense-power.toml` seed 1 run 17 100 s, the texts sent from 2700 s on,
four hours; main on `dense-movers.toml`; the branch at its tip:

| | main | 1 h | 10 min |
|---|---|---|---|
| delivered, all | 97.90 % (71 972) | 99.12 % (73 406) | 98.51 % (73 403) |
| to a switching node | 95.70 % | 99.46 % | 98.29 % |
| of them walkers | 92.97 % | 99.20 % | 97.18 % |
| of them at random | 99.15 % | 99.79 % | 99.70 % |
| to a sleepy node of a switching anchor | 96.44 % (7584) | 97.60 % (11 544) | 95.67 % (10 564) |
| to neither | 99.27 % | 99.35 % | 99.23 % |
| airtime per delivered text | 3.00 s | 4.49 s | 3.87 s |
| latency p50 / p90 | 5.2 / 34.1 s | 9.7 / 36.3 s | 8.8 / 37.4 s |
| all airtime over the run | 398 885 s | 628 753 s | 570 724 s |
| anchor changes after the settle time | 31 850 | 8906 | 27 196 |
| unplugs after the settle time; back within the grace | | 18 544; 17 701 | 18 544; 9709 |
| turns to sleepy; to relay | | 562; 1160 | 8868; 9152 |
| texts held at a grace's end: flush / dropped | | 0 / 35 | 12 / 182 |
| sleepy nodes moved off an anchor turned sleepy | | 337 | 2487 |
| heartbeats for a change of flag: frames, airtime | | 3748, 47.9 s | 23 232, 324.7 s |
| turns to carried; fixed again | 994; 12 | 3997; 47 | 9028; 47 |
| flush frames, airtime | 45, 19.1 s | 161, 68.2 s | 406, 172.0 s |

Over four hours the hour's lead holds and nothing drifts: 95 % of the
unplugs change nothing against 52 %, and at 10 min a walker that turned
sleepy in a walk turns carried again, 9028 turns against 3997. The
branch as first built, with the heartbeat for a position change, gave
99.12 and 98.55 % over the same four hours, 10 625 and 6516 s of airtime
for those heartbeats.

### S4 and the dense city

| | main | branch |
|---|---|---|
| S4 seed 1 | 94.20 %, 1.32 s a text | the same to the byte: JSON summary, messages CSV and printed table but the wall time, at the first commit (`4be3cb2`), with the simulator's power section (`66f3c95`) and at the tip |
| `dense.toml` seed 1 | 97.31 %, 1.85 s | the same to the byte, likewise |

Nobody switches there and no relay is handed a fix, so no heartbeat for a
change goes; the power each node is placed with is reported at its boot,
and a report at boot changes nothing but the role it sets.

## The checks

- `cargo test --workspace`, `cargo clippy --workspace --all-targets`,
  `cargo fmt --check`, the core for `thumbv7em-none-eabihf`, and the
  firmware's clippy for its two images are clean at each commit. The
  timing test of `bark-sim/tests/scenarios.rs` passes on an idle machine
  within its unchanged bounds (see the end).
- `tests/power.rs`: the power at boot sets the role, with no trace and
  the role's boot listen; a sleepy node plugged in relays at once, at its
  own position, and its old anchor hands it its mail within seconds by the
  heartbeat at once; a relay unplugged relays through its grace and power
  back changes nothing; after the grace it flushes, with the long
  preamble, and turns sleepy, a text the flush did not deliver dropped,
  and its sleepy node moves to another relay; a relay carried in its grace
  turns sleepy without a second flush; a lone relay unplugged is
  anchorless and plugged in again a relay; a change of flag heartbeats
  once at once, on the home channel, and a carried node's fixes add none;
  a relay turning sleepy says so on its old channel.
- `bark-driver`: the power reaches the core after the boot wakeup and once
  at each change. The simulator test `nodes_switch_with_their_power`.

## Why

A relay is where mail waits for sleepy nodes and where their addresses
point, so every turn of a relay to sleepy costs its sleepy nodes: they
miss its slots, check, move to another anchor, and the texts sent to the
old address meanwhile find no holder. The grace decides how many unplugs
become such a turn. People unplug often and briefly; the scenario's
spells on battery, half an hour on average and exponential, end within
ten minutes 28 % of the time and within an hour 86 %, and a walk ends
within an hour almost always. So at 10 min half the unplugs turn a node,
and sleepy nodes move off a turned anchor 18 to 25 times as often; at an
hour one unplug in twelve turns one. That is the 2-point gap in texts to the sleepy nodes of a
switching anchor, and most of the gap in all delivered.

The hour costs the node at most 0.68 % of its battery each time, and
nothing on the radio: the node goes on doing what it did. What it does
cost in this scenario is the walkers: a walker stays a relay through its
walk, carried after 72 s, a moving relay that routes, as D70 makes it.
Moving relays as hops cost airtime, which is why the airtime per text
rises with the walkers switching and falls with only still nodes
switching (2.74 s against main's 3.13 s, more relays among the people).

The mail at a turn is small for the reason D70 found for the far fix:
a relay that leaves holds little, and what it holds is mostly for nodes
it could not reach either. The flush delivers a few, the rest are
dropped, which a sleepy node must do since it holds nothing.

The heartbeat for a change of a flag is a few frames an hour per node
and is what makes the old anchor hand over a node's mail within seconds
of its plug; the heartbeat for a change of position doubled the walkers'
heartbeats for nothing measurable, since the slots carry the position
each interval and the change heartbeat reaches only the neighbours on
the node's own channel.

## What this does not show

- How people charge these nodes. The walkers' tie and the random means
  are assumptions; the grace's effect depends on how long spells on
  battery last, which nobody has measured.
- The senders here learn a switching node's address from its core when
  they send. A real sender has what the app last told it; a node that
  turns relay or sleepy changes its address, and how fast its senders
  learn sets the real figure for texts to it.
- Solar relays (O17): a box that charges its battery from a panel has no
  USB; what it reports as power decides whether it relays at night.
- The firmware: VBUS detection, the POWER interrupt beside the USB
  driver's and the crystal started on the cable are compiled, not run.
  Whether USB enumerates when the cable comes after a boot on battery is
  not known.
- The current of a node on battery as a relay in its grace, 5.8 mA, is
  the datasheet's, as is everything of the radio's current.
- Some sleepy-node metrics still go by placement (What was changed).
- Two seeds for each comparison, one for the long runs.

## Recommendation

1. **Adopt the role from the power** (spec proposal, D71): one image, no
   setting, a relay on USB at once, and the turn to sleepy after the grace
   with the flush and the drop.
2. **The grace: one hour**, the default on the branch: 92 % of unplugs
   change nothing against 53 % at ten minutes, sleepy nodes moved off a
   turned anchor 18 to 25 times less often, all delivered +0.6 points, for at
   most 0.68 % of a battery an unplug. If the walkers' airtime matters
   more than that, ten minutes is the other measured value.
3. **The heartbeat at once for a change of a flag or of the map point**:
   keep. **For a position a fix changes**: not adopted, taken out on the
   branch.
4. **Keep a node's carried state across a sleepy spell**: not built; it
   moved nothing.
5. **O17 before rooftop boards**: how a solar relay reports its power.

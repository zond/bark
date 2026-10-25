# When a sleepy node runs its GPS

**Date:** 2026-10-25. **Status:** measured, not decided. Spec references
are to `protocol.md`. The base is main `1654017`.

## Question

Spec "Beats" says a sleepy node refreshes its GPS fix before each beat,
and that the GPS goes off after a fix or after 10 minutes without one.
Indoors, where most sleepy nodes are, the GPS mostly finds nothing, so a
search before each beat, a beat every 225 s on average, keeps the
receiver on all the time and empties the battery in about a day. The
simulator counted no GPS at all, so its sleepy-node current, 0.639 mA in
S4, was the radio's alone.

The rule "position from GPS, unless the fix is unreliable or the app
sets a map point" (D63, D74) stands. What is open is when a node runs its
GPS and for how long. The user's hypothesis: an anchored sleepy node may
need no GPS at all, since hearing its anchor each rotation says it is
within radio range of a known fixed relay; only an anchorless node, a
node whose anchor changed, or the app asking would need a fix.

## What a sleepy node uses its own position for

From the spec and the core (`relay.rs`, `relay/motion.rs`,
`relay/power.rs`, `relay/forward.rs`, `wire.rs`):

| Use | Who reads it | Accuracy needed | Freshness needed | Without a fix |
|---|---|---|---|---|
| 1. Its address while anchored | senders, routing | none: the address is the anchor's position, not its own | — | unchanged |
| 2. Its address while anchorless | senders; routing toward it | within radio reach of the node, so that routing toward the position ends at a node that hears it | as old as the address a sender holds; the network keeps what it holds for a node a delivery TTL, 600 s | stale once the node moves out of reach of the old position |
| 3. The short position in its beats | every neighbour, into its neighbour table | a sleepy neighbour is a next hop only as a last resort, where no relay is live (spec "Next hop"); its position orders it there | loose | a wrong last-resort hop now and then |
| 4. The centre of its own flat plane, and the decoding of every short position it hears, its anchor's among them | itself | within half a period of the short position: 19.5 km north-south, 12.5 km east-west at 50° N (spec "Positions on the wire") | — | after a trip of more than 12.5 km without a fix, every position it decodes, its new anchor's and so its address, is 25 km off; it heals at the first fix and the anchor's next heartbeat heard |
| 5. The move check: a node that has moved 0.67 of the way to its anchor, at least 50 m, from where it chose, checks the anchor (spec "Relay choice") | itself | 50 m | each beat, to act before the anchor fades | the node checks when it misses its anchor's heartbeat twice instead, a rotation or two later |
| 6. Carried or fixed (D70, D72) | others read the flag in its beats, but nothing acts on a sleepy node's carried flag beyond its sleepy flag (`carried-sleepy-2026-10-22.md`) | 50 m | two fixes 72 s apart | at plug-in, a node that walked here is a fixed relay at the old place until its relay GPS sees 72 s of far fixes, then carried for four hours; one carried by fixes is carried for four hours at once. Both hold no mail for four hours |
| 7. The place a fixed node stands, kept across a reboot (D73) | itself after a reboot | 50 m | — | the place of its last fix |
| 8. The position it takes as a relay when plugged in (`fixed_here`) | its neighbours and the sleepy nodes that anchor to it | 50 m (the owner rule) | at plug-in | see 6; a relay's GPS runs all the time on USB, so its first fix after 72 s corrects it, through the carried state |
| 9. The app's own location in a message | the recipient | — | — | not used: the sender's address in the payload is the anchor's position and the node's ID (spec "Phone link"); the phone has its own GPS |

So the hypothesis holds for what the network needs from an anchored
node: its address (1) does not depend on its own position, and the
radio already says it is within reach of its anchor. A sleepy node's
carried flag does no work while it sleeps (6); it matters at plug-in,
where the relay's own GPS corrects it within 72 s either way. Two uses
lose something without fixes:

- **The move check (5).** It is the one use where a fix each beat buys
  delivery: measured below, about two points of the texts to walkers.
  Radio evidence replaces it only late: the anchor missed twice, or a
  stronger relay heard during a check.
- **The decoding of short positions (4).** An anchored node that travels
  more than 12.5 km east-west without a fix decodes its new anchor's
  position, and so its address, 25 km off. Radio evidence cannot catch
  this; a fix at the anchor change does, when the sky is in view.

## What the L76K costs

Quectel, *L76K Hardware Design* V1.0, 2021-10-20:

| Fact | Value | Where |
|---|---|---|
| Current, acquisition and tracking | 29 mA (GPS + BeiDou or GPS + GLONASS) | Table 2, p. 11 |
| Current, standby (RF off, core and RAM on; WAKEUP pin low) | 20 µA | Table 2; §3.3.3, p. 20 |
| Current, backup (VCC off, V_BCKP keeps the clock and RAM) | 8 µA | Table 2; §3.3.4, p. 20 |
| Time to first fix, cold start | 30 s (5.5 s with assistance data) | Table 2 |
| Time to first fix, hot start | 2 s | Table 2 |
| Sensitivity: acquisition, reacquisition, tracking | −148, −160, −162 dBm | Table 2 |
| Conditions of those figures | room temperature, every satellite at −130 dBm; open sky | Table 2, notes 1, 2 |
| What a hot start needs | valid time and satellite orbits kept in the RTC domain; without backup power the module cold-starts | §3.2.2, p. 18 |

A satellite's broadcast orbit is fitted over four hours (IS-GPS-200,
§20.3.3.4.3.1), so a fix within the last four hours, with the receiver kept
in standby or backup, allows a hot start; older, the receiver reads the
orbits afresh, 30 s, which the signal repeats every 30 s (§20.3.2).

**Indoors.** The open-sky level is −130 dBm (Table 2, note 1; the GPS
L1 C/A minimum is −128.5 dBm, IS-GPS-200 §3.3.1.6). The L76K acquires
only 18 dB below it. The scenarios put a 14.4 dB wall between a sleepy
node and the street at 433 MHz; building entry loss grows with frequency
(ITU-R P.2109), and the satellites are seen through the floors above as
well. So a node inside a building should expect no fix, and a search
there runs to its limit. What a search costs:

| Search | Time | Charge |
|---|---|---|
| hot start that finds a fix | 2 s | 0.016 mAh |
| cold start that finds a fix | 30 s | 0.24 mAh |
| one that finds nothing, at the limit proposed below | 60 s | 0.48 mAh |
| one that finds nothing, at the spec's limit | 600 s | 4.8 mAh |

Before each beat, a beat every 150–300 s, the spec's 600 s searches
overlap: the receiver never stops, 29 mA, and 850 mAh lasts 29 h, about
1.2 days. The spec's "≈27 mA, ≈1.3 days" is that, with an older figure.

## The rules proposed

The core asks for a search with a new action, `Action::Locate(until)`:
the driver runs the GPS until `until` and hands the first fix that
passes its checks as `AppCommand::Fix`, then stops it. A search that
reaches `until` found nothing; the core knows, since it set the end.
A relay's GPS runs all the time, on USB, so only a sleepy node asks.

**Rule.** A sleepy node searches when it turns sleepy or boots sleepy,
when it takes an anchor or changes it, when it turns anchorless, and
once a delivery TTL while it stays anchorless. A search ends at the
first fix, or after 60 s. After one that found nothing it searches no
more for 8 hours. The fix asked for at a new anchor is where the node
chose from.

| Parameter | Value | Derivation |
|---|---|---|
| search limit | 60 s | two of the 30 s frames that carry the orbits (IS-GPS-200 §20.3.2), so that a cold start under the open sky, 30 s (L76K Table 2), survives one frame lost to an error; indoors, below −148 dBm, a longer search finds nothing either |
| a search that finds nothing | 0.48 mAh | 60 s at 29 mA |
| back-off after it | 8 h | the most the GPS may take from a node that never sees the sky is a tenth of its sleepy current, 0.063 mA of 0.63 (below): 0.48 mAh / 0.063 mA = 7.7 h |
| anchorless, a search every | 600 s, the delivery TTL | its address is its own position; the network keeps what it holds for the node a TTL |
| hot start | 2 s, a fix in the last 4 h | L76K Table 2; IS-GPS-200 §20.3.3.4.3.1 |
| between searches | standby, 20 µA | L76K Table 2; keeps what a hot start needs (§3.2.2) |

*Why at these events.* They are the times the radio says the node may
have moved: a new anchor, or none. Between them, hearing its anchor each
rotation says the node is within reach of a fixed relay, which is all
its address needs. A fix at a new anchor also keeps the decoding of
short positions right across a long trip.

*Why the back-off holds every event back.* Indoors a search finds
nothing, and an indoor node's anchor changes too: the back-off is what
bounds a node that never sees the sky, whatever its radio does.

*Why the fix at a new anchor is where it chose from.* It comes after the
choice; measured from the position before, a node that moved since its
last fix would check the anchor it has just chosen. Spec "Beats" as
written has the same fault at its first beat after a choice (the test
`the_fix_at_a_choice_is_where_the_node_chose`); how many of the 7786
checks after a fix in S4's settled hour on main it causes is not
counted.

**Relays.** A relay's GPS runs all the time. After its first fix it
needs the GPS for one thing: to see that it is carried (D72), 72 s of
far fixes at the receiver's rate of one a second. On USB 29 mA costs
nothing that matters. A solar relay (O17) cannot afford it, and should
search as a sleepy node does; that is O17's to decide.

**The app.** An app that wants the node's own position, for a map, can
ask with a new command that starts a search; the phone usually has a
better GPS of its own. Not built.

## Measurements

The simulator's model (`bark-sim/src/gps.rs`, `simulator-design.md`):
the core's `Locate` starts a search; a node under the sky gets its fix
after a hot start, 2 s, if it had one in the last four hours, else after
a cold start, 30 s; a node that hears no sky gets none and its search
runs to the end. Under the sky are the walkers while they walk; the
sleepy nodes indoors, and walkers at their stops, hear none, all of them
by default (`[gps] no_sky = 1`, what the placement indoors implies), half
in the second set of runs. The time a search runs is counted at 29 mA,
the rest at 20 µA. The variants (`[protocol] gps_schedule`,
`gps_backoff_ms`):

- **(a)** `every_beat`: spec "Beats" as written, a search of up to 10
  minutes before every beat.
- **(b)** `events`: the rule above; back-off 1 h (the user's suggestion)
  and 8 h.
- **(c)** `anchorless`: the user's hypothesis, no search while anchored;
  only when anchorless.
- **(f)** `follow`: (b), and a search at each beat while the last two
  fixes were more than 50 m apart, to keep the move check for walkers;
  back-off 0, 1 h and 8 h.

S4 (`s4-capacity.toml`), seed 1, the settled hour; currents are the
means over the 20 000 sleepy nodes, battery life on 850 mAh. The radio
is listening, duty-cycled receive and transmitting; the MCU is not
modelled. Delivered: of the texts sent in the settled hour.

| Variant | radio mA | GPS mA | total mA | days | delivered | to walkers | to a walker walking | searches /node/h | walkers turned carried | anchor changes | checks after a fix |
|---|---|---|---|---|---|---|---|---|---|---|---|
| main (no GPS model, a fix at each walker's beat) | 0.639 | — | — | — | 94.10 % | — | — | — | 3230 | 9195 | 7786 |
| (a) every beat | 0.631 | 28.157 | 28.788 | 1.2 | 93.74 % | 89.22 % | 80.73 % | † | 2561 | 8954 | 6649 |
| (b) events, 1 h | 0.608 | 0.075 | 0.683 | 51.8 | 93.66 % | 87.29 % | 75.60 % | 0.135 | 433 | 6915 | 0 |
| (b) events, 8 h | 0.609 | 0.023 | 0.632 | 56.0 | 93.56 % | 87.18 % | 76.70 % | 0.007 | 315 | 6983 | 0 |
| (c) anchorless only | 0.612 | 0.020 | 0.632 | 56.0 | 93.50 % | 87.40 % | 75.78 % | 0.001 | 0 | 7067 | 0 |
| (f) follow, 0 | 0.610 | 0.173 | 0.783 | 45.3 | 93.53 % | 87.96 % | 77.43 % | 0.444 | 1097 | 7104 | 428 |
| (f) follow, 1 h | 0.611 | 0.079 | 0.691 | 51.3 | 93.33 % | 87.29 % | 77.25 % | 0.177 | 608 | 7235 | 227 |
| (f) follow, 8 h | 0.609 | 0.022 | 0.632 | 56.1 | 93.39 % | 87.82 % | 78.17 % | 0.008 | 329 | 7029 | 0 |

† With (a) an indoor node's search never ends, so "searches" counts
little; its receiver is on all the time.

Half the indoor nodes under the sky (`no_sky = 0.5`):

| Variant | radio mA | GPS mA | total mA | days | delivered | to walkers | to a walker walking | searches /node/h, found | walkers turned carried |
|---|---|---|---|---|---|---|---|---|---|
| (a) every beat | 0.633 | 14.132 | 14.765 | 2.4 | 93.84 % | 89.44 % | 82.54 % | 8.76, 100 % | 2882 |
| (b) events, 8 h | 0.611 | 0.024 | 0.635 | 55.8 | 93.45 % | 87.34 % | 75.96 % | 0.18, 99 % | 1103 |
| (f) follow, 8 h | 0.613 | 0.026 | 0.639 | 55.5 | 93.34 % | 87.34 % | 76.65 % | 0.27, 99 % | 1108 |

`dense-movers.toml` (D70's scenario: a fifth of the people's relays and
of the sleepy nodes walk, GPS checks on), seed 1, settled hour. A
relay's fixes come at its heartbeats in every variant, so the walking
relays' carried detection does not change; what changes is the sleepy
walkers'. "To walkers" is the texts to any node that walks, relay or
sleepy:

| Variant | radio mA | GPS mA | total mA | days | delivered | to walkers | to a walker walking | nodes turned carried | anchor changes |
|---|---|---|---|---|---|---|---|---|---|
| (a) every beat | 0.540 | 28.170 | 28.709 | 1.2 | 96.85 % | 91.55 % | 76.60 % | 2763 | 8500 |
| (b) events, 8 h | 0.513 | 0.022 | 0.535 | 66.2 | 96.47 % | 89.91 % | 70.88 % | 1067 | 6679 |

What the runs say:

- **(a) is not a design.** Indoors it holds the receiver on: 28 mA, 1.2
  days; with half the nodes under the sky, 14 mA, 2.4 days. A node under
  the sky alone pays 2 s a beat, 0.26 mA, 40 % on top of its radio.
- **What a fix each beat buys is the move check.** The texts to walkers
  arrive 1.9 points more often with (a) than with (b) or (c) in S4, 89.2
  against 87.2 to 87.4 %, and to a walker while it walks 4 to 5 points
  more; overall 0.1 to 0.2 points, within a seed's spread. In
  `dense-movers.toml` the same: 1.6 points to walkers, 5.7 to a walker
  walking, 0.4 overall. The price is 6649 anchor checks an hour after a
  fix, each a rotation of listening, which is why the radio current is
  0.02 mA higher with (a).
- **(a) is 0.36 points below main.** Main handed each walker a fix at
  each beat at once, indoors too; (a) gives it one after the receiver's
  start, and none at a stop. Within two seeds' spread (D72: ±0.3).
- **Following a moving node does not buy it back.** (f) starts at the
  first anchor change of a walk, by when the move check would have acted
  already; its walkers stay at 87.3 to 88.0 %. Rejected.
- **(b) and (c) deliver alike.** The differences are within a seed. (b)
  with 8 h costs what (c) costs, 0.003 mA of searching over the standby,
  since an indoor node's first search fails and holds the rest back; with
  1 h it costs 0.055 mA, 9 % of the radio. With the sky in view, (b)
  takes a fix at each anchor change, which (c) never does.
- **Carried detection of sleepy walkers** falls from 2561 (a) to 315 to
  433 (b) and none (c). Nothing reads it while the node sleeps; at
  plug-in the relay's own GPS sees the motion again (use 6).

## Recommendation

Adopt (b), events with an 8 h back-off, and replace the GPS paragraph
of spec "Beats" with the text below. The spec is not edited here: the
back-off is a choice of how much battery to protect, the user's to make. It keeps the user's rule and the user's
hypothesis where it holds: an anchored node runs no GPS between anchor
changes. It costs what no GPS costs, 56 days on 850 mAh in S4, against
1.2 days as written, and it keeps the two things the hypothesis alone
loses: a fix at each anchor change where the sky is in view, which keeps
a travelling node's decoding of positions right, and the fix at boot
that the place it keeps across a reboot (D73) needs.

The two points of delivery to walkers that a fix each beat buys are
not worth 28 mA. If they are wanted, a radio signal that the node
moves, the anchor's averaged RSSI falling by the switching margin, could
start the move check without the GPS; not measured.

Remove `EveryBeat`, `Follow` and `Anchorless` before a merge, and the
`gps` switch and `[protocol] gps_schedule` with them; `gps_backoff`
becomes a constant.

## Proposed spec text

For spec "Beats", in place of its last paragraph but one ("A sleepy node
may have moved since its last beat, …"); D77 is the next free number, to
be given at the decision:

> A sleepy node runs its GPS only when the radio says it may have moved
> (roadmap D77). It searches when it turns sleepy, when it takes an
> anchor or changes it, when it turns anchorless, and once a delivery
> TTL while it stays anchorless. A search ends at the first fix that
> passes the checks ([position](#position)) or after 60 s, and one that
> finds nothing holds every search back for 8 hours. The fix asked for
> at a new anchor is where the node chose from.
>
> | | |
> |---|---|
> | search limit | 60 s: two of the 30 s frames that carry the satellites' orbits, so that a cold start under the open sky, 30 s, survives one lost frame |
> | after a search that found nothing | none for 8 h: 60 s at the L76K's 29 mA is 0.48 mAh, and 8 h keeps a node that never sees the sky within a tenth of its 0.63 mA |
> | between searches | standby, 20 µA, which keeps what a hot start, 2 s, needs |
>
> *Why only then.* Hearing its anchor each rotation says the node is
> within reach of a fixed relay, which is all its address needs; a new
> anchor, or none, is when it may have moved. Indoors the L76K, which
> acquires 18 dB below the open sky, mostly finds nothing, and a search
> before each beat would hold it on at 29 mA, about a day on the battery.
>
> *Why the fix at a new anchor is where it chose from.* It comes after
> the choice: measured from the position before, a node that moved
> since its last fix would check the anchor it has just chosen.

And under "Delivery", after "A node listens 3.38 s after each beat …",
a budget:

> | A sleepy node with an anchor, S4 | mean current |
> |---|---|
> | listening: windows, the anchor's heartbeats, checks | 0.42 mA |
> | receiving duty-cycled | 0.18 mA |
> | transmitting: beats and its own messages | 0.006 mA |
> | GPS standby | 0.020 mA |
> | GPS searches | 0.003 mA; at most 0.06 mA, one failed search in 8 h |
> | **total** | **0.63 mA, 56 days on 850 mAh; at most 0.69 mA, 51 days** |
>
> The MCU is not counted.

## What I am unsure of

- **Indoor reception** is assumed, not measured: none of the indoor
  nodes under the sky by default, half in the second set. A board at a
  windowsill may well fix. The bench (O16, O18) can log it in a day.
- **The 8 h back-off rests on a tenth** of the battery for a node that
  never sees the sky. It is a choice of what battery life to protect;
  1 h is the user's figure, and costs 9 % on average in S4 and up to
  0.48 mA, 31 days, for a node indoors whose anchor changes every hour or
  that is anchorless.
- **The back-off holds the anchor-change fix back too.** A node that
  failed indoors and then travels more than 12.5 km within 8 hours
  decodes its new anchor's position 25 km off (use 4). Measured nowhere:
  the scenarios' trips are a kilometre.
- **Hot starts need the receiver kept in standby or backup.** Whether
  the T-Echo keeps the L76K's V_BCKP powered, or its WAKEUP pin wired to
  the nRF (the board file has a standby line on P1.02), is a bench item.
  Without it every search is a cold start, 30 s, 0.24 mAh.
- **The IS-GPS-200 section numbers** (the frame, the fit interval, the
  minimum power) are cited from memory, not checked against the
  document; the facts themselves are standard.
- **The figures are for one seed** of S4; the differences between (b),
  (c) and (f) are within what two seeds have shown (D72: ±0.3 points).
- **The firmware has no GPS driver**; `bark-driver` logs `Locate`.
- The other agent's airtime correction moves the radio currents by about
  1 %; the comparisons are within one binary.

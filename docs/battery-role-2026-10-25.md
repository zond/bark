# A fixed relay on battery relays until its battery is low

**Date:** 2026-10-25, updated 2026-10-26. **Status:** adopted
2026-10-26 as roadmap D81, with the flush's bound. The measurements below
were taken on main `1654017` behind a research switch, D71's rule against
this one; since the adoption the switch is gone, the firmware reads its
battery and turns the levels on, and the core calibrates the readings
(The rule). Spec references are to `protocol.md`.

## The problem

Under D71 a node is a relay while on USB power and turns sleepy an hour
after losing it. The network is for blackouts, and in a blackout every
relay in a district loses USB power in the same second, so an hour later
all of them turn sleepy together. The relays that the routing and the
capacity figures assume are gone exactly when they are needed, and every
sleepy node loses its anchor at once. Rolling blackouts repeat this every
day. A solar relay that reads its panel as external power does the same
every night (O17).

The user's proposal: a fixed node on battery stays a relay until its
battery is at about 20 %, then turns sleepy. A reviewer added: only fixed
nodes, since a carried node in a pocket is its owner's and would run flat
in days; and hysteresis, sleepy at about 20 % and a relay again at about
30 % or on external power, since a Li-ion cell's voltage is flat and
noisy in the middle. Also to consider: spreading the grace at random.

## Summary

- **Built**, behind `Config::battery` (off by default): the driver
  reports the battery's voltage (`AppCommand::Battery`); a fixed relay on
  battery relays past its grace until its battery reads at or below
  3.741 V, a typical Li-ion cell at 20 % (Maxim AN4189), then flushes and
  turns sleepy; it relays again at 3.775 V, 30 %, or on external power;
  a carried relay keeps D71's grace; a fixed relay rebooted on battery
  relays on, from a flag in its stored record. In the simulator, every
  node's battery and the grid's outages (`[battery]`), and
  `scenarios/grid-down.toml`. S4 seed 1 repeats main to the byte.
- **In an outage the old rule collapses the domain and the new one does
  not notice it.** Four hours off (S4, seed 1): under D71 the 269 relays
  turn sleepy within five minutes of the grace's end, 16 000 sleepy nodes
  go anchorless, the airtime rises ninety-fold and 7.35 % of the texts
  are delivered until power returns, 68 % in the hour after; under the
  new rule 96.24 % (seeds 2 / 3: 96.57 / 96.12 %), no relay turns. A day
  off: the old rule 8 to 12 % from the first hour to the seventh, the new
  one 96.3 % all day, its relays at 80 % of their charge at the end.
  Four hours off and four on: the old rule collapses in every outage,
  the new one delivers 96.1 to 96.5 % in every phase.
- **The people's nodes lose nothing by it; they lose by the old rule.**
  An anchorless node listens and beats with the long preamble: in (a)'s
  four hours the people's nodes used 1.75 points of their charge under
  D71 against 0.37 under the new rule. None ran flat in any run.
- **In ordinary life it changes almost nothing.** `dense-power.toml`
  with batteries: delivered 97.96 % under both, turns to sleepy 9 against
  67, since spells on battery there end within the grace.
- **The arithmetic: four days, or fifteen hours with the GPS on.** A
  relay draws 6.4 mA; 80 % of 850 mAh lasts 106 h, the remaining 20 %
  keeps the node asleep 11 days. The L76K tracking draws 29 mA, and with
  it a relay reaches 20 % in about 16 h. How a fixed relay on battery
  uses its GPS decides what the rule is worth.
- **The spread of starting charge does not desynchronise the turns**:
  relays on chargers start full. Their loads, cells and readings do,
  over about a day (radio only) or four hours (GPS on). But (d), with the
  GPS on, shows that a spread does not keep the domain alive: while the
  relays fall from three quarters to a third it delivers 64 %, below a
  third 12 % and less, with none as little as under D71. What keeps it
  alive is that the relays last. No spread of the grace is proposed; it
  would only stretch D71's collapse.
- **"20 %" needs calibration.** The SAADC's gain error, up to ±3 % at
  gain 1/6, spans 4 to 57 % of charge at 3.74 V. Scaled so that a rested
  full cell reads 4.177 V, a board is left with the charger's tolerance,
  about 1 %, and turns between 14 and 32 % of charge.
- **Found on the way: an unbounded flush.** A relay leaving flushed
  through its duty budget for hours in a full domain, listening at full
  current, and three relays ran flat. The flush now lasts a delivery TTL
  at most, under either rule (core and spec).

## The arithmetic

### Currents

| Load | Current | Source |
|---|---|---|
| A relay's radio, listening | 5.8 mA | `sniff::RX_CURRENT_UA`, above the SX1262's 5.3 mA in boosted receive (datasheet Table 3-5) |
| A relay's radio, S4's settled hour | 6.38 mA mean, 5.98 to 7.40 | 5.8 mA listening, 32 mA for its airtime, 2.2 % of the time on average and 6.1 % at most (S4 seed 1, main) |
| A sleepy node's radio, S4's settled hour | 0.639 mA mean | the simulator's sleepy-node current (S4 seed 1, main) |
| The battery divider | 12 µA, always | `bark-fw/src/board.rs` |
| The GPS, tracking | 29 mA | Quectel L76K product specification V1.4 (LILYGO T-Echo repository, `datasheet/`) |
| The charger | 500 mA | LILYGO T-Echo README, "Electrical Parameters" |
| The cell | 850 mAh | LILYGO T-Echo README |

The nRF52840, the e-ink display and BLE are left out; what the
microcontroller draws on battery, its high-frequency clock off
(`bark-fw/src/main.rs`), is a bench measurement still to be made.

### How long a fixed relay lasts

A relay draws 6.4 mA (radio and divider). From a full cell:

| | Charge | Radio only | With the GPS tracking |
|---|---|---|---|
| relaying to the low level | 80 %, 680 mAh | 106 h, 4.4 days | 19 h |
| the same, a cell aged to 80 % of its capacity | 544 mAh | 85 h, 3.5 days | 15 h |
| then sleepy, at 0.65 mA | 20 %, 170 mAh | 261 h, 10.9 days | 187 h, 7.8 days, with a 2 s fix before each beat |
| D71's grace | 6.4 mAh, 0.75 % | | |

So the rule turns an hour of relaying into four days, if the GPS does
not run all the time on battery. **With the L76K tracking at 29 mA the
GPS draws four and a half times the radio**, and a relay reaches 20 % in
under a day. Spec "Position" has every node measure from its fixes
whether it moves, one fix a second for the 72 s of far fixes that make it
carried. On battery a fixed relay should fix as a sleepy node does, once
before each heartbeat interval at most, or the rule buys little; that is
a rule of its own, not proposed here.

### Does the spread of charge desynchronise the turns?

Not the way the proposal hoped. A relay on a charger is full or nearly:
the T-Echo's charger tops the cell up whenever it is plugged in, so a
district's relays start an outage within a few percent of each other.
What spreads their turns is everything else:

- their load: a relay at the duty cap draws 7.4 mA, a quiet one 6.0 mA;
- their cells: an aged cell holds less, 80 to 100 % of nominal assumed;
- their readings: each board's reading is off by its calibration error,
  about 1 % after calibration (below), which at the flat 20 % point of
  the curve moves the turn between 14 and 32 % of charge.

Drawing each of these uniformly, with the relays' loads of S4's settled
hour (100 000 draws):

| | first | 10 % | median | 90 % | last | 10 to 90 % |
|---|---|---|---|---|---|---|
| radio only, readings exact | 65 h | 80 h | 90 h | 100 h | 113 h | 20 h |
| radio only, readings within 1 % | 56 h | 76 h | 89 h | 101 h | 119 h | 25 h |
| radio only, readings within 3 % | 33 h | 51 h | 88 h | 111 h | 135 h | 61 h |
| GPS tracking, readings within 1 % | 11 h | 14 h | 16 h | 18 h | 21 h | 4 h |

A district's relays then turn over about a day instead of a second, a
relay every few minutes out of 269. The relays a sleepy node can reach
fall one at a time, and each turn moves its sleepy nodes to a relay that
is still there.

### What outages it covers

How long outages last in the target setting is known only from the
reporting:

- Kyiv's scheduled outages of October 2022 were to last "no more than
  four hours", and a household could be cut again eight hours later
  (DTEK, reported by Ukrainska Pravda, 2022-10-20,
  pravda.com.ua/eng/news/2022/10/20/7372665).
- After the strikes of 2022-11-23, "some districts of the city were
  without power for 36 hours", and afterwards Kyivites "have had access
  to electricity on average for about 12-14 hours a day" (Kyiv Post,
  2022-11-25, kyivpost.com/post/65).

With the radio alone a fixed relay relays through every one of these; on
four hours off and four on it uses 3 % of its cell each time and the
charger puts it back within minutes. What it does not cover is an outage
of more than about four days, and with the GPS on, one of more than about
fifteen hours. How often those happen nobody here knows.

## Measuring the battery

### The cell's curve

A Li-ion cell's open-circuit voltage against its charge (Maxim
Integrated, Application Note 4189, "Characterizing a Lithium-Ion (Li+)
Cell for Use with an Open-Circuit-Voltage (OCV) Based Fuel Gauge",
Table 1: a typical cell, each point read after an hour's rest following
a discharge of 5 % at about 0.2 C):

| Charge | 100 % | 90 % | 80 % | 70 % | 60 % | 50 % | 40 % | 30 % | 20 % | 15 % | 10 % | 5 % | 0 % |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Volts | 4.177 | 4.086 | 4.008 | 3.945 | 3.884 | 3.821 | 3.793 | 3.775 | 3.741 | 3.710 | 3.687 | 3.675 | 3.306 |

The T-Echo's own cell has no published curve. Between 30 and 50 % the
curve rises 46 mV, between 20 and 30 % 34 mV: a percentage near 20 % is a
few tens of millivolts wide.

### The thresholds

| Level | Volts | Charge on the curve |
|---|---|---|
| low: turn sleepy at or below | 3.741 | 20 % |
| high: relay again at or above | 3.775 | 30 % |

A relay's load moves its cell's voltage by its current times the cell's
resistance, assumed a few hundred milliohms for a small pouch cell:
about 2 mV at 6.4 mA, and about 10 mV during a transmission at 32 mA,
which is why the
reading is taken while the radio does not transmit. The gap of 34 mV is
well above that and above a reading's noise, so a node near the low level
does not flap; on the T-Echo the cell rises only on the charger, which
ends the low reading anyway, so the high level matters only for a cell
charged without USB, a solar box's.

### What the board reads

The T-Echo reads its cell on AIN2 (P0.04) behind a 2:1 divider
(`bark-fw/src/board.rs`). The SAADC at its default gain of 1/6 against
the internal 0.6 V reference has a full scale of 3.6 V at 12 bits: a step
of 0.88 mV at the pin, 1.76 mV at the cell. The steps do not matter; the
gain error does. Nordic gives the SAADC's gain error at gain 1/6 as up to
±3 % after calibration (nRF52840 Product Specification, SAADC electrical
specification, as Nordic's support quotes it; not read from the table
here), and the divider's resistors add theirs, of a tolerance the board's
documentation does not give. ±3 % of 3.74 V is ±112 mV, which on the
curve above spans 4 to 57 % of charge. An uncalibrated board cannot tell
20 % from 50 %.

So the readings are calibrated: a cell that has finished charging and rested
reads the curve's 100 % point, 4.177 V. A board that has been on USB long
enough to be full (the 500 mA charger's constant current brings an empty
850 mAh cell to its end voltage within 1.7 h, and the constant-voltage
end takes longer) takes its first reading on battery after the cell has
rested, and scales its readings so that it reads 4177 mV. What is left
is the charger's tolerance on its end voltage, about 1 % for common
charger chips (the T-Echo's is not documented), and the difference
between this cell and the typical one: about ±40 mV, a turn between 14
and 32 % of charge. The LILYGO README warns that the reading is wrong
while the cell charges; the core takes no reading on external power.

## The rule

### In the core's terms

Today the role follows `AppCommand::Power` with a grace
(`relay/power.rs`). The smallest addition is one command and one
parameter:

- `AppCommand::Battery { millivolts }`: the driver reports the cell's
  voltage as its converter reads it, uncalibrated, on battery, once a
  minute, between radio operations, so never while the radio transmits
  (`bark-driver`, `BATTERY_EVERY`).
- `Config::battery: Option<BatteryLevels { low_mv, high_mv }>`, the
  design's 3741 and 3775 mV. `None` only for a driver that reports no
  battery.

Built after the adoption, and changed from the proposal above: the core,
not the driver, calibrates, so that the simulator runs the calibration
too. After 3 h on external power, its first reading an hour into battery
is taken as 4177 mV, and the readings after it are scaled to match; a
scale beyond ±3 % is not taken; the scale is kept in the stored record
across reboots. Before the first calibration the readings are taken as
read: a relay unplugged from a full charge calibrates within its first
hour, days before it is low, so only a discharge begun short of full
turns uncalibrated, anywhere between 4 and 57 % of charge. The firmware
reads AIN2 with eight samples averaged in hardware and its offset
calibrated before each reading (`bark-fw/src/battery.rs`).

The rule, as built:

1. **A relay unplugged keeps its grace.** At its end a carried relay
   turns sleepy as under D71, flushing first. A fixed relay relays on.
2. **A relay on battery whose battery reads at or below the low level
   turns sleepy**, grace or not: it flushes and turns as at the end of a
   grace (`Trace::BatteryLow`), and says so at once with a heartbeat for
   the change, as D71 does for any change of a flag.
3. **The low reading holds** until a reading at or above the high level,
   or external power.
4. **A node its battery turned sleepy relays again at the high level**
   while it is fixed, and any node at once on external power.
5. **A fixed relay carried later** turns sleepy at once if its grace is
   over.
6. **A fixed relay rebooted on battery relays on.** The stored record
   (D73) gains a flag, set while the node is a relay; a node that boots
   on battery with the flag set and not carried is a relay, its grace
   counted from the boot. Without the flag, as a sleepy node's record
   has, it boots sleepy as today.

The heartbeat for the change stays: it is one heartbeat per turn, and
it tells the node's sleepy nodes to move now rather than at their
next missed slot. Under the new rule turns come one at a time, so it
costs nothing that a storm would add.

### The grace, spread at random?

Spreading D71's grace would spread the turns of a district's relays over
the spread. It does not change where they end: once the last relay in
reach has turned, every sleepy node there is anchorless, with or without
a spread. The cost of a turn on the air is small: one heartbeat for the
change, and the turned relay's sleepy nodes take another anchor from
their tables at once, or after a rotation's listen, without a frame of
their own beyond their next beat (spec "Relay choice"). D73's burst
and answers are a node's first rotation, which a turn does not repeat. What costs is anchorless mode, long beats every 34.6 s with
long answers, and only the absence of relays brings it. So a spread of
the grace would only stretch the change, not avoid it, and under the
battery rule the cells, loads and readings spread the fixed relays' turns
over a day by themselves. Carried relays unplugged in a blackout keep the
grace; they are people's nodes on their way somewhere, and none of them
was an anchor (spec "Nodes"). No spread is proposed.

### Across a reboot

D73 keeps the home channel, the place a fixed node stands, the carried
flag and a fixed sleepy node's anchor. Without more, a fixed relay that
reboots on battery during an outage, after a crash or a watchdog reset,
boots sleepy and its sleepy nodes move. Rule 6 keeps it a relay. The flag
is kept only with battery levels, so that a node without them writes no
more than before; with them, a sleepy node writes its record once more at
each boot, between its boot and its power's report.

## The scenario

`scenarios/grid-down.toml` is S4 (`s4-capacity.toml`: 28 km², 269 relays
of which 53 on roofs, 20 000 sleepy nodes indoors, a fifth of them
walking, 23 texts per person a day) with every node's battery
(`[battery]`, `bark-sim/src/battery.rs`):

| | | |
|---|---|---|
| cell | 850 mAh times 0.8 to 1.0, per node | T-Echo README; the share assumed, for aged cells |
| charge at the start | relays 0.9 to 1.0, people's nodes 0.2 to 1.0 | assumed: a charger keeps a relay nearly full; people charge whenever |
| on USB | charges at 500 mA less its load | T-Echo README |
| on battery | the radio's currents for its time listening, receiving duty-cycled and transmitting, and 12 µA for the divider | `bark_proto::sniff`, `bark-fw/src/board.rs`; the GPS and the microcontroller left out |
| reading | the curve above times 1 ± up to 1 %, per node, once a minute | the calibrated error |
| outage | USB gone from every node on it, the 269 relays | |

The outage begins at 3600 s, an hour after the settle time's start at
2700 s, so 900 s of the settled network come before it. Three outages:

- (a) four hours, 3600 to 18 000 s; run to 27 900 s, 2.6 hours after;
- (b) 24 hours, 3600 to 90 000 s; run to 97 200 s, 2 hours after;
- (c) rolling: four hours off and four on, three times, from 3600 s;
  run to 79 200 s, an hour after the last.

And (d): (b) with each relay's GPS tracking all the time, 29 mA more,
and each sleepy node's GPS on 2 s before each 225 s beat, 0.26 mA more;
the only outage here in which relays reach the low level.

Each under the old rule (`[protocol] battery_levels = false`, D71: every
relay sleepy an hour after its power goes) and the new one (`true`).

**What had to be coarsened.** A run of S4 samples its metrics every
second; these sample every minute, which keeps a day's run at about
4 GB. Under the new rule a day's run takes a little over an hour. Under
the old rule it does not finish in reasonable time: once the relays are
gone, 16 000 anchorless nodes beat and send with the long preamble, the
domain's airtime rises about a hundredfold, and an hour of it takes about
half an hour to simulate. So the old rule's (b) stops at 32 400 s, seven
hours into the outage and six after the turn, and its (c) after two of
the three outages, at 50 400 s; both had reached a steady state by then,
and the return of power after a day is the return after four hours,
which (a) has.

Delivered is of the texts sent in each phase, delivered within an hour
of their sending; the texts of the run's last hour are left out, so that
runs of different lengths compare. Latency is of those delivered.
Airtime is the whole domain's, per hour of the phase.

## The measurements

Seed 1 unless said; the new rule also seeds 2 and 3 where it acts or
where a run is short. "Relays" is the nodes not sleepy, "anchorless" the
sleepy nodes without an anchor, at their most in the phase. Airtime is
the domain's, seconds an hour: all of it, then the part that is DATA with
the long preamble and the part that is anchorless beats.

### (a) Four hours off

| Phase | Rule | Texts | Delivered | Latency p50 / p90 | Relays | Anchorless | Airtime: all; long DATA; anchorless beats |
|---|---|---|---|---|---|---|---|
| before, 2700–3600 s | old | 4447 | 95.77 % | 1.7 / 206 s | 269 | 32 | 36 160; 9814; 973 |
| | new | 4447 | 95.88 % | 1.7 / 210 s | 269 | 32 | 36 160; 9814; 973 |
| the grace hour, 3600–7200 s | old | 17 449 | 94.61 % | 1.6 / 185 s | 269 | 22 | 34 463; 9704; 676 |
| the rest of the outage, 7200–18 000 s | old | 51 779 | 7.35 % | 389 / 661 s | **0** | **16 136** | **3 149 053**; 2 582 819; 446 742 |
| the whole outage, 3600–18 000 s | new | 70 237 | 96.24 % | 1.6 / 229 s | 269 | 22 | 31 462; 9674; 405 |
| power back, first hour | old | 17 237 | 67.87 % | 62 / 698 s | 269 | 15 927 | 692 168; 459 305; 43 668 |
| | new | 17 403 | 96.52 % | 1.6 / 233 s | 269 | 6 | 29 366; 9543; 216 |
| after, 21 600–24 300 s | old | 12 965 | 95.96 % | 1.8 / 230 s | 269 | 17 | 32 631; 9666; 488 |
| | new | 12 963 | 95.83 % | 1.6 / 216 s | 269 | 5 | 28 585; 9365; 138 |

New rule, seeds 2 / 3: the outage 96.57 / 96.12 %, the hour after
96.49 / 96.37 %, no turns, 269 relays throughout, 30 / 39 anchorless at
most.

Under the old rule the 269 relays turn sleepy within five minutes of the
grace's end (265 relays at 7200 s, 1 at 7500 s), and the sleepy nodes go
anchorless over the next hours: 693 at 7500 s, 9363 at 8700 s, 16 136 at
the end of the outage. The domain's airtime rises 90-fold, almost all of
it long DATA and anchorless beats, and the first half hour after the
turn delivers 1.5 %. When power returns, the relays are relays at once,
and the anchorless nodes find them over half an hour: 4859 left after
five minutes, 412 after 25, 30 after 45. The first half hour after the
return delivers 41 %, the second 94 %.

### (b) 24 hours off

| Phase | Rule | Texts | Delivered | Latency p50 / p90 | Relays | Anchorless | Airtime: all; long DATA; anchorless beats |
|---|---|---|---|---|---|---|---|
| hours 1 to 4 of the outage, 7200–18 000 s | old | 51 779 | 7.60 % | 391 / 670 s | 0 | 16 138 | 3 149 053; 2 582 819; 446 742 |
| | new | 52 787 | 96.31 % | 1.6 / 230 s | 269 | 12 | 30 517; 9669; 322 |
| hours 4 to 7, 18 000–28 800 s | old | 51 123 | 12.34 % | 390 / 681 s | 0 | 16 981 | 4 090 034; 3 418 354; 563 410 |
| | new | 52 443 | 96.18 % | 1.6 / 223 s | 269 | 6 | 28 946; 9542; 149 |
| hours 7 to 24, 28 800–90 000 s | new | 298 123 | 96.29 % | 1.6 / 224 s | 269 | 2 | 27 945; 9579; 39 |
| power back, first hour | new | 17 504 | 96.55 % | 1.5 / 208 s | 269 | 1 | 27 751; 9516; 37 |

Under the new rule a day on battery took the relays from full to 79.9 %
on average, 75.9 % the emptiest, the 6.4 mA of the arithmetic; none
turned, and the charger had them back above 90 % on average within ten
minutes of the return.

The old rule's run stops seven hours into the outage (The scenario);
the state it has reached by then lasts to the end of the outage, and the
return is (a)'s.

### (c) Four hours off, four on, three times

| Phase | Rule | Delivered | Relays | Anchorless |
|---|---|---|---|---|
| first outage: grace hour; after it | old | 94.61 %; 7.35 % | 269; 0 | 22; 16 136 |
| first outage | new | 96.24 % | 269 | 22 |
| power back, first hour; the rest | old | 67.87 %; 96.08 % | 269 | 15 927; 17 |
| | new | 96.52 %; 96.09 % | 269 | 6; 5 |
| second outage: grace hour; after it | old | 95.03 %; 5.73 % | 269; 0 | 6; 14 668 |
| second outage | new | 96.31 % | 269 | 1 |
| power back, first hour; the rest | new | 96.35 %; 96.34 % | 269 | 1 |
| third outage | new | 96.22 % | 269 | 1 |

Each outage under the old rule is (a) again: 269 turns to sleepy and
269 back each time, 538 of each by the second return. Under the new rule
no relay turns: four hours on battery take 3 % of a cell and the charger
gives it back in minutes.

### (d) 24 hours off with the GPS on all the time

The new rule, with the flush's bound (below), seed 1. The old rule is
(b)'s: the GPS changes only the batteries, and the old rule's relays
turn at the grace's end whatever they hold.

| Window | Relays | Texts | Delivered | Latency p50 / p90 | Anchorless | Airtime: all; long DATA; anchorless beats |
|---|---|---|---|---|---|---|
| hours 0 to 13, 3600–50 400 s | 269 | 227 623 | 96.26 % | 1.6 / 227 s | 22 | 29 235; 9590; 178 |
| hours 13 to 16 | 269 to 204 | 52 574 | 93.61 % | 2.8 / 301 s | 40 | 40 883; 9567; 193 |
| hours 16 to 17.5 | 204 to 90 | 26 143 | 64.34 % | 96 / 663 s | 189 | 71 407; 10 212; 2665 |
| hours 17.5 to 19 | 90 to 25 | 25 368 | 12.09 % | 527 / 1256 s | 2998 | 270 907; 141 363; 34 724 |
| hours 19 to 21 | 25 to 0 | 34 441 | 2.49 % | 425 / 758 s | 10 907 | 2 347 241; 1 885 055; 298 605 |
| hours 21 to 24 | 0 | 51 973 | 3.81 % | 392 / 616 s | 11 617 | 3 104 509; 2 536 050; 394 333 |
| power back, first half hour | 269 | 8619 | 22.18 % | 510 / 1224 s | 11 501 | 1 789 735; 1 386 804; 135 511 |
| second half hour | 269 | 8737 | 83.27 % | 63 / 549 s | 1551 | 290 309; 180 156; 19 338 |

The relays reach the low level between 13.1 and 20.5 hours into the
outage, a tenth of them by 15.2 h, half by 17.0 h, nine tenths by
19.0 h: the arithmetic's 11 to 21 h with a median of 16 h. The turns are
spread, and the heartbeat for each is one frame; yet delivery holds only
while most relays remain. With three quarters or more left it delivers
94 %, while they fall from three quarters to a third 64 %, below a third
almost nothing, and once the last has turned the domain is where the old
rule put it at the end of its grace.
Spreading the turns does not keep the network alive; it only avoids the
moment when all go at once. What keeps it alive is that the relays keep
relaying, for as long as their batteries last.

Seeds 2 / 3 (seed 1): the first 13 hours 96.44 / 96.27 % (96.26 %); the
whole outage 69.68 / 70.40 % (69.61 %); the first turn 13.5 / 13.6 h
into the outage, half 17.2 / 17.2 h, the last 20.3 / 20.2 h; the hour
after the return 53.43 / 54.09 % (52.93 %); no cell flat.

### The people's nodes

The charge of the 20 000 people's nodes, as a share of their cells, mean
and lowest:

| | start, 2700 s | end of (a)'s outage, 18 000 s | (b) at 32 400 s | (b) at the end of its outage, 90 000 s |
|---|---|---|---|---|
| old rule | 60.07 %, 20.01 % | 58.32 %, 16.96 % | 55.89 %, 12.94 % | |
| new rule | 60.07 %, 20.01 % | 59.70 %, 19.21 % | 59.37 %, 18.22 % | 58.03 %, 13.68 % |

The new rule costs the people's nodes nothing; the old one costs them
five times their usual drain while the relays are gone, since an
anchorless node listens and beats with the long preamble: 1.75 points in
the four hours of (a) against 0.37. With the GPS on, (d), they used 5.4
points in the day of the outage, and the lowest stood at 12 % at its
end. No people's
node ran flat in any run.

### Ordinary life: `dense-power.toml`

D71's scenario, where a quarter of the people's nodes are on USB and a
fifth of the others switch between USB and battery, with `[battery]` as
in grid-down and no outage, seed 1, the settled hour:

| | old rule | new rule |
|---|---|---|
| delivered | 97.96 % (17 861 / 18 233) | 97.96 % (17 862 / 18 234) |
| graces ended; turns to sleepy | 67; 67 | 1; 9 |
| sleepy nodes moved off an anchor turned sleepy | 29 | 7 |
| heartbeats for a change | 1921, 25.9 s | 1865, 24.8 s |
| all airtime over the run | 260 253 s | 253 774 s |
| relays on battery at the end | 1121 | 1165 |

A spell on battery there is half an hour on average, so the grace ends
rarely under either rule; the new rule turns the fixed ones that outlast
it into relays on battery instead of sleepy nodes, and the measured
difference is within the noise but for the turns. These two runs predate
the flush's bound (below); no relay there flushed for long.

### The flush

In (d) as first run, three relays ran flat while flushing: the flush
goes through the duty budget as any delivery, and in a domain full of
long frames a relay leaving waited for its budget for hours, listening
at full current, its battery already low. 25 to 41 relays were leaving
at once for two hours. The flush now lasts a delivery TTL at most;
spec "Sleepy nodes" said only "until each message has had its attempt",
and is edited on this branch. What the flush has not done by then is
dropped, as what it did not deliver already was. Run again with the
bound, (d) has no cell flat, at most 9 relays leaving at once, and the
emptiest relay at 12 % when the last one turned and at 10 % at the end
of the outage; delivered over the whole outage 69.61 % against 70.26 %
without the bound (texts sent in the outage, delivered within an hour),
since the bound drops what a flush held up would have delivered late.
In S4 no relay leaves, and S4 seed 1 repeats main to the byte with the
bound.

## The spec text, proposed

For spec "Sleepy nodes", where D71's grace lives ("Plugged in and
unplugged"), in place of the paragraph that begins "A relay unplugged
changes nothing", and the "Sleepy" item of "Nodes" to match ("it is on
battery, past a grace of an hour if it is carried, or with its battery
low"):

> A relay unplugged changes nothing for a **grace** of one hour: it
> relays, owns and holds mail as before, and power back within the grace
> is as if nothing happened. After the grace a carried relay turns
> sleepy; a fixed one relays on until its **battery is low**. A relay on
> battery whose battery is low turns sleepy, grace or not. A node its
> battery turned sleepy relays again when its battery is high again
> while it is fixed, and any node on external power.
>
> | | |
> |---|---|
> | battery low | a reading at or below 3.741 V, until one at or above the high level or external power |
> | battery high | a reading at or above 3.775 V |
> | reading | the cell's voltage while the radio does not transmit, once a minute, scaled so that the cell rested after a full charge reads 4.177 V |
> | after a reboot | a fixed node that was a relay and boots on battery is a relay, its grace counted from the boot |
>
> *Why a fixed relay relays on.* In an outage every relay in a district
> loses its power in the same second, and if each turned sleepy an hour
> later, every sleepy node there would lose its anchor in the same minute
> and go anchorless (`docs/battery-role-2026-10-25.md`). On battery a
> fixed relay draws 6.4 mA, and 80 % of its 850 mAh lasts it four days;
> its owner's node keeps a fifth of its charge, eleven days asleep.
>
> *Why a carried relay keeps the grace.* A node in a pocket is its
> owner's, and nobody anchors to it; relaying, it would be flat in days.
>
> *Why those levels.* They are a typical Li-ion cell's voltage at 20 and
> 30 % of its charge (Maxim AN4189, Table 1). The curve is flat there,
> 34 mV between them, and a board's reading is off by up to 3 % until it
> is scaled at a full charge; scaled, by the charger's 1 %, a turn between
> 14 and 32 % of charge. The gap is several times a reading's noise and
> the load's drop across the cell, so the role does not flap; the cell
> rises without external power only on a solar box.

The heartbeat on a change, the flush with its bound, and the rest of
the paragraph stay as they are. A reading on external power is not
taken: the charger holds the cell's voltage up.

## What this does not show

- **The cell.** The curve is Maxim's typical cell, not the T-Echo's,
  whose maker publishes none; the levels are to be checked against a
  T-Echo's own discharge on the bench, with the board's load.
- **The reading.** The SAADC's ±3 % is Nordic's support quoting the
  product specification, not read from its table here; the divider's
  tolerance and the charger's end voltage on the T-Echo are not
  documented. The ±1 % after calibration is an assumption until a bench
  measures a few boards. Cold moves a cell's curve and its capacity;
  nothing here models temperature.
- **The board's current.** The microcontroller's share, BLE and the
  display are left out, and the GPS is either all the time or not at all.
  How a fixed relay on battery uses its GPS decides whether the rule buys
  four days or fifteen hours.
- **How people charge.** The relays' starting charge, the people's spread
  of 20 to 100 %, and the cells' ageing are assumptions.
- **Long outages.** Beyond about four days the relays go anyway, and (d)
  shows what follows: below a third of the relays the domain delivers an
  eighth of its texts, and with none, 11 000 to 16 000 anchorless nodes
  fill the channels with long frames. Anchorless mode was designed for a
  node that hears no relay, not for a district of them; a district
  without relays is a problem of its own, which this rule only
  postpones.
- **The old rule's long runs** stop after seven hours of the outage, and
  its rolling run after two outages (The scenario).
- **Seeds.** Seeds 2 and 3 for the new rule in (a) and (d); one seed for
  the rest, and for the old rule, whose collapse is a hundred times the
  differences between seeds.
- **Firmware.** Built since: the board reads its battery once a minute
  on battery and the levels are on, but nothing has run on a board. The
  curve of a T-Echo's own cell and the readings after calibration are
  Phase 2 bench rows (roadmap D81).

## Recommendation

1. **Adopt the rule**: a fixed relay on battery relays until its
   battery is low, 3.741 V, and relays again at 3.775 V or on external
   power; a carried relay keeps the grace; a fixed relay rebooted on
   battery relays on (spec text above). It turns an outage of up to
   four days from a collapse into nothing, costs the people's nodes
   nothing, and changes nothing measurable in ordinary life. It also
   answers O17: a solar box relays on its battery through the night,
   whatever it reads as external power, until the battery is low.
2. **Adopt the flush's bound**, a delivery TTL at most, whatever is
   decided about 1; it is already in the spec on this branch.
3. **The firmware's reading**: once a minute while the radio does not
   transmit, calibrated at a full charge; a bench measures a T-Echo's
   own discharge at a relay's load to check the levels. Built since
   (The rule).
4. **Decide how a fixed relay on battery uses its GPS.** At one fix a
   second the rule buys fifteen hours instead of four days.
5. **No spread of the grace.**
6. **Open: a district with no relay left.** Beyond about four days on
   battery, or fifteen hours with the GPS on, the relays go anyway, and
   thousands of anchorless nodes fill the channels as under D71.
   Anchorless mode was made for a few nodes out of reach, not a
   district; that is a question of its own.

The 20 % itself is the user's number. A higher one leaves the owner more
of a node that relayed through the outage and turns the relays sooner;
at 20 % a node turned sleepy lasts eleven days more.

The variant built and not recommended, D71's grace alone under a
battery model, was the switch's `None`; with the adoption the switch is
gone and the levels are the design's.

# Transmit power control per hand-over

**Status:** research note, 2026-11-02. Roadmap D99. Measured against main
`8ff3a74` (D93 in; before D96). Not adopted; the code was removed after the
decision, as measured and rejected code is, and is kept on the branch
`worktree-agent-a3c80232a915e8727-code` at commit `2a67e3b`.

## Summary

A DATA goes at the least power that still reaches its receiver with the
margin of roadmap D38, in the SX1262's 1 dB steps down to −9 dBm, from
the receiver's averaged RSSI; heartbeats and ACKs stay at full power. It
was built in the core, the contract and the drivers, and measured
against main `8ff3a74`.

At the design rate it costs. The city delivers 97.8 → 97.5 % (seeds 1, 2
and 7 within 0.1 point of each other), latency at the median rises by
half, airtime by 7 to 9 %, failed hand-overs by a quarter; S4 93.6 →
92.8 %, and at three times the rate 63.6 → 52.9 % with 17 % more airtime;
the movers 96.5 → 94.9 %; the ring and the mesh within their noise. In the
city at twice the rate, where main is in the storm, it gains: 88.8 → 93.2
and 89.1 → 92.2 %, airtime −23 and −16 %, split copies −45 %.

Why: the margin is derived against noise and one fade, not against other
frames. At full power a DATA beats almost every frame that arrives after
it; at the margin it loses to any of them, and each loss is a retry wait,
three a ban and a demotion. The DATA lost at a receiver that was free
when they started rise from 43 to 52 % of those sent, while the
collisions the receiver saw coming hardly move. A quarter of the DATA
need full power and half are within 3 dB of it: the forwards go to the
neighbour nearest the destination, the farthest in reach, and have
little power to give. The ACK at the DATA's power is worse than no
power control at all: the ACKs lost at a free receiver go from 15 to
24 % at the design rate, and delivery under main's at both rates. Five
decibels of headroom over the margin, on the
two seed-1 cities only, gives the design-rate loss back (97.7 %) and
keeps half the overload gain (91.3 %).

Not adopted as built; the spec is unchanged. The ACK at the DATA's power is
rejected outright. The headroom form is a possible follow-up only after
the bench has measured the capture threshold at SF5, and after a
re-measurement on a base with D96, since the gain at twice the rate was
the storm's and D96 has since changed that base. The code is on the
side branch named above.

## The question

Every frame goes at one power, `Config::tx_power_dbm`, +10 dBm, the
band's limit (spec "Regulatory limits"). In the dense city of
`scenarios/dense-power.toml` the frames that are lost are lost to each
other: every node is heard by every node within its full reach, and a
DATA frame to a neighbour 50 m away disturbs the same 3 km as one to a
neighbour at the edge of reach (`storm-2026-10-30.md`,
`storm-wait-2026-10-31.md` "The fates", `storm-ack-2026-10-31.md`).

A sender knows each neighbour's RSSI from its heartbeats (spec
"Neighbour table", "Heartbeat"), and the spec already derives from the
radio how much RSSI a full frame needs to get through a fade (roadmap
D38; `radio.rs` `FULL_FRAME_ABOVE_SENSITIVITY_DB`, `FADE_DEPTH_DB`,
`full_frame_rssi_above_noise_db`). The question is whether a DATA sent
at the least power that still meets that margin, and no more, cuts the
collisions enough to show in delivery and airtime, and what it costs
where links are long.

Power control has been measured twice before and not adopted. D40
measured, among other rules, heartbeats at full power and every other
frame at the power its receiver's loss needs, on S4 at twice the design
rate: 94.6 against 94.4 %, no change, because there a relay's duty
budget bound before the channel did. D68 measured one operating power
per relay, stepped by how many relays it heard with margin, among dense
relays, and rejected it: the relays that heard most turned down and
stayed down, and they carried the long hops. Since then the dense-power
scenarios have shown the channel binding (`storm-2026-10-30.md`): the
case D40 could not test. This note measures D40's per-receiver rule, as
the design now stands, where the channel binds.

## The rule

A DATA frame goes at the least power, in whole steps, at which its
receiver would still have margin for a full frame through a fade. The
sender reads the receiver's averaged RSSI from its table and the noise
floor it has calibrated, and sends `tx_power_dbm` less the whole
decibels by which the RSSI exceeds the margin, bounded below by the
amplifier's floor:

| | |
|---|---|
| the margin | the noise floor + 4.1 dB, the RSSI at which a 255-byte frame arrives 99 times in 100 after a 4 dB fade (spec "Neighbour table", roadmap D38) |
| the step | 1 dB, the SX1262's SetTxParams step (datasheet §13.4.4) |
| the floor | −9 dBm, the least SetTxParams accepts (datasheet §13.4.4) |
| the ceiling | `Config::tx_power_dbm`, +10 dBm, the band's limit (spec "Regulatory limits") |
| a neighbour without margin | full power: the rule sends no less than the margin asks, only no more |
| a node whose RSSI the sender has not measured | full power: a parent known only by its address |

The RSSI is the average the neighbour table keeps (spec
"Neighbour table"), of the frames the neighbour sent at full power: its heartbeats,
and its ACKs. A DATA no longer feeds that average: sent at the power its
receiver needs, its RSSI says nothing of the link's loss. The heard
table's RSSI, of a sleepy node's beats, serves a delivery the same way.

**What stays at full power, and why.**

- Heartbeats: they are what every neighbour measures the node by, so
  the measurement is of a full-power frame, and a sleepy node's beats
  likewise.
- ACKs, in form A, which the measurement keeps; see below.
- A frame to a node whose RSSI the sender does not have.

**Why the link may be taken as symmetric.** The path between two
antennas on one frequency has the same loss both ways. What differs is
the two ends' noise floors, which the sender cannot know, and the
fading on the two channels, since the sender hears the neighbour on its
own home channel and sends on the neighbour's. The rule uses the
sender's own floor for the neighbour's, and the margin of D38 was
derived for exactly this cross-channel case: it covers one standard
deviation of the fade. What the assumption costs when it fails is a
hand-over's three attempts and a ban, after which the neighbour is
demoted (spec "Hand-over" step 6, "Neighbour table"), and its ACKs, when
it does answer, count toward its margin again. A link that is asymmetric
by more than the margin is one the ACK tells on, as it does today for a
neighbour heard by a fade.

**What the measurement's RSSI does not cover.** The packet RSSI is signal
and noise together (datasheet §13.5.3), so lowering the signal by k dB
lowers the RSSI by less than k; the rule treats it as k, which errs
toward more power. The setting's and the reading's own errors, which the
datasheet does not bound beyond the 1 dB step and the 0.5 dB RSSI
resolution, go in one for one; the bench measures them.

## What else reads the power

| | |
|---|---|
| the duty-cycle bucket (spec "Duty cycle cap") | airtime, unchanged: the band's duty cycle counts time on air whatever the power |
| the frame | as built, each frame carried its own conducted power, set with its channel and preamble before each attempt, and the contract's rule for `Transmit` (`core-interface.md` §3) said the driver transmits at it, never below −9 dBm nor above `Config::tx_power_dbm`; the driver passed it to the chip, which the lean sequencer already sets only when the power differs from the one the chip holds (`bark-driver/src/sx1262.rs`) |
| the simulator's links (`bark-sim/src/links.rs`) | built at full power; as built, each hearer's level was lowered, when a frame reached the air, by what the frame's power was under the full power the links were built at. No link appears that full power did not make |
| the simulator's capture (`bark-sim/src/phy.rs`) | modelled: interference `capture_db` under a packet costs it what noise at sensitivity does, and a second preamble steals the lock if stronger by that much. `capture_db` is 5 dB, the datasheet's co-channel rejection `CCR_L` at SF7 (Table 3-8), measured there with a tone, not a LoRa interferer, and not given for SF5. The 6 dB often quoted for same-SF capture is of the same order; the bench measures it |
| the neighbour's RSSI of us | from our heartbeats and ACKs only, above |
| the simulator's current (`bark-sim/src/battery.rs`, `summary.rs`) | one transmit current, 32 mA, the least the datasheet lists for the SX1262 (Table 3-6, +14 dBm), at every power. The datasheet gives the amplifier's current at a few powers, none under +14 dBm, so the model cannot say what −9 dBm draws; the currents in the tables below move only with airtime |

## The ACK's power: two forms

An ACK has one recipient, the sender of the DATA it answers, which is
listening for it on its home channel for 100 ms (spec "The ACK"). Two
forms were measured:

- **A, the ACK at full power.** The case: the ACK is 17 ms, so what it
  disturbs is small; losing it costs a full DATA retry and, when the take
  was final, a second searcher (spec "The ACK", roadmap D93); and at full
  power it arrives at the sender above a DATA that came in at controlled
  power by as much as that DATA was turned down, which is the margin
  capture needs.
- **B, the ACK at the DATA's power.** The receiver knows the sender's
  RSSI as it knows any neighbour's, and sends the ACK at the power a DATA
  to it would get; a research switch of the core, set by a scenario
  override. The
  case: with every DATA turned down, a full-power ACK is the one frame
  still heard by everyone in reach. In this form an ACK no longer feeds
  the neighbour's RSSI average either.

Measured on dense-power seed 1 and the doubled-rate city seed 1 (the
tables below, columns A and B): form B loses the ACK far more often, and
every lost ACK is a DATA retry at a receiver that took the packet. On
seed 1 at the design rate the ACKs lost at a free receiver go from 14.9 %
(A) to 24.4 % (B), the hand-overs that failed with the receiver holding
the packet from 120 000 to 381 000, the airtime from 309 000 to 577 000 s
and delivery from 97.5 to 97.1 %. At twice the rate the ACKs lost at a
free receiver go from 17.5 % (A) to 27.2 % (B), more than main's 22.4 %,
and delivery from 93.2 to 87.1 %, under main's 88.8 %: form B gives back
everything the DATA's power control gained and more. The case for A holds: the
ACK competes at the sender with frames that arrive there at full power,
heartbeats and other nodes' ACKs among them, and a frame turned down to
the margin loses to any of them; a full-power ACK wins by the amount
the controlled DATA gave up. Form B is rejected: whatever is done with
the DATA's power, the ACK stays at full power.

## How it was measured

Release simulators of main `8ff3a74` and the branch (form A; form B and
the headroom by scenario overrides of the two research switches), the
same scenarios and seeds, two runs at a time. Main's ring run matched another agent's
main run at `8ff3a74` byte for byte, so every main column below is that
agent's run; the branch's runs were run here.

| run | scenario | seed |
|---|---|---|
| dp1, dp2, dp7 | `scenarios/dense-power.toml`: 28 km² of city, 7223 of the people's nodes switching between USB and battery, a fifth of the people walking, one settled hour after 2700 s | 1, 2, 7 |
| dpx2-s1, dpx2-s7 | the same at twice the texts per node | 1, 7 |
| s4x1 | `scenarios/s4-capacity.toml` at the design rate | 1 |
| s4x3-s1 | S4 at three times the rate | 1 |
| ring | `scenarios/ring.toml` | 1 |
| movers | `scenarios/dense-movers.toml` | 1 |
| mesh-burst-s1 | `scenarios/mesh-burst.toml` | 1 |
| dp1-ackc, dpx2-s1-ackc | dp1 and dpx2-s1 with form B, the ACK at the DATA's power | 1 |

The figures are those of the storm notes (`storm-wait-2026-10-31.md`),
with the powers used and the simulator's currents added. The frame fates
are of frames addressed to nodes that are not sleepy: received; lost
while the receiver listened and was free of any frame when it started,
which is a loss to noise, to a fade, or to a stronger frame that came
later; lost while the receiver was receiving another frame when it
started, the collision the receiver saw coming; lost while it was deaf;
or sent while it listened elsewhere.

## The tables

Each cell is main / D99 (form A). The city at the design rate and at twice it:

| | dp1 | dp2 | dp7 | dpx2-s1 | dpx2-s7 |
|---|---|---|---|---|---|
| texts sent in the settled hour | 18234 / 18233 | 18199 / 18198 | 18189 / 18189 | 36559 / 36622 | 36595 / 36662 |
| delivered, settled hour % | 97.8 / 97.5 | 98.1 / 97.8 | 98.1 / 97.5 | 88.8 / 93.2 | 89.1 / 92.2 |
| delivered, whole run % | 96.5 / 96.2 | 96.7 / 96.3 | 96.9 / 96.4 | 90.5 / 93.4 | 90.4 / 92.5 |
| latency p50 s | 10.2 / 15.6 | 10.0 / 15.6 | 10.0 / 15.3 | 46.5 / 38.9 | 49.1 / 42.1 |
| latency p90 s | 47.4 / 72.5 | 43.2 / 70.2 | 45.8 / 70.9 | 349.6 / 274.8 | 349.5 / 291.1 |
| airtime, whole run s | 283128 / 308618 | 283210 / 302541 | 271385 / 295972 | 1118451 / 860928 | 1036475 / 871438 |
| hand-overs taken | 417166 / 417464 | 421560 / 419222 | 404816 / 409282 | 1935441 / 1376187 | 1832111 / 1407156 |
| hand-overs failed | 480395 / 606493 | 477520 / 584021 | 453585 / 571497 | 2777351 / 2265629 | 2540036 / 2294938 |
| failed, receiver had it | 108676 / 119622 | 114376 / 113427 | 101670 / 106687 | 559603 / 361949 | 496462 / 369526 |
| returns taken from a child | 3193 / 2800 | 3510 / 2379 | 2880 / 2264 | 10038 / 4908 | 8775 / 5242 |
| copies answered (not returns) | 246311 / 203834 | 253003 / 198636 | 214802 / 178344 | 1037572 / 565735 | 933784 / 568622 |
| of those merged (case 4) | 2108 / 1773 | 2216 / 1722 | 1823 / 1573 | 36174 / 11731 | 32295 / 12460 |
| of those seen (case 6) | 244127 / 201970 | 250721 / 196839 | 212914 / 176707 | 1001196 / 553835 | 901277 / 555967 |
| seen ACKs sent / acted on | 275948 / 233731 / 227288 / 193420 | 284166 / 239851 / 220933 / 189093 | 241349 / 203739 / 198392 / 169621 | 1161943 / 947962 / 620306 / 531239 | 1040011 / 856442 / 622870 / 533758 |
| hand-overs held back by the bucket | 18179 / 25635 | 18324 / 20805 | 17495 / 19746 | 1902851 / 1057944 | 1804117 / 1176134 |
| neighbours demoted | 201105 / 258228 | 198509 / 249732 | 190020 / 245422 | 1185913 / 990329 | 1083714 / 1001903 |
| copies dropped: hop cap | 1502 / 1118 | 1641 / 1097 | 1314 / 960 | 2692 / 1646 | 2689 / 1676 |
| copies dropped: TTL | 687 / 910 | 823 / 802 | 658 / 755 | 38725 / 8809 | 33357 / 10629 |
| copies dropped: return seen | 1 / 0 | 0 / 0 | 2 / 0 | 1 / 0 | 1 / 0 |
| copies dropped: no route | 23 / 22 | 33 / 17 | 16 / 19 | 62 / 35 | 63 / 49 |
| copies dropped: return failed | 48 / 62 | 38 / 52 | 29 / 57 | 233 / 182 | 195 / 164 |
| texts lost: hop cap | 261 / 193 | 267 / 228 | 222 / 184 | 386 / 315 | 426 / 350 |
| texts lost: TTL | 78 / 103 | 90 / 114 | 80 / 117 | 1741 / 809 | 1599 / 922 |
| texts lost: return seen | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 |
| texts lost: no route | 4 / 5 | 4 / 5 | 5 / 4 | 14 / 11 | 19 / 15 |
| texts lost: return failed | 3 / 8 | 3 / 8 | 2 / 4 | 48 / 41 | 35 / 48 |
| texts that waited at their origin | 80 / 84 | 87 / 82 | 48 / 91 | 8430 / 1452 | 7970 / 1769 |
| relays ever at their cap | 73 / 117 | 86 / 84 | 73 / 89 | 813 / 584 | 726 / 615 |
| largest settled airtime of a node % | 4.87 / 5.00 | 4.10 / 4.09 | 4.24 / 4.30 | 9.99 / 9.83 | 9.99 / 9.88 |
| heartbeats received % | 85.5 / 86.0 | 85.8 / 86.3 | 85.8 / 86.2 | 67.8 / 75.4 | 69.8 / 75.6 |
| DATA mean power dBm / at full % | - / 6.1 / 24.2 | - / 6.1 / 24.6 | - / 6.1 / 24.7 | - / 5.9 / 23.2 | - / 6.0 / 23.3 |
| ACK mean power dBm / at full % | - / 10.0 / 100.0 | - / 10.0 / 100.0 | - / 10.0 / 100.0 | - / 10.0 / 100.0 | - / 10.0 / 100.0 |
| a sleepy node's radio current mA (mean / median) | 2.325 / 0.543 / 2.329 / 0.542 | 2.322 / 0.544 / 2.325 / 0.542 | 2.289 / 0.543 / 2.293 / 0.541 | 2.625 / 0.743 / 2.503 / 0.575 | 2.554 / 0.717 / 2.468 / 0.569 |
| a relay's radio current mA | 5.916 / 5.927 | 5.914 / 5.923 | 5.913 / 5.926 | 6.426 / 6.249 | 6.364 / 6.254 |

S4, the ring, the movers and the mesh:

| | s4x1 | s4x3-s1 | ring | movers | mesh-burst-s1 |
|---|---|---|---|---|---|
| texts sent in the settled hour | 17457 / 17454 | 48831 / 48288 | 2964 / 3027 | 17893 / 17893 | 100 / 100 |
| delivered, settled hour % | 93.6 / 92.8 | 63.6 / 52.9 | 92.6 / 92.8 | 96.5 / 94.9 | 99.0 / 98.0 |
| delivered, whole run % | 93.6 / 92.8 | 69.9 / 60.1 | 93.3 / 93.3 | 95.6 / 94.3 | 99.0 / 98.0 |
| latency p50 s | 1.4 / 2.6 | 83.0 / 171.1 | 41.5 / 59.1 | 5.4 / 9.0 | 3.5 / 2.9 |
| latency p90 s | 187.4 / 215.6 | 582.0 / 675.3 | 159.2 / 152.9 | 52.8 / 88.5 | 8.9 / 6.7 |
| airtime, whole run s | 43498 / 45758 | 150125 / 174948 | 1807 / 1819 | 184045 / 200673 | 232 / 225 |
| hand-overs taken | 116504 / 120699 | 391122 / 402415 | - / - | 321282 / 332687 | 623 / 585 |
| hand-overs failed | 18614 / 28056 | 139390 / 195614 | - / - | 286420 / 358692 | 82 / 77 |
| failed, receiver had it | 3406 / 4081 | 28661 / 27358 | - / - | 154020 / 167895 | 45 / 44 |
| returns taken from a child | 41 / 33 | 340 / 254 | - / - | 3312 / 2861 | 1 / 1 |
| copies answered (not returns) | 7792 / 6225 | 43049 / 48764 | - / - | 213649 / 190580 | 158 / 129 |
| of those merged (case 4) | 26 / 17 | 1139 / 991 | - / - | 1639 / 1545 | 1 / 1 |
| of those seen (case 6) | 7766 / 6208 | 41910 / 47773 | - / - | 211976 / 189008 | 157 / 128 |
| seen ACKs sent / acted on | 8405 / 7681 / 6655 / 6137 | 47328 / 41057 / 51724 / 47077 | 0 / 0 / 0 / 0 | 242252 / 201958 / 215111 / 180265 | 162 / 157 / 130 / 127 |
| hand-overs held back by the bucket | 46 / 94 | 710046 / 991124 | - / - | 9898 / 11872 | 0 / 0 |
| neighbours demoted | 7074 / 11011 | 40601 / 60131 | - / - | 110837 / 143496 | 13 / 12 |
| copies dropped: hop cap | 34 / 28 | 49 / 27 | 0 / 0 | 1604 / 1215 | 0 / 0 |
| copies dropped: TTL | 326 / 381 | 4772 / 6786 | 0 / 0 | 896 / 1161 | 0 / 1 |
| copies dropped: return seen | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 1 | 0 / 0 |
| copies dropped: no route | 89 / 92 | 168 / 117 | 12 / 12 | 15 / 15 | 0 / 0 |
| copies dropped: return failed | 0 / 0 | 0 / 4 | 2 / 0 | 37 / 31 | 0 / 0 |
| texts lost: hop cap | 32 / 25 | 29 / 19 | 0 / 0 | 168 / 135 | 0 / 0 |
| texts lost: TTL | 304 / 369 | 4582 / 6504 | 0 / 0 | 117 / 219 | 0 / 1 |
| texts lost: return seen | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 |
| texts lost: no route | 3 / 7 | 15 / 10 | 12 / 12 | 3 / 3 | 0 / 0 |
| texts lost: return failed | 0 / 0 | 0 / 3 | 2 / 0 | 2 / 1 | 0 / 0 |
| texts that waited at their origin | 154 / 214 | 186140 / 253373 | 2269 / 2506 | 190 / 140 | 0 / 0 |
| relays ever at their cap | 0 / 0 | 148 / 193 | - / - | 179 / 211 | 0 / 0 |
| largest settled airtime of a node % | 5.42 / 6.05 | 9.93 / 9.92 | 9.89 / 9.86 | 4.58 / 5.64 | 0.53 / 0.47 |
| heartbeats received % | 94.4 / 94.4 | 88.6 / 88.8 | 91.0 / 87.9 | 89.4 / 89.6 | 99.5 / 99.6 |
| DATA mean power dBm / at full % | - / 5.0 / 26.8 | - / 3.2 / 18.5 | - / -0.8 / 0.0 | - / 6.2 / 28.3 | - / 3.5 / 28.0 |
| ACK mean power dBm / at full % | - / 10.0 / 100.0 | - / 10.0 / 100.0 | - / 10.0 / 100.0 | - / 10.0 / 100.0 | - / 10.0 / 100.0 |
| a sleepy node's radio current mA (mean / median) | 0.535 / 0.464 / 0.528 / 0.456 | 0.618 / 0.546 / 0.594 / 0.529 | 2.364 / 2.378 / 2.253 / 2.251 | 0.450 / 0.384 / 0.448 / 0.377 | 0.255 / 0.254 / 0.255 / 0.254 |
| a relay's radio current mA | 6.297 / 6.352 | 7.747 / 7.943 | 8.354 / 8.361 | 5.926 / 5.938 | 5.891 / 5.887 |

The ACK's two forms and the headroom variant on the two seed-1 cities: main / A / B / A with 5 dB of headroom.

| | dp1 | dpx2-s1 |
|---|---|---|
| texts sent in the settled hour | 18234 / 18233 / 18233 / 18234 | 36559 / 36622 / 36509 / 36577 |
| delivered, settled hour % | 97.8 / 97.5 / 97.1 / 97.7 | 88.8 / 93.2 / 87.1 / 91.3 |
| delivered, whole run % | 96.5 / 96.2 / 96.1 / 96.4 | 90.5 / 93.4 / 88.6 / 92.2 |
| latency p50 s | 10.2 / 15.6 / 19.9 / 11.2 | 46.5 / 38.9 / 74.3 / 39.6 |
| latency p90 s | 47.4 / 72.5 / 92.4 / 52.3 | 349.6 / 274.8 / 386.9 / 307.9 |
| airtime, whole run s | 283128 / 308618 / 576834 / 283311 | 1118451 / 860928 / 1434543 / 1015937 |
| hand-overs taken | 417166 / 417464 / 685637 / 410813 | 1935441 / 1376187 / 2345350 / 1713040 |
| hand-overs failed | 480395 / 606493 / 1408356 / 492711 | 2777351 / 2265629 / 3836082 / 2565652 |
| failed, receiver had it | 108676 / 119622 / 381019 / 108024 | 559603 / 361949 / 839334 / 493156 |
| returns taken from a child | 3193 / 2800 / 14033 / 3148 | 10038 / 4908 / 20335 / 8580 |
| copies answered (not returns) | 246311 / 203834 / 653028 / 225617 | 1037572 / 565735 / 1225141 / 888018 |
| of those merged (case 4) | 2108 / 1773 / 11951 / 2022 | 36174 / 11731 / 43107 / 24379 |
| of those seen (case 6) | 244127 / 201970 / 640951 / 223518 | 1001196 / 553835 / 1181729 / 863442 |
| seen ACKs sent / acted on | 275948 / 233731 / 227288 / 193420 / 784672 / 597187 / 252844 / 213924 | 1161943 / 947962 / 620306 / 531239 / 1469708 / 1101418 / 990509 / 821020 |
| hand-overs held back by the bucket | 18179 / 25635 / 168785 / 17787 | 1902851 / 1057944 / 2476225 / 1523724 |
| neighbours demoted | 201105 / 258228 / 592298 / 206958 | 1185913 / 990329 / 1639823 / 1103693 |
| copies dropped: hop cap | 1502 / 1118 / 3122 / 1329 | 2692 / 1646 / 2459 / 2526 |
| copies dropped: TTL | 687 / 910 / 2860 / 841 | 38725 / 8809 / 48909 / 21711 |
| copies dropped: return seen | 1 / 0 / 2 / 0 | 1 / 0 / 3 / 3 |
| copies dropped: no route | 23 / 22 / 148 / 18 | 62 / 35 / 311 / 51 |
| copies dropped: return failed | 48 / 62 / 122 / 43 | 233 / 182 / 317 / 222 |
| texts lost: hop cap | 261 / 193 / 136 / 244 | 386 / 315 / 108 / 370 |
| texts lost: TTL | 78 / 103 / 160 / 84 | 1741 / 809 / 2756 / 1170 |
| texts lost: return seen | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 1 |
| texts lost: no route | 4 / 5 / 6 / 5 | 14 / 11 / 31 / 12 |
| texts lost: return failed | 3 / 8 / 9 / 4 | 48 / 41 / 52 / 49 |
| texts that waited at their origin | 80 / 84 / 366 / 110 | 8430 / 1452 / 6005 / 2721 |
| relays ever at their cap | 73 / 117 / 558 / 74 | 813 / 584 / 1286 / 710 |
| largest settled airtime of a node % | 4.87 / 5.00 / 7.24 / 4.68 | 9.99 / 9.83 / 9.95 / 9.98 |
| heartbeats received % | 85.5 / 86.0 / 81.5 / 85.8 | 67.8 / 75.4 / 71.0 / 70.7 |
| DATA mean power dBm / at full % | - / 6.1 / 24.2 / 5.9 / 24.1 / 8.5 / 77.3 | - / 5.9 / 23.2 / 5.9 / 23.5 / 8.4 / 73.9 |
| ACK mean power dBm / at full % | - / 10.0 / 100.0 / 6.4 / 48.6 / 10.0 / 100.0 | - / 10.0 / 100.0 / 6.5 / 44.8 / 10.0 / 100.0 |
| a sleepy node's radio current mA (mean / median) | 2.325 / 0.543 / 2.329 / 0.542 / 2.397 / 0.547 / 2.324 / 0.542 | 2.625 / 0.743 / 2.503 / 0.575 / 2.633 / 0.679 / 2.579 / 0.697 |
| a relay's radio current mA | 5.916 / 5.927 / 6.058 / 5.916 | 6.426 / 6.249 / 6.500 / 6.357 |

**Frame fates**, of frames addressed to nodes that are not sleepy, after the settle time: received; lost while the receiver listened and was free when the frame started; lost while it was receiving another frame; lost while it was deaf; sent while it listened on another channel. The last column is how the DATA received were answered.

**dp1.**

| | received | lost free | lost receiving | lost deaf | other channel | DATA answered: taken / refused / late / none |
|---|---|---|---|---|---|---|
| main DATA | 272786 (43.7 %) | 269750 (43.2 %) | 54916 (8.8 %) | 18508 (3.0 %) | 7899 (1.3 %) | 272770 / 0 / 0 / 0 |
| main ACK | 229009 (84.0 %) | 43141 (15.8 %) | 618 (0.2 %) | 0 (0.0 %) | 2 (0.0 %) |  |
| A DATA | 253441 (35.6 %) | 370057 (51.9 %) | 60694 (8.5 %) | 21695 (3.0 %) | 6584 (0.9 %) | 253415 / 15 / 0 / 0 |
| A ACK | 215190 (84.9 %) | 37695 (14.9 %) | 540 (0.2 %) | 0 (0.0 %) | 4 (0.0 %) |  |
| B DATA | 553063 (35.5 %) | 748685 (48.1 %) | 171895 (11.0 %) | 68658 (4.4 %) | 13634 (0.9 %) | 552967 / 72 / 0 / 0 |
| B ACK | 415986 (75.2 %) | 135142 (24.4 %) | 1907 (0.3 %) | 0 (0.0 %) | 4 (0.0 %) |  |
| A+5dB DATA | 265647 (42.5 %) | 279895 (44.7 %) | 53865 (8.6 %) | 18864 (3.0 %) | 7202 (1.2 %) | 265637 / 0 / 0 / 0 |
| A+5dB ACK | 222953 (83.9 %) | 42100 (15.8 %) | 581 (0.2 %) | 0 (0.0 %) | 2 (0.0 %) |  |
**dpx2-s1.**

| | received | lost free | lost receiving | lost deaf | other channel | DATA answered: taken / refused / late / none |
|---|---|---|---|---|---|---|
| main DATA | 1151562 (36.9 %) | 1209873 (38.8 %) | 516258 (16.6 %) | 214836 (6.9 %) | 26627 (0.9 %) | 1127952 / 23523 / 0 / 0 |
| main ACK | 886128 (77.0 %) | 257759 (22.4 %) | 7576 (0.7 %) | 0 (0.0 %) | 9 (0.0 %) |  |
| A DATA | 736256 (31.5 %) | 1121321 (47.9 %) | 329536 (14.1 %) | 136270 (5.8 %) | 16562 (0.7 %) | 729245 / 6960 / 1 / 0 |
| A ACK | 603845 (82.0 %) | 129152 (17.5 %) | 3201 (0.4 %) | 0 (0.0 %) | 6 (0.0 %) |  |
| B DATA | 1467317 (34.3 %) | 1835379 (42.9 %) | 635708 (14.9 %) | 302780 (7.1 %) | 33010 (0.8 %) | 1449707 / 17517 / 0 / 0 |
| B ACK | 1059176 (72.2 %) | 398992 (27.2 %) | 9039 (0.6 %) | 0 (0.0 %) | 13 (0.0 %) |  |
| A+5dB DATA | 1001294 (35.8 %) | 1145834 (41.0 %) | 442827 (15.8 %) | 182073 (6.5 %) | 25493 (0.9 %) | 987232 / 14004 / 0 / 0 |
| A+5dB ACK | 784878 (78.4 %) | 210645 (21.0 %) | 5706 (0.6 %) | 0 (0.0 %) | 5 (0.0 %) |  |
**s4x3-s1.**

| | received | lost free | lost receiving | lost deaf | other channel | DATA answered: taken / refused / late / none |
|---|---|---|---|---|---|---|
| main DATA | 502098 (56.1 %) | 177611 (19.9 %) | 127981 (14.3 %) | 82947 (9.3 %) | 3913 (0.4 %) | 436925 / 64793 / 0 / 0 |
| main ACK | 431897 (86.1 %) | 67511 (13.5 %) | 2304 (0.5 %) | 1 (0.0 %) | 4 (0.0 %) |  |
| A DATA | 525642 (49.3 %) | 289527 (27.1 %) | 147557 (13.8 %) | 101084 (9.5 %) | 2964 (0.3 %) | 445644 / 79780 / 1 / 0 |
| A ACK | 472098 (89.9 %) | 51423 (9.8 %) | 1898 (0.4 %) | 1 (0.0 %) | 1 (0.0 %) |  |

**The powers used** (form A), DATA frames after the settle time by their steps under full power, +10 dBm, step 19 being the amplifier's floor, −9 dBm:

- dp1: 0:24.2% 1:16.6% 2:12.3% 3:9.2% 4:7.1% 5:5.6% 6:4.4% 7:3.5% 8:2.8% 9:2.3% 10:1.8% 11:1.5% 12:1.2% 13:1.0% 14:0.9% 15:0.8% 16:0.6% 17:0.6% 18:0.5% 19:3.1%
- dpx2-s1: 0:23.2% 1:16.1% 2:11.9% 3:9.2% 4:7.2% 5:5.7% 6:4.6% 7:3.7% 8:3.0% 9:2.5% 10:2.1% 11:1.7% 12:1.4% 13:1.2% 14:1.0% 15:0.8% 16:0.7% 17:0.6% 18:0.5% 19:2.8%
- s4x3-s1: 0:18.5% 1:9.2% 2:7.8% 3:6.8% 4:5.9% 5:5.3% 6:4.8% 7:4.3% 8:3.9% 9:3.4% 10:3.1% 11:2.8% 12:2.6% 13:2.3% 14:2.1% 15:1.8% 16:1.7% 17:1.5% 18:1.4% 19:10.9%

## What it revealed

**Where the DATA frames go.** The powers used say where the hand-overs
go: in the dense city a quarter of the DATA frames need full power, half
are within 3 dB of it, and the count falls by a quarter with each
further decibel; only 3 % reach the amplifier's floor. The next hop is
the neighbour nearest the destination (spec "Next hop"), which is the
farthest neighbour in reach, so most forwards go to a neighbour near
the margin and have little to give. In S4, 11 % of the DATA go at the
floor: the deliveries to sleepy nodes beside their anchors, and their
own messages back.

**What a frame at the margin loses.** The margin is derived against
noise and one standard deviation of the fade (roadmap D38). It is not
derived against other frames. At full power a DATA that arrives 20 or
30 dB over the floor wins the receiver from almost any frame that
arrives after it; at the margin it loses to any frame that arrives
within the capture threshold of it, and to the fades beyond the margin
that would not have mattered. The fates show it: at the design rate the
DATA lost at a receiver that was free when they started rise from 43 to
52 % of those sent in the city (seeds 1, 2 and 7 alike), and from 20 to
27 % in S4 at three times the rate, while the collisions the receiver
saw coming, lost while receiving another frame, hardly move, 8.8 to
8.5 %. Each lost attempt costs its retry wait, three of them a ban and a
demotion: hand-overs that failed rise by a quarter in the city and by
half in S4, demotions by a quarter to a half, latency at the median by
half in the city and from 1.4 to 2.6 s in S4, airtime by 7 to 9 % at the
design rate and by 17 % in S4 at three times it, where the budget binds
and every retry is a frame some other message does not get: delivery
63.6 → 52.9 %.

**Where it pays.** In the city at twice the rate, main is in the storm
(`storm-2026-10-30.md`): 1.9 million hand-overs taken for 37 000 texts,
split copies answered a million times, 8400 texts waiting at their
origin. There every frame turned down is a frame fewer nodes hear, and
the storm feeds on being heard: delivery 88.8 → 93.2 %, airtime −23 %,
hand-overs taken −29 %, copies answered −45 %, texts dropped by their
time to live −77 %, heartbeats received 67.8 → 75.4 %, the ACKs lost at a
free receiver 22.4 → 17.5 %. The gain is of the storm, not of the
collisions as such: the DATA lost while the receiver was receiving fall
from 16.6 to 14.1 % of those sent, and those lost at a free receiver
rise from 38.8 to 47.9 %, as at the design rate; what falls is the
number of frames sent at all.

**The ACK.** Form B, the ACK at the DATA's power, loses the ACK where
form A does not: a sender listens for its ACK on its home channel among
every other frame that reaches it there, most at full power, and an ACK
turned down to the margin loses to them. Every ACK lost is a DATA retry
to a receiver that took the packet, and under D93 a second searcher
when the retry fails too.

**Headroom.** If what the frame at the margin loses is the contest with
later frames, keeping some power over the margin should give it back.
Measured with 5 dB, the simulator's capture threshold, on the two seed-1
cities (the column "A with 5 dB headroom"): at the design rate 97.7 %
against main's 97.8 and A's 97.5, latency at the median 11.2 s against
10.2 and 15.6, airtime as main's, 77 % of the DATA at full power; at
twice the rate 91.3 % against 88.8 and 93.2, airtime −9 % against −23 %.
The headroom buys the loss back and keeps about half the gain. The 5 dB
is a guess at the quantity, not a derivation: the frames a DATA must
beat arrive at every level, so the share it beats rises with each
decibel of headroom while the gain falls with it, and where the right
point lies is not in the datasheet. Not measured on S4 at three times
the rate, where the rule as built lost most, nor on the movers.

## Recommendation

Do not adopt the rule as built. Power down to the margin loses at the
design rate everywhere, by 0.3 to 1.6 points of delivery and half again
the latency, and by 10.7 points in S4 at three times the rate, for a
gain only in the city under overload. Keep the ACK at full power
whatever is done with the DATA: form B is worse than no power control.

The headroom form, A with a margin over the margin, is a possible
follow-up and nothing more: on the two cities it was within 0.1 point of
main at the design rate and 2.5 points over it at twice the rate. It is
worth measuring again only after two things: the bench has measured the
capture threshold at SF5, which the simulator takes as 5 dB from the SF7
figure, since the whole gain stands on frames beating each other by
about that much; and the base has D96 (roadmap), since the gain at twice
the rate was the storm's, and D96 has since changed what the storm does
on that base. That measurement then needs a headroom derived from the
radio or from the measured levels, not guessed, and the whole suite, S4
at three times the rate and the movers first, where the rule as built
lost most. The code, the frame's power field, the drivers that honour
it, the simulator's model of it and its count of the powers used, with
the unit tests, is removed from the branch as measured and rejected
code is, and kept on `worktree-agent-a3c80232a915e8727-code` at
`2a67e3b` for that measurement; the spec is unchanged.

The current columns of the tables say nothing: the model has one
transmit current at every power, and the datasheet gives the
amplifier's current at a few powers only, none under +14 dBm. What the
rule would save in current is for the bench to measure.

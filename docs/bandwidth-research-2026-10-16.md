# Wider channels, and the band's 1 mW row

**Date:** 2026-10-16. Scenario S4 (`scenarios/s4-capacity.toml`) and the
standard suite. Spec references are to `protocol.md`. Protocol code: main
`f1fa059` (D38 and D39 in, before D41), patched for each variant as
described below; nothing here changed main. Builds on
`capacity-research-2026-10-15.md` and is explained by
`collapse-research-2026-10-16.md`, which was written from the same runs.
Decision: roadmap D43.

## The questions

The duty cycle, not the channel, binds first in the city (the capacity
note). Two ways round it were asked about:

1. **Wider channels.** A LoRa frame's time on air scales with 1 / bandwidth,
   so 250 kHz halves and 500 kHz quarters what each frame costs a relay's
   10 % budget, at the price of 3 and 6 dB of sensitivity (noise bandwidth)
   and fewer channels in the same 1.74 MHz.
2. **The band's 1 mW row.** ERC/REC 70-03 Annex 1 band g2 allows 1 mW ERP
   in 433.05–434.79 MHz with no duty-cycle limit (band g1 is 10 mW at
   10 %). Ten dB less power, no cap at all.

The regulation was read first, because the answer depends on whether the
10 % is per channel or per device: it is per device over the whole band.
ERC/REC 70-03 (edition of October 2025, general part) defines the duty
cycle as the ratio of a single transmitter device's on time, summed, over
one hour, measured in the applicable frequency band of the Annex, and
band g1 lists no channel spacing. EN 300 220-1 V3.1.1 §5.4.1 and
EN 300 220-2 V3.3.1 §4.4.3.2 say the same: the observation bandwidth is
the operational, or permitted, frequency band. So thirteen channels do
not give thirteen budgets.

## What was run

Five builds of `bark-sim`, each from main `f1fa059` with constants
changed in `bark-proto/src/radio.rs` and `tables/busy.rs` (and the
compile-time pins of the table sizes derived from them dropped, since
those sizes legitimately change):

| Build | Bandwidth | Channels | Plan | ERP | Duty cap |
|---|---|---|---|---|---|
| A | 125 kHz | 13 | 433.170 MHz + k × 125 kHz | 10 mW | 10 % |
| B250 | 250 kHz | 6 | 433.175 MHz + k × 250 kHz | 10 mW | 10 % |
| C500 | 500 kHz | 3 | 433.300 MHz + k × 500 kHz | 10 mW | 10 % |
| D1mW | 125 kHz | 13 | as A | 1 mW | none |
| E1mW500 | 500 kHz | 3 | as C500 | 1 mW | none |

The simulator derives the symbol time, the noise floor and the receiver's
sensitivity from the bandwidth, so the 3 and 6 dB follow by themselves.
The nominal noise floor the core starts from was moved by the same 3 and
6 dB. The scenarios whose links are given as received power by hand (S1,
S1b, the corridor) cannot show a power change and are listed for
completeness only.

Scenarios: S4 at 1, 2, 3 and 4 times the §13 texting rate (seeds 1 and
2); the standard suite at 10 seeds. For the 1 mW row a second S4 was
built with 3.6 times the relay density (966 relays), since 10 dB less
power is 0.53 of the reach at the scenarios' path-loss exponent of 3.6,
and 1 / 0.53² = 3.6 times the relays keep the same neighbour graph.

## Results

**S4, delivered share of reachable texts, seeds 1 and 2 averaged:**

| Rate | A 125 kHz | B250 | C500 | D1mW, same relays | D1mW, 3.6 × relays |
|---|---|---|---|---|---|
| ×1 | 94.9 % | 94.9 % | 94.8 % | 26.1 % | 95.4 % |
| ×2 | 94.2 % | 94.9 % | 94.5 % | 14.5 % | 95.5 % |
| ×3 | 16.4 % | 14.1 % | 75.1 % (one seed held, one collapsed) | | |
| ×4 | 2.4 % | 1.8 % | 33.3 % | | 23.2 % |

**What wider channels improve** (A → B250 → C500):

- Airtime per delivered text at ×1: 0.72 → 0.34 → 0.16 s; at ×2:
  1.03 → 0.38 → 0.17 s.
- The busiest relay's share of its airtime at ×2: 8.4 → 4.1 → 1.7 %;
  relay time at the cap at ×2: 0.7 % → 0 → 0.
- Per-hop latency, from the frame time 139 → 70 → 35 ms: the line
  scenario's median 213 → 108 → 56 ms.
- The loaded ring at its cap: carried 2.75 → 3.98 → 4.00 texts/s,
  no-route drops 4.5 → 1.1 → 0.2 a run.
- Total airtime per run in every suite scenario, halved and quartered.
- A rotation of the channels, so neighbour discovery, 130 → 60 → 30 s.

**What stays the same:**

- Delivery at ×1 and ×2, within noise, and S1, S1b, the corridor and the
  meshes.
- The channel's busy share at the relays: 1.4 % at ×1 for all three,
  2.8, 2.3 and 2.0 % at ×2. A wider channel carries proportionally more
  texts, so each text occupies its receiver's channel for the same time.
- The collapse. All three go from 94 % to nothing at ×3 within the same
  ten minutes (500 kHz held one seed at ×3 and lost both at ×4). The
  collapse note shows why: what breaks is the hand-over on a busy
  channel, and the channel is as busy at any width.
- Hops per text at ×1: 3.8, 3.7, 3.6. At S4's relay density the shorter
  reach did not lengthen routes.

**What gets worse:**

- Sensitivity, 3 and 6 dB, about 0.82 and 0.68 of the reach at the
  scenarios' exponent. S4's relays are dense enough to hide it; the
  indoor leaf link through a 14 dB wall is where it would show, and the
  simulator's leaf links are not calibrated for that. Bench item.
- Leaves change relay more: 8 000 → 15 400 → 28 700 times a run at ×1.
  They hear every relay every 30 s instead of 130 s and switch more
  often. Each change costs an attach exchange. Measured before D41,
  which changed how leaves keep and choose relays.
- Three channels cannot avoid 433.92 MHz, the busiest spot in the band
  for keyfobs and sensors (§5), and leave the move rule two alternatives
  instead of twelve. Six still can.
- Neighbour expiry, three rotations, falls from 390 s to 180 and 90 s,
  so a neighbour in a fade leaves the table sooner. Not measured either
  way.
- Interference from the band's other users, which a wide channel collects
  more of, is not modelled.

**The 1 mW row.** At S4's relay density it disconnects the city: 26 %
delivered at ×1 with hundreds of no-route drops and 30 hops a text. With
3.6 times the relays it delivers 95.4 % at ×1 and 95.5 % at ×2, the same
as 10 mW, with 0.94 and 1.01 s of airtime per text, a little more than
10 mW's 0.72 and 1.03 s because each text takes a third more hops. At ×4
it collapses anyway, to 23 %, with no cap to hit: 50 000 texts still in
flight at the end, 17 hops a text, 291 000 demotions. The cap is also a
collision brake; without it the retry-and-detour loop of the collapse
note runs free.

## Conclusions

- The duty cycle is per device over the band, so more channels cannot
  buy budget. Only shorter frames can, and wider channels are the one
  way to those on this radio: the SX1262's LoRa bandwidths stop at
  500 kHz, and a signal across the whole band would cost 11 dB.
- Wider channels relieve the cap, which binds at the centre below twice
  the rate, and do not move the collapse, which the channel sets. Their
  cost is sensitivity on the leaf link, unmeasured, and a poorer channel
  choice.
- The 1 mW row needs 3.6 times the relays for the same coverage and
  buys nothing in airtime, and the cap it removes is also what keeps a
  busy network from feeding on its own retries.

Decided (D43): the channel plan stays at 13 × 125 kHz at 10 mW, and the
1 mW row is not used. Wider channels are revisited once the boards have
measured the loss curve and the indoor wall loss (Phase 2 bench), which
is what the sensitivity side of the trade turns on, and once the collapse
fix (D42) has set where the cliff lies.

## Not verified

- Two seeds of S4 per variant; one build each of the 1 mW variants.
- The simulator's DATA-frames-per-hand-over column was computed with the
  125 kHz frame time and is wrong for the other widths; it is not quoted.
- Runs predate D41. The relay-change figures in particular will differ.
- Scripts, patches and runs: scratchpad `bw/` of this session (not in
  the repository).

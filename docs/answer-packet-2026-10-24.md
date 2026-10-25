# One answer packet, the wait in the first byte (2026-10-24)

Roadmap D76. With six-byte node IDs (D75) the ACK is 17 bytes and the
refusing NACK 18: the ACK and a byte for the wait. This note measures
the change that folds the wait into the first byte, so that every
answer is the 17-byte ACK, and compares two ways to code the wait in
the six bits it gets.

*Corrected 2026-10-25.* This note first said that the 18th byte took
another block of five symbols, 1.28 ms, a NACK 16.96 ms against the
ACK's 15.68. That was a formula error in `time_on_air`, found on
2026-10-25 (`docs/airtime-correction-2026-10-25.md`): at SF5 the 17-byte
ACK and the 18-byte NACK both take 16.96 ms. The runs below were made
with the error, so the 1.28 ms each refusal saved in them is not real,
and the comparison of the answer packet with the NACK measured only
noise. The comparison of the two codes, made with one packet on both
sides, stands.

## The change

The first byte has two shapes (spec "Packet formats"):

| first byte | packet | the other bits |
|---|---|---|
| `1fff cccc` | heartbeat | three flags, the home channel 0 to 12 |
| `00aa aaaa` | DATA | `aux`, the sender's home channel; above 12 rejected |
| `01aa aaaa` | ACK | `aux`, 0 taken, else the wait asked for |

What follows from it:

- One reply airtime, 16.96 ms. The reply reserve holds ⌊237.0 / 16.96⌋
  = 13 replies and a heartbeat, 234.88 ms, as it held 13 NACKs
  (`tables/budget.rs`). In the runs below, with the formula error, it
  held 15 replies of 15.68 ms and a heartbeat, 248.32 ms, against 13
  NACKs and a heartbeat, 233.6 ms.
- A refusal never says 0. The old byte rounded waits under one ban
  down to 0, which the core read as one ban: 0.7 % of refusals at three
  times the design rate, 8 % at twice. Now `wait_units` gives at least 1,
  and the encoder clamps a refusal's wait to 1 to 63.
- The bench frames leave the type space: a heartbeat byte on channel
  15, which no node sends and every node rejects, with the bench kind in
  the flag bits (`wire::bench_first`).

## Two codes for the wait

- **Linear**: code c is c bans, 1 to 63, 98.8 s. A longer wait is told
  as 63, and the sender comes back early to be refused again, as at the
  old byte's 255.
- **Piecewise**: codes 1 to 39 are 1 to 39 bans, exact up to the refill
  of the whole bucket, 61 s, the longest anything waits for a bucket;
  codes 40 to 63 grow by a constant ratio, 8.1 %, to 255 bans, 400 s,
  the old byte's reach: 42, 46, 49, 53, 58, 62, 67, 73, 79, 85, 92, 100,
  108, 117, 126, 136, 147, 159, 172, 186, 202, 218, 236, 255 bans. A wait
  is told as the longest code not above it.

Both were built behind one table in `tables/budget.rs` and measured;
the piecewise code is removed.

## Method

Baseline: D75 rebased onto main `008715d` (fast boot, D73), with the
18-byte NACK and its byte wait, plus a simulator counter of the
refusals sent by the wait each told (`told_waits` in the summary).
Linear and piecewise: the same with the change. Release builds of
`bark-sim`; S4 is `scenarios/s4-capacity.toml` at
`per_leaf_per_hour` 0.9583333 (×1, the design rate), 1.9166667 (×2)
and 2.875 (×3); `dense-power.toml`; the ring, `ring.toml` with
`sleepy_listens = true`, as its test runs it, seeds 1 to 40.

- *Delivered*: of the texts sent in the settled hour.
- *Airtime per delivered text*: all airtime after the settle time over
  the texts the settled hour delivered.
- *Latency*: over the whole run.
- *Refusals at the origin*: refusing ACKs to a DATA with hop count 0,
  a sleepy node's own text; *on the way*: to higher counts.
- *Texts the origin made wait*: the application held a text because
  its node did not take it (`waited`).
- *Told at the cap*: refusals that told the code's longest wait; for
  the baseline, the 255 cap and, in brackets, waits over 63, which six
  bits would cut.

## The waits asked for

Baseline, S4 seed 1, refusals sent over the whole run, by the wait
told in bans:

| | ×1 | ×2 | ×3 |
|---|---|---|---|
| refusals | 52 | 2749 | 261 430 |
| p50 / p90 / p99 / p99.9 | 1 / 1 / 1 / 1 | 2 / 5 / 11 / 13 | 13 / 29 / 43 / 57 |
| longest | 1 | 14 | 66 |
| 0 (under a ban) | 0 | 221 (8.0 %) | 2021 (0.77 %) |
| 1 | 52 | 960 (34.9 %) | 12 697 (4.9 %) |
| 2–3 | | 934 (34.0 %) | 23 927 (9.2 %) |
| 4–7 | | 515 (18.7 %) | 44 357 (17.0 %) |
| 8–15 | | 119 (4.3 %) | 72 569 (27.8 %) |
| 16–31 | | | 87 068 (33.3 %) |
| 32–38 | | | 12 463 (4.8 %) |
| 39–47 | | | 5167 (2.0 %) |
| 48–63 | | | 1145 (0.44 %) |
| 64–127 | | | 16 (0.006 %) |

At ×3 seeds 2 and 3 the longest was 63 bans both times, and 0.73 % and
1.00 % asked for 48 to 63. So at the design rate and twice it every
wait is a few bans, and six bits cut nothing; at three times the rate,
past the relays' cap, one refusal in 16 000 asked for more than 63.

## Results

S4, seed 1:

| | ×1 base | ×1 linear | ×2 base | ×2 linear | ×3 base | ×3 linear | ×3 piecewise |
|---|---|---|---|---|---|---|---|
| delivered, settled hour | 94.07 % | 94.10 % | 93.21 % | 93.40 % | 65.52 % | 61.44 % | 65.43 % |
| texts delivered | 16 421 | 16 426 | 32 814 | 32 879 | 32 158 | 29 933 | 32 195 |
| airtime per delivered text | 2.10 s | 2.12 s | 1.96 s | 1.83 s | 3.70 s | 4.06 s | 3.70 s |
| settled airtime | 34 457 s | 34 842 s | 64 265 s | 60 229 s | 118 910 s | 121 468 s | 119 130 s |
| latency p50 / p90 | 1.6 / 187 s | 1.6 / 187 s | 3.4 / 239 s | 3.1 / 237 s | 116 / 606 s | 128 / 632 s | 115 / 604 s |
| refusals at the origin | 52 | 67 | 2046 | 1225 | 195 806 | 211 011 | 193 275 |
| refusals on the way | 0 | 0 | 703 | 407 | 65 624 | 70 723 | 64 343 |
| texts the origin made wait | 175 | 187 | 2568 | 1753 | 190 128 | 204 624 | 187 916 |
| told at the cap | 0 (0) | 0 | 0 (0) | 0 | 0 (16) | 233 (0.08 %) | 0 |

At ×1 and ×2 the two codes give the same run to the event, since no
wait reaches 40 bans; the piecewise columns are left out there.

S4 at ×3, seeds 1 to 9, delivered in the settled hour:

| seed | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | mean |
|---|---|---|---|---|---|---|---|---|---|---|
| base | 65.52 | 64.10 | 70.10 | | | | | | | |
| linear | 61.44 | 63.77 | 74.72 | 57.45 | 62.37 | 63.07 | 61.48 | 65.26 | 62.79 | 63.59 |
| piecewise | 65.43 | 64.14 | 69.61 | 61.03 | 64.13 | 63.02 | 58.97 | 61.77 | 67.01 | 63.90 |

Linear less piecewise, paired by seed, over the nine: delivered
−0.31 ± 1.14 points (mean ± standard error), airtime per text
+0.04 ± 0.10 s, latency p50 +1.8 ± 3.9 s, p90 +0.2 ± 7.1 s, refusals
+2200 ± 6500 a run. Linear told the cap in 0 to 270 refusals a run,
at most 0.10 %. Over seeds 1 to 3 the base averages 66.57 %, linear
66.64 %, piecewise 66.39 %.

The chosen code, linear, against the base, seeds 2 and 3:

| | ×1 s2 base | ×1 s2 linear | ×1 s3 base | ×1 s3 linear | ×3 s2 base | ×3 s2 linear | ×3 s3 base | ×3 s3 linear |
|---|---|---|---|---|---|---|---|---|
| delivered, settled hour | 94.12 % | 93.87 % | 93.93 % | 93.94 % | 64.10 % | 63.77 % | 70.10 % | 74.72 % |
| texts delivered | 16 420 | 16 374 | 16 487 | 16 489 | 31 159 | 31 020 | 35 121 | 37 760 |
| airtime per delivered text | 2.17 s | 2.19 s | 2.32 s | 2.32 s | 3.81 s | 3.82 s | 3.39 s | 3.13 s |
| latency p50 / p90 | 1.7 / 188 s | 1.7 / 187 s | 1.6 / 191 s | 1.6 / 192 s | 125 / 628 s | 131 / 630 s | 101 / 574 s | 98 / 568 s |
| refusals at the origin | 76 | 80 | 70 | 77 | 202 245 | 207 667 | 161 476 | 151 770 |
| refusals on the way | 0 | 1 | 0 | 0 | 58 118 | 61 062 | 51 568 | 46 138 |
| texts the origin made wait | 175 | 177 | 183 | 184 | 195 760 | 201 243 | 157 603 | 148 704 |
| told at the cap | 0 (0) | 0 | 0 (0) | 0 | 0 (0) | 90 | 0 (0) | 20 |

`dense-power.toml`, seed 1: delivered 97.92 → 97.96 %, settled airtime
153 409 → 160 097 s, latency p50 / p90 9.8 / 44 → 9.9 / 46 s, refusals
39 → 37, the longest wait told 3 bans; piecewise the same run as
linear, as no wait reaches 40 bans.

The ring, seeds 1 to 40, settled phase: base 0.973–0.985 (mean 0.980),
linear and piecewise the same runs, 0.972–0.986 (mean 0.980), texts
carried a run 3414 → 3430. The ring's test bound stays at 0.96 (D75):
the worst seed is still below the 0.98 it had before D75.

## What it shows

- The answer packet costs nothing measurable and gains nothing
  measurable in delivery. The 1.28 ms a refusal saved in these runs was
  the formula error, and with the radio's true airtime the two answers
  cost the same; the differences between base and change, a few tenths
  of a point and a few percent of airtime either way at ×1 and ×2, and
  57 to 75 % between seeds at ×3, are the run-to-run spread of a
  network whose every hand-over shifts when one frame does. The one
  packet stays because it is simpler: one length, one airtime, and one
  figure for the reply reserve.
- The two codes are the same wherever no wait reaches 40 bans, which is
  everywhere below the cliff, and the same within one standard error at
  ×3. Six linear bits cut a wait in at most one refusal in a thousand.
- So linear, for being one rule: whole bans, 1 to 63. The piecewise
  table, a knee and a ratio found at compile time, buys reach that
  nothing measured uses.

# Airtime correction (2026-10-25)

## The defect

`time_on_air` in `bark-proto/src/radio.rs` counted the payload of an
SF5 packet as `8·len + 16 − 4·SF + 8` bits, and its comment called the
`+ 8` the SF5/SF6 header correction. That misread the SX1261/2
datasheet, §6.1.4, which gives for SF5 and SF6:

```
N_symbol = N_preamble + 6.25 + 8
         + ceil(max(8·N_bytes + N_bit_CRC − 4·SF + N_symbol_header, 0) / (4·SF)) · (CR + 4)
```

with `N_bit_CRC` = 16 with the CRC on and `N_symbol_header` = 20 for
the explicit header (0 for implicit). The formula for SF7 and above has
a further `+ 8` inside the `max`, which SF5 and SF6 drop; the header's
20 bits stay in both. Semtech's reference driver for the SX126x
(LoRaMac-node, `RadioGetLoRaTimeOnAirNumerator` in `sx126x/radio.c`)
computes the same: for SF 5 and 6 the numerator is `8·PL + 16 − 4·SF +
20`, with no `+ 8`.

At SF5 the payload term is then `8·len + 16` bits, in blocks of 20
bits, 5 symbols each at coding rate 4/5. The old term was 12 bits
short, so a packet takes one block more, 5 symbols or 1.28 ms, wherever
the 12 bits cross a block boundary: for lengths of 1, 2 or 4 mod 5
bytes.

## Frames that moved

| frame | bytes | before | after |
|---|---|---|---|
| heartbeat | 11 | 13.12 ms | 14.40 ms |
| ACK | 17 | 15.68 ms | 16.96 ms |
| heartbeat, long preamble | 11 | 367.42 ms | 368.70 ms |
| the datasheet's sensitivity packet, 8-symbol preamble | 64 | 152.25 symbols | 157.25 symbols |
| DATA header alone | 28 | 22.08 ms | unchanged |
| DATA with a 100-byte text | 128 | 73.28 ms | unchanged |
| full frame | 255 | 138.56 ms | unchanged |
| full frame, long preamble | 255 | 492.86 ms | unchanged |

With the correct formula the 17-byte ACK and the 18-byte NACK it
replaced (roadmap D76) both take 8 blocks, 16.96 ms. The 1.28 ms that
the answer packet saved a refusal in the D76 runs was this error, so
that comparison measured only noise (`docs/answer-packet-2026-10-24.md`).
The layout stays: one packet with one airtime is simpler, and the reply
reserve is one figure.

## Constants that moved

| constant | before | after | from |
|---|---|---|---|
| `RETRY_BACKOFF`, the retry unit | 154.24 ms | 155.52 ms | full frame + ACK |
| retry windows | 308 / 617 ms | 311 / 622 ms | 2 and 4 units |
| retrying span | 2402.56 ms | 2410.24 ms | 2 attempts of 738.56 ms + the windows |
| `HEARTBEAT_RESERVE` | 13.12 ms | 14.40 ms | one heartbeat |
| `ACK_AIRTIME` | 15.68 ms | 16.96 ms | one ACK |
| `REPLIES` | 15 | 13 | ⌊9.83 % × 2.41 s / ACK⌋ = ⌊237.0 / 16.96⌋ |
| `REPLY_RESERVE` | 248.32 ms | 234.88 ms | 13 ACKs + a heartbeat |
| `BAN` | 1.5685 s | 1.5816 s | the retry unit / 9.83 % |
| the longest wait told, 63 bans | 98.8 s | 99.6 s | 63 × `BAN` |
| `DRAIN` | 459.58 s | 463.40 s | 293 places × `BAN` |
| `RECORDS` | 1648 | 1613 | ⌊(6 s + 9.83 % × `DRAIN`) / (ACK + DATA header)⌋ + 293 |
| `HeardParams::linger` | 3141.12 ms | 3148.80 ms | 3 attempts + the windows |
| `HeardParams::awake`, the window | 3379.68 ms | 3387.36 ms | an attempt under way + `linger` |
| `burst_gap` | 1205.98 ms | 1207.26 ms | backoff + the lag + a heartbeat |
| intervals argument | 10 s / 26.24 ms = 381 | 10 s / 28.80 ms = 347 | `INTERVALS` stays 256 |
| `FULL_FRAME_ABOVE_SENSITIVITY_DB` | 0.44 dB | 0.43 dB | 541 symbols against the reference's 157.25 |
| the simulator's offset from ideal detection | 1.34 dB | 1.36 dB | the reference's 157.25 symbols |

`SEND_QUEUE` stays 39 (⌈6 s / 155.52 ms⌉), the full frames a bucket
takes above the reply reserve stay 37, the heartbeat lag stays
992.86 ms, and the margin stays 4.1 dB (4.08 against 4.09). `Relay`
shrinks by the 35 records, 173 840 → 172 440 B. In the spec a text of
100 bytes over 8 hops takes 0.72 s of channel time (0.71), per hop
85.1 / 90.2 / 96.6 ms for 4 / 6 / 8-byte IDs (83.8 / 89.0 / 94.1), and
the heartbeat load per channel is 3.0 % (2.7 %).

## Measured

S4, `scenarios/s4-capacity.toml`, seed 1, release builds of `bark-sim`
on main `1654017` and on the correction; three times the design rate
is the same file with `per_leaf_per_hour` 2.875. Delivered is of the
texts sent in the settled hour; airtime per delivered text is all
airtime after the settle time over the texts the settled hour
delivered; relay time at the cap is the share of the relays' load
samples taken with the bucket at the cap; the busiest node is the
largest share of the settled hour one node spent transmitting.

| | ×1 before | ×1 after | ×3 before | ×3 after |
|---|---|---|---|---|
| delivered, settled hour | 94.10 % | 94.08 % | 61.44 % | 63.35 % |
| texts delivered | 16 426 | 16 425 | 29 933 | 30 963 |
| airtime per delivered text | 2.12 s | 2.13 s | 4.06 s | 3.82 s |
| settled airtime | 34 842 s | 34 993 s | 121 468 s | 118 400 s |
| heartbeat airtime, whole run | 2297 s | 2521 s | 2264 s | 2488 s |
| ACK airtime, whole run | 2557 s | 2801 s | 13 300 s | 14 088 s |
| relay time at the cap | 0 % | 0.001 % | 31.0 % | 29.5 % |
| relays ever at the cap | 0 | 1 | 175 | 167 |
| busiest node, settled hour | 6.09 % | 6.31 % | 9.95 % | 9.92 % |
| latency p50 / p90 | 1.6 / 187 s | 1.6 / 187 s | 128 / 632 s | 120 / 613 s |
| refusals | 68 | 52 | 297 383 | 283 298 |

The "before" columns are the D76 runs' linear columns to the text, so
the builds match what was measured then.

The release timing test (`capacity_scenario_runs_a_minute_quickly`)
takes 9.3 s on the correction, three runs, and 11.0 s on main, two
runs, at a load average of about 2; its bound is 20 s.

## What it shows

- Heartbeats and ACKs cost what the radio takes: 9.7 % and 9.5 % more
  airtime, as 14.40 / 13.12 and 16.96 / 15.68 say. At the design rate
  that is 0.4 % more airtime per text and no change in delivery.
- At three times the rate the corrected run delivered 1.9 points more
  with 6 % less airtime per text. That is inside the spread between
  seeds there, 57 to 75 % (D76), and the D76 runs showed one frame's
  change shifting a seed by several points either way; one seed does
  not show a gain, only that the correction costs nothing measurable.
- No derived constant moved enough to change behaviour beyond the
  airtimes themselves: the reply reserve lost 2 ACKs, 13 ms, and the
  ban grew 13 ms.

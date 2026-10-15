# Packet loss near sensitivity

**Date:** 2026-10-15. Research note behind the simulator's reception model
(`simulator-design.md` §3.3, `bark-sim/src/phy.rs`).

## What the sources give

- **Sensitivity is a 1% point.** The SX1261/2 datasheet (Rev 1.2, §3.5)
  states every LoRa sensitivity at "PER = 1%, packet 64 bytes, preamble 8
  symbols, CR = 4/5, CRC on payload enabled, explicit header mode". At
  sensitivity one such packet in a hundred is lost. Co-channel rejection
  (5 dB at SF7) is measured with a single tone 3 dB above sensitivity.
- **Required SNR per spreading factor.** Datasheet Table 6-1: −2.5 dB at
  SF5. The simulator's −119 dBm sensitivity is the noise floor plus this.
- **Any wrong symbol loses the packet at CR 4/5.** The 4/5 and 4/6 codes
  only detect errors; only 4/7 and 4/8 correct one bit (Afisiadis, Burg,
  Balatsoukas-Stimming, "Coded LoRa frame error rate analysis",
  arXiv:1911.10245, 2019, §III). With CR 4/5 a packet arrives only if
  every symbol does.
- **A symbol is one of 2^SF orthogonal signals, detected without phase**
  (same paper, §II). The symbol error rate in white noise is then the
  textbook one for noncoherent detection of M orthogonal signals (Proakis
  and Salehi, *Digital Communications*, 5th ed., §4.5), with M = 32 at SF5
  and symbol energy to noise density equal to 32 times the in-band SNR.

No source found gives a measured packet error rate against SNR for SF5.

## What we do

1. Each symbol is lost with the probability the formula gives at the
   symbol's SNR. A packet survives if all its symbols do, so longer
   packets fail more at the same SNR.
2. The curve is moved along the SNR axis until the datasheet's reference
   packet loses exactly 1% at sensitivity. The move is 1.34 dB: the
   datasheet's figures are that much better than ideal detection would
   allow. Either Table 6-1 is rounded or the noise figure derived from it
   is off; anchoring on the sensitivity keeps the stated number right.
3. Interference adds to the noise, weighted so that the capture threshold
   against interference alone has the same effect as sensitivity against
   noise alone: with the defaults, interference counts 7.5 dB (5 − (−2.5))
   more than noise of the same power.
4. Interference is followed in time: symbols during an overlap face it,
   symbols outside do not, so a packet that overlaps another by a few
   symbols often survives.
5. A preamble longer than the 12-symbol minimum counts as 12 symbols: the
   receiver needs only that many to detect it (datasheet §6.1.1.1).
6. A packet lost before its header ends frees the receiver at once. One
   lost later keeps it busy to the packet's end, the length the header
   gave. A stronger packet during the preamble still takes the receiver.

Loss for our packets against SNR relative to sensitivity, with no
interference (12-symbol preamble):

| dB from sensitivity | 8 bytes (heartbeat) | 40 bytes | 64 bytes | 255 bytes |
|---|---|---|---|---|
| −4 | 93% | 100% | 100% | 100% |
| −3 | 59% | 88% | 95% | 100% |
| −2 | 20% | 41% | 52% | 92% |
| −1 | 3.4% | 7.9% | 11% | 33% |
| 0 | 0.3% | 0.7% | 1.0% | 3.5% |
| +1 | 0.01% | 0.02% | 0.04% | 0.2% |

The fall from 90% to 1% loss takes about 3 dB. There is no width to
choose: it follows from the symbol count and the formula. Because the
datasheet places sensitivity at 1% loss, the curve mostly adds reception
below sensitivity: a heartbeat 2 dB under it arrives four times in five.

`reception = "threshold"` in a scenario's `[radio]` section restores the
earlier rule (at or above sensitivity, and above interference by the
capture threshold) inside the same receiver behaviour, for comparison.

## What it does not capture

- Frequency offset, clock drift and fading within a packet. Fading is held
  for a packet's duration (`simulator-design.md` §3.3); real receivers lose
  a little more than ideal detection, which here is folded into the 1.34 dB
  move.
- The header's stronger 4/8 code: its symbols count like the rest.
- The true statistics of LoRa interference. A same-spreading-factor
  interferer is not noise; treating it as weighted noise is our choice.
- Channel activity detection and the start-of-run reachability count still
  use the sensitivity as a line.

Bench measurements in Phase 2 should replace the position and check the
steepness: send a few thousand packets of two lengths at stepped
attenuation and count losses.

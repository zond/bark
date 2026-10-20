# Bench measurements, bring-up steps 1 to 3 and 5

Written 2026-10-14, step 5 2026-10-20, before any board has run this
firmware. Command
syntax and output fields are in `src/console.rs` and `README.md`.
A quoted name after "spec" is a heading of `docs/protocol.md`.

The table at the end has one row per field of `RadioParams` in
`bark-sim/src/phy.rs`. Fill in the "measured" column and copy the
values into a scenario's `[radio]` table (see the end).

## Setup

- Two boards, A and B, flashed with the same console UF2 (built with
  `cargo build --release --features console`), both on USB to one
  laptop (or B on a battery with its own laptop for range tests).
- Log each session to a file: `picocom --logfile A-<date>.log /dev/ttyACM0`.
  Lines starting with `>` are the commands typed; every result is one
  log line of `key=value` fields.
- On boot each board prints `radio up in N us` and `console ready`. If
  it prints `radio init failed: Busy`, the chip never released BUSY:
  power, reset or wiring (README, Unverified).
- Record on each board: `status` (battery, counters). `status reset`
  zeroes the counters before a run.
- Antennas: stock antennas, boards about 2 m apart, unless a step says
  otherwise. For C1 and sensitivity, SMA attenuators in a cable between
  the boards are better than distance if the antenna connector allows.

Repeat every timing measurement at least 10 times and record median and
range; the stopwatch resolves 1 µs.

## Step 1: transmit, receive, CAD, RSSI, retune (spec "Implementation notes" step 1)

### 1.1 Transmit and receive

```
A> ch 3
B> tx 3 9
B> tx 3 64
B> tx 3 255
```

A prints one `rx ch=3 ... crc=ok` line per frame. B prints
`tx ... tx_us=… airtime_us=… excess_us=… rest_us=…`.

- `airtime_us` is the datasheet formula (`bark_proto::radio::time_on_air`);
  `tx_us` is SetTx to TxDone handled. `excess_us = tx_us − airtime_us` is
  the switch to TX, PA ramp and SPI overhead: the start of a transmission.
- `rest_us` is the time from TX done back to listening.
- Check `excess_us` is the same for all three lengths; if it grows with
  length, the airtime formula is wrong.

Expected: `excess_us` of a few hundred µs at most. The radio idles in
STDBY_XOSC with the TCXO running, so SetTx waits for no TCXO start and
the chip reaches TX in 105 µs (DS Table 8-2). The simulator assumes
92 µs (RX→TX). An `excess_us` near 5 000 means the chip started the TCXO
again (`radio::TCXO_START`): it was in STDBY_RC, and the driver's standby
setting did not take.

### 1.2 Retune

```
A> ch 3      (repeat: same channel, no synthesiser hop)
A> ch 4      (one channel)
A> ch 12
A> ch 0      (twelve channels, 1.5 MHz)
```

`retune_us` with and without a hop. The radio retunes through
STDBY_XOSC, so no TCXO start is expected: the chip's part is about 62 µs
(STDBY_XOSC→RX, DS Table 8-2) plus the unlisted RX→STDBY_XOSC step, against
the simulator's 86 µs (spec "Busy estimate" assumes ≈0.1 ms); the SPI commands add to it.
About 5 000 means a TCXO start (see 1.1).

### 1.3 RSSI settle time and noise floor

On a quiet channel (check with `noise` first):

```
A> ch 0
A> noise 5 1000 1000      (1000 readings over about 1 s)
A> rssi 5 0
A> rssi 5 50
A> rssi 5 100
A> rssi 5 200
A> rssi 5 300
A> rssi 5 500
A> rssi 5 1000
```

Repeat each `rssi` 10 times. `rssi` retunes from home to 5, waits
`dwell_us`, reads once, retunes home. The settle time is the smallest
dwell whose readings fall in the `noise` min..max range of the same
channel; readings far below it (or 0) mean the receiver was not yet up.
`read_us` is the cost of one GetRssiInst.

Noise floor: `noise k 1000 1000` on each of the 13 channels, at a few
times of day. The median is the floor; p10 is the low percentile the
noise-floor calibration of spec "Busy estimate" uses. Channel 6 (433.92 MHz) is
expected to be the busiest.

### 1.4 Sweep

```
A> sweep
```

13 readings, `total_us`. Spec "Busy estimate" assumes about 5 ms for all 13.

### 1.5 CAD

```
A> cad 5     (×20 with nothing on air: count busy=true, the false alarms)
B> beacon 5 1390 255
A> cad 5     (×50 while B beacons)
B> stop
```

`prepare_us`, `cad_us`, `rest_us`. `cad_us` contains the CAD itself, 8
symbols at SF5 ≈ 2.2 ms, and the TCXO start after it (`radio::TCXO_START`,
5 ms): CAD ends in STDBY_RC, which turns the TCXO off (DS Table 13-73), and
the driver starts it again before it returns. B's frames are
on air 10 % of the time, so about 5 of the 50 CADs land on one; if far
fewer report busy, CAD misses frames or detects only the preamble.

### 1.6 Ping and the per-hop ACK budget (spec "Hand-over")

```
B> ch 5
A> ch 2
A> ping 5    (×20)
```

A prints `ping … rtt_us=… within=…`; B prints `pong … cad_us=…
tx_us=… rx_to_tx_done_us=…`. `rtt_us` runs from A's TxDone to the pong
received, which is what the 100 ms ACK timeout of spec "Hand-over" must cover: B's
reception handling, CAD on A's channel, its transmit start and the 13-byte
pong (14.4 ms on air). If `within=false`, the ACK timeout or the
receiver's path needs changing; note which part of B's line dominates.

### 1.7 Sensitivity

With attenuators (or distance), B sends `beacon 3 120 9` (9 bytes every
120 ms, 10 % duty) and A listens on 3. Raise the attenuation in steps
until A receives less than 90 % of the beacons. Per step: `status reset`
on A, wait 60 s (500 beacons), `status` on A: `rx_ok` against 500. The
RSSI A reports for the last frames received at the 90 % step is the
sensitivity. CRC failures (`rx_crc_error`) and header failures appear
before frames stop entirely.

### C1: adjacent-channel rejection (spec "Channel plan", O5)

The question is whether 125 kHz spacing (1.0 × bandwidth) separates
channels. The datasheet gives 60 dB only at 1.5 × bandwidth.

Two boards, two measurements:

1. **Rejection as RSSI sees it.** B beacons long frames at full power:
   `power 10`, `beacon 5 1390 255`. Set attenuation or distance so A
   reads about −50 dBm on channel 5 (strong, but far from overload).
   ```
   A> ch 0
   A> noise 5 1000 2000     (2 s, catches at least one frame: max is its RSSI)
   A> noise 6 1000 2000
   A> noise 7 1000 2000
   A> noise 4 1000 2000
   ```
   Rejection at ±1 channel = max on 5 − max on 6 (and on 4); at ±2 =
   max on 5 − max on 7. If the max on 6 equals that channel's noise
   floor, the result is only a lower bound: reduce the attenuation and
   repeat.
2. **Decoding across channels.** Keep B beaconing on 5. On A: `ch 6`,
   `status reset`, wait 60 s, `status`. Any `rx_ok` means a frame on 5
   decodes on 6 at this level; note the RSSI A reported. Repeat with
   less attenuation until frames decode or the boards are side by side.

With a third board C (we have four), measure rejection the way it
matters: B sends the wanted signal on 5 at about sensitivity + 3 dB at A
(`beacon 5 120 9`); C interferes on 6 with long frames
(`beacon 6 1390 255`), and its level at A is read with
`noise 6 1000 2000` on A. Each C frame overlaps about one B frame. Raise
C's level until A loses the B frames that overlap C's (lost sequence
numbers about every 1.39 s). Rejection = C's level − B's level at that
point. This is what the simulator's `acr1_db` models; measurement 1 is
the fallback when only two boards are at hand.

If rejection at ±1 channel is well below the 60 dB the simulator
assumes, run the simulator with the measured value; if the network
suffers, the fallback is 11 channels at 150 kHz (spec "Channel plan").

## Step 2: listen-before-talk, heartbeat, neighbour table (spec "Implementation notes" step 2)

The heartbeat and neighbour table need the protocol core, which the
default build runs and the console does not (README). What the console
measures for step 2:

- The cost of listen-before-talk: `cad` (1.5) plus `rssi` on the target
  channel (1.3).
- That all three pairs of boards hear each other: `ping` each way
  between each pair, on different home channels.

## Step 3: busy estimate and home channel (spec "Implementation notes" step 3)

Also needs the core. What the console measures for step 3:

- Noise floor per channel and its spread (1.3), which sets the busy
  threshold (floor + 6 dB, spec "Busy estimate"; O3, O11).
- The cost of the sweep (1.4).
- An interferer: a fourth board running `beacon k 1390 255` occupies
  channel k 10 % of the time. A continuous-wave interferer (spec "Implementation notes" step 3)
  is not in this console.

## Step 5: a sleepy node, duty-cycled receive and current (spec "Implementation notes" step 5)

Board S runs the sleepy image (`cargo build --release --features sleepy`),
board A the console. With no node of the protocol near it, S hears no
relay and stays without an anchor (spec "Sleepy nodes alone"): after its
boot listen it receives duty-cycled at all times but its own beats. S's
log gives what the step needs:

- `sleepy node id=[..] at ...` at start, and `HomeChannelMoved { from, to }`
  when the boot listen ends: `to` is the channel S receives on, h below.
- The `driver` line, at the first event 10 s or more after the last:
  `frames` counts every frame with a good CRC, the console's bench frames
  included (the core discards them), and `sniffs` the starts of
  duty-cycled receive, one at each `Sniff` and one after each packet.
- A rising `sniffs` with no frames on air shows the image receives
  duty-cycled at all.

S logs only when it booted on USB power (README, The sleepy image's
current); 5.1 and 5.2 run with S on USB, 5.3 on its battery.

### 5.1 Every long preamble is caught

On A, a bench frame of a DATA's length (30 bytes: the 22-byte DATA header
and a short text) with the long preamble, 100 times, 5 s apart: 377.7 ms
on air, 7.6 % of the period, under the console's 10 % cap. 5 s is not a
multiple of S's 361.125 ms cycle, so the frames start at offsets spread
over it (5000 mod 361.125 = 305.375 ms a step).

```
A> preamble long
A> ch h
A> beacon h 5000 30     (500 s; then:)
A> stop
```

Note S's `frames` before and after: it should rise by 100. A frame that
overlaps one of S's own beats (S sends one every 34.6 s on average, a
366 ms frame A also prints as `rx`) cannot be caught, about 2 in 100;
count those apart. What the window catches depends on the preamble alone,
not on what follows it, so the bench frame stands for a DATA (the
roadmap's bench row).

The control, which shows the radio really sleeps: the same with
`preamble short`. A 12-symbol frame is caught only when it starts in or
just before a window, 1.9 % of the time (`bark_proto::sniff`, the test
`a_short_preamble_is_caught_only_in_a_window`): about 2 of 100. Nearly all
caught means S listens continuously and `Sniff` did not take.

### 5.2 How much preamble the cycle needs

The long preamble is `sleep + wake + 2D` rounded up to whole 256 µs
symbols, with `wake` = 6 ms (a TCXO start assumed at 5 ms and about 1 ms
to restore the chip's state) and `D` = 4 symbols, the time to lock
(spec "Duty-cycled receive and the long preamble"). Repeat 5.1 with
`preamble 1300`, `preamble 1350`, `preamble 1380` and so on: the shortest
preamble `L` that is caught 100 times in 100 gives

```
wake + 2D = L × 256 µs − 349 125 µs
```

which checks the two assumptions together; with `D` = 4 it is 1396
symbols, with the wake 1 ms shorter 1392. 5.3 measures the wake alone.

### 5.3 Current

S on its battery, USB unplugged (VBUS powers the rail and the charger), a
meter in series with the battery that resolves the 6 ms windows: a power
profiler sampling at 10 kHz or more, or a scope across a 10 Ω shunt.
Record each state for at least a minute:

| State | When | Expected, radio alone (README) |
|---|---|---|
| Boot listen | from power-on to the end of the boot listen | listening, 6.1 mA (4.6 mA receive, DS Table 3-5, and the TCXO's nominal 1.5 mA, DS §4.1.4) |
| Duty-cycled receive | after the boot listen, between beats | a pulse every 361.125 ms, 0.14 to 0.20 mA on average; between pulses 1.2 µA (DS Table 3-5) |
| A beat | every 34.6 s on average | 366 ms at the transmit current, under 32 mA at +10 dBm (DS Table 3-6) |
| Catching a frame | during 5.1, on USB: shape only | the pulse lasts the frame's airtime, then the cycle starts again |

From the trace of duty-cycled receive:

- The pulse period: 361.125 ms if the chip takes the periods as given.
- The pulse width and its two parts: the TCXO start and state restore
  (`wake`, assumed 6 ms) at a lower level, then the 6 ms window at the
  receive current. This replaces `SNIFF_WAKE` and `RX_CURRENT_UA` in
  `bark_proto::sniff`, and with them the cycle and the long preamble.
- The level between pulses: everything on the board but the radio. The
  GPS is powered and dominates it (README); the rest is the MCU asleep,
  the battery divider's 12 µA and the parts not driven. A level well
  above their sum points at a clock kept running, the 32 MHz crystal or
  a TIMER (README); a 1 Hz square wave, the LED.
- The average over whole cycles, between beats, against the README's
  0.14 to 0.20 mA plus that level.

## Not measured here

- The preamble length the modem needs to lock (`D` in spec "Busy estimate",
  assumed 4 symbols) on its own: 5.2 gives it only together
  with the wake, which 5.3 measures; a direct measurement needs a
  receiver that leaves the channel for a controlled gap at random times
  and counts the frames lost.
- Same-channel capture (`capture_db`): needs two transmitters on one
  channel at controlled levels and a third board receiving; the C1
  three-board setup with C on channel 5 instead of 6 does it.

## Table: simulator radio parameters

Defaults from `RadioParams::default()`; DS is the SX1261/2 datasheet.

| `RadioParams` field | default | measured by | how | measured |
|---|---|---|---|---|
| `noise_figure_db` | 6.5 (DS) | 1.3 `noise` | median floor − (−123.0 dBm, kTB at 125 kHz) | |
| `sensitivity_dbm` | none: −119.0 derived | 1.7 | RSSI at 90 % reception | |
| `capture_db` | 5 (DS, CW, SF7) | not here | three boards, see above | |
| `acr1_db` | 60 (DS, at 1.5 × BW) | C1 | three-board result, else measurement 1 at ±1 | |
| `acr2_db` | 60 | C1 | measurement 1 at ±2 | |
| `cad_symbols` | 8 | fixed | the driver hard-codes 8 | 8 |
| `cad_miss_probability` | 0 | 1.5 | 1 − busy count / expected count (rough) | |
| `rssi_dwell_us` | 300 | 1.3 | settle dwell + `read_us` | |
| `rx_to_fs_us` | 15 (DS) | 1.2 | with `fs_to_rx_us`: `ch` to the same channel, split 15 : 41 | |
| `fs_to_rx_us` | 41 (DS) | 1.2 | see above | |
| `synth_hop_us` | 30 (DS) | 1.2 | `ch` to another channel − `ch` to the same | |
| `rx_to_tx_us` | 92 (DS) | 1.1 | `excess_us` | |
| `stby_to_rx_us` | 83 (DS) | 1.1, 1.5 | `rest_us` after `tx` and `cad` | |
| `interference_margin_db` | 10 | not measured | a modelling cut-off | 10 |

The CAD phase in the simulator is its sampling time, computed from
`cad_symbols` (2.2 ms at 8 symbols). `cad_us` in 1.5 exceeds it by the
TCXO start after the CAD, when the radio hears nothing; the simulator has
no field for that yet, so note the excess.

Into a scenario file:

```toml
[radio]
noise_figure_db = 0.0
sensitivity_dbm = 0.0
acr1_db = 0.0
acr2_db = 0.0
rssi_dwell_us = 0
rx_to_fs_us = 0
fs_to_rx_us = 0
synth_hop_us = 0
rx_to_tx_us = 0
stby_to_rx_us = 0
```

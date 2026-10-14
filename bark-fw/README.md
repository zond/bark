# bark-fw

Firmware for the LILYGO T-Echo (nRF52840 + SX1262, 433 MHz build). Its own
Cargo workspace, cross-compiled for `thumbv7em-none-eabihf` (roadmap D7).

Current state (2026-10-14): bring-up firmware. It blinks the green LED at
1 Hz, logs over USB CDC-ACM (a banner with the firmware version and
constants from `bark-proto`, the previous boot's panic message if there
was one, an uptime line every 10 s), powers up the SX1262 at bark's
modulation (SF5, 125 kHz, CR 4/5, 12-symbol preamble, CRC on, private sync
word) and runs a measurement console on the same serial port. It does not
run the protocol core yet, and does not touch GPS, e-paper or BLE (the GPS
does get power, see Radio power below).

## Verified and unverified

**Nothing here has run on hardware.** The boards arrive around 2026-11-04.
The radio code and the console are written against the datasheet and the
driver's source, and have only been compiled.

Verified on the host:

- `cargo build --release` and `cargo clippy --release -- -D warnings` pass.
- The ELF's first loadable byte (the vector table) is at `0x26000`, the
  initial stack pointer is `0x20040000`, and the image is about 89 KiB.
- `scripts/uf2.py` produces a UF2 that its own `--check` accepts: correct
  magic numbers, family `0xADA52840`, contiguous 256-byte payloads starting
  at `0x26000`.

Unverified, in the order they will fail if wrong:

- The bootloader accepts the UF2 and jumps to `0x26000`.
- Interrupts reach the application. The firmware sets VTOR itself
  (cortex-m-rt `set-vtor`) rather than relying on MBR/SoftDevice forwarding.
- The 32 MHz crystal starts (USB needs it); the low-frequency clock uses the
  internal RC oscillator so no 32.768 kHz crystal is assumed.
- The green LED is on P0.15 and active low.
- USB enumerates as a CDC-ACM serial port and log lines arrive.
- A panic message survives the soft reset through the bootloader (PANDUMP in
  `memory.x`) and is logged on the next boot.
- P0.13 (`PWR_EN`) and P0.12 (`PWR_ON`) high power the radio, both on USB
  and on battery, and the 10 ms wait after them covers the rail's ramp.
- The radio wiring of `src/board.rs`: SPI on P0.19/P0.22/P0.23, NSS P0.24,
  NRESET P0.25, BUSY P0.17, DIO1 P0.20; DIO2 drives the RF switch; DIO3
  supplies a TCXO at 1.8 V. A wrong TCXO voltage or a board without a TCXO
  shows as `Busy` or `DIO1` errors on the first transmit or receive.
- 8 MHz SPI works on the board's traces.
- The driver's behaviour beyond what its source shows: CRC-failed frames
  can be read back after the error (`get_rx_result` in continuous receive),
  RSSI reads are valid immediately after a retune, and continuous receive
  keeps running after a header or CRC error.
- Transmit power: the chip's 10 dBm setting gives about 10 dBm at the
  antenna port, and the stock antenna has no gain over a dipole, so the
  console's 10 dBm cap meets the 10 mW ERP limit.
- Battery voltage reads correctly (P0.04, 2:1 divider; unreliable on USB).
- Every console command. The timings it prints are its purpose; none has
  been seen.

## Toolchain

Stable Rust and the Cortex-M4F target:

```
rustup target add thumbv7em-none-eabihf
```

Python 3.9 or later for the UF2 converter (standard library only). No probe, objcopy or
cargo-binutils is needed.

## Build and package

From `bark-fw/`, one line:

```
cargo build --release && scripts/uf2.py target/thumbv7em-none-eabihf/release/bark-fw target/bark-fw.uf2
```

`scripts/uf2.py --check target/bark-fw.uf2` validates a UF2 and prints its
block count and address range.

## Flash

The T-Echo ships with a UF2 bootloader and keeps it (roadmap D6). The
application lives at `0x26000..0xF4000`; the bootloader, its settings and the
MBR parameter page sit above, and the MBR and the (unused) SoftDevice S140
below. `memory.x` has the full map.

1. Connect the T-Echo over USB-C. A USB-C-to-USB-C cable may not power it;
   use a USB-A-to-C cable if nothing happens.
2. Double-tap the top-left button, within about half a second (the
   bootloader waits 500 ms after a pin reset for the second one). The
   bootloader stays resident and a USB drive named `TECHOBOOT` appears. `INFO_UF2.TXT` on it reports the
   bootloader and SoftDevice versions; expect bootloader 0.6.1, S140 6.1.1.
3. Copy the UF2 to it:
   ```
   cp target/bark-fw.uf2 /media/$USER/TECHOBOOT/   # or /run/media/$USER/TECHOBOOT
   ```
   The drive disconnects when the copy completes and the board restarts into
   the application. (Some file managers report an error on that disconnect;
   it is harmless.)
4. Expected: the green LED blinks, 500 ms on and 500 ms off. A serial port
   appears on the host, and once it is opened the log shows
   `radio up in N us` and `console ready`.

If the LED does not blink, double-tap reset again: the bootloader is intact
and will accept another image.

## Radio power

At boot the firmware holds NRESET low, drives P0.13 (`PWR_EN`) and P0.12
(`PWR_ON`) high, and waits 10 ms for the switched rail. No source gives the
rail's ramp time; 10 ms is an assumption with a wide margin. Only then does
it drive NSS and the SPI lines, because a pin driven high into an unpowered
chip feeds it through its protection diodes. The driver's init then pulses
NRESET (200 µs, twice the datasheet's typical 100 µs, DS §8.1) and waits
for BUSY low, which the chip raises from reset until it is ready (DS §9.1;
3.5 ms cold start, DS Table 8-2), before the first SPI command.

`PWR_ON` is needed on battery: the rail is up when
`PWR_EN AND (PWR_ON OR VBUS)`. The rail also feeds the GPS, which is
therefore powered (and drawing current) though this firmware does not talk
to it.

## Console

The serial port carries the log and takes commands. Type a command and
Enter; the firmware echoes it as a line starting with `>`, and every result
is a log line of `key=value` fields, so a saved transcript separates
commands from everything else by the first character. A leading `>` on
input is accepted, so transcript lines can be pasted back.

`k` is a channel, 0 to 12 (`bark_proto::radio::Channel`, 433.170 MHz +
k × 125 kHz). Durations are µs from TIMER1 at 1 MHz on the 32 MHz crystal;
the embassy clock ticks at 30.5 µs from an RC oscillator, too coarse for
retune times.

| Command | Does | Prints |
|---|---|---|
| `ch <k>` | set the home channel, where the radio listens between commands | `retune_us` |
| `tx <k> <hex\|len>` | send one frame on k: hex bytes (`0a0b…`, or `0x…` to force hex), or a bench frame of len bytes (1 to 3 decimal digits) | `prepare_us`, `tx_us` (SetTx to TxDone), `airtime_us` (datasheet formula), `excess_us`, `rest_us` (back to listening) |
| `rx <k> [ms]` | listen on k until a frame or ms (default 10 000) | per frame: `len`, `rssi_dbm`, `snr_db`, `crc=ok\|error`, `bytes` |
| `cad <k>` | one CAD (8 symbols, the driver's fixed setting) | `busy`, `prepare_us`, `cad_us`, `rest_us` |
| `rssi <k> [dwell_us]` | retune to k, wait, read RSSI once, retune home | `dbm`, `retune_us`, `dwell_us`, `read_us` |
| `sweep` | one RSSI reading on each of the 13 channels | 13 values, `total_us` |
| `noise <k> <n> [gap_us]` | n readings (up to 1000) on k | `min`, `p10`, `median`, `max`, `total_us` |
| `power <dbm>` | transmit power, clamped to −9..10 dBm | the power in effect |
| `beacon <k> <ms> [len]` | send a bench frame (default 9 bytes) on k every ms; refused below a 10 % duty cycle | one line per beacon |
| `stop` | stop the beacon | |
| `ping <k>` | send a 9-byte ping on k, wait 1 s at home for the pong | `rtt_us` from TxDone to the pong, against the 100 ms ACK timeout (protocol §9.3) |
| `status [reset]` | uptime, battery mV, channel, power, counters; `reset` zeroes the counters | |

Between commands the radio listens on the home channel and prints every
frame it hears, CRC failures included. A board that receives a ping on its
home channel answers it like an ACK (protocol §9.3): CAD on the sender's
home channel (named in the ping), then a 9-byte pong, then back home; it
prints the time of each part. It does not do the RSSI half of
listen-before-talk.

Bench frames are packet type 0xF (reserved in protocol §14): byte 0 is
`0xF0 | kind` (0 test or beacon, 1 ping, 2 pong), bytes 1 to 4 a
big-endian sequence number, byte 5 the sender's home channel, then
`byte[i] = i`. RSSI is whole dBm (the driver drops the chip's half-dB
step); packet RSSI and SNR are in quarter dB.

`scripts/measure.md` is the measurement plan for bring-up steps 1 to 3 and
bench item C1, with a table that maps the results onto the simulator's
radio parameters.

## Radio driver

`lora-phy` from git (roadmap D4, D20), pinned to revision `7a9c2ec` of `main`
(2026-10-06). That revision reports payload and header CRC failures as
errors instead of returning the bytes as a good frame, and has `get_rssi`,
`rx_switch_channel` and quarter-dB packet RSSI. The firmware gives the
driver its own board interface (`radio::Pins`) instead of the driver's
generic SX126x one: the generic one polls BUSY immediately after each
command, but BUSY may take up to 600 ns to rise (DS §8.3.1), so the poll
can see the previous low and send the next command too early. Ours waits
out the rise first, bounds every BUSY wait (27 ms, twice a cold start plus
a TCXO start) and can bound the wait for DIO1, so a radio that does not
answer gives an error instead of a hung console.

## Risks

- **Every retune, CAD and transmit costs a TCXO start, about 10 ms.** The
  driver returns to STDBY_RC before each operation and programs a 10 ms
  TCXO start allowance; the chip waits it out on every start of RX, TX or
  CAD from STDBY_RC (DS §13.3.6, note). Protocol §6.1 assumes a retune of
  about 0.1 ms, and the simulator 86 µs. If `ch` measures about 10 ms, the
  sweep and listen-before-talk timings of the protocol need a driver that
  retunes in RX or idles in STDBY_XOSC; the console measures it either way.
- The CAD parameters are fixed in the driver (8 symbols, detection peak
  SF + 13, minimum 10). Tuning them means patching it.
- The sync word is 0x1424, the conventional private LoRa word, which other
  private networks also use. Their frames would reach the console, and
  bark's decoder rejects them by content. A bark-specific word is an open
  choice.
- The driver is a moving git dependency; the pin must be reviewed before it
  moves.

## Log

The firmware enumerates as USB VID:PID `1209:0001`, a pid.codes test ID that
must not be used on distributed devices. On Linux it appears as
`/dev/ttyACM0` (or the next free number; `dmesg | tail` says which).

CDC-ACM is a USB bulk pipe: the baud rate, parity and stop bits a terminal
sets are ignored, so any value works. What matters is that the terminal
asserts DTR, because the firmware holds log output until the port is opened.
Lines logged before that wait in a 2 KiB buffer (the banner is always
there); once it fills, newer lines are dropped until the port opens.

```
picocom /dev/ttyACM0          # exit with C-a C-x
# or
screen /dev/ttyACM0           # exit with C-a k
# or, read-only
cat /dev/ttyACM0
```

Linux raises DTR when a tty is opened, so all three work. Your user needs to
be in the `dialout` group (or `uucp`, depending on the distribution).

Each line is `[seconds.millis LEVEL] message`, seconds since boot; command
echoes are `> command`.

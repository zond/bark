# bark-fw

Firmware for the LILYGO T-Echo (nRF52840 + SX1262, 433 MHz build). Its own
Cargo workspace, cross-compiled for `thumbv7em-none-eabihf` (roadmap D7).

Current state (2026-10-14): a bring-up skeleton. It blinks the green LED at
1 Hz and logs over USB CDC-ACM: a banner with the firmware version and
constants from `bark-proto` (which proves the protocol core builds for the
target), the previous boot's panic message if there was one, and an uptime
line every 10 s. It does not touch the LoRa radio, GPS, e-paper or BLE.

## Verified and unverified

**Nothing here has run on hardware.** The boards arrive around 2026-11-04.

Verified on the host:

- `cargo build --release` and `cargo clippy --release -- -D warnings` pass.
- The ELF's first loadable byte (the vector table) is at `0x26000`, the
  initial stack pointer is `0x20040000`, and the image is about 30 KiB.
- `scripts/uf2.py` produces a UF2 that its own `--check` accepts: correct
  magic numbers, family `0xADA52840`, contiguous 256-byte payloads starting
  at `0x26000`.

Unverified, in the order they will fail if wrong:

- The bootloader accepts the UF2 and jumps to `0x26000`.
- Interrupts reach the application. The firmware sets VTOR itself
  (cortex-m-rt `set-vtor`) rather than relying on MBR/SoftDevice forwarding.
- The 32 MHz crystal starts (USB needs it); the low-frequency clock uses the
  internal RC oscillator so no 32.768 kHz crystal is assumed.
- The green LED is on P0.15 and active low, and is lit without driving the
  power-enable lines (P0.13, P0.12), which this firmware leaves alone.
- USB enumerates as a CDC-ACM serial port and log lines arrive.
- A panic message survives the soft reset through the bootloader (PANDUMP in
  `memory.x`) and is logged on the next boot.

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
   appears on the host.

If the LED does not blink, double-tap reset again: the bootloader is intact
and will accept another image.

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

Each line is `[seconds.millis LEVEL] message`, seconds since boot.

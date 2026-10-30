# bark-fw

Firmware for the LILYGO T-Echo (nRF52840 + SX1262, 433 MHz build). Its own
Cargo workspace, cross-compiled for `thumbv7em-none-eabihf` (roadmap D7).

Current state (2026-10-25). It starts behind a loader of its own, which
can swap in a staged image and swap it back (Updates, below). The firmware blinks the green LED at 1 Hz
while on USB, logs over USB CDC-ACM (a banner with the firmware version
and constants from `bark-proto`, the previous boot's panic message if
there was one, an uptime line every 10 s while on USB), shows the
board's node ID on the e-paper (Display, below), powers up the
SX1262 at bark's modulation (SF5, 125 kHz, CR 4/5, CRC on, private sync
word, the receiver set for the 1396-symbol long preamble) and then does
one of two things, chosen when it is built:

| Build | Runs |
|---|---|
| `cargo build --release` | the protocol core: a relay on USB, sleepy on battery |
| `cargo build --release --features console` | the measurement console (below) |
| `BARK_SURVEY=<k> cargo build --release --features console` | the console as a survey on channel k, for a board left away from the host (Survey build, below) |

Every board runs the same node. Its role is not a setting: the firmware
reads whether USB power is present from the nRF52840's VBUS detection
(Product Specification, POWER, USB supply: `USBREGSTATUS.VBUSDETECT`,
and the `USBDETECTED` and `USBREMOVED` events) and reports it to the core
at boot and at every change (`AppCommand::Power`, `src/power.rs`). On
battery it reads the cell once a minute between radio operations
(`AppCommand::Battery`, `src/battery.rs`). The core is a relay on USB; on
battery a carried one turns sleepy after its grace and a fixed one when
its battery is low (spec "Sleepy nodes"). For bench work,
`BARK_ON_BATTERY=1` at build time reports battery whatever USB says and
leaves the battery levels off, so that a board on the bench's USB runs
sleepy and still logs (`scripts/measure.md` step 5).

The core runs in the loop of `bark-driver` (`src/node.rs`), the same loop
its host tests run against a fake radio; `docs/firmware-adapter-2026-10-15.md`
describes it. A node's position is set when it is built, from `BARK_LAT`
and `BARK_LON` in degrees, until the phone app or the GPS can set it:

```
BARK_LAT=50.4501 BARK_LON=30.5234 cargo build --release
```

Without them the position is 0, 0, and boards built alike share one
identity. Every protocol event (`Trace`), every message for the node, and
a sleepy node's own message given back by its first hop with the time
left to wait (`given back ... retry_in_ms=`) is a log line, and at
the first event 10 s or more after the last a `driver` line gives the
loop's counters (`sniffs` counts the starts of duty-cycled receive) and
its longest times. There is no application yet: the node sends no
messages of its own. It does not touch GPS or BLE (the GPS does
get power, see Radio power below).

## Verified and unverified

**Nothing here has run on hardware.** The boards arrive around 2026-11-04.
The radio code and the console are written against the datasheet and the
driver's source, and have only been compiled.

Verified on the host:

- `cargo build --release` and `cargo clippy --release -- -D warnings` pass,
  with each of the two builds.
- The ELF's first loadable byte (the vector table) is at `0x2B000`, the
  initial stack pointer is `0x20040000`. The node image is 167 KiB of
  flash (`.text`, `.rodata` and `.data`, 171 644 B, 671 UF2 blocks) and
  uses 176 KiB of RAM before the stack (`.data` and `.bss`, 180 212 B),
  which leaves 79 KiB for the stack (2026-10-25, `llvm-size -A`); the
  console image is 105 804 B. The
  relay, 172 432 B, is in `.bss`: its static holds zero bytes, which the
  startup code writes, and `src/node.rs` sets it up in place. Its deepest
  single stack frame, the largest stack adjustment in the disassembly
  (`llvm-objdump`), is 34 560 B (33.8 KiB), in the main task
  (2026-10-25).
- The driver loop, against a fake radio (`cargo test -p bark-driver` in
  the host workspace), duty-cycled receive included: it starts at the
  core's `Sniff`, starts again after each frame, ends at the sniff's end or
  at the core's next use of the radio, waits for a packet under way, and
  starts again after a packet that never ends (2026-10-20).
- `scripts/uf2.py` produces UF2s that its own `--check` accepts: correct
  magic numbers, family `0xADA52840`, 256-byte payloads in ascending
  order inside `0x26000..0xF4000`; the loader's at `0x26000`, the
  firmware's at `0x2B000`, the erased state page and record.
- The loader builds to 8748 B at `0x26000`, and `cargo clippy --release
  -- -D warnings` passes in `../bark-loader`.
- The swap, on a flash in RAM with the board's layout
  (`cargo test -p bark-image` in the host workspace): a staged image is
  swapped in and kept once confirmed, swapped back when it does not
  confirm, refused when a bit of it is lost or its record is missing; and
  with the power cut at every flash operation of a staging, a swap, a
  swap back and a confirmation, one image is always whole afterwards,
  the new one after a swap and the old one after a swap back (2026-10-25).
- `scripts/image.py stage` sends an image whole, in order, through a
  fake console on a pseudo-terminal (2026-10-25).

Unverified, in the order they will fail if wrong:

- The bootloader accepts the UF2 with the loader and jumps to it at
  `0x26000`, and the loader jumps to the firmware at `0x2B000`.
- The bootloader writes a UF2 block of `0xFF` bytes as an erased page,
  so that a copy clears a swap asked for before it (Build and package).
- The loader's write protection: after the jump, an erase of its pages
  or the bootloader's by the firmware raises a bus fault (nRF52840
  Product Specification, ACL).
- The watchdog: started by the loader for a trial, it resets a firmware
  that never confirms after 600 s and keeps running through the soft
  reset of a panic (Product Specification, POWER, reset behaviour); the
  firmware's clocks start after such a soft reset with the watchdog
  running.
- Interrupts reach the application. The firmware sets VTOR itself
  (cortex-m-rt `set-vtor`) rather than relying on MBR/SoftDevice forwarding.
- The 32 MHz crystal starts (USB needs it) when a node boots on USB
  power or the cable comes, and stops when it goes; whether USB enumerates
  when the cable comes after a boot on battery is not known; the
  low-frequency clock uses the internal RC oscillator so no 32.768 kHz
  crystal is assumed.
- VBUS detection: `USBREGSTATUS.VBUSDETECT` reads the cable, and the
  POWER interrupt comes at each change with the USB driver's handler
  bound beside the node's, so that the core is told within an event's
  time and the node turns a relay on the cable at once.
- The green LED is on P1.01 (the board's label, 2026-10-30) and active low.
- USB enumerates as a CDC-ACM serial port and log lines arrive.
- A panic message survives the soft reset through the bootloader and the
  loader (`PANIC_DUMP` in `bark-image/src/layout.rs`) and is logged on the
  next boot.
- The stored state: the NVMC writes each record the core asks to keep into
  the next slot of the `STORE` page and erases the page when it is full,
  and the next boot logs `stored Some(..)` and comes back on the same home
  channel (`src/store.rs`, roadmap D73).
- P0.13 (`PWR_EN`) and P0.12 (`PWR_ON`) high power the radio, both on USB
  and on battery, and the 10 ms wait after them covers the rail's ramp.
- The radio wiring of `src/board.rs`: SPI on P0.19/P0.22/P0.23, NSS P0.24,
  NRESET P0.25, BUSY P0.17, DIO1 P0.20; DIO2 drives the RF switch; DIO3
  supplies a TCXO at 1.8 V. A wrong TCXO voltage or a board without a TCXO
  shows as `Busy` or `DIO1` errors when the radio starts or on its first
  transmit or receive.
- 8 MHz SPI works on the board's traces.
- The driver's behaviour beyond what its source shows: CRC-failed frames
  can be read back after the error (`get_rx_result` in continuous receive),
  RSSI reads are valid immediately after a retune, and continuous receive
  keeps running after a header or CRC error.
- Duty-cycled receive (`Radio::sniff`, below), in the order it would fail:
  the chip accepts SetRxDutyCycle(384, 22 344) and cycles; it catches a
  frame with the long preamble wherever it starts (`sniff::detects`);
  after the frame it raises RxDone and stops in standby, and the driver
  starts it again; a preamble without a header ends in a timeout or in the
  cycle, and either way the cycle goes on; a falling NSS edge wakes the
  chip from its sleep, and a second try after a BUSY timeout covers an
  edge lost as it goes to sleep; set for 1396 symbols the receiver still
  takes the 12-symbol preamble in continuous receive. `scripts/measure.md`
  step 5 tests each.
- Transmit power: the chip's 10 dBm setting gives about 10 dBm at the
  antenna port, and the stock antenna has no gain over a dipole, so the
  console's 10 dBm cap meets the 10 mW ERP limit.
- Battery voltage reads correctly (P0.04, 2:1 divider; unreliable on USB).
- Every console command. The timings it prints are its purpose; none has
  been seen.
- The display (Display, below): the controller answers on SPIM3 at
  4 MHz, BUSY falls after the reset, the software reset and the refresh,
  the picture comes out black on white the right way up (`ROTATION` in
  `src/display.rs` turns it if not), and the controller's deep sleep
  keeps it on the glass.

## Toolchain

Stable Rust and the Cortex-M4F target:

```
rustup target add thumbv7em-none-eabihf
```

Python 3.9 or later for the scripts (standard library only). No probe, objcopy or
cargo-binutils is needed.

## Flash layout

The T-Echo ships with a UF2 bootloader and keeps it (roadmap D6): it
jumps to `0x26000`. There sits a loader of our own, `../bark-loader`,
which starts the firmware from the slot above it and can swap in a
staged image (Updates, below; roadmap D79). One table,
`bark-image/src/layout.rs`, gives every region; the build scripts of the
firmware and of the loader write each one's `memory.x` from it, with a
symbol for each region, which `scripts/uf2.py` reads from the ELF.
Pages are 4 KiB, the erase unit (nRF52840 Product Specification, NVMC).

| Region | Start | Size | Why |
|---|---|---|---|
| MBR | `0x00000` | 4 KiB | the chip's first code; the bootloader's linker script |
| SoftDevice S140 6.1.1 | `0x01000` | 148 KiB | kept, never enabled; whether the bootloader starts the application without it is not known |
| loader | `0x26000` | 16 KiB, 4 pages | where the bootloader jumps; 8.5 KiB as built, the fourth page room to grow, since the layout cannot move once boards are in the field |
| loader state | `0x2A000` | 4 KiB | the swap's request, progress and confirmation (embassy-boot's state page: 2 + 4 words a slot page, 354 of its 1024) |
| firmware, running | `0x2B000` | 352 KiB, 88 pages | what is left once the loader, its state and the staging have theirs, halved: 2.1 times today's 167 KiB node image, 1.44 times the 250 kB the update note plans for once BLE and the application are in |
| firmware, staged | `0x83000` | 356 KiB, 89 pages | one page larger than the running slot, which embassy-boot's swap needs |
| staged image's record | `0xDC000` | 4 KiB | its header, and the loader's word that it matched it |
| coded pieces | `0xDD000` | 88 KiB, 22 pages | 352 pieces of 256 bytes, five and a half blocks of 64, for the update over the mesh (`docs/mesh-update-2026-10-25.md`, "The code"); unused until then |
| stored state | `0xF3000` | 4 KiB | the node's stored state (`src/store.rs`, roadmap D73), where it was |
| bootloader | `0xF4000` | 38 KiB | the bootloader's linker script |
| bootloader config | `0xFD800` | 2 KiB | |
| MBR parameters | `0xFE000` | 4 KiB | |
| bootloader settings | `0xFF000` | 4 KiB | |

The staging the update note sets aside, the record and the 22 pages of
pieces, does not depend on the image's size: it holds what the mesh
decoder gathers, so it stays at the note's 23 pages, and the image's two
slots take the rest.

RAM, 256 KiB: `0x20000000..0x20000008` is left to the MBR;
`0x20000008..0x20000400` holds a panic message for the next boot
(`panic-persist`), below the RAM of the loader and the bootloader; the
firmware and the loader each use the rest.

## Build and package

From `bark-fw/`, the firmware alone, for a board that already has the
loader:

```
cargo build --release && scripts/uf2.py target/thumbv7em-none-eabihf/release/bark-fw target/bark-fw.uf2
```

The loader and the firmware in one UF2, for a board without the loader
(every board at bring-up) or to replace the loader:

```
(cd ../bark-loader && cargo build --release)
cargo build --release && scripts/uf2.py --loader ../bark-loader/target/thumbv7em-none-eabihf/release/bark-loader target/thumbv7em-none-eabihf/release/bark-fw target/bark.uf2
```

Both also write the loader state page and the record as erased, so a copy
always boots the copied firmware, even over a swap asked for and not yet
made. The firmware alone on a board without the loader does not start:
the bootloader jumps to `0x26000`, which then holds an old image or nothing. `scripts/uf2.py
--check target/bark.uf2` validates a UF2 and prints its block count and
address ranges.

## Flash

1. Connect the T-Echo over USB-C. A USB-C-to-USB-C cable may not power it;
   use a USB-A-to-C cable if nothing happens.
2. Double-tap the top-left button, within about half a second (the
   bootloader waits 500 ms after a pin reset for the second one). The
   bootloader stays resident and a USB drive named `TECHOBOOT` appears. `INFO_UF2.TXT` on it reports the
   bootloader and SoftDevice versions; expect bootloader 0.6.1, S140 6.1.1.
3. Copy the UF2 to it:
   ```
   cp target/bark.uf2 /media/$USER/TECHOBOOT/   # or /run/media/$USER/TECHOBOOT
   ```
   The drive disconnects when the copy completes and the board restarts into
   the application. (Some file managers report an error on that disconnect;
   it is harmless.)
4. Expected: the green LED blinks, 500 ms on and 500 ms off. A serial port
   appears on the host, and once it is opened the log shows a banner
   with the image, its version and the board's node ID (`id=`), then
   `radio up in N us`, then `node id=... at ... usb=true`, or
   `console ready: id=...` from a console build.

If the LED does not blink, double-tap reset again: the bootloader is intact
and will accept another image.

## Updates

A new image is staged in the second slot by the running firmware, swapped
in by the loader at the next boot, and kept only if it confirms itself
(roadmap D79, `docs/mesh-update-2026-10-25.md`). The firmware's side is
`src/update.rs`; both sides of the swap are in `bark-image/src/boot.rs`,
where the host tests run them.

**The image.** A 256-byte header, then the bytes of the firmware's slot
as `scripts/uf2.py` lays them:

| Bytes | Holds |
|---|---|
| 0–3 | `bark` |
| 4 | format, 1 |
| 5–7 | zero |
| 8–11 | version, little-endian, raised with every image |
| 12–15 | the image's length in bytes, little-endian |
| 16–47 | SHA-256 of the image (FIPS 180-4) |
| 48–255 | zero: room for three signatures of 65 bytes, an Ed25519 signature (RFC 8032) and a byte naming its key |

The header is not signed yet: signatures come with the key policy, and
the running firmware will check them before it stages, never the loader
(D79). The firmware's version is `BARK_IMAGE_VERSION` at build time (0
without it), kept under its own symbol, from which `scripts/image.py`
writes the header's; an image is staged only above the running one's
version. The hash is computed with the `sha2` crate (0.10.9), the
firmware's over what it wrote to flash, the loader's again before the
swap.

**The loader**, at every boot:

1. With no swap asked for, it changes nothing.
2. With one asked for, it first checks the staged slot's first `length`
   bytes against the record's SHA-256. If they differ, or the record holds
   no header, it cancels the swap and the running firmware boots, which
   logs `update: the loader refused the staged image`. If they match, it
   notes it in the record, so that a swap a power cut stopped half way,
   whose slot no longer matches anything, goes on without the check.
3. It runs embassy-boot's `prepare_boot`, which swaps the slots, goes on
   with a swap that was stopped, or swaps them back if the new image did
   not mark itself booted. It copies a page at a time into a page whose
   content is held in another, and writes a progress word in the state
   page once each copy is whole, so a power cut anywhere is taken up from
   the copy it stopped (embassy-boot's documentation of `prepare_boot`,
   "SWAPPING" and "REVERTING", read at version 0.7.0). A swap is two
   copies of each of the 88 pages, an erase of 85 ms and 1024 word writes
   of 41 µs each (nRF52840 Product Specification, NVMC), 22 s; a swap back
   as long.
4. On those boots it starts the watchdog first, with a period of
   600 s, `bark_image::TRIAL_SECONDS` (Product Specification, WDT: the
   timeout is (CRV + 1) / 32 768 s), counting while the CPU sleeps, and
   feeds it at every flash operation of the swap.
5. It write-protects `0x00000..0x2A000` (the MBR, the SoftDevice and
   itself) and `0xF4000..0x100000` (the bootloader, its settings and the
   MBR's parameters) through two ACL regions, until the next reset
   (Product Specification, ACL).
6. It jumps to the firmware at `0x2B000`.

**The firmware** confirms itself once its core is set up and its radio
is up (`update::started`), and from then on feeds the watchdog twice a
period, since a started watchdog cannot be stopped before a reset
(Product Specification, WDT). An image that hangs before then is reset
by the watchdog; one that panics resets itself through `panic-persist`,
and the watchdog runs on through that soft reset (Product Specification,
POWER, reset behaviour); either way the loader swaps the previous image
back, which logs `update: the new image did not confirm itself`. The
confirmation will also ask for a neighbour heard, within the same 600 s,
once the mesh carries updates.

Each update erases each page of the slots at most twice, once in staging
and once in the swap, and the state page twice: the 10 000 erase cycles
of the flash (Product Specification, NVMC) last 5000 updates.

**Staging is a bench tool** until the update has a transport (BLE, then
the mesh). A console build takes an image over its serial port:
`stage <header hex>`, `chunk <offset> <hex>` for up to 256 bytes at a
time, in order, `commit`, which checks the hash, records the header and
asks for the swap, and `reboot`. `scripts/image.py` packs and sends it:

```
BARK_IMAGE_VERSION=2 cargo build --release --features console
scripts/image.py pack target/thumbv7em-none-eabihf/release/bark-fw target/v2.img
BARK_IMAGE_VERSION=1 cargo build --release --features console
scripts/uf2.py --loader ../bark-loader/target/thumbv7em-none-eabihf/release/bark-loader target/thumbv7em-none-eabihf/release/bark-fw target/bark.uf2
# copy target/bark.uf2 to the board, then:
scripts/image.py stage target/v2.img /dev/ttyACM0 --reboot
```

The log then shows `image version 2` and `update: version 2 confirmed`.
`BARK_UPDATE_BENCH` at build time makes the images that must be swapped
back: `unconfirmed` runs and never confirms, `panic` panics where it
would confirm, `hang` spins there with everything else stopped; and
`erase-loader` tries to erase the loader's first page, which the write
protection must stop with a bus fault (should it get through, the board's
bootloader finds nothing to start and waits for a UF2, so the test costs
at most a copy). Every image logs the two protected regions as the ACL
reads them back, `update: acl 0 start=0x0 len=0x2a000 write=Disable`
and `update: acl 1 start=0xf4000 len=0xc000 write=Disable`.

## Display

At boot every image writes one picture to the 1.54" e-paper
(200 × 200, SSD1681 controller, `src/display.rs`): the board's node ID
as twelve hex digits in two rows of six, 25 × 35 pixels each, the
largest six fit across the panel, below them the image's name, `NODE`,
`CONSOLE` or `SURVEY`, and its version as `V` and `BARK_IMAGE_VERSION`,
a survey's with its channel as `CH`. The text
comes from a 5 × 7 bitmap font of the hex digits and the few letters
those words need, scaled by whole pixels. A board can then be told from
another unplugged, and the hex matches the `id=` of its log and its USB
serial number.

The panel has its own SPI (SPIM3, 4 MHz) and its refresh runs as its
own future beside the radio's start, so it delays nothing: the picture
is rendered in microseconds, the 5000-byte RAM write is DMA, and every
wait on the controller's BUSY line is on an interrupt and bounded at
6 s, three times the 2 s full refresh the board's label gives. A
controller that does not answer logs `display: busy past 6 s at <step>;
no picture` and the boot goes on without it. The sequence is the
controller's: reset, software reset, the driver output, data entry,
RAM window, border and temperature settings, the picture into both RAM
planes, one full refresh, then deep sleep mode 1, which holds the
picture on the glass and draws nothing until a hardware reset. The
front light (P1.11) stays off.

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

Built with `--features console`. The serial port carries the log and takes
commands. Type a command and
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
| `rx <k> [ms]` | listen on k for ms (default 10 000), reporting every frame; what is typed meanwhile runs after | per frame: `len`, `rssi_dbm`, `snr_db`, `crc=ok\|error`, `bytes`, or for a survey frame its sender's `survey_power_dbm` and `seq` in place of the bytes; for a heartbeat also its sender's `id`, the newest `format` it speaks, `map_point` and its `home`; at the end the count of `frames` and `crc_errors` |
| `cad <k>` | one CAD (8 symbols, the driver's fixed setting) | `busy`, `prepare_us`, `cad_us`, `rest_us` |
| `rssi <k> [dwell_us]` | retune to k, wait, read RSSI once, retune home | `dbm`, `retune_us`, `dwell_us`, `read_us` |
| `sweep` | one RSSI reading on each of the 13 channels | 13 values, `total_us` |
| `noise <k> <n> [gap_us]` | n readings (up to 1000) on k | `min`, `p10`, `median`, `max`, `total_us` |
| `power <dbm>` | transmit power, clamped to −9..10 dBm | the power in effect |
| `preamble <short\|long\|n>` | preamble of `tx` and `beacon` frames: 12 symbols, the 1396 of a frame to a sleepy node, or n between | `symbols`, the airtime of a heartbeat, 13 bytes, and of a 255-byte frame |
| `beacon <k> <ms> [len]` | send a bench frame (default 18 bytes, an ACK's length) on k every ms; refused below a 10 % duty cycle | one line per beacon |
| `stop` | stop the beacon | |
| `ping <k>` | send an 18-byte ping on k, wait 1 s at home for the pong | `rtt_us` from TxDone to the pong, against the 100 ms ACK timeout (spec "Hand-over") |
| `status [reset]` | node ID, uptime, battery mV, channel, power, counters; `reset` zeroes the counters | |
| `survey [n]` | a survey build's noise log, the newest n minutes (default all), oldest first | the survey's channel and progress, then per minute `t_s`, `p10` and `median`, 13 values each |
| `stage <header hex>` | start staging an image (Updates) | `version`, `length`, `sha256`, or why it is refused |
| `chunk <offset> <hex>` | write up to 256 bytes of it, at the offset the last chunk ended | `next` |
| `commit` | check it, record it for the loader and ask for the swap | `version`, `length`, or why it is refused |
| `reboot` | reset, after the log has reached the host | |

Between commands the radio listens on the home channel and prints every
frame it hears, CRC failures included. A board that receives a ping on its
home channel answers it like an ACK (spec "The ACK"): CAD on the sender's
home channel (named in the ping), then an 18-byte pong, then back home; it
prints the time of each part. It does not do the RSSI half of
listen-before-talk.

Bench frames start with a heartbeat's first byte on channel 15, which no
node sends and every node rejects (spec "Packet formats"): byte 0 is
`0x8F`, byte 1 the format version as in every packet, byte 2 the kind
(0 test or beacon, 1 ping, 2 pong, 3 survey), byte 3 the kind's
argument, the sender's home channel or for a survey frame its transmit
power in dBm as a signed byte, bytes 4 to 7 a big-endian sequence
number, then `byte[i] = i`. RSSI is whole dBm (the driver drops the
chip's half-dB step); packet RSSI and SNR are in quarter dB.

### Survey build

A console built with `BARK_SURVEY=<k>`, k a channel, surveys a place
from the moment it boots, on battery and with no host: a board left on
a windowsill, in another room or across the street, read back when it
is plugged in again. It needs no command. The near board, on the host,
listens with `rx <k> <ms>` for as long as the host logs.

- **Beacons** on channel k, forever: a frame of 8, 64 and then 255
  bytes, and for each length every transmit power from −9 to 10 dBm in
  1 dB steps, 60 frames a cycle, each a survey bench frame saying its
  power. After each frame the board is silent for the rest of the
  period the 10 % duty cycle allows that frame, as `beacon` enforces
  (spec "Regulatory limits"): 38 s a cycle, most of it the 255-byte
  frames' 1.39 s periods, so each power and length is sent some 94
  times an hour. The near board's `rx` lines
  give, per power and length, the frames heard and their RSSI and SNR,
  from which the loss curve and the RSSI calibration follow; the
  frames it did not hear are the cycle's missing sequence numbers.
- **Noise**, once a minute: on each of the 13 channels, a retune, a
  wait of four symbol periods for the reading to settle
  (`docs/bench-2026-10-30.md`, RSSI against dwell), then 100 RSSI
  readings back to back, as `noise <k> 100`, kept as the 10th
  percentile and the median in whole dBm. The minute's line is logged,
  and kept in a ring in RAM of the last 48 hours (86 KiB, only in this
  build), which `survey` prints when the board is back on the host:
  one line a minute, paced so the log buffer never drops one, about
  12 s for two days.

The log never waits for a host: lines logged with no port open fill
the 2 KiB buffer and newer ones are dropped (Log, below), so a survey
board runs the same with and without one. Its home channel is k, so
`ping k` from the near board is answered. The display says `SURVEY`
and `CH<k>` (Display, above).

`scripts/measure.md` is the measurement plan for bring-up steps 1 to 3 and 5
and bench item C1, with a table that maps the results onto the simulator's
radio parameters.

## Radio driver

`lora-phy`, copied into `vendor/lora-phy` from revision `7a9c2ec` of the
lora-rs `main` branch (2026-10-06; roadmap D4, D20); `vendor/README.md` lists
what differs from upstream and how to update it. That revision reports
payload and header CRC failures as errors instead of returning the bytes as
a good frame, and has `get_rssi`, `rx_switch_channel` and quarter-dB packet
RSSI. The firmware gives the
driver its own board interface (`radio::Pins`) instead of the driver's
generic SX126x one: the generic one polls BUSY immediately after each
command, but BUSY may take up to 600 ns to rise (DS §8.3.1), so the poll
can see the previous low and send the next command too early. Ours waits
out the rise first, bounds every BUSY wait (17 ms, twice a calibration plus
a TCXO start) and can bound the wait for DIO1, so a radio that does not
answer gives an error instead of a hung console.

Between operations the radio waits in STDBY_XOSC, with its oscillator
running, and transmit and receive end there (SetRxTxFallbackMode). The
SX1262 powers the T-Echo's TCXO from DIO3 in every mode but sleep and
STDBY_RC, and on each start it waits out a programmed delay in full
(DS §13.3.6). From STDBY_XOSC no start is needed: the chip reaches RX in
62 µs and TX in 105 µs (DS Table 8-2). A retune is a SetStandby, a
SetRfFrequency and the SetRx with its three set-up commands; the chip's
part is about 62 µs plus the unlisted RX to STDBY_XOSC step, and the
board measured 668 µs for the whole, the SPI commands the most of it
(`docs/bench-2026-10-30.md`). Waiting in STDBY_XOSC costs 0.8 mA against 0.6 mA in STDBY_RC
(DS Table 3-5), plus the TCXO's own current, nominally 1.5 mA (DS §4.1.4),
which it also draws in RX (4.6 mA, DS Table 3-5). The radio listens
whenever it is not transmitting or checking a channel, so it spends only
the moments between operations in standby and the extra current does not
matter.

A sleepy node's radio receives duty-cycled whenever its core allows it
(`Action::Sniff`, `docs/core-interface.md` §3 rule 10): SetRxDutyCycle
(DS §13.1.7) with a 6 ms window (384 steps of 15.625 µs) and a 349.125 ms
sleep (22 344 steps), `bark_proto::sniff`. The command is sent in
STDBY_RC, where the datasheet starts the cycle, and the receive timer
stops at a header rather than at a preamble, so that after a preamble the
chip waits `2 × rx + sleep` for the header and no longer (DS §13.1.6,
§13.1.7; `vendor/README.md`). In each sleep the chip keeps its
configuration and its 64 kHz clock and powers the TCXO down: DIO3 supplies
it only in STDBY_XOSC, FS, TX and RX (DS §13.3.6). Each wake costs about
1 ms to restore its state and the 2 ms TCXO start (DS Figures 13-2 and
13-3); the cycle was derived for a 6 ms wake, which only adds margin
(`bark_proto::sniff::SNIFF_WAKE`). The Rx gain register is lost in each sleep (DS §9.6); the firmware
uses the default power-saving gain, which is what the register returns to.
After a frame the chip stops in standby, and the driver starts the cycle
again once the core has the frame; any command to the chip is preceded by
a falling NSS edge, which wakes it.

The radio's expected current in the cycle, from the datasheet: the window
in receive at 4.6 mA (DS Table 3-5, DC-DC, LoRa 125 kHz, power-saving
gain) and the TCXO at its nominal 1.5 mA (DS §4.1.4), 6.1 mA; the 6 ms of
each wake, for which the datasheet gives no figure, between STDBY_RC's
0.6 mA with the TCXO, 2.1 mA, and the full 6.1 mA; the sleep at 1.2 µA
(DS Table 3-5, configuration kept and the 64 kHz clock on). Over the
361.125 ms cycle:

```
upper: (12 × 6.1 + 349.125 × 0.0012) / 361.125 = 0.204 mA
lower: (6 × 6.1 + 6 × 2.1 + 349.125 × 0.0012) / 361.125 = 0.137 mA
```

against 6.1 mA listening, 30 to 44 times less. The core's own figure,
`sniff::SNIFF_CURRENT_NA`, is 0.194 mA, from 5.8 mA for receive and wake.
`scripts/measure.md` step 5 measures it.

The programmed delay, 2 ms (`radio::TCXO_START`), is still paid where the
chip turns the TCXO off on its own: at power-up, after a calibration and
after every CAD, which ends in STDBY_RC whatever the driver asks
(DS Table 13-73). The driver returns to STDBY_XOSC at once, so the wait
falls inside the CAD (the console's `cad_us`), not inside the next
operation.

## The current on battery

On battery the node is sleepy, and its current is what matters.
Between the core's wakeups and the radio's interrupts nothing runs: the
driver's loop waits on one embassy timer (the core's next wakeup, the end
of a sniff or of a hold), DIO1 through GPIOTE, an application that
has nothing to send and a change of USB power, and the embassy executor
sleeps in `wfe` meanwhile. The USB device waits for VBUS and the log for
the host to open the port, both on interrupts. Three things that would
keep the board awake stop while it is on battery, and start again on USB:

- The LED blink: the LED stays dark, and the CPU is not woken twice a
  second.
- TIMER1, the console's 1 µs stopwatch: a running TIMER keeps the
  high-frequency clock requested while the CPU sleeps (nRF52840 Product
  Specification, TIMER and CLOCK). A node's timings come from the embassy
  clock, to 30.5 µs, on USB too.
- The 32 MHz crystal, once started, runs on while the CPU sleeps. A node
  starts it at boot on USB power and whenever USB comes, which USB
  needs (the console always does), and stops it when USB goes. On
  battery the high-frequency clock is the internal oscillator, started
  only while something needs it. Unverified: that USB enumerates when
  the cable comes after a boot on battery, the crystal started on the
  power event (Unverified, below).

What stays, and is not this firmware's to fix without a board:

- The GPS. It is on the switched rail with the radio (Radio power) and
  draws its own current, likely more than everything else together; its
  standby pin (P1.02, `board.rs`) has an unverified polarity. Measure
  the board with it as it is first, then decide.
- The battery divider, about 12 µA all the time (`board.rs`, Battery).
- The nRF52840's regulator runs as an LDO (embassy's default); the DC-DC
  needs an inductor the T-Echo may or may not have.
- The e-paper's controller is in deep sleep after the boot picture and
  the external flash is not touched; their idle current is unknown.

The radio's part is the cycle above, 0.14 to 0.20 mA, plus its beats and
the windows after them (spec "Beats"); without an anchor each beat is a
369 ms frame with the long preamble at the transmit current, every 34.6 s
on average (`bark_proto::sniff`). No figure for the whole board is claimed here;
`scripts/measure.md` step 5 measures it in each state.

## Risks

- **Every CAD costs a TCXO start, 2 ms.** No source names the module's
  TCXO or its start-up time; 1 ms worked on two boards at room
  temperature (`docs/bench-2026-10-30.md`, roadmap D87). Too short a delay
  raises XOSC_START_ERR (the firmware does not read the chip's error flags
  yet) and lets the chip run on a clock still settling; a cold board may
  need more than the bench's.
- The CAD parameters are fixed in the driver (8 symbols, detection peak
  SF + 13, minimum 10). Tuning them means patching it.
- The sync word is 0x1424, the conventional private LoRa word, which other
  private networks also use. Their frames would reach the console, and
  bark's decoder rejects them by content. A bark-specific word is an open
  choice.
- The driver is a copy of a moving upstream; an update must be reviewed and
  our changes reapplied.

## Log

The firmware enumerates as USB VID:PID `1209:0001`, a pid.codes test ID that
must not be used on distributed devices, with the board's node ID, twelve
hex digits, as its serial number. On Linux it appears as `/dev/ttyACM0`
(or the next free number; `dmesg | tail` says which) and, by board, as
`/dev/serial/by-id/usb-bark_bark_T-Echo_<id>-if00`.

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

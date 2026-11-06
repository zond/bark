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
word, the receiver set for the 927-symbol long preamble) and then does
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
describes it. A node's place can be set when it is built, from `BARK_LAT`
and `BARK_LON` in degrees:

```
BARK_LAT=50.4501 BARK_LON=30.5234 cargo build --release
```

`build.rs` converts them to the position's counts with the core's
conversion, so the image parses no numbers; one set without the other,
or a value that is not a number, fails the build. Without them the node
has no place until the phone sets a map point (roadmap D83).

A built place is not a map point: the GPS runs with one as without, and
only a map point the phone sets keeps it at rest (spec "Position"; the
`boot:` line says `gps=` and `map_point=`). But a fix is taken only
within 3 km of a relay the node knows (spec "Position"), and a board
built with a place it does not stand at, beside others built the same,
knows only relays at that place: its every true fix is dropped
(`gps: fix … beyond every known relay; dropped`, once), though it still
keeps the time from them. To bench the GPS, build without a place or
with the place the board stands at. Every protocol event (`Trace`), every message for the node, and
a sleepy node's own message given back by its first hop with the time
left to wait (`given back ... retry_in_ms=`) is a log line, and at
the first event 10 s or more after the last a `driver` line gives the
loop's counters (`sniffs` counts the starts of duty-cycled receive) and
its longest times and `stack_used`, the call stack's high-water mark.
Its application is the phone link over BLE (`src/ble.rs`;
spec "Phone link"), whose bench procedure in nRF Connect is in
`docs/phone-link-2026-11-05.md`, "On the bench"; since roadmap D135 the
link needs a pairing first, with the code the screen shows (Phone
link, below). Its GPS runs as the
core says, in its own task (`src/gps.rs`; `docs/gps-time-2026-11-05.md`):
on USB all the time, on battery in standby but for the searches the core
asks for. Once a fix or the phone has given the time, the 10 s line ends
in `utc_s=`, `offset_min=` and its source.

## Verified and unverified

**Nothing here has run on hardware.** The boards arrive around 2026-11-04.
The radio code and the console are written against the datasheet and the
driver's source, and have only been compiled.

Verified on the host:

- `cargo build --release` and `cargo clippy --release -- -D warnings` pass,
  with each of the two builds.
- The ELF's first loadable byte (the vector table) is at `0x06000`, the
  initial stack pointer is `0x20040000`. The node image is 167 KiB of
  flash (`.text`, `.rodata` and `.data`, 171 644 B, 671 UF2 blocks) and
  uses 176 KiB of RAM before the stack (`.data` and `.bss`, 180 212 B),
  which leaves 79 KiB for the stack (2026-10-25, `llvm-size -A`); the
  console image is 105 804 B. The
  relay, 172 432 B, is in `.bss`: its static holds zero bytes, which the
  startup code writes, and `src/node.rs` sets it up in place. The
  stack's depth is in "Tasks and the stack", below.
- The driver loop, against a fake radio (`cargo test -p bark-driver` in
  the host workspace), duty-cycled receive included: it starts at the
  core's `Sniff`, starts again after each frame, ends at the sniff's end or
  at the core's next use of the radio, waits for a packet under way, and
  starts again after a packet that never ends (2026-10-20).
- The radio's command sequences, against a chip that records them
  (`bark-driver/src/sx1262.rs`, the same `cargo test`): what is sent once,
  what each operation sends, the whole configuration again after a reset,
  a sleep or an error, the oscillator's delay short after a CAD and probed
  after a sleep, the search for the delay at boot, the step up on a start
  error and the probe below as the die warms, and the self-check of the
  reading inside a CAD (2026-10-30, roadmap D88).
- `scripts/uf2.py` produces UF2s that its own `--check` accepts: correct
  magic numbers, family `0xADA52840`, 256-byte payloads in ascending
  order inside `0x01000..0xEA000`; the loader's at `0x01000`, the rest
  of the old SoftDevice's pages erased, the firmware's at `0x06000`, the
  erased state page and record.
- The loader builds to 8.5 KiB at `0x01000`, and `cargo clippy --release
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

- The loader jumps to the firmware at `0x06000`. The bootloader's jump
  to the loader at `0x01000` was seen on f4f5 on 2026-11-06, with the
  firmware still at `0x2B000` (`docs/softdevice-2026-11-05.md`).
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
- The 32 MHz crystal, which USB needs: the console starts it at boot and
  keeps it, and the node asks it of the MPSL while on USB (`src/main.rs`,
  `src/ble.rs`) and lets it go on battery; whether USB enumerates
  when the cable comes after a boot on battery is not known.
- Pairing (Phone link, below; roadmap D135): that a phone pairing
  with the code on the screen bonds, that the bond survives a reboot and
  encrypts the next connection without a code, that a second phone's
  pairing replaces it and the first phone's reads then fail, and that
  the controller takes the host's resolving-list commands. Only built.
- The 32.768 kHz clock: whether the board has the crystal is not known.
  The node tries it at boot, giving it 1 s, and falls back to the
  calibrated internal RC oscillator, logging which runs (`src/lfclk.rs`);
  the console keeps the RC oscillator.
- The bootloader from the serial port (`src/touch.rs`): 0x57 in GPREGRET
  as the bootloader's UF2 reset magic is taken from its source as
  remembered, not read from the board's 0.6.1; the bench's first
  `stty … 1200` shows the drive or not.
- VBUS detection: `USBREGSTATUS.VBUSDETECT` reads the cable, and the
  POWER interrupt comes at each change with the USB driver's handler
  bound beside the node's, so that the core is told within an event's
  time and the node turns a relay on the cable at once.
- The green LED is on P1.01 (the board's label, 2026-10-30) and active low.
- USB enumerates as a CDC-ACM serial port and log lines arrive.
- A panic message survives the soft reset through the bootloader and the
  loader (`PANIC_DUMP` in `bark-image/src/layout.rs`) and is logged on the
  next boot.
- The stored state: the NVMC, through the MPSL's flash timeslots, writes each record the core asks to keep into
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
- The chip's behaviour beyond what the datasheet says: CRC-failed frames
  can be read back after the error, continuous receive keeps running after
  a header or CRC error, `GetRssiInst` answers during a CAD as the bench
  saw (the boot self-check guards it, Radio driver below), the
  configuration sent once holds across every operation but a sleep, and a
  delay of 100 µs restarts the oscillator cleanly within a millisecond of
  a CAD's end (`docs/bench-2026-10-30.md`, roadmap D88).
- Duty-cycled receive (`Radio::sniff`, below), in the order it would fail:
  the chip accepts SetRxDutyCycle(240, 14 952) and cycles; it catches a
  frame with the long preamble wherever it starts (`sniff::detects`);
  after the frame it raises RxDone and stops in standby, and the driver
  starts it again; a preamble without a header ends in a timeout or in the
  cycle, and either way the cycle goes on; a falling NSS edge wakes the
  chip from its sleep, and a second try after a BUSY timeout covers an
  edge lost as it goes to sleep; set for 927 symbols the receiver still
  takes the 12-symbol preamble in continuous receive. `scripts/measure.md`
  step 5 tests each.
- Transmit power: the chip's 10 dBm setting gives about 10 dBm at the
  antenna port, and the stock antenna has no gain over a dipole, so the
  console's 10 dBm cap meets the 10 mW ERP limit.
- Battery voltage reads correctly (P0.04, 2:1 divider; unreliable on USB).
- The GPS (`src/gps.rs`): NMEA at 9600 bps on P1.09 through UARTE1,
  its sentences set by PCAS03 on P1.08 at each start, WAKEUP on P1.02
  high to run and low for standby, a hot start after standby; `docs/gps-time-2026-11-05.md`, "On the bench".
- Every console command. The timings it prints are its purpose; none has
  been seen.
- The display (Display, below): the controller answered on SPIM3 at
  4 MHz in the console build, and in the node build never started the
  refresh (bench, 2026-11-05); it now runs on SPIM1. The node's state
  screen and its redraws are untried. BUSY
  falls after the reset, the software reset and the refresh, the
  picture comes out black on white the right way up (`ROTATION` in
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

The T-Echo ships with a UF2 bootloader and keeps it (roadmap D6). With
the SoftDevice it came with erased, it jumps to `0x01000`, the MBR's end
(roadmap D129, `docs/softdevice-2026-11-05.md`). There sits a loader of our own, `../bark-loader`,
which starts the firmware from the slot above it and can swap in a
staged image (Updates, below; roadmap D79). One table,
`bark-image/src/layout.rs`, gives every region; the build scripts of the
firmware and of the loader write each one's `memory.x` from it, with a
symbol for each region, which `scripts/uf2.py` reads from the ELF.
Pages are 4 KiB, the erase unit (nRF52840 Product Specification, NVMC).

| Region | Start | Size | Why |
|---|---|---|---|
| MBR | `0x00000` | 4 KiB | the chip's first code; the bootloader's linker script |
| loader | `0x01000` | 16 KiB, 4 pages | where the bootloader jumps with no SoftDevice; 8.5 KiB as built, the fourth page room to grow, since the layout cannot move once boards are in the field; a UF2 with the loader erases what is left of the SoftDevice up to `0x26000` |
| loader state | `0x05000` | 4 KiB | the swap's request, progress and confirmation (embassy-boot's state page: 2 + 4 words a slot page, 394 of its 1024) |
| firmware, running | `0x06000` | 392 KiB, 98 pages | what is left once the loader, its state, the staging and the mailbox have theirs, halved: 40 KiB more than the 351 368 B node image of 2026-11-05 |
| firmware, staged | `0x68000` | 396 KiB, 99 pages | one page larger than the running slot, which embassy-boot's swap needs |
| staged image's record | `0xCB000` | 4 KiB | its header, and the loader's word that it matched it; below `0xEA000`, where the bootloader's drive stops writing |
| coded pieces | `0xCC000` | 88 KiB, 22 pages | 352 pieces of 256 bytes, five and a half blocks of 64, for the update over the mesh (`docs/mesh-update-2026-10-25.md`, "The code"); unused until then |
| mailbox | `0xE2000` | 68 KiB, 17 pages | its state, then a ring of 16 pages, 256 items (`docs/app-layer-2026-11-05.md`, section 5); unused until the mailbox is built |
| stored state | `0xF3000` | 4 KiB | the node's stored state (`src/store.rs`, roadmap D73), where it was; with it the phone's UTC offset and the bond with the paired phone |
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
the bootloader jumps to `0x01000`, which then holds an old loader, the SoftDevice or nothing. `scripts/uf2.py
--check target/bark.uf2` validates a UF2 and prints its block count and
address ranges.

The release profile optimises for size, `opt-level = "z"`, with fat LTO
and one codegen unit: the node image is 396 864 B of the 401 408 B slot,
4 544 B free; the pairing of roadmap D135 took 55.8 KB of it
(`docs/flash-2026-11-06.md` measures the options,
`docs/pairing-2026-11-06.md` the pairing's). The core's slowest
step shows on the board as `longest_handle_us` in the 10 s line.

The image is the same wherever the repository is checked out. Source
paths end up in it, a panic's location among them, and absolute ones
would make its size depend on the checkout's path: a build from a path
18 characters longer once took 168 B more flash. So Cargo runs rustc
through `scripts/rustc-remap.sh` (`build.rustc-wrapper` in
`.cargo/config.toml`, the loader's too), which maps the checkout's root
to `/bark`, Cargo's home to `/cargo` and the standard library's source
to `/rustc/<commit>`, where the toolchain's compiled library has it; the build commands above need
nothing more, here and in the workflows. A wrapper changed after a
build does not rebuild by itself: `cargo clean` first.

## Flash

1. Connect the T-Echo over USB-C. A USB-C-to-USB-C cable may not power it;
   use a USB-A-to-C cable if nothing happens.
2. Double-tap the top-left button, within about half a second (the
   bootloader waits 500 ms after a pin reset for the second one). The
   bootloader stays resident and a USB drive named `TECHOBOOT` appears. `INFO_UF2.TXT` on it reports the
   bootloader and SoftDevice versions: bootloader 0.6.1, and
   `SoftDevice: not found` once the board has had the loader at `0x01000`
   (S140 6.1.1 before that).
   A board running this firmware takes the same step from the serial
   port, without the button: `stty -F /dev/ttyACM0 1200` (its port by
   ID is `/dev/serial/by-id/*bark*`) resets it into the bootloader with
   its drive (`src/touch.rs`). Any other rate, the log's included, does
   nothing.
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

A board of the layout before 2026-11-06, with its loader at `0x26000` above
the SoftDevice, takes `bark.uf2`, the UF2 built with `--loader`, once:
enter its drive by a double tap or by the 1200-baud touch, which its
firmware has as well, and copy it. The copy puts the loader at `0x01000`
and erases the rest of the SoftDevice, and from then on the bootloader
starts the loader there and reports `SoftDevice: not found`. `bark-fw.uf2`
alone is not for such a board: it leaves the SoftDevice's magic number
in place, so the bootloader still jumps to `0x26000`, where the new
firmware's bytes now lie, and the board does not start; a double tap
and `bark.uf2` put it right.

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
(D79). The firmware's version is `BARK_IMAGE_VERSION` at build time,
kept under its own symbol, from which `scripts/image.py`
writes the header's; an image is staged only above the running one's
version. Without the variable the version is 0: a bench build, flashed
by UF2, which any staged image replaces and which can itself never be
staged over another, since `scripts/image.py` packs only a version
above 0. The hash is computed with the `sha2` crate (0.10.9), the
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
   copies of each of the 98 pages, an erase of 85 ms and 1024 word writes
   of 41 µs each (nRF52840 Product Specification, NVMC), 25 s; a swap back
   as long.
4. On those boots it starts the watchdog first, with a period of
   600 s, `bark_image::TRIAL_SECONDS` (Product Specification, WDT: the
   timeout is (CRV + 1) / 32 768 s), counting while the CPU sleeps, and
   feeds it at every flash operation of the swap.
5. It write-protects `0x00000..0x05000` (the MBR and itself) and `0xF4000..0x100000` (the bootloader, its settings and the
   MBR's parameters) through two ACL regions, until the next reset
   (Product Specification, ACL).
6. It jumps to the firmware at `0x06000`.

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
reads them back, `update: acl 0 start=0x0 len=0x5000 write=Disable`
and `update: acl 1 start=0xf4000 len=0xc000 write=Disable`.

## Display

The 1.54" e-paper (200 × 200, SSD1681 controller, `src/display.rs`)
says what a person needs to tell boards apart and to see how a node is
doing without a phone. The text comes from a 5 × 7 bitmap font of the
digits, the capital letters and a few signs, scaled by whole pixels.

The console and survey images draw one picture at boot: the node ID as
twelve hex digits, then the image's name, `CONSOLE` or `SURVEY`, and its
version as `V` and `BARK_IMAGE_VERSION`, a survey's with its channel as
`CH`.

The node image draws its state, built from what its core says after
each step (`src/node.rs`, `screen`) by `bark-screen`, which is tested on
the host. Sixteen characters to a line, every line from the left
margin. A header says what the node is, as a word or two at the left,
and a gap sets it apart; every line below it is a label at the left and
its value flush right. Top to bottom:

| line | says | from |
|---|---|---|
| role | `FIXED RELAY`, `CARRIED RELAY`, `RELAY NO PLACE` (before its first place), `SLEEPY ANCHORED` or `SLEEPY NO ANCHOR` | the core's sleepy, anchorless, placed and carried flags |
| `STARTING` | while the first rotation runs, under the role in the header | booting |
| `ID BAE6E6CBF4F5` | twelve hex digits, as in the log's `id=` and the USB serial number | the chip |
| image | `NODE V12` | `BARK_IMAGE_VERSION` |
| phone | `PHONE NONE` or `PHONE PAIRED`; while a phone pairs, `PAIR CODE 012345`, drawn at once | the phone link (Phone link, below) |
| `KNOWN 5/4/2` | neighbours: those whose heartbeats it hears now; of them, those heard loudly enough that a full frame reaches it through a fade; of them, those that have taken a message from it | the neighbour table's live entries; their averaged RSSI against the margin the core routes by (spec "Next hop"); their `taken_at` |
| battery | `POWER` `USB`, or `BATTERY` with `73%`, `LOW` or `NOT READ` | the reading as the core calibrates it |
| `UP 12 MIN` | time since boot, in minutes, then hours, then days | the clock |
| `TIME` `14:35 +02` | the time of day when it was drawn: local with the phone's offset (`+0530` for one with minutes), or `12:35 UTC` without one; no line while the time is not known | `src/utc.rs` (`docs/gps-time-2026-11-05.md`) |

*Why a sleepy node's place is not shown.* A sleepy node is reached
through its anchor: its address is its anchor's position, and while it
has one it takes no fix of its own (spec "Sleepy nodes"). So its place
matters little to a reader, and the line has no room for it beside
`SLEEPY NO ANCHOR`.

*Why three neighbour counts.* A heartbeat is one way: a node that hears
a neighbour does not know the neighbour hears it. Only an answer to its
own frame proves that, and the table keeps one sign of it, when the
neighbour last took a hand-over of this node's: the third count. The
second is a good guess, shown apart from the third for that reason:
links on one channel are mostly symmetric, and a neighbour heard with
margin will likely hear a frame at the same power. A refusal or an ACK that has seen the packet also prove the
neighbour hears this node; the table keeps neither per neighbour. A
bit for "answered" would cost no RAM, since it fits the padding of the
104-byte entry (measured), and would count those two as well; it is
left out while the display is its only reader, since on a quiet network
neither comes before a take does.

*Why the percentage is "about".* The node knows three open-circuit
voltages of a typical cell: 20 %, 30 % and full (Maxim AN4189, the core's
battery levels). It reads the charge off the straight line between them
(`bark_proto::node::battery_percent`), which is close at the low end
and a coarse guess between 30 % and full, where a cell's curve is not
straight. At or below 20 % it says `LOW`: the curve falls steeply there,
and a fixed relay turns sleepy.

*Why the uptime.* A picture stays on the glass when the node stops,
so a stale one would lie. The node redraws at least once an hour on
battery and every ten minutes on USB; an uptime that has not moved
after that long means the node is not running.

*When it redraws.* At boot, once the node's loop has run (at most 5 s),
then when a fact it shows changes, but not within 5 minutes of the
last refresh on battery or a minute on USB, and at least once an hour on
battery and every ten minutes on USB. A press of the user button
(P1.10, `src/button.rs`) redraws it at once, a bench aid that leaves
that schedule as it was. The cost on battery: a full
refresh holds BUSY high for 2 s (the board's label). The panel's
current during a refresh is not in the repository's notes, nor its
datasheet; taking 10 mA, an assumption to be replaced by a measurement
with `scripts/measure.md`, a refresh is 20 mAs, 5.6 µAh,
of the 850 mAh cell (spec "Hardware") one part in 150 000. At the
5-minute floor, with something changing all the time, that is 0.067 mA
on average, a tenth of a sleepy node's 0.65 mA
(`docs/battery-role-2026-10-25.md`); with the hourly redraw alone,
0.006 mA, under 1 %. So 5 minutes is the floor at which the display
costs a sleepy node at most a tenth of its battery life, and the
hourly redraw is the price of knowing the node runs. On USB the
energy is the charger's; a minute keeps the picture current enough.
Between refreshes the controller is in deep sleep, holding the picture
and drawing nothing measurable; each refresh wakes it with a hardware
reset.

*Beside the radio.* The panel has its own SPI (SPIM1, 4 MHz; not
SPIM3, whose transmitted data can be corrupted while the CPU or another
EasyDMA master uses its buffer's RAM block, nRF52840 Errata anomaly
198) and shares no pin with the radio. Its refreshes run as a task of
their own beside the radio's loop, with the picture in a static: joined
into the main task as a future, they grew that task's stack frame, the
deepest, from 29 184 B to 42 752 B and a nested 8 000 B beside, past
the 43 KiB left for the stack, and the first image hung at boot before
USB came up (bench, 2026-11-05); "Tasks and the stack", below, has the
rest. In its task the picture is drawn a line at a time,
tens of microseconds each, the executor given back between lines; the
5000-byte RAM writes are DMA; and every wait on the controller's BUSY
line is on an interrupt, bounded at 6 s, three times the 2 s refresh.
So a refresh during a hand-over holds the radio's loop for no more than
one line's drawing, far inside the 3.6 ms the loop must be back by
(D37); the bench checks it in `longest_away_us` across a refresh. The
switched rail, which the panel shares with the radio, is raised by the
radio first thing and held for as long as the firmware runs; the
panel's pins stay driven between refreshes, RES# high, since a
disconnected RES# could wake the controller.

A controller that does not answer logs `display: busy past 6 s at
<step>; no picture`; one that never raises BUSY within 100 ms of being
asked to refresh did not take the commands and logs `display: the
refresh never started; no picture`; the node goes on either way. The
sequence is the controller's: hardware reset, software reset, the
driver output, data entry, RAM window, border and temperature
settings, the picture into both RAM planes, one full refresh, then deep
sleep mode 1. The front light (P1.11) stays off.

## Phone link

The link (`src/ble.rs`, spec "Phone link") is open only to the paired
phone (roadmap D135). Every characteristic of the service needs a link
encrypted with an authenticated key; any other connection's read, write
or subscription gets ATT's insufficient authentication, and an Android
phone then pairs: it shows a dialog for a code, and the node draws a
random six-digit one on its screen at once, as `PAIR CODE`. The host,
TrouBLE (a copy, `vendor/README.md`), runs LE Secure Connections with
the node displaying and the phone typing. When the pairing completes and
bonds, the node keeps the bond, the phone's identity address, its
identity resolving key and the long-term key, as a 64-byte record of two
slots of the stored-state page (`src/store.rs`, `bark_link::bond`), and
forgets any other: one phone at a time, and a new pairing replaces the
last. A pairing that is not authenticated or does not bond leaves the
bond as it was and its link is closed. The screen says `PHONE PAIRED`
from then on, at boot too, and `PHONE NONE` on a node that never paired.
The controller is built with LE privacy, as the host puts the bonded
phone in its resolving list.

The log says `ble: pairing, the code shown`, then `ble: paired; the bond
replaces any before it` or `ble: pairing failed ..`, and at boot `ble:
bond kept` for a node with one. A phone whose bond the node replaced
asks to encrypt with its old key as soon as it connects; the node has no
such key, and the host ends the link, authentication failure (bench,
2026-11-06: `ble: disconnected Connection Terminated By Local Host` half a
second after `ble: connected`); the app says the node is no longer the
person's.

Pairing costs 55.8 KB of flash, about 52 KB of it the host's pairing
with its P-256 and AES in software and 4 KB the node's own code, the
host's central pairing, which the node never uses, left out of the
copy; and 1.8 KB of static RAM, which leaves the stack 35 KB. How much
of it the P-256 arithmetic takes in the phone link's task is for the
bench's `stack_used` to show (`docs/pairing-2026-11-06.md`).

## Mailbox

A node set as a person's home (spec "Mailbox"; Home on the phone link)
keeps every message for its application in a ring in flash, the region
`MAILBOX` (`bark-image/src/layout.rs`): a page of state, then 16 pages
of 16 slots of 256 bytes, the oldest page erased when the ring is full
(`bark_app_layer::flash_ring`, `src/mailbox.rs`). Its phone's Inbox reads
from the ring, and a fetch from the person's phone elsewhere is answered
with the items, sealed again, through the core's one application send
slot, the phone's own message first (`src/node.rs`, `LinkApp::take`).

The mailbox runs in microseconds and never waits on the flash: its
writes and erases are queued and done by a task of their own through
the MPSL's flash timeslots, the flash shared with the store, each op a
timeslot (an erase 85 ms, a slot 2.6 ms). A message the core delivered
is confirmed to the core only once its slot is written, so that a
reboot meanwhile loses none: the core keeps it and delivers it again. A
cut write leaves a slot that fails its check, and a cut state record
the one before it, as the store's does; the host tests cut every write
of a run at every eighth byte (`bark-app-layer`, `flash_ring` tests).
While the node is nobody's home nothing changes: its inbox is the
core's, read and confirmed by the phone as before.

The cost (release, with a place, against `a3d9fbe`): 15 688 B of flash,
374 012 B of the 401 408 B slot; 2 880 B of static RAM, the mailbox
with its queue of eight ops; the flash task's deepest chain 1 064 B, the
others unchanged within 1.2 KB, the radio's 9 392 B now the deepest,
and the main task's 6 344 B at boot, where it opens the ring.

## Tasks and the stack

In the node image every long-lived part of the firmware is an embassy
task of its own, spawned by the main task, which sets up the chip, the
board, USB and the log, spawns, and ends: the USB device, the log, the
bootloader touch, the watchdog, the LED and crystal, the MPSL, the
phone link, the radio and the core on it, the store, the mailbox's
flash writes, the GPS, the screen and the user button. A task's future lies in its own static storage, and the
executor polls one task at a time, so the stack needs only the deepest
task's poll and the interrupts above it, not the sum of everything
joined into one future. Futures stay what a task waits on for a while,
a read raced against a timer. The console keeps its one-shot structure,
its radio, LED and picture joined in the main task, which holds no
relay; its USB device, log, touch and watchdog are tasks as in the node.

Measured with `llvm-objdump` (the release image with a place): the
largest single frames, and the deepest chain of calls the disassembly
shows from each task's poll, an upper bound since calls through
pointers are not followed.

| | before (`5248d65`) | after |
|---|---|---|
| main task's frame | 29 312 B | 1 372 B |
| main task's deepest chain | 38 840 B | 2 476 B |
| phone link's deepest chain | in the main task's | 10 352 B (its own frame 8 384 B) |
| radio and core's deepest chain | in the main task's | 9 344 B (its own frame 2 224 B) |
| screen, GPS, store | 880, 760, 516 B | 880, 760, 516 B |
| any other task | in the main task's | under 400 B |

The main task's 29 KB frame was mostly the relay's set-up at boot: the
neighbour table, the heard table, the delivery queue and the inbox,
each built and assigned, passed through it, 32 KB together; they are
now emptied in place (`Relay::reset`, `docs/core-interface.md` §6). So
the stack's high-water mark should fall from the 37.0 KB painted
(`stack_used`) to about 11 KB, the phone link's chain and the executor's
200 B, with the interrupts' frames above it, of the 41.7 KB the stack
has.

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
therefore powered whenever the radio is; its WAKEUP line (P1.02) keeps it
in standby when the core does not want it (`src/gps.rs`).

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
| `cad <k>` | one CAD on k alone (8 symbols, the driver's fixed setting), nothing read inside it: the CAD flag by itself, against `lbt` | `busy`, `prepare_us`, `cad_us` (SetCAD to its result read and cleared), `rest_us` |
| `lbt <k>` | one listen-before-talk on k: the CAD with the RSSI read inside it, then home (roadmap D88) | `busy`, `dbm` (on the footing of a reading while listening), `inside_dbm` (as read), `prepare_us`, `read_us`, `cad_us`, `after_us` (a reading after the CAD, if the self-check chose that), `rest_us`, `total_us` |
| `tcxo` | the oscillator's delay in use and how it got there | `delay_us`, `short_us`, `step_us`, `probes`, `errors` (starts in use that set the flag), `cold_starts`, the last probe's `last_probe_us`, `last_ok`, `last_at_s`, `last_temp_qc` (die quarter degrees), `lbt_read` |
| `probe` | probe the oscillator's start now, two steps below the delay in use, as the hourly re-probe does | `took_us`, then the `tcxo` line |
| `rssi <k> [dwell_us]` | retune to k, wait, read RSSI once, retune home | `dbm`, `retune_us`, `dwell_us`, `read_us` |
| `sweep` | one RSSI reading on each of the 13 channels | 13 values, `total_us` |
| `noise <k> <n> [gap_us]` | n readings (up to 1000) on k | `min`, `p10`, `median`, `max`, `total_us` |
| `power <dbm>` | transmit power, clamped to −9..10 dBm | the power in effect |
| `preamble <short\|long\|n>` | preamble of `tx` and `beacon` frames: 12 symbols, the 927 of a frame to a sleepy node, or n between | `symbols`, the airtime of a heartbeat, 13 bytes, and of a 255-byte frame |
| `beacon <k> <ms> [len]` | send a bench frame (default 18 bytes, an ACK's length) on k every ms; refused below a 10 % duty cycle | one line per beacon |
| `stop` | stop the beacon | |
| `ping <k>` | send an 18-byte ping on k, wait 1 s at home for the pong | `rtt_us` from TxDone to the pong, against the 100 ms ACK timeout (spec "Hand-over") |
| `status [reset]` | node ID, uptime, battery mV, channel, power, counters, the radio's `resets`, `configured` (times the whole configuration was sent), `cold_starts` and `tcxo_us`; `reset` zeroes the counters | |
| `survey [n]` | a survey build's noise log, the newest n minutes (default all), oldest first | the survey's channel and progress, then per minute `t_s`, `p10` and `median`, 13 values each |
| `stage <header hex>` | start staging an image (Updates) | `version`, `length`, `sha256`, or why it is refused |
| `chunk <offset> <hex>` | write up to 256 bytes of it, at the offset the last chunk ended | `next` |
| `commit` | check it, record it for the loader and ask for the swap | `version`, `length`, or why it is refused |
| `reboot` | reset, after the log has reached the host | |

Between commands the radio listens on the home channel and prints every
frame it hears, CRC failures included. A board that receives a ping on its
home channel answers it like an ACK (spec "The ACK"): listen-before-talk
on the sender's home channel (named in the ping), then an 18-byte pong,
then back home; it prints the time of each part.

Bench frames start with a heartbeat's first byte on channel 15, which no
node sends and every node rejects (spec "Packet formats"): byte 0 is
`0x8F`, byte 1 the format version as in every packet, byte 2 the kind
(0 test or beacon, 1 ping, 2 pong, 3 survey), byte 3 the kind's
argument, the sender's home channel or for a survey frame its transmit
power in dBm as a signed byte, bytes 4 to 7 a big-endian sequence
number, then `byte[i] = i`. RSSI, packet RSSI and SNR are whole dB (the
chip's half and quarter steps rounded down).

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

The radio is driven in two layers (roadmap D88). `bark-driver/src/sx1262.rs`,
in the host workspace and tested there, keeps what the chip holds and sends
an operation only what it changes: the settings that never change go once,
a retune is standby, frequency and receive, a listen-before-talk is the
CAD's preparation and sampling with the RSSI read inside it, and the whole
configuration goes again after a reset, a sleep or an error, when the chip
may not hold it. `src/radio.rs` is the chip on this board, one command at a
time: power-up, the pins, the SPI bus, and the encoding of each command
from `lora-phy`, copied into `vendor/lora-phy` from revision `7a9c2ec` of
the lora-rs `main` branch (2026-10-06; roadmap D4, D20); `vendor/README.md`
lists what differs from upstream, the one-command methods among it, and how
to update it.

The bus is driven blocking inside the asynchronous device: a command is a
few bytes at 8 MHz, and the interrupt and executor wake of a DMA transfer
cost 35 µs more on each, a third of every command on the bench. The pins
are the firmware's own board interface (`radio::Pins`) rather than the
driver's generic SX126x one: the generic one polls BUSY immediately after
each command, but BUSY may take up to 600 ns to rise (DS §8.3.1), so the
poll can see the previous low and send the next command too early. Ours
waits out the rise first, bounds every BUSY wait (15 ms, twice a
calibration plus the longest oscillator delay the search allows) and
bounds the wait for DIO1 in every operation, so a radio that does not
answer gives an error, after which the chip is reset and configured again,
instead of a hung console.

Between operations the radio waits in STDBY_XOSC, with its oscillator
running, and transmit and receive end there (SetRxTxFallbackMode). The
SX1262 powers the T-Echo's TCXO from DIO3 in every mode but sleep and
STDBY_RC, and on each start it waits out a programmed delay in full, with
no ready signal; a delay too short sets XOSC_START_ERR and lets the chip
run on a clock still settling, which costs a few dB at the start of a
frame, and a delay of zero hangs it (DS §13.3.6,
`docs/bench-2026-10-30.md`). From STDBY_XOSC no start is needed: the chip
reaches RX in 62 µs and TX in 105 µs (DS Table 8-2). A CAD ends in
STDBY_RC whatever the driver asks (DS Table 13-73), so every
listen-before-talk restarts the oscillator; restarted within a
millisecond of stopping it is still running and needs no delay, so that
restart is programmed at 100 µs (`bark_driver::tcxo::SHORT`). Every other
start is cold and takes a delay the board probes for itself
(`bark_driver::tcxo`): at boot a binary search over 0.25 to 4 ms in
0.25 ms steps, each probe a standby with the oscillator off for 2 ms, the
delay programmed, the errors cleared, a receive and the error word read;
four more probes at the step found, since the flag flickers at the edge;
and one step of margin. In use the flag is read after every cold start and
the delay stepped up at once when it is set, which covers a board getting
cold; and once an hour at most, when the processor's die temperature has
moved 3 °C since the delay settled, a probe two steps below it brings it
down a step if that passes, which recovers the margin as it warms. The
console's `tcxo` and `probe` show it; the node's 10 s `driver` line
carries `tcxo_us` and `tcxo_errors`.

A listen-before-talk reads the channel's RSSI four symbol periods into the
CAD, where the bench found the reading settled; the datasheet documents
`GetRssiInst` for receive only, so at boot the radio takes five pairs of
readings on its home channel, one inside a CAD and one while listening,
and keeps the median difference as the offset it takes off every reading
inside a CAD. If a pair has no reading inside, or the offset is beyond
12 dB, twice the largest the bench saw, the reading is taken after the CAD
instead, with the receiver started on the channel and settled, 1.1 ms more.
The choice and the offset are logged at boot (`radio up`, `lbt reading`).

Waiting in STDBY_XOSC costs 0.8 mA against 0.6 mA in STDBY_RC (DS Table
3-5), plus the TCXO's own current, nominally 1.5 mA (DS §4.1.4), which it
also draws in RX (4.6 mA, DS Table 3-5). The radio listens whenever it is
not transmitting or checking a channel, so it spends only the moments
between operations in standby and the extra current does not matter.

A sleepy node's radio receives duty-cycled whenever its core allows it
(`Action::Sniff`, `docs/core-interface.md` §3 rule 10): SetRxDutyCycle
(DS §13.1.7) with a 3.75 ms window (240 steps of 15.625 µs) and a
233.625 ms sleep (14 952 steps), `bark_proto::sniff`; a board whose probed
delay is above the 0.5 ms the cycle is derived for takes the excess off
its sleep and half of it onto its window (`sniff::periods`), so that the
gap between its windows, and so the preamble that reaches it, is the same
as every other node's. The command is sent in
STDBY_RC, where the datasheet starts the cycle, with the probed oscillator
delay, since each wake is a cold start, and the receive timer stops at a
header rather than at a preamble, so that after a preamble the chip waits
`2 × rx + sleep` for the header and no longer (DS §13.1.6, §13.1.7;
`vendor/README.md`). In each sleep the chip keeps its configuration and
its 64 kHz clock and powers the TCXO down: DIO3 supplies it only in
STDBY_XOSC, FS, TX and RX (DS §13.3.6). Each wake costs about 1 ms to
restore its state and the TCXO start (DS Figures 13-2 and 13-3), 1.5 ms
at the delay the cycle is derived for (`bark_proto::sniff::SNIFF_WAKE`,
roadmap D92). The receiver gain register is kept on
the chip's retention list, with the transmit modulation errata bit, so
that the cycle's own wakes restore them. After a frame the chip stops in
standby, and the driver starts the cycle again once the core has the
frame, sending the whole configuration first, since the chip slept; any
command to the chip is preceded by a falling NSS edge, which wakes it, and
the error word is read for a start error in the cycle's wakes.

The radio's expected current in the cycle, from the datasheet: the window
in receive at 4.6 mA (DS Table 3-5, DC-DC, LoRa 125 kHz, power-saving
gain) and the TCXO at its nominal 1.5 mA (DS §4.1.4), 6.1 mA; the 1.5 ms
of each wake, for which the datasheet gives no figure, between STDBY_RC's
0.6 mA with the TCXO, 2.1 mA, and the full 6.1 mA; the sleep at 1.2 µA
(DS Table 3-5, configuration kept and the 64 kHz clock on). Over the
238.875 ms cycle:

```
upper: (5.25 × 6.1 + 233.625 × 0.0012) / 238.875 = 0.135 mA
lower: (3.75 × 6.1 + 1.5 × 2.1 + 233.625 × 0.0012) / 238.875 = 0.110 mA
```

against 6.1 mA listening, 45 to 55 times less. The core's own figure,
`sniff::SNIFF_CURRENT_NA`, is 0.129 mA, from 5.8 mA for receive and wake.
`scripts/measure.md` step 5 measures it.

The probed delay is paid at power-up, after a calibration, at each of the
cycle's wakes, and after a CAD only if more than a millisecond passed
before the next command; the short one after every other CAD, inside the
return to listening (the console's `lbt`, `rest_us`).

## The current on battery

On battery the node is sleepy, and its current is what matters.
Between the core's wakeups and the radio's interrupts nothing runs: the
driver's loop waits on one embassy timer (the core's next wakeup, the end
of a sniff or of a hold), DIO1 through GPIOTE, an application that
has nothing to send and a change of USB power, and the embassy executor
sleeps in `wfe` meanwhile. An application that says it is ready and then
gives nothing is not waited on again until the timer or DIO1 wakes the
loop (`docs/core-interface.md` §4), and the log says `app said ready but
offered nothing`, at most once a minute. The USB device waits for VBUS and the log for
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

- The GPS's standby: 20 µA with WAKEUP low (L76K Hardware Design,
  Table 2), if the pin's polarity is as taken (`src/gps.rs`); its cost
  per day in each state is in `docs/gps-time-2026-11-05.md`.
- The battery divider, about 12 µA all the time (`board.rs`, Battery).
- The nRF52840's regulator runs as an LDO (embassy's default); the DC-DC
  needs an inductor the T-Echo may or may not have.
- The e-paper's controller is in deep sleep after the boot picture and
  the external flash is not touched; their idle current is unknown.

The radio's part is the cycle above, 0.11 to 0.14 mA, plus its beats and
the windows after them (spec "Beats"); without an anchor each beat is a
249 ms frame with the long preamble at the transmit current, every 34.6 s
on average (`bark_proto::sniff`). No figure for the whole board is claimed here;
`scripts/measure.md` step 5 measures it in each state.

## Risks

- **The oscillator's start on a cold board is unmeasured.** The search
  at boot and the step up on every start error in use follow it wherever
  it goes, up to 4 ms; until the flag clears, the chip runs on a clock
  still settling for a few dB at the start of each frame (roadmap D88).
- **The reading inside a CAD is undocumented.** The boot self-check
  guards it on one channel at one moment; a chip whose reading inside a
  CAD drifts from its reading in receive later would mis-set the busy
  verdict by the drift (roadmap D88).
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

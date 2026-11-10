# bark-fw

Firmware for the LILYGO T-Echo (nRF52840 + SX1262, 433 MHz build). Its own
Cargo workspace, cross-compiled for `thumbv7em-none-eabihf` (roadmap D7).

Current state (2026-11-09). It runs on four boards on the bench and is
released as `bark-fw-<F>`, F the firmware's version in a release, which
stays when only the app changed (`README.md`, "Releases";
Verified and unverified, below). It starts behind a loader of its own,
which swaps in a staged image, from the phone, the mesh or the console,
and swaps it back if it does not confirm itself (Updates, below). The
firmware blinks the green LED at 1 Hz while on USB, logs over USB CDC-ACM (a banner with the firmware version
and constants from `bark-proto`, the previous boot's panic message if
there was one, an uptime line every 10 s while on USB), shows the
board's node ID on the e-paper (Display, below), powers up the
SX1262 at bark's modulation (SF5, 125 kHz, CR 4/5, CRC on, private sync
word, the receiver set for the 927-symbol long preamble) and then does
one of two things, chosen when it is built:

| Build | Runs |
|---|---|
| `cargo build --release` | the protocol core: a relay on USB, and on battery what its motion and its battery make it |
| `cargo build --release --features console` | the measurement console (below) |
| `BARK_SURVEY=<k> cargo build --release --features console` | the console as a survey on channel k, for a board left away from the host (Survey build, below) |

Every board runs the same node. Its role is not a setting: the firmware
reads whether USB power is present from the nRF52840's VBUS detection
(Product Specification, POWER, USB supply: `USBREGSTATUS.VBUSDETECT`,
and the `USBDETECTED` and `USBREMOVED` events) and reports it to the core
at boot and at every change (`AppCommand::Power`, `src/power.rs`). On
battery it reads the cell once a minute between radio operations
(`AppCommand::Battery`, `src/battery.rs`). The core is a relay on USB; on
battery a carried one turns sleepy at once and a fixed one when its
battery is low (spec "Sleepy nodes"; roadmap D152). A person at the
board may force the other role by holding the user button 3 s, until
USB is next plugged in or pulled (`AppCommand::ForceRole`; Display,
below). For bench work,
`BARK_ON_BATTERY=1` at build time reports battery whatever USB says and
leaves the battery levels off, so that a board on the bench's USB runs
sleepy and still logs (`scripts/measure.md` step 5).

`BARK_IGNORE=<id>` at build time, twelve hex digits as the banner prints a
node ID, makes a bench build deaf to that node: the driver drops each of
its frames, heartbeat, DATA or ACK, as if the radio had not heard it
(`bark_driver::BENCH_DEAF_TO`). Boards on one desk all hear each other;
two built deaf to each other, with a third between, are a line for the
routing tests (`docs/bringup-2026-11-06.md`). The radio still receives
those frames, so they still keep it busy and still make its
listen-before-talk wait, which a line's ends would not. The node logs
`bench build: deaf to ..` after its `node id=` line. Neither flag is for
a board that leaves the bench; off, each costs nothing (`size`).

`BARK_OPEN=1` at build time leaves the chip's debug port open, for the
bench. Without it the image closes the port at its first boot, for good
(Read-out protection, below), so that no image leaves the bench open by
mistake; any other value fails the build. `scripts/bench.py` sets it.

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
has no place until the phone sets a map point (roadmap D83). A built
place wins over the place the board stored (`src/store.rs`) at every
boot, and the board boots fixed there, whatever it stored: a board
rebuilt with another place stands at the new one, and boards built in a
line stay a line.

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

On the boards, four T-Echo 433 on one desk; each measurement, what was
expected and what was seen, is in the bench list of `docs/roadmap.md`,
Phase 2:

- The radio at bark's modulation: transmit, receive, channel activity
  detection and RSSI, and their timings on the lean driver
  (`docs/bench-2026-10-30.md`, `docs/lean-driver-2026-10-30.md`);
  capture of one frame over another and a still link's fading
  (`docs/bench-capture-2026-11-03.md`, `docs/bench-fading-2026-11-04.md`).
- USB logging, the bootloader from the serial port's 1200-baud touch,
  the external flash's ID and its bus at about 1 Mbit/s, and GPS fixes
  indoors at a window (`docs/gps-bench-2026-11-06.md`).
- The loader and the swap: images staged from the console swap in and
  confirm; one that panics is swapped back and its panic message logged
  at the next boot; a reset at a random moment in each of 75 swaps, and
  at each of 19 chosen moments, always left one whole image running, the
  one expected (roadmap D143).
- Pairing and the phone link with the app, and an update from the phone
  across a lost link, under the loader of roadmap D148, the image then
  on the 32.768 kHz crystal (2026-11-07).
- An update over the mesh from one board to another, the gatherer once
  left unattended; version 11 of 471 544 bytes was whole an hour after
  it began and swapped in at a quiet moment (2026-11-09).
- On the bench since 2026-11-09, with what each is to show in the bench
  list: the neighbour status and its retries (roadmap D157, D158,
  D161), the code on the screen, the touch pad and the button (D160),
  an image gathered over the mesh that hears no one (D159), and whole
  log lines.

Verified on the host:

- `cargo build --release` and `cargo clippy -- -D warnings` pass with
  each build, plain, with `--features console` and with `BARK_OPEN=1`.
- The ELF's first loadable byte (the vector table) is at `0x06000`, the
  initial stack pointer is `0x20040000`. The open node image of version
  16 is 481 464 B of flash (`.vector_table`, `.text`, `.rodata` and
  `.data`) of the 929 792 B slot, and uses 149 592 B of RAM before the
  stack (`.data` and `.bss`), which leaves the stack 111 528 B
  (2026-11-09 with roadmap D159, `size -A`, `scripts/uf2.py --stack`). The relay,
  95 016 B, is in `.bss`: its static holds zero bytes, which the
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
- The loader builds to 8 948 B at `0x01000` (roadmap D148), and
  `cargo clippy --release -- -D warnings` passes in `../bark-loader`.
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

- The bootloader writes a UF2 block of `0xFF` bytes as an erased page,
  so that a copy clears a swap asked for before it (Build and package).
- The loader's write protection: after the jump, an erase of its pages
  or the bootloader's by the firmware raises a bus fault (nRF52840
  Product Specification, ACL).
- The watchdog: started by the loader for a trial, it resets a firmware
  that never confirms, or hangs, after 600 s.
- An image gathered over the mesh that hears no node resets itself
  585.9 s after its start, and the image back gives it up (roadmap D159;
  the revert in Updates, below); on the host only so far.
- Whether USB enumerates when the cable comes after a boot on battery,
  the 32 MHz crystal then asked of the MPSL (`src/main.rs`,
  `src/ble.rs`).
- That a second phone's pairing replaces the first's bond, and the first
  phone's reads then fail (Phone link, below; roadmap D135).
- VBUS detection: `USBREGSTATUS.VBUSDETECT` reads the cable, and the
  POWER interrupt comes at each change with the USB driver's handler
  bound beside the node's, so that the core is told within an event's
  time and the node turns a relay on the cable at once.
- The green LED is on P1.01 (the board's label, 2026-10-30) and active low.
- The stored state: the NVMC, through the MPSL's flash timeslots, writes each record the core asks to keep into
  the next slot of the `STORE` page and erases the page when it is full,
  and the next boot logs `stored Some(..)` and comes back on the same home
  channel (`src/store.rs`, roadmap D73).
- The external flash drops from standby to deep power-down, 12 µA to
  0.5 µA typical (`src/ext_flash.rs`,
  `docs/external-flash-2026-11-06.md`); `scripts/measure.md` step 5 sees
  it or not.
- The chip's behaviour beyond what the datasheet says, beyond what
  `docs/bench-2026-10-30.md` saw: CRC-failed frames
  can be read back after the error, continuous receive keeps running after
  a header or CRC error, `GetRssiInst` answers during a CAD as the bench
  saw (the boot self-check guards it, Radio driver below), the
  configuration sent once holds across every operation but a sleep, and a
  delay of 100 µs restarts the oscillator cleanly within a millisecond of
  a CAD's end (roadmap D88).
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
- The GPS's hot start after standby (`src/gps.rs`,
  `docs/gps-time-2026-11-05.md`, "On the bench").
- The display (Display, below): BUSY falls after the reset, the software
  reset and the refresh, the picture comes out black on white the right
  way up (`ROTATION` in `src/display.rs` turns it if not), and the
  controller's deep sleep keeps it on the glass.

## Toolchain

Stable Rust and the Cortex-M4F target:

```
rustup target add thumbv7em-none-eabihf
```

Python 3.9 or later for the scripts, with the standard library alone but
for signing an image, which takes the `cryptography` package
(`apt install python3-cryptography`; Updates, below). No probe, objcopy or
cargo-binutils is needed.

## Flash layout

The T-Echo ships with a UF2 bootloader and keeps it (roadmap D6). With
the SoftDevice it came with erased, it jumps to `0x01000`, the MBR's end
(roadmap D129, `docs/softdevice-2026-11-05.md`). There sits a loader of our own, `../bark-loader`,
which starts the firmware from the slot above it and can swap in a
staged image (Updates, below; roadmap D79). One table,
`bark-image/src/layout.rs`, gives every region; the build scripts of the
firmware and of the loader write each one's `memory.x` from it, with a
symbol for each internal region, which `scripts/uf2.py` reads from the
ELF. Pages are 4 KiB, the erase unit (nRF52840 Product Specification,
NVMC).
The bootloader's drive writes a UF2 into `0x1000..0xEA000` and, after a
double tap, serves every byte of that range as `CURRENT.UF2`
(`docs/softdevice-2026-11-05.md`, "How the drive places a UF2"): anyone
holding the board can copy it off. So the secrets, the mailbox's state
and the stored state, lie in `0xEA000..0xF4000`, which no UF2 writes or
reads.

What is large and not secret, the staged image, the update's coded
pieces and the mailbox's ring, lies on the board's 2 MiB external flash
(`docs/external-flash-2026-11-06.md`), and what runs, boots or is secret
stays inside (roadmap D139, `docs/flash-plan-2026-11-06.md`, section 1).
Inside:

| Region | Start | Size | Why |
|---|---|---|---|
| MBR | `0x00000` | 4 KiB | the chip's first code; the bootloader's linker script |
| loader | `0x01000` | 16 KiB, 4 pages | where the bootloader jumps with no SoftDevice; 8.6 KiB as built, the fourth page room to grow, since the layout cannot move once boards are in the field; a UF2 with the loader erases what is left of the SoftDevice up to `0x26000` |
| loader state | `0x05000` | 4 KiB | the swap's request, progress and confirmation (embassy-boot's state page: 2 + 4 words a slot page, 910 of its 1024); a UF2 writes it erased |
| firmware, running | `0x06000` | 908 KiB, 227 pages | the CPU runs from internal flash; everything up to the record |
| staged image's record | `0xE9000` | 4 KiB | its header, and the loader's word that it matched it; inside, since it holds the hash the firmware checked before it staged, and the last page the bootloader's drive writes, so every UF2 writes it erased |
| given up | `0xEA000` | 4 KiB | the names of the images the node swapped back, 128, which it never gathers again (Updates, below); outside every UF2, so a copy over USB keeps them |
| spare | `0xEB000` | 28 KiB, 7 pages | unused, outside every UF2: for secrets to come |
| mailbox state | `0xF2000` | 4 KiB | the mailbox key, the last fetch and the phone's last confirmed item; secret, so above `0xEA000` |
| stored state | `0xF3000` | 4 KiB | the node's stored state (`src/store.rs`, roadmap D73), where it was; with it the phone's UTC offset and the bond's keys with the paired phone; secret, so above `0xEA000` |
| bootloader | `0xF4000` | 38 KiB | the bootloader's linker script |
| bootloader config | `0xFD800` | 2 KiB | |
| MBR parameters | `0xFE000` | 4 KiB | |
| bootloader settings | `0xFF000` | 4 KiB | |

On the external flash, in sectors of 4 KiB:

| Region | Start | Size | Why |
|---|---|---|---|
| firmware, staged | `0x000000` | 912 KiB, 228 sectors | one sector larger than the running slot, which embassy-boot's swap needs; after a swap it holds the previous image |
| coded pieces | `0x0E4000` | 772 KiB, 193 sectors | a head of what has been gathered and a lane of four sectors, 64 pieces of 256 bytes, for each of an image's 48 blocks at most, for the update over the mesh (`bark-image` `pieces.rs`, roadmap D151) |
| mailbox ring | `0x1A5000` | 256 KiB, 64 sectors | 1024 items, 44 days at 23 a day (`docs/app-layer-2026-11-05.md`, section 5; Mailbox, below); sealed to the phone, so not secret |
| carried | `0x1E5000` | 44 KiB, 11 sectors | what the node still holds when it reboots into an image gathered over the mesh, 167 messages at most, a page each, offered again after the reboot (Updates, below) |
| free | `0x1F0000` | 64 KiB, 16 sectors | |

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
and one codegen unit: the open node image of version 14 is 478 704 B of
the 929 792 B slot, 451 088 B free (2026-11-09, without a place); the
pairing of roadmap D135 took 55.8 KB of it
(`docs/flash-2026-11-06.md` measures the options,
`docs/pairing-2026-11-06.md` the pairing's). The core's slowest
step shows on the board as `longest_handle_us` in the 10 s line.

The image is the same wherever the repository is checked out. Source
paths end up in it, a panic's location among them, and absolute ones
would make its size depend on the checkout's path: a build from a path
18 characters longer once took 168 B more flash. So Cargo runs rustc
through `scripts/rustc-remap.sh` (`build.rustc-wrapper` in
`.cargo/config.toml`, the loader's too), which cuts each path to the
crate's directory and the file in it: `bark-proto/src/wire.rs` for the
checkout, `aes-0.8.4/src/soft/fixslice32.rs` for Cargo's registry,
`core/src/slice/index.rs` for the standard library, and `src/main.rs`
for the firmware's own. A panic still names its crate, file and line,
and the prefixes cost no flash: 3 120 B less than mapping them to
`/bark`, `/cargo/registry/src/<index>` and `/rustc/<commit>/library`.
The build commands above need nothing more, here and in the workflows. A wrapper changed after a
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
   with the image, its version and the board's node ID (`id=`), its
   `protection=` line (Read-out protection, below), then
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

A board with the loader of the layout before roadmap D139, whose staged
image lay inside, takes `bark.uf2` once too: an old loader with a new
firmware would swap the wrong regions. The mailbox's state, key
included, stays where it was; its ring starts empty on the external
flash, and the items of the old one, inside, are gone.

## Read-out protection

A probe on the SWD pads reads the whole internal flash, keys and all,
unless the chip's access port protection is on (roadmap D138;
`docs/approtect-2026-11-06.md` has the facts and their sources). First
thing at every boot, the firmware (`src/protect.rs`) reads the chip's
build code from `FICR.INFO.VARIANT` and does what the build asks:

| Build | Build code D and earlier | Build code F (revision 3) and later |
|---|---|---|
| protected, the default | UICR.APPROTECT `Enabled` | UICR.APPROTECT `Enabled`; `APPROTECT.FORCEPROTECT` at every boot |
| `BARK_OPEN=1` | nothing: erased UICR is open | UICR.APPROTECT `HwDisabled`; `APPROTECT.DISABLE` at every boot |

A UICR value takes effect at the next reset, so after writing one the
firmware resets the chip at once. A write only clears bits, so `Enabled`
can never be written back: a board a protected image has run on stays
closed under any image, and only a probe's erase of the whole chip opens
it, which also erases the board's bootloader and every key
(`docs/approtect-2026-11-06.md`, "Recovery"). The log's line after the
banner, and the `boot:` line, say `protection=on` or `off`, the build
code and what UICR.APPROTECT holds.

The bench loses only the pads, which it has no probe for: a closed board
takes UF2s as before, since the board's bootloader runs on the chip and
writes its flash through the NVMC, which the protection does not touch.
For the same reason the protection does not keep the keys from someone
who copies an image of their own to the board's drive.

## Updates

A new image is staged on the external flash by the running firmware,
swapped in by the loader at the next boot, and kept only if it confirms
itself (roadmap D79, D139, `docs/mesh-update-2026-10-25.md`). The firmware's side is
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
| 48–242 | three signatures of 65 bytes: a byte naming the key, 1 to 255, then an Ed25519 signature (RFC 8032) of bytes 0–47; all zero if empty |
| 243–255 | zero |

The firmware's version is `BARK_IMAGE_VERSION` at build time,
kept under its own symbol, from which `scripts/image.py`
writes the header's; an image is staged only above the running one's
version. Without the variable the version is 0: a bench build, flashed
by UF2, which any staged image replaces and which can itself never be
staged over another, since `scripts/image.py` packs only a version
above 0. The hash is computed with the `sha2` crate (0.10.9), the
firmware's over what it wrote to flash, the loader's again before the
swap.

**Signatures.** The running firmware stages an image only if one of
the header's signatures verifies under one of the up to three public
keys it was built with, the one its byte names (roadmap D144;
`bark-image/src/sign.rs`, checked with the `ed25519-compact` crate,
2.6.0). The loader checks none (D79): no key sits in the one part
that is never updated. A signature covers bytes 0–47, the hash
standing for the image: the firmware hashes the staged bytes anyway, so
it checks a signature over 48 bytes, not the whole image, and checks
it as the staging starts, before a byte of the image has come; the hash
once the last byte has. A header is refused with the reason:
`Signature(Unsigned)`, `Signature(UnknownKey)` when no signature is by a
key the firmware holds, or `Signature(Bad)`.

The version and the signature together stop a captured image from
being staged again: it is signed, but at or below the running version.
A bench build, version 0, takes any signed version.

**The loader**, at every boot:

1. With no swap asked for, it changes nothing, and leaves the external
   flash unpowered.
2. With one asked for, it raises the switched rail, waits 10 ms for it
   and 70 µs for the chip (tVSL), and reads the chip's ID
   (`bark-image/src/ext_flash.rs`). A chip that does not answer `BA 60 15`
   cancels a swap not yet started; one under way waits for a boot that
   finds the chip.
3. It checks the staged slot's first `length` bytes against the record's
   SHA-256. If they differ, or the record holds no header, it cancels the
   swap and the running firmware boots, which logs `update: the loader
   refused the staged image`. If they match, it notes it in the record,
   so that a swap a power cut stopped half way, whose slot no longer
   matches anything, goes on without the check.
4. It runs embassy-boot's `prepare_boot`, which swaps the slots, goes on
   with a swap that was stopped, or swaps them back if the new image did
   not mark itself booted. It copies a page at a time into a page or
   sector whose content is held in another, and writes a progress word in
   the state page once each copy is whole, so a power cut anywhere is
   taken up from the copy it stopped (embassy-boot's documentation of
   `prepare_boot`, "SWAPPING" and "REVERTING", read at version 0.7.0).
   The previous image ends on the external flash, ready for the swap
   back. Each of the 227 pages costs an internal erase of 85 ms and 1024
   word writes of 41 µs (nRF52840 Product Specification, NVMC), an
   external erase of 10 ms and 16 page programs of 1.3 ms (ZD25WQ16B
   datasheet, Table 4-3), and 8 KiB over the external flash's bus, about
   30 ms at the 2 to 3 Mbit/s its software clock makes by its
   instructions: about 190 ms a page, 44 s, and 3 s more for the hash; a
   swap back as long without the hash. Unmeasured; the mailbox's open
   logs the bus's speed (Mailbox, below). Then the rail falls again.
5. On those boots it first starts the 32.768 kHz clock as the firmware
   does, on the crystal if it starts within 1 s (`bark_image::lfclk`),
   since the watchdog holds the clock on its source until a reset
   (roadmap D148); then the watchdog, with a period of
   600 s, `bark_image::TRIAL_SECONDS` (Product Specification, WDT: the
   timeout is (CRV + 1) / 32 768 s), counting while the CPU sleeps, and
   feeds it at every flash operation of the swap.
6. It write-protects `0x00000..0x05000` (the MBR and itself) and `0xF4000..0x100000` (the bootloader, its settings and the
   MBR's parameters) through two ACL regions, until the next reset
   (Product Specification, ACL).
7. It jumps to the firmware at `0x06000`.

**The firmware** at its start, once its core is set up and its radio is
up (`update::started`, `bark_image::boot::start`), reads in the record
how the image came (spec "Updates over the mesh", the install; roadmap
D159). An image a person staged, from the phone or over USB, confirms
itself at once. An image gathered over the mesh, whose record has the
word `FROM_MESH` after the bench loader's, logs `update: version N on
trial: it confirms once it hears a node, within 585 s`, and confirms
only at the core's `AppEvent::Heard`, the first frame from another node
that decodes, through the MPSL's flash timeslots (`update::trial`),
logging `update: version N confirmed, a node heard S s after its
start`; with none 585.9 s after its start
(`bark_proto::update::heard_within`) it logs `heard no node within 585
s` and resets itself. From the confirmation on it feeds the watchdog
twice a period, since a started watchdog cannot be stopped before a
reset (Product Specification, WDT); not before, so the loader's 600 s
still ends a trial. An image that hangs before then is reset by the
watchdog; one that panics resets itself through `panic-persist`, and the
watchdog runs on through that soft reset (Product Specification, POWER,
reset behaviour); either way the loader swaps the previous image back,
which logs `update: the new image did not confirm itself` and, before
it confirms itself, gives the failed image up: it writes its name, from
the header the swap back left in the record, into the given-up page and
logs `update: version N given up: never gathered again`
(`bark_image::given_up`). The mesh then never gathers an image of that
name, and so never serves it; a later release is another name.

Each update erases each page of the running slot once, twice if it is
swapped back, and the state page twice: the internal flash's 10 000
erase cycles (Product Specification, NVMC) last 5 000 updates. Each
sector of the staged slot is erased twice, in staging and in the swap,
three times with a swap back: the external flash's 100 000 (datasheet,
"Features") last 33 000.

**Staging** takes three paths, the same steps on each
(`bark_image::boot::Stager`): the phone link and the mesh, in the node
build, and the serial port, in a console build. Each begins with the
checks of `update::stage`, the version and the header's signature
against the keys the image holds, commits through `update::commit`,
which checks that every byte came, the battery (below), the hash,
records the header and asks for the swap, and reboots through
`update::reboot`, so that a check added there applies to all. `scripts/image.py pack` makes the image
either takes and `scripts/image.py sign` signs it; a release carries it,
signed, as `bark-fw-<F>.img`, and writes its header into the record of
both its UF2s (`uf2.py --header`, Over the mesh, below), so that a node
flashed with a release holds it and serves it. F is the release's
number N when the firmware changed since the previous release, and the
previous release's F, its files carried on unchanged, when it did not
(`README.md`, "Releases"), so nodes gather a new image only for a new
firmware.

From the phone (spec "Phone link", Updates; roadmap D145): the app
writes the header to Update, the image to Image in pieces of 240 bytes
without a response, reads Update after every seven and goes on from the
bytes the node holds, then commits and reboots (`bark-app/README.md`,
"Updating a node"). The node stages through the MPSL's flash timeslots
for the record and the state page, as the store writes, and through the
external flash's driver with the executor free while the chip erases or
programs, as the mailbox writes; the commit's hash reads 256 bytes at a
time and lets the executor run between, about 3 s for the whole image.
What is staged stays across connections until the node resets. The log
has a line for each change, none for a piece:

```
update: receiving version=8 length=394112 from the phone
update: link closed with 120960 bytes of version 8 held
update: staged version=8, all 394112 bytes held; commit next
update: committed version=8 length=394112: the loader swaps it in at the next boot
update: reboot asked by the phone
ble: disconnected 0x16
reboot
```

and a refusal as `update: begin refused=Version { running: 8, staged: 8 }`,
`update: begin refused=Signature(Unsigned)`,
`update: commit refused=BatteryLow usb=false battery_mv=3702`, or the
like.

From a console build, over its serial port: `stage <header hex>`,
`chunk <offset> <hex>` for up to 256 bytes at a time, in order,
`commit` and `reboot`. `scripts/image.py` packs, signs and sends it;
an open build holds the bench key as well, below:

```
BARK_OPEN=1 BARK_IMAGE_VERSION=2 cargo build --release --features console
scripts/image.py pack target/thumbv7em-none-eabihf/release/bark-fw target/v2.img
scripts/image.py sign target/v2.img keys/bench.pem --key-id 255
BARK_OPEN=1 BARK_IMAGE_VERSION=1 cargo build --release --features console
scripts/uf2.py --loader ../bark-loader/target/thumbv7em-none-eabihf/release/bark-loader target/thumbv7em-none-eabihf/release/bark-fw target/bark.uf2
# copy target/bark.uf2 to the board, then:
scripts/image.py stage target/v2.img /dev/ttyACM0 --reboot
```

The log then shows `image version 2` and `update: version 2 confirmed`.
`scripts/image.py check target/v2.img ELF` says whether the firmware
built as ELF would stage the image, and `stage --running ELF` asks it
first; `scripts/uf2.py` prints the keys a firmware holds.
`BARK_UPDATE_BENCH` at build time makes the images that must be swapped
back: `unconfirmed` runs and never confirms, `panic` panics where it
would confirm, `hang` spins there with everything else stopped; and
`erase-loader` tries to erase the loader's first page, which the write
protection must stop with a bus fault (should it get through, the board's
bootloader finds nothing to start and waits for a UF2, so the test costs
at most a copy). `deaf` drops every frame it receives on the boot that
swapped it in, and on no other: gathered over the mesh it hears no node
and resets itself 585.9 s after its start, to be swapped back and given
up; staged from the phone it confirms and stays deaf until a reset, for
the app's warning; flashed by UF2 it hears, so a board holds and serves
it. Every image logs the two protected regions as the ACL
reads them back, `update: acl 0 start=0x0 len=0x5000 write=Disable`
and `update: acl 1 start=0xf4000 len=0xc000 write=Disable`.

On battery at or below the low level of spec "Sleepy nodes" a commit,
and a reboot with a swap asked for, are refused
(spec "Implementation notes"): the console says `commit refused=BatteryLow battery_mv=...
low_mv=3741`, and over its USB cable it is never on battery; the phone
link answers refused, battery, and the node keeps the image staged, to
commit once it is on USB or charged. The node reads its battery as the
core does, calibrated; the console, without a core, uncalibrated.

**Over the mesh** (spec "Updates over the mesh"; roadmap D151, D153):
`src/update/mesh.rs`, a task of its own, which starts once the image has
confirmed itself.

- *What it holds.* The record keeps the header of the image swapped in.
  If that header is the running version's and the active slot hashes to
  it, the node holds the image: it makes block 0, the table, from the
  slot, tells the core (`Holds`) and serves its pieces. A UF2 made
  without `scripts/uf2.py --header` (below) leaves no header, and a
  swap back leaves the newer image's, so such a node serves nothing until it takes an image over the mesh or the phone.
- *What it gathers.* When a neighbour's heartbeat names a newer image,
  the node gathers it if the version is above its own, the key the
  heartbeat names is one it holds, and neither the phone's update nor a
  swap holds the staged slot (`bark_image::policy::takes`). The pieces go
  into the store on the external flash (`bark_image::pieces`), one flash
  operation or one piece read a step, the executor free between and the
  chip's erases and programs waited out with a timer. A reset keeps what
  was gathered, and the node goes on with it unless it is no longer
  newer than the running image.
- *What it serves.* A piece of the running image is coded from the
  slot's bytes, memory-mapped in the internal flash; one of a block it
  gathered and decoded, from the external flash. A piece not filled
  within 372.8 ms, the longest a check of the channel may be held
  (`docs/core-interface.md` §3 rule 8), is not sent: it would go on an
  older check than the core allows for. The task fills pieces between
  the store's steps, so one asked for while a whole image is checked and
  staged, about 3 s, is not sent. The 10 s line counts them,
  `unserved=`.
- *The swap.* Once every block is decoded, the header's version and
  signature are checked as `update::stage` does, the slot is hashed
  against the header, the record written and the swap asked for
  (`Stager::written`, `update::commit`). The node then drains (spec
  "Send queue", "Updates over the mesh"; roadmap D170): while its power
  allows a swap and no phone update is under way, it asks the core to
  take no message, `update: draining for the reboot: the node takes no
  message`, and takes them again, `update: taking messages again until
  the reboot may come`, once either fails. It reboots at the first quiet
  moment (`bark_image::policy::quiet`): the core drained
  (`Relay::drained`: nothing in its send queue, its delivery queue or
  its inbox, or 600 s since it began), the mailbox's writes done, and
  no exchange with a phone half done (`ble::doing`: a write not yet
  answered or its reply not yet sent, a notification not yet sent, a
  pairing). What the core still holds then goes into the carried region
  of the external flash, `carry: id=... kept across the reboot` for
  each, and after the reboot back to the core as the node's own, one
  at a time ahead of the phone's message and the mailbox's replies,
  `carry: id=... offered again`, the region erased once all have gone
  (`src/carry.rs`). A phone that stays linked does not hold the reboot
  back: the node ends every link first, `ble: disconnected 0x16 slot N`
  for each, and waits for them to end, at most the longest supervision
  timeout, 32 s; the app looks for its node and connects again once it
  is back (`bark-app/README.md`, "Screens"), so for the minute the swap
  takes its person sees what a node out of reach shows. The loader's
  trial follows as from
  the phone, but the image confirms itself only once it hears a node,
  and is swapped back and given up if it hears none within 585.9 s
  (The firmware, above). An offer of an image given up is logged
  `not taken, given up`.
- *With the phone's update.* A phone's begin gives up an image the node
  gathers, and is refused, Busy, while a gathered image is being staged
  or is staged; the mesh takes no image while the phone's is staged or
  committed.

The 10 s line ends in `update=`: `idle`, `holds:8 serving:B:N` (the last
block served and its pieces so far), `gathering:8 blocks:K/B`,
`installing:8` or `staged:8`.

A bench of two boards: A holds version 8, B runs version 7, or 0, a UF2
bench build, which takes any. A holds an image only with its header in
the record, as a swap into it leaves it: from the phone, the console's
`stage`, or, without staging, `scripts/uf2.py --header`, which writes
the header of the packed, signed image into the record of the UF2 that
carries the same build, with the word that says the loader checked it,
and refuses an image of another version or hash. It combines with
`--loader`, and the release's UF2s are made this way:

```
BARK_OPEN=1 BARK_IMAGE_VERSION=8 cargo build --release
scripts/image.py pack target/thumbv7em-none-eabihf/release/bark-fw target/v8.img
scripts/image.py sign target/v8.img keys/bench.pem --key-id 255
scripts/uf2.py --header target/v8.img target/thumbv7em-none-eabihf/release/bark-fw target/v8.uf2
```

A board given `v8.uf2` boots with no swap asked for and logs `update:
holds version=8`; a UF2 without `--header` leaves the record erased, and
the board serves nothing. Both hold the key image 8 is signed with: the project's, ids
1 and 2, or, in open builds, the bench key, id 255 (roadmap D154). Image
8 of 471 064 bytes travels in 31 blocks, the last of 53 pieces:

```
A: update: holds version=8 blocks=31 name=[..] key=255
B: update: offered version=8 blocks=31 key=255 name=[..]: taken
B: update: gathering version=8 blocks=31 (Started)
A: update: serving block=0 piece=113 (1 of this block)
B: update: block 0 decoded and written, 1 of 31
B: update: block 7 failed its check; gathered again
B: update: version=8 length=471064 whole and checked: the loader swaps it in at the next boot
B: update: draining for the reboot: the node takes no message
B: update: rebooting into version 8 at a quiet moment
B: reboot
B: update: version 8 confirmed
B: update: holds version=8 blocks=31 name=[..] key=255
```

A full block takes at least a holder's 80 pieces, its 64 and the loss
margin, at one a gap of 4.16 s on an idle channel: 5.5 minutes, 18.5 at
the pace of 13.86 s (spec "Updates over the mesh"). The 31 blocks take
about 2 390 pieces, 2.8 hours at the gap and 9.2 at the pace, and each
block's decode about 8.6 s of reads more. A line `update: piece ... not
filled; not sent` or a rising `unserved=` says the holder could not fill
a piece in time; `block N failed its check` more than now and then, that
pieces arrive wrong.

The revert, on the same two boards: A holds version 9 built deaf, and B,
running version 8, gathers it (roadmap Phase 2, "An image that hears no
one"):

```
BARK_OPEN=1 BARK_IMAGE_VERSION=9 BARK_UPDATE_BENCH=deaf cargo build --release
scripts/image.py pack target/thumbv7em-none-eabihf/release/bark-fw target/v9.img
scripts/image.py sign target/v9.img keys/bench.pem --key-id 255
scripts/uf2.py --header target/v9.img target/thumbv7em-none-eabihf/release/bark-fw target/v9.uf2
```

A, given `v9.uf2`, boots with no swap and hears as any node. B gathers
version 9 and reboots into it:

```
B: update: version 9 on trial: it confirms once it hears a node, within 585 s
B: bench image: deaf to every frame until the next reset
B: update: version 9 heard no node within 585 s; resetting so that the loader swaps the previous image back
B: reboot
B: update: the new image did not confirm itself; version 8 is back
B: update: version 9 given up: never gathered again
B: heard a node
B: update: offered version=9 blocks=31 key=255 name=[..]: not taken, given up
```

The last line comes again after a reset, and after a UF2 of version 8
copied over USB, which leaves the given-up page. A build of version 10
is taken as usual.

**A bench loader**, built with `BARK_LOADER_BENCH=reset`, resets the
chip once in each swap and once in each swap back, at a moment drawn
from the RNG evenly over the time the two flashes spend erasing and
writing (`bark-image/src/boot/cut.rs`, roadmap D143). The operation the
moment falls in is done only in part, as a power cut would leave it, the
loader notes the moment and where it fell in the record and resets, and
the next boot ends the swap. The firmware logs them at its start, as
`update: bench reset in the swap at 20117355 us: internal erase of page
117, 43.5 % through`. The build fails without `BARK_OPEN=1`, and
`scripts/uf2.py` packs such a loader only with an open firmware.
`scripts/swapbench.py ID 200` builds one, copies it with a console image
of version 1 to the board through its bootloader, and runs 200 rounds:
in turn a console image one version up, to confirm, and a `panic` image
one version up, to be swapped back (roadmap Phase 2, "Resets in a
swap").

The same loader stops a phase at a chosen moment instead, when the
record names one: the console's `cut swap N` or `cut back N`, after
`commit` and before `reboot`, writes it, and `none` stops nothing, where
the loader would otherwise draw. N is the index of an operation in the
phase's erases and writes on both flashes at the boot that starts the
phase, each page of an erase one: the swap's 1 363, the record's word
that notes the image checked and then six a page, a copy out to the
external flash and a copy in, each an erase, a write and a progress word
in the state page; the swap back's 1 365, six a page, then the state
page's word that marks the progress invalid, its erase and its word
(`bark_image::boot::cut::operations`). The host test that cuts the power
at every operation counts the same way, and its test
`a_chosen_moment_falls_in_the_operation_the_power_cut_of_its_index_takes`
checks, for every N of both phases, that the loader stops the operation
that test's cut N takes. The loader does that operation half way, as the
host test's cut does, and logs it as `update: bench reset in the swap at
operation 4: internal erase of page 232, 50.0 % through`; where it fell,
written before the reset, keeps the next boot from stopping it again.
`scripts/cutbench.py ID` copies the bench loader and version 1 as
`swapbench.py` does, then walks 19 moments once each
(`scripts/cutbench.py --list` prints them): around each progress word of
each phase's first and last pair of copies, the operation before it and
the one after, and the operations outside the copies. A moment in the
swap takes a console image one version up, which must confirm; one in
the swap back a `panic` image, which must leave the previous version
back. A round also fails unless the log names the operation expected,
its flash, erase or write, and page, and the other phase's moment as not
reached (roadmap Phase 2, "Resets at chosen moments of a swap").

**Keys.** A firmware holds up to three public keys, each under an id,
a byte from 1 to 255 that names the key in a signature, and each 32
bytes (`build.rs`); they are kept under their own symbol, from which
the scripts read them. Every build holds the project's keys,
`keys/project.pub`, one `id:hex` a line, so that a node takes the
project's releases whichever build it runs (roadmap D154). An open
build holds the bench key, `keys/bench.pub`, id 255, as well: its
private key is beside it in `keys/bench.pem` for every bench to sign
with, and a protected build fails if given it. A fork that signs its
own images gives its public keys at build time in `BARK_SIGNING_KEYS`,
`id:hex` comma-separated, in place of the project's.

The release key is made once with `scripts/image.py keygen`, which
writes the private key, PEM, readable by its owner alone, and prints
the public key. The private key is kept outside the repository, in the
same folder as the app's release key and backed up the same way
(README.md, "Releases"): a node can take an image only from a key it
holds, so with the key lost, every node is updated by hand at its next
visit. Its public key goes in `keys/project.pub`, and the release
workflow signs with it from the secret `BARK_SIGNING_KEY`:

```
scripts/image.py keygen bark-signing-1.pem   # in the folder of the app's key
gh secret set BARK_SIGNING_KEY < bark-signing-1.pem
echo 1:<the public key it printed> >> keys/project.pub
```

The workflow signs under the id the built firmware gives the key and
fails if it holds no such key, so no release leaves the nodes unable to
take the next one. To replace a key: make key 2, release once holding
both in `keys/project.pub`, still signed with key 1, which the nodes
hold; then set the secret to key 2 and remove key 1 from the file. Once every
node runs a release holding key 2, key 1 signs nothing they take. A
stolen key is replaced the same way, at once: until the nodes run a
release without it, its holder can sign for them. A lost key cannot
sign the release that brings in the next; a second key held from the
start, kept apart, can.

## Display

The 1.54" e-paper (200 × 200, SSD1681 controller, `src/display.rs`)
says what a person needs to tell boards apart and to see how a node is
doing without a phone. The text comes from a 5 × 7 bitmap font of the
digits, the capital letters and a few signs, scaled by whole pixels.

The console and survey images draw one picture at boot: the node ID as
twelve hex digits, then the image's name, `CONSOLE` or `SURVEY`, and its
version as `V` and `BARK_IMAGE_VERSION`, a survey's with its channel as
`CH`.

The node image shows a QR code of the project's page,
`zond.github.io/bark` (`site/`, published by
`.github/workflows/pages.yml`), with a small orb web in its centre and
the web's threads in the margin, and under it the node's ID, or the
pairing code while a phone pairs (spec "Screen and buttons", roadmap
D160). `bark-screen/src/qr.rs` draws it from the modules
`bark-screen/scripts/qr.py generate` wrote, which a host test checks
against the address encoded again; another host test decodes the drawn
picture, and `qr.py check` decodes it with two more decoders and writes
it as `target/bark-screen/code.png`:

```
python3 -m venv .venv
.venv/bin/pip install qrcodegen zxing-cpp opencv-python-headless pillow
.venv/bin/python bark-screen/scripts/qr.py check
```

A touch of the pad (P0.11, `src/button.rs`) shows the node's state for
30 s, or until the next touch, then the code again; a pairing code
shows the lines by itself for as long as it shows
(spec "Screen and buttons"). The state is built
from what its core says after each step (`src/node.rs`, `screen`) by
`bark-screen`, which is tested on the host. Sixteen characters to a
line, every line from the left margin. A header says what the node is, as a word or two at the left,
and a gap sets it apart; every line below it is a label at the left and
its value flush right. Top to bottom:

| line | says | from |
|---|---|---|
| role | `FIXED RELAY`, `CARRIED RELAY`, `RELAY NO PLACE` (before its first place), `SLEEPY ANCHORED` or `SLEEPY NO ANCHOR` | the core's sleepy, anchorless, placed and carried flags |
| `STARTING` | while the first rotation runs, under the role in the header | booting |
| `ID BAE6E6CBF4F5` | twelve hex digits, as in the log's `id=` and the USB serial number | the chip |
| image | `NODE V12` | `BARK_IMAGE_VERSION` |
| `GET V14 14/31` | while it gathers an image over the mesh: its version, the blocks held from the first of all its blocks; no line otherwise | the core's `Status::gathering` (spec "Neighbour status") |
| phone | `PHONE UNPAIRED` before anyone has paired (the node is public all the same), then `PHONE PUBLIC` or, once its owner has made it theirs alone, `PHONE PRIVATE`; while a phone pairs, `PAIR CODE 012345` | the phone link (Phone link, below) |
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
120-byte entry (`docs/core-interface.md` §6), and would count those two as well; it is
left out while the display is its only reader, since on a quiet network
neither comes before a take does.

*Why the percentage is "about".* The node knows three open-circuit
voltages of a typical cell: 20 %, 30 % and full (Maxim AN4189, the core's
battery levels). It reads the charge off the straight line between them
(`bark_proto::node::battery_percent`), which is close at the low end
and a coarse guess between 30 % and full, where a cell's curve is not
straight. At or below 20 % it says `LOW`: the curve falls steeply there,
and a fixed relay turns sleepy.

*Why the uptime.* A picture stays on the glass when the node stops.
The lines are drawn at the touch that asks for them, so their uptime is
of that moment; a touch that brings no lines within a few seconds means
the node is not running.

*When it redraws.* At boot, the code, once the node's loop has run (at
most 5 s). After that, a change of picture, a touch or the 30 s ending,
and a new pairing code are drawn at once, since a person stands at the
node for each (spec "Phone link", Pairing); a pairing code turns the
screen to the lines, and the pairing's end back to the code unless a
touch's 30 s still run. On the code nothing else changes. While the
lines show, a change a person acts on is drawn one refresh after it,
2 s (the board's label): the header (the role, or `STARTING`
ending), the rest of the phone's line, and the power, USB or battery.
Changes within those 2 s, such as the power and the role of a sleepy
node when the cable goes in, are drawn in one picture, and a change
undone within them is not drawn. The neighbour counts and the battery's
level wait for the next touch. Nothing else is drawn. Each picture is
drawn to its end before the next is considered, so none starts while a
refresh runs. `bark_screen::schedule` decides, tested on the host.

*The buttons.* Held 3 s, the user button (P1.10, `src/button.rs`)
flips the role (spec "Nodes", "Screen and buttons") at the moment the
3 s have passed, while still held; once the core has taken it, within
milliseconds, the lines are drawn at once with the new role. The
release does nothing. A shorter press, let go before the 3 s, flips the
picture as a touch of the pad does, debounced as the pad, 40 ms: the
bench found a board whose pad gives no edge. A press of the reset button resets
the chip into the image it ran; a double tap within half a second
enters the bootloader instead (Flash, above), which the screen does not
say. The lines label both at their left edge, in 1-pixel text on the
margin nearer each, after an arrow pointing at the button: `REBOOT`
at the top, and at the bottom the role a hold would give, `SLEEP (3s)`
or `WAKE (3s)`. The screen says nothing of a forced role. The ten lines
start 10 px down to make room.

*The pad.* A TTP223 touch sensor on the chip's own supply drives P0.11
both ways (`docs/touch-pad-2026-11-09.md`), so the pin has no pull. The
idle level differed between boards on the bench, so the firmware takes
the level at boot, when nobody touches, as idle, logs it (`pad: idle
low`), and counts a change away from it lasting 40 ms as a touch, then
waits for 40 ms back at idle before the next. An open build logs every
edge (`pad: edge, High`).

The cost on battery: a full refresh holds BUSY high for 2 s (the
board's label). The panel's current during a refresh is not in the
repository's notes, nor its datasheet; taking 10 mA, an assumption to
be replaced by a measurement with `scripts/measure.md`, a refresh is
20 mAs, 5.6 µAh, of the 850 mAh cell (spec "Hardware") one part in
150 000. The screen draws when a person acts, two pictures a touch, and
for a pairing; twenty touches a day cost 40 × 5.6 µAh = 224 µAh a day,
0.009 mA on average, under 1.5 % of a sleepy node's 0.65 mA
(`docs/battery-role-2026-10-25.md`). A node nobody touches draws its
code once at boot and then nothing. On USB the energy is the charger's.
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

The link (`src/ble.rs`, spec "Phone link") is the paired phone's
(roadmap D135), and on a public node, as every node is unless its owner
makes it private, also its guests' (below). Every characteristic of the service needs a link
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
bond as it was and its link is closed. The phone whose bond the node
keeps pairs anew with a code like any other once it has forgotten the
node, the log first saying `ble: a phone the node holds a bond with pairs
again`; a pairing that fails leaves the old bond in place. The screen says `PHONE UNPAIRED`
until a phone has paired, then `PHONE PUBLIC`, or `PHONE PRIVATE` on a
node its owner has made private.
The controller is built with LE privacy, as the host puts the bonded
phone in its resolving list.

The log says `ble: pairing, the code shown`, in an open build with the
code, `ble: pairing, the code shown: 123456`, so that the bench pairs a
phone without reading the screen, and never in a protected build, whose
cable must not stand in for being at the node; then `ble: paired; the bond
replaces any before it` or `ble: pairing failed ..`, and at boot `ble:
bond kept` for a node with one; each names the bond's phone by its
identity address, kind and an IRK fingerprint, and each connection says
whether the peer is the bond's (`the bond's=Some(true)`). Only an
authenticated pairing that bonds replaces the kept bond
(`bark_link::bond::after_pairing`). A phone whose node's bond names
another phone, as the log then shows, is no longer the owner and pairs
again; c367's bond came to name another phone on 2026-11-08, when its
owner paired a second phone with it while it ran on battery with no
USB log, and the first phone paired again. A disconnect's reason, and a host error
that carries an HCI or ATT error, is logged as that code in hex
(Bluetooth Core, Vol 1, Part F, and Vol 3, Part F, 3.4.1.1): `ble:
disconnected 0x13` when the phone ends the link, `0x16` when the node
does; other host errors by name. A phone whose bond the node replaced
asks to encrypt with its old key as soon as it connects; the node has no
such key, and the host ends the link, authentication failure (bench,
2026-11-06: `ble: disconnected 0x16`, terminated by the node, half a
second after `ble: connected`); the app says the node is no longer the
person's.

Pairing costs 55.8 KB of flash, about 52 KB of it the host's pairing
with its P-256 and AES in software and 4 KB the node's own code, the
host's central pairing, which the node never uses, left out of the
copy; and 1.8 KB of static RAM, which leaves the stack 35 KB. How much
of it the P-256 arithmetic takes in the phone link's task is for the
bench's `stack_used` to show (`docs/pairing-2026-11-06.md`).

Neighbours (link version 4) gives the phone the neighbour table a page
at a time. The node's task copies the live entries into a static by
slot after each step (`src/node.rs`, `publish_neighbours`), and the BLE
task builds the page the phone reads from that copy, each age counted
from when the entry was last heard, so it reads nothing of the core.
The copy starts zeroed, apart from `LINK`, so that it lies in `.bss`
and not in flash. It costs 2.5 KB of flash and 2.9 KB of static RAM,
taken from the stack (2026-11-07, `llvm-size`).

The node builds the GAP and GATT services itself (`src/ble.rs`,
`server`), as the host's builder leaves the GATT service empty: Service
Changed and the database hash, which the host computes once the table is
whole (spec "Phone link", Changes to the table). The bond's record keeps
the link version the phone last saw and its subscription to Service
Changed. At boot the log says `ble: bond kept, table seen at version 4,
service changed false` or the like; on the bond's encrypted link, while
the phone is subscribed and saw another version, `ble: service changed:
the phone saw version 4, this is 5` and, once the phone confirms,
`ble: service changed confirmed`. It costs 4 KB of flash and 0.4 KB of
static RAM (2026-11-07, `llvm-size`).

Update and Image (link version 6) stage a new image from the phone
(Updates, above). The BLE task serves them itself and keeps the staging
across connections (`src/update/phone.rs`); it awaits each piece's
writes before it takes the next request, so the pieces wait in the
host's queue of 8 meanwhile, which the app's window of seven and its
read fill. The pieces and the reads of Update are not logged as ATT
requests, as the rest are, and a piece reads none of the values the
node publishes. A reboot asked for answers, ends the link, waits for it
to close, at most the 32 s of the longest supervision timeout, and
resets. They cost 14.8 KB of flash and 1.4 KB of static RAM, the
SHA-256 among it, which the node build did not have before
(2026-11-07, `llvm-size`, an open build).

Recipient and Public (link version 7) serve guests on a public node
(spec "Phone link", Public; roadmap D150). A node is public unless the
paired phone writes 0 to Public, private; the bond's record keeps that
in its byte 14, so a node never paired, and a bond kept before, is
public, and it keeps the paired phone's recipient tag, which it writes
to Recipient at every connection. The host takes 2 links, the owner's
and one guest's at a time, each served by a task of its own (`slot`),
so that its future lies in static storage and the stack holds one poll
at a time; one slot advertises at a time, and on a node that is not
public only while no link is up. A guest's link ends 30 s after it
began, and a second guest's as soon as it writes Recipient while one is
up. Node, Status, Send, Inbox, Given back, Recipient and Guest have no
permission in the attribute table, as the host cannot tell a public node
from another, and `allowed` refuses them on a link that is not the
bond's while the node is not public, as the host refused them before.
The node's loop puts every message for its application on a public
node into the ring (`src/node.rs`, `Placing`), a guest's written as
sent, and confirms it to the core once it is written; the mailbox tells
the owner's from the guests' by tag (`bark_app_layer::mailbox::Owner`),
and a guest's read of Inbox asks the node for the oldest item after the
number it wrote to Guest (`for_tag`). Whose turn it is at the one
message the node holds, and so whose Ready is set, is `Turn` in
`src/ble.rs`. The
console's `status` ends in `public=true` or `public=false`, read from
the bond in the stored-state page.

The paired phone unpairs by writing 2 to Public (link version 12): the
node answers, removes the bond from the host, writes a forgotten bond's
record (`bark_link::bond::forgotten`, byte 14 at 2, which an older image
skips, keeping the bond before it), follows it as a node never paired,
public with the screen's `PHONE UNPAIRED`, and closes the link.

The log says `ble: link 3 in slot 1` as a link comes, `ble: recipient
5a slot 1 owner false` as a phone writes its tag, `ble: public false`, `ble: unpaired by the owner; the bond is
forgotten and the node is public`,
`public: id=... tag=Some(5a) kept` for each message kept on a public
node, `ble: a second guest in slot 1, closed`, and `ble: the guest in
slot 1 closed after its time`.

Ask (link version 9, the image gathered from 10) asks a neighbour for
its status (spec "Neighbour status", "Phone link", Ask; roadmap D157,
D158). The phone's write of an ID goes to the core as
`bark_driver::Request::Ask`; the core sends the ask, and the answer
comes back as `AppEvent::Status`, which the node keeps as Ask's value
and notifies to the paired phone (`told_status` in `src/ble.rs`). The
log says `ble: ask [..]` for the write and `status of [..]: Status {
.. }` for each answer.

A link beyond the first costs 5.6 KB of static RAM, taken from the
stack; a third, as first built, left the stack under its floor once the
mesh spread's tables came, and two boards faulted at boot (Tasks and the
stack, below).

## Mailbox

A node set as a person's home (spec "Mailbox"; Home on the phone link)
keeps every message for its application in a ring on the external flash,
64 sectors of 16 slots of 256 bytes, the oldest sector erased when the
ring is full, and its state in a page inside, since it holds the mailbox
key (`bark-image/src/layout.rs`; `bark_app_layer::flash_ring`,
`src/mailbox.rs`). Its phone's Inbox reads
from the ring, and a fetch from the person's phone elsewhere is answered
with a batch of the items not yet sent, sealed again, through the core's
one application send slot, the phone's own message first (`src/node.rs`,
`LinkApp::take`). An item sent, or confirmed by the phone at home, is
marked by clearing one byte of its slot, a write without an
erase; it stays in the ring.

The mailbox reads a slot when asked, about a millisecond on the
external flash's bus, and never waits on a write: its writes and erases
are queued and done by a task of their own, the ring's on the external
flash with the executor free while the chip erases (10 ms) or programs
a slot (1.3 ms), the state's through the MPSL's flash timeslots, the
flash shared with the store, each op a timeslot (an erase 85 ms, a
record 1.3 ms). A read that meets an erase under way waits for it, up
to 12 ms; the mailbox's users all run in thread mode, so that wait
masks no interrupt. At boot the mailbox opens once the radio has raised
the rail: it reads all 1024 slots, 256 KiB, and logs how long that
took, `mailbox: ... 1024 slots read in N ms`, which gives the bus's
speed; about a second by its instructions. The same read counts the
items not yet sent. A message the core delivered
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
flash writes, the update over the mesh, the GPS, the screen, the user button and the touch pad. A task's future lies in its own static storage, and the
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

**The floor.** The stack is the RAM static memory leaves, and nothing
else bounds it: an image whose statics grow past it faults at boot, as
two boards did on 2026-11-08, with 14.6 KiB left, after the public
node's links and the mesh spread's tables came together. The most a
node image has used is what the boards painted (`stack_used`) over a
day, 2026-11-08: 16 564 B on version 4 (`42dd68f`), 15 412 B on version
0, against a deepest poll of 9.5 KiB by the disassembly, which does not
follow calls through pointers or count the interrupts. So the stack
keeps at least 24 KiB: the measured 16.2 KiB and 6 KiB for what an image
since may add, 22.2 KiB, rounded up. `scripts/uf2.py` refuses to pack an
image that leaves less, from its `__sheap` and `_stack_start`, and
`uf2.py --stack` only checks, which CI runs on the release build. A
measurement above it raises the floor.

To get there, the phone link keeps no copy of what the node holds: a
read of Neighbours or of Inbox asks the node, which writes the page or
the message into one buffer as the phone reads it (`ble::READ`), where
the link kept the whole neighbour table, 2.6 KB, and two messages; the
host keeps no room for L2CAP's connection-oriented channels, which the
node never opens; the update's header is kept once, not in every link's
task; and the mailbox queues 6 flash ops, twice the 3 a message or a
confirmation takes, not 8.

| node image, release, open, version 6 | data + bss | stack |
|---|---|---|
| merged with 3 links (`4fdcf58`) | 246 136 B | 14 984 B |
| 2 links | 240 520 B | 20 576 B |
| 2 links, no copies (above) | 236 280 B | 24 832 B |
| one guest's link, its mail in the ring | 236 472 B | 24 640 B |
| console | 16 264 B | 244 840 B |

Since the send queue holds 64 of its hop places at once (roadmap D155),
the relay is 95 600 B smaller: the open release image of version 14
left the stack 112 008 B, and with the trial of roadmap D159 version 16
leaves it 111 528 B (2026-11-09, `uf2.py --stack`).

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
| `status [reset]` | node ID, uptime, battery mV, channel, power, counters, the radio's `resets`, `configured` (times the whole configuration was sent), `cold_starts` and `tcxo_us`, and whether the node is public, from the bond; `reset` zeroes the counters | |
| `survey [n]` | a survey build's noise log, the newest n minutes (default all), oldest first | the survey's channel and progress, then per minute `t_s`, `p10` and `median`, 13 values each |
| `stage <header hex>` | start staging an image (Updates) | `version`, `length`, `sha256`, or why it is refused |
| `chunk <offset> <hex>` | write up to 256 bytes of it, at the offset the last chunk ended | `next` |
| `commit` | check it, record it for the loader and ask for the swap | `version`, `length`, or why it is refused |
| `reboot` | reset, after the log has reached the host | |
| `cut <swap\|back> <op\|none>` | an open build's alone: where a bench loader stops the swap just committed, or its swap back, an operation's index or none (Updates) | the command, or why it is refused |

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
radio parameters. `scripts/bench.py` builds, flashes and logs the boards of
steps 4 and 5 from the host and reads their logs
(`docs/bringup-2026-11-06.md`).

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
sleeps in `wfe` meanwhile. The application wakes the loop only for
something the driver takes (`docs/core-interface.md` §4): a phone's
write it serves itself, such as the clock, it answers without waking it,
and a power event that changed nothing it ignores. A readiness that gives
nothing anyway is a fault: it is not waited on again until the timer or
DIO1 wakes the loop, the 10 s line counts it in `empty_readies`, and the
log says `app said ready but offered nothing`, at most once a minute. The
USB device waits for VBUS and the log for
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
- The e-paper's controller is in deep sleep after the boot picture; its
  idle current is unknown.

The external flash sits in standby at 12 µA unless told otherwise, so
the node puts it in deep power-down, 0.5 µA typical, once the radio has
raised the rail that feeds it, and wakes it only for a read or a write
(`src/ext_flash.rs`).

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
Lines logged before that wait in a 2 KiB ring (the banner is always
there); once it fills, newer lines are dropped until the port opens.

```
picocom /dev/ttyACM0          # exit with C-a C-x
# or
screen /dev/ttyACM0           # exit with C-a k
# or, read-only
cat /dev/ttyACM0
```

Linux raises DTR when a tty is opened, so all three work, and the kernel
hands over bytes only while something holds the port open: whatever opens
it first takes the waiting boot lines, so a `stty` run before the reader
takes them into nothing. Set the line and read it in one open, as in
`( stty 115200 raw -echo <&3; cat <&3 ) 3<>/dev/ttyACM0`. Until `-echo`
takes hold, the kernel echoes what the board sends back into its console,
so a port opened just as the board boots can leave stray text on the
console's line: type a bare Enter before the first command.
`scripts/image.py stage` does so itself. Your user needs to be in the
`dialout` group (or `uucp`, depending on the distribution).

Each line is `[seconds.millis LEVEL] message`, seconds since boot; command
echoes are `> command`.

A line reaches the host whole or not at all (`src/usb_log.rs`,
`bark_driver::record`). The logger formats each record into one line
buffer, then copies it into the ring only if the ring has room for all
of it; otherwise it drops the whole record and counts it, and once a
minute logs the count as one line, `log: N records dropped`, so a loss
is seen. Copied in pieces as it was formatted, a record would lose its
tail when the ring filled part way and the next record would join the
stump, a line missing and two merged, which `scripts/image.py stage`
cannot read. The console's echoes and answers take the same path.

The line buffer is 638 bytes, the longest line the firmware writes: the
console's report of a received frame of 255 bytes in hex, with its time
and fields. A longer line is cut and ends in `...`. It is the only static
memory the change added: the open release image of version 19 leaves the
stack 110 752 B against 111 392 B before (`uf2.py --stack`).

Nothing waits on the log, and it masks interrupts for at most one
64-byte copy. The buffer is held by a lock that masks nothing, which is
enough because every task logs from thread mode, one poll at a time; a
record logged from an interrupt, or while another record is being
formatted, is dropped and counted rather than waited for. So the record
in the buffer is the ring's only writer: the room it found only grows
while it goes in, and it goes in 64 bytes, one USB packet, at a time,
each copy inside a critical section of its own.

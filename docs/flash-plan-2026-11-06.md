# Plans for more room in the image slot (2026-11-06)

**Status:** plans; section 1 built 2026-11-06 (roadmap D139, its last
subsection). Follows `docs/flash-2026-11-06.md` (the
study), whose numbers this note uses where it says so. Measured on the
node image of `09a1895`: 390 324 B of the 401 408 B slot, 11 084 B free.

Every size below is the flash image (text + data) of `cargo build
--release` from `bark-fw/`, or of `bark-loader/` where named. A
measurement says so; anything else is an estimate with how it was made.

## 1. The external flash

### The chip

The board carries a 2 MB SPI NOR flash on the QSPI pins (`board.rs`,
`Flash`: SCK P1.14, CS P1.15, IO0 P1.12, IO1 P1.13). The factory firmware
read its JEDEC ID as `BA 60 15` (`docs/bench-2026-10-30.md`), which is
the Zetta ZD25WQ16B (its datasheet, "Table ID Definitions"), not the
MX25R1635F that the schematic and the pin research name. From the
ZD25WQ16B datasheet:

| | |
|---|---|
| size | 16 Mbit, 512 sectors of 4 KiB |
| supply | 1.65–3.6 V |
| standby current | 12 µA typ. |
| deep power-down current | 0.5 µA typ., 1.5 µA max. |
| enter deep power-down | `B9h`, done 3 µs after CS# rises (tDP) |
| leave it | `ABh`, ready 8 µs after CS# rises (tRES1) |
| page program, 256 B | 1.3 ms typ., 3 ms max. |
| sector erase, 4 KiB | 10 ms typ., 12 ms max. |
| endurance, retention | 100 000 program/erase cycles, 20 years |
| quad I/O | needs the QE bit set; with QE clear, IO2 and IO3 are WP# and HOLD#, which must be held high |

IO2 and IO3 are WP# on P0.07 and HOLD# on P0.05, with no pull-ups,
and the chip is on the switched rail, which the firmware holds up on
battery too (`docs/external-flash-2026-11-06.md`). Single-line reads and
writes need only IO0 and IO1, but WP# and HOLD# must be driven high.

Untouched, the chip sits in standby at 12 µA. The firmware now sends
`B9h` once the rail is up, 0.5 µA (item b, `bark-fw/src/ext_flash.rs`).

### The idea

Move what is large and not secret to the external chip: the staged
image, the coded pieces of the mesh update and the mailbox's ring. The
running image, the loader and its state, and every key stay inside.
The running slot then takes nearly all of the internal flash the board's
bootloader lets a UF2 write.

### What must stay inside, and why

- **The running image**: the CPU executes from internal flash. The
  QSPI's execute-in-place window is slow and read-only.
- **The loader and its state page**: the loader must run with nothing
  but the NVMC, and the state page is what a UF2 erases to cancel a
  pending swap (`scripts/uf2.py` writes it erased).
- **The staged image's record** (its length and SHA-256): the loader
  checks the staged image against it. On the external chip, anyone with
  a clip could write a matching image and record while a swap is
  pending. Inside, the record holds the hash the running firmware
  checked before it staged.
- **The stored state** (the bond's long-term and identity keys,
  `src/store.rs`) and **the mailbox's state page** (its first 32 bytes
  are the mailbox key, `bark-app-layer/src/mailbox.rs`): both secret.
  The ring's items are sealed to the phone, so the node cannot read them
  and neither can whoever reads the chip.

Two limits on "inside":

- The board's bootloader serves `CURRENT.UF2` on its drive after a
  double tap: every byte of `0x1000..0xEA000` (`ghostfat.c`, the
  `CURRENT.UF2` reader, `USER_FLASH_START` to `USER_FLASH_END`). The
  mailbox's state page today is at `0xE2000`, inside that range: **the
  mailbox key can be copied off any board by a double tap and a file
  copy.** The stored state at `0xF3000` is outside it. Secrets belong in
  `0xEA000..0xF4000`, which no UF2 writes or reads. This holds with or
  without the external chip; moving the mailbox's state page to
  `0xF2000` and its ring below it fixes it today (Recommended order).
- Nothing in the firmware or the loader sets APPROTECT, so the SWD pads
  read the whole internal flash. Inside is safer than outside only once
  read-out protection is on, which is its own decision (the chip's
  revision decides how: on later nRF52840 revisions it is on unless the
  firmware opens it at every boot; Product Specification, APPROTECT).

### The regions

Internal, 1 MiB in 256 pages:

| Region | Start | Pages | Holds |
|---|---|---|---|
| MBR | `0x00000` | 1 | as today |
| loader | `0x01000` | 4 | as today; grows by the QSPI reader, below |
| loader state | `0x05000` | 1 | as today; 2 + 4 × 227 = 910 of its 1024 words |
| firmware, running | `0x06000` | 227 | 929 792 B, against 401 408 B today |
| staged image's record | `0xE9000` | 1 | the last page a UF2 writes, so it stays erased by every UF2 as today |
| spare | `0xEA000` | 8 | outside every UF2; for secrets to come, such as the update's given-up versions |
| mailbox state | `0xF2000` | 1 | its key and counters |
| stored state | `0xF3000` | 1 | as today |
| bootloader and the rest | `0xF4000` | 12 | as today |

The running slot ends at `0xE9000`, below `0xEA000`, so a UF2 still
writes it whole (`docs/softdevice-2026-11-05.md`, "How the drive places
a UF2"). That limit is what keeps the slot at 227 pages and not 238.

External, 2 MiB in 512 sectors:

| Region | Start | Sectors | Holds |
|---|---|---|---|
| firmware, staged | `0x000000` | 228 | one sector larger than the running slot (embassy-boot) |
| coded pieces | `0x0E4000` | 22 | as the mesh update note sizes them; could be more |
| mailbox ring | `0x0FA000` | 64 | 1024 items, 44 days at 23 a day (today 16 pages, 11 days) |
| free | `0x13A000` | 198 | 792 KiB |

**Room gained**: the running slot grows from 98 to 227 pages, +129
pages, **+528 384 B**; the image would have 539 468 B free.

### The loader

embassy-boot already takes its three partitions on different flashes
(`BootLoaderConfig`'s three type parameters), and embassy-nrf's `Qspi`
implements `NorFlash`. The swap stays embassy-boot's: the running slot
inside, the staged slot outside. It copies each running page out to the
external chip and the staged page in, so the previous image stays
outside after a swap, ready for the swap back of D79, exactly as today.

The loader needs the QSPI set up for single-line reads, page program
and sector erase, and the chip woken from deep power-down and put back
before the jump. Measured: the loader with embassy-nrf's `Qspi::new`, a
read, an erase and a write added is 9 248 B against 8 744 B, +504 B. The
SHA-256 check reads the staged image through the same driver. Its 16 KiB
region holds it with room to spare. `bark-image/src/boot/tests.rs` runs
the swap on two RAM flashes with different page sizes instead of one.

**Timing**, per page of the swap, by the datasheets: the internal page
written, an erase of 85 ms and 1024 words of 41 µs, 127 ms (nRF52840
Product Specification, NVMC); the external sector written, 10 ms erase
and 16 pages of 1.3 ms, 31 ms typ., 60 ms max.; the reads, about 1 ms
at 32 MHz single-line. About 160 ms a page, 36 s for 227 pages, 43 s at
the datasheet maximums; a swap back as long. Today: 25 s for 98 pages.
embassy-boot swaps the whole slot, not just the image's length; making
it stop at the image's end would halve this, but needs a change to
embassy-boot. The 600 s trial is unaffected.

### The firmware

- Staging (`src/update.rs`) writes the staged slot through the QSPI
  instead of the MPSL's flash timeslots, and the record as today.
  External writes do not stop the CPU, so they need no timeslot and do
  not disturb the radio or BLE.
- The mailbox's `Region` (`src/mailbox.rs`) reads through the QSPI, or
  through its read-only memory map at `0x12000000` (Product
  Specification, QSPI, execute in place), and writes and erases through
  it. Its state page stays on the NVMC.
- Between uses the chip is in deep power-down and the QSPI peripheral
  deactivated, CS# held high. embassy-nrf's `Config::deep_power_down`
  has the peripheral send `B9h` and `ABh` itself around each use. A
  mailbox write wakes it for 8 µs plus 1.3 ms of program at a few mA:
  negligible next to a radio frame.
- Code: about 1 KB for the driver and the two backends, by the loader's
  measurement and the size of today's backends.

**Wear.** Each update erases each staged sector at most twice, staging
and swap: 50 000 updates. The ring at 64 sectors erases each one every
64 × 16 / 23 = 45 days. The internal running slot is erased once per
update, as today.

### What can go wrong

- The chip is on the switched rail (`docs/external-flash-2026-11-06.md`).
  The firmware keeps it up for the radio; the loader cannot count on
  it, since the regulator's enable has a pull-down and nothing drives
  `PWR_EN` from a reset until the firmware runs. The loader must raise
  `PWR_EN` and `PWR_ON` and wait for the chip before a swap.
- A bad external chip makes a swap impossible. The loader then must
  not touch the running slot: the loader checks the staged slot against
  the record's hash before any copy, so a chip that reads wrong fails
  the check and the swap is cancelled. A chip that fails
  mid-swap leaves the swap to resume at the next boot, as a power cut
  does; one that never comes back leaves the board with half an image.
  The board's bootloader still takes a UF2.
- It is a layout change: every board takes one UF2 with `--loader`.
  Before the field this costs one copy per bench board; after, a visit
  per node. It must be decided before the first field image
  (`docs/mesh-update-2026-10-25.md`, "What must be fixed before the
  first field deployment").

**Effort**: two to three days of agent work (layout, loader, staging,
mailbox backend, the host tests with two flashes, `uf2.py`), then the
bench's swap, swap back and power-cut rows of `docs/roadmap.md` again.

**Verdict**: do it, before the field. It is the one change that gives
hundreds of kilobytes, it removes the layout's hardest trade (the ring
against the update's pages against the image), and the key fixes and
the `B9h` at boot fall out of it.

### What it makes moot

From the study's "Recommended next" and "How much room":

- halving the mailbox's ring, +16 384 B: the ring moves out;
- lending the update's 22 pages, +45 056 B: they move out;
- the host's packet pool built at boot, 4 036 B of `.data`: still a
  small, safe saving, but no longer needed;
- the nightly flags (−28 312 B and more) stay unneeded.

### What was built (2026-11-06)

As above (roadmap D139), with these differences:

- **The bus.** SPI mode 0 driven by software on the four pins
  (`bark-image/src/ext_flash.rs`), not the QSPI peripheral, in the
  loader and the firmware alike. The pins then stay at the levels deep
  power-down's current needs between uses (CS#, WP#, HOLD# high, the
  clock and DI low, DO pulled down), the same code runs in both, and no
  peripheral, interrupt or memory map is shared. Its speed is 2 to 3
  Mbit/s by the loop's instructions, unmeasured: the firmware logs the
  time its mailbox takes to read its 1024 slots at boot, 256 KiB, about
  a second by that speed.
- **The firmware's writes** to the chip wait out the typical time, then
  poll every 50 µs up to the datasheet's maximum, with the executor free
  (`bark-fw/src/ext_flash.rs`). A read that meets a write under way
  waits for it, up to 12 ms for an erase; the mailbox's lock becomes a
  thread-mode one, so that wait masks no interrupt.
- **The loader** powers the rail only on the boots with a swap or a swap
  back to do, waits 10 ms and tVSL, and checks the chip's ID; without
  the chip a swap not yet started is cancelled (the firmware logs the
  refusal), and one under way waits for a boot that finds the chip. It
  lets the rail fall before the jump.
- **Sizes.** The loader is 8 820 B, not the 9 248 B measured with the
  QSPI: the driver is small, and the loader now prepares only the boots
  with a swap. The node image is 378 784 B of the 929 792 B slot,
  551 008 B free; the driver, the chip's init and the mailbox's backend
  cost 3 064 B.
- **Times**, unmeasured: per page, the internal 127 ms, the external
  erase and programs 31 ms, and 8 KiB over the bus about 30 ms: 190 ms,
  44 s for 227 pages, and 3 s for the hash before a swap; a swap back
  as long without the hash.
- **The mailbox's replies** stay at 256 a round (spec "Mailbox"): a ring
  of 1024 sends a fetch its newest 256. Whether a round should carry
  more is a protocol question left open.

## 2. Binary logging

Measured on `main` at `5bc477a` (398 628 B, before this branch's two
savings; the differences carry over):

| Build | Image | Δ |
|---|---|---|
| as built | 398 628 B | |
| every log line removed (`log`'s `max_level_off`) | 368 008 B | −30 620 |
| and a panic handler that only resets | 312 012 B | −86 616 |
| log lines removed, a panic handler that keeps file and line but formats nothing | 362 576 B | −36 052 |
| every log call rewritten as a binary logger would compile it, the panic handler as today | 374 472 B | −24 156 |

**Who logs.** Only `bark-fw` and `bark-driver`: no dependency has its
`log` or `defmt` feature on (`cargo tree -i log`). 98 call sites, 159
arguments, 35 of them `{:?}`. The format strings are only 3.4 KB; the
rest is the calls, the `fmt::Arguments` built at each, and the `Debug`
and `Display` they keep in the image.

**The panic path is not logging.** Removing the panic handler's
formatting as well saves 56 KB more, the source locations and messages
and every `Debug` an `unwrap` or `assert_eq` keeps alive (`u128`'s
1.5 KB, the MPSL's errno names, the ATT requests). Any handler that
reads its `PanicInfo` keeps most of it: file and line alone keep
10.8 KB of formatting. The panic message is the only thing a board
nobody watches tells at its next boot, so it stays, and the bound on
what binary logging saves is the 30.6 KB of the log lines. The screen's
lines (`bark_screen::Line`, the clock, the ID) keep about 3.3 KB of
integer formatting in any case.

**The realistic saving.** The rewritten build compiles each call as a
binary logger does: start a frame with the line's number, one call per
argument copying its bytes, end. That costs 6.4 KB over no logging,
about 65 B a call site, and drops 8.4 KB of `Debug` and `Display` in 73
functions. A real conversion then adds:

- binary format impls for what our lines print (`Trace`, the boot
  facts, the stored state, positions, IDs, times, the GPS's sentences;
  3.4 KB of `Debug` today) and the host's and the controller's errors
  (2.9 KB of `Debug` today, needing those crates' `defmt` features):
  estimated 2.5–4 KB, since a binary impl writes fields without names;
- the logger's runtime, frame encoding, the lock and the timestamp:
  1–2 KB, less the 0.3 KB of today's `log` glue.

So **about 19 KB (18–21 KB)**, against 14.6 KB for keeping only warnings
and errors as text (the study) and 30.6 KB for no log at all.

**The host side.** Today people read the port with `picocom`, `screen`
or `cat` (`bark-fw/README.md`, "Log"), `scripts/measure.md` logs
sessions with `picocom --logfile` and reads `key=value` lines, and
`scripts/image.py stage` matches the console build's text replies. A
binary log needs `defmt-print -e <the image's ELF>` (or our own decoder)
between the port and the reader, and the ELF of every image ever
flashed, kept by its commit and version: an ELF that does not match
decodes to nonsense, and a field board is readable only if its ELF was
kept. The console build must stay on text for `image.py`. The panic
message, a string read from RAM at boot, goes as one string argument.

**Bandwidth**: a typical line of 45 B as text is 10–12 B as frames; the
10 s driver line, about 250 B, becomes 70–90 B. The logger's 2 KiB
buffer before the port opens holds three to four times as many lines.

**Verdict**: 19 KB is real, the most that keeps every line and the
panic message, but it buys 5 KB over warnings-and-errors-only at the
cost of a decoder, an ELF archive, 98 call sites rewritten and two
logging paths (the console stays on text). With section 1 done it is
not needed. Without it, cut to warnings and errors first. **Effort**:
two days for the conversion and the archive in the release flow.

## 3. Our loader in place of the board's bootloader

**Room**: the bootloader's 38 KiB, its config page's 2 KiB, the MBR's
parameter page and the bootloader's settings page, 4 KiB each: 48 KiB,
12 pages (`bark-fw/README.md`, "Flash layout"). With nothing left that
calls the MBR, its 4 KiB too, the loader moving to `0x0`. And the
`0xEA000` limit is the bootloader's, so it goes with it.

**What our loader would need.** The bootloader's job that we use is one:
a board that a double tap puts on USB takes an image. For that the
loader needs:

- the 32 MHz crystal and the USB peripheral (embassy-nrf), and
  embassy-usb's device stack: the firmware's USB with its logger is
  2.9 KiB of embassy-usb plus part of embassy-nrf's 3.4 KiB (the study);
- either USB DFU (the `embassy-usb-dfu` crate, built for embassy-boot;
  the host runs `dfu-util`) or a mass-storage device that takes UF2
  files by copy (SCSI over bulk-only transport and a FAT image made up
  on the fly; embassy-usb has no such class, so we would write one, a
  few kilobytes);
- the double tap: a word in RAM kept across a reset, and a half-second
  wait at every boot.

Estimated at 16–22 KB in all, against the loader's 8.7 KB: the loader
region grows from 4 to 6 pages, and the net gain is about 40 KiB.

**Bricking.** The bootloader is replaced by a UF2 the bootloader itself
writes into its own region; a fault in that copy, or a loader whose USB
does not come up, leaves a board that only SWD can recover: a probe on
the board's SWD pads and `probe-rs` writing the loader back. Every board
in the field would then depend on our USB code never being broken by an
update to the loader, which D79 made the one part never updated.

**Why D6 kept it**: replacing it needed a probe and gained nothing; the
UF2 drive recovers any board by a double tap. D79 kept it for the same
reason and put our loader behind it.

**Verdict**: not with the external chip (section 1) in place, which
gives ten times the room without the risk. If the external chip proves
unusable, the order is the ring halved, then the update's pages lent,
and only then this, with a probe at hand for every bench board.
**Effort**: a week with bench time, mostly the USB class and the
recovery tests.

## 4. The BLE host's pairing

Measured against `main` (398 716 B), each change built on its own.

**What is already gone.** The vendored host (`bark-fw/vendor/trouble-host`)
has its central side behind `central`, legacy pairing off (its
`legacy-pairing` feature is not on, and a request without Secure
Connections is refused), and distributes only identity keys, which the
phone's private addresses need. Nothing to cut there.

**The paths a display-only peripheral never takes**, in
`security_manager/pairing/peripheral.rs`:

| Path | Δ, measured | Lost |
|---|---|---|
| out of band (`WaitingOob*` states, `handle_oob_*`; the method answers `OobNotAvailable`) | −1 144 B | nothing: the node offers no OOB data, so no phone chooses it |
| Just Works and numeric comparison (`WaitingNumericComparison*`, `numeric_compare_*`, `Nonce::g2`; refused with `AuthenticationRequirements`) | −632 B | nothing: numeric comparison needs a display with yes/no on our side; Just Works comes only from a central with no keyboard, and `settle` in `src/ble.rs` already closes such a link. Refusing at the pairing step is stricter |
| passkey typed on the node (`WaitingPassKeyInput`, `handle_pass_key_input`) | −240 B | nothing: the node has no keyboard |
| all three | −2 296 B | Android and iOS both say keyboard-and-display, which gives the passkey the node shows |

The whole state machine after the key exchange is 3.8 KB (measured by
refusing everything there): the host's pairing cost is the crypto and
the bond plumbing, not these paths.

**The crypto**, by symbol and by stubbing:

| Part | Bytes |
|---|---|
| P-256 (`p256`, `primeorder`, `elliptic-curve`, `crypto-bigint`) | 14 300, measured by stubbing key generation and agreement |
| AES and CMAC in software (`aes`, `cmac`, `dbl`) | about 4 700, by symbol |
| the ChaCha12 generator for the host's random numbers | about 2 800, by symbol, more inlined |

- **P-256 cannot move to the controller**: the controller library in
  `nrf-sdc-sys` 0.3.0 lists the commands LE Read Local P-256 Public Key
  and LE Generate DHKey in its masks but implements neither
  (`docs/pairing-2026-11-06.md` found the same). The CryptoCell-310
  would need Nordic's closed library, which nothing here wraps. It
  stays.
- **AES in hardware**: the MPSL's ECB block encryption is linked
  already, for the controller, with a safe wrapper
  (`MultiprotocolServiceLayer::ecb_block_encrypt`). The host's AES
  calls through it and CMAC written out (RFC 4493, 50 lines): **−5 280 B
  measured**; the `aes`, `cmac` and `digest` crates leave the image.
  The GATT database hash uses the same code.
- **Random numbers from the controller's LE Rand**, which draws on the
  hardware generator the firmware already gives the controller, in place
  of ChaCha12: **−5 984 B measured**. A passkey pairing draws about
  350 bytes. An AES counter generator over the hardware block, seeded
  once from LE Rand, would be smaller still.
- Both: **−11 404 B measured**; with the three paths cut, **−13 292 B**.

**Costs and risks.** The host must not call Nordic's functions
directly: the patch adds a feature with a block cipher and a random
source the firmware provides at link time, and keeps the software AES
as a development dependency so the host's tests of f4, f5, f6, g2 and
ah against the specification's sample data still run on the PC. About
120 lines in `crypto.rs` and `security_manager/mod.rs`, a file that
changes little upstream. To verify before shipping: the CMAC against
RFC 4493's test vectors, a pairing with Android and iOS on the bench,
and that the MPSL's ECB may be called from a task while the controller
uses it from its interrupts (the API is public for applications, which
suggests so; not verified). The path cuts are 30 lines in the pairing
state machine, which changes between releases: reworked at each update
of the host.

**Verdict**: the hardware AES and random source, 11.4 KB, yes, if
section 1 is not done or after it, as hygiene; the path cuts are not
worth carrying for 2.3 KB, except refusing Just Works at the pairing
step, which is clearer behaviour, not a saving. **Effort**: a day, plus
the bench pairing.

## 5. The tasks' state machines

Measured against `main` (398 660 B). The largest poll functions
(`cargo bloat`):

| Task | Largest functions | KiB |
|---|---|---|
| phone link | `ble::run`'s joined blocks, `serve` inlined into the link's | 12.8 + 10.7 |
| main | its poll | 8.6 |
| radio | its poll, `radio::boot` inlined; `Driver::step` apart | 6.7 (+13.0) |
| GPS | its poll | 5.4 |
| screen | its poll | 5.1 |
| USB | its poll | 4.1 |
| the rest | log sink 1.3, store 1.0, mailbox 0.8, blink 0.5, watchdog 0.5, button 0.3 | |

The bytes are the work: each poll holds functions it alone calls,
inlined. The main task's 2 911 instructions are one-time setup (the
chip, USB's descriptors, the MPSL, the store, the mailbox, the core)
with no `.await` in the node build. Repeated sequences are already
folded by `"z"`'s outliner.

Samples, each measured alone:

| Change | Δ |
|---|---|
| `serve`'s events other than GATT, with their log lines, in a sync `#[inline(never)]` function | +112 B, RAM −64 B |
| the main task's whole body in a sync `#[inline(never)]` function | +32 B |
| one seven-argument GPS warning in its own `#[inline(never)]` function | +16 B |

Moving cold code out of an async body saves nothing; each move adds a
call. The other tasks are of the same shape. **Verdict**: drop it.
Bytes leave the tasks only with the work they do, such as their log
lines (section 2).

## 6. The panic sites, on stable

Counted on `main` (398 660 B) from the disassembly: 580 call sites into
the panic functions.

| Kind | Sites |
|---|---|
| async fn resumed after completion | 259 |
| formatted (`panic_fmt`) | 99 |
| index and slice checks | 125 |
| `core::panicking::panic` | 44 |
| `RefCell` borrows | 23 |
| `unwrap`, `expect` | 21 |
| `assert` | 9 |

By crate: `bark-fw` 187 (111 of them resumed-after-completion),
`bark-driver` 101 (95), `trouble-host` 74, `embassy-usb` 29,
`bark-proto` 24, `embassy-nrf` 24, the rest fewer.

Each site points to a source location, 16 B in `.rodata`: 1 128 of
them, 18 048 B, more than the sites because every `log!` call and
other `#[track_caller]` functions carry one too. Their file names are
119 strings, 6 782 B, 57 B on average: 52 of them begin with the
52-character registry prefix
`/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`.

| Change, measured | Δ |
|---|---|
| `scripts/rustc-remap.sh` maps each registry crate's directory to nothing (`aes-0.8.4/src/soft/fixslice32.rs`), the workspace to nothing (`bark-proto/src/...`) and the standard library's `/rustc/<commit>/library/` to nothing | **−3 120 B** |
| every `expect("…")` of ours as `unwrap()` | −144 B |
| the 13 `expect("spawned once")` of `main.rs` through one cold function that panics | −288 B, 22 B a site |
| the same, the function `#[track_caller]` | +32 B: the per-site location is the cost |

- **The remap** keeps what the script is for, paths independent of the
  checkout and the home directory, and loses nothing a panic message
  says: each still names its crate, file and line. The wrapper is not
  in Cargo's fingerprint, so the change needs a `cargo clean`, and the
  script's comment, which argues for `/rustc/<commit>`, is rewritten
  with it. **Take it.**
- **Resumed after completion**, 259 sites, about 6–7 KB: one per async
  state machine, inlined or not. `panic = "abort"` is already in effect
  and keeps them. Only fewer async functions and blocks remove them.
  Not actionable.
- **A shared cold panic per module** for our 30–60 `unwrap`s and
  `expect`s: 22 B a site, 0.5–1 KB, and the message loses its line.
  Only where the line tells nothing.
- **`get` in place of indexing**: about 40 of the 125 index sites are
  ours, each needing its own `None` path: at most 0.8 KB. Only in code
  being changed anyway.
- **The log lines' locations** (our logger never prints them) go only
  with a macro of our own, whose per-site code is likely larger. No.

## Recommended order

| | Room | Effort | Why now |
|---|---|---|---|
| a. The mailbox's state page to `0xF2000`, its ring below it | 0 | an hour, one UF2 per board | the mailbox key is readable from the board's drive today (section 1) |
| b. `B9h` to the external chip at boot | −152 B, and 11.5 µA less on battery | done, `bark-fw/src/ext_flash.rs` | the chip idles in standby otherwise |
| c. The path remap | −3 120 B | an hour | measured, free |
| d. The external chip (section 1) | +528 384 B | built 2026-11-06 (D139); the bench's swap rows to redo | the layout must be settled before the field; it ends the question |
| e. The host's AES and random numbers in hardware (section 4) | −11 404 B | a day and a bench pairing | worth having once (d) is in; the first choice if (d) waits |
| f. The host's packet pool built at boot (the study) | −4 036 B | half a day | small and safe |
| g. Warnings and errors only, or binary logging (section 2) | −14.6 KB, or about −19 KB | an hour, or two days | only if (d) is not done |
| h. Our loader for the bootloader (section 3) | about +40 KiB | a week | only if the external chip proves unusable, after halving the ring (+16 KiB) and lending the update's pages (+44 KiB) |

Dropped: the tasks' cold paths (section 5), the pairing path cuts
(2.3 KB of patch to carry, section 4), the remaining panic ideas
(section 6).

With (c) and (e) alone, about 25.6 KB free in today's slot: room for
pairing's follow-ups, not for the mesh update's receiving side.
With (d), the room is no longer the question.

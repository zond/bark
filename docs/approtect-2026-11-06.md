# Read-out protection (2026-11-06)

**Status:** built (roadmap D138); the facts below are from the nRF52840
Product Specification and the board maker's files, read 2026-11-06. Not
yet seen on a board; the bench check is at the end.

## The question

The internal flash holds the node's secrets: the bond's keys with the
paired phone and the mailbox key (`bark-fw/README.md`, "Flash layout").
A debug probe on the board's SWD pads reads the whole internal flash
unless the chip's access port protection is on. The user decided on
2026-11-06 to turn it on for every board that leaves the bench, and to
keep the bench's boards open.

## How the chip protects its debug port

nRF52840 Product Specification, "Access port protection", "UICR", "NVMC",
"FICR", and Nordic's nRF52840 IC revision overview:

- **Build codes D and earlier** (revisions 1 and 2): the port is open by
  default. Programming `UICR.APPROTECT` to `Enabled` (`0x00`) closes it
  from the next reset.
- **Build codes F and later** (revision 3): the port is closed after every
  pin, power-on, brown-out or watchdog reset and every wake from System
  OFF. It opens only if `UICR.APPROTECT` holds `HwDisabled` (`0x5A`) and
  the firmware writes `SwDisable` (`0x5A`) to `APPROTECT.DISABLE`
  (`0x40000558`) after each such reset. Writing `Force` (`0x00`) to
  `APPROTECT.FORCEPROTECT` (`0x40000550`) keeps it closed until the next
  reset whatever is written to `DISABLE`. For a closed chip the
  Specification advises `Enabled` in UICR and `Force` from the firmware.
- **UICR** is written as flash is, through the NVMC with `CONFIG.WEN`
  set; a write can only turn bits from 1 to 0, and the new value takes
  effect only after a reset. So `Enabled`, all zeros, can be written over
  any value, and nothing can be written over it.
- **The only way back** is `ERASEALL` through the CTRL-AP, the one part
  of the debug port a closed chip still answers. It erases the flash,
  UICR and RAM, and so every key, the board's bootloader and the
  bootloader's address in UICR with them. `CTRL-AP.APPROTECTSTATUS` then
  reads whether the port is open.
- **Which chip:** `FICR.INFO.VARIANT` (`0x10000104`) holds the variant's
  two letters and the build code's first two characters in ASCII, `AAF0`
  read as one word; its third byte is the build code's letter. Revision 3
  is build code `Fx0`, revision 2 `Dx0`, revision 1 `Cx0`. There is no
  `INFO.REVISION`.

The bench's boards have never logged their build code, so which revision
they are is not known yet; the boot line now says it.

Until this change the firmware did not leave the port alone: the HAL's
start-up (`embassy_nrf::init` with its default `Debug::Allowed`) programs
`HwDisabled` and writes `SwDisable` on build code F, so the boards have
been open on either revision.

## The rule

A build is open or protected, chosen when it is built: `BARK_OPEN=1` for
the bench, protected otherwise. At every boot, before anything else, the
firmware (`bark-fw/src/protect.rs`) does what `bark_image::protect::steps`
gives for the build and the chip:

| Build | Build code D and earlier | Build code F and later |
|---|---|---|
| protected | UICR `Enabled` | UICR `Enabled`; `Force` at every boot |
| open | nothing | UICR `HwDisabled`; `SwDisable` at every boot |

When it wrote UICR it resets the chip at once, and the next boot finds
the value in place. Protection is therefore one way on both chips: an
open image on a board a protected image has closed leaves it closed, and
only an erase of the whole chip opens it. That is why the protected build
writes `Enabled` on build code F as well, rather than leaving `HwDisabled`
and only not writing `SwDisable`: otherwise any open image copied to the
board would reopen the port, and so would a bootloader that copies UICR
into `DISABLE` at its start. (No
address of the `APPROTECT` registers appears as a literal in the board's
bootloader 0.6.1; that does not prove it never writes them.)

**Why the firmware and not the loader.** The loader is the same for both
builds and is never updated (roadmap D79), so it would have to read the
choice from the image, a field it does not have. The firmware needs no
such field. The time before it runs is harmless: on a protected board the
chip closes the port from reset, in hardware, so the MBR, the bootloader
and the loader run with it closed; the only open moment is the first boot
after a protected image is copied to an open board, from reset until the
write, a few milliseconds before the reset that closes it.

## What it costs the bench

Nothing it uses today. The board's bootloader takes an image over USB
from the chip itself: it runs on the CPU and writes the flash through the
NVMC, which the access port protection does not touch; it only stops a
probe reading or writing the chip. A closed board still takes `bark.uf2`
and `bark-fw.uf2` by a double tap and its drive. What it loses is the
debug pads, which the bench has no probe for.

A closed board that must be open again needs a probe and the recovery
below. A bench build without `BARK_OPEN=1`, or a released node image,
closes a bench board for good in that sense; `scripts/bench.py` builds
with it, and the released console image is built with it.

## What it does not protect

Anyone holding the board can still copy any UF2 to its drive by a double
tap: the board's bootloader takes every image. An image of their own runs
with every key in reach and can send them over USB. Read-out protection
closes the pads, not that. Closing it needs a bootloader that takes only
signed images (`docs/flash-plan-2026-11-06.md`, "Our loader in place of
the board's bootloader"), with this protection on, since a probe could
otherwise write any image past it.

## Recovery: a closed board back to open

From the board maker's T-Echo repository (github.com/Xinyuan-LilyGO/T-Echo,
`bootloader/README.MD`, read 2026-11-06): connect a J-Link to the board's
SWD pads, open J-Flash Lite, select the device `NRF52840_XXAA`, erase the
chip, and program the bootloader hex,
`bootloader/lilygo_techo_bootloader-0.6.1_s140_6.1.1.hex` in that
repository, the version the boards came with (`docs/softdevice-2026-11-05.md`,
"The bootloader on the boards"). On a closed chip the erase is the
CTRL-AP's `ERASEALL`; J-Link offers it when it finds the port closed.
The hex holds the MBR, the S140 SoftDevice, the bootloader and its two
UICR words (`0x10001014` = `0xF4000`, `0x10001018` = `0xFE000`), and no
`APPROTECT` value. Then, as for a new board: a double tap, and
`bark.uf2` built with `BARK_OPEN=1` and `--loader` (`bark-fw/README.md`,
"Build and package", "Flash"). The erase has wiped the node's stored
state, its bond and its mailbox key; it keeps its node ID, which is in
FICR.

## On the bench

| Check | How | Result |
|---|---|---|
| The build code of each bench board | the `protection=` line after the banner, `build_code=` | open |
| An open build leaves the port open | `protection=off`, and on build code F `approtect=0x5a` | open |
| A protected build closes it | `protection=on approtect=0x0` on the boot after the one that wrote it | open |
| A probe cannot connect to a protected board | the bench has no probe | open |
| A closed board takes a UF2 | a double tap and `bark-fw.uf2` | open |

# The SoftDevice's pages (2026-11-05)

**Status:** read from the board's bootloader, its source and its binary;
the bench test below is to run on board f4f5. The flash layout does not
change until it passes.

## The question

The flash layout (`bark-image/src/layout.rs`, `bark-fw/README.md`
"Flash layout") keeps `0x01000..0x26000`, 148 KiB, for the S140
SoftDevice, which bark never enables: its BLE is the SoftDevice
Controller library linked into the image (roadmap D5). The pages are kept
only because nobody knew whether the board's UF2 bootloader starts the
application without them. The layout cannot move once boards are in the
field.

## The bootloader on the boards

`INFO_UF2.TXT` on a board's drive reports `UF2 Bootloader
0.6.1-2-g1224915`, S140 6.1.1, `Board-ID: nRF52840-TEcho-v1`
(`docs/bench-2026-10-30.md`). The board maker's T-Echo repository
(github.com/Xinyuan-LilyGO/T-Echo, read 2026-11-05) ships it as
`bootloader/lilygo_techo_bootloader-0.6.1_s140_6.1.1.hex`: its bootloader
region carries that version string, the volume name `TECHOBOOT` and that
board ID, and it sets the bootloader address in UICR (`0x10001014` =
`0xF4000`). Its MBR (`0x0000..0x0B00`) and SoftDevice (`0x1000..0x25DE8`)
bytes are identical to the S140 6.1.1 hex in the bootloader's own
repository at tag 0.6.1. The two commits past 0.6.1 add the board; the
code below is read at tag 0.6.1 (github.com/adafruit/Adafruit_nRF52_Bootloader)
and each step checked in the disassembly of the hex's bootloader.

**Where it starts the application.** `bootloader_app_start`
(`lib/sdk11/components/libraries/bootloader_dfu/bootloader.c`; in the
binary at `0xF567C`) reads the SoftDevice's information structure at
`0x3000`. If the word at `0x3004` is the SoftDevice's magic number
`0x51B1E5DB` (`is_sd_existed`, `dfu_types.h`), the application starts at
the SoftDevice's end address, the word at `0x3008` (`0x26000` for S140
6.1.1), and the bootloader asks the SoftDevice to forward interrupts
there (`sd_softdevice_vector_table_base_set`). Otherwise the application
starts at `0x1000`, the end of the MBR (`MBR_SIZE`), and the bootloader
asks the MBR to forward interrupts there (`SD_MBR_COMMAND_IRQ_FORWARD_ADDRESS_SET`,
command 6). Either way it then loads the stack pointer and the reset
vector from that address and branches (`bootloader_util_app_start`,
`bootloader_util.c`). No address is fixed but those two.

**Whether the application is valid.** `bootloader_app_is_valid`
(`bootloader.c`) looks at the same address (`DFU_BANK_0_REGION_START`,
which is `CODE_REGION_1_START`, the same choice): the first two words must
not be erased, and the settings page must mark the application valid,
which every completed UF2 copy does, with a CRC of 0 that means no CRC is
checked. An application that is not valid keeps the bootloader in its
drive at every reset, without a double tap (`main` in `src/main.c`).

**So the answer is no.** With the SoftDevice's pages erased, the
bootloader does not start our loader at `0x26000`: it looks for the
application at `0x1000`. If `0x1000` is erased it stays in its drive.

**It runs without a SoftDevice.** Since 0.4.0 "bootloader will always
work with and/or without softdevice present" (`changelog.md`). Every
call into the SoftDevice on the USB path is behind `is_sd_existed`
(`src/main.c`, `src/usb/usb.c`, `src/usb/uf2/ghostfat.c`); flash is
written through the NVMC directly (`src/flash_nrf5x.c`,
`bootloader_settings_save`). `INFO_UF2.TXT` then says
`SoftDevice: not found`. The one exception is its BLE update path,
entered when the application leaves `0xA8` or `0xB1` in GPREGRET before a reset or
when the DFU and FRESET buttons are held at reset: it enables the
SoftDevice without checking (`softdev_init`, `src/main.c`), and with none
it faults. bark never sets that value; a reset with the buttons released
leaves the path.

**How the drive places a UF2.** `write_block`
(`src/usb/uf2/ghostfat.c`) writes a block of the application family
(`0xADA52840`, or the board's own ID) when its address is in
`0x1000..0xEA000`: from `MBR_SIZE` to the bootloader's start less the
ten pages it keeps for the application's data (`USER_FLASH_END`,
`uf2cfg.h`; `DFU_APP_DATA_RESERVED=10*4096` for the nRF52840 in the
`Makefile`; in the binary, `addr - 0x1000 < 0xE9000`). It skips a block
below `0x1000` and counts it written. Any other block is ignored and not
counted, so a file with a block at or above `0xEA000` never completes:
the drive takes it and the board does not restart. "SoftDevice is
considered as part of application and can be (or not) included in uf2"
(its comment there). The drive neither protects nor requires the
SoftDevice's pages: it erases and writes them as it would any page of an
application, so both an erase and a restore by UF2 are allowed.
`scripts/uf2.py` took `0xF4000` as the window's end; it is `0xEA000`.
Our images end at `0xDD000`.

**What the MBR needs.** At reset the MBR starts the bootloader at the
address in UICR; only with no address set does it start the SoftDevice
at `0x1000`. With S140 present and no address the SoftDevice would start
the application at `0x26000` and never the bootloader, so a board that
reaches its bootloader after a double tap, as every board does today,
has the MBR going there directly, past the SoftDevice's pages. The MBR's
own parameter page, `0xFE000`, is outside them.

**What our side needs.** The loader sets VTOR before it jumps and the
firmware sets its own (cortex-m-rt `set-vtor`), so the forwarding the
bootloader sets up serves only the loader, whose vector table is at its
start. The MBR keeps the forwarding address in `0x20000000..0x20000008`,
which the layout already leaves alone.

## What follows

The pages can be ours, but by moving the loader to `0x1000`, not by
erasing them under the loader where it is. A UF2 of such a layout must
write the whole of `0x1000..0x26000`, so that nothing of the SoftDevice
stays, and the word at `0x3004`, inside the loader's own bytes, must
never be the magic, or the bootloader would take the rest for a
SoftDevice and jump to the word at `0x3008`. `scripts/uf2.py` can check
both.

The gain is 37 pages, 148 KiB, once. It is either both image slots
larger by 18 pages, 72 KiB, or the mailbox's 37 pages
(`docs/app-layer-2026-11-05.md`, section 5), or a split; not both. The
staged image's record must stay below `0xEA000`, since every UF2 writes
it erased. Every board takes the new layout by one UF2 copy; there are
three on the bench and none in the field.

## The bench test

On board f4f5, entered by a double tap, which is a pin reset and so
clears the write protection the loader sets (nRF52840 Product
Specification, ACL). No step touches the bootloader, its settings, the
MBR or UICR; each is undone by a copy to the drive.

The files are built into `bark-fw/target/`:

| File | Writes |
|---|---|
| `sd-1-erase-softdevice.uf2` | `0xFF` over `0x1000..0x26000`, then the loader and the node firmware as `bark.uf2` today |
| `sd-2-loader-at-0x1000.uf2` | the loader linked at `0x1000`, `0xFF` over the rest of `0x1000..0x26000`, the firmware where it is today at `0x2B000`; built from a copy of the tree whose layout moves the loader alone, so it protects `0x0..0x5000` |
| `sd-restore-softdevice.uf2` | S140 6.1.1 from the board maker's hex at `0x1000..0x26000`, then the loader and firmware as today |

0. Double tap. Check `INFO_UF2.TXT`: `0.6.1-2-g1224915`, `S140 version
   6.1.1`. Copy `CURRENT.UF2` off the drive and keep it: it is every byte
   of `0x1000..0xEA000`, the SoftDevice included, about 1.9 MB, and
   copying it back restores that window as it was.
1. Copy `sd-1-erase-softdevice.uf2`. Expected: the drive goes, the board
   restarts and the drive comes back by itself, without a tap;
   `INFO_UF2.TXT` says `SoftDevice: not found`; no serial port, no 1 Hz
   blink. That is the bootloader running without a SoftDevice and
   finding nothing at `0x1000`. If the firmware starts instead (serial
   port, `update: acl 0 start=0x0 len=0x2a000`), this note is wrong:
   stop there.
2. Copy `sd-2-loader-at-0x1000.uf2`. Expected: the firmware starts as it
   does today, the 1 Hz blink and the serial port, and the log shows
   `update: acl 0 start=0x0 len=0x5000 write=Disable` and `update: acl 1
   start=0xf4000 len=0xc000 write=Disable`, then `radio up`. `len=0x5000`
   says the loader at `0x1000` ran; today's loader, still at `0x26000`
   from step 1, would log `len=0x2a000`. Then `stty -F <port> 1200` must
   give the drive, a double tap must give it, and unplugging and
   plugging must start the firmware again.
3. Double tap and copy `sd-restore-softdevice.uf2`, or the kept
   `CURRENT.UF2`. Expected: the board as before step 1, `INFO_UF2.TXT`
   with `S140 version 6.1.1`, `acl 0 ... len=0x2a000`.

What a wrong reading would look like. If step 2 shows the drive again
instead of the firmware, the bootloader did not take the loader at
`0x1000`: copy the restore file. If a step leaves no drive, no port and
no LED, and a double tap brings no drive, the MBR needed the SoftDevice's
pages on its way to the bootloader after all; only a probe on the SWD
pads recovers that, flashing the board maker's hex. The reading above
says it cannot happen, since the MBR goes to the bootloader by the UICR
address; step 1 is where it would show, before anything else is moved.

## The layout if the test passes

The loader at `0x1000`, the application region from there to the stored
state's page: 242 pages instead of 205. The loader, its state, the
staged image's record and the 22 pages of pieces stay as they are; the
37 pages go to the slots or the mailbox:

| | slots | mailbox | running image | ring at 23 a day |
|---|---|---|---|---|
| A | 88, 89 | 1 + 36 | 352 KiB | 560 items, 24 days |
| B | 106, 107 | none (8 of the pieces' pages, the note's fallback) | 424 KiB | 112 items, 4.9 days |
| C | 98, 99 | 1 + 16 | 392 KiB | 256 items, 11 days |

The node image is 351 368 bytes today in a slot of 360 448: 9 KB free,
and the mailbox's code is estimated at 8 KB of it
(`docs/app-layer-2026-11-05.md`, "Code"). A leaves no room for anything
after the mailbox; B leaves a ring that holds five days. C is proposed:
40 KiB more for each image, and a ring that holds eleven days of a busy
person's mail.

| Region | Start | Pages |
|---|---|---|
| MBR | `0x00000` | 1 |
| loader | `0x01000` | 4 |
| loader state | `0x05000` | 1 |
| firmware, running | `0x06000` | 98 |
| firmware, staged | `0x68000` | 99 |
| staged image's record | `0xCB000` | 1 |
| coded pieces | `0xCC000` | 22 |
| mailbox | `0xE2000` | 17: its state, then a ring of 16 |
| stored state | `0xF3000` | 1 |
| bootloader and the rest | `0xF4000` | as today |

The loader then write-protects `0x0..0x5000`, the MBR and itself.

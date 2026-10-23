/* nRF52840 memory map for the LILYGO T-Echo with its stock UF2 bootloader
 * (roadmap decision D6). 1 MiB flash, 256 KiB RAM (nRF52840 Product
 * Specification, Memory chapter).
 *
 * Flash, as laid out by the bootloader's own linker script
 * (Adafruit_nRF52_Bootloader, linker/nrf52840.ld) and the S140 6.1.1
 * SoftDevice specification:
 *
 *   0x00000000..0x00001000   MBR
 *   0x00001000..0x00026000   SoftDevice S140 6.1.1 (kept, never enabled by us)
 *   0x00026000..0x000F3000   application                      <- FLASH
 *   0x000F3000..0x000F4000   the node's stored state, one page <- STORE
 *   0x000F4000..0x000FD800   bootloader code
 *   0x000FD800..0x000FE000   bootloader config
 *   0x000FE000..0x000FF000   MBR parameter page
 *   0x000FF000..0x00100000   bootloader settings page
 *
 *   LENGTH(FLASH) = 0xF3000 - 0x26000 = 0xCD000 = 839 680 B = 820 KiB.
 *
 * STORE is one 4 KiB page (nRF52840 Product Specification, NVMC: the
 * erase unit) that src/store.rs writes the core's stored state into
 * (roadmap D73); src/store.rs names its address.
 *
 * RAM. On a normal boot to the application the bootloader does not enable
 * the SoftDevice, so the application owns all 256 KiB. Two carve-outs at the
 * bottom:
 *
 *   0x20000000..0x20000008   left untouched: the MBR's RAM words (Nordic SDK
 *                            MBR builds start RAM at 0x20000008). We set VTOR
 *                            ourselves (cortex-m-rt `set-vtor`), so nothing
 *                            in this firmware depends on them, but the
 *                            bootloader calls into the MBR on every reset.
 *   0x20000008..0x20000400   PANDUMP: panic-persist writes the panic message
 *                            here and soft-resets; the next boot logs it over
 *                            USB. The bootloader's own RAM starts at
 *                            0x20008000 (plus 0x20007F7C..0x20008000 for its
 *                            double-reset word and noinit block), so a soft
 *                            reset through it should leave this range intact.
 *                            Unverified on hardware; if it is clobbered the
 *                            magic word does not match and nothing is logged.
 *   0x20000400..0x20040000   RAM: .data, .bss, heap-free, stack at the top.
 *
 * When nrf-sdc (BLE controller, roadmap D5) arrives it takes its memory as a
 * static buffer inside RAM; no region needs carving for it here.
 */
MEMORY
{
  FLASH   : ORIGIN = 0x00026000, LENGTH = 0xF3000 - 0x26000
  STORE   : ORIGIN = 0x000F3000, LENGTH = 0x1000
  PANDUMP : ORIGIN = 0x20000008, LENGTH = 0x400 - 0x8
  RAM     : ORIGIN = 0x20000400, LENGTH = 256K - 0x400
}

_panic_dump_start = ORIGIN(PANDUMP);
_panic_dump_end   = ORIGIN(PANDUMP) + LENGTH(PANDUMP);

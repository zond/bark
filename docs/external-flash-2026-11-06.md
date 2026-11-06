# The external flash's wiring and supply (2026-11-06)

**Status:** facts from the board's schematic and the chip's datasheet,
none measured. Fills what the pin research
(`docs/implementation-research-2026-10-14.md`) left out and
`docs/flash-plan-2026-11-06.md` section 1 needs.

Sources:

- The schematic: `schematic/T-Echo_Schematic.pdf` in the maker's T-Echo
  repository on GitHub (Xinyuan-LilyGO/T-Echo), one sheet, dated
  2021-06-23 in the PDF. Read by its net names and part references.
- The datasheet: Zetta "ZD25WQ16B Ultra Low Power, 16M-bit Serial Multi
  I/O Flash Memory", as LCSC serves it for part C969904.

## The chip

The schematic draws U4 as an MX25R1635FZUIL0. The fitted chip is not
one: the factory firmware read its JEDEC ID as `BA 60 15`
(`docs/bench-2026-10-30.md`), which is the ZD25WQ16B's (datasheet,
"Table ID Definitions": RDID gives manufacturer `BA`, memory type `60`,
density `15`). An MX25R1635F answers `C2 28 15`. The two share the
package's pin-out and the commands used here, `B9h` and `ABh`.

## The pins

| U4 pin | Function | Net | nRF52840 |
|---|---|---|---|
| 1 | CS# | `F_SPI_CS` | P1.15 |
| 2 | DO (IO1) | `F_SPI_MISO` | P1.13 |
| 3 | WP# (IO2) | `P0.07_WP` | P0.07 |
| 4 | GND | | |
| 5 | DI (IO0) | `F_SPI_MOSI` | P1.12 |
| 6 | CLK | `F_SPI_CLK` | P1.14 |
| 7 | HOLD# (IO3) | `P0.05_HOLD` | P0.05 |
| 8 | VCC | `VDD_POWR`, 100 nF to ground (Cp) | |

Each of the six signal nets appears twice on the sheet, at U4 and at the
nRF52840, and nowhere else: **no pull-up on CS#, WP# or HOLD#**. With
the nRF52840's pins in their reset state (input, disconnected) all
three float.

The datasheet wants them high: HOLD# low pauses a command, WP# low
locks the status register, and the quad commands that use them as IO2
and IO3 need the QE bit, clear by default (datasheet 3.3, "Hold
Feature", and "QE bit"). Any use of the chip drives both high.

## The supply

`VDD_POWR` is the switched rail. On the sheet, the 3.3 V regulator U11
(AP2112K-3.3) makes `VDD3V3`; its EN is `PWR_EN` (P0.13) through Rw,
10 kΩ, with R37, 100 kΩ, to ground. A P-channel MOSFET Q8 (SI2301)
passes `VDD3V3` to `VDD_POWR`; its gate is pulled low, turning it on,
by the transistor Q5 (PMBT3904), whose base is fed through R28, 10 kΩ,
from `VBUS` (diode D5) or `PWR_ON` (P0.12, diode D6). So:

    VDD_POWR = PWR_EN AND (PWR_ON OR VBUS)

as `bark-fw/src/board.rs` has it. R37 keeps the regulator off while
`PWR_EN` floats.

The firmware drives both high when the radio starts and holds them for
as long as it runs, on USB and on battery (`bark-fw/README.md`, "Radio
power"). So the chip is powered whenever the firmware runs, from about
10 ms after boot. Before that, and in the loader, it is not.

## Logic levels and power-up

- The nRF52840 runs from VDDH, its I/O at the output of its own
  regulator (`VDD_nRF`); the chip runs at 3.3 V and wants 0.7 VCC for a
  high (datasheet Table 4-2, VIH). The factory firmware read the ID over
  these lines, so the levels meet on this board.
- At power-up CS# must follow VCC, and the chip takes its first command
  70 µs (tVSL) after VCC reaches its minimum (datasheet 4.5, "Operation
  Conditions", Figure 4-9). CS# floats here while the rail rises; a write
  still needs a Write Enable first, so a stray select cannot change
  the contents.
- A power cycle of the rail leaves deep power-down: the chip comes up in
  standby (datasheet 5.30, "Deep Power-down (DP)").

## Currents and commands

From the datasheet, Table 4-2 (DC) and Table 4-3 (AC):

| | |
|---|---|
| standby, ISB | 12 µA typical, CS#, WP# and HOLD# high |
| deep power-down, IDPD | 0.5 µA typical, 1.5 µA max., CS# at VCC and the other inputs at a rail |
| enter deep power-down | `B9h`; down tDP = 3 µs after CS# rises |
| leave it | `ABh`; ready tRES1 = 8 µs after CS# rises |

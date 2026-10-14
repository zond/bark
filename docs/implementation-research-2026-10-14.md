# Implementation research, 2026-10-14

What exists to implement `hrafnar/docs/civilian-433-mesh-design.md`, what is missing, and
where the design's numbers disagree with datasheets and regulators. Sources at the end.
Section numbers (§) refer to the design.

## 1. Rust stack

### Routing: Darkmap

`~/projects/darkforest/darkmap` (18.6k lines incl. tests) is `no_std` + `alloc`, with
greedy forwarding, DFS backtracking with a visited list, recent-RX dedup, per-hop ACK, and a
pull-based `Transport` trait. Reusable pieces and the reasons we copy rather than depend on
it are in `roadmap.md` (D1) and `simulator-design.md` §5. Facts that drove that decision:

- `Cargo.lock` resolves embassy-time 0.4.0 and embassy-sync 0.6.2. lora-phy (git), trouble,
  nrf-sdc and embassy-nrf 0.11 all use embassy-time 0.5 and embassy-sync 0.8. Darkmap's
  public API exposes `embassy_time::Instant`, so two versions cannot coexist.
- `node/mod.rs` (2250 lines) interleaves routing with gossip selection, the UKF and the
  global table. `NodeId = [u8; 4]` keys every table.
- It needs a global allocator; `embedded-alloc` is the documented choice.

### Radio driver: lora-phy

- crates.io latest is 3.0.1 (2024-07-11). Git main has, merged since: `get_rssi()` on
  SX126x (Nov 2024), sync word API (Nov 2024) and a runtime setter (Aug 2026),
  quarter-dB RSSI/SNR (Oct 2026). Use git, pin a revision (D4).
- `SpreadingFactor::_5` exists. The driver forces preamble ≥ 12 symbols for SF5/SF6.
- CAD: `prepare_for_cad` / `cad() -> bool`. Parameters are hard-coded: 8 symbols,
  `detPeak = SF + 13`, `detMin = 10`, exit mode CAD_ONLY. Tuning them in bench step 3
  means patching the driver.
- `RxMode::{Single, SingleMs, Continuous, DutyCycle(DutyCycleParams{rx_time, sleep_time})}`.
  Duty-cycle RX is implemented for SX126x (SetRxDutyCycle, 15.625 µs units).
- `rx_switch_channel(hz)` and `listen(hz, bw)` exist for the sweep. `prepare_for_rx` and
  `prepare_for_tx` both call SetRfFrequency every time and re-send modulation and packet
  params; image calibration runs once per `init()` at the first frequency. All 13 channels
  are in the single 430–440 MHz calibration band (datasheet Table 9-2), so retunes need no
  recalibration.
- Board config: `sx126x::Config { chip: Sx1262, tcxo_ctrl: Some(TcxoCtrlVoltage::Ctrl1V8),
  use_dcdc: true, rx_boost }` and `GenericSx126xInterfaceVariant::new(reset, dio1, busy,
  None, None)` with DIO2-as-RF-switch enabled (T-Echo has no RXEN/TXEN pins).
- The umsh project pins its own lora-rs fork with BUSY-before-command and CRC-error
  handling fixes, and shortens the TCXO settle from 10 ms to 5 ms. Expect similar edges.
- The nrf52840 example in lora-rs targets the RAK4631 (embassy-nrf 0.11, embassy-executor
  0.10, embassy-time 0.5, embedded-hal-bus 0.3, defmt 1).

### MCU HAL: embassy-nrf 0.11.0 (2026-06-16)

SPIM, UARTE, SAADC, GPIOTE, RTC time driver (`time-driver-rtc1`) are all current. Use
RTC1 for embassy-time because nrf-sdc takes RTC0.

### BLE: TrouBLE + nrf-sdc (D5)

- trouble-host 0.8.0 (2026-08-25); nrf-sdc 0.4.0 and nrf-mpsl 0.4 (2026-08-21).
  nrf-softdevice 0.1.0 (2024-01-15) has had no release since.
- nrf-sdc wraps Nordic's closed-source SoftDevice Controller binary; building it needs
  clang. The MPSL claims RTC0, TIMER0, TEMP and PPI channels 19, 30, 31; the controller
  claims PPI 17, 18, 20–29; RADIO, TIMER0, RTC0, RNG, CLOCK_POWER and EGU0_SWI0 interrupts
  are bound to its handlers. Controller memory is a static buffer (`sdc::Mem::<4720>` in the
  peripheral example; grows with links and MTU).
- GATT server with derive macros, notifications, security feature; `default-packet-pool-
  mtu-255` for 255-byte characteristics.
- umsh's T-Echo firmware uses exactly this: nrf-sdc 0.4 (`nrf52840, peripheral, central`),
  nrf-mpsl 0.3, trouble-host 0.7 (`derive, peripheral, gatt, security`), embassy-nrf 0.11,
  embassy-usb 0.6, embedded-alloc 0.5.

### Phone app

flutter_rust_bridge_codegen is installed on the dev machine. flutter_blue_plus covers BLE
central on Android and iOS; iOS background BLE is the documented weak spot. The ble_plus
package claims background support via iOS state restoration and an Android foreground
service (unverified).

### Toolchain gaps on the dev machine

No `thumbv7em-none-eabihf` target, no probe-rs, no UF2 converter. Needed before Phase 2.

## 2. Hardware

### LILYGO T-Echo

- All T-Echo, T-Echo Plus and T-Echo Lite variants were sold out on lilygo.cc on
  2026-10-14. OpenELAB (EU) lists the 433 MHz SoftRF build at 104.90 EUR, pre-order,
  10–20 days. Four boards are on order from China (arrive ~2026-11-04).
- T-Echo Card (433 MHz, in stock, 59 USD) is a different board: no e-ink (0.42" OLED),
  solar, 0.25 W panel, ICM20948 IMU, same nRF52840 + SX1262 + L76K. A possible relay
  form factor.
- The 433 MHz build was the default variant at launch (868/915 cost 1.33 USD more).

Pins (Meshtastic `variants/nrf52840/t-echo/variant.h`, confirmed by umsh hardware notes):

| Function | Pin |
|---|---|
| SX1262 SCK / MISO / MOSI | P0.19 / P0.23 / P0.22 |
| SX1262 NSS / RESET / BUSY / DIO1 | P0.24 / P0.25 / P0.17 / P0.20 |
| SX1262 DIO2 | internal to the module, drives its RF switch |
| SX1262 DIO3 | TCXO supply, 1.8 V (`TCXO_OPTIONAL` in Meshtastic) |
| Radio power | P0.13 (`PWR_EN`), **not** a red LED as the variant file says |
| Peripheral rail enable | P0.12 (`PWR_ON`); rail = PWR_EN AND (PWR_ON OR VBUS) |
| GPS L76K UART | P1.08 / P1.09 (NMEA arrives on P1.09 despite the names); standby P1.02, reset P1.05, PPS P1.04 |
| E-ink SSD1681 (GDEH0154D67, 200×200) | SCLK P0.31, MOSI P0.29, CS P0.30, DC P0.28, RES P0.02, BUSY P0.03, EN P1.11 |
| Buttons | P1.10 (user), P0.18 (labelled RESET, plain GPIO under the bootloader), touch P0.11 |
| LEDs | P0.14 blue, P0.15 green (P0.13 is power enable) |
| Battery ADC | AIN2 / P0.04, 2:1 divider, ~12 µA constant drain; unreliable while on USB |
| I2C | SDA P0.26, SCL P0.27; PCF8563 RTC, INT P0.16 |
| QSPI flash MX25R1635F | SCK P1.14, CS P1.15, IO0 P1.12, IO1 P1.13 |

Gotchas from umsh bring-up: do not hold GPS reset low while on USB; a floating P0.13
leaves the rail half-up; embassy-nrf SAADC default full scale is 3.6 V so battery mV =
raw × 7200 / 4096; USB-C-to-USB-C cables may not power the board.

### Bootloader and flash layout (D6)

Adafruit nRF52 UF2 bootloader, LILYGO build 0.6.1, SoftDevice S140 6.1.1 preinstalled.
Layout used by the umsh and LoRa-APRS firmware without modifying the bootloader:

```
0x00000000..0x00026000   MBR + SoftDevice slot (152 KiB, reserved)
0x00026000..0x000F4000   application (760 KiB)   <- FLASH ORIGIN in memory.x
0x000F4000..0x00100000   bootloader + UICR (48 KiB)
```

UF2 family `0xADA52840`, base `0x26000`, volume `TECHOBOOT`, DFU entry by double-tap of
the top-left button. INFO_UF2.TXT on the mounted drive reports the versions. SWD pads:
"re-flashing the bootloader requires an SWD programmer" per LoRa-APRS, but pad locations
not confirmed in any source I read; plan on UF2 + defmt over USB-CDC.

### Relays: RAK WisBlock

RAK4631 EU433 (PN 111000) is stocked at Rokland (22.95 EUR). No 433 MHz WisMesh Repeater
Mini SKU found; RAK's antenna options are 8xx/9xx only. O12 stays open; rooftop relays
are likely RAK4631 + RAK19007 base + RAK12500 GPS in a box, or a T-Echo Card.

## 3. Radio numbers from the SX1261/2 datasheet (Rev 1.2) and SX1268 (Rev 1.0)

| Quantity | Value | Design said | Note |
|---|---|---|---|
| Minimum preamble, SF5/SF6 | **12 symbols** recommended; lora-phy enforces it | 8 symbols | §6.1 sweep-loss argument and §7 airtime need redoing. Heartbeat ≈ 11.8 ms, 255 B ≈ 139 ms with 12-symbol preamble. |
| ToA, SF5/SF6 | own formula: 6.25-symbol sync overhead vs 4.25 for SF7+ | | Use the datasheet formula in the simulator and firmware airtime accounting. |
| Sensitivity, 125 kHz | SF7 −124 dBm, SF12 −137 dBm (boosted). **SF5 not listed** | −120 | Extrapolate ≈ −119 dBm; measure in Phase 2. |
| Co-channel rejection, same SF | SF7 5 dB, SF12 19 dB | | SF5 unlisted, likely lower. This is the capture threshold in the simulator. |
| Adjacent-channel rejection | 60 dB (SF7) and 72 dB (SF12) at **±1.5 × BW** offset | | 125 kHz spacing is 1.0 × BW: not characterised. Bench item C1 is real. |
| Blocking, SF12 | 88 / 90 / 99 dB at ±1 / 2 / 10 MHz | | |
| CAD thresholds, 4 symbols (SX1268 Table 13-73) | SF5 detPeak 18, detMin 10; SF6 19; SF7 20 … SF11 24 | | lora-phy's SF+13 matches at 8 symbols; the table is for 4. |
| CAD duration | (N_sym + 0.5 to 1) × T_sym + 32/BW; SF5: ≈ 1.3 ms at 4 symbols | ≈ 1 ms | |
| Synthesizer hop | 30 µs (10 MHz step) | | |
| Switching | RX→FS 15 µs, FS→RX 41 µs, STBY_XOSC→RX 62 µs, RX→TX 92 µs, STBY_RC→RX 83 µs | | 13-channel sweep in ≈ 5 ms is plausible with bare SetRfFrequency; not with `prepare_for_rx` (full param re-send) |
| RX current, 125 kHz, DC-DC | 4.6 mA; boosted 5.3 mA; LDO 8.8 / 10.1 mA | 5.8 mA | |
| TCXO regulator start-up | ≤ 100 µs; driver allows 5–10 ms | | matters for sleep/wake, not for retune |
| GetRssiInst | opcode 0x15, dBm = −value/2, "during reception" | | settle time after SetRx not specified; measure |
| SetRxDutyCycle | rx and sleep periods in 15.625 µs units; preamble must span sleep + 2 × rx | ≥ 512 ms preamble | ≈ 2000 symbols at SF5; "Rx boosted must be re-applied" caveat (datasheet §9.6) |
| Image calibration band | 430–440 MHz = one band (0x6B, 0x6F) | | one calibration covers all 13 channels |
| Sync word | 16-bit register; public 0x3444, private 0x1424, arbitrary allowed | private | lora-phy `set_lora_sync_word(u16)` |

## 4. Regulatory

- **ERC/REC 70-03, edition June 2026** (amended FM#113, 5 June 2026), Annex 1: band **g1**
  433.05–434.79 MHz, 10 mW e.r.p., ≤ 10% duty cycle, channel spacing not specified, EN 300
  220. g2: same band, 1 mW, no duty-cycle requirement, −13 dBm/10 kHz density. g3:
  434.04–434.79 MHz, 10 mW, no duty cycle, ≤ 25 kHz channels. The design cites "band f1",
  which is the 2011 labelling. Update §3 and §17.
- **Ukraine, NCEC decision 81 of 12.02.2025, annex row РІ 50-3-1** (section Р10): 433.04–
  434.79 MHz, ЕВП ≤ 10 мВт, робочий цикл < 10 %, EN 300 220-2 / ДСТУ EN 300 220-2, general
  authorisation, integral or dedicated antenna. Confirmed verbatim from the annex PDF.
  РІ 50-3-2 is the 0 dBm no-duty-cycle row. Decision 212 of 06.05.2026 restates sections
  6, 10 and 12; its annex PDF, as extracted, contains no 433 MHz row, so the РІ 50-3-1 text
  above stands. Worth a second look at the published consolidated text.

## 5. Prior art worth reading

- **umsh** (darconeous/umsh, Apache/MIT): Rust, nRF52840 incl. T-Echo, lora-phy fork,
  TrouBLE + nrf-sdc, UF2 flashing, Ed25519/X25519/AES-SIV crypto, no_std layered crates.
  Its `docs/hardware/lilygo-techo-hardware.md` is the best T-Echo bring-up reference found.
- **cfr34k/t-echo-lora-aprs** (C, nRF5 SDK): works with the stock bootloader, combined
  SoftDevice+app UF2, 100 µA standby with BLE.
- **Meshtastic PR #11137**: a careful rework of CAD-based LBT across SX126x/127x/128x/LR11xx,
  with symbol-time formulas and the CAD_RX exit-mode handoff. Relevant to §9.3.
- **meshtastenstein-rs**: Rust Meshtastic on XIAO nRF52840 + Wio-SX1262, lora-phy +
  trouble; never run on nRF hardware; no licence.

## 6. Sources

- lora-rs: https://github.com/lora-rs/lora-rs (lora-phy/src/sx126x/mod.rs, lib.rs,
  mod_params.rs, examples/nrf52840), https://crates.io/crates/lora-phy
- TrouBLE: https://github.com/embassy-rs/trouble (examples/nrf52); nrf-sdc:
  https://github.com/alexmoon/nrf-sdc; crates.io version pages for trouble-host, nrf-sdc,
  nrf-softdevice, embassy-nrf
- Meshtastic variant: https://github.com/meshtastic/firmware/blob/master/variants/nrf52840/t-echo/variant.h
- umsh: https://github.com/darconeous/umsh (Cargo.toml, firmware/techo/{Cargo.toml,memory.x},
  scripts/firmware_image.py, docs/hardware/lilygo-techo-hardware.md)
- LILYGO: https://github.com/Xinyuan-LilyGO/T-Echo, https://lilygo.cc/products/t-echo-lilygo,
  https://lilygo.cc/en-us/products/t-echo-card, https://lilygo.cc/en-us/products/t-echo-lite,
  https://openelab.io/products/lilygo-t-echo-sotfrf-lora-sx1262
- Meshtastic bootloader docs: https://meshtastic.org/docs/getting-started/flashing-firmware/nrf52/update-nrf52-bootloader/
- RAK4631 EU433: https://store.rokland.com/products/rakwireless-rak4631l-eu433-mhz-core-module-lora-sx1262-111000
- SX1261/2 datasheet Rev 1.2: https://cdn.sparkfun.com/assets/6/b/5/1/4/SX1262_datasheet.pdf;
  SX1268 Rev 1.0 (CAD table 13-73): https://www.waveshare.com/w/upload/c/c4/SX1268_V1.0.pdf
- Semtech CAD note summary: https://news.rakwireless.com/channel-activity-detection-ensuring-your-lora-r-packets-are-sent/
- Meshtastic LBT PR: https://github.com/meshtastic/firmware/pull/11137
- ERC/REC 70-03 June 2026: https://docdb.cept.org/download/4980 (document page
  https://docdb.cept.org/document/related/693)
- NCEC 81/2025: https://nkek.gov.ua/npas/81-12-02-2025 (annex PDF linked there);
  NCEC 212/2026: https://nkek.gov.ua/npas/212-06-05-2026

# Vendored crates

## lora-phy

The SX1262 driver, copied from https://github.com/lora-rs/lora-rs, directory
`lora-phy`, at revision `7a9c2ec4f6f926cae053ea8f7095dcf77e5d4185` (2026-10-06).
Licensed MIT or Apache-2.0, at the user's choice; both licence texts are in the
directory.

It is a copy, not a git dependency, because the firmware needs a change the
library does not offer (roadmap D20). Changes from the upstream revision:

- `Cargo.toml`: `lora-modulation` and `lorawan-device`, which upstream takes from
  sibling directories, come from the same git revision.
- `src/sx126x/mod.rs`: two fields in `Config`. `tcxo_delay_us` replaces the fixed
  10 ms TCXO start (`BRD_TCXO_WAKEUP_TIME`). `standby_xosc` makes the chip idle in
  STDBY_XOSC instead of STDBY_RC once its clock is configured, end TX and RX there
  (SetRxTxFallbackMode), and return there after image calibration, which runs from
  STDBY_RC. With `standby_xosc: false` and `tcxo_delay_us: 10_000` the driver sends
  the same commands as upstream; the upstream tests pass with those values.
- `src/sx126x/mod.rs`, duty-cycled receive (`RxMode::DutyCycle`): the chip is put in
  STDBY_RC before SetRxDutyCycle, the mode the datasheet starts the cycle from
  (§13.1.7), and the receive timer is not stopped at a preamble, so that the chip's own
  bound on the wait for the header applies (§13.1.6, §13.1.7). Other receive modes send
  the same commands as before.

- `src/sx126x/mod.rs`, an `impl Sx126x` block of one-command methods (`standby`,
  `tcxo`, `device_errors`, `rx_continuous`, `start_cad`, `irq_status`, ...) and
  `iv_mut`, for a caller that keeps track of what the chip holds and sends only what
  changes (`bark-driver/src/sx1262.rs`, roadmap D88). They add to the crate and change
  nothing in it.

To move to a newer upstream revision: replace the directory with that revision's
`lora-phy`, reapply the changes listed here, and review the upstream diff as for
any driver update.

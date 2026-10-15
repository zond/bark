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

To move to a newer upstream revision: replace the directory with that revision's
`lora-phy`, reapply the changes listed here, and review the upstream diff as for
any driver update.

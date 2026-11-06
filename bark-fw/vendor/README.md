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

## trouble-host

The BLE host, version 0.8.0 as published on crates.io, which is
https://github.com/embassy-rs/trouble, directory `host`, at revision
`42e3e04de00db3951b126741e1f3602bfdf5df47`. Licensed MIT or Apache-2.0, at the
user's choice; both licence texts, from that repository, are in the directory.

It is a copy because its pairing compiles the central's side even in a build
without the `central` feature, 6 KB the image has no room for
(`docs/pairing-2026-11-06.md`; roadmap D135). Changes from the published crate:

- `src/security_manager/pairing/mod.rs` and `src/security_manager/mod.rs`: the
  central's pairing (`pairing::central`, `State::Central`, `new_central`,
  `initiate_central`, `handle_central`) only with the `central` feature. Without
  it, an SMP command on a link where the node is central, or a pairing the node
  would start as central, is `Error::InvalidState`; the node is never central.
- `src/security_manager/crypto.rs` and `Cargo.toml`: AES-128, the security
  function `e` (Bluetooth Core Vol 3 Part H §2.2.1), is a function the
  application defines, `trouble_host_aes128`; the firmware's runs on the
  chip's AES block (`src/ble.rs`). AES-CMAC, on which f4, f5, f6 and g2
  (§2.2.5 to §2.2.9) and the GATT database hash are built, is written out
  over it as RFC 4493 gives it. The `aes` and `cmac` crates leave the
  dependencies; `aes` stays for the tests, which define the function in
  software and check the CMAC against RFC 4493's examples (§4) and the
  pairing functions against the specification's sample data. 5 616 B of
  flash less.
- The unit tests of `pairing` and `pairing::peripheral` pair against the
  central's side, so they run only with `central`.

The tests: `cargo test --lib --features security` runs all 62, and
`cargo test --lib --no-default-features --features
peripheral,gatt,derive,default-packet-pool,security` the 48 without
`central`. The integration tests in `tests/` do not build with
`security`, as published.

To move to a newer version: replace the directory with that version's published
crate, reapply the changes listed here, and run its tests both ways.

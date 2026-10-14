# Roadmap and decisions

**Status:** 2026-10-14, pre-code. Spec: `hrafnar/docs/civilian-433-mesh-design.md`
(referred to below as "the design"; section numbers are its sections).

## Decisions

| # | Decision | Why |
|---|---|---|
| D1 | **No dependency on the darkmap crate.** Copy the pieces we need into `bark-proto` and adapt them. | Darkmap's `Node` is built around gossip, a global table, the UKF and a 4-byte node ID that keys every table; the design removes all four and makes the 7-byte position the relay identity. Feature-gating would leave the wrong types in the API. Darkmap also pins embassy-time 0.4 / embassy-sync 0.6, while every current radio and BLE crate is on 0.5 / 0.8. A copy lets bark evolve without pulling darkforest along. |
| D2 | **Simulator first.** Before any firmware, build a discrete-event simulator that places relays and leaves on a grid, takes a sparse pairwise signal-power table (only pairs in range), and runs traffic through the real protocol core. | The design's capacity numbers (§13) are order-of-magnitude estimates with 2× uncertainty on hearing radius and hop count. The thresholds in O3 and the eviction policy in O4 can only be tuned against a model. Boards arrive in three weeks; the simulator is what we can do now and what we will keep using after. |
| D3 | **Protocol core is sans-IO** (`bark-proto`): a state machine driven by `(event, now)` that returns actions, no executor, no async, no radio driver. Firmware wraps it in an embassy task; the simulator drives it from an event queue. | One implementation under test in both places. Thousands of nodes in one process need a core that does not own tasks or channels. (Proposed; see open question Q1.) |
| D4 | `lora-phy` from the lora-rs **git main**, not crates.io 3.0.1 (July 2024). | SF5, `get_rssi`, arbitrary sync word, `rx_switch_channel`, RX duty cycle all landed after 3.0.1. Pin a revision. |
| D5 | BLE via **TrouBLE + nrf-sdc**, not nrf-softdevice. | nrf-softdevice last released January 2024; trouble-host 0.8 and nrf-sdc 0.4 both released August 2026 with GATT, notifications and security. Proven on a T-Echo by the umsh project. |
| D6 | **Keep the stock bootloader.** Application linked at `0x26000`, flashed as UF2 (family `0xADA52840`, volume `TECHOBOOT`). | The T-Echo ships Adafruit's UF2 bootloader with SoftDevice S140 6.1.1 occupying `0x1000..0x26000`. Replacing it needs SWD and buys nothing. nrf-sdc does not use the SoftDevice; the slot simply stays reserved. |
| D7 | Repository is `bark`; crates `bark-proto`, `bark-sim`, `bark-fw` in one workspace. | Answers design open item O9. |

## Phases

### Phase 0: skeleton and protocol core (now)

- Cargo workspace, CI for host tests.
- `bark-proto`: wire codecs for every packet in design §14 (heartbeat, ACK, ATTACH, DATA, NACK, DELIVER), the 7-byte position codec (§7), local tangent-plane distance (§11), neighbour table, leaf table, delivery queue, recent-RX window, busy estimator and home-channel rule (§6), heartbeat scheduler (§7), per-hop transfer state machine (§9.3), duty-cycle budget (§3).
- Copy from `darkforest/darkmap`: `collections/visited.rs`, `collections/recent_rx.rs`, the shape of `collections/neighbor_table.rs` (re-keyed by position), `routing.rs` (`RoutingContext::decide`, `select_best`, `nearest_to`). Leave behind: `positioning`, gossip, `global_table`, `messages.rs`, `sim/executor.rs`.
- Exit criterion: every codec round-trips under property tests; a two-node unit test exchanges heartbeat, ATTACH, DATA, ACK through the core with a fake clock.

### Phase 1: simulator (until boards arrive, ~2026-11-04)

See `simulator-design.md`. Milestones:

1. Three relays in a line, one leaf: greedy forward, ACK, demotion when the middle relay dies. Mirrors bench step 4 so the same scenario later runs on hardware.
2. One km² block, ~10 relays, ~80 leaves: attach, keepalive, delivery, queue, NACK (bench step 5).
3. Channel selection: inject a busy channel, confirm avoidance and no oscillation among relays that all hear each other (bench step 3).
4. Capacity: a 28 km² collision domain, ~270 relays, 20k leaves, texts at the §13 rates. Measure delivery ratio, latency, per-channel busy fraction, per-relay airtime, where the 10% cap binds first.
5. Sweep the unknowns: adjacent-channel rejection at 125 kHz spacing (bench item C1), hearing radius, SF5 sensitivity, heartbeat interval. Produce the table that replaces §13.

### Phase 2: bring-up (boards in hand)

Design §15 steps 1 to 3 on real boards with `lora-phy` git: TX/RX at SF5, CAD, RSSI read, retune time, sweep time, noise floor, adjacent-channel rejection with two boards and attenuators. Every measurement goes back into the simulator parameters. Toolchain: `thumbv7em-none-eabihf` target, a UF2 converter, defmt over USB-CDC (SWD pad location unconfirmed).

### Phase 3: protocol on hardware

Steps 4 and 5 with four boards (three relays in a line plus one leaf), then BLE GATT and a minimal phone app (Flutter; flutter_rust_bridge is already installed here). Field test needs more boards or RAK4631 EU433 relays (research doc, procurement).

## Open questions for discussion

- **Q1 (D3).** Sans-IO core versus Darkmap's async `Node::run` over embassy channels. Sans-IO costs an explicit timer-wheel in the core and a thin adapter in firmware; async costs a mini-executor in the simulator (darkmap has one, 932 lines) and makes 20k simulated leaves expensive. Recommendation: sans-IO.
- **Q2.** Link model fidelity. Start with a user-supplied sparse pairwise RX-power table plus a few generators (log-distance with exponent, rooftop bonus, indoor wall loss). Terrain-class models like darkmap's `sim/radio.rs` can be one generator but should not be the primary interface.
- **Q3.** Leaf behaviour model: static, random walk, or trace-driven? Phones in pockets go dark for minutes; that drives the delivery-queue sizing (§10.4).
- **Q4.** Whether the simulator exercises the real wire format (bytes through the codec) or passes structs. Recommendation: real bytes, so a codec bug is a simulator failure too, and airtime is computed from actual lengths.
- **Q5.** The design's 8-symbol preamble assumption is wrong for SF5 (12 symbols minimum, enforced by the driver). §6.1's sweep-loss argument and §7's heartbeat airtime need a pass. Who updates the design doc, and when?
- **Q6.** Scenario file format: Rust builder only, or a TOML/RON file so scenarios can be shared and diffed? Recommendation: both, with the file format as the stable interface.

## Timeline

| When | What |
|---|---|
| 2026-10-14 | This document. |
| Oct 2026 | Phase 0 and simulator milestones 1 to 3. |
| late Oct 2026 | Capacity scenario and parameter sweeps; revise §13. |
| ~2026-11-04 | Four T-Echo 433 arrive. Phase 2. |
| Nov 2026 | Phase 3 on four boards. Decide procurement for a dozen-relay field test. |

# bark

Firmware, protocol core and simulator for a civilian resilience mesh on
433 MHz LoRa: off-the-shelf LILYGO T-Echo nodes (nRF52840 + SX1262) that let
phone apps message each other across a city when ordinary infrastructure is
gone. Licence-free in the EU and Ukraine (10 mW effective radiated power,
10% duty cycle).

The name is from Darwin's bark spider (*Caerostris darwini*), the Madagascan
spider that builds the largest orb webs known, anchored on bridge lines up to
25 m long across rivers, out of the toughest biological silk measured. A big
web from small, cheap threads.

## Status (2026-10-14)

Skeleton and core interface in place; protocol core and simulator in
progress. Four T-Echo boards are on order, expected around 2026-11-04.
Until they arrive the work is the simulator.

Read, in order:

1. `docs/protocol.md`: the specification. Radio, channels, neighbour
   discovery, routing, leaves, packet formats, capacity, open items.
2. `docs/roadmap.md`: phases, decisions taken, open questions.
3. `docs/core-interface.md`: the contract between the protocol core and
   its drivers (simulator, firmware).
4. `docs/simulator-design.md`: the capacity simulator we build first.
5. `docs/implementation-research-2026-10-14.md`: what exists in the Rust
   ecosystem, hardware facts, and corrections to the design doc.
6. `docs/reception-research-2026-10-15.md`: how the simulator decides
   that a packet near sensitivity is lost.

## Layout

```
bark-proto/   no_std protocol core: packet codecs, tables, routing, MAC state
              machine. No I/O of its own: no executor, no radio driver, time
              passed in, no allocation. docs/core-interface.md is its contract.
bark-sim/     discrete-event simulator and scenario runner (std, host).
bark-fw/      T-Echo firmware: embassy-nrf + lora-phy + TrouBLE around
              bark-proto. Its own Cargo workspace, cross-compiled.
docs/         design notes, research, decisions.
```

Each relay forwards a message to the neighbour closest to the destination's
position, backtracks around gaps, and expects an acknowledgement from every
hop. The protocol core has no external routing dependency (roadmap D1).

## Building

Host (core and simulator), stable Rust:

```
cargo test --workspace
```

Firmware, from `bark-fw/` (its own workspace, so the host workspace never
cross-compiles):

```
rustup target add thumbv7em-none-eabihf
cd bark-fw && cargo build --release
```

See `bark-fw/README.md` for packaging the UF2 and flashing.

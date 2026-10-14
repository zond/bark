# bark

Firmware, protocol core and simulator for a civilian resilience mesh on
433 MHz LoRa: off-the-shelf LILYGO T-Echo nodes (nRF52840 + SX1262) that let
phone apps message each other across a city when ordinary infrastructure is
gone. Licence-free in the EU and Ukraine (10 mW ERP, 10% duty cycle).

The name is from Darwin's bark spider (*Caerostris darwini*), the Madagascan
spider that builds the largest orb webs known, anchored on bridge lines up to
25 m long across rivers, out of the toughest biological silk measured. A big
web from small, cheap threads.

## Status (2026-10-14)

Pre-code. The specification is `civilian-433-mesh-design.md` in the hrafnar
repo (`~/projects/hrafnar/docs/`). Four T-Echo boards are on order, expected
around 2026-11-04. Until they arrive the work is the simulator.

Read, in order:

1. `docs/roadmap.md`: phases, decisions taken, open questions.
2. `docs/simulator-design.md`: the capacity simulator we build first.
3. `docs/implementation-research-2026-10-14.md`: what exists in the Rust
   ecosystem, hardware facts, and corrections to the design doc.

## Layout (planned)

```
bark-proto/   no_std protocol core: packet codecs, tables, routing, MAC state
              machine. Sans-IO: no executor, no radio driver, explicit time.
bark-sim/     discrete-event simulator and scenario runner (std, host).
bark-fw/      T-Echo firmware: embassy-nrf + lora-phy + TrouBLE around bark-proto.
docs/         design notes, research, decisions.
```

Routing is a reduced Darkmap. The code is copied in and adapted, not
depended upon; see roadmap decision D1.

## Building

Nothing to build yet. Host toolchain: stable Rust (1.98 on the dev machine).
Firmware will need `rustup target add thumbv7em-none-eabihf` and a UF2
converter; see the research doc.

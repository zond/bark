//! Discrete-event simulator for the bark mesh.
//!
//! The simulator drives the real protocol core (`bark_proto::Relay`)
//! through the contract in `docs/core-interface.md`, with
//! a radio model built from SX1262 datasheet numbers (`phy`), a sparse
//! time-varying link table (`links`), and per-reason accounting of every
//! packet a node could have heard (`metrics`). `docs/simulator-design.md` is
//! the specification.

#![forbid(unsafe_code)]

mod battery;
pub mod boots;
pub mod geo;
pub mod links;
pub mod metrics;
pub mod phy;
mod power;
mod queue;
mod radio;
pub mod rng;
pub mod scenario;
pub mod sim;
pub mod summary;
mod trace;
mod walk;

pub use scenario::Scenario;
pub use sim::Sim;

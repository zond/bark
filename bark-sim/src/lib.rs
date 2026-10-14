//! Discrete-event simulator for the bark mesh.
//!
//! The simulator drives the real protocol cores (`bark_proto::Relay` and
//! `bark_proto::Leaf`) through the contract in `docs/core-interface.md`, with
//! a radio model built from SX1262 datasheet numbers (`phy`), a sparse
//! time-varying link table (`links`), and per-reason accounting of every
//! packet a node could have heard (`metrics`). `docs/simulator-design.md` is
//! the specification.

#![forbid(unsafe_code)]

pub mod geo;
pub mod links;
pub mod metrics;
pub mod phy;
mod radio;
pub mod rng;
pub mod scenario;
pub mod sim;
pub mod summary;

pub use scenario::Scenario;
pub use sim::Sim;

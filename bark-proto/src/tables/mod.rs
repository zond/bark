//! Bounded state the protocol core keeps. Every container has a capacity
//! fixed by the design and lives inline, so nothing here allocates
//! (roadmap D8).

pub mod visited;

mod fixed;
pub mod neighbours;
pub mod leaves;
pub mod delivery;
pub mod recent_rx;
pub mod budget;

//! Sans-IO protocol core for the bark mesh.
//!
//! The core is a state machine. A driver (firmware or simulator) feeds it
//! [`Event`]s stamped with the current [`Instant`] and collects the
//! [`Action`]s it emits. The core never blocks, never sleeps, never allocates
//! and never touches a radio; everything it needs from the world arrives as
//! an event. See `docs/core-interface.md` for the contract a driver must
//! honour, and `docs/roadmap.md` for the design decisions behind it.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[cfg(feature = "std")]
extern crate std;

pub mod event;
pub mod geo;
pub mod ids;
pub mod node;
pub mod position;
pub mod radio;
pub mod sniff;
pub mod tables;
pub mod time;
pub mod wire;

pub use event::{
    Action, AppCommand, AppEvent, Delivery, DropReason, Event, Frame, Trace, MAX_FRAME,
};
pub use ids::{MsgId, NodeId};
pub use node::{Config, Core, Load, Node, Relay, SleepyNode};
pub use position::{Position, ShortName};
pub use radio::{Channel, CHANNELS};
pub use time::{Duration, Instant};

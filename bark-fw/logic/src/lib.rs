//! The firmware's decisions, each a function of a plain value, so that
//! they are tested on the host: the firmware itself builds only for the
//! board (`bark-fw/README.md`, "Tasks and the stack").
//!
//! ```text
//! cd bark-fw && cargo test -p bark-fw-logic --target x86_64-unknown-linux-gnu
//! ```

#![cfg_attr(not(test), no_std)]

pub mod bench;
pub mod install;
pub mod line;
pub mod link;

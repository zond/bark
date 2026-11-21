//! A phone for a bark node on the command line: what the app does over
//! the phone link (spec "Phone link"), from a Linux computer's Bluetooth.
//! The transport is a trait ([`link`]); the tool's steps above it
//! ([`phone`]) are tested against a fake node.

#![forbid(unsafe_code)]

pub mod bluez;
#[cfg(test)]
mod fake;
pub mod link;
pub mod phone;
pub mod show;
pub mod state;

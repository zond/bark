//! What a node says of itself when a neighbour asks (spec "Neighbour status"): what
//! its screen shows, for a person looking at a map of the nodes around
//! theirs. The frames are in `wire.rs`; the node's side, the asking and
//! the answering, in `node/relay/status.rs`.

use crate::node::NeighbourCounts;

/// The battery as a node's screen says it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Battery {
    /// On external power, USB: the reading would be the charger's.
    Usb,
    /// On battery, not read yet.
    #[default]
    NotRead,
    /// At or below the level at which a fixed relay turns sleepy, about
    /// 20 % ([`crate::BatteryLevels::DESIGN`]).
    Low,
    /// About this much of its charge left, in percent, above the low
    /// level ([`crate::node::battery_percent`]).
    Percent(u8),
}

/// What a node's screen shows of its state (spec "Neighbour status").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// It has a place (spec "Position").
    pub placed: bool,
    /// It is sleepy (spec "Nodes").
    pub sleepy: bool,
    /// Sleepy, with an anchor that holds its mail (spec "Relay choice").
    pub anchored: bool,
    /// It is carried (spec "Nodes").
    pub carried: bool,
    /// Its power and battery.
    pub battery: Battery,
    /// Its live neighbours: heard, heard with margin, proven.
    pub neighbours: NeighbourCounts,
    /// The version of the firmware image it runs, 0 if not known
    /// ([`crate::Config::version`]).
    pub version: u32,
    /// The image it gathers over the mesh, all zero when none.
    pub gathering: Gathering,
}

/// How far a node is in gathering an image (spec "Neighbour status"), as
/// its heartbeats' section says it (spec "Updates over the mesh").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Gathering {
    /// The image's version, 0 when it gathers none.
    pub version: u32,
    /// The image's blocks, the table's among them.
    pub blocks: u8,
    /// The blocks it holds from the first.
    pub complete: u8,
    /// The pieces it holds of the block after them.
    pub have: u8,
}

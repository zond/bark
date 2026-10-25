//! What a node keeps across a reboot (spec "Implementation notes",
//! roadmap D73).
//!
//! The core says what to keep with [`crate::Action::Store`] whenever it
//! changes, and the driver hands the last one back at boot in
//! [`crate::Config::stored`]. Only slow-changing state is kept, so that
//! writes are rare: the home channel, the place a fixed node stands and
//! whether it is a map point, the carried flag, and a sleepy node's anchor. Time is not kept, since the
//! board has no clock that runs while it is off.
//!
//! [`Stored::record`] lays it out as a fixed 32-byte record with a
//! checksum, for a driver that keeps it in flash: eight words, the unit
//! the nRF52840 writes, and a record cut short by a power cut fails its
//! checksum and is not taken.

use crate::ids::NodeId;
use crate::node::Anchor;
use crate::position::Position;
use crate::radio::Channel;

/// The state a node keeps across a reboot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stored {
    /// Its home channel; a sleepy node's is its anchor's.
    pub home: Channel,
    /// The place it stands while fixed (spec "Position"), its position
    /// after the reboot; while carried, the place it stood when last
    /// fixed, since a place that changes with each fix is not kept.
    pub place: Position,
    /// Whether it is carried.
    pub carried: bool,
    /// Whether `place` is a map point a person set in the app, not a fix.
    pub map_point: bool,
    /// A sleepy node's anchor, whose home channel is `home`.
    pub anchor: Option<Anchor>,
}

/// Bytes in a [`Stored::record`].
pub const RECORD_LEN: usize = 32;

/// The layout's version, the record's first byte: a record of another
/// layout is not taken.
const VERSION: u8 = 1;

const FLAG_CARRIED: u8 = 0x1;
const FLAG_ANCHOR: u8 = 0x2;
const FLAG_MAP_POINT: u8 = 0x4;

// Offsets in the record.
const AT_VERSION: usize = 0;
const AT_HOME: usize = 1;
const AT_FLAGS: usize = 2;
const AT_PLACE: usize = 3;
const AT_ANCHOR_ID: usize = AT_PLACE + Position::WIRE_LEN;
const AT_ANCHOR_POS: usize = AT_ANCHOR_ID + NodeId::LEN;
const AT_CHECK: usize = RECORD_LEN - 4;
const _: () = assert!(AT_ANCHOR_POS + Position::WIRE_LEN <= AT_CHECK);

/// 32-bit FNV-1a, as the message ID uses (`ids.rs`).
fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5, |h, &b| {
        (h ^ b as u32).wrapping_mul(0x0100_0193)
    })
}

impl Stored {
    /// The fixed-size record, its last four bytes a checksum of the rest.
    pub fn record(&self) -> [u8; RECORD_LEN] {
        let mut r = [0u8; RECORD_LEN];
        r[AT_VERSION] = VERSION;
        r[AT_HOME] = self.home.index();
        let anchor = self.anchor.map_or(0, |_| FLAG_ANCHOR);
        let flag = |set: bool, bit: u8| if set { bit } else { 0 };
        r[AT_FLAGS] =
            anchor | flag(self.carried, FLAG_CARRIED) | flag(self.map_point, FLAG_MAP_POINT);
        r[AT_PLACE..AT_ANCHOR_ID].copy_from_slice(&self.place.encode());
        if let Some(a) = self.anchor {
            r[AT_ANCHOR_ID..AT_ANCHOR_POS].copy_from_slice(&a.id.0);
            r[AT_ANCHOR_POS..AT_ANCHOR_POS + Position::WIRE_LEN]
                .copy_from_slice(&a.position.encode());
        }
        let check = fnv1a(&r[..AT_CHECK]);
        r[AT_CHECK..].copy_from_slice(&check.to_be_bytes());
        r
    }

    /// The state in `r`, if it is a whole record of this layout.
    pub fn from_record(r: &[u8; RECORD_LEN]) -> Option<Stored> {
        let check = u32::from_be_bytes([
            r[AT_CHECK],
            r[AT_CHECK + 1],
            r[AT_CHECK + 2],
            r[AT_CHECK + 3],
        ]);
        if r[AT_VERSION] != VERSION || fnv1a(&r[..AT_CHECK]) != check {
            return None;
        }
        let pos = |at: usize| {
            let mut b = [0u8; Position::WIRE_LEN];
            b.copy_from_slice(&r[at..at + Position::WIRE_LEN]);
            Position::decode(&b)
        };
        let flags = r[AT_FLAGS];
        let anchor = (flags & FLAG_ANCHOR != 0).then(|| {
            let mut id = [0u8; NodeId::LEN];
            id.copy_from_slice(&r[AT_ANCHOR_ID..AT_ANCHOR_POS]);
            Anchor {
                id: NodeId(id),
                position: pos(AT_ANCHOR_POS),
            }
        });
        Some(Stored {
            home: Channel::new(r[AT_HOME])?,
            place: pos(AT_PLACE),
            carried: flags & FLAG_CARRIED != 0,
            map_point: flags & FLAG_MAP_POINT != 0,
            anchor,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored() -> Stored {
        Stored {
            home: Channel::new(7).unwrap(),
            place: Position::from_degrees(50.45, 30.52),
            carried: true,
            map_point: false,
            anchor: Some(Anchor {
                id: NodeId([1, 2, 3, 4, 5, 6]),
                position: Position::from_degrees(50.46, 30.51),
            }),
        }
    }

    #[test]
    fn a_record_round_trips() {
        let s = stored();
        assert_eq!(Stored::from_record(&s.record()), Some(s));
        let pinned = Stored {
            carried: false,
            map_point: true,
            ..s
        };
        assert_eq!(Stored::from_record(&pinned.record()), Some(pinned));
        let bare = Stored {
            anchor: None,
            carried: false,
            ..s
        };
        assert_eq!(Stored::from_record(&bare.record()), Some(bare));
    }

    /// Erased flash, a record cut short and one of another layout are
    /// not taken.
    #[test]
    fn a_damaged_record_is_not_taken() {
        assert_eq!(Stored::from_record(&[0xff; RECORD_LEN]), None);
        let r = stored().record();
        for cut in [8, 16, 24] {
            let mut partial = [0xff; RECORD_LEN];
            partial[..cut].copy_from_slice(&r[..cut]);
            assert_eq!(Stored::from_record(&partial), None, "cut at {cut}");
        }
        let mut other = r;
        other[AT_VERSION] = VERSION + 1;
        assert_eq!(Stored::from_record(&other), None);
    }
}

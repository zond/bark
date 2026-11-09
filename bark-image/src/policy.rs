//! What the firmware decides of an image spreading over the mesh (spec
//! "Updates over the mesh"; `bark-fw/README.md`, "Updates"): whether it
//! gathers an image a neighbour holds, and when it may reboot into one it
//! gathered. Pure, so that the host tests them.

use crate::sign::{Key, NONE, SLOTS};

/// What the node knows when a neighbour's image is offered to it.
#[derive(Clone, Copy, Debug)]
pub struct Offer {
    /// The image's version.
    pub version: u32,
    /// The key the neighbour's section says signed it.
    pub key: u8,
    /// The running image's version.
    pub running: u32,
    /// The staged slot is the phone's: its update is staged or
    /// committed, or a swap is asked for.
    pub slot_taken: bool,
}

/// Whether the node gathers the image: above the version it runs, signed
/// by a key it holds, and while nothing else holds the staged slot. The
/// key is the section's word, checked against the header's signatures
/// once the image is whole; a node gathers nothing that says it was
/// signed by a key it cannot check.
pub fn takes(offer: Offer, keys: &[Key; SLOTS]) -> bool {
    offer.version > offer.running
        && offer.key != NONE
        && keys.iter().any(|k| k.id == offer.key)
        && !offer.slot_taken
}

/// What the node is doing when an image it gathered is checked and
/// staged, and it would reboot into it.
#[derive(Clone, Copy, Debug)]
pub struct Moment {
    /// Phone links open.
    pub links: usize,
    /// It holds a message a reset would lose: in its send queue, its
    /// delivery queue or its inbox, or one its mailbox is writing.
    pub holding: bool,
    /// The phone's update is under way.
    pub phone_update: bool,
    /// A swap may start on its power: on USB, or on a battery above the
    /// low level (roadmap D143).
    pub swap_allowed: bool,
}

/// Whether the node reboots into the staged image now: with no phone
/// connected, which would lose its link, no message held, which would be
/// lost, outside a phone's update, and with the power a swap needs.
pub fn quiet(m: Moment) -> bool {
    m.links == 0 && !m.holding && !m.phone_update && m.swap_allowed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys() -> [Key; SLOTS] {
        let mut k = [Key::NONE; SLOTS];
        k[0].id = 1;
        k[1].id = 2;
        k
    }

    fn offer() -> Offer {
        Offer {
            version: 8,
            key: 2,
            running: 7,
            slot_taken: false,
        }
    }

    #[test]
    fn a_newer_image_by_a_key_held_is_taken() {
        assert!(takes(offer(), &keys()));
        // A bench image, version 0, takes any signed version.
        assert!(takes(
            Offer {
                version: 1,
                running: 0,
                ..offer()
            },
            &keys()
        ));
    }

    #[test]
    fn an_image_not_newer_is_not_taken() {
        for version in [6, 7] {
            assert!(!takes(Offer { version, ..offer() }, &keys()));
        }
    }

    #[test]
    fn an_image_by_a_key_not_held_is_not_taken() {
        assert!(!takes(Offer { key: 3, ..offer() }, &keys()));
        assert!(!takes(
            Offer {
                key: NONE,
                ..offer()
            },
            &keys()
        ));
        assert!(!takes(offer(), &[Key::NONE; SLOTS]));
    }

    #[test]
    fn nothing_is_taken_while_the_phone_holds_the_slot() {
        let busy = Offer {
            slot_taken: true,
            ..offer()
        };
        assert!(!takes(busy, &keys()));
    }

    fn moment() -> Moment {
        Moment {
            links: 0,
            holding: false,
            phone_update: false,
            swap_allowed: true,
        }
    }

    #[test]
    fn a_quiet_moment_has_nothing_under_way() {
        assert!(quiet(moment()));
        assert!(!quiet(Moment {
            links: 1,
            ..moment()
        }));
        assert!(!quiet(Moment {
            holding: true,
            ..moment()
        }));
        assert!(!quiet(Moment {
            phone_update: true,
            ..moment()
        }));
        assert!(!quiet(Moment {
            swap_allowed: false,
            ..moment()
        }));
    }
}

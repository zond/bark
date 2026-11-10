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
    /// What its phones' links are doing.
    pub links: Links,
    /// It drains and may reboot: its core holds nothing a reset would
    /// lose, or has drained for the bound, and what it holds then is
    /// kept across the reboot (`bark_proto::Relay::drained`).
    pub drained: bool,
    /// Its mailbox writes to its flash.
    pub writing: bool,
    /// The phone's update is under way.
    pub phone_update: bool,
    /// A swap may start on its power: on USB, or on a battery above the
    /// low level (roadmap D143).
    pub swap_allowed: bool,
}

/// What the node's phone links are doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Links {
    /// No phone is linked.
    None,
    /// Linked phones wait for the node, or it for them: nothing is
    /// half done.
    Idle,
    /// A transfer is mid-way on one: a request written and not yet
    /// answered, a notification not yet sent, or a pairing.
    MidWay,
}

/// Whether the node drains for the reboot, taking no message so that its
/// queues empty (roadmap D170): while a swap may start on its power and
/// no phone's update is under way. Otherwise it would refuse traffic for
/// as long as its battery is low or the phone's update runs, for a
/// reboot that cannot come; it takes messages again then.
pub fn drains(m: Moment) -> bool {
    m.swap_allowed && !m.phone_update
}

/// Whether the node reboots into the staged image now: drained, its
/// mailbox's writes done, no transfer with a phone mid-way, which would
/// be cut, and as long as it drains. A phone's standing link does not hold
/// it back (roadmap D170): the node ends it, and the app looks for its
/// node and connects again once it is back, so the person sees it out of
/// reach for the minute a swap takes; held back by it, a home node whose
/// owner is at home would never install.
pub fn quiet(m: Moment) -> bool {
    drains(m) && m.drained && !m.writing && m.links != Links::MidWay
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
            links: Links::None,
            drained: true,
            writing: false,
            phone_update: false,
            swap_allowed: true,
        }
    }

    #[test]
    fn a_quiet_moment_has_nothing_under_way() {
        assert!(drains(moment()));
        assert!(quiet(moment()));
    }

    #[test]
    fn a_standing_idle_link_does_not_hold_the_reboot_back() {
        assert!(quiet(Moment {
            links: Links::Idle,
            ..moment()
        }));
    }

    #[test]
    fn a_transfer_mid_way_holds_it_back() {
        assert!(!quiet(Moment {
            links: Links::MidWay,
            ..moment()
        }));
    }

    #[test]
    fn a_phone_update_holds_it_back_and_opens_the_node() {
        for links in [Links::None, Links::Idle] {
            let m = Moment {
                links,
                phone_update: true,
                ..moment()
            };
            assert!(!quiet(m) && !drains(m));
        }
    }

    #[test]
    fn a_message_held_within_the_bound_holds_it_back() {
        for links in [Links::None, Links::Idle] {
            let m = Moment {
                links,
                drained: false,
                ..moment()
            };
            assert!(!quiet(m) && drains(m));
        }
    }

    #[test]
    fn a_mailbox_write_holds_it_back() {
        assert!(!quiet(Moment {
            writing: true,
            ..moment()
        }));
    }

    #[test]
    fn a_low_battery_holds_it_back_and_opens_the_node() {
        for links in [Links::None, Links::Idle] {
            let m = Moment {
                links,
                swap_allowed: false,
                ..moment()
            };
            assert!(!quiet(m) && !drains(m));
        }
    }
}

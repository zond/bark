//! The phone link's decisions (spec "Phone link"): what a link may ask
//! of the attribute table, when the node takes another link, which link
//! is a guest's and when it ends, and what the links are doing for a
//! reboot (`bark_image::policy::quiet`).

use bark_image::policy::Links;

/// An ATT request as [`allowed`] judges it, by what it touches.
#[derive(Clone, Copy, Debug)]
pub enum Access<'a> {
    /// A read, a long read's part, a write or a prepared write of one
    /// handle, by request or command.
    One(u16),
    /// A read of several handles, as little-endian pairs.
    Several(&'a [u8]),
    /// A read by type over `start..=end`; `open_type` whether the type is
    /// an open value's or a subscription's.
    ByType {
        start: u16,
        end: u16,
        open_type: bool,
    },
    /// Discovery, and whatever else the host answers by its own rules.
    Other,
}

/// Whether a link may make a request touching `access`: a paired phone's
/// may make any the host allows; on a public node any link may; on a
/// private one, any link may but for the `open` handles, those the host
/// leaves open for a guest (spec "Phone link", Public). A read by type
/// reads an open value only by the type of one (Vol 3 Part F §3.4.4.1).
pub fn allowed(owner: bool, public: bool, open: &[u16], access: Access<'_>) -> bool {
    if owner || public {
        return true;
    }
    let touches = |h: u16| open.contains(&h);
    match access {
        Access::One(h) => !touches(h),
        Access::Several(handles) => !handles
            .as_chunks::<2>()
            .0
            .iter()
            .any(|h| touches(u16::from_le_bytes(*h))),
        Access::ByType {
            start,
            end,
            open_type,
        } => !open_type || !open.iter().any(|h| (start..=end).contains(h)),
        Access::Other => true,
    }
}

/// Whether the node takes another link with `links` up: one for each of
/// its `paired` phones and one more, `most` at all.
pub fn takes_link(links: usize, paired: usize, most: usize) -> bool {
    links < (paired + 1).min(most)
}

/// Whether a link is a guest's: on a public node, neither a paired
/// phone's nor one that pairs.
pub fn guest(public: bool, pairing: bool, owner: bool) -> bool {
    public && !pairing && !owner
}

/// When a link's task is to look again: when the turn ends, or a guest's
/// time does, whichever is first.
pub fn wake_at<T: Ord>(turn_ends: Option<T>, guest_ends: Option<T>) -> Option<T> {
    match (turn_ends, guest_ends) {
        (Some(t), Some(g)) => Some(t.min(g)),
        (t, g) => t.or(g),
    }
}

/// Another link, as [`second_guest`] sees it.
#[derive(Clone, Copy, Debug)]
pub struct Other {
    pub live: bool,
    pub owner: bool,
    /// It wrote a recipient tag.
    pub tagged: bool,
}

/// Whether a link that writes its recipient tag is a second guest, to be
/// closed: not a paired phone's, while another link is a guest's that
/// wrote its tag. The other links are the paired phones' to take.
pub fn second_guest(owner: bool, others: impl IntoIterator<Item = Other>) -> bool {
    !owner && others.into_iter().any(|o| o.live && !o.owner && o.tagged)
}

/// A link, as [`doing`] sees it.
#[derive(Clone, Copy, Debug)]
pub struct Doing {
    pub live: bool,
    /// A reply or a notification is on its way to its phone, one waits
    /// to be sent, or a pairing runs on it.
    pub busy: bool,
}

/// What the links are doing: mid-way through what a reset would cut, a
/// request the node has not answered or whose answer is not yet sent
/// (`asking`), or a link that is busy; idle with a link up and none of
/// these, however long it stays; or no link at all.
pub fn doing(asking: bool, links: impl IntoIterator<Item = Doing> + Clone) -> Links {
    let live = || links.clone().into_iter().filter(|l| l.live);
    let mid_way = asking || live().any(|l| l.busy);
    match (mid_way, live().next()) {
        (true, _) => Links::MidWay,
        (false, Some(_)) => Links::Idle,
        (false, None) => Links::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPEN: [u16; 3] = [3, 7, 8];

    #[test]
    fn a_private_node_keeps_the_open_handles_from_a_link_that_is_no_phones() {
        let several = |h: &[u16]| -> Vec<u8> { h.iter().flat_map(|h| h.to_le_bytes()).collect() };
        let open_two = several(&[2, 7]);
        let closed_two = several(&[2, 9]);
        let by_type = |start, end, open_type| Access::ByType {
            start,
            end,
            open_type,
        };
        // (owner, public, access) -> allowed
        let table: [(bool, bool, Access<'_>, bool); 14] = [
            (false, false, Access::One(3), false),
            (false, false, Access::One(4), true),
            (false, false, Access::Several(&open_two), false),
            (false, false, Access::Several(&closed_two), true),
            (false, false, by_type(1, 5, true), false),
            (false, false, by_type(4, 6, true), true),
            (false, false, by_type(1, 0xffff, false), true),
            (false, false, by_type(8, 8, true), false),
            (false, false, Access::Other, true),
            (true, false, Access::One(3), true),
            (true, false, by_type(1, 0xffff, true), true),
            (false, true, Access::One(3), true),
            (false, true, Access::Several(&open_two), true),
            (false, true, by_type(1, 0xffff, true), true),
        ];
        for (owner, public, access, want) in table {
            assert_eq!(
                allowed(owner, public, &OPEN, access),
                want,
                "owner {owner} public {public} {access:?}"
            );
        }
    }

    #[test]
    fn a_link_more_than_the_paired_phones_and_never_more_than_the_most() {
        // (links, paired) -> takes, of at most 9
        for (links, paired, takes) in [
            (0, 0, true),
            (1, 0, false),
            (1, 1, true),
            (2, 1, false),
            (8, 8, true),
            (8, 9, true),
            (9, 9, false),
            (9, 12, false),
        ] {
            assert_eq!(takes_link(links, paired, 9), takes, "{links} {paired}");
        }
    }

    #[test]
    fn a_guest_is_a_public_nodes_link_that_neither_pairs_nor_is_paired() {
        // (public, pairing, owner) -> guest
        for (public, pairing, owner, g) in [
            (true, false, false, true),
            (true, true, false, false),
            (true, false, true, false),
            (false, false, false, false),
        ] {
            assert_eq!(guest(public, pairing, owner), g);
        }
    }

    #[test]
    fn a_link_wakes_at_the_first_of_its_ends() {
        for (turn, guest, at) in [
            (None, None, None),
            (Some(5), None, Some(5)),
            (None, Some(7), Some(7)),
            (Some(5), Some(7), Some(5)),
            (Some(9), Some(7), Some(7)),
        ] {
            assert_eq!(wake_at(turn, guest), at);
        }
    }

    #[test]
    fn one_guest_with_a_tag_at_a_time() {
        let other = |live, owner, tagged| Other {
            live,
            owner,
            tagged,
        };
        // (owner, others) -> a second guest
        let table: [(bool, &[Other], bool); 6] = [
            (false, &[], false),
            (false, &[other(true, false, true)], true),
            (false, &[other(true, false, false)], false),
            (false, &[other(false, false, true)], false),
            (false, &[other(true, true, true)], false),
            (true, &[other(true, false, true)], false),
        ];
        for (owner, others, second) in table {
            assert_eq!(second_guest(owner, others.iter().copied()), second);
        }
    }

    #[test]
    fn the_links_are_mid_way_while_anything_would_be_cut() {
        let link = |live, busy| Doing { live, busy };
        // (asking, links) -> doing
        let table: [(bool, &[Doing], Links); 7] = [
            (false, &[], Links::None),
            (true, &[], Links::MidWay),
            (false, &[link(false, true)], Links::None),
            (false, &[link(true, false)], Links::Idle),
            (false, &[link(true, false), link(true, true)], Links::MidWay),
            (true, &[link(true, false)], Links::MidWay),
            (false, &[link(false, true), link(true, false)], Links::Idle),
        ];
        for (asking, links, want) in table {
            assert_eq!(doing(asking, links.iter().copied()), want);
        }
    }
}

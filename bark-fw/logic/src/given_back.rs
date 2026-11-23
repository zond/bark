//! Who sent each message the firmware hands the core, by the tag the core
//! hands back with a give-back (`docs/core-interface.md`, "Application"),
//! and what each phone was given back (spec "Phone link", Given back).

use bark_link::phones::RECORDS;
use bark_link::{seconds_left, GivenBack};
use bark_proto::{Instant, MsgId, Why};

/// Who sent a message, as its tag says. Every byte names one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sender {
    /// The phone of the paired record at this place: tags 0 to 7.
    Record(usize),
    /// The mailbox's reply (spec "Mailbox"): tag 8.
    Mailbox,
    /// The message the node held across a reboot into a new image
    /// (`carry.rs`), whose sender the flash does not keep: tag 9.
    Carried,
    /// A bench build's command (`bench.rs`): tag 10.
    Bench,
    /// A guest's link, by its number among the node's guest links, which
    /// wraps after the 245 tags from 11 on. A guest's link ends 30 s after
    /// it began (spec "Public nodes"), so a give-back reaches a later
    /// guest only after that many links, and then names an ID its app
    /// never sent.
    Guest(u8),
}

const MAILBOX: u8 = RECORDS as u8;
const CARRIED: u8 = MAILBOX + 1;
const BENCH: u8 = CARRIED + 1;
const FIRST_GUEST: u8 = BENCH + 1;

impl Sender {
    /// Its tag.
    pub const fn tag(self) -> u8 {
        match self {
            Sender::Record(i) => i as u8,
            Sender::Mailbox => MAILBOX,
            Sender::Carried => CARRIED,
            Sender::Bench => BENCH,
            Sender::Guest(g) => g,
        }
    }

    /// Whose `tag` is.
    pub const fn of(tag: u8) -> Sender {
        match tag {
            MAILBOX => Sender::Mailbox,
            CARRIED => Sender::Carried,
            BENCH => Sender::Bench,
            t if t < MAILBOX => Sender::Record(t as usize),
            t => Sender::Guest(t),
        }
    }
}

/// Whether a message of the node's own, the mailbox's reply or the one
/// carried across a reboot, is offered again after it was given back for
/// `why`: busy or with no neighbour once the node takes one, but not one
/// whose search found no way, which the relays that searched would take
/// for one they have had (spec "Mailbox", The answer; "Sending", Given
/// back).
pub const fn goes_again(why: Why) -> bool {
    !matches!(why, Why::NoWay)
}

/// `why` as the log says it, as `bark-cli` prints it.
pub const fn why_text(why: Why) -> &'static str {
    match why {
        Why::Busy => "busy",
        Why::NoNeighbour => "no-neighbour",
        Why::NoWay => "no-way",
    }
}

/// A message given back to a phone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Given {
    pub msg_id: MsgId,
    /// The end of a busy one's wait.
    pub retry_at: Instant,
    pub why: Why,
}

impl Given {
    /// Given back as the phone reads it at `now`: the wait counts down for
    /// busy and is 0 otherwise (spec "Sending", Given back).
    pub fn value(&self, now: Instant) -> GivenBack {
        GivenBack {
            msg_id: self.msg_id,
            wait_s: match self.why {
                Why::Busy => seconds_left(now, self.retry_at),
                Why::NoNeighbour | Why::NoWay => 0,
            },
            why: self.why,
        }
    }
}

/// A guest's link while it is up: its tag and what it was given back.
#[derive(Clone, Copy, Debug)]
struct GuestLink {
    tag: u8,
    given: Option<Given>,
}

/// What each phone was last given back, until the node takes another
/// message from it, over `N` links. A paired phone's is kept with its
/// record across its links, in RAM: a reboot forgets it, as it forgets the
/// message's record whose tag would name the phone. A guest's lasts as
/// long as its link.
#[derive(Clone, Copy, Debug)]
pub struct GivenBacks<const N: usize> {
    records: [Option<Given>; RECORDS],
    guests: [Option<GuestLink>; N],
    /// The tag the next guest link takes.
    next_guest: u8,
}

/// Which phone a give-back is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum To {
    /// The paired record's phone, if linked, notified now; else it reads
    /// it when it next links.
    Record(usize),
    /// The guest on the link in this slot.
    Guest(usize),
}

impl<const N: usize> Default for GivenBacks<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> GivenBacks<N> {
    /// Nothing given back, no guest.
    pub const fn new() -> Self {
        GivenBacks {
            records: [None; RECORDS],
            guests: [None; N],
            next_guest: FIRST_GUEST,
        }
    }

    /// A link came up in `slot`: as a guest's, until it turns out a paired
    /// phone's, it takes the next guest tag.
    pub fn linked(&mut self, slot: usize) {
        let tag = self.next_guest;
        self.next_guest = tag.checked_add(1).unwrap_or(FIRST_GUEST);
        self.guests[slot] = Some(GuestLink { tag, given: None });
    }

    /// The link in `slot` ended: a guest's give-back is dropped with it.
    pub fn ended(&mut self, slot: usize) {
        self.guests[slot] = None;
    }

    /// The record at `i` changed hands: what its phone was given back is
    /// not the new one's.
    pub fn record_changed(&mut self, i: usize) {
        self.records[i] = None;
    }

    /// The tag for a message from the link in `slot`, encrypted with
    /// `record`'s bond if a paired phone's; `None` for a link that is
    /// down.
    pub fn tag(&self, slot: usize, record: Option<usize>) -> Option<u8> {
        match record {
            Some(i) => Some(Sender::Record(i).tag()),
            None => self.guests[slot].map(|g| g.tag),
        }
    }

    /// The node took a message from that link: what it was given back
    /// before is over.
    pub fn taken(&mut self, slot: usize, record: Option<usize>) {
        match record {
            Some(i) => self.records[i] = None,
            None => {
                if let Some(g) = self.guests[slot].as_mut() {
                    g.given = None;
                }
            }
        }
    }

    /// The core gave back a message sent with `tag`: the phone to tell,
    /// or `None` for a guest whose link has ended, or a tag that names no
    /// phone.
    pub fn give_back(&mut self, tag: u8, given: Given) -> Option<To> {
        match Sender::of(tag) {
            Sender::Record(i) => {
                self.records[i] = Some(given);
                Some(To::Record(i))
            }
            Sender::Guest(t) => {
                let slot = self
                    .guests
                    .iter()
                    .position(|g| g.is_some_and(|g| g.tag == t))?;
                if let Some(g) = self.guests[slot].as_mut() {
                    g.given = Some(given);
                }
                Some(To::Guest(slot))
            }
            Sender::Mailbox | Sender::Carried | Sender::Bench => None,
        }
    }

    /// What the link in `slot` reads, as `record`'s phone or a guest.
    pub fn read(&self, slot: usize, record: Option<usize>) -> Option<Given> {
        match record {
            Some(i) => self.records[i],
            None => self.guests[slot].and_then(|g| g.given),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bark_proto::Duration;

    const LINKS: usize = RECORDS + 1;

    fn given(id: u32, why: Why) -> Given {
        Given {
            msg_id: MsgId(id),
            retry_at: Instant::ZERO + Duration::from_secs(30),
            why,
        }
    }

    #[test]
    fn every_tag_names_one_sender() {
        for tag in 0..=u8::MAX {
            assert_eq!(Sender::of(tag).tag(), tag);
        }
        assert_eq!(Sender::of(3), Sender::Record(3));
        assert_eq!(Sender::of(8), Sender::Mailbox);
        assert_eq!(Sender::of(9), Sender::Carried);
        assert_eq!(Sender::of(10), Sender::Bench);
        assert_eq!(Sender::of(11), Sender::Guest(11));
    }

    #[test]
    fn the_wait_counts_down_for_busy_alone() {
        let at = Instant::ZERO + Duration::from_millis(10_500);
        assert_eq!(given(1, Why::Busy).value(at).wait_s, 20);
        assert_eq!(given(1, Why::NoNeighbour).value(at).wait_s, 0);
        assert_eq!(given(1, Why::NoWay).value(at).wait_s, 0);
        assert_eq!(given(1, Why::NoWay).value(at).why, Why::NoWay);
    }

    #[test]
    fn a_paired_phones_reaches_its_record_whoever_sent_last() {
        let mut g = GivenBacks::<LINKS>::new();
        g.linked(0);
        g.linked(1);
        let first = g.tag(0, Some(2)).unwrap();
        g.taken(0, Some(2));
        // Another phone sends after it.
        let _ = g.tag(1, Some(5));
        g.taken(1, Some(5));
        assert_eq!(
            g.give_back(first, given(1, Why::NoWay)),
            Some(To::Record(2))
        );
        assert_eq!(g.read(0, Some(2)), Some(given(1, Why::NoWay)));
        assert_eq!(g.read(1, Some(5)), None);
    }

    #[test]
    fn a_paired_phones_is_kept_while_away_until_it_sends_again() {
        let mut g = GivenBacks::<LINKS>::new();
        g.linked(3);
        let tag = g.tag(3, Some(4)).unwrap();
        g.ended(3);
        assert_eq!(g.give_back(tag, given(1, Why::Busy)), Some(To::Record(4)));
        // A later one replaces it.
        assert_eq!(g.give_back(tag, given(2, Why::NoWay)), Some(To::Record(4)));
        // Read at its next link, in any slot.
        g.linked(6);
        assert_eq!(g.read(6, Some(4)), Some(given(2, Why::NoWay)));
        g.taken(6, Some(4));
        assert_eq!(g.read(6, Some(4)), None);
    }

    #[test]
    fn a_records_new_phone_reads_nothing_of_the_old() {
        let mut g = GivenBacks::<LINKS>::new();
        g.give_back(Sender::Record(1).tag(), given(1, Why::Busy));
        g.record_changed(1);
        assert_eq!(g.read(0, Some(1)), None);
    }

    #[test]
    fn a_guests_is_dropped_with_its_link_and_never_the_next_guests() {
        let mut g = GivenBacks::<LINKS>::new();
        g.linked(8);
        let tag = g.tag(8, None).unwrap();
        assert_eq!(g.give_back(tag, given(1, Why::Busy)), Some(To::Guest(8)));
        assert_eq!(g.read(8, None), Some(given(1, Why::Busy)));
        g.taken(8, None);
        assert_eq!(g.read(8, None), None);
        g.ended(8);
        // The next guest, in the same slot.
        g.linked(8);
        assert_ne!(g.tag(8, None), Some(tag));
        assert_eq!(g.give_back(tag, given(2, Why::NoWay)), None);
        assert_eq!(g.read(8, None), None);
    }

    #[test]
    fn guest_tags_wrap_past_the_others() {
        let mut g = GivenBacks::<LINKS>::new();
        let mut seen = std::vec::Vec::new();
        for _ in 0..300 {
            g.linked(0);
            let tag = g.tag(0, None).unwrap();
            assert!(matches!(Sender::of(tag), Sender::Guest(_)));
            seen.push(tag);
        }
        assert_eq!(seen[0], seen[245]);
        assert_ne!(seen[0], seen[244]);
    }

    #[test]
    fn the_nodes_own_go_again_but_for_no_way() {
        assert!(goes_again(Why::Busy));
        assert!(goes_again(Why::NoNeighbour));
        assert!(!goes_again(Why::NoWay));
        let mut g = GivenBacks::<LINKS>::new();
        for s in [Sender::Mailbox, Sender::Carried, Sender::Bench] {
            assert_eq!(g.give_back(s.tag(), given(1, Why::Busy)), None);
        }
    }
}

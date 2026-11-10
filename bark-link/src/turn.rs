//! Whose turn it is at the one message a node holds for its phones, and so
//! whose Status has ready set (spec "Phone link", Sending; roadmap D184).
//! The node keeps one [`Turn`] for its links; it lives here, beside the
//! values it acts on, so that the host tests it.

use bark_proto::tables::budget::BAN;
use bark_proto::{Duration, Instant};

/// How long a turn lasts unused while another phone wants it: a ban, the
/// shortest wait the protocol tells, as nothing in it acts sooner (spec
/// "Bans").
pub const UNUSED: Duration = BAN;

/// What the turn knows of the phone on one link.
#[derive(Clone, Copy, Debug, Default)]
struct Phone {
    /// A link is up.
    live: bool,
    /// It asked for the turn and has not had it since.
    asked: bool,
}

/// The turn among `N` links, by their slots.
#[derive(Clone, Copy, Debug)]
pub struct Turn<const N: usize> {
    phones: [Phone; N],
    /// The phone that has the turn.
    holder: Option<usize>,
    /// Since when it has had ready: the turn counts unused only while the
    /// node takes a message.
    since: Option<Instant>,
}

impl<const N: usize> Default for Turn<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Turn<N> {
    /// No links, and the turn nobody's.
    pub const fn new() -> Self {
        Turn {
            phones: [Phone {
                live: false,
                asked: false,
            }; N],
            holder: None,
            since: None,
        }
    }

    /// The phone whose ready is set while the node takes a message.
    pub fn holder(&self) -> Option<usize> {
        self.holder
    }

    /// A link came up in `slot`: its phone wants the turn once it asks.
    pub fn linked(&mut self, slot: usize) {
        self.phones[slot] = Phone {
            live: true,
            ..Phone::default()
        };
    }

    /// The link in `slot` ended.
    pub fn ended(&mut self, slot: usize) {
        self.phones[slot] = Phone::default();
    }

    /// The phone in `slot` wrote Send: if the turn is not its, it asks.
    pub fn ask(&mut self, slot: usize) {
        if self.holder != Some(slot) {
            self.phones[slot].asked = true;
        }
    }

    /// The node took the message of the phone in `slot`: if the turn is
    /// still its, it goes to the next phone that wants it, or stays. A
    /// write that waited for another phone's request may be taken after
    /// the turn passed on, and does not end the new holder's.
    pub fn taken(&mut self, slot: usize) {
        if self.holder == Some(slot) {
            self.pass();
        }
    }

    /// Move the turn on to `now`, `ready` saying whether the node takes a
    /// message: to a phone if none has it, and on from one that has had
    /// ready for [`UNUSED`] while another wants it. Whether it moved.
    pub fn advance(&mut self, now: Instant, ready: bool) -> bool {
        let before = self.holder;
        if before.is_none_or(|s| !self.phones[s].live) {
            self.pass();
        }
        if !ready {
            self.since = None;
        } else {
            let since = *self.since.get_or_insert(now);
            if now.since(since) >= UNUSED && self.others_want() {
                self.pass();
                self.since = Some(now);
            }
        }
        self.holder != before
    }

    /// When the turn next moves by itself: when the holder's time unused
    /// is over, if another phone wants it.
    pub fn ends(&self) -> Option<Instant> {
        self.since
            .filter(|_| self.others_want())
            .map(|t| t + UNUSED)
    }

    /// Whether the phone in `slot` wants the turn: it asked.
    fn wants(&self, slot: usize) -> bool {
        let p = &self.phones[slot];
        p.live && p.asked
    }

    /// The first phone after `after`, in the order of the slots and coming
    /// round to `after` itself last, that `f` takes.
    fn next(&self, after: Option<usize>, f: impl Fn(usize) -> bool) -> Option<usize> {
        let from = after.map_or(0, |a| a + 1);
        (from..from + N).map(|i| i % N).find(|&i| f(i))
    }

    fn others_want(&self) -> bool {
        self.next(self.holder, |i| self.wants(i) && Some(i) != self.holder)
            .is_some()
    }

    /// The holder's turn is over: to the next phone that wants it, else
    /// the holder keeps it, else, with no holder, the next phone linked.
    fn pass(&mut self) {
        let holder = self.holder.filter(|&s| self.phones[s].live);
        let to = self
            .next(self.holder, |i| self.wants(i))
            .or(holder)
            .or_else(|| self.next(self.holder, |i| self.phones[i].live));
        if let Some(s) = to {
            self.phones[s].asked = false;
        }
        self.holder = to;
        self.since = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: Instant = Instant::from_micros(1_000_000);

    fn at(n: u64) -> Instant {
        T0 + UNUSED.mul(n)
    }

    /// Two phones linked and idle.
    fn two() -> Turn<3> {
        let mut t = Turn::<3>::new();
        t.linked(0);
        t.linked(1);
        t.advance(T0, true);
        t
    }

    #[test]
    fn idle_phones_see_no_flips() {
        let mut t = two();
        assert_eq!(t.holder(), Some(0));
        for n in 1..100 {
            assert!(!t.advance(at(n), true));
        }
        assert_eq!(t.holder(), Some(0));
        assert_eq!(t.ends(), None);
    }

    #[test]
    fn a_phone_that_asks_gets_the_turn_and_the_other_does_not() {
        let mut t = two();
        t.ask(1);
        assert_eq!(t.ends(), Some(at(1)));
        assert!(!t.advance(at(1) - Duration::from_micros(1), true));
        assert!(t.advance(at(1), true));
        assert_eq!(t.holder(), Some(1));
        // It keeps it unused, as nobody else asks.
        for n in 2..10 {
            assert!(!t.advance(at(n), true));
        }
        assert_eq!(t.holder(), Some(1));
    }

    #[test]
    fn an_ask_after_a_long_idle_turn_moves_it_at_once() {
        let mut t = two();
        t.advance(at(5), true);
        t.ask(1);
        assert!(t.advance(at(5), true));
        assert_eq!(t.holder(), Some(1));
    }

    #[test]
    fn the_holder_asks_nothing() {
        let mut t = two();
        t.ask(0);
        assert_eq!(t.ends(), None);
    }

    #[test]
    fn two_asking_take_turns() {
        let mut t = Turn::<3>::new();
        for s in 0..3 {
            t.linked(s);
        }
        t.advance(T0, true);
        assert_eq!(t.holder(), Some(0));
        t.ask(1);
        t.ask(2);
        // The holder's message taken: on to the next that asked.
        t.taken(0);
        assert_eq!(t.holder(), Some(1));
        t.taken(1);
        assert_eq!(t.holder(), Some(2));
        // Nobody asks: the turn stays.
        t.taken(2);
        assert_eq!(t.holder(), Some(2));
        // 1 asks again, 2 leaves it unused for a ban.
        t.ask(1);
        t.advance(at(1), true);
        assert!(t.advance(at(2), true));
        assert_eq!(t.holder(), Some(1));
    }

    #[test]
    fn the_unused_time_counts_only_while_the_node_takes_one() {
        let mut t = two();
        t.ask(1);
        assert!(!t.advance(at(3), false));
        assert_eq!(t.ends(), None);
        assert!(!t.advance(at(4), true));
        assert!(t.advance(at(5), true));
    }

    #[test]
    fn a_phone_alone_has_the_turn_before_it_asks() {
        let mut t = Turn::<3>::new();
        t.linked(2);
        assert!(t.advance(T0, true));
        assert_eq!(t.holder(), Some(2));
    }

    /// The holder's write waited behind another phone's request, and the
    /// turn passed on meanwhile: its message taken late leaves the new
    /// holder its turn.
    #[test]
    fn a_message_taken_after_its_turn_passed_leaves_the_new_holders() {
        let mut t = two();
        t.ask(1);
        assert!(t.advance(at(1), true));
        assert_eq!(t.holder(), Some(1));
        t.ask(0);
        t.taken(0);
        assert_eq!(t.holder(), Some(1));
        t.taken(1);
        assert_eq!(t.holder(), Some(0));
    }

    #[test]
    fn a_link_that_ends_passes_the_turn() {
        let mut t = two();
        t.ended(0);
        assert!(t.advance(at(0), true));
        assert_eq!(t.holder(), Some(1));
        t.ended(1);
        assert!(t.advance(at(0), true));
        assert_eq!(t.holder(), None);
    }
}

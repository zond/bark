//! The leaf table (design §10.2): leaves attached to this relay, keyed by
//! [`LeafId`], each with its place in the relay's poll schedule.
//!
//! The relay polls one leaf at a time, at most one every `poll_interval`
//! (N), on a grid of slots N apart. A cycle is [`LEAVES`] slots, the
//! poll period P = 256 N, and each leaf owns one slot of the cycle, so it is
//! polled every P whatever the table holds, and a leaf that misses a poll
//! knows when the next one comes. A leaf that has not answered
//! [`Polls::missed`] polls in a row is dropped.
//!
//! When the table is full, an ATTACH from an unknown leaf is not answered,
//! so the leaf attaches elsewhere. At 256 entries against ≈80 expected
//! leaves per relay (§10.2) this is an overload case.
//!
//! §10.2 also lists an `rssi` field; nothing in the design reads it, so it
//! is not kept.

use crate::ids::LeafId;
use crate::node::Config;
use crate::radio::{time_on_air, MIN_PREAMBLE_SYMBOLS};
use crate::tables::fixed::FixedVec;
use crate::tables::LBT_GIVE_UP;
use crate::time::{Duration, Instant};
use crate::wire::{RxMode, ATTACHED_LEN};

/// Table capacity (design §10.2).
pub const LEAVES: usize = 256;

/// The relay polls at most one leaf this often (design §10.2: N =
/// 300 s / 256 = 1.171875 s).
///
/// A poll is one ATTACHED, 10.56 ms, so polls take at most 10.56 ms / N of
/// the relay's time however many leaves it has: 0.90 % at this N, 9.2 % of
/// the duty budget's 9.83 % (§9.4), and at the ≈80 leaves expected per
/// relay 2.9 % of the budget. The period P = 256 N is half the delivery
/// TTL (§10.4: 600 s), so that with [`Polls::missed`] = 3 the relay keeps a
/// leaf through any dark spell shorter than a TTL and does not keep it
/// past the life of its messages by more than a period. A longer N costs
/// less and makes a returning phone, a dead relay and a moved leaf known
/// later; one under 1.07 s would let a full table's polls take more than a
/// tenth of the budget.
pub const POLL_INTERVAL: Duration = Duration::from_micros(300_000_000 / LEAVES as u64);

/// How late after its slot a poll may start; one that cannot is skipped
/// and counts as missed (design §10.2). It is listen-before-talk's give-up
/// time (§9.3): a poll goes before a hand-over, so it waits at most for
/// one already under way, a full frame and its ACK timeout, 238.6 ms, and
/// for an owed ACK, 114.4 ms, which leaves 147 ms of channel checks.
pub const POLL_LATE: Duration = LBT_GIVE_UP;

/// Airtime of a poll: one ATTACHED (§14).
pub const POLL_AIRTIME: Duration = time_on_air(ATTACHED_LEN as u8, MIN_PREAMBLE_SYMBOLS);

/// The poll schedule's parameters, from a configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Polls {
    /// N: the grid of slots.
    pub interval: Duration,
    /// P = [`LEAVES`] × N: how often each leaf is polled.
    pub period: Duration,
    /// Polls in a row a leaf may leave unanswered before it is dropped,
    /// and a leaf may miss before it gives its relay up: ⌈TTL / P⌉ + 1, so
    /// that a leaf dark for less than a delivery TTL keeps its relay. The
    /// leaf answers the first poll after it is back; if it went dark just
    /// before a poll, the polls it misses span less than (missed − 1) P.
    pub missed: u8,
    /// How long after its slot the relay waits for the answer to a poll:
    /// the poll's lateness and airtime, and the leaf's reply time
    /// (`ack_timeout`, §9.3).
    pub reply_wait: Duration,
}

impl Polls {
    /// The schedule for `config`.
    pub const fn from_config(config: &Config) -> Polls {
        let period = config.poll_interval.mul(LEAVES as u64);
        let ttl = config.delivery_ttl.as_micros();
        let p = if period.as_micros() == 0 {
            1
        } else {
            period.as_micros()
        };
        let missed = ttl.div_ceil(p) + 1;
        Polls {
            interval: config.poll_interval,
            period,
            missed: if missed > u8::MAX as u64 {
                u8::MAX
            } else {
                missed as u8
            },
            reply_wait: Duration::from_micros(
                POLL_LATE.as_micros() + POLL_AIRTIME.as_micros() + config.ack_timeout.as_micros(),
            ),
        }
    }
}

/// One attached leaf.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeafEntry {
    /// The leaf.
    pub leaf: LeafId,
    /// Its slot: when it is next polled. Always on the grid.
    pub next_poll: Instant,
    /// Its declared receive mode.
    pub mode: RxMode,
    /// Its last DELIVER went unanswered: it is not listening, and its
    /// messages wait for its next answer to a poll or the delivery retry
    /// (§10.4).
    pub missed: bool,
    /// Polls sent or skipped since it last answered.
    pub unanswered: u8,
}

/// The table is full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;

/// The leaf table.
#[derive(Clone)]
pub struct Leaves {
    entries: FixedVec<LeafEntry, LEAVES>,
    polls: Polls,
}

impl Leaves {
    /// An empty table on the schedule of `config`.
    pub const fn from_config(config: &Config) -> Leaves {
        let fill = LeafEntry {
            leaf: LeafId::RELAY,
            next_poll: Instant::ZERO,
            mode: RxMode::Listening,
            missed: false,
            unanswered: 0,
        };
        Leaves {
            entries: FixedVec::new(fill),
            polls: Polls::from_config(config),
        }
    }

    /// The schedule.
    pub fn polls(&self) -> &Polls {
        &self.polls
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// All entries, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = &LeafEntry> {
        self.entries.as_slice().iter()
    }

    /// The entry for `leaf`.
    pub fn get(&self, leaf: LeafId) -> Option<&LeafEntry> {
        self.iter().find(|e| e.leaf == leaf)
    }

    fn get_mut(&mut self, leaf: LeafId) -> Option<&mut LeafEntry> {
        self.entries
            .as_mut_slice()
            .iter_mut()
            .find(|e| e.leaf == leaf)
    }

    /// An ATTACH that attaches, from `leaf`, at `now`. A known leaf keeps
    /// its slot; a new one gets the first free slot at least `lead` from
    /// now, so that the ATTACHED that answers can name it.
    pub fn attached(
        &mut self,
        leaf: LeafId,
        mode: RxMode,
        lead: Duration,
        now: Instant,
    ) -> Result<&LeafEntry, Full> {
        if self.get(leaf).is_some() {
            let e = self.get_mut(leaf).expect("found");
            e.mode = mode;
            e.missed = false;
            e.unanswered = 0;
            return Ok(e);
        }
        if self.len() == LEAVES {
            return Err(Full);
        }
        let entry = LeafEntry {
            leaf,
            next_poll: self.free_slot(now + lead),
            mode,
            missed: false,
            unanswered: 0,
        };
        let _ = self.entries.push(entry);
        Ok(self.get(leaf).expect("pushed"))
    }

    /// The slot index in the cycle of grid instant `t`.
    fn phase(&self, t: Instant) -> usize {
        (t.as_micros() / self.polls.interval.as_micros().max(1)) as usize % LEAVES
    }

    /// The earliest grid instant at or after `from` whose phase no entry
    /// holds. There is one: the table is not full.
    fn free_slot(&self, from: Instant) -> Instant {
        let mut used = [0u64; LEAVES / 64];
        for e in self.iter() {
            let p = self.phase(e.next_poll);
            used[p / 64] |= 1 << (p % 64);
        }
        let n = self.polls.interval.as_micros().max(1);
        let first = from.as_micros().div_ceil(n);
        (first..first + LEAVES as u64)
            .find(|g| {
                let p = *g as usize % LEAVES;
                used[p / 64] & (1 << (p % 64)) == 0
            })
            .map_or(from, |g| Instant::from_micros(g * n))
    }

    /// `leaf` answered a poll, so it is listening. Returns whether it is in
    /// the table.
    pub fn answered(&mut self, leaf: LeafId, mode: RxMode) -> bool {
        match self.get_mut(leaf) {
            Some(e) => {
                e.mode = mode;
                e.unanswered = 0;
                e.missed = false;
                true
            }
            None => false,
        }
    }

    /// The leaf whose poll is due at `now`, the earliest slot first, and
    /// its slot.
    pub fn poll_due(&self, now: Instant) -> Option<(LeafId, Instant)> {
        self.iter()
            .filter(|e| e.unanswered < self.polls.missed && e.next_poll <= now)
            .min_by_key(|e| e.next_poll)
            .map(|e| (e.leaf, e.next_poll))
    }

    /// The poll of `leaf` was sent or skipped: it waits for its next slot.
    pub fn polled(&mut self, leaf: LeafId) {
        let period = self.polls.period;
        if let Some(e) = self.get_mut(leaf) {
            e.unanswered = e.unanswered.saturating_add(1);
            e.next_poll += period;
        }
    }

    /// When the entry next needs the relay: its poll, or once its polls are
    /// spent, the end of the wait for the answer to the last one.
    fn due(&self, e: &LeafEntry) -> Instant {
        if e.unanswered < self.polls.missed {
            e.next_poll
        } else {
            Instant::from_micros(e.next_poll.as_micros() - self.polls.period.as_micros())
                + self.polls.reply_wait
        }
    }

    /// Drop every leaf that left its last [`Polls::missed`] polls
    /// unanswered, and call `dropped` with each.
    pub fn expire(&mut self, now: Instant, mut dropped: impl FnMut(LeafId)) {
        loop {
            let gone = self
                .iter()
                .find(|e| e.unanswered >= self.polls.missed && self.due(e) <= now)
                .map(|e| e.leaf);
            let Some(leaf) = gone else { return };
            self.remove(leaf);
            dropped(leaf);
        }
    }

    /// When the table next needs the relay: a poll or a drop.
    pub fn next_due(&self) -> Option<Instant> {
        self.iter().map(|e| self.due(e)).min()
    }

    /// The next slot of a leaf that is still polled.
    pub fn next_poll(&self) -> Option<Instant> {
        self.iter()
            .filter(|e| e.unanswered < self.polls.missed)
            .map(|e| e.next_poll)
            .min()
    }

    /// When the next leaf is dropped, if none answers meanwhile.
    pub fn next_drop(&self) -> Option<Instant> {
        self.iter()
            .filter(|e| e.unanswered >= self.polls.missed)
            .map(|e| self.due(e))
            .min()
    }

    /// Mark whether `leaf`, if present, left its last DELIVER unanswered.
    pub fn set_missed(&mut self, leaf: LeafId, missed: bool) {
        if let Some(e) = self.get_mut(leaf) {
            e.missed = missed;
        }
    }

    /// Forget `leaf`, if present.
    pub fn remove(&mut self, leaf: LeafId) {
        self.entries.retain(|e| e.leaf != leaf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: u64) -> Instant {
        Instant::from_micros(s * 1_000_000)
    }

    fn id(i: u32) -> LeafId {
        LeafId(i.to_be_bytes())
    }

    #[test]
    fn the_design_schedule() {
        let p = Polls::from_config(&Config::default());
        assert_eq!(p.interval, Duration::from_micros(1_171_875));
        assert_eq!(p.period, Duration::from_secs(300));
        assert_eq!(p.missed, 3, "⌈600 / 300⌉ + 1");
        assert_eq!(POLL_AIRTIME, Duration::from_micros(10_560));
        // A full table's polls against the budget's rate (§9.4).
        let share = POLL_AIRTIME.as_micros() as f64 / p.interval.as_micros() as f64;
        assert!((share - 0.0090).abs() < 0.0001, "{share}");
    }

    #[test]
    fn each_leaf_owns_a_slot_and_is_polled_every_period() {
        let config = Config::default();
        let p = Polls::from_config(&config);
        let mut t = Leaves::from_config(&config);
        let lead = Duration::from_secs(1);
        let a = t
            .attached(id(1), RxMode::Listening, lead, at(10))
            .unwrap()
            .next_poll;
        let b = t
            .attached(id(2), RxMode::Hybrid, lead, at(10))
            .unwrap()
            .next_poll;
        assert!(a >= at(11) && a < at(11) + p.interval, "{a:?}");
        assert_eq!(b, a + p.interval, "the next free slot");
        // A known leaf keeps its slot and its mode is updated.
        let again = t.attached(id(1), RxMode::DutyCycled, lead, at(20)).unwrap();
        assert_eq!((again.next_poll, again.mode), (a, RxMode::DutyCycled));
        assert_eq!(t.poll_due(at(10)), None);
        assert_eq!(t.poll_due(a), Some((id(1), a)));
        t.polled(id(1));
        assert_eq!(t.get(id(1)).unwrap().next_poll, a + p.period);
        assert_eq!(t.poll_due(b), Some((id(2), b)));
        t.polled(id(2));
        assert_eq!(t.next_due(), Some(a + p.period));
    }

    #[test]
    fn a_leaf_that_misses_its_polls_is_dropped_after_the_answer_wait() {
        let config = Config::default();
        let p = Polls::from_config(&config);
        let mut t = Leaves::from_config(&config);
        let first = t
            .attached(id(1), RxMode::Listening, Duration::ZERO, at(0))
            .unwrap()
            .next_poll;
        let mut slot = first;
        for _ in 0..p.missed {
            assert_eq!(t.poll_due(slot).map(|(l, _)| l), Some(id(1)));
            t.polled(id(1));
            slot += p.period;
        }
        assert_eq!(t.poll_due(slot), None, "no fourth poll");
        let last = Instant::from_micros(slot.as_micros() - p.period.as_micros());
        let drop_at = last + p.reply_wait;
        assert_eq!(t.next_due(), Some(drop_at));
        let mut gone = None;
        t.expire(Instant::from_micros(drop_at.as_micros() - 1), |l| {
            gone = Some(l)
        });
        assert_eq!(gone, None);
        t.expire(drop_at, |l| gone = Some(l));
        assert_eq!(gone, Some(id(1)));
        assert!(t.is_empty());
        // Dark for less than a TTL: answering the last poll keeps it.
        let first = t
            .attached(id(2), RxMode::Listening, Duration::ZERO, at(0))
            .unwrap()
            .next_poll;
        for _ in 0..p.missed {
            t.polled(id(2));
        }
        assert!(t.answered(id(2), RxMode::Listening));
        t.expire(first + p.period.mul(10), |_| panic!("dropped"));
        assert_eq!(t.get(id(2)).unwrap().unanswered, 0);
        assert!(!t.answered(id(3), RxMode::Listening), "unknown");
    }

    #[test]
    fn a_full_table_answers_no_new_leaf_and_reuses_a_dropped_slot() {
        let config = Config::default();
        let p = Polls::from_config(&config);
        let mut t = Leaves::from_config(&config);
        for i in 0..LEAVES as u32 {
            t.attached(id(i), RxMode::Listening, Duration::ZERO, at(0))
                .unwrap();
        }
        let mut slots: [Instant; LEAVES] = [Instant::ZERO; LEAVES];
        for (s, e) in slots.iter_mut().zip(t.iter()) {
            *s = e.next_poll;
        }
        slots.sort_unstable();
        for w in slots.windows(2) {
            assert_eq!(w[1].since(w[0]), p.interval, "one poll per N");
        }
        assert_eq!(
            t.attached(id(1000), RxMode::Listening, Duration::ZERO, at(0)),
            Err(Full)
        );
        let freed = t.get(id(7)).unwrap().next_poll;
        t.remove(id(7));
        let new = t
            .attached(id(1000), RxMode::Listening, Duration::ZERO, at(0))
            .unwrap();
        assert_eq!(new.next_poll, freed);
    }
}

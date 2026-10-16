//! The leaf table (spec "Attach and polls"): leaves attached to this relay, keyed by
//! [`NodeId`], each with its place in the relay's poll schedule.
//!
//! The relay polls one leaf at a time, each every [`PERIOD`], and with L
//! leaves the slots are at least the spacing S = P / L apart: each leaf's
//! next slot is the first time at least a period after its last one that
//! is S from every other leaf's. So the table settles into a round of L
//! polls S apart, and a leaf that comes or goes moves only its own slot.
//! A leaf that has left its polls unanswered for a delivery TTL is
//! dropped.
//!
//! When the table is full, an ATTACH from an unknown leaf is not answered,
//! so the leaf attaches elsewhere. At 256 entries against ≈80 expected
//! leaves per relay (spec "Attach and polls") this is an overload case.
//!
//! Spec "Attach and polls" also lists an `rssi` field; nothing in the design reads it, so it
//! is not kept.

use crate::event::MAX_FRAME;
use crate::ids::NodeId;
use crate::node::Config;
use crate::radio::{time_on_air, MIN_PREAMBLE_SYMBOLS};
use crate::tables::fixed::FixedVec;
use crate::tables::send_queue::SEND_QUEUE;
use crate::tables::{attempt_span, retry_waits, LBT_GIVE_UP};
use crate::time::{Duration, Instant};
use crate::wire::{RxMode, ACK_LEN, ATTACHED_LEN};

/// Table capacity (spec "Attach and polls").
pub const LEAVES: usize = 256;

/// Airtime of a poll: one ATTACHED, 10.56 ms (spec "Packet formats").
pub const POLL_AIRTIME: Duration = time_on_air(ATTACHED_LEN as u8, MIN_PREAMBLE_SYMBOLS);

/// P, how often each leaf is polled (spec "Attach and polls"): half the delivery
/// TTL, so that with the drop after a TTL of unanswered polls a gone leaf
/// is kept at most a period past the life of its messages.
pub const PERIOD: Duration = Duration::from_secs(300);

/// The closest two slots come: the spacing of a full table, P / 256 =
/// 1.17 s (spec "Attach and polls", roadmap D39). A poll costs the relay 10.56 ms,
/// so a full table's polls take 0.90 % of its time, 9.2 % of the duty
/// budget's 9.83 % (spec "Duty cycle cap"); a full table must leave forwarding nine tenths
/// of the budget, which needs at least 10.56 ms / (10 % × 9.83 %) =
/// 1.07 s.
pub const SPACING_MIN: Duration = PERIOD.div(LEAVES as u64);

/// How late after its slot a poll may start; one that cannot is skipped
/// and counts as unanswered (spec "Attach and polls"). It is listen-before-talk's
/// give-up time (spec "Hand-over"): a poll goes before a hand-over, so it waits at most
/// for one already under way, a full frame and its ACK timeout, 238.6 ms,
/// and for an owed ACK, 114.4 ms, which leaves 147 ms of channel checks.
pub const POLL_LATE: Duration = LBT_GIVE_UP;

/// The poll schedule's parameters, from a configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Polls {
    /// The delivery TTL (spec "Delivery queue"): a leaf that leaves its polls unanswered
    /// for this long is dropped, so that one dark for less keeps its
    /// relay.
    pub ttl: Duration,
    /// How long after its slot the relay waits for the answer to a poll:
    /// the poll's lateness and airtime, and the leaf's reply time
    /// (`ack_timeout`, spec "Hand-over").
    pub reply_wait: Duration,
    /// How long one hand-over of a full frame lasts with all its attempts
    /// unanswered: each attempt's listen-before-talk to its give-up, the
    /// frame and the ACK timeout, and the waits between (spec "Hand-over"), 3.13 s.
    pub span: Duration,
    /// How long the DELIVERs ahead of a leaf's can hold the relay: a full
    /// send queue (spec "Send queue") of full frames, each with its ACK, 6.12 s.
    pub queue_wait: Duration,
}

impl Polls {
    /// The schedule for `config`.
    pub const fn from_config(config: &Config) -> Polls {
        let attempt = attempt_span(config.ack_timeout).as_micros();
        let span = attempt * config.hop_attempts as u64
            + retry_waits(config.retry_backoff, config.hop_attempts).as_micros();
        let full = time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS).as_micros()
            + time_on_air(ACK_LEN as u8, MIN_PREAMBLE_SYMBOLS).as_micros();
        Polls {
            ttl: config.delivery_ttl,
            reply_wait: Duration::from_micros(
                POLL_LATE.as_micros() + POLL_AIRTIME.as_micros() + config.ack_timeout.as_micros(),
            ),
            span: Duration::from_micros(span),
            queue_wait: Duration::from_micros(SEND_QUEUE as u64 * full),
        }
    }

    /// S for a table of `leaves`: P / L, the least time between two slots,
    /// never under [`SPACING_MIN`].
    pub const fn spacing(&self, leaves: usize) -> Duration {
        let l = if leaves == 0 { 1 } else { leaves as u64 };
        let s = PERIOD.div(l);
        if s.as_micros() < SPACING_MIN.as_micros() {
            SPACING_MIN
        } else {
            s
        }
    }

    /// How long a sleeping leaf listens after the answer to a poll that
    /// said `waiting` messages wait, unless it has them sooner (spec "Leaf receive mode"):
    /// the DELIVERs ahead of them, and each of them with all its attempts.
    pub const fn listen(&self, waiting: u8) -> Duration {
        if waiting == 0 {
            return Duration::ZERO;
        }
        Duration::from_micros(self.queue_wait.as_micros() + waiting as u64 * self.span.as_micros())
    }
}

/// One attached leaf.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeafEntry {
    /// The leaf.
    pub leaf: NodeId,
    /// Its next slot, as the last ATTACHED to it named.
    pub next_poll: Instant,
    /// The slot of its last poll, sent or skipped.
    pub last_slot: Instant,
    /// The slot of the first poll in a row it left unanswered.
    pub missing_since: Instant,
    /// When the last ATTACHED went to it.
    pub told: Instant,
    /// When it last answered a poll or sent an ATTACH.
    pub heard: Instant,
    /// Its declared receive mode.
    pub mode: RxMode,
    /// Its last DELIVER went unanswered: it is not listening, and its
    /// messages wait for its next answer to a poll or the delivery retry
    /// (spec "Delivery queue").
    pub missed: bool,
    /// Its polls have gone unanswered for a TTL: no more are sent, and it
    /// is dropped once the answer to the last is overdue.
    pub done: bool,
    /// Polls sent or skipped since it last answered.
    pub unanswered: u8,
    /// Messages the last ATTACHED to it said wait.
    pub waiting: u8,
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
            leaf: NodeId::NONE,
            next_poll: Instant::ZERO,
            last_slot: Instant::ZERO,
            missing_since: Instant::ZERO,
            told: Instant::ZERO,
            heard: Instant::ZERO,
            mode: RxMode::Listening,
            missed: false,
            done: false,
            unanswered: 0,
            waiting: 0,
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

    /// S now.
    pub fn spacing(&self) -> Duration {
        self.polls.spacing(self.len())
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
    pub fn get(&self, leaf: NodeId) -> Option<&LeafEntry> {
        self.iter().find(|e| e.leaf == leaf)
    }

    fn get_mut(&mut self, leaf: NodeId) -> Option<&mut LeafEntry> {
        self.entries
            .as_mut_slice()
            .iter_mut()
            .find(|e| e.leaf == leaf)
    }

    /// An ATTACH that attaches, from `leaf`, at `now`, to be answered with
    /// an ATTACHED that says `waiting` messages wait. A known leaf keeps
    /// its slot unless its polls had stopped; a new one, or one whose polls
    /// had stopped, gets the first slot a period from now that keeps the
    /// spacing.
    pub fn attached(
        &mut self,
        leaf: NodeId,
        mode: RxMode,
        waiting: u8,
        now: Instant,
    ) -> Result<&LeafEntry, Full> {
        let known = self.get(leaf).copied();
        if known.is_none() {
            if self.len() == LEAVES {
                return Err(Full);
            }
            let entry = LeafEntry {
                leaf,
                next_poll: now,
                last_slot: now,
                missing_since: now,
                told: now,
                heard: now,
                mode,
                missed: false,
                done: true,
                unanswered: 0,
                waiting,
            };
            let _ = self.entries.push(entry);
        }
        let next = match known {
            Some(e) if !e.done => e.next_poll,
            _ => self.free(leaf, now + PERIOD),
        };
        let e = self.get_mut(leaf).expect("present");
        *e = LeafEntry {
            next_poll: next,
            // It listens for the ATTACHED now, and for the messages it
            // counts after it (spec "Leaf receive mode").
            told: now,
            heard: now,
            mode,
            missed: false,
            done: false,
            unanswered: 0,
            waiting,
            ..*e
        };
        Ok(e)
    }

    /// The first instant at or after `target` at least a spacing from the
    /// next slot of every other leaf still polled.
    fn free(&self, leaf: NodeId, target: Instant) -> Instant {
        let spacing = self.spacing();
        let mut taken = [Instant::ZERO; LEAVES];
        let mut n = 0;
        for e in self.iter() {
            if e.leaf != leaf && !e.done && e.next_poll + spacing > target {
                taken[n] = e.next_poll;
                n += 1;
            }
        }
        let taken = &mut taken[..n];
        taken.sort_unstable();
        let mut at = target;
        for &t in taken.iter() {
            if t >= at + spacing {
                break;
            }
            if t + spacing > at {
                at = t + spacing;
            }
        }
        at
    }

    /// `leaf` answered a poll at `now`, so it is listening. Returns whether
    /// it is in the table.
    pub fn answered(&mut self, leaf: NodeId, mode: RxMode, now: Instant) -> bool {
        match self.get_mut(leaf) {
            Some(e) => {
                e.mode = mode;
                e.unanswered = 0;
                e.done = false;
                e.missed = false;
                e.heard = now;
                true
            }
            None => false,
        }
    }

    /// The leaf whose poll is due at `now`, the earliest slot first, and
    /// its slot.
    pub fn poll_due(&self, now: Instant) -> Option<(NodeId, Instant)> {
        self.iter()
            .filter(|e| !e.done && e.next_poll <= now)
            .min_by_key(|e| e.next_poll)
            .map(|e| (e.leaf, e.next_poll))
    }

    /// The poll of `leaf` at its slot was sent or skipped: it gets its next
    /// slot, and once its polls have gone unanswered for a TTL it is done.
    pub fn polled(&mut self, leaf: NodeId) {
        let Some(e) = self.get(leaf).copied() else {
            return;
        };
        let slot = e.next_poll;
        let next = self.free(leaf, slot + PERIOD);
        let ttl = self.polls.ttl;
        let e = self.get_mut(leaf).expect("found");
        if e.unanswered == 0 {
            e.missing_since = slot;
        }
        e.unanswered = e.unanswered.saturating_add(1);
        e.last_slot = slot;
        e.next_poll = next;
        e.done = slot.since(e.missing_since) >= ttl;
    }

    /// An ATTACHED that says `waiting` messages wait goes to `leaf` at
    /// `now`.
    pub fn told(&mut self, leaf: NodeId, waiting: u8, now: Instant) {
        if let Some(e) = self.get_mut(leaf) {
            e.told = now;
            e.waiting = waiting;
        }
    }

    /// Whether `leaf`, which sleeps, is listening at `now` for a message
    /// queued at `enqueued`: it answered the last ATTACHED, which counted
    /// the message, and its wait for those messages has not run out
    /// (spec "Leaf receive mode").
    pub fn awake_for(&self, leaf: NodeId, enqueued: Instant, now: Instant) -> bool {
        self.get(leaf).is_some_and(|e| {
            e.heard >= e.told && enqueued <= e.told && now < e.heard + self.polls.listen(e.waiting)
        })
    }

    /// When the entry next needs the relay: its poll, or once its polls
    /// are done, the end of the wait for the answer to the last one.
    fn due(&self, e: &LeafEntry) -> Instant {
        if e.done {
            e.last_slot + self.polls.reply_wait
        } else {
            e.next_poll
        }
    }

    /// Drop every leaf whose polls are done and whose last answer is
    /// overdue, and call `dropped` with each.
    pub fn expire(&mut self, now: Instant, mut dropped: impl FnMut(NodeId)) {
        loop {
            let gone = self
                .iter()
                .find(|e| e.done && self.due(e) <= now)
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
        self.iter().filter(|e| !e.done).map(|e| e.next_poll).min()
    }

    /// When the next leaf is dropped, if none answers meanwhile.
    pub fn next_drop(&self) -> Option<Instant> {
        self.iter().filter(|e| e.done).map(|e| self.due(e)).min()
    }

    /// Mark whether `leaf`, if present, left its last DELIVER unanswered.
    pub fn set_missed(&mut self, leaf: NodeId, missed: bool) {
        if let Some(e) = self.get_mut(leaf) {
            e.missed = missed;
        }
    }

    /// Forget `leaf`, if present.
    pub fn remove(&mut self, leaf: NodeId) {
        self.entries.retain(|e| e.leaf != leaf);
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    fn at(s: u64) -> Instant {
        Instant::from_micros(s * 1_000_000)
    }

    fn id(i: u32) -> NodeId {
        NodeId(i.to_be_bytes())
    }

    #[test]
    fn the_design_schedule() {
        let p = Polls::from_config(&Config::default());
        assert_eq!(POLL_AIRTIME, Duration::from_micros(10_560));
        assert_eq!(p.span, Duration::from_micros(3_133_440));
        assert_eq!(p.queue_wait, Duration::from_micros(6_118_400));
        assert_eq!(p.listen(0), Duration::ZERO);
        assert_eq!(p.listen(1), Duration::from_micros(9_251_840));
        assert_eq!(p.spacing(1), PERIOD);
        assert_eq!(p.spacing(10), Duration::from_secs(30));
        assert_eq!(p.spacing(LEAVES), Duration::from_micros(1_171_875));
        assert_eq!(p.spacing(1000), SPACING_MIN);
        // A full table's polls against the budget's 9.83 % (spec "Duty cycle cap").
        let budget = 59.0 / 600.0;
        let share = POLL_AIRTIME.as_micros() as f64 / SPACING_MIN.as_micros() as f64;
        assert!((share / budget - 0.0917).abs() < 0.0005, "{share}");
    }

    #[test]
    fn slots_keep_the_spacing_and_each_leaf_is_polled_every_period() {
        let mut t = Leaves::from_config(&Config::default());
        // Ten leaves attach in the same second.
        for i in 0..10 {
            t.attached(id(i), RxMode::Listening, 0, at(10)).unwrap();
        }
        let s = t.spacing();
        assert_eq!(s, Duration::from_secs(30));
        let mut slots: Vec<Instant> = t.iter().map(|e| e.next_poll).collect();
        slots.sort_unstable();
        for w in slots.windows(2) {
            assert!(w[1].since(w[0]) >= s, "{slots:?}");
        }
        // Run eight periods of polls; once the round has settled, after the
        // first five, every leaf once per period, S apart.
        let mut sent = Vec::new();
        let mut now = at(10);
        while now < at(10) + PERIOD.mul(8) {
            if let Some((leaf, slot)) = t.poll_due(now) {
                if slot >= at(10) + PERIOD.mul(5) {
                    sent.push((slot, leaf));
                }
                t.polled(leaf);
                assert!(t.answered(leaf, RxMode::Listening, slot));
            } else {
                now = t.next_due().unwrap();
            }
        }
        for w in sent.windows(2) {
            assert!(w[1].0.since(w[0].0) >= s);
        }
        for i in 0..10 {
            let mine: Vec<_> = sent.iter().filter(|s| s.1 == id(i)).collect();
            assert!(mine.len() >= 2);
            for w in mine.windows(2) {
                assert_eq!(w[1].0.since(w[0].0), PERIOD);
            }
        }
        // A known leaf keeps its slot, and its mode is updated.
        let before = t.get(id(3)).unwrap().next_poll;
        let again = t.attached(id(3), RxMode::DutyCycled, 2, now).unwrap();
        assert_eq!((again.next_poll, again.mode), (before, RxMode::DutyCycled));
    }

    #[test]
    fn a_leaf_that_misses_its_polls_for_a_ttl_is_dropped_after_the_answer_wait() {
        // P = 300 s, so the third poll is the TTL after the first.
        let config = Config::default();
        let p = Polls::from_config(&config);
        let mut t = Leaves::from_config(&config);
        let first = t
            .attached(id(1), RxMode::Listening, 0, at(0))
            .unwrap()
            .next_poll;
        assert_eq!(first, at(300));
        let mut slot = first;
        for _ in 0..3 {
            assert_eq!(t.poll_due(slot).map(|(l, _)| l), Some(id(1)));
            t.polled(id(1));
            slot += PERIOD;
        }
        assert_eq!(t.poll_due(slot), None, "no fourth poll");
        let drop_at = first + Duration::from_secs(600) + p.reply_wait;
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
        t.attached(id(2), RxMode::Listening, 0, at(0)).unwrap();
        for _ in 0..3 {
            t.polled(id(2));
        }
        assert!(t.answered(id(2), RxMode::Listening, at(901)));
        t.expire(at(3000), |_| panic!("dropped"));
        assert_eq!(t.get(id(2)).unwrap().unanswered, 0);
        assert!(!t.answered(id(3), RxMode::Listening, at(0)), "unknown");
    }

    #[test]
    fn a_full_table_answers_no_new_leaf_and_polls_every_1_17_s() {
        let mut t = Leaves::from_config(&Config::default());
        for i in 0..LEAVES as u32 {
            t.attached(id(i), RxMode::Listening, 0, at(0)).unwrap();
        }
        assert_eq!(t.attached(id(1000), RxMode::Listening, 0, at(0)), Err(Full));
        let mut slots: Vec<Instant> = t.iter().map(|e| e.next_poll).collect();
        slots.sort_unstable();
        for w in slots.windows(2) {
            assert!(w[1].since(w[0]) >= SPACING_MIN);
        }
        t.remove(id(7));
        assert!(t.attached(id(1000), RxMode::Listening, 0, at(0)).is_ok());
    }

    #[test]
    fn a_sleeping_leaf_is_awake_for_what_its_answered_poll_counted() {
        let mut t = Leaves::from_config(&Config::default());
        let p = *t.polls();
        t.attached(id(1), RxMode::Hybrid, 0, at(0)).unwrap();
        t.polled(id(1));
        t.told(id(1), 2, at(300));
        assert!(!t.awake_for(id(1), at(100), at(300)), "no answer yet");
        assert!(t.answered(id(1), RxMode::Hybrid, at(300)));
        let window = p.listen(2);
        let last = Instant::from_micros((at(300) + window).as_micros() - 1);
        assert!(t.awake_for(id(1), at(100), last));
        assert!(!t.awake_for(id(1), at(100), at(300) + window));
        assert!(
            !t.awake_for(id(1), at(301), at(302)),
            "queued after the poll"
        );
    }
}

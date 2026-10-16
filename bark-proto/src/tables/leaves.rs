//! The leaf table (design §10.2): leaves attached to this relay, keyed by
//! [`LeafId`], each with its place in the relay's poll schedule.
//!
//! The relay polls one leaf at a time. With L leaves the period is
//! P = clamp(L N, [`PERIOD_MIN`], [`PERIOD_MAX`]) while the relay's home
//! channel is quiet ([`QUIET_ENTER`], [`QUIET_LEAVE`]) and [`PERIOD_MAX`]
//! otherwise, and the leaves' slots are at least the spacing S = P / L
//! apart: each leaf's next slot is the first time at least a period after
//! its last one that is S from every other leaf's. So the table settles
//! into a round of L polls S apart, which is N while L N lies between the
//! bounds, and a leaf that comes or goes moves only its own slot. A leaf
//! that has left its polls unanswered for a delivery TTL is dropped.
//!
//! When the table is full, an ATTACH from an unknown leaf is not answered,
//! so the leaf attaches elsewhere. At 256 entries against ≈80 expected
//! leaves per relay (§10.2) this is an overload case.
//!
//! §10.2 also lists an `rssi` field; nothing in the design reads it, so it
//! is not kept.

use crate::event::MAX_FRAME;
use crate::ids::LeafId;
use crate::node::Config;
use crate::radio::{time_on_air, MIN_PREAMBLE_SYMBOLS};
use crate::tables::busy::ONE;
use crate::tables::fixed::FixedVec;
use crate::tables::send_queue::SEND_QUEUE;
use crate::tables::{attempt_span, retry_waits, LBT_GIVE_UP};
use crate::time::{Duration, Instant};
use crate::wire::{RxMode, ACK_LEN, ATTACHED_LEN};

/// Table capacity (design §10.2).
pub const LEAVES: usize = 256;

/// Airtime of a poll: one ATTACHED, 10.56 ms (§14).
pub const POLL_AIRTIME: Duration = time_on_air(ATTACHED_LEN as u8, MIN_PREAMBLE_SYMBOLS);

/// N, the spacing of polls while the period is between its bounds
/// (design §10.2).
///
/// A poll every N costs the relay 10.56 ms / N of its time however many
/// leaves it has: at 4 s that is 0.26 %, 2.7 % of the duty budget's 9.83 %
/// (§9.4); N = 10.56 ms / (3 % × 9.83 %) = 3.6 s, rounded up to whole
/// seconds. Three per cent is what
/// the polls of roadmap D39 took at the ≈70 leaves of a relay in the
/// capacity domain, where they left the domain working at twice the §13
/// rate. A spacing of 300 s / 256 = 1.17 s, the other value measured,
/// took 8.5 % of the budget and collapsed the domain at a lower rate
/// (roadmap D41).
pub const POLL_INTERVAL: Duration = Duration::from_secs(4);

/// The shortest period (design §10.2): a small cell's leaves are polled at
/// most every 10 s, so that a phone back from the dark is known within
/// 10 s there.
pub const PERIOD_MIN: Duration = Duration::from_secs(10);

/// The longest period (design §10.2): half the delivery TTL, as under
/// roadmap D39, so that a full table's spacing is 300 s / 256 = 1.17 s and
/// a gone leaf is kept at most a period past the life of its messages.
pub const PERIOD_MAX: Duration = Duration::from_secs(300);

/// The relay's home busy estimate (§6.1) below which the period may
/// shorten, once it has been at [`PERIOD_MAX`]: 3.5 %.
pub const QUIET_ENTER: u32 = (ONE as u64 * 35 / 1000) as u32;

/// The estimate at or above which the period is [`PERIOD_MAX`]: 4.5 %.
///
/// Shortened polls add frames to the channel, and extra frames on a loaded
/// channel are what start the collapse of the capacity domain
/// (`collapse-research-2026-10-16.md`). The domain works at the §13 rate
/// and at twice it, where relays read a median of 3.2 to 3.4 % and 5.3 to
/// 5.8 % (roadmap D41). 4.5 % is the reading halfway between them in load;
/// a relay below it that shortens its period to one poll every N adds at
/// most a poll and its answer per N, (10.56 + 11.84) ms / 4 s = 0.56 % of
/// the channel, and stays under the median of twice the rate. The 1 point
/// between [`QUIET_ENTER`] and this is the hysteresis: 2.4 times the
/// median standard deviation of a relay's reading over time at a steady
/// load, 0.41 %, and more than the 0.56 % one co-channel neighbour's
/// shortened polls can add to it (a relay's own polls and answers do not
/// count in its own reading, §6.1).
pub const QUIET_LEAVE: u32 = (ONE as u64 * 45 / 1000) as u32;

/// How late after its slot a poll may start; one that cannot is skipped
/// and counts as unanswered (design §10.2). It is listen-before-talk's
/// give-up time (§9.3): a poll goes before a hand-over, so it waits at most
/// for one already under way, a full frame and its ACK timeout, 238.6 ms,
/// and for an owed ACK, 114.4 ms, which leaves 147 ms of channel checks.
pub const POLL_LATE: Duration = LBT_GIVE_UP;

/// The poll schedule's parameters, from a configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Polls {
    /// N.
    pub interval: Duration,
    /// The delivery TTL (§10.4): a leaf that leaves its polls unanswered
    /// for this long is dropped, so that one dark for less keeps its
    /// relay.
    pub ttl: Duration,
    /// How long after its slot the relay waits for the answer to a poll:
    /// the poll's lateness and airtime, and the leaf's reply time
    /// (`ack_timeout`, §9.3).
    pub reply_wait: Duration,
    /// How long one hand-over of a full frame lasts with all its attempts
    /// unanswered: each attempt's listen-before-talk to its give-up, the
    /// frame and the ACK timeout, and the waits between (§9.3), 3.13 s.
    pub span: Duration,
    /// How long the DELIVERs ahead of a leaf's can hold the relay: a full
    /// send queue (§9.3) of full frames, each with its ACK, 6.12 s.
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
            interval: config.poll_interval,
            ttl: config.delivery_ttl,
            reply_wait: Duration::from_micros(
                POLL_LATE.as_micros() + POLL_AIRTIME.as_micros() + config.ack_timeout.as_micros(),
            ),
            span: Duration::from_micros(span),
            queue_wait: Duration::from_micros(SEND_QUEUE as u64 * full),
        }
    }

    /// P for a table of `leaves`: L N within [`PERIOD_MIN`] and
    /// [`PERIOD_MAX`] while `quiet`, else [`PERIOD_MAX`].
    pub const fn period(&self, leaves: usize, quiet: bool) -> Duration {
        if !quiet {
            return PERIOD_MAX;
        }
        let p = self.interval.as_micros().saturating_mul(leaves as u64);
        let p = if p < PERIOD_MIN.as_micros() {
            PERIOD_MIN.as_micros()
        } else if p > PERIOD_MAX.as_micros() {
            PERIOD_MAX.as_micros()
        } else {
            p
        };
        Duration::from_micros(p)
    }

    /// S for a table of `leaves`: P / L, the least time between two slots.
    pub const fn spacing(&self, leaves: usize, quiet: bool) -> Duration {
        let l = if leaves == 0 { 1 } else { leaves as u64 };
        self.period(leaves, quiet).div(l)
    }

    /// How long a sleeping leaf listens after the answer to a poll that
    /// said `waiting` messages wait, unless it has them sooner (§10.5):
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
    pub leaf: LeafId,
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
    /// (§10.4).
    pub missed: bool,
    /// Its polls have gone unanswered for a TTL: no more are sent, and it
    /// is dropped once the answer to the last is overdue.
    pub done: bool,
    /// Polls sent or skipped since it last answered.
    pub unanswered: u8,
    /// Path loss to it in whole dB, rounded up, from its last ATTACH,
    /// sent at the ERP limit as every leaf sends.
    pub loss_db: u8,
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
    quiet: bool,
}

impl Leaves {
    /// An empty table on the schedule of `config`, at the longest period.
    pub const fn from_config(config: &Config) -> Leaves {
        let fill = LeafEntry {
            leaf: LeafId::RELAY,
            next_poll: Instant::ZERO,
            last_slot: Instant::ZERO,
            missing_since: Instant::ZERO,
            told: Instant::ZERO,
            heard: Instant::ZERO,
            mode: RxMode::Listening,
            missed: false,
            done: false,
            unanswered: 0,
            loss_db: 0,
            waiting: 0,
        };
        Leaves {
            entries: FixedVec::new(fill),
            polls: Polls::from_config(config),
            quiet: false,
        }
    }

    /// The schedule.
    pub fn polls(&self) -> &Polls {
        &self.polls
    }

    /// Whether the home channel counts as quiet, so that the period may
    /// shorten.
    pub fn quiet(&self) -> bool {
        self.quiet
    }

    /// Update from the home busy estimate `busy` (§6.1, fixed point):
    /// quiet below [`QUIET_ENTER`], until [`QUIET_LEAVE`]. Returns whether
    /// it changed.
    pub fn busy(&mut self, busy: u32) -> bool {
        let quiet = if self.quiet {
            busy < QUIET_LEAVE
        } else {
            busy < QUIET_ENTER
        };
        let changed = quiet != self.quiet;
        self.quiet = quiet;
        changed
    }

    /// P now.
    pub fn period(&self) -> Duration {
        self.polls.period(self.len(), self.quiet)
    }

    /// S now.
    pub fn spacing(&self) -> Duration {
        self.polls.spacing(self.len(), self.quiet)
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

    /// An ATTACH that attaches, from `leaf`, at `now`, to be answered with
    /// an ATTACHED that says `waiting` messages wait. A known leaf keeps
    /// its slot unless its polls had stopped; a new one, or one whose polls
    /// had stopped, gets the first slot a period from now that keeps the
    /// spacing.
    pub fn attached(
        &mut self,
        leaf: LeafId,
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
                loss_db: 0,
                waiting,
            };
            let _ = self.entries.push(entry);
        }
        let next = match known {
            Some(e) if !e.done => e.next_poll,
            _ => self.free(leaf, now + self.period()),
        };
        let e = self.get_mut(leaf).expect("present");
        *e = LeafEntry {
            next_poll: next,
            // It listens for the ATTACHED now, and for the messages it
            // counts after it (§10.5).
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
    fn free(&self, leaf: LeafId, target: Instant) -> Instant {
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

    /// An ATTACH from `leaf` arrived over a path loss of `loss_q`.
    pub fn measured(&mut self, leaf: LeafId, loss_q: i16) {
        if let Some(e) = self.get_mut(leaf) {
            e.loss_db = ((loss_q as i32 + 3) / 4).clamp(0, u8::MAX as i32) as u8;
        }
    }

    /// `leaf` answered a poll at `now`, so it is listening. Returns whether
    /// it is in the table.
    pub fn answered(&mut self, leaf: LeafId, mode: RxMode, now: Instant) -> bool {
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
    pub fn poll_due(&self, now: Instant) -> Option<(LeafId, Instant)> {
        self.iter()
            .filter(|e| !e.done && e.next_poll <= now)
            .min_by_key(|e| e.next_poll)
            .map(|e| (e.leaf, e.next_poll))
    }

    /// The poll of `leaf` at its slot was sent or skipped: it gets its next
    /// slot, and once its polls have gone unanswered for a TTL it is done.
    pub fn polled(&mut self, leaf: LeafId) {
        let Some(e) = self.get(leaf).copied() else {
            return;
        };
        let slot = e.next_poll;
        let next = self.free(leaf, slot + self.period());
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
    pub fn told(&mut self, leaf: LeafId, waiting: u8, now: Instant) {
        if let Some(e) = self.get_mut(leaf) {
            e.told = now;
            e.waiting = waiting;
        }
    }

    /// Whether `leaf`, which sleeps, is listening at `now` for a message
    /// queued at `enqueued`: it answered the last ATTACHED, which counted
    /// the message, and its wait for those messages has not run out
    /// (§10.5).
    pub fn awake_for(&self, leaf: LeafId, enqueued: Instant, now: Instant) -> bool {
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
    pub fn expire(&mut self, now: Instant, mut dropped: impl FnMut(LeafId)) {
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
    extern crate std;
    use super::*;
    use std::vec::Vec;

    fn at(s: u64) -> Instant {
        Instant::from_micros(s * 1_000_000)
    }

    fn id(i: u32) -> LeafId {
        LeafId(i.to_be_bytes())
    }

    fn quiet_table() -> Leaves {
        let mut t = Leaves::from_config(&Config::default());
        assert!(t.busy(0));
        t
    }

    #[test]
    fn the_design_schedule() {
        let p = Polls::from_config(&Config::default());
        assert_eq!(p.interval, POLL_INTERVAL);
        assert_eq!(POLL_AIRTIME, Duration::from_micros(10_560));
        assert_eq!(p.span, Duration::from_micros(3_133_440));
        assert_eq!(p.queue_wait, Duration::from_micros(6_118_400));
        assert_eq!(p.listen(0), Duration::ZERO);
        assert_eq!(p.listen(1), Duration::from_micros(9_251_840));
        // A poll every N against the budget's 9.83 % (§9.4): under 3 %.
        let budget = 59.0 / 600.0;
        let share = POLL_AIRTIME.as_micros() as f64 / p.interval.as_micros() as f64;
        assert!((share / budget - 0.0269).abs() < 0.0005, "{share}");
        // The period follows the table between its bounds while quiet.
        assert_eq!(p.period(1, true), PERIOD_MIN);
        assert_eq!(p.period(2, true), PERIOD_MIN);
        assert_eq!(p.spacing(2, true), Duration::from_secs(5));
        assert_eq!(p.period(10, true), Duration::from_secs(40));
        assert_eq!(p.spacing(10, true), POLL_INTERVAL);
        assert_eq!(p.period(75, true), PERIOD_MAX);
        assert_eq!(p.period(256, true), PERIOD_MAX);
        assert_eq!(p.spacing(256, true), Duration::from_micros(1_171_875));
        // Busy, it is the longest whatever the table.
        assert_eq!(p.period(10, false), PERIOD_MAX);
        assert_eq!(p.spacing(10, false), Duration::from_secs(30));
    }

    #[test]
    fn the_quiet_rule_has_hysteresis() {
        let mut t = Leaves::from_config(&Config::default());
        assert!(!t.quiet(), "starts at the longest period");
        let pct = |tenths: u64| (ONE as u64 * tenths / 1000) as u32;
        assert!(!t.busy(pct(40)), "4.0 % does not make it quiet");
        assert!(t.busy(pct(34)));
        assert!(t.quiet());
        assert!(!t.busy(pct(44)), "nor does 4.4 % end it");
        assert!(t.busy(pct(45)));
        assert!(!t.quiet());
    }

    #[test]
    fn slots_keep_the_spacing_and_each_leaf_is_polled_every_period() {
        let mut t = quiet_table();
        // Ten leaves attach in the same second.
        for i in 0..10 {
            t.attached(id(i), RxMode::Listening, 0, at(10)).unwrap();
        }
        assert_eq!(t.period(), Duration::from_secs(40));
        let mut slots: Vec<Instant> = t.iter().map(|e| e.next_poll).collect();
        slots.sort_unstable();
        for w in slots.windows(2) {
            assert!(w[1].since(w[0]) >= Duration::from_secs(4), "{slots:?}");
        }
        // Run five periods of polls; once the round has settled, after the
        // first two, every leaf once per period, N apart.
        let mut sent = Vec::new();
        let mut now = at(10);
        while now < at(210) {
            if let Some((leaf, slot)) = t.poll_due(now) {
                if slot >= at(90) {
                    sent.push((slot, leaf));
                }
                t.polled(leaf);
                assert!(t.answered(leaf, RxMode::Listening, slot));
            } else {
                now = t.next_due().unwrap();
            }
        }
        for w in sent.windows(2) {
            assert!(w[1].0.since(w[0].0) >= Duration::from_secs(4));
        }
        for i in 0..10 {
            let mine: Vec<_> = sent.iter().filter(|s| s.1 == id(i)).collect();
            assert!(mine.len() >= 3);
            for w in mine.windows(2) {
                assert_eq!(w[1].0.since(w[0].0), Duration::from_secs(40));
            }
        }
        // A known leaf keeps its slot, and its mode is updated.
        let before = t.get(id(3)).unwrap().next_poll;
        let again = t.attached(id(3), RxMode::DutyCycled, 2, now).unwrap();
        assert_eq!((again.next_poll, again.mode), (before, RxMode::DutyCycled));
    }

    #[test]
    fn a_leaf_that_misses_its_polls_for_a_ttl_is_dropped_after_the_answer_wait() {
        // Busy: P = 300 s, so the third poll is the TTL after the first.
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
            slot += PERIOD_MAX;
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
    fn with_a_short_period_the_drop_still_waits_a_ttl() {
        let mut t = quiet_table();
        t.attached(id(1), RxMode::Listening, 0, at(0)).unwrap();
        assert_eq!(t.period(), PERIOD_MIN);
        let mut polls = 0;
        while let Some((leaf, _)) = t.next_poll().and_then(|s| t.poll_due(s)) {
            t.polled(leaf);
            polls += 1;
        }
        assert_eq!(polls, 61, "a poll every 10 s for 600 s");
    }

    #[test]
    fn a_full_table_answers_no_new_leaf_and_polls_every_1_17_s() {
        let mut t = quiet_table();
        for i in 0..LEAVES as u32 {
            t.attached(id(i), RxMode::Listening, 0, at(0)).unwrap();
        }
        assert_eq!(t.attached(id(1000), RxMode::Listening, 0, at(0)), Err(Full));
        let mut slots: Vec<Instant> = t.iter().map(|e| e.next_poll).collect();
        slots.sort_unstable();
        for w in slots.windows(2) {
            assert!(w[1].since(w[0]) >= Duration::from_micros(1_171_875));
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

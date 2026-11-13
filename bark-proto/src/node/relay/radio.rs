//! The radio's side of the node: the one excursion outstanding and the job
//! that owns it, the duty-cycle budget (spec "Duty cycle cap"), the busy
//! estimate (spec "Busy estimate"), the time away from listening, and the
//! replies owed (spec "The ACK"). It also holds what each job keeps between
//! excursions; each job's module owns that state.

use super::heartbeat::Beacon;
use super::status::Asking;
use super::sweep::Sweep;
use super::Job;
use crate::event::{Action, Frame};
use crate::ids::MsgId;
use crate::node::mac::{airtime, Excursion};
use crate::node::transfer::{Owed, Replies, Reply, Take};
use crate::node::Config;
use crate::radio::Channel;
use crate::tables::budget::Budget;
use crate::tables::busy::BusyEstimator;
use crate::time::{Duration, Instant};

pub(super) struct Radio {
    excursion: Excursion<Job>,
    /// Since when the radio has been away from listening on the home
    /// channel, free: an excursion under way, or a frame being received
    /// (`count_missed_heartbeats`).
    away_since: Option<Instant>,
    /// How long it has been away in all before that, µs, wrapping: only
    /// differences are read.
    away_total_us: u32,
    replies: Replies,
    sweep: Sweep,
    pub(super) beacon: Beacon,
    pub(super) asking: Asking,
    busy: BusyEstimator,
    budget: Budget,
    /// A frame from another node has decoded since boot
    /// (`AppEvent::Heard`).
    heard: bool,
}

impl Radio {
    /// Before the first event: the sweep and the heartbeat's rotation start
    /// on the given channels.
    pub(super) const fn new(config: &Config, sweep: Channel, rotation: Channel) -> Radio {
        Radio {
            excursion: Excursion::new(),
            away_since: None,
            away_total_us: 0,
            replies: Replies::new(),
            sweep: Sweep::new(sweep),
            beacon: Beacon::new(rotation),
            asking: Asking::IDLE,
            busy: BusyEstimator::from_config(config),
            budget: Budget::new(),
            heard: false,
        }
    }

    // --- The excursion (`docs/core-interface.md` §3 rule 1) ---

    pub(super) fn idle(&self) -> bool {
        self.excursion.idle()
    }

    /// The job whose excursion just completed, if one was outstanding.
    pub(super) fn complete(&mut self) -> Option<Job> {
        self.excursion.complete()
    }

    pub(super) fn out_for(&self, job: Job) -> bool {
        self.excursion.step_of(job).is_some()
    }

    pub(super) fn check(&mut self, job: Job, channel: Channel, emit: &mut dyn FnMut(Action)) {
        self.excursion.check(job, channel, emit);
    }

    /// Transmit `frame` for `job`, paid from the budget.
    pub(super) fn send(
        &mut self,
        job: Job,
        frame: Frame,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        self.budget.record(airtime(&frame), now);
        self.excursion.transmit(job, frame, emit);
    }

    /// Have the driver fill and transmit the piece frame `frame` for
    /// `job`, paid from the budget (`Action::Serve`).
    pub(super) fn serve(
        &mut self,
        job: Job,
        frame: Frame,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        self.budget.record(airtime(&frame), now);
        self.excursion.serve(job, frame, emit);
    }

    // --- The budget and the busy estimate ---

    pub(super) fn budget(&self) -> &Budget {
        &self.budget
    }

    pub(super) fn busy(&self) -> &BusyEstimator {
        &self.busy
    }

    // --- The sweep (`relay/sweep.rs`) ---

    pub(super) fn sweep_at(&self) -> Instant {
        self.sweep.next_at()
    }

    pub(super) fn restart_sweep(&mut self, now: Instant) {
        self.sweep.restart(now);
    }

    pub(super) fn forget_home_reading(&mut self) {
        self.sweep.forget_home();
    }

    pub(super) fn start_sweep(&mut self, step: Duration, emit: &mut dyn FnMut(Action)) {
        let channel = self.sweep.start(step);
        self.excursion.read(Job::Sweep, channel, emit);
    }

    /// A reading of the sweep came back ([`Sweep::sampled`]).
    pub(super) fn sweep_sample(
        &mut self,
        channel: Channel,
        dbm: i16,
        home: Channel,
        step: Duration,
        now: Instant,
    ) {
        if let Some((c, dbm)) = self.sweep.sampled(channel, dbm, home, step, now) {
            self.busy.sample(c, dbm);
        }
    }

    /// A reception that follows this node, of `airtime`, ended at `now`
    /// ([`Sweep::set_aside`]).
    pub(super) fn set_aside_home_reading(&mut self, airtime: Duration, now: Instant) {
        self.sweep.set_aside(airtime, now);
    }

    pub(super) fn move_at(&self) -> Option<Instant> {
        self.sweep.move_at()
    }

    pub(super) fn move_due(&self, now: Instant) -> bool {
        self.sweep.move_at().is_some_and(|t| now >= t)
    }

    pub(super) fn clear_move(&mut self) {
        self.sweep.clear_move();
    }

    /// A round of readings ended ([`Sweep::move_rule`]).
    pub(super) fn move_rule(
        &mut self,
        excess: bool,
        dwell: Duration,
        wait: impl FnOnce() -> Duration,
        now: Instant,
    ) {
        self.sweep.move_rule(excess, dwell, wait, now);
    }

    // --- Away from listening ---

    /// How long the radio has been away in all up to `until` (at most
    /// `now`), µs, wrapping; only as far as the core knows: a frame it did
    /// not decode is not reported (`docs/core-interface.md` §3 rule 3).
    pub(super) fn away_us(&self, now: Instant, until: Instant) -> u32 {
        let open = self
            .away_since
            .map_or(0, |a| until.min(now).since(a).as_micros());
        self.away_total_us.wrapping_add(open as u32)
    }

    /// A frame that started at `started` has been received: the radio was
    /// away from then, if not already.
    pub(super) fn received_from(&mut self, started: Instant) {
        let since = self.away_since.map_or(started, |a| a.min(started));
        self.away_since = Some(since);
    }

    pub(super) fn went(&mut self, now: Instant) {
        self.away_since.get_or_insert(now);
    }

    /// The radio is back listening at `now`, unless an excursion is
    /// still out.
    pub(super) fn back(&mut self, now: Instant) {
        if let (Some(a), true) = (self.away_since, self.excursion.idle()) {
            self.away_total_us = self
                .away_total_us
                .wrapping_add(now.since(a).as_micros() as u32);
            self.away_since = None;
        }
    }

    pub(super) fn first_heard(&mut self) -> bool {
        !core::mem::replace(&mut self.heard, true)
    }

    // --- The replies owed (`transfer.rs`) ---

    pub(super) fn replies(&self) -> &Replies {
        &self.replies
    }

    /// Whether `reply` on `channel` can be owed now ([`Replies::can_owe`]).
    pub(super) fn can_owe(
        &mut self,
        reply: impl Into<Reply>,
        channel: Channel,
        now: Instant,
    ) -> bool {
        let sending = self.out_for(Job::Reply);
        self.replies.can_owe(reply, channel, sending, now)
    }

    /// Owe `reply` on `channel` until `deadline` ([`Replies::owe`]).
    pub(super) fn owe(
        &mut self,
        reply: impl Into<Reply>,
        channel: Channel,
        deadline: Instant,
        renews: bool,
        takes: Option<Take>,
        now: Instant,
    ) {
        let sending = self.out_for(Job::Reply);
        self.replies
            .owe(reply, channel, deadline, renews, takes, sending, now);
    }

    /// The message of a reply abandoned since last asked, whose DATA the
    /// node did not take.
    pub(super) fn untaken(&mut self) -> Option<MsgId> {
        self.replies.untaken()
    }

    pub(super) fn reply_due(&mut self, now: Instant) -> Option<Owed> {
        self.replies.due(now)
    }

    /// Give up the first owed reply: no budget for it.
    pub(super) fn abandon_reply(&mut self) {
        self.replies.abandon();
    }

    /// The reply being sent left the antenna ([`Replies::sent`]).
    pub(super) fn reply_sent(&mut self) -> (bool, Option<Take>) {
        self.replies.sent()
    }

    /// A listen-before-talk reading of a node that does not sweep, for
    /// the noise floor of `channel` (spec "Busy estimate").
    pub(super) fn floor_reading(&mut self, channel: Channel, dbm: i16, window: u32) {
        self.busy.floor_reading(channel, dbm, window);
    }

    #[cfg(test)]
    pub(super) fn budget_mut(&mut self) -> &mut Budget {
        &mut self.budget
    }

    #[cfg(test)]
    pub(super) fn busy_mut(&mut self) -> &mut BusyEstimator {
        &mut self.busy
    }
}

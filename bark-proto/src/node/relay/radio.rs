//! The radio's side of the node (`relay.rs` "Radio discipline"): the one
//! excursion outstanding and the job that owns it, the duty-cycle budget
//! every transmission is paid from (spec "Duty cycle cap"), the busy
//! estimate (spec "Busy estimate"), how long the radio has been away from
//! listening, and the replies this node owes (spec "The ACK"). Beside them
//! it holds what each job keeps between its excursions, each owned by its
//! job's module.

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

/// The radio's side: the one excursion outstanding, what each job keeps
/// between its excursions, and the airtime budget and the busy estimate
/// that gate them.
pub(super) struct Radio {
    excursion: Excursion<Job>,
    /// Since when the radio has been away from listening on the home
    /// channel, free: an excursion under way, or a frame being received
    /// (`relay/heartbeat.rs`, `count_missed_heartbeats`).
    away_since: Option<Instant>,
    /// How long it has been away in all before that, µs, wrapping: only
    /// differences are read.
    away_total_us: u32,
    /// The replies this node owes (`transfer.rs`).
    replies: Replies,
    /// The busy sweep (`relay/sweep.rs`).
    sweep: Sweep,
    /// The heartbeat (`relay/heartbeat.rs`).
    pub(super) beacon: Beacon,
    /// A neighbour's status asked or told (`relay/status.rs`).
    pub(super) asking: Asking,
    busy: BusyEstimator,
    budget: Budget,
    /// A frame from another node has decoded since boot
    /// (`AppEvent::Heard`).
    heard: bool,
}

impl Radio {
    /// Before the first event: the sweep from `sweep`, the heartbeat's
    /// rotation from `rotation`.
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

    /// Whether no excursion is outstanding.
    pub(super) fn idle(&self) -> bool {
        self.excursion.idle()
    }

    /// The job whose excursion just completed, if one was outstanding.
    pub(super) fn complete(&mut self) -> Option<Job> {
        self.excursion.complete()
    }

    /// Whether `job`'s excursion is outstanding.
    pub(super) fn out_for(&self, job: Job) -> bool {
        self.excursion.step_of(job).is_some()
    }

    /// Start a listen-before-talk check on `channel` for `job`.
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

    /// The duty-cycle budget.
    pub(super) fn budget(&self) -> &Budget {
        &self.budget
    }

    /// The busy estimates and noise floor.
    pub(super) fn busy(&self) -> &BusyEstimator {
        &self.busy
    }

    // --- The sweep (`relay/sweep.rs`) ---

    /// When the sweep's next reading is due.
    pub(super) fn sweep_at(&self) -> Instant {
        self.sweep.next_at()
    }

    /// The sweep's next reading is due at `now`.
    pub(super) fn restart_sweep(&mut self, now: Instant) {
        self.sweep.restart(now);
    }

    /// The home channel moved: a home reading held is forgotten.
    pub(super) fn forget_home_reading(&mut self) {
        self.sweep.forget_home();
    }

    /// Start the sweep's next reading, the one after it a `step` later.
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

    // --- Away from listening ---

    /// How long the radio has been away in all up to `until`, at most
    /// `now`, µs, wrapping; as far as the core knows: a frame it did not
    /// decode is not known to it (`docs/core-interface.md` §3 rule 3).
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

    /// An excursion started at `now`.
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

    /// Whether this is the first frame from another node since boot.
    pub(super) fn first_heard(&mut self) -> bool {
        !core::mem::replace(&mut self.heard, true)
    }

    // --- The replies owed (`transfer.rs`) ---

    /// The replies this node owes.
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

    /// The reply to start sending now, if one is ready.
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

    #[cfg(test)]
    pub(super) fn budget_mut(&mut self) -> &mut Budget {
        &mut self.budget
    }

    #[cfg(test)]
    pub(super) fn busy_mut(&mut self) -> &mut BusyEstimator {
        &mut self.busy
    }
}

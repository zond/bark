//! The reboot into an image gathered over the mesh, as the firmware's
//! driver does it (spec "Updates over the mesh", the install; roadmap
//! D170; `bark-fw/src/node.rs`, `carry.rs`): once a node's image is whole
//! and staged it drains while `bark_image::policy::drains` says so, and
//! after each event checks `policy::quiet`; at the quiet moment it keeps
//! what its core still holds (`Relay::held`) in the slots the firmware
//! writes to its external flash (`bark_app_layer::carry`), goes off the
//! air for the swap, and comes back with a new core that offers each kept
//! message again as its own, one at a time, before its application's.
//!
//! The image becomes whole either when the update's pieces make it so
//! (`update.rs`, with `[update]`) or at a time drawn for each node
//! (`scenario::Installs`), without the pieces. The simulator has no phone
//! links and its mailboxes write at once, so the moment's links are none
//! and nothing is written.

use crate::rng::{stream_seed, Purpose, Stream};
use crate::sim::{secs, Ev, Sim};
use bark_app_layer::carry::{decode, encode, HELD_MAX, SLOT_LEN};
use bark_image::policy::{self, Links, Moment};
use bark_proto::update::Image;
use bark_proto::{AppCommand, BatteryLevels, Config, Core, Duration, Event, Instant, MsgId, Relay};
use std::collections::{HashSet, VecDeque};
use std::fmt::Write;

/// The swap's time off the air: the loader copies the 227 pages of the
/// running slot, each an internal erase of 85 ms and 1024 word writes of
/// 41 µs (nRF52840 Product Specification, NVMC), an external erase of
/// 10 ms and 16 page programs of 1.3 ms (ZD25WQ16B datasheet, Table 4-3)
/// and 8 KiB over the external flash's bus, about 30 ms; then the hash,
/// 3 s, and the start of the 32.768 kHz clock, at most 1 s
/// (`bark-fw/README.md`, "Updates").
pub const SWAP_US: u64 =
    227 * (85_000 + 1024 * 41 + 10_000 + 16 * 1_300 + 30_000) + 3_000_000 + 1_000_000;

/// Where a node's install stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// It runs; no image is staged.
    Running,
    /// An image is staged: it drains, and reboots at the quiet moment.
    Staged,
    /// Off the air for the swap.
    Swapping,
}

/// One node's reboot into its image.
#[derive(Clone, Debug)]
pub struct InstallRow {
    pub node: u32,
    /// It was not sleepy when its image became whole.
    pub relay: bool,
    /// When its image became whole and was staged.
    pub staged: Instant,
    /// When its core began to drain.
    pub drained_from: Option<Instant>,
    /// When it went off the air for the swap.
    pub rebooted: Option<Instant>,
    /// Messages its core held then, kept across the reboot.
    pub kept: usize,
    /// Of them, those for its sleepy nodes and its own application.
    pub kept_delivering: usize,
    /// When it was back.
    pub back: Option<Instant>,
    /// Refusals for draining by its core, from its staging to its reboot.
    pub refused: u64,
}

/// The installs of a run.
pub(crate) struct InstallModel {
    phase: Vec<Phase>,
    /// The image staged, for an update gathered from its pieces; `None`
    /// for an image whose pieces are not simulated.
    image: Vec<Option<Image>>,
    /// A check of the moment is queued.
    due: Vec<bool>,
    /// The messages kept across the reboot not yet offered again, in the
    /// slots the firmware writes.
    carry: Vec<VecDeque<[u8; SLOT_LEN]>>,
    /// The slot offered last, until its message is given back.
    offered: Vec<Option<[u8; SLOT_LEN]>>,
    /// Per node, its row in `rows` while it installs.
    row: Vec<Option<usize>>,
    /// Refusals for draining at the node before its staging.
    refused_before: Vec<u64>,
    pub(crate) rows: Vec<InstallRow>,
    /// Every message kept across a reboot, by its ID.
    pub(crate) kept: HashSet<u32>,
    /// Kept messages offered again, and given back.
    pub(crate) offered_again: [u64; 2],
}

impl InstallModel {
    fn new(n: usize) -> InstallModel {
        InstallModel {
            phase: vec![Phase::Running; n],
            image: vec![None; n],
            due: vec![false; n],
            carry: vec![VecDeque::new(); n],
            offered: vec![None; n],
            row: vec![None; n],
            refused_before: vec![0; n],
            rows: Vec::new(),
            kept: HashSet::new(),
            offered_again: [0; 2],
        }
    }

    /// Whether the node is off the air for its swap.
    pub(crate) fn swapping(&self, node: u32) -> bool {
        self.phase[node as usize] == Phase::Swapping
    }

    /// The messages kept across reboots and not yet offered again.
    pub(crate) fn waiting(&self) -> impl Iterator<Item = MsgId> + '_ {
        self.carry
            .iter()
            .flatten()
            .chain(self.offered.iter().flatten())
            .filter_map(|s| decode(s).map(|m| m.msg_id))
    }
}

impl Sim {
    /// Draw which nodes install and when each one's image becomes whole
    /// (`scenario::Installs`); with an update, nodes install as they
    /// gather it whatever is drawn.
    pub(crate) fn install_setup(&mut self, sc: &crate::Scenario) -> Result<(), String> {
        let n = self.node_count();
        if sc.update.is_none() && sc.install.is_none() {
            return Ok(());
        }
        self.install = Some(InstallModel::new(n));
        let Some(p) = &sc.install else {
            return Ok(());
        };
        if p.over_s < 0.0 || !(0.0..=1.0).contains(&p.share) {
            return Err("install: over_s must be at least 0 and share within 0 to 1".into());
        }
        let from = p.from_s.unwrap_or_else(|| sc.settled_s());
        for i in 0..n as u32 {
            let mut r = Stream::new(sc.seed, Purpose::Install, i as u64);
            if r.chance(p.share) {
                let at = from + r.uniform() * p.over_s;
                self.push(Instant::ZERO + secs(at), Ev::Stage(i));
            }
        }
        Ok(())
    }

    /// `node`'s image is whole and staged: `image` names it when its
    /// pieces were simulated. A node off, or with an image staged
    /// already, stages nothing.
    pub(crate) fn stage(&mut self, node: u32, image: Option<Image>) {
        let alive = self.nodes[node as usize].alive;
        let sleepy = self.relays[node as usize].sleepy();
        let refused = self.metrics.nodes[node as usize].core.refused.draining;
        let Some(m) = self.install.as_mut() else {
            return;
        };
        let i = node as usize;
        if !alive || m.phase[i] != Phase::Running {
            return;
        }
        m.phase[i] = Phase::Staged;
        m.image[i] = image;
        m.refused_before[i] = refused;
        m.row[i] = Some(m.rows.len());
        m.rows.push(InstallRow {
            node,
            relay: !sleepy,
            staged: self.now,
            drained_from: None,
            rebooted: None,
            kept: 0,
            kept_delivering: 0,
            back: None,
            refused: 0,
        });
        self.check_moment(node);
    }

    /// After an event at `node`: if it has an image staged, check the
    /// moment once the event is done, as the firmware does after each
    /// step of its driver.
    pub(crate) fn after_install_event(&mut self, node: u32) {
        let Some(m) = self.install.as_mut() else {
            return;
        };
        let i = node as usize;
        if m.phase[i] == Phase::Staged && !m.due[i] {
            m.due[i] = true;
            self.push(self.now, Ev::Install(node));
        }
    }

    /// Whether a swap may start on `node`'s power: on USB, or on a
    /// battery above the low level (roadmap D143); without cells in the
    /// run, a battery is taken to allow it.
    fn swap_allowed(&self, node: u32) -> bool {
        let levels = self.relays[node as usize]
            .config()
            .battery
            .unwrap_or(BatteryLevels::DESIGN);
        let mv = self.millivolts(node).unwrap_or(u16::MAX);
        levels.swap_allowed(self.external[node as usize], mv)
    }

    /// The firmware's step for a staged image (`bark-fw/src/node.rs`): ask
    /// the core to drain or to take messages again as the policy says,
    /// and reboot at the quiet moment.
    pub(crate) fn check_moment(&mut self, node: u32) {
        let i = node as usize;
        let Some(m) = self.install.as_mut() else {
            return;
        };
        m.due[i] = false;
        if m.phase[i] != Phase::Staged || !self.nodes[i].alive {
            return;
        }
        let local = self.clocks[i].local(self.now);
        let core = &self.relays[i];
        let moment = Moment {
            links: Links::None,
            drained: core.drained(local),
            writing: false,
            phone_update: false,
            swap_allowed: self.swap_allowed(node),
        };
        let draining = core.draining();
        let want = policy::drains(moment);
        if want != draining {
            let m = self.install.as_mut().expect("installs");
            if want {
                let row = m.row[i].expect("staged");
                m.rows[row].drained_from.get_or_insert(self.now);
            }
            self.deliver(node, Event::App(AppCommand::Drain(want)));
        }
        if policy::quiet(moment) {
            self.install_reboot(node);
        }
    }

    /// Keep what the core holds and go off the air for the swap.
    fn install_reboot(&mut self, node: u32) {
        let i = node as usize;
        let slots: Vec<[u8; SLOT_LEN]> = self.relays[i]
            .held()
            .take(HELD_MAX)
            .filter_map(|h| encode(&h))
            .collect();
        let core = &self.relays[i];
        let delivering = core.delivery_queue().count() + core.inbox_len();
        let refused = self.metrics.nodes[i].core.refused.draining;
        let m = self.install.as_mut().expect("installs");
        let row = m.row[i].expect("staged");
        let r = &mut m.rows[row];
        r.rebooted = Some(self.now);
        r.kept = slots.len();
        r.kept_delivering = delivering;
        r.refused = refused - m.refused_before[i];
        for s in &slots {
            if let Some(h) = decode(s) {
                m.kept.insert(h.msg_id.0);
            }
        }
        m.carry[i].extend(slots);
        m.phase[i] = Phase::Swapping;
        self.power_off(node, "rebooting into the staged image");
        self.push(self.now + Duration::from_micros(SWAP_US), Ev::Swapped(node));
    }

    /// The swap is done: the node starts the new image, with what it kept
    /// in flash, and offers the kept messages again.
    pub(crate) fn swapped(&mut self, node: u32) {
        let i = node as usize;
        let m = self.install.as_mut().expect("installs");
        let image = m.image[i].take();
        m.phase[i] = Phase::Running;
        let row = m.row[i].take().expect("staged");
        m.rows[row].back = Some(self.now);
        let st = &mut self.nodes[i];
        st.alive = true;
        // A stream apart from the restarts', as a board's hardware
        // generator gives another after each reset.
        let index = 1 << 63 | (row as u64) << 32 | node as u64;
        let seed = stream_seed(self.seed, Purpose::CoreSeed, index);
        let r = &mut self.relays[i];
        let config = Config {
            stored: self.stored[i],
            version: image.map_or(r.config().version, |im| im.version),
            ..*r.config()
        };
        *r = Relay::new(config, seed);
        self.power_on(node);
        if let Some(im) = image {
            self.deliver(node, Event::App(AppCommand::Holds(im)));
        }
        self.set_presence(node);
        self.offer(node);
    }

    /// Offer `node`'s core the next kept message, ahead of its
    /// application's: whether one was offered.
    pub(crate) fn offer_kept(&mut self, node: u32) -> bool {
        let i = node as usize;
        let Some(m) = self.install.as_mut() else {
            return false;
        };
        let Some(s) = m.carry[i].pop_front() else {
            return false;
        };
        // The one offered before and not given back was taken.
        m.offered[i] = Some(s);
        m.offered_again[0] += 1;
        let h = decode(&s).expect("a slot kept is whole");
        let cmd = AppCommand::Send {
            dst_pos: h.dst_pos,
            dst_id: h.dst_id,
            payload: h.payload,
        };
        self.deliver(node, Event::App(cmd));
        true
    }

    /// Every node's reboot into its image, in the order they were staged.
    pub fn install_rows(&self) -> &[InstallRow] {
        self.install.as_ref().map_or(&[], |m| &m.rows)
    }

    /// Whether `node` has kept messages to offer.
    pub(crate) fn has_kept(&self, node: u32) -> bool {
        self.install
            .as_ref()
            .is_some_and(|m| !m.carry[node as usize].is_empty())
    }

    /// The core gave `msg_id` back: whether it was the kept message
    /// offered last, which is then offered again first.
    pub(crate) fn kept_given_back(&mut self, node: u32, msg_id: MsgId) -> bool {
        let i = node as usize;
        let Some(m) = self.install.as_mut() else {
            return false;
        };
        let ours = m.offered[i]
            .and_then(|s| decode(&s).map(|h| h.msg_id))
            .is_some_and(|id| id == msg_id);
        if ours {
            let s = m.offered[i].take().expect("offered");
            m.carry[i].push_front(s);
            m.offered_again[1] += 1;
        }
        ours
    }

    /// The installs' report: per node, the time from its staging to its
    /// reboot against the bound, what it kept, and what became of the
    /// messages kept.
    pub fn install_report(&self) -> Option<String> {
        let m = self.install.as_ref()?;
        let mut out = String::new();
        let staged = m.rows.len();
        let rebooted: Vec<&InstallRow> = m.rows.iter().filter(|r| r.rebooted.is_some()).collect();
        let back = m.rows.iter().filter(|r| r.back.is_some()).count();
        let bound = self.relays.first().map(|r| r.config().delivery_ttl);
        let _ = writeln!(
            out,
            "installs: {staged} staged, {} rebooted, {back} back",
            rebooted.len()
        );
        for (what, relay) in [("relays", true), ("sleepy", false)] {
            let mut waits: Vec<f64> = rebooted
                .iter()
                .filter(|r| r.relay == relay)
                .map(|r| r.rebooted.expect("rebooted").since(r.staged).as_micros() as f64 / 1e6)
                .collect();
            if waits.is_empty() {
                continue;
            }
            waits.sort_by(f64::total_cmp);
            let at_bound = rebooted
                .iter()
                .filter(|r| r.relay == relay && r.kept > 0)
                .count();
            let kept: usize = rebooted
                .iter()
                .filter(|r| r.relay == relay)
                .map(|r| r.kept)
                .sum();
            let refused: u64 = rebooted
                .iter()
                .filter(|r| r.relay == relay)
                .map(|r| r.refused)
                .sum();
            let _ = writeln!(
                out,
                "  {what}: {} rebooted; staging to reboot min {:.1} s, median {:.1} s, max {:.1} s (bound {:.0} s); {at_bound} kept messages, {kept} in all; {refused} refused for draining",
                waits.len(),
                waits[0],
                waits[waits.len() / 2],
                waits[waits.len() - 1],
                bound.map_or(0.0, |b| b.as_micros() as f64 / 1e6),
            );
        }
        let fates = self.kept_fates();
        let _ = writeln!(
            out,
            "  kept across a reboot: {} messages; offered again {} times, given back {}; delivered {}, dropped {}, in flight {}, unaccounted {}",
            m.kept.len(),
            m.offered_again[0],
            m.offered_again[1],
            fates[0],
            fates[1],
            fates[2],
            fates[3],
        );
        Some(out)
    }

    /// Per node that installed, one row: node, relay, staged, drained
    /// from, rebooted, back (s), messages kept and of them those for its
    /// sleepy nodes and itself, refusals for draining.
    pub fn install_csv(&self) -> Option<String> {
        let m = self.install.as_ref()?;
        let s = |t: Option<Instant>| {
            t.map_or(String::new(), |t| {
                format!("{:.3}", t.as_micros() as f64 / 1e6)
            })
        };
        let mut out = String::from("node,relay,staged_s,drained_from_s,rebooted_s,back_s,kept,kept_delivering,refused_draining\n");
        for r in &m.rows {
            let _ = writeln!(
                out,
                "{},{},{},{},{},{},{},{},{}",
                r.node,
                u8::from(r.relay),
                s(Some(r.staged)),
                s(r.drained_from),
                s(r.rebooted),
                s(r.back),
                r.kept,
                r.kept_delivering,
                r.refused
            );
        }
        Some(out)
    }
}

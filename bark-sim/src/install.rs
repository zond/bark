//! The reboot into an image gathered over the mesh, as the firmware's
//! driver does it (spec "Updates over the mesh", the install; roadmap
//! D170; `bark-fw/src/node.rs`, `carry.rs`): once a node's image is whole
//! and staged it drains while `bark_image::policy::drains` says so, and
//! after each event checks `policy::quiet`; at the quiet moment it keeps
//! what its core still holds (`Relay::held`) in the slots the firmware
//! writes to its external flash (`bark_app_layer::carry`) and its tables
//! as the firmware writes them (`bark_app_layer::tables`, roadmap D180),
//! goes off the air for the swap, and comes back with a new core that
//! takes its tables back, aged by the swap, and puts back each kept
//! message where it was (roadmap D201); the application's own message,
//! if it held one, it offers again first.
//!
//! The image becomes whole either when the update's pieces make it so
//! (`update.rs`, with `[update]`) or at a time drawn for each node
//! (`scenario::Installs`), without the pieces. The simulator has no phone
//! links and its mailboxes write at once, so the moment's links are none
//! and nothing is written.

use crate::rng::{stream_seed, Purpose, Stream};
use crate::sim::{secs, Ev, Sim};
use bark_app_layer::carry::{decode, encode, HELD_MAX, SLOT_LEN};
use bark_app_layer::tables::{self, Check, Header};
use bark_image::policy::{self, Links, Moment};
use bark_proto::update::Image;
use bark_proto::{
    AppCommand, BatteryLevels, Config, Core, Duration, Event, HeldAt, Instant, MsgId, Relay,
};
use std::collections::{HashMap, VecDeque};
use std::fmt::Write;

/// The swap's time off the air, 59.0 s: the loader's figures for an image
/// that fills the slot (`bark_image::boot::time`), as the simulator does
/// not model the image's length.
pub const SWAP_US: u64 = bark_image::boot::time::swap_us(bark_image::layout::ACTIVE.len);

/// Where a kept message was, for the report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeptAt {
    Own,
    Forward,
    /// In the delivery queue, for a sleepy node or a leaving neighbour.
    Delivery,
    Inbox,
}

impl KeptAt {
    fn of(at: HeldAt) -> KeptAt {
        match at {
            HeldAt::Own => KeptAt::Own,
            HeldAt::Forward { .. } => KeptAt::Forward,
            HeldAt::Delivery { .. } => KeptAt::Delivery,
            HeldAt::Inbox { .. } => KeptAt::Inbox,
        }
    }
}

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
    /// Its neighbour entries and its message records kept across the
    /// reboot, and of them those its new core took back.
    pub tables: [usize; 4],
}

/// The installs of a run.
pub(crate) struct InstallModel {
    phase: Vec<Phase>,
    /// The image staged, for an update gathered from its pieces; `None`
    /// for an image whose pieces are not simulated.
    image: Vec<Option<Image>>,
    /// A check of the moment is queued.
    due: Vec<bool>,
    /// The messages kept across the reboot not yet put back or offered
    /// again, in the slots the firmware writes.
    carry: Vec<VecDeque<[u8; SLOT_LEN]>>,
    /// The slot offered last, until its message is given back.
    offered: Vec<Option<[u8; SLOT_LEN]>>,
    /// The tables kept across the reboot, as the firmware writes them.
    tables: Vec<Vec<u8>>,
    /// Per node, its row in `rows` while it installs.
    row: Vec<Option<usize>>,
    /// Refusals for draining at the node before its staging.
    refused_before: Vec<u64>,
    pub(crate) rows: Vec<InstallRow>,
    /// Every message kept across a reboot, by its ID, and where it was
    /// the last time it was kept.
    pub(crate) kept: HashMap<u32, KeptAt>,
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
            tables: vec![Vec::new(); n],
            row: vec![None; n],
            refused_before: vec![0; n],
            rows: Vec::new(),
            kept: HashMap::new(),
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

/// The tables `core` keeps across a reboot at `now`, as the firmware
/// writes them (`bark-fw/src/tables.rs`), and how many neighbour entries
/// and records they hold.
fn written_tables(core: &Relay, now: Instant) -> (Vec<u8>, [usize; 2]) {
    let mut check = Check::new();
    let mut body = Vec::new();
    let mut counts = [0; 2];
    for k in core.remembered(now) {
        counts[usize::from(matches!(k, bark_proto::Kept::Record(_)))] += 1;
        check.add(tables::bytes(&k));
        body.extend_from_slice(tables::bytes(&k));
    }
    let h = Header::new(counts[0] as u16, counts[1] as u16, 0, check);
    let mut out = h.encode().to_vec();
    out.extend(body);
    (out, counts)
}

/// Give `core`, new after the swap, the tables `written` kept, as the
/// firmware does at its boot at `now`, the swap having taken [`SWAP_US`]:
/// the neighbour entries and records it took back, and how long before
/// `now` they were taken.
fn take_back(core: &mut Relay, written: &[u8], now: Instant) -> ([usize; 2], Option<Duration>) {
    let mut back = [0; 2];
    let Some(h) = written
        .get(..tables::HEADER_LEN)
        .and_then(|b| Header::decode(b.try_into().ok()?))
    else {
        return (back, None);
    };
    let ago = Duration::from_micros(SWAP_US + u64::from(h.written_us));
    for i in 0..h.entries() {
        let (at, len) = h.place(i);
        let Some(k) = written.get(at..at + len).and_then(|b| h.entry(i, b)) else {
            return (back, Some(ago));
        };
        if core.remember(&k, ago, now) {
            back[usize::from(matches!(k, bark_proto::Kept::Record(_)))] += 1;
        }
    }
    (back, Some(ago))
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
            tables: [0; 4],
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
            // No phone or mailbox to wait for.
            waited_ms: 0,
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
        let local = self.clocks[i].local(self.now);
        let slots: Vec<[u8; SLOT_LEN]> = self.relays[i]
            .held(local)
            .take(HELD_MAX)
            .filter_map(|h| encode(&h))
            .collect();
        let core = &self.relays[i];
        let (written, counts) = written_tables(core, local);
        let delivering = core.delivery_queue().count() + core.inbox_len();
        let refused = self.metrics.nodes[i].core.refused.draining;
        let m = self.install.as_mut().expect("installs");
        let row = m.row[i].expect("staged");
        let r = &mut m.rows[row];
        r.rebooted = Some(self.now);
        r.kept = slots.len();
        r.kept_delivering = delivering;
        r.refused = refused - m.refused_before[i];
        r.tables[..2].copy_from_slice(&counts);
        m.tables[i] = written;
        for s in &slots {
            if let Some(h) = decode(s) {
                m.kept.insert(h.msg_id.0, KeptAt::of(h.at));
            }
        }
        m.carry[i].extend(slots);
        m.phase[i] = Phase::Swapping;
        self.power_off(node, "rebooting into the staged image");
        self.push(self.now + Duration::from_micros(SWAP_US), Ev::Swapped(node));
    }

    /// The swap is done: the node starts the new image, with what it kept
    /// in flash, puts back the kept messages and offers its application's
    /// own again.
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
        let local = self.clocks[i].local(self.now);
        let m = self.install.as_mut().expect("installs");
        let written = core::mem::take(&mut m.tables[i]);
        let (back, ago) = take_back(r, &written, local);
        m.rows[row].tables[2..].copy_from_slice(&back);
        // Unknown, the time away is taken as the delivery TTL: a queued
        // message is then stale, never fresher than it is.
        let ago = ago.unwrap_or(r.config().delivery_ttl);
        let mut own = VecDeque::new();
        let mut lost = Vec::new();
        for s in core::mem::take(&mut m.carry[i]) {
            let h = decode(&s).expect("a slot kept is whole");
            if h.at == HeldAt::Own {
                own.push_back(s);
            } else if !r.put_back(&h, ago, local) {
                lost.push(h.msg_id);
            }
        }
        m.carry[i] = own;
        for id in lost {
            self.message_dropped(node, id, bark_proto::DropReason::SendQueueFull);
        }
        self.power_on(node);
        // What it put back in its inbox, its application reads now.
        while let Some((msg_id, _)) = self.relays[i].inbox() {
            self.app_event(node, bark_proto::AppEvent::Message { msg_id });
        }
        if let Some(im) = image {
            self.deliver(node, Event::App(AppCommand::Holds(im)));
        }
        self.set_presence(node);
        self.offer(node);
    }

    /// Offer `node`'s core its application's own message kept across the
    /// reboot, ahead of what is new: whether it was.
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
        let sum = |k: usize| m.rows.iter().map(|r| r.tables[k]).sum::<usize>();
        let _ = writeln!(
            out,
            "  tables kept across a reboot: {} neighbour entries, {} taken back; {} records, {} taken back",
            sum(0),
            sum(2),
            sum(1),
            sum(3),
        );
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
        for (what, at) in [
            ("own", KeptAt::Own),
            ("forwards", KeptAt::Forward),
            ("in the delivery queue", KeptAt::Delivery),
            ("in the inbox", KeptAt::Inbox),
        ] {
            let [d, x, f, u] = self.kept_fates_of(|k| k == at);
            if d + x + f + u > 0 {
                let _ = writeln!(
                    out,
                    "    {what}: delivered {d}, dropped {x}, in flight {f}, unaccounted {u}"
                );
            }
        }
        let mut why = std::collections::BTreeMap::new();
        for f in self.kept_messages(|_| true) {
            if let crate::sim::Fate::Dropped { reason, .. } = f {
                *why.entry(format!("{reason:?}")).or_insert(0u64) += 1;
            }
        }
        if !why.is_empty() {
            let _ = writeln!(out, "  kept and dropped, by cause: {why:?}");
        }
        Some(out)
    }

    /// Per node that installed, one row: node, relay, staged, drained
    /// from, rebooted, back (s), messages kept and of them those for its
    /// sleepy nodes and itself, refusals for draining, and the neighbour
    /// entries and records kept and taken back.
    pub fn install_csv(&self) -> Option<String> {
        let m = self.install.as_ref()?;
        let s = |t: Option<Instant>| {
            t.map_or(String::new(), |t| {
                format!("{:.3}", t.as_micros() as f64 / 1e6)
            })
        };
        let mut out = String::from("node,relay,staged_s,drained_from_s,rebooted_s,back_s,kept,kept_delivering,refused_draining,neighbours_kept,records_kept,neighbours_back,records_back\n");
        for r in &m.rows {
            let _ = writeln!(
                out,
                "{},{},{},{},{},{},{},{},{},{},{},{},{}",
                r.node,
                u8::from(r.relay),
                s(Some(r.staged)),
                s(r.drained_from),
                s(r.rebooted),
                s(r.back),
                r.kept,
                r.kept_delivering,
                r.refused,
                r.tables[0],
                r.tables[1],
                r.tables[2],
                r.tables[3]
            );
        }
        Some(out)
    }
}

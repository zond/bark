//! The driver's side of an update spreading over the mesh, without bytes
//! (spec "Updates over the mesh", `docs/core-interface.md` "Application"):
//! a block decodes once the node has been handed as many distinct pieces
//! as the block is made of. The run measures when each node came to hold
//! the image, and per sample how many hold it, the relays' busy estimates
//! and the update's airtime.

use crate::scenario::{resolve, PlacedNode, Scenario};
use crate::sim::{key, secs, Ev, Sim};
use bark_proto::radio::time_on_air;
use bark_proto::update::{Image, Piece, BLOCK_PIECES};
use bark_proto::wire::{Packet, Update, HEARTBEAT_LEN, PIECE_BYTES};
use bark_proto::{Action, AppCommand, Duration, Frame, Instant, Trace};
use std::collections::HashMap;
use std::fmt::Write;

/// The version a forger's sections name: far above any release.
pub const FORGED: u32 = 0x7fff_ffff;

/// How often a row of the series is taken.
pub const SAMPLE_US: u64 = 600_000_000;

/// The pieces each block of an image of `bytes` bytes is made of
/// (spec "Updates over the mesh"): block 0 the block table, the image's
/// name and 8 bytes of each block's hash, its own last, one piece up to 26
/// blocks, two up to 48; then the image in blocks of [`BLOCK_PIECES`]
/// pieces, the last shorter.
pub fn blocks(bytes: u32) -> Vec<usize> {
    let pieces = (bytes as usize).div_ceil(PIECE_BYTES);
    let full = pieces / BLOCK_PIECES;
    let mut b = vec![0];
    b.extend(std::iter::repeat_n(BLOCK_PIECES, full));
    if !pieces.is_multiple_of(BLOCK_PIECES) {
        b.push(pieces % BLOCK_PIECES);
    }
    b[0] = (32 + 8 * b.len()).div_ceil(PIECE_BYTES);
    b
}

/// The run's update.
pub struct UpdateRun {
    pub image: Image,
    needs: Vec<usize>,
    /// The relay given the image.
    pub holder: u32,
    /// When it was given it.
    pub at: Instant,
    /// Per node and block: pieces handed to it.
    kept: std::collections::HashMap<(u32, u8), u16>,
    /// Per node: blocks decoded.
    decoded: Vec<u8>,
    /// Per node: when it came to hold the image.
    pub holds_at: Vec<Option<Instant>>,
    /// Commands the driver owes cores, handed after the event at hand.
    pub owed: Vec<(u32, AppCommand<'static>)>,
    /// The series: one row every [`SAMPLE_US`].
    series: String,
    /// Airtime of the update by kind (heartbeat sections, none, pieces),
    /// and of every frame, µs, since the last row, and in all.
    air: [u64; 4],
    air_total: [u64; 4],
    /// Pieces on the air not yet heard by a neighbour that lacked them,
    /// as the sender's table said: block, index and that neighbour.
    expected: std::collections::HashMap<(u8, u8, u32), usize>,
    /// Such neighbours a piece was sent to, and those that heard it:
    /// relays, and sleepy nodes, which listen only at times.
    targeted: [u64; 2],
    reached: [u64; 2],
    /// When each relay last sent a piece on each channel, and its pieces
    /// a pace or more after the one before there, and within a pace.
    last_piece: std::collections::HashMap<(u32, u8), Instant>,
    paced: [u64; 2],
    /// When each relay last sent a heartbeat on its home channel.
    home_beats: std::collections::HashMap<u32, Instant>,
    /// Per relay that lacked a piece sent from another channel than the
    /// sender's home: the last such sender and when.
    last_foreign: std::collections::HashMap<u32, (u32, Instant)>,
    /// Such pieces to such relays, and those that came within a pace of
    /// one from another such sender.
    foreign: [u64; 2],
    /// What became of the pieces due after anchors' slot heartbeats.
    slots: std::collections::BTreeMap<String, u64>,
    /// The blocks each node holds.
    pub has: std::collections::HashSet<(u32, u8)>,
    /// What became of each slot piece at each sleepy node of its anchor.
    fates: std::collections::BTreeMap<&'static str, u64>,
    /// Slot pieces a sleepy node was listening for, until it hears them.
    listened: std::collections::HashSet<(u8, u8, u32)>,
    /// Pieces stored by sleepy nodes.
    pub stored_sleepy: u64,
    /// Anchors' home heartbeats, by whether the slot opened after them.
    opens: std::collections::BTreeMap<(bool, &'static str), u64>,
    /// Pieces of the image heard by relays that gather it, those of a
    /// block they held already, and those whose index of the block they
    /// had heard before: what the senders spent for nothing.
    gathered: [u64; 3],
    /// The pieces each gathering relay heard, by block and index.
    heard_by: std::collections::HashSet<(u32, u8, u8)>,
    /// The forger and when it stops (`scenario::UpdateSpec::forger`).
    pub forger: Option<(u32, Instant)>,
    /// The image its sections name.
    pub forged: Image,
    /// The nodes that heard the forger's section, and those offered its
    /// image.
    pub forged_heard: std::collections::BTreeSet<u32>,
    pub forged_offered: std::collections::BTreeSet<u32>,
    /// Per node, when it began to gather the forged image, while it does.
    forged_since: std::collections::HashMap<u32, Instant>,
    /// The longest any node gathered it at a stretch.
    pub forged_longest: bark_proto::Duration,
    /// Its gathers given up unproven.
    pub forged_given_up: usize,
    /// A second release, one version up, and per node when it came to hold
    /// it (`scenario::UpdateSpec::next_at_s`).
    pub next: Image,
    pub next_holds_at: Vec<Option<Instant>>,
    /// Per node, the image it gathers.
    gathers: Vec<Option<Image>>,
    /// The release's gathers given up unproven: the node and when.
    pub dropped: Vec<(u32, Instant)>,
    /// Per node gathering the release, when it began, until its proof.
    began: std::collections::HashMap<u32, Instant>,
    /// How long each gather of the release took to its proof.
    pub proved_after: Vec<bark_proto::Duration>,
    /// Each anchor's sleepy nodes, as of the last row of the series.
    sleepers: HashMap<u32, Vec<u32>>,
    /// The last heartbeat each node heard, its sender and when.
    beat_from: HashMap<u32, (u64, Instant)>,
}

impl UpdateRun {
    pub fn new(image_bytes: u32, holder: u32, at: Instant, nodes: usize) -> UpdateRun {
        let needs = blocks(image_bytes);
        let image = Image {
            version: 2,
            blocks: needs.len() as u8,
            last: *needs.last().expect("blocks") as u8,
            key: 0,
        };
        UpdateRun {
            image,
            needs,
            holder,
            at,
            kept: std::collections::HashMap::new(),
            decoded: vec![0; nodes],
            holds_at: vec![None; nodes],
            owed: Vec::new(),
            series: String::from(
                "t_s,relays,relays_holding,placed_relays,placed_holding,sleepy,sleepy_holding,gathering,gathering_block_mean,gathering_block_max,busy_home_mean,busy_home_p90,air_section_s,air_unused_s,air_piece_s,air_all_s\n",
            ),
            air: [0; 4],
            air_total: [0; 4],
            expected: std::collections::HashMap::new(),
            targeted: [0; 2],
            reached: [0; 2],
            home_beats: std::collections::HashMap::new(),
            last_piece: std::collections::HashMap::new(),
            paced: [0; 2],
            last_foreign: std::collections::HashMap::new(),
            foreign: [0; 2],
            slots: std::collections::BTreeMap::new(),
            has: std::collections::HashSet::new(),
            fates: std::collections::BTreeMap::new(),
            listened: std::collections::HashSet::new(),
            stored_sleepy: 0,
            opens: std::collections::BTreeMap::new(),
            gathered: [0; 3],
            heard_by: std::collections::HashSet::new(),
            forger: None,
            forged: Image {
                version: FORGED,
                ..image
            },
            forged_heard: std::collections::BTreeSet::new(),
            forged_offered: std::collections::BTreeSet::new(),
            forged_since: std::collections::HashMap::new(),
            forged_longest: bark_proto::Duration::ZERO,
            forged_given_up: 0,
            next: Image {
                version: image.version + 1,
                ..image
            },
            next_holds_at: vec![None; nodes],
            gathers: vec![None; nodes],
            dropped: Vec::new(),
            began: std::collections::HashMap::new(),
            proved_after: Vec::new(),
            sleepers: HashMap::new(),
            beat_from: HashMap::new(),
        }
    }

    /// The image is given to its first holder.
    pub fn give(&mut self, now: Instant) {
        self.holds_at[self.holder as usize] = Some(now);
    }

    /// The second release is given to the first holder.
    pub fn give_next(&mut self, now: Instant) {
        self.next_holds_at[self.holder as usize] = Some(now);
    }

    /// The policy takes every image offered.
    pub fn offered(&mut self, node: u32, image: Image, now: Instant) {
        self.forged_ends(node, now);
        if image == self.forged {
            self.forged_offered.insert(node);
            self.forged_since.insert(node, now);
            self.began.remove(&node);
        } else {
            self.began.insert(node, now);
        }
        self.gathers[node as usize] = Some(image);
        self.kept.retain(|k, _| k.0 != node);
        self.has.retain(|k| k.0 != node);
        self.decoded[node as usize] = 0;
        self.owed
            .push((node, AppCommand::Gather { image, from: 0 }));
    }

    /// The core gave up the image it gathered unproven: its pieces are
    /// forgotten.
    pub fn unproven(&mut self, node: u32, version: u32, now: Instant) {
        if version == self.forged.version {
            self.forged_ends(node, now);
            self.forged_given_up += 1;
        } else {
            self.dropped.push((node, now));
            self.began.remove(&node);
        }
        self.kept.retain(|k, _| k.0 != node);
        self.has.retain(|k| k.0 != node);
        self.decoded[node as usize] = 0;
    }

    fn forged_ends(&mut self, node: u32, now: Instant) {
        if let Some(t) = self.forged_since.remove(&node) {
            self.forged_longest = self.forged_longest.max(now.since(t));
        }
    }

    /// The longest any node has gathered the forged image at a stretch,
    /// those that still do counted to `now`, and how many still do.
    pub fn forged_gathers(&self, now: Instant) -> (bark_proto::Duration, usize) {
        let open = self.forged_since.values().map(|&t| now.since(t)).max();
        (
            open.map_or(self.forged_longest, |o| o.max(self.forged_longest)),
            self.forged_since.len(),
        )
    }

    /// Node `node` heard the forger's section.
    pub fn heard_forged(&mut self, node: u32) {
        self.forged_heard.insert(node);
    }

    /// A heartbeat of the forger, while it forges, carries the forged
    /// section in place of its own.
    pub fn forge(&self, node: u32, f: &mut bark_proto::Frame, now: Instant) {
        use bark_proto::update::{Section, SECTION_LEN};
        use bark_proto::wire::{encode_section, Packet, HEARTBEAT_LEN};
        if self
            .forger
            .is_none_or(|(n, until)| n != node || now >= until)
            || !matches!(Packet::decode(f.payload()), Ok(Packet::Heartbeat(_)))
        {
            return;
        }
        let section = Section {
            runs: 1,
            image: Some(self.forged),
            complete: bark_proto::update::PROOF_BLOCKS,
            have: 0,
        };
        let mut b = [0u8; SECTION_LEN];
        encode_section(&section, &mut b);
        f.bytes[HEARTBEAT_LEN..HEARTBEAT_LEN + SECTION_LEN].copy_from_slice(&b);
        f.len = (HEARTBEAT_LEN + SECTION_LEN) as u8;
    }

    /// A piece kept; a block decodes at its pieces, and the last block
    /// makes the node hold the image, its signature taken as checked.
    pub fn piece(&mut self, node: u32, p: Piece, now: Instant) {
        let k = self.kept.entry((node, p.block)).or_default();
        *k += 1;
        if usize::from(*k) != self.needs[usize::from(p.block)] {
            return;
        }
        self.kept.remove(&(node, p.block));
        self.has.insert((node, p.block));
        let proof = (0..bark_proto::update::PROOF_BLOCKS).all(|b| self.has.contains(&(node, b)));
        if proof {
            if let Some(t) = self.began.remove(&node) {
                self.proved_after.push(now.since(t));
            }
        }
        let d = &mut self.decoded[node as usize];
        *d += 1;
        let all = usize::from(*d) == self.needs.len();
        self.owed.push((
            node,
            AppCommand::Block {
                index: p.block,
                ok: true,
            },
        ));
        if all {
            let image = self.gathers[node as usize].take().unwrap_or(self.image);
            self.owed.push((node, AppCommand::Holds(image)));
            let at = match image == self.next {
                true => &mut self.next_holds_at,
                false => &mut self.holds_at,
            };
            at[node as usize].get_or_insert(now);
        }
    }

    /// A slot piece at one of its anchor's sleepy nodes: `fate` says why
    /// it could not take it, or `None` when it should listen for it.
    pub fn slot_fate(&mut self, node: u32, block: u8, index: u8, fate: Option<&'static str>) {
        match fate {
            Some(f) => *self.fates.entry(f).or_default() += 1,
            None => {
                *self.fates.entry("listened for").or_default() += 1;
                self.listened.insert((block, index, node));
            }
        }
    }

    /// Whether an anchor opened the slot after a home heartbeat, or why not.
    /// Counted apart for the first 15 hours, while the relays gather, and
    /// after.
    pub fn slot_open_fate(&mut self, why: &'static str, now: Instant) {
        let early = now.since(self.at).as_micros() < 15 * 3_600_000_000;
        *self.opens.entry((early, why)).or_default() += 1;
    }

    /// What became of a piece due after a slot heartbeat.
    pub fn slot_piece(&mut self, o: bark_proto::SlotPiece) {
        *self.slots.entry(format!("{o:?}")).or_default() += 1;
    }

    /// Relay `node` sent a heartbeat on its home channel.
    pub fn home_beat(&mut self, node: u32, now: Instant) {
        self.home_beats.insert(node, now);
    }

    /// Whether a frame of `node` at `now` is the one right after its slot
    /// heartbeat: 40 ms from its start covers the heartbeat with its
    /// section, 22.08 ms, and the piece's check and start, under 5 ms.
    pub fn after_home_beat(&self, node: u32, now: Instant) -> bool {
        self.home_beats
            .get(&node)
            .is_some_and(|&t| now.since(t).as_micros() < 40_000)
    }

    /// Relay `sender` sent a piece on `channel` at `now`: within a pace of
    /// its last there, or not.
    pub fn piece_gap(&mut self, sender: u32, channel: u8, now: Instant) {
        let last = self.last_piece.insert((sender, channel), now);
        let fast = last.is_some_and(|t| now.since(t) < bark_proto::update::PACE);
        self.paced[usize::from(fast)] += 1;
    }

    /// Relay `sender` sent piece `index` of `block` at `now`, from
    /// another channel than its home if `foreign`, to `targets`: the
    /// neighbours its table said lack it, each with whether it is sleepy,
    /// sleepy ones only for their anchor's piece after its slot.
    pub fn piece_sent(
        &mut self,
        sender: u32,
        foreign: bool,
        now: Instant,
        block: u8,
        index: u8,
        targets: Vec<(u32, bool)>,
    ) {
        for (t, sleepy) in targets {
            if foreign && !sleepy {
                self.foreign[0] += 1;
                let pace = bark_proto::update::PACE.as_micros();
                if let Some(&(other, at)) = self.last_foreign.get(&t) {
                    if other != sender && now.since(at).as_micros() < pace {
                        self.foreign[1] += 1;
                    }
                }
                self.last_foreign.insert(t, (sender, now));
            }
            let k = usize::from(sleepy);
            self.targeted[k] += 1;
            // A target that missed the last copy of this piece missed it.
            self.expected.insert((block, index, t), k);
        }
    }

    /// Relay `node`, which gathers the image, heard piece `index` of
    /// `block`: whether it needed it.
    pub fn gatherer_heard(&mut self, node: u32, block: u8, index: u8) {
        self.gathered[0] += 1;
        if self.has.contains(&(node, block)) {
            self.gathered[1] += 1;
        } else if !self.heard_by.insert((node, block, index)) {
            self.gathered[2] += 1;
        }
    }

    /// A node heard piece `index` of `block`.
    pub fn piece_heard(&mut self, node: u32, block: u8, index: u8) {
        if self.listened.remove(&(block, index, node)) {
            *self.fates.entry("listened for, heard").or_default() += 1;
        }
        if let Some(k) = self.expected.remove(&(block, index, node)) {
            self.reached[k] += 1;
        }
    }

    /// A frame went on the air: `kind` 0 to 2 for the update's, else
    /// `None`.
    pub fn aired(&mut self, kind: Option<usize>, us: u64) {
        if let Some(k) = kind {
            self.air[k] += us;
            self.air_total[k] += us;
        }
        self.air[3] += us;
        self.air_total[3] += us;
    }

    /// A row of the series. `nodes` says, per node now alive, whether it
    /// is a relay, else sleepy, and whether it was placed as a relay;
    /// `gathering` has the first block each node that gathers lacks;
    /// `busy` the busy share of each relay's home channel as it estimates
    /// it.
    pub fn sample(
        &mut self,
        now: Instant,
        nodes: &[(bool, bool)],
        gathering: &[u8],
        mut busy: Vec<f64>,
    ) {
        let holds = |i: usize| self.holds_at[i].is_some();
        let mut c = [0usize; 6];
        for (i, &(relay, placed)) in nodes.iter().enumerate() {
            let h = usize::from(holds(i));
            if relay {
                c[0] += 1;
                c[1] += h;
            } else {
                c[4] += 1;
                c[5] += h;
            }
            if placed {
                c[2] += 1;
                c[3] += h;
            }
        }
        busy.sort_by(f64::total_cmp);
        let mean = busy.iter().sum::<f64>() / busy.len().max(1) as f64;
        let p90 = busy
            .get((busy.len() * 9 / 10).min(busy.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0.0);
        let s = |us: u64| us as f64 / 1e6;
        let _ = writeln!(
            self.series,
            "{},{},{},{},{},{},{},{},{:.2},{},{:.5},{:.5},{:.3},{:.3},{:.3},{:.3}",
            now.as_micros() / 1_000_000,
            c[0],
            c[1],
            c[2],
            c[3],
            c[4],
            c[5],
            gathering.len(),
            gathering.iter().map(|&b| f64::from(b)).sum::<f64>() / gathering.len().max(1) as f64,
            gathering.iter().max().copied().unwrap_or(0),
            mean,
            p90,
            s(self.air[0]),
            s(self.air[1]),
            s(self.air[2]),
            s(self.air[3]),
        );
        self.air = [0; 4];
    }

    /// The series, and after it one row a node that came to hold the
    /// image: `node,<index>,<seconds after the image was given>`.
    pub fn csv(&self) -> String {
        let mut out = self.series.clone();
        let mut proved: Vec<u64> = self
            .proved_after
            .iter()
            .map(|d| d.as_micros() / 1_000_000)
            .collect();
        proved.sort_unstable();
        let at =
            |q: usize| proved.get((proved.len() * q / 100).min(proved.len().saturating_sub(1)));
        let _ = writeln!(
            out,
            "# gathers proven {}, after s: median {:?}, p90 {:?}, p99 {:?}, most {:?}; given up unproven {}; unproven at the end {}, begun at s {:?}",
            proved.len(),
            at(50),
            at(90),
            at(99),
            proved.last(),
            self.dropped.len(),
            self.began.len(),
            self.began.values().map(|t| t.as_micros() / 1_000_000).min(),
        );
        out.push_str("# node,index,holds_after_s\n");
        for (i, t) in self.holds_at.iter().enumerate() {
            if let Some(t) = t {
                let _ = writeln!(
                    out,
                    "node,{i},{:.1}",
                    t.since(self.at).as_micros() as f64 / 1e6
                );
            }
        }
        let s = |us: u64| us as f64 / 1e6;
        let _ = writeln!(
            out,
            "# airtime in all, s: heartbeat sections {:.1}, unused {:.1}, piece {:.1}, all {:.1}",
            s(self.air_total[0]),
            s(self.air_total[1]),
            s(self.air_total[2]),
            s(self.air_total[3]),
        );
        let all: u64 =
            self.fates.values().sum::<u64>() - self.fates.get("listened for, heard").unwrap_or(&0);
        let _ = write!(out, "# slot pieces at their anchor's sleepy nodes: {all}");
        for (k, n) in &self.fates {
            let _ = write!(
                out,
                ", {k} {n} ({:.1} %)",
                100.0 * *n as f64 / all.max(1) as f64
            );
        }
        let _ = writeln!(
            out,
            "; pieces stored by sleepy nodes {}",
            self.stored_sleepy
        );
        for (early, when) in [(true, "first 15 h"), (false, "after")] {
            let all: u64 = self
                .opens
                .iter()
                .filter(|(k, _)| k.0 == early)
                .map(|(_, n)| n)
                .sum();
            let _ = write!(out, "# anchors' home heartbeats, {when}: {all}");
            for ((e, k), n) in &self.opens {
                if *e == early {
                    let _ = write!(
                        out,
                        ", {k} {n} ({:.1} %)",
                        100.0 * *n as f64 / all.max(1) as f64
                    );
                }
            }
            out.push('\n');
        }
        let _ = writeln!(
            out,
            "# pieces a pace or more after the sender's last on the channel {}, within a pace {}",
            self.paced[0], self.paced[1]
        );
        let all: u64 = self.slots.values().sum();
        let _ = write!(out, "# pieces due after a slot heartbeat: {all}");
        for (k, n) in &self.slots {
            let _ = write!(
                out,
                ", {k} {n} ({:.1} %)",
                100.0 * *n as f64 / all.max(1) as f64
            );
        }
        out.push('\n');
        let _ = writeln!(
            out,
            "# pieces from another channel than the sender's home to relays that lacked them: {}, within a pace of another such sender's {} ({:.1} %)",
            self.foreign[0],
            self.foreign[1],
            100.0 * self.foreign[1] as f64 / self.foreign[0].max(1) as f64,
        );
        let g = self.gathered;
        let share = |n: u64| 100.0 * n as f64 / g[0].max(1) as f64;
        let _ = writeln!(
            out,
            "# pieces heard by relays that gathered: {}, of a block held already {} ({:.1} %), a piece heard before {} ({:.1} %)",
            g[0],
            g[1],
            share(g[1]),
            g[2],
            share(g[2]),
        );
        for (k, who) in ["relays", "sleepy nodes after their anchor's slot"]
            .iter()
            .enumerate()
        {
            let _ = writeln!(
                out,
                "# pieces to {who} that lacked them: {}, heard {}, lost {:.1} %",
                self.targeted[k],
                self.reached[k],
                100.0 * (1.0 - self.reached[k] as f64 / self.targeted[k].max(1) as f64),
            );
        }
        out
    }
}

impl Sim {
    /// The update's first relay, the time it is given the image, and the
    /// series' first row (`update.rs`).
    pub(crate) fn update_setup(
        &mut self,
        u: &crate::scenario::UpdateSpec,
        sc: &Scenario,
        placed: &[PlacedNode],
    ) -> Result<(), String> {
        let holder = match &u.holder {
            Some(name) => resolve(placed, name)?,
            None => {
                let placed_relays: Vec<u32> = (0..self.relays())
                    .filter(|&i| self.placed_relay(i))
                    .collect();
                let n = self.xy.len() as f64;
                let cx = self.xy.iter().map(|p| p.x).sum::<f64>() / n;
                let cy = self.xy.iter().map(|p| p.y).sum::<f64>() / n;
                let d = |i: &u32| {
                    let p = self.xy[*i as usize];
                    (p.x - cx).hypot(p.y - cy)
                };
                *placed_relays
                    .iter()
                    .min_by(|a, b| d(a).total_cmp(&d(b)))
                    .ok_or("[update] needs a node placed as a relay")?
            }
        };
        let at = Instant::ZERO + secs(u.at_s.unwrap_or_else(|| sc.settled_s()));
        let mut run = crate::update::UpdateRun::new(u.image_bytes, holder, at, self.xy.len());
        if let Some(name) = &u.forger {
            let until = u
                .forged_until_s
                .map_or(Instant::MAX, |s| Instant::ZERO + secs(s));
            run.forger = Some((resolve(placed, name)?, until));
        }
        self.update = Some(run);
        self.push(at, Ev::Update);
        self.push(at, Ev::UpdateSample);
        if let Some(s) = u.next_at_s {
            self.push(Instant::ZERO + secs(s), Ev::UpdateNext);
        }
        Ok(())
    }

    /// The update's run, if the scenario has one.
    pub fn update(&self) -> Option<&crate::update::UpdateRun> {
        self.update.as_ref()
    }

    /// The update's series, and when each node came to hold the image.
    pub fn update_csv(&self) -> Option<String> {
        self.update.as_ref().map(|u| u.csv())
    }

    /// A row of the update's series.
    pub(crate) fn update_sample(&mut self) {
        let Some(mut u) = self.update.take() else {
            return;
        };
        let nodes: Vec<(bool, bool)> = (0..self.relays.len())
            .map(|i| {
                let placed = self.placed_relay(i as u32);
                let alive = self.nodes[i].alive;
                (alive && !self.relays[i].sleepy(), alive && placed)
            })
            .collect();
        let busy = (0..self.relays.len())
            .filter(|&i| nodes[i].0)
            .map(|i| {
                let r = &self.relays[i];
                r.busy().fraction(r.home()) as f64 / bark_proto::tables::busy::ONE as f64
            })
            .collect();
        let gathering: Vec<u8> = (0..self.relays.len())
            .filter(|&i| self.nodes[i].alive)
            .filter_map(|i| self.relays[i].gathering().map(|g| g.1))
            .collect();
        u.sample(self.now, &nodes, &gathering, busy);
        u.sleepers.clear();
        for i in 0..self.relays.len() {
            let r = &self.relays[i];
            if !self.nodes[i].alive || !r.sleepy() {
                continue;
            }
            let Some(a) = r.anchor() else { continue };
            if let Some(&ai) = self
                .relay_index
                .get(&key(a.id))
                .or(self.sleepy_index.get(&key(a.id)))
            {
                u.sleepers.entry(ai).or_default().push(i as u32);
            }
        }
        self.update = Some(u);
        self.push(
            self.now + Duration::from_micros(crate::update::SAMPLE_US),
            Ev::UpdateSample,
        );
    }

    /// `node`'s core asks for something of the update's driver side.
    pub(crate) fn update_action(&mut self, node: u32, a: Action) {
        let Some(u) = self.update.as_mut() else {
            return;
        };
        match a {
            Action::Offered(image) => u.offered(node, image, self.now),
            Action::Unproven(version) => u.unproven(node, version, self.now),
            Action::StorePiece(p) => {
                if self.relays[node as usize].sleepy() {
                    u.stored_sleepy += 1;
                }
                u.piece(node, p, self.now);
            }
            Action::Trace(Trace::SlotPiece(o)) => u.slot_piece(o),
            _ => {}
        }
    }

    /// `node` hears `bytes`: a piece, or a heartbeat with a section.
    pub(crate) fn update_received(&mut self, node: u32, bytes: &[u8]) {
        let Some(u) = self.update.as_mut() else {
            return;
        };
        match Packet::decode(bytes) {
            Ok(Packet::Update(Update::Piece {
                version,
                block,
                index,
                ..
            })) => {
                let r = &self.relays[node as usize];
                let gathers =
                    !r.sleepy() && r.gathering().is_some_and(|(i, _)| i.version == version);
                if gathers {
                    u.gatherer_heard(node, block, index);
                }
                u.piece_heard(node, block, index)
            }
            Ok(Packet::Heartbeat(hb)) => {
                u.beat_from.insert(node, (key(hb.id), self.now));
                let forger = u.forger.map(|(f, _)| self.relays[f as usize].id());
                let section = bark_proto::wire::decode_section(bytes);
                if forger == Some(hb.id) && section.and_then(|s| s.image) == Some(u.forged) {
                    u.heard_forged(node);
                }
            }
            _ => {}
        }
    }

    /// `node` sends `f`: its airtime by what it carries of the update, a
    /// piece's targets by the sender's table, and what became of the slot
    /// after an anchor's heartbeat and of the piece that follows it.
    pub(crate) fn update_transmit(&mut self, node: u32, f: &Frame) {
        let air = time_on_air(f.len, f.preamble_symbols).as_micros();
        let decoded = Packet::decode(f.payload());
        let home = self.relays[node as usize].home();
        let home_beat = f.channel == home && matches!(decoded, Ok(Packet::Heartbeat(_)));
        let open = match (home_beat, self.update.as_ref()) {
            (true, Some(u)) => u
                .sleepers
                .get(&node)
                .map(|own| self.slot_opens(node, own, u)),
            _ => None,
        };
        let Some(u) = self.update.as_mut() else {
            return;
        };
        if home_beat {
            u.home_beat(node, self.now);
            if let Some(why) = open {
                u.slot_open_fate(why, self.now);
            }
        }
        match decoded {
            // A heartbeat's section: what it adds to the frame.
            Ok(Packet::Heartbeat(_)) if usize::from(f.len) > HEARTBEAT_LEN => {
                let bare = time_on_air(HEARTBEAT_LEN as u8, f.preamble_symbols).as_micros();
                u.aired(Some(0), air - bare);
                u.aired(None, bare);
            }
            Ok(Packet::Update(Update::Piece { block, index, .. })) => {
                u.aired(Some(2), air);
                let image = u.image;
                let table = self.relays[node as usize].neighbour_table();
                let (relays, sleepy) = (&self.relay_index, &self.sleepy_index);
                let me = self.relays[node as usize].id();
                let cores = &self.relays;
                // A sleepy node listens only for its anchor's piece right
                // after the anchor's slot heartbeat.
                let slot = u.after_home_beat(node, self.now);
                let targets = table
                    .live(self.clocks[node as usize].local(self.now))
                    .map(|(_, n)| n)
                    .filter(|n| n.home == f.channel)
                    .filter(|n| {
                        table.section(n).is_some_and(|s| {
                            s.gathers(image.version)
                                && (s.complete < block
                                    || s.complete == block && s.have < image.pieces(block))
                        })
                    })
                    .filter_map(|n| {
                        let i = relays.get(&key(n.id)).or(sleepy.get(&key(n.id)));
                        i.map(|&i| (i, n.sleepy))
                    })
                    .filter(|&(i, sleepy)| {
                        !sleepy || slot && cores[i as usize].anchor().is_some_and(|a| a.id == me)
                    })
                    .collect::<Vec<_>>();
                let foreign = f.channel != home;
                u.piece_gap(node, f.channel.index(), self.now);
                u.piece_sent(node, foreign, self.now, block, index, targets);
                if slot && !foreign {
                    let me_key = key(me);
                    let own = u.sleepers.get(&node).cloned().unwrap_or_default();
                    for s in own {
                        let c = &self.relays[s as usize];
                        let st = &self.nodes[s as usize];
                        if c.anchor().is_none_or(|a| a.id != me) {
                            u.slot_fate(s, block, index, Some("anchor changed"));
                            continue;
                        }
                        let fate = if !st.alive || !st.present {
                            Some("absent")
                        } else if let Some((g, first)) = c.gathering() {
                            let local = self.clocks[s as usize].local(self.now);
                            let t = c.neighbour_table();
                            let anchor_says = t
                                .live(local)
                                .find(|(_, n)| n.id == me)
                                .and_then(|(_, n)| t.section(n))
                                .filter(|x| x.image.is_some_and(|i| i.version == g.version))
                                .map_or(0, |x| x.complete);
                            let heard_beat = u.beat_from.get(&s).is_some_and(|&(k, t)| {
                                k == me_key && self.now.since(t).as_micros() < 60_000
                            });
                            if u.has.contains(&(s, block)) {
                                Some("has the block")
                            } else if !heard_beat {
                                Some("missed the slot heartbeat")
                            } else if anchor_says <= first {
                                Some("anchor's section says it holds nothing more")
                            } else {
                                None
                            }
                        } else if u.holds_at[s as usize].is_some() {
                            Some("holds the image")
                        } else {
                            Some("not gathering")
                        };
                        u.slot_fate(s, block, index, fate);
                    }
                }
            }
            _ => u.aired(None, air),
        }
    }

    /// Whether relay `a`, an anchor of the sleepy nodes `own`, opens the
    /// slot after its heartbeat on its home channel, and if not, why, by
    /// the simulator's truth against its table.
    fn slot_opens(&self, a: u32, own: &[u32], u: &UpdateRun) -> &'static str {
        let r = &self.relays[a as usize];
        let image = u.image;
        let whole = r.image().is_some();
        let holds = |b: u8| whole || u.has.contains(&(a, b));
        if !whole && !(0..image.blocks).any(holds) {
            return "anchor holds no block";
        }
        let me = r.id();
        let lacking = own.iter().any(|&s| {
            let c = &self.relays[s as usize];
            self.nodes[s as usize].alive
                && c.anchor().is_some_and(|x| x.id == me)
                && c.gathering().is_some()
                && (0..image.blocks).any(|b| holds(b) && !u.has.contains(&(s, b)))
        });
        if !lacking {
            return "no sleepy node of its own lacks a block it holds";
        }
        let local = self.clocks[a as usize].local(self.now);
        let table = r.neighbour_table();
        let table_says = table
            .live(local)
            .map(|(_, n)| n)
            .filter(|n| n.sleepy && n.home == r.home())
            .filter_map(|n| table.section(n))
            .filter(|x| x.gathers(image.version))
            .any(|x| {
                (x.complete..image.blocks)
                    .any(|b| holds(b) && (b > x.complete || x.have < image.pieces(b)))
            });
        if !table_says {
            return "its table's sleepy sections show nothing lacking";
        }
        "opens"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn todays_image_is_the_table_and_27_blocks() {
        // 420 808 B packed in pieces of 247 B: 1704 pieces, 26 blocks of 64
        // and one of 40, after the table's block of two pieces.
        let b = blocks(420_808);
        assert_eq!(b.len(), 28);
        assert_eq!(b[0], 2);
        assert_eq!(b[27], 40);
        assert_eq!(b.iter().sum::<usize>(), 2 + 1704);
    }
}

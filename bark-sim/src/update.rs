//! The driver's side of an update spreading over the mesh, without bytes
//! (spec "Updates over the mesh", `docs/core-interface.md` §4): the policy
//! takes every image offered, a piece kept is counted, and a block
//! decodes once the node has been handed as many distinct pieces as the
//! block is made of. What the run measures: when each node came to hold
//! the image, and every ten minutes how many hold it, the channels' busy
//! share as the relays estimate it, and the update's airtime.

use bark_proto::update::{Image, Piece, BLOCK_PIECES};
use bark_proto::wire::PIECE_BYTES;
use bark_proto::{AppCommand, Instant};
use std::fmt::Write;

/// How often a row of the series is taken.
pub const SAMPLE_US: u64 = 600_000_000;

/// The pieces each block of an image of `bytes` bytes is made of: block 0
/// the block table, the image's hash and 8 bytes of each block's hash,
/// one piece up to 26 blocks, two up to 57 (spec "Updates over the mesh"); then the image in blocks
/// of [`BLOCK_PIECES`] pieces, the last one shorter.
pub fn blocks(bytes: u32) -> Vec<usize> {
    let pieces = (bytes as usize).div_ceil(PIECE_BYTES);
    let full = pieces / BLOCK_PIECES;
    let mut b = vec![0];
    b.extend(std::iter::repeat_n(BLOCK_PIECES, full));
    if !pieces.is_multiple_of(BLOCK_PIECES) {
        b.push(pieces % BLOCK_PIECES);
    }
    b[0] = (32 + 8 * (b.len() - 1)).div_ceil(PIECE_BYTES);
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
    /// When each relay last sent a heartbeat on its home channel.
    home_beats: std::collections::HashMap<u32, Instant>,
    /// Per relay that lacked a piece sent from another channel than the
    /// sender's home: the last such sender and when.
    last_foreign: std::collections::HashMap<u32, (u32, Instant)>,
    /// Such pieces to such relays, and those that came within a pace of
    /// one from another such sender.
    foreign: [u64; 2],
}

impl UpdateRun {
    pub fn new(image_bytes: u32, holder: u32, at: Instant, nodes: usize) -> UpdateRun {
        let needs = blocks(image_bytes);
        let image = Image {
            name: bark_proto::update::ImageTag([0x5a; 4]),
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
            last_foreign: std::collections::HashMap::new(),
            foreign: [0; 2],
        }
    }

    /// The image is given to its first holder.
    pub fn give(&mut self, now: Instant) {
        self.holds_at[self.holder as usize] = Some(now);
    }

    /// The policy takes every image offered.
    pub fn offered(&mut self, node: u32, image: Image) {
        self.kept.retain(|k, _| k.0 != node);
        self.decoded[node as usize] = 0;
        self.owed
            .push((node, AppCommand::Gather { image, from: 0 }));
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
            self.owed.push((node, AppCommand::Holds(self.image)));
            self.holds_at[node as usize].get_or_insert(now);
        }
    }

    /// Relay `node` sent a heartbeat on its home channel.
    pub fn home_beat(&mut self, node: u32, now: Instant) {
        self.home_beats.insert(node, now);
    }

    /// Whether a frame of `node` at `now` is the one right after its slot
    /// heartbeat: within the heartbeat's airtime with its section, 23.36
    /// ms, and the piece's check and start, under 5 ms, with a margin, 40
    /// ms from the heartbeat's start.
    pub fn after_home_beat(&self, node: u32, now: Instant) -> bool {
        self.home_beats
            .get(&node)
            .is_some_and(|&t| now.since(t).as_micros() < 40_000)
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

    /// A node heard piece `index` of `block`.
    pub fn piece_heard(&mut self, node: u32, block: u8, index: u8) {
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

    /// A row of the series. `relays` and `sleepy` say, per node now alive,
    /// whether it is a relay or sleepy and whether it was placed as a
    /// relay; `busy` the busy share of each relay's home channel as it
    /// estimates it.
    /// `gathering` has the first block each node that gathers lacks.
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
        let _ = writeln!(
            out,
            "# pieces from another channel than the sender's home to relays that lacked them: {}, within a pace of another such sender's {} ({:.1} %)",
            self.foreign[0],
            self.foreign[1],
            100.0 * self.foreign[1] as f64 / self.foreign[0].max(1) as f64,
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

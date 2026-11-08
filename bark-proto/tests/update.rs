//! Firmware images spreading over the mesh
//! (spec "Updates over the mesh"), driven through real frames on the test medium, with a driver
//! side that keeps no bytes.

mod common;

use bark_proto::update::{sweep_pieces, Image, ImageTag, PACE, SERVED_CHANNELS};
use bark_proto::wire::{Packet, Update};
use bark_proto::{AppCommand, Config, Instant, Position, Relay};
use common::{at, secs, Harness, Updates};

fn pos(east: f64) -> Position {
    let m_per_deg = 6_371_000.0 * core::f64::consts::PI / 180.0;
    Position::from_degrees(
        50.45,
        30.52 + east / (m_per_deg * 50.45f64.to_radians().cos()),
    )
}

fn relay(east: f64, seed: u64) -> Relay {
    Relay::new(
        Config {
            position: pos(east),
            ..Config::default()
        },
        seed,
    )
}

/// A relay whose home channel is set.
fn relay_on(east: f64, seed: u64, home: u8) -> Relay {
    Relay::new(
        Config {
            position: pos(east),
            home_override: bark_proto::Channel::new(home),
            ..Config::default()
        },
        seed,
    )
}

/// An image of three blocks: the table, a whole block and a short one.
const IMAGE: Image = Image {
    name: ImageTag([7; 4]),
    version: 2,
    blocks: 3,
    last: 7,
    key: 0,
};
const NEEDS: [usize; 3] = [1, 64, 7];

/// `n` relays that all hear each other, every one with the update's
/// driver side, node 0 told it holds [`IMAGE`] once the first rotations
/// are over.
fn mesh(n: usize) -> Harness<Relay> {
    let mut h = Harness::new(
        (0..n)
            .map(|i| relay(100.0 * i as f64, i as u64 + 1))
            .collect(),
    );
    for a in 0..n {
        for b in a + 1..n {
            h.link(a, b);
        }
        h.nodes[a].updates = Some(Updates {
            needs: NEEDS.to_vec(),
            ..Updates::default()
        });
    }
    h.run_until(at(400), |_| false);
    h.deliver_app(0, AppCommand::Holds(IMAGE));
    h.nodes[0].updates.as_mut().unwrap().holds_at = Some(h.now);
    h
}

fn update_frames(h: &Harness<Relay>, i: usize) -> Vec<(Instant, bark_proto::Channel, u8)> {
    h.nodes[i]
        .sent
        .iter()
        .filter_map(|(t, f)| match Packet::decode(f.payload()) {
            Ok(Packet::Update(u)) => Some((
                *t,
                f.channel,
                match u {
                    Update::Piece { block, .. } => block,
                },
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn every_relay_comes_to_hold_the_image() {
    let mut h = mesh(4);
    let start = h.now;
    let done = h.run_until(start + secs(6 * 3600), |h| {
        h.nodes
            .iter()
            .all(|n| n.updates.as_ref().unwrap().holds_at.is_some())
    });
    assert!(done, "not every relay holds the image within six hours");
    for i in 1..4 {
        assert_eq!(h.nodes[i].core.image(), Some(IMAGE));
        let u = h.nodes[i].updates.as_ref().unwrap();
        assert_eq!(u.offered.first().map(|o| o.1), Some(IMAGE));
        // Never a piece twice, and only of the block gathered.
        let mut seen = std::collections::HashSet::new();
        for (_, p) in &u.pieces {
            assert!(seen.insert((p.block, p.index)), "piece handed twice");
        }
    }
    // The 72 pieces at most one a pace on a channel: at least 71 paces.
    let took = h.now.since(start);
    assert!(took >= PACE.mul(71), "faster than the pace: {took:?}");
    // Each learned of the image from a heartbeat's section.
    for i in 1..4 {
        assert!(h.nodes[i].sent.iter().any(|(_, f)| f.len > 13));
    }
}

#[test]
fn update_frames_keep_the_pace() {
    let mut h = mesh(5);
    let start = h.now;
    h.run_until(start + secs(3 * 3600), |_| false);
    for i in 0..5 {
        let frames = update_frames(&h, i);
        for (k, &(t, c, _)) in frames.iter().enumerate() {
            let mut channels = std::collections::HashSet::new();
            channels.insert(c);
            for &(u, d, _) in &frames[k + 1..] {
                if u.since(t) >= PACE {
                    break;
                }
                assert_ne!(
                    c, d,
                    "node {i}: two update frames on a channel within a pace"
                );
                channels.insert(d);
            }
            assert!(
                channels.len() <= SERVED_CHANNELS,
                "node {i}: update frames on {} channels within a pace",
                channels.len()
            );
        }
    }
}

#[test]
fn a_node_that_holds_the_image_is_offered_nothing_and_two_holders_share_the_pace() {
    let mut h = mesh(3);
    // Node 1 holds the same image already: it is offered nothing.
    h.deliver_app(1, AppCommand::Holds(IMAGE));
    let start = h.now;
    h.run_until(start + secs(2 * 3600), |_| false);
    assert!(h.nodes[1].updates.as_ref().unwrap().offered.is_empty());
    // Node 2 gathers it from both.
    assert!(h.nodes[2].updates.as_ref().unwrap().holds_at.is_some());
    // The two holders share one piece a pace on node 2's channel: together
    // they send at most a pace's worth, and a little over for the moment
    // each takes to hear the other.
    let home = h.nodes[2].core.home();
    let done = h.nodes[2].updates.as_ref().unwrap().holds_at.unwrap();
    let pieces = (0..2)
        .flat_map(|i| update_frames(&h, i))
        .filter(|f| f.1 == home && f.0 < done)
        .count() as u64;
    let paces = done.since(start).as_micros() / PACE.as_micros();
    assert!(
        pieces <= paces + paces / 4 + 2,
        "{pieces} pieces in {paces} paces"
    );
}

/// Today's image: the table of two pieces and 27 blocks, the last of 40
/// pieces, and the pieces each block is made of.
fn todays_image() -> (Image, Vec<usize>) {
    let image = Image {
        name: ImageTag([9; 4]),
        version: 3,
        blocks: 28,
        last: 40,
        key: 0,
    };
    let mut needs = vec![2];
    needs.extend(std::iter::repeat_n(64, 26));
    needs.push(40);
    (image, needs)
}

/// Today's image crosses one hop without loss within one sweep: each
/// block's pieces at the pace at least, and no more than the loss margin,
/// since the neighbours' sections say when a block is done; and to the
/// other nodes too within twelve hours.
#[test]
fn todays_image_crosses_a_hop_in_about_one_sweep() {
    let (image, needs) = todays_image();
    let pieces: u64 = needs.iter().map(|&k| k as u64).sum();
    let sweep: u64 = needs
        .iter()
        .map(|&k| u64::from(sweep_pieces(k as u8)))
        .sum();
    let n = 4;
    let mut h = Harness::new(
        (0..n)
            .map(|i| relay(100.0 * i as f64, i as u64 + 1))
            .collect(),
    );
    for a in 0..n {
        for b in a + 1..n {
            h.link(a, b);
        }
        h.nodes[a].updates = Some(Updates {
            needs: needs.clone(),
            ..Updates::default()
        });
    }
    h.run_until(at(400), |_| false);
    h.deliver_app(0, AppCommand::Holds(image));
    let start = h.now;
    let done = h.run_until(start + secs(12 * 3600), |h| {
        h.nodes[1..]
            .iter()
            .all(|n| n.updates.as_ref().unwrap().holds_at.is_some())
    });
    assert!(done, "not every relay holds the image within twelve hours");
    let first = h.nodes[1..]
        .iter()
        .filter_map(|n| n.updates.as_ref().unwrap().holds_at)
        .min()
        .unwrap();
    let paces = first.since(start).as_micros() / PACE.as_micros();
    assert!(
        (pieces..=sweep).contains(&paces),
        "{paces} paces, {pieces} pieces, a sweep {sweep}"
    );
}

/// A line of three relays, each at home on its own channel, the last out
/// of the first's reach: the middle one serves the blocks it has decoded
/// before it holds the whole image, so the last gathers block by block
/// behind it (spec "Updates over the mesh").
#[test]
fn a_relay_serves_the_blocks_it_holds_before_the_whole_image() {
    let mut h = Harness::new(
        (0..3)
            .map(|i| relay_on(300.0 * i as f64, i as u64 + 7, 3 + i as u8))
            .collect(),
    );
    h.link(0, 1);
    h.link(1, 2);
    let (image, needs) = todays_image();
    for n in &mut h.nodes {
        n.updates = Some(Updates {
            needs: needs.clone(),
            ..Updates::default()
        });
    }
    h.run_until(at(400), |_| false);
    h.deliver_app(0, AppCommand::Holds(image));
    let start = h.now;
    let done = h.run_until(start + secs(24 * 3600), |h| {
        h.nodes[2].updates.as_ref().unwrap().holds_at.is_some()
    });
    assert!(done, "the last relay does not hold the image within a day");
    let middle = h.nodes[1].updates.as_ref().unwrap().holds_at.unwrap();
    let last = h.nodes[2].updates.as_ref().unwrap();
    let first_piece = last.pieces.first().unwrap().0;
    assert!(
        first_piece < middle,
        "the last relay took its first piece only once the middle held the whole image"
    );
    // The middle's heartbeats said what it held as it grew.
    let complete: Vec<u8> = h.nodes[1]
        .sent
        .iter()
        .filter(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Heartbeat(_))))
        .filter_map(|(_, f)| bark_proto::wire::decode_section(f.payload()))
        .map(|s| s.complete)
        .collect();
    assert!(complete.windows(2).all(|w| w[0] <= w[1]), "{complete:?}");
    assert!(complete.iter().any(|&c| c > 0 && c < 28), "{complete:?}");
}

/// A block that fails its hash is gathered again, and a node serves and
/// counts only the blocks that passed.
#[test]
fn a_block_that_fails_its_hash_is_gathered_again_and_not_served() {
    let mut h = Harness::new(
        (0..3)
            .map(|i| relay_on(300.0 * i as f64, i as u64 + 11, 3 + i as u8))
            .collect(),
    );
    h.link(0, 1);
    h.link(1, 2);
    for n in &mut h.nodes {
        n.updates = Some(Updates {
            needs: NEEDS.to_vec(),
            ..Updates::default()
        });
    }
    h.nodes[1].updates.as_mut().unwrap().fail_once = Some(1);
    h.run_until(at(400), |_| false);
    h.deliver_app(0, AppCommand::Holds(IMAGE));
    let start = h.now;
    let done = h.run_until(start + secs(10 * 3600), |h| {
        h.nodes[1..]
            .iter()
            .all(|n| n.updates.as_ref().unwrap().holds_at.is_some())
    });
    assert!(done, "not every relay holds the image");
    // Block 1 was handed twice over at the middle: 64 pieces, then 64 more.
    let ones = h.nodes[1]
        .updates
        .as_ref()
        .unwrap()
        .pieces
        .iter()
        .filter(|(_, p)| p.block == 1)
        .count();
    assert_eq!(ones, 128);
    // Nothing of block 1 went from the middle before it passed.
    let passed = h.nodes[1].updates.as_ref().unwrap().passed[1].unwrap();
    assert!(h.nodes[1].sent.iter().all(|(t, f)| {
        !matches!(
            Packet::decode(f.payload()),
            Ok(Packet::Update(Update::Piece { block: 1, .. }))
        ) || *t > passed
    }));
}

/// A sleepy node gathers from its anchor: its beats say what it lacks, the anchor sends
/// after its slot heartbeat on its home channel, and the node listens for
/// that frame (spec "Updates over the mesh").
#[test]
fn a_sleepy_node_gathers_after_its_anchor_s_slot_heartbeats() {
    let boot = Config::default().boot_listen;
    let anchor = Relay::new(
        Config {
            position: pos(0.0),
            boot_listen: secs(5),
            ..Config::default()
        },
        21,
    );
    let sleepy = Relay::new(
        Config {
            position: pos(50.0),
            boot_listen: boot,
            ..Config::default()
        },
        22,
    );
    let mut h = Harness::powered(vec![(anchor, true), (sleepy, false)]);
    h.link(0, 1);
    for n in &mut h.nodes {
        n.updates = Some(Updates {
            needs: NEEDS.to_vec(),
            ..Updates::default()
        });
    }
    h.run_until(at(1200), |_| false);
    assert!(
        h.nodes[1].core.anchor().is_some(),
        "the sleepy node has an anchor"
    );
    h.deliver_app(0, AppCommand::Holds(IMAGE));
    let start = h.now;
    let done = h.run_until(start + secs(12 * 3600), |h| {
        h.nodes[1].updates.as_ref().unwrap().holds_at.is_some()
    });
    assert!(
        done,
        "the sleepy node does not hold the image within twelve hours"
    );
    // Its beats carried its section, and every piece it took came right
    // after one of its anchor's heartbeats on its channel.
    assert!(h.nodes[1].sent.iter().any(|(_, f)| {
        matches!(Packet::decode(f.payload()), Ok(Packet::Heartbeat(_)))
            && bark_proto::wire::decode_section(f.payload()).is_some()
    }));
    let home = h.nodes[0].core.home();
    let beats: Vec<Instant> = h.nodes[0]
        .sent
        .iter()
        .filter(|(_, f)| f.channel == home)
        .filter(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Heartbeat(_))))
        .map(|(t, _)| *t)
        .collect();
    let after_slot = h.nodes[1]
        .updates
        .as_ref()
        .unwrap()
        .pieces
        .iter()
        .filter(|(t, _)| beats.iter().any(|b| *t > *b && t.since(*b) < secs(1)))
        .count();
    assert_eq!(after_slot, NEEDS.iter().sum::<usize>());
}

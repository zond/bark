//! Firmware images spreading over the mesh
//! (spec "Updates over the mesh"), driven through real frames on the test medium, with a driver
//! side that keeps no bytes.

mod common;

use bark_proto::update::{Image, PACE, SERVED_CHANNELS};
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

/// An image of three blocks: the table, a whole block and a short one.
const IMAGE: Image = Image {
    id: [7; 32],
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
                    Update::Announce { .. } => 0,
                    Update::Request { .. } => 1,
                    Update::Piece { .. } => 2,
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
fn a_node_that_holds_nothing_new_asks_for_nothing() {
    let mut h = mesh(3);
    // Node 1 holds the same image already: it is offered nothing.
    h.deliver_app(1, AppCommand::Holds(IMAGE));
    let start = h.now;
    h.run_until(start + secs(2 * 3600), |_| false);
    assert!(h.nodes[1].updates.as_ref().unwrap().offered.is_empty());
    assert!(update_frames(&h, 1).iter().all(|f| f.2 != 1));
    // Node 2 gathers it, from whichever announced last.
    assert!(h.nodes[2].updates.as_ref().unwrap().holds_at.is_some());
    // One announcement on the home channel in the hour does for both
    // holders: the one that hears the other's holds its own back.
    let announced = |i| {
        update_frames(&h, i)
            .iter()
            .filter(|f| f.2 == 0 && f.1 == h.nodes[i].core.home())
            .count()
    };
    assert_eq!(announced(0).min(announced(1)), 0);
}

/// Today's image, the table and 25 blocks, the last of 33 pieces, crosses
/// one hop without loss in about what a request buys for every block,
/// 2430 pieces at the pace, 9.4 hours, within a tenth; and to a node on
/// another channel too within twelve hours.
#[test]
fn todays_image_crosses_a_hop_in_about_its_pieces_paces() {
    let image = Image {
        id: [9; 32],
        version: 3,
        blocks: 26,
        last: 33,
        key: 0,
    };
    let mut needs = vec![1];
    needs.extend(std::iter::repeat_n(64, 24));
    needs.push(33);
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
    assert!((2190..2670).contains(&paces), "{paces} paces");
}

//! Firmware images spreading over the mesh (spec "Updates over the mesh"),
//! driven through real frames on the test medium, with a driver side that
//! keeps no bytes.

mod common;

use bark_proto::update::{sweep_pieces, Image, FASTEST, GAP, PACE, SERVED_CHANNELS};
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
    // The 72 pieces at most one a gap apart.
    let took = h.now.since(start);
    assert!(took >= GAP.mul(71), "faster than the gap: {took:?}");
    // Each learned of the image from a heartbeat's section.
    for i in 1..4 {
        assert!(h.nodes[i].sent.iter().any(|(_, f)| f.len > 13));
    }
}

/// A holder sends pieces on at most two channels in a pace
/// (spec "Updates over the mesh").
#[test]
fn update_frames_go_on_at_most_two_channels_in_a_pace() {
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
    // On node 2's channel, home to all three, a holder that hears the
    // other's piece falls back to the pace: together they send at most
    // what one sends alone a gap apart, and a little over.
    let home = h.nodes[2].core.home();
    let done = h.nodes[2].updates.as_ref().unwrap().holds_at.unwrap();
    let frames: Vec<_> = (0..2).map(|i| update_frames(&h, i)).collect();
    let on = |i: usize| {
        frames[i]
            .iter()
            .filter(|f| f.1 == home && f.0 < done)
            .count() as u64
    };
    let fastest = done.since(start).as_micros() / GAP.as_micros();
    assert!(
        on(0) + on(1) <= fastest + fastest / 4 + 2,
        "{} + {} in {fastest} refills",
        on(0),
        on(1)
    );
}

/// Today's image: the table of two pieces and 27 blocks, the last of 40
/// pieces, and the pieces each block is made of.
fn todays_image() -> (Image, Vec<usize>) {
    let image = Image {
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

/// Today's image crosses one hop without loss far faster than a sweep at
/// the pace, the holders idle; and to the other nodes too within twelve
/// hours.
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
    // Idle, the holders send as fast as their buckets refill, a full
    // frame's airtime at the cap, sharing them among the channels they
    // serve: no faster than the pieces at that rate, and far faster than
    // a sweep at the pace.
    let took = first.since(start).as_micros();
    assert!(
        took >= FASTEST.mul(pieces).as_micros() * 9 / 10,
        "{took} µs"
    );
    assert!(took < PACE.mul(sweep).as_micros() / 2, "{took} µs");
}

/// An idle holder alone on its home channel sends its pieces about a gap
/// apart, not a pace (spec "Updates over the mesh").
#[test]
fn an_idle_holder_alone_sends_a_gap_apart() {
    let mut h = mesh(2);
    let start = h.now;
    h.run_until(start + secs(1800), |_| false);
    let home = h.nodes[0].core.home();
    let times: Vec<Instant> = update_frames(&h, 0)
        .into_iter()
        .filter(|f| f.1 == home)
        .map(|f| f.0)
        .collect();
    assert!(times.len() > 20, "{}", times.len());
    let mut gaps: Vec<u64> = times
        .windows(2)
        .map(|w| w[1].since(w[0]).as_micros())
        .collect();
    gaps.sort_unstable();
    let median = gaps[gaps.len() / 2];
    assert!(
        median >= GAP.as_micros() && median < PACE.as_micros() / 2,
        "{median} µs"
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

/// A sleepy node gathers from its anchor: its beats say what it lacks, the
/// anchor sends after its slot heartbeat on its home channel, and the node
/// listens for that frame (spec "Updates over the mesh").
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
    // A run a rotation of the anchor, the pieces the pace gives its home
    // channel in one, 9 in the shortest of 130 s: the image's 72 pieces
    // and the loss margin within 12 of the longest rotations, 195 s
    // (spec "Heartbeat").
    let took = h.nodes[1]
        .updates
        .as_ref()
        .unwrap()
        .holds_at
        .unwrap()
        .since(start);
    assert!(took.as_micros() <= 12 * 195_000_000, "{took:?}");
    // Its beats carried its section, and every piece it took came in a
    // run after one of its anchor's heartbeats on its channel: 14 pieces
    // at most, of 138.6 ms and a check each.
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
        .filter(|(t, _)| beats.iter().any(|b| *t > *b && t.since(*b) < secs(3)))
        .count();
    assert_eq!(after_slot, NEEDS.iter().sum::<usize>());
}

/// With nothing changing, a node's heartbeats carry its section for one
/// rotation an hour (spec "Updates over the mesh").
#[test]
fn a_section_rides_one_rotation_an_hour_when_nothing_changes() {
    let mut h = mesh(2);
    h.deliver_app(1, AppCommand::Holds(IMAGE));
    let start = h.now;
    h.run_until(start + secs(5 * 3600 / 2), |_| false);
    let beats = |with: bool| {
        h.nodes[0]
            .sent
            .iter()
            .filter(|(t, _)| *t >= start)
            .filter(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Heartbeat(_))))
            .filter(|(_, f)| bark_proto::wire::decode_section(f.payload()).is_some() == with)
            .count()
    };
    // A rotation in each of the three hours begun, 13 heartbeats each.
    assert_eq!(beats(true), 39);
    assert!(beats(false) > 600, "{}", beats(false));
}

/// A quota that falls short by lost pieces is made up: the holder goes on
/// with fresh pieces of the block until the gatherer's section shows it
/// complete, and is never silent while it lacks a block the holder has
/// (spec "Updates over the mesh").
#[test]
fn a_quota_short_by_lost_pieces_is_made_up() {
    let mut h = mesh(2);
    // Lose the first 30 pieces of block 1 on their way to node 1.
    let mut lost = 0;
    h.lose = Some(Box::new(move |_, from, to, bytes| {
        let piece = matches!(
            Packet::decode(bytes),
            Ok(Packet::Update(Update::Piece { block: 1, .. }))
        );
        if from == 0 && to == 1 && piece && lost < 30 {
            lost += 1;
            return true;
        }
        false
    }));
    let start = h.now;
    let done = h.run_until(start + secs(3600), |h| {
        h.nodes[1].updates.as_ref().unwrap().holds_at.is_some()
    });
    assert!(done, "the gatherer does not hold the image within an hour");
    let frames = update_frames(&h, 0);
    let longest = frames
        .windows(2)
        .map(|w| w[1].0.since(w[0].0))
        .max()
        .unwrap();
    assert!(longest <= PACE.mul(2), "silent for {longest:?}");
    // Block 1's quota, 80, and its tail at the pace until it decoded.
    let ones: Vec<Instant> = frames.iter().filter(|f| f.2 == 1).map(|f| f.0).collect();
    let quota = usize::from(sweep_pieces(64));
    assert!(ones.len() > quota, "{} of block 1", ones.len());
    assert!(ones[quota..].windows(2).all(|w| w[1].since(w[0]) >= PACE));
}

/// A relay that runs version `version` and holds no image.
fn relay_running(east: f64, seed: u64, home: u8, version: u32) -> Relay {
    Relay::new(
        Config {
            position: pos(east),
            home_override: bark_proto::Channel::new(home),
            version,
            ..Config::default()
        },
        seed,
    )
}

/// The heartbeats node `i` sent that carried a section, and their channels.
fn sectioned(h: &Harness<Relay>, i: usize) -> Vec<(Instant, bark_proto::Channel)> {
    h.nodes[i]
        .sent
        .iter()
        .filter(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Heartbeat(_))))
        .filter(|(_, f)| bark_proto::wire::decode_section(f.payload()).is_some())
        .map(|(t, f)| (*t, f.channel))
        .collect()
}

/// The longest rotation of heartbeats, 13 of the longest interval, and
/// the most a heartbeat waits behind its slot (spec "Heartbeat").
fn longest_rotation() -> bark_proto::Duration {
    let c = Config::default();
    c.heartbeat_max.mul(13) + bark_proto::node::heartbeat_lag(&c)
}

/// Boot is a change: a node that knows the version it runs and holds no
/// image tells it in every heartbeat of its first rotation, on every
/// channel, and then not again within the hour
/// (spec "Updates over the mesh").
#[test]
fn boot_carries_the_section_for_a_rotation() {
    let mut h = Harness::new(vec![
        relay_running(0.0, 31, 3, 13),
        relay_running(100.0, 32, 5, 0),
    ]);
    h.link(0, 1);
    h.run_until(at(3000), |_| false);
    let told = sectioned(&h, 0);
    let channels: std::collections::HashSet<_> = told.iter().map(|t| t.1).collect();
    assert_eq!(told.len(), 13, "{told:?}");
    assert_eq!(channels.len(), 13);
    assert!(told.iter().all(|t| t.0 < at(200)), "{told:?}");
    let s = h.nodes[0]
        .sent
        .iter()
        .find_map(|(_, f)| bark_proto::wire::decode_section(f.payload()))
        .unwrap();
    assert_eq!((s.runs, s.image), (13, None));
    // A node that knows nothing to tell tells nothing.
    assert!(sectioned(&h, 1).is_empty());
}

/// A gatherer says what it lacks every rotation on its helper's home
/// channel, and on the other channels only when it changes or hourly
/// (spec "Updates over the mesh").
#[test]
fn a_gatherer_s_section_rides_on_its_helper_s_channel_each_rotation() {
    let (image, needs) = todays_image();
    let mut h = Harness::new(vec![relay_on(0.0, 41, 3), relay_on(100.0, 42, 5)]);
    h.link(0, 1);
    for n in &mut h.nodes {
        n.updates = Some(Updates {
            needs: needs.clone(),
            ..Updates::default()
        });
    }
    h.run_until(at(400), |_| false);
    h.deliver_app(0, AppCommand::Holds(image));
    let start = h.now;
    let done = h.run_until(start + secs(12 * 3600), |h| {
        h.nodes[1].updates.as_ref().unwrap().holds_at.is_some()
    });
    assert!(
        done,
        "the gatherer does not hold the image within twelve hours"
    );
    let u = h.nodes[1].updates.as_ref().unwrap();
    let (first, held) = (u.pieces.first().unwrap().0, u.holds_at.unwrap());
    let told = sectioned(&h, 1);
    let on = |c: u8| {
        told.iter()
            .filter(|t| t.1 == common::ch(c) && t.0 > first && t.0 < held)
            .map(|t| t.0)
            .collect::<Vec<_>>()
    };
    let helper = on(3);
    let gaps = helper.windows(2).map(|w| w[1].since(w[0]));
    assert!(
        gaps.clone().all(|g| g <= longest_rotation()),
        "{:?}",
        gaps.max()
    );
    // Elsewhere, a rotation for each block completed and hourly.
    assert!(
        on(7).len() * 2 < helper.len(),
        "{} against {}",
        on(7).len(),
        helper.len()
    );
}

/// A holder that hears sections naming nothing as new as its image
/// answers with a rotation of its own, once a rotation however many lag
/// (spec "Updates over the mesh").
#[test]
fn a_holder_answers_a_lagging_neighbour_once_a_rotation() {
    let mut h = Harness::new(vec![
        relay_on(0.0, 51, 3),
        relay_running(100.0, 52, 5, 1),
        relay_running(200.0, 53, 6, 1),
        relay_running(300.0, 54, 7, 1),
        relay_running(400.0, 55, 8, 1),
    ]);
    for a in 0..5 {
        for b in a + 1..5 {
            h.link(a, b);
        }
    }
    // Three switched on together, one later, none taking images.
    for (i, s) in [(1, 1000), (2, 1005), (3, 1010), (4, 2000)] {
        h.start_at(i, at(s));
    }
    h.run_until(at(400), |_| false);
    h.deliver_app(0, AppCommand::Holds(IMAGE));
    h.run_until(at(3000), |_| false);
    let told = sectioned(&h, 0);
    let between = |a: u64, b: u64| told.iter().filter(|t| t.0 >= at(a) && t.0 < at(b)).count();
    assert_eq!(between(400, 1000), 13, "the rotation after Holds");
    assert_eq!(between(1000, 2000), 13, "one answer to the three");
    assert_eq!(between(2000, 3000), 13, "one answer to the fourth");
}

/// A gatherer at home on channel 5 that hears only a holder at home on 3,
/// which hears another holder at home on 5 out of the gatherer's reach:
/// the holder on 3 streams to it on 5, and does so as well
/// where the gatherer hears the holder on 5 too, since no relay leaves a
/// gatherer to another (spec "Updates over the mesh").
#[test]
fn a_gatherer_that_hears_only_a_holder_elsewhere_is_served() {
    for hears_home in [false, true] {
        let mut h = Harness::new(vec![
            relay_on(0.0, 61, 5),
            relay_on(300.0, 62, 3),
            relay_on(600.0, 63, 5),
        ]);
        h.link(0, 1);
        h.link(1, 2);
        if hears_home {
            h.link(0, 2);
        }
        for n in &mut h.nodes {
            n.updates = Some(Updates {
                needs: NEEDS.to_vec(),
                ..Updates::default()
            });
        }
        h.run_until(at(400), |_| false);
        h.deliver_app(1, AppCommand::Holds(IMAGE));
        h.deliver_app(2, AppCommand::Holds(IMAGE));
        let start = h.now;
        let done = h.run_until(start + secs(2 * 3600), |h| {
            h.nodes[0].updates.as_ref().unwrap().holds_at.is_some()
        });
        assert!(done, "hears the holder at home {hears_home}: not served");
        let on_5 = update_frames(&h, 1)
            .iter()
            .filter(|f| f.1 == common::ch(5))
            .count();
        assert!(on_5 > 0, "hears the holder at home {hears_home}: none on 5");
    }
}

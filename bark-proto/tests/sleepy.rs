//! Sleepy nodes (spec "Sleepy nodes"): their home and beat, delivery to
//! them on the beat, their place in routing where nothing else is, and sleepy
//! nodes on their own.

mod common;

use bark_proto::tables::heard::{beat_period, HeardParams};
use bark_proto::wire::{Data, Packet, TTL_UNIT};
use bark_proto::{
    AppCommand, AppEvent, Config, DropReason, Duration, Instant, MsgId, NodeId, Position, Relay,
    Trace,
};
use common::{secs, Harness};

/// A position `east`, `north` metres from a point in Kyiv.
fn pos(east: f64, north: f64) -> Position {
    let (lat, lon) = (50.45, 30.52);
    let m_per_deg = 6_371_000.0 * core::f64::consts::PI / 180.0;
    Position::from_degrees(
        lat + north / m_per_deg,
        lon + east / (m_per_deg * lat.to_radians().cos()),
    )
}

/// A node at `east`, sleepy or not. The boot listen is a full rotation in
/// the design (spec "Channel at boot"); a short one keeps the tests quick,
/// except for a sleepy node, which chooses its anchor at its end.
fn node(east: f64, sleepy: bool, seed: u64) -> Relay {
    node_with(east, sleepy, seed, Config::default())
}

fn node_with(east: f64, sleepy: bool, seed: u64, config: Config) -> Relay {
    let boot_listen = if sleepy { config.boot_listen } else { secs(5) };
    Relay::new(
        Config {
            position: pos(east, 0.0),
            boot_listen,
            sleepy,
            ..config
        },
        seed,
    )
}

fn at(s: u64) -> Instant {
    Instant::ZERO + secs(s)
}

/// The heartbeats node `i` sent: when, and the frame's channel.
fn beats(h: &Harness<Relay>, i: usize) -> Vec<(Instant, bark_proto::Channel, bool)> {
    h.nodes[i]
        .sent
        .iter()
        .filter_map(|(t, f)| match Packet::decode(f.payload()) {
            Ok(Packet::Heartbeat(hb)) => Some((*t, f.channel, hb.sleepy)),
            _ => None,
        })
        .collect()
}

fn delivered_at(h: &Harness<Relay>, i: usize, id: MsgId) -> Option<Instant> {
    h.nodes[i].app.iter().find_map(|(t, e)| match e {
        AppEvent::Message { msg_id, .. } if *msg_id == id => Some(*t),
        _ => None,
    })
}

/// Node `from`'s application sends `text` to node `to` at the address `to`
/// has now (spec "Addresses").
fn send(h: &mut Harness<Relay>, from: usize, to: usize, text: &[u8]) -> MsgId {
    let (dst_pos, dst_id) = (h.nodes[to].core.address(), h.nodes[to].core.id());
    h.command(
        from,
        AppCommand::Send {
            dst_pos,
            dst_id,
            payload: text,
        },
    );
    MsgId::of(dst_pos, dst_id, text)
}

/// A sleepy node takes the strongest node that is not sleepy as its
/// anchor, listens on its home and beats there only, with the sleepy
/// type: twice close together after it chooses, then at a random time in
/// the second half of each beat period. Its address is its anchor's
/// position.
#[test]
fn a_sleepy_node_beats_on_its_anchor_s_channel_slowly() {
    let (strong, weak, s) = (0, 1, 2);
    let mut h = Harness::new(vec![
        node(0.0, false, 1),
        node(300.0, false, 2),
        node(50.0, true, 3),
    ]);
    h.link(strong, s);
    h.link(weak, s);
    h.set_rssi(strong, s, -85);
    h.set_rssi(weak, s, -100);
    // The sleepy node boots once the others have chosen their homes.
    h.start_at(s, at(10));
    h.run_until(at(2000), |_| false);
    let sleepy = &h.nodes[s].core;
    let anchor = sleepy.anchor().expect("an anchor");
    assert!(sleepy.sleeps());
    assert_eq!(anchor.id, h.nodes[strong].core.id());
    assert_eq!(sleepy.address(), h.nodes[strong].core.config().position);
    let home = h.nodes[strong].core.home();
    assert_eq!(h.resting(s), Some(home));
    // From its choice on: before it, it beats where it boots.
    let chose = h.nodes[s]
        .traces
        .iter()
        .filter(|(_, t)| matches!(t, Trace::HomeChannelMoved { .. }))
        .map(|(t, _)| *t)
        .next_back()
        .unwrap();
    let b: Vec<_> = beats(&h, s).into_iter().filter(|b| b.0 >= chose).collect();
    assert!(b.len() >= 6, "{} beats", b.len());
    assert!(b.iter().all(|&(_, c, sleepy)| c == home && sleepy));
    let c = Config::default();
    let gap = |k: usize| b[k + 1].0.since(b[k].0);
    assert!(gap(0) >= c.heartbeat_min && gap(0) <= c.heartbeat_max + secs(1));
    let period = beat_period(c.delivery_ttl);
    for k in 1..b.len() - 1 {
        assert!(
            gap(k) >= period.div(2) && gap(k) <= period + secs(1),
            "beat {k}: {:?}",
            gap(k)
        );
    }
}

/// A message for a sleepy node waits at its holder, the owner of its
/// address, until the node beats, and goes to it in the window after the
/// beat (`tables::heard`). With `sleepy_listens` the node listens all the
/// time and the holder sends it at once. The wait is the delivery queue's and does not count against the
/// message's time left: the DATA carries the time it arrived with.
#[test]
fn a_message_for_a_sleepy_node_goes_on_its_beat() {
    for listens in [false, true] {
        let (r, s) = (0, 1);
        let config = Config {
            sleepy_listens: listens,
            ..Config::default()
        };
        let mut h = Harness::new(vec![
            node_with(0.0, false, 11, config),
            node_with(50.0, true, 12, config),
        ]);
        h.link(r, s);
        h.start_at(s, at(10));
        h.run_until(at(1000), |_| false);
        assert!(h.nodes[s].core.anchor().is_some());
        // Past the window of the sleepy node's last beat.
        let last = beats(&h, s).last().unwrap().0;
        let window = HeardParams::from_config(&Config::default()).awake;
        h.run_until(last + window + secs(1), |_| false);
        let sent_at = h.now;
        let id = send(&mut h, r, s, b"wake up");
        assert!(h.run_until(sent_at + secs(400), |h| delivered_at(h, s, id).is_some()));
        let got = delivered_at(&h, s, id).unwrap();
        if listens {
            assert!(got.since(sent_at) < secs(1), "{:?}", got.since(sent_at));
        } else {
            let beat = beats(&h, s)
                .into_iter()
                .map(|(t, _, _)| t)
                .rfind(|&t| t <= got)
                .unwrap();
            assert!(beat > sent_at, "delivered before the node woke");
            assert!(
                got.since(beat) < window,
                "{:?} after the beat",
                got.since(beat)
            );
        }
        let ttl = Data::units(Config::default().delivery_ttl);
        let to_node: Vec<_> = h.nodes[r]
            .sent
            .iter()
            .filter_map(|(_, f)| match Packet::decode(f.payload()) {
                Ok(Packet::Data(d)) if d.dst_hop == h.nodes[s].core.id() => Some(d.time_left),
                _ => None,
            })
            .collect();
        assert!(
            !to_node.is_empty() && to_node.iter().all(|&left| left == ttl),
            "{to_node:?}"
        );
        h.run_for(secs(1));
        assert!(h.nodes[r].traces.iter().any(|(_, t)| *t
            == Trace::Delivered {
                msg_id: id,
                node: h.nodes[s].core.id()
            }));
    }
}

/// A sleepy neighbour is a next hop only where no node that is not sleepy
/// is live: O hands a message for D to A although S is nearer D, and to S
/// once A has gone from its table (spec "Next hop", extended). All share one channel: a
/// sleepy node beats only on its home, and only nodes there hear it. S
/// has an anchor and would sleep, taking the message only in its window
/// after a beat; it listens here (`sleepy_listens`), so that the test is
/// of the choice of next hop.
///
/// ```text
///   O ---- A ---- D
///    \          /
///     ---- S ---
/// ```
#[test]
fn a_sleepy_neighbour_is_the_last_resort() {
    let (o, a, s, d) = (0, 1, 2, 3);
    let one = Config {
        home_override: bark_proto::Channel::new(4),
        ..Config::default()
    };
    let mut h = Harness::new(vec![
        node_with(0.0, false, 21, one),
        node_with(400.0, false, 22, one),
        node_with(
            500.0,
            true,
            23,
            Config {
                sleepy_listens: true,
                ..Config::default()
            },
        ),
        node_with(800.0, false, 24, one),
    ]);
    for (x, y) in [(o, a), (a, d), (o, s), (s, d)] {
        h.link(x, y);
    }
    h.run_until(at(700), |_| false);
    let first_hop = |h: &Harness<Relay>, id: MsgId| {
        h.nodes[o].traces.iter().find_map(|(_, t)| match t {
            Trace::Forwarded { msg_id, to, .. } if *msg_id == id => Some(*to),
            _ => None,
        })
    };
    let id = send(&mut h, o, d, b"via A");
    assert!(h.run_until(h.now + secs(10), |h| delivered_at(h, d, id).is_some()));
    assert_eq!(first_hop(&h, id), Some(h.nodes[a].core.id()));
    h.kill(a);
    // Once A has expired at O (spec "Neighbour table": 390 s).
    h.run_for(secs(400));
    let id = send(&mut h, o, d, b"via S");
    assert!(h.run_until(h.now + secs(10), |h| delivered_at(h, d, id).is_some()));
    assert_eq!(first_hop(&h, id), Some(h.nodes[s].core.id()));
}

/// Sleepy nodes that hear no other kind route among themselves: S1
/// reaches S3 through S2. Without an anchor none sleeps: each heartbeats
/// as a relay does, every 5 to 15 s on the rotating channel, with the
/// sleepy type (spec "Sleepy nodes alone").
#[test]
fn sleepy_nodes_alone_meet_and_route() {
    let mut h = Harness::new(vec![
        node(0.0, true, 31),
        node(400.0, true, 32),
        node(800.0, true, 33),
    ]);
    h.link(0, 1);
    h.link(1, 2);
    h.run_until(at(1000), |_| false);
    assert_eq!(
        h.nodes[2].core.address(),
        pos(800.0, 0.0),
        "its own position"
    );
    assert!(!h.nodes[1].core.sleeps());
    let b: Vec<_> = beats(&h, 1)
        .into_iter()
        .filter(|b| b.0 >= at(400))
        .collect();
    let c = Config::default();
    assert!(b.iter().all(|&(_, _, sleepy)| sleepy));
    assert!(b
        .windows(2)
        .all(|w| w[1].0.since(w[0].0) <= c.heartbeat_max + secs(1)));
    let channels: std::collections::HashSet<_> = b.iter().map(|&(_, ch, _)| ch).collect();
    assert_eq!(channels.len(), bark_proto::radio::CHANNELS as usize);
    let id = send(&mut h, 0, 2, b"among ourselves");
    assert!(h.run_until(h.now + secs(10), |h| delivered_at(h, 2, id).is_some()));
}

/// A sleepy node's own message with no live neighbour waits for one
/// until its time runs out (spec "Receiving DATA").
#[test]
fn a_sleepy_node_alone_keeps_its_message_for_a_ttl() {
    let mut h = Harness::new(vec![node(0.0, true, 41)]);
    h.run_until(at(200), |_| false);
    let text = b"nobody there";
    let (dst_pos, dst_id) = (pos(500.0, 0.0), NodeId([9; 4]));
    h.command(
        0,
        AppCommand::Send {
            dst_pos,
            dst_id,
            payload: text,
        },
    );
    let id = MsgId::of(dst_pos, dst_id, text);
    let start = h.now;
    let dropped = |h: &Harness<Relay>| {
        h.nodes[0].traces.iter().find_map(|(t, tr)| match tr {
            Trace::Dropped { msg_id, reason } if *msg_id == id => Some((*t, *reason)),
            _ => None,
        })
    };
    let last = start + Config::default().delivery_ttl - TTL_UNIT;
    h.run_until(last - secs(1), |_| false);
    assert_eq!(dropped(&h), None);
    assert!(h.nodes[0].core.holding().any(|m| m == id));
    h.run_until(start + secs(700), |_| false);
    let (t, reason) = dropped(&h).expect("dropped");
    assert_eq!(reason, DropReason::Expired);
    assert!(t >= last);
}

/// A sleepy node keeps its anchor against a node a little stronger and
/// moves to one 8 dB stronger once it has heard it twice
/// (spec "Relay choice"), beating at once on the new channel. It listens
/// here (`sleepy_listens`): asleep it would hear the other's heartbeats
/// only in its windows.
#[test]
fn a_sleepy_node_moves_only_to_a_much_stronger_node() {
    let (first, second, s) = (0, 1, 2);
    let quiet = |home: u8| Config {
        home_override: bark_proto::Channel::new(home),
        ..Config::default()
    };
    let mut h = Harness::new(vec![
        node_with(0.0, false, 51, quiet(3)),
        node_with(300.0, false, 52, quiet(7)),
        node_with(
            50.0,
            true,
            53,
            Config {
                sleepy_listens: true,
                ..Config::default()
            },
        ),
    ]);
    h.link(first, s);
    h.set_rssi(first, s, -95);
    h.start_at(s, at(10));
    // A boot listen, a rotation to first hear the relay, and another.
    h.run_until(at(700), |_| false);
    let id_of = |h: &Harness<Relay>, i: usize| h.nodes[i].core.id();
    assert_eq!(
        h.nodes[s].core.anchor().map(|a| a.id),
        Some(id_of(&h, first))
    );
    h.link(second, s);
    h.set_rssi(second, s, -90);
    h.run_for(secs(600));
    assert_eq!(
        h.nodes[s].core.anchor().map(|a| a.id),
        Some(id_of(&h, first)),
        "5 dB is not enough"
    );
    h.set_rssi(second, s, -80);
    let moved =
        |h: &Harness<Relay>| h.nodes[s].core.anchor().map(|a| a.id) == Some(id_of(h, second));
    assert!(h.run_until(h.now + secs(600), moved));
    let moved_at = h.now;
    assert_eq!(h.resting(s), bark_proto::Channel::new(7));
    h.run_for(Duration::from_millis(500));
    let b = beats(&h, s);
    assert!(
        b.iter()
            .any(|&(t, c, _)| t >= moved_at && Some(c) == bark_proto::Channel::new(7)),
        "a beat at once on the new channel"
    );
}

/// Time node `i`'s radio slept between `from` and `to`.
fn slept(h: &Harness<Relay>, i: usize, from: Instant, to: Instant) -> Duration {
    h.nodes[i]
        .asleep
        .iter()
        .map(|&(a, b)| (a.max(from), b.min(to)))
        .filter(|(a, b)| b > a)
        .fold(Duration::ZERO, |sum, (a, b)| sum + b.since(a))
}

/// A sleepy node with an anchor lets its radio sleep (`Action::Sleep`,
/// core-interface §3 rule 10) except while it beats and through the
/// window after each beat (spec "Delivery"), and a message for it still
/// reaches it there.
#[test]
fn a_sleepy_node_sleeps_between_its_windows() {
    let (r, s) = (0, 1);
    let mut h = Harness::new(vec![node(0.0, false, 61), node(50.0, true, 62)]);
    h.link(r, s);
    h.start_at(s, at(10));
    h.run_until(at(1000), |_| false);
    assert!(h.nodes[s].core.sleeps());
    let id = send(&mut h, r, s, b"in the window");
    h.run_until(at(3000), |_| false);
    assert!(delivered_at(&h, s, id).is_some());
    let window = HeardParams::from_config(&Config::default()).awake;
    let b: Vec<_> = beats(&h, s)
        .into_iter()
        .map(|(t, _, _)| t)
        .filter(|&t| t >= at(1000))
        .collect();
    assert!(b.len() >= 5, "{} beats", b.len());
    // Awake from each beat through its window, or until it has answered a
    // message that said nothing more follows.
    let answered: Vec<Instant> = h.nodes[s]
        .sent
        .iter()
        .filter(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Ack(_))))
        .map(|(t, _)| *t)
        .collect();
    for &t in &b {
        let end = answered
            .iter()
            .copied()
            .find(|&a| a > t && a < t + window)
            .unwrap_or(t + window);
        assert_eq!(slept(&h, s, t, end), Duration::ZERO, "beat at {t:?}");
    }
    let (from, to) = (at(1000), at(3000));
    let awake = to.since(from) - slept(&h, s, from, to);
    // The windows, the delivery's, and once a rotation of its anchor's
    // heartbeat a watch for it, from 420 ms before to 431 ms after the
    // time its interval gives (spec "Relay choice").
    let c = Config::default();
    let anchor = h.nodes[s].core.anchor().unwrap().id;
    let rotation = bark_proto::node::heartbeat_interval(anchor, &c).mul(13);
    let watches = to.since(from).as_micros() / rotation.as_micros() + 1;
    let watch = Duration::from_millis(860);
    let most = window.mul(b.len() as u64 + 2) + watch.mul(watches);
    assert!(awake <= most, "awake {awake:?} of {:?}", to.since(from));
}

/// A sleeping node wakes to hand its own message to its anchor and stays
/// awake for the ACK, so one transmission is enough.
#[test]
fn a_sleeping_node_hears_the_ack_to_its_own_message() {
    let (r, s) = (0, 1);
    let mut h = Harness::new(vec![node(0.0, false, 71), node(50.0, true, 72)]);
    h.link(r, s);
    h.start_at(s, at(10));
    h.run_until(at(1000), |_| false);
    // Asleep when the application sends.
    let asleep = |h: &Harness<Relay>| {
        h.nodes[s]
            .asleep
            .last()
            .is_some_and(|&(a, b)| a <= h.now && b > h.now + secs(1))
    };
    assert!(h.run_until(h.now + secs(400), asleep));
    let id = send(&mut h, s, r, b"to my anchor");
    assert!(h.run_until(h.now + secs(5), |h| delivered_at(h, r, id).is_some()));
    h.run_for(secs(1));
    let data = h.nodes[s]
        .sent
        .iter()
        .filter(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Data(_))))
        .count();
    assert_eq!(data, 1, "answered at the first transmission");
    assert!(h.nodes[s].core.holding().next().is_none());
}

/// Messages waiting for a sleepy node go one after the other in the
/// window after its beat: each DATA but the last says more follow, the
/// node listens one hand-over longer after each such, and sleeps once it
/// has answered the last (spec "Delivery").
#[test]
fn a_burst_goes_on_one_beat_and_the_node_sleeps_after_the_last() {
    let (r, s) = (0, 1);
    let mut h = Harness::new(vec![node(0.0, false, 81), node(50.0, true, 82)]);
    h.link(r, s);
    h.start_at(s, at(10));
    h.run_until(at(1000), |_| false);
    let asleep = |h: &Harness<Relay>| {
        h.nodes[s]
            .asleep
            .last()
            .is_some_and(|&(a, b)| a <= h.now && b > h.now + secs(1))
    };
    assert!(h.run_until(h.now + secs(400), asleep));
    let ids: Vec<MsgId> = [b"one", b"two", b"six"]
        .iter()
        .map(|t| send(&mut h, r, s, *t))
        .collect();
    let all = |h: &Harness<Relay>| ids.iter().all(|&id| delivered_at(h, s, id).is_some());
    assert!(h.run_until(h.now + secs(310), all));
    let got: Vec<Instant> = ids
        .iter()
        .map(|&id| delivered_at(&h, s, id).unwrap())
        .collect();
    let beat = beats(&h, s)
        .into_iter()
        .map(|(t, _, _)| t)
        .rfind(|&t| t <= got[0])
        .unwrap();
    let p = HeardParams::from_config(&Config::default());
    assert!(got[2].since(beat) < p.awake + p.more.mul(2), "one beat");
    let to_node: Vec<bool> = h.nodes[r]
        .sent
        .iter()
        .filter(|(t, _)| *t >= beat)
        .filter_map(|(_, f)| match Packet::decode(f.payload()) {
            Ok(Packet::Data(d)) if d.dst_hop == h.nodes[s].core.id() => Some(d.more),
            _ => None,
        })
        .collect();
    assert_eq!(to_node, [true, true, false]);
    h.run_for(secs(1));
    let last_ack = h.nodes[s]
        .sent
        .iter()
        .filter(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Ack(_))))
        .map(|(t, _)| *t)
        .next_back()
        .unwrap();
    let sleeps_at = h.nodes[s].asleep.last().unwrap().0;
    assert!(
        sleeps_at.since(last_ack) < Duration::from_millis(50),
        "asleep {:?} after its last answer",
        sleeps_at.since(last_ack)
    );
}

/// A node that sleeps wakes for its anchor's heartbeat each time the
/// anchor's rotation brings it to its channel, and hears it
/// (spec "Relay choice").
#[test]
fn a_sleeping_node_hears_its_anchor_every_rotation() {
    let (r, s) = (0, 1);
    let mut h = Harness::new(vec![node(0.0, false, 111), node(50.0, true, 112)]);
    h.link(r, s);
    h.start_at(s, at(10));
    h.run_until(at(3000), |_| false);
    let home = h.resting(s).unwrap();
    let to_s: Vec<Instant> = h.nodes[r]
        .sent
        .iter()
        .filter(|(t, f)| *t >= at(1000) && f.channel == home)
        .filter(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Heartbeat(_))))
        .map(|(t, _)| *t)
        .collect();
    let heard = to_s
        .iter()
        .filter(|&&t| {
            h.nodes[s]
                .received
                .iter()
                .any(|(at, _)| *at > t && at.since(t) < secs(1))
        })
        .count();
    assert!(to_s.len() >= 10, "{} heartbeats on its channel", to_s.len());
    assert_eq!(heard, to_s.len(), "heard every one");
}

/// A node whose anchor is gone notices when two of the anchor's rotations
/// pass without its heartbeat, listens a rotation, and takes the other node
/// it hears (spec "Relay choice").
#[test]
fn a_node_whose_anchor_is_gone_chooses_again() {
    let (first, second, s) = (0, 1, 2);
    let mut h = Harness::new(vec![
        node(0.0, false, 101),
        node(300.0, false, 102),
        node(50.0, true, 103),
    ]);
    h.link(first, s);
    h.link(second, s);
    h.set_rssi(first, s, -80);
    h.set_rssi(second, s, -100);
    h.start_at(s, at(10));
    h.run_until(at(1000), |_| false);
    let id_of = |h: &Harness<Relay>, i: usize| h.nodes[i].core.id();
    assert_eq!(
        h.nodes[s].core.anchor().map(|a| a.id),
        Some(id_of(&h, first))
    );
    h.kill(first);
    let gone = h.now;
    let moved =
        |h: &Harness<Relay>| h.nodes[s].core.anchor().map(|a| a.id) == Some(id_of(h, second));
    assert!(h.run_until(gone + secs(1000), moved));
    // Two rotations at the longest interval, and the rotation it listens.
    let c = Config::default();
    let most = c.heartbeat_max.mul(13 * 3) + secs(1);
    assert!(h.now.since(gone) <= most, "{:?}", h.now.since(gone));
    assert!(h.nodes[s]
        .traces
        .iter()
        .any(|(_, t)| *t == Trace::AnchorCheck));
}

/// A node that sleeps follows its anchor to a new home channel: it hears
/// the anchor's next heartbeat on its old channel, which names the new
/// home, and knows from the anchor's interval when it comes to the new.
#[test]
fn a_sleeping_node_follows_its_anchor_to_a_new_channel() {
    let (r, s) = (0, 1);
    let mut h = Harness::new(vec![node(0.0, false, 121), node(50.0, true, 122)]);
    h.link(r, s);
    h.start_at(s, at(10));
    h.run_until(at(1000), |_| false);
    let old = h.resting(r).unwrap();
    assert_eq!(h.resting(s), Some(old));
    // A carrier on the anchor's home a third of the time: it moves after
    // the dwell and wait of spec "Channel moves", and its heartbeats still
    // get through on the old channel. One there all the time would keep them
    // off it, and the node would lose its anchor (`a_node_whose_anchor_is_gone`).
    // Its period shares no factor with the sweep's (as in the move test of
    // `tests/mac.rs`).
    h.jam_part(
        r,
        old,
        Duration::from_millis(32),
        Duration::from_millis(107),
    );
    let moved = |h: &Harness<Relay>| h.resting(r) != Some(old);
    assert!(h.run_until(h.now + secs(1500), moved));
    let at_move = h.now;
    let new = h.resting(r).unwrap();
    let follows = |h: &Harness<Relay>| h.resting(s) == Some(new);
    assert!(h.run_until(at_move + secs(600), follows));
    let c = Config::default();
    assert!(
        h.now.since(at_move) <= c.heartbeat_max.mul(13) + secs(1),
        "{:?}",
        h.now.since(at_move)
    );
    assert_eq!(
        h.nodes[s].core.anchor().map(|a| a.id),
        Some(h.nodes[r].core.id())
    );
}

/// A node out of reach of everything, in a basement, misses its anchor's
/// heartbeat, checks, hears nothing at all, and keeps its anchor without
/// checking again while it hears nothing; back in reach it hears it again
/// and sleeps as before (spec "Relay choice").
#[test]
fn a_node_out_of_reach_keeps_its_anchor() {
    let (r, s) = (0, 1);
    let mut h = Harness::new(vec![node(0.0, false, 131), node(50.0, true, 132)]);
    h.link(r, s);
    h.start_at(s, at(10));
    h.run_until(at(1000), |_| false);
    let anchor = h.nodes[s].core.anchor();
    assert!(anchor.is_some());
    h.unlink(r, s);
    h.run_for(secs(1800));
    // One check, which hears nothing; none after it while it hears nothing.
    let checks = h.nodes[s]
        .traces
        .iter()
        .filter(|(_, t)| *t == Trace::AnchorCheck)
        .count();
    assert_eq!(checks, 1);
    assert_eq!(h.nodes[s].core.anchor(), anchor);
    h.link(r, s);
    let back = h.now;
    h.run_for(secs(1200));
    assert_eq!(h.nodes[s].core.anchor(), anchor);
    let awake = secs(600) - slept(&h, s, back + secs(600), back + secs(1200));
    assert!(awake < secs(30), "awake {awake:?} in ten minutes once back");
}

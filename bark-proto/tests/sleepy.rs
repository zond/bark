//! Sleepy nodes (spec "Sleepy nodes"): their home and beat, delivery to
//! them on the beat, their place in routing where nothing else is, and sleepy
//! nodes on their own.

mod common;

use bark_proto::sniff::{anchorless_beat_period, LONG_PREAMBLE_SYMBOLS};
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

/// A sleepy node takes the first relay it hears as its anchor, at once,
/// and a rotation later the strongest it heard, a little stronger being
/// enough; after that a little stronger is not (spec "Relay choice").
#[test]
fn a_sleepy_node_anchors_at_the_first_relay_and_then_takes_the_strongest() {
    for late in [false, true] {
        let (weak, strong, s) = (0, 1, 2);
        let mut h = Harness::new(vec![
            node(0.0, false, 4),
            node(300.0, false, 5),
            node(50.0, true, 6),
        ]);
        h.link(weak, s);
        h.set_rssi(weak, s, -100);
        h.set_rssi(strong, s, -97);
        h.start_at(s, at(10));
        let anchored = |h: &Harness<Relay>| h.nodes[s].core.anchor().is_some();
        // The weak relay's heartbeat visits the node's channel within a
        // rotation of the node's start.
        let rotation = Config::default().heartbeat_max.mul(13);
        assert!(h.run_until(at(10) + rotation + secs(1), anchored));
        let first = h.now;
        let id_of = |h: &Harness<Relay>, i: usize| h.nodes[i].core.id();
        assert_eq!(
            h.nodes[s].core.anchor().map(|a| a.id),
            Some(id_of(&h, weak))
        );
        if late {
            h.run_until(first + rotation + secs(10), |_| false);
        }
        h.link(strong, s);
        h.run_until(first + rotation.mul(3), |_| false);
        let expected = if late { weak } else { strong };
        assert_eq!(
            h.nodes[s].core.anchor().map(|a| a.id),
            Some(id_of(&h, expected)),
            "strong linked late: {late}"
        );
    }
}

/// A message for a sleepy node waits at its holder, the owner of its
/// address, and goes to it at once with the long preamble while the
/// holder's bucket has room beyond what it has to send; it would otherwise
/// wait for the node's beat (`relay/delivery.rs`). With `sleepy_listens`
/// the node listens all the time and the holder sends it at once with the
/// short preamble. The wait is the delivery queue's and does not count
/// against the message's time left: the DATA carries the time it arrived
/// with.
#[test]
fn a_message_for_a_sleepy_node_goes_at_once_with_the_long_preamble() {
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
        assert!(got.since(sent_at) < secs(1), "{:?}", got.since(sent_at));
        let sid = h.nodes[s].core.id();
        let preambles: Vec<_> = h.nodes[r]
            .sent
            .iter()
            .filter(|(_, f)| {
                matches!(Packet::decode(f.payload()), Ok(Packet::Data(d)) if d.dst_hop == sid)
            })
            .map(|(_, f)| f.preamble_symbols)
            .collect();
        let want = if listens {
            bark_proto::radio::MIN_PREAMBLE_SYMBOLS
        } else {
            LONG_PREAMBLE_SYMBOLS
        };
        assert_eq!(preambles, [want]);
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

/// A sleepy node with an anchor is a next hop only as a last resort: O
/// hands a message for D to A, though S is nearer D, and once A has gone
/// from its table it hands the next to S, with the long preamble, and S
/// forwards it at once to D with the short one. S's beats put it in O's
/// neighbour table, marked sleepy, and in O's heard table (spec
/// "Neighbour table", "Next hop"). All share one channel, so O hears S's
/// beats.
///
/// ```text
///   O ---- A ---- D
///    \          /
///     ---- S ---
/// ```
#[test]
fn a_sleepy_node_is_a_last_resort() {
    let (o, a, s, d) = (0, 1, 2, 3);
    let one = Config {
        home_override: bark_proto::Channel::new(4),
        ..Config::default()
    };
    let mut h = Harness::new(vec![
        node_with(0.0, false, 21, one),
        node_with(400.0, false, 22, one),
        node(500.0, true, 23),
        node_with(800.0, false, 24, one),
    ]);
    for (x, y) in [(o, a), (a, d), (o, s), (s, d)] {
        h.link(x, y);
    }
    h.run_until(at(700), |_| false);
    assert!(h.nodes[s].core.anchor().is_some());
    let sid = h.nodes[s].core.id();
    let entry = h.nodes[o].core.neighbours().find(|n| n.id == sid).copied();
    assert!(
        entry.is_some_and(|n| n.sleepy && n.last_resort()),
        "{entry:?}"
    );
    assert!(h.nodes[o].core.heard_nodes().get(sid).is_some());
    let forwarded = |h: &Harness<Relay>, id: MsgId| {
        h.nodes[o].traces.iter().find_map(|(_, t)| match t {
            Trace::Forwarded { msg_id, to, .. } if *msg_id == id => Some(*to),
            _ => None,
        })
    };
    let id = send(&mut h, o, d, b"via A");
    assert!(h.run_until(h.now + secs(10), |h| delivered_at(h, d, id).is_some()));
    assert_eq!(forwarded(&h, id), Some(h.nodes[a].core.id()));
    h.kill(a);
    // Once A has expired at O (spec "Neighbour table": 390 s), and while S
    // is still in O's table from its last beat.
    h.run_for(secs(400));
    let id = send(&mut h, o, d, b"via S");
    assert!(h.run_until(h.now + secs(10), |h| delivered_at(h, d, id).is_some()));
    assert_eq!(forwarded(&h, id), Some(sid));
    let data = |i: usize, to: NodeId| {
        h.nodes[i]
            .sent
            .iter()
            .filter(move |(_, f)| {
                matches!(Packet::decode(f.payload()), Ok(Packet::Data(x)) if x.dst_hop == to)
            })
            .map(|(_, f)| f.preamble_symbols)
            .collect::<Vec<_>>()
    };
    assert_eq!(data(o, sid), [LONG_PREAMBLE_SYMBOLS]);
    let did = h.nodes[d].core.id();
    assert_eq!(data(s, did), [bark_proto::radio::MIN_PREAMBLE_SYMBOLS]);
}

/// Sleepy nodes that hear no other kind route among themselves: S1
/// reaches S3 through S2. Without an anchor each beats with the
/// anchorless bit and the long preamble, at most a beat period and a half
/// apart and on the next channel each time, and receives duty-cycled in
/// between; every DATA to one has the long preamble
/// (spec "Sleepy nodes alone").
#[test]
fn sleepy_nodes_alone_meet_and_route() {
    let mut h = Harness::new(vec![
        node(0.0, true, 31),
        node(400.0, true, 32),
        node(800.0, true, 33),
    ]);
    h.link(0, 1);
    h.link(1, 2);
    // Two rotations of their beats, by when each has heard the other's.
    let rotation = anchorless_beat_period(Config::default().delivery_ttl)
        .mul(bark_proto::radio::CHANNELS as u64);
    h.run_until(at(195) + rotation.mul(2), |_| false);
    assert_eq!(
        h.nodes[2].core.address(),
        pos(800.0, 0.0),
        "its own position"
    );
    assert!(h.nodes[1].core.anchor().is_none());
    assert!(h.nodes[1].core.anchorless());
    let b: Vec<_> = beats(&h, 1)
        .into_iter()
        .filter(|b| b.0 >= at(400))
        .collect();
    assert!(b.iter().all(|&(_, _, sleepy)| sleepy));
    let long = |f: &bark_proto::Frame| f.preamble_symbols == LONG_PREAMBLE_SYMBOLS;
    let anchorless = h.nodes[1]
        .sent
        .iter()
        .all(|(_, f)| match Packet::decode(f.payload()) {
            Ok(Packet::Heartbeat(hb)) => hb.anchorless && long(f),
            _ => true,
        });
    assert!(anchorless);
    // Each has the others in its neighbour table, sleepy and anchorless,
    // and keeps no heard table.
    let n = h.nodes[1]
        .core
        .neighbours()
        .filter(|n| n.anchorless)
        .count();
    assert_eq!(n, 2);
    assert!(h.nodes[1].core.heard_nodes().is_empty());
    let longest = anchorless_beat_period(Config::default().delivery_ttl)
        .mul(3)
        .div(2)
        + secs(1);
    assert!(b.windows(2).all(|w| w[1].0.since(w[0].0) <= longest));
    assert!(b.windows(2).all(|w| w[1].1 == w[0].1.next()));
    // Its radio receives duty-cycled nearly all the time.
    let span = h.now.since(at(400));
    let sniffed: u64 = h.nodes[1]
        .sniffed
        .iter()
        .map(|&(a, z)| z.min(h.now).since(a.max(at(400))).as_micros())
        .sum();
    assert!(
        sniffed * 100 > span.as_micros() * 98,
        "{sniffed} of {span:?}"
    );
    let id = send(&mut h, 0, 2, b"among ourselves");
    assert!(h.run_until(h.now + secs(10), |h| delivered_at(h, 2, id).is_some()));
    for i in 0..2 {
        let data = h.nodes[i]
            .sent
            .iter()
            .filter(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Data(_))));
        assert!(data.clone().count() > 0);
        assert!(data.clone().all(|(_, f)| long(f)), "node {i}");
    }
}

/// An anchorless node is no anchor: S1 hears only S2, which is awake and
/// acts as a relay until it finds R, and S1 never takes it, since S2 would
/// sleep as soon as it had an anchor of its own (spec "Relay choice").
#[test]
fn an_anchorless_node_is_no_anchor() {
    let (s1, s2, r) = (0, 1, 2);
    let mut h = Harness::new(vec![
        node(0.0, true, 34),
        node(400.0, true, 35),
        node(800.0, false, 36),
    ]);
    h.link(s1, s2);
    h.link(s2, r);
    h.start_at(r, at(600));
    h.run_until(at(1500), |h| h.nodes[s1].core.anchor().is_some());
    assert_eq!(h.nodes[s1].core.anchor(), None);
    let anchor = h.nodes[s2].core.anchor().map(|a| a.id);
    assert_eq!(anchor, Some(h.nodes[r].core.id()));
}

/// A relay that comes into reach of a sleepy node without an anchor hears
/// its beat when the beat comes to the relay's channel and answers it on
/// the node's channel with the long preamble, at once and a fade later; the
/// node, receiving duty-cycled, hears the answer and takes the relay as its
/// anchor (spec "Sleepy nodes alone").
#[test]
fn a_relay_answers_an_anchorless_beat_and_brings_the_node_in() {
    let (s, r) = (0, 1);
    let mut h = Harness::new(vec![node(0.0, true, 37), node(400.0, false, 38)]);
    h.link(s, r);
    h.start_at(r, at(600));
    let rotation = anchorless_beat_period(Config::default().delivery_ttl)
        .mul(bark_proto::radio::CHANNELS as u64);
    let limit = at(600) + rotation.mul(2);
    assert!(h.run_until(limit, |h| h.nodes[s].core.anchor().is_some()));
    let anchored = h.now;
    assert_eq!(
        h.nodes[s].core.anchor().map(|a| a.id),
        Some(h.nodes[r].core.id())
    );
    // What took it was an answer: a heartbeat of R's on S's channel with
    // the long preamble, after a beat of S's.
    let answers: Vec<_> = h.nodes[r]
        .sent
        .iter()
        .filter(|(_, f)| {
            f.preamble_symbols == LONG_PREAMBLE_SYMBOLS
                && matches!(Packet::decode(f.payload()), Ok(Packet::Heartbeat(_)))
        })
        .map(|(t, f)| (*t, f.channel))
        .collect();
    assert!(!answers.is_empty());
    let (first, ch) = answers[0];
    assert!(
        anchored.since(first) < secs(1),
        "{:?}",
        anchored.since(first)
    );
    let s_beats = beats(&h, s);
    let beat = s_beats.iter().rev().find(|b| b.0 < first).expect("a beat");
    assert!(first.since(beat.0) < secs(2));
    let home = h.nodes[s]
        .sent
        .iter()
        .find_map(|(_, f)| match Packet::decode(f.payload()) {
            Ok(Packet::Heartbeat(hb)) => Some(hb.home),
            _ => None,
        });
    assert_eq!(Some(ch), home, "on the channel its beat names");
    // Once anchored the node beats with the sleepy bit, and R answers no more.
    h.run_for(secs(300));
    let after = h.nodes[r]
        .sent
        .iter()
        .filter(|(t, f)| *t > anchored && f.preamble_symbols == LONG_PREAMBLE_SYMBOLS)
        .count();
    assert_eq!(after, 0);
}

/// A sleepy node's own message with no live neighbour waits for one
/// until its time runs out (spec "Receiving DATA").
#[test]
fn a_sleepy_node_alone_keeps_its_message_for_a_ttl() {
    let mut h = Harness::new(vec![node(0.0, true, 41)]);
    h.run_until(at(400), |_| false);
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

/// Time node `i`'s radio received duty-cycled between `from` and `to`.
fn sniffed(h: &Harness<Relay>, i: usize, from: Instant, to: Instant) -> Duration {
    h.nodes[i]
        .sniffed
        .iter()
        .map(|&(a, b)| (a.max(from), b.min(to)))
        .filter(|(a, b)| b > a)
        .fold(Duration::ZERO, |sum, (a, b)| sum + b.since(a))
}

/// A sleepy node with an anchor lets its radio receive duty-cycled
/// (`Action::Sniff`, core-interface §3 rule 10) except while it beats and
/// through the window after each beat (spec "Delivery"), and a message for
/// it still reaches it there.
#[test]
fn a_sleepy_node_receives_duty_cycled_between_its_windows() {
    let (r, s) = (0, 1);
    let mut h = Harness::new(vec![node(0.0, false, 61), node(50.0, true, 62)]);
    h.link(r, s);
    h.start_at(s, at(10));
    h.run_until(at(1000), |_| false);
    assert!(h.nodes[s].core.anchor().is_some());
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
    // Awake from each beat through its window, and a hand-over after each
    // message it answers.
    let linger = HeardParams::from_config(&Config::default()).linger;
    let answered: Vec<Instant> = h.nodes[s]
        .sent
        .iter()
        .filter(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Ack(_))))
        .map(|(t, _)| *t)
        .collect();
    for &t in &b {
        // An answer in the window starts the hand-over after it instead.
        let end = answered
            .iter()
            .find(|&&a| a >= t && a < t + window)
            .map_or(t + window, |&a| a + linger);
        assert_eq!(sniffed(&h, s, t, end), Duration::ZERO, "beat at {t:?}");
    }
    for &a in &answered {
        assert_eq!(
            sniffed(&h, s, a, a + linger),
            Duration::ZERO,
            "answer at {a:?}"
        );
    }
    let (from, to) = (at(1000), at(3000));
    let awake = to.since(from) - sniffed(&h, s, from, to);
    // The windows, the delivery's, and once a rotation of its anchor's
    // heartbeat a watch for it, from 420 ms before to 431 ms after the
    // time its interval gives (spec "Relay choice").
    let c = Config::default();
    let anchor = h.nodes[s].core.anchor().unwrap().id;
    let rotation = bark_proto::node::heartbeat_interval(anchor, &c).mul(13);
    let watches = to.since(from).as_micros() / rotation.as_micros() + 1;
    let watch = Duration::from_millis(860);
    let most =
        window.mul(b.len() as u64 + 2) + linger.mul(answered.len() as u64) + watch.mul(watches);
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
    // Receiving duty-cycled when the application sends.
    let sniffing = |h: &Harness<Relay>| {
        h.nodes[s]
            .sniffed
            .last()
            .is_some_and(|&(a, b)| a <= h.now && b > h.now + secs(1))
    };
    assert!(h.run_until(h.now + secs(400), sniffing));
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

/// A sleepy node's own message that its anchor refuses goes back to the
/// application with the wait the refusal asked for, and the node takes no
/// message until that wait is over (spec "A sleepy node's own message").
#[test]
fn a_refused_message_goes_back_to_the_application_with_its_wait() {
    use bark_proto::radio::{time_on_air, MIN_PREAMBLE_SYMBOLS};
    use bark_proto::tables::budget::told;
    use bark_proto::wire::Ack;
    use bark_proto::{Frame, MAX_FRAME};
    let (r, s) = (0, 1);
    let mut h = Harness::new(vec![node(0.0, false, 75), node(50.0, true, 76)]);
    h.link(r, s);
    h.start_at(s, at(10));
    h.run_until(at(1000), |_| false);
    assert!(h.accepts(s));
    // The anchor does not hear the DATA; the test answers for it.
    h.lose = Some(Box::new(move |_, from, to, bytes| {
        from == s && to == r && matches!(Packet::decode(bytes), Ok(Packet::Data(_)))
    }));
    let id = send(&mut h, s, r, b"refused");
    let data = |h: &Harness<Relay>| {
        h.nodes[s]
            .sent
            .iter()
            .find(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Data(_))))
            .map(|(t, f)| *t + time_on_air(f.len, f.preamble_symbols))
    };
    assert!(h.run_until(h.now + secs(5), |h| data(h).is_some()));
    let end = data(&h).unwrap();
    h.run_until(end + Duration::from_millis(5), |_| false);
    let bans = 4;
    let nack = Ack {
        msg_id: id,
        from: h.nodes[r].core.id(),
        to: h.nodes[s].core.id(),
        refused: true,
        wait: bans,
    };
    let mut frame = Frame {
        channel: h.resting(s).unwrap(),
        preamble_symbols: MIN_PREAMBLE_SYMBOLS,
        len: 0,
        bytes: [0; MAX_FRAME],
    };
    frame.len = Packet::Ack(nack).encode(&mut frame.bytes).unwrap() as u8;
    let sent = h.now;
    h.inject(r, frame);
    let given = |h: &Harness<Relay>| {
        h.nodes[s].app.iter().find_map(|(t, e)| match *e {
            AppEvent::GivenBack { msg_id, retry_at } if msg_id == id => Some((*t, retry_at)),
            _ => None,
        })
    };
    assert!(h.run_until(h.now + secs(1), |h| given(h).is_some()));
    let (t, retry_at) = given(&h).unwrap();
    let heard = sent + time_on_air(frame.len, frame.preamble_symbols);
    assert!(t >= heard && t < heard + Duration::from_millis(5), "{t:?}");
    assert_eq!(retry_at, t + told(bans));
    assert_eq!(retry_at, h.nodes[s].core.retry_at());
    assert!(h.nodes[s].traces.iter().any(|(_, x)| *x
        == Trace::Dropped {
            msg_id: id,
            reason: DropReason::Pushback
        }));
    assert!(h.nodes[s].core.holding().next().is_none());
    h.run_until(retry_at - Duration::from_millis(1), |h| h.accepts(s));
    assert!(!h.accepts(s), "took a message before the wait was over");
    assert!(h.run_until(retry_at + secs(1), |h| h.accepts(s)));
}

/// Messages waiting for a sleepy node go one after the other in the
/// window after its beat: the node listens one hand-over after each it
/// answers, and receives duty-cycled again once a hand-over has passed
/// after the last (spec "Delivery"). The first DATA, with the long
/// preamble before the beat, is lost, so the holder waits for the beat.
#[test]
fn a_burst_goes_on_one_beat_and_the_node_sniffs_a_hand_over_after_the_last() {
    let (r, s) = (0, 1);
    let mut h = Harness::new(vec![node(0.0, false, 81), node(50.0, true, 82)]);
    h.link(r, s);
    h.start_at(s, at(10));
    h.run_until(at(1000), |_| false);
    let attempts = Config::default().hop_attempts as usize;
    let mut lost = 0;
    h.lose = Some(Box::new(move |_, _, to, bytes| {
        let data = to == s && matches!(Packet::decode(bytes), Ok(Packet::Data(_)));
        let lose = data && lost < attempts;
        lost += usize::from(lose);
        lose
    }));
    let sniffing = |h: &Harness<Relay>| {
        h.nodes[s]
            .sniffed
            .last()
            .is_some_and(|&(a, b)| a <= h.now && b > h.now + secs(1))
    };
    assert!(h.run_until(h.now + secs(400), sniffing));
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
    assert!(got[2].since(beat) < p.awake + p.linger.mul(2), "one beat");
    h.run_for(p.linger + secs(1));
    let last_ack = h.nodes[s]
        .sent
        .iter()
        .filter(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Ack(_))))
        .map(|(t, _)| *t)
        .next_back()
        .unwrap();
    let sniffs_at = h.nodes[s].sniffed.last().unwrap().0;
    assert_eq!(
        sniffed(&h, s, last_ack, last_ack + p.linger),
        Duration::ZERO
    );
    assert!(
        sniffs_at.since(last_ack) < p.linger + Duration::from_millis(50),
        "duty-cycled {:?} after its last answer",
        sniffs_at.since(last_ack)
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

/// A heartbeat of its anchor that a sleeping node hears between its
/// watches, as it hears an answer to a node without an anchor on its
/// channel, shows the anchor alive and leaves the time it expects the
/// next where it was: the node checks its anchor no more for answers once
/// a minute, and hears every heartbeat of a slot (spec "Relay choice").
#[test]
fn heartbeats_out_of_slot_do_not_move_the_watch() {
    use bark_proto::wire::Heartbeat;
    use bark_proto::{Frame, MAX_FRAME};
    let (r, s) = (0, 1);
    let mut h = Harness::new(vec![node(0.0, false, 131), node(50.0, true, 132)]);
    h.link(r, s);
    h.start_at(s, at(10));
    h.run_until(at(1000), |_| false);
    let home = h.resting(s).unwrap();
    let relay = &h.nodes[r].core;
    let hb = Heartbeat {
        sleepy: false,
        anchorless: false,
        home: relay.home(),
        id: relay.id(),
        short: relay.config().position.short(),
    };
    let mut answer = Frame {
        channel: home,
        preamble_symbols: LONG_PREAMBLE_SYMBOLS,
        len: 0,
        bytes: [0; MAX_FRAME],
    };
    answer.len = Packet::Heartbeat(hb).encode(&mut answer.bytes).unwrap() as u8;
    // Answers as a relay sends them to a node without an anchor that
    // does not hear them: at once and a fade later, each time the node's
    // beat comes to the relay's channel, about every 450 s, for an hour.
    for k in 0..8 {
        for fade in [0, 60] {
            h.run_until(at(1000 + 450 * k + fade), |_| false);
            h.inject(r, answer);
        }
    }
    h.run_until(at(4800), |_| false);
    let checks = h.nodes[s]
        .traces
        .iter()
        .filter(|(t, x)| *t >= at(1000) && *x == Trace::AnchorCheck)
        .count();
    assert_eq!(checks, 0, "checked its anchor");
    let missed = h.nodes[s]
        .traces
        .iter()
        .filter(|(t, x)| *t >= at(1000) && matches!(x, Trace::AnchorWatch { heard: false, .. }))
        .count();
    assert_eq!(missed, 0, "missed a watch");
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
/// home, and knows from the anchor's interval when it comes to the new. It
/// beats there soon after, so its holder knows where it listens.
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
    // It beats on the new channel within a heartbeat interval, give or take
    // the listen-before-talk, and its holder learns where it listens.
    let followed = h.now;
    let id = h.nodes[s].core.id();
    let known =
        |h: &Harness<Relay>| h.nodes[r].core.heard_nodes().get(id).map(|n| n.home) == Some(new);
    assert!(h.run_until(followed + c.heartbeat_max + secs(1), known));
    assert!(beats(&h, s)
        .iter()
        .any(|&(t, on, sleepy)| t >= followed && on == new && sleepy));
}

/// A holder that moves its home takes along the sleepy nodes that listen
/// now: they hear the heartbeat it sends on the old channel and follow at
/// once, so it sends to them on the new channel without waiting for their
/// next beat (spec "Channel moves"). Here the node listens all the time
/// (`sleepy_listens`).
#[test]
fn a_holder_that_moves_sends_on_its_new_channel_to_who_listens() {
    let (r, s) = (0, 1);
    let config = Config {
        sleepy_listens: true,
        ..Config::default()
    };
    let mut h = Harness::new(vec![
        node_with(0.0, false, 121, config),
        node_with(50.0, true, 122, config),
    ]);
    h.link(r, s);
    h.start_at(s, at(10));
    h.run_until(at(1000), |_| false);
    let old = h.resting(r).unwrap();
    assert_eq!(h.resting(s), Some(old));
    // As in the follow test above.
    h.jam_part(
        r,
        old,
        Duration::from_millis(32),
        Duration::from_millis(107),
    );
    let moved = |h: &Harness<Relay>| h.resting(r) != Some(old);
    assert!(h.run_until(h.now + secs(1500), moved));
    let new = h.resting(r).unwrap();
    let id = h.nodes[s].core.id();
    assert_eq!(
        h.nodes[r].core.heard_nodes().get(id).map(|n| n.home),
        Some(new)
    );
    let follows = |h: &Harness<Relay>| h.resting(s) == Some(new);
    assert!(h.run_until(h.now + secs(1), follows));
    let sent_at = h.now;
    let msg = send(&mut h, r, s, b"after the move");
    assert!(h.run_until(sent_at + secs(1), |h| delivered_at(h, s, msg).is_some()));
}

/// A node out of reach of everything, in a basement, misses its anchor's
/// heartbeat, checks, hears nothing at all, and keeps its anchor without
/// checking again while it hears nothing; back in reach it hears it again
/// and receives duty-cycled as before (spec "Relay choice").
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
    let awake = secs(600) - sniffed(&h, s, back + secs(600), back + secs(1200));
    assert!(awake < secs(30), "awake {awake:?} in ten minutes once back");
}

/// A delivery whose every attempt goes unanswered marks the node asleep:
/// the holder says the message waits for the next beat
/// (`Trace::DeliveryWaits`), and it goes then (spec "Delivery").
#[test]
fn an_unanswered_delivery_waits_for_the_next_beat() {
    let (r, s) = (0, 1);
    let mut h = Harness::new(vec![node(0.0, false, 141), node(50.0, true, 142)]);
    h.link(r, s);
    h.start_at(s, at(10));
    h.run_until(at(1000), |_| false);
    let id = send(&mut h, r, s, b"lost thrice");
    let attempts = Config::default().hop_attempts as usize;
    let mut lost = 0;
    h.lose = Some(Box::new(move |_, _, to, bytes| {
        let data = to == s && matches!(Packet::decode(bytes), Ok(Packet::Data(_)));
        let lose = data && lost < attempts;
        lost += usize::from(lose);
        lose
    }));
    let waits = |h: &Harness<Relay>| {
        h.nodes[r].traces.iter().find_map(|(t, e)| match e {
            Trace::DeliveryWaits { msg_id, tried, .. } if *msg_id == id => Some((*t, *tried)),
            _ => None,
        })
    };
    assert!(h.run_until(h.now + secs(400), |h| waits(h).is_some()));
    let (waited, tried) = waits(&h).unwrap();
    assert!(tried, "its hand-over was tried");
    assert!(h.run_until(h.now + secs(400), |h| delivered_at(h, s, id).is_some()));
    let got = delivered_at(&h, s, id).unwrap();
    let beat = beats(&h, s)
        .into_iter()
        .map(|(t, _, _)| t)
        .rfind(|&t| t <= got)
        .unwrap();
    assert!(beat > waited, "delivered on a later beat");
}

/// The way out of a pocket: A, a sleepy node without an anchor, hears no
/// relay, and S, a sleepy node anchored to R, is its only neighbour. S
/// hears one of A's long beats when it comes to S's channel, and from then
/// on beats once a period on A's channel too, with the long preamble and
/// the sleepy bit alone; A keeps it as a last resort, and its message for
/// R goes through S, with the long preamble to S and the short one from S
/// to R (spec "Beats", "Next hop").
///
/// ```text
///   A ---- S ---- R
/// ```
#[test]
fn a_pocket_learns_its_way_out_from_long_beats() {
    let (r, s, a) = (0, 1, 2);
    let on = |c: u8| Config {
        home_override: bark_proto::Channel::new(c),
        ..Config::default()
    };
    let mut h = Harness::new(vec![
        node_with(0.0, false, 151, on(3)),
        node(400.0, true, 152),
        node_with(800.0, true, 153, on(9)),
    ]);
    h.link(r, s);
    h.link(s, a);
    let rotation = anchorless_beat_period(Config::default().delivery_ttl)
        .mul(bark_proto::radio::CHANNELS as u64);
    let sid = h.nodes[s].core.id();
    let knows_s = |h: &Harness<Relay>| {
        h.nodes[a]
            .core
            .neighbours()
            .any(|n| n.id == sid && n.last_resort())
    };
    let limit = at(400) + rotation.mul(2) + beat_period(Config::default().delivery_ttl);
    assert!(h.run_until(limit, knows_s));
    assert!(h.nodes[a].core.anchorless());
    let anchor = h.nodes[s].core.anchor().map(|x| x.id);
    assert_eq!(anchor, Some(h.nodes[r].core.id()));
    let n = *h.nodes[a].core.neighbours().find(|n| n.id == sid).unwrap();
    assert_eq!(Some(n.home), bark_proto::Channel::new(3));
    let reach = h.nodes[s].sent.iter().any(|(_, f)| {
        let beat = matches!(Packet::decode(f.payload()),
            Ok(Packet::Heartbeat(hb)) if hb.sleepy && !hb.anchorless);
        beat && f.preamble_symbols == LONG_PREAMBLE_SYMBOLS
            && Some(f.channel) == bark_proto::Channel::new(9)
    });
    assert!(reach, "a long beat on A's channel");
    // Only once S has heard A anchorless in two rotations of A's beats, at
    // least half a rotation apart: one heard once may be switching on.
    let aid = h.nodes[a].core.id();
    let heard_a: Vec<Instant> = h.nodes[s]
        .received
        .iter()
        .filter(|(_, b)| matches!(Packet::decode(b), Ok(Packet::Heartbeat(hb)) if hb.id == aid))
        .map(|(t, _)| *t)
        .collect();
    let first_long = h.nodes[s]
        .sent
        .iter()
        .find(|(_, f)| f.preamble_symbols == LONG_PREAMBLE_SYMBOLS)
        .map(|(t, _)| *t)
        .unwrap();
    let before: Vec<_> = heard_a.iter().filter(|&&t| t < first_long).collect();
    assert!(
        before.len() >= 2 && before[before.len() - 1].since(*before[0]) >= rotation.div(2),
        "{heard_a:?} {first_long:?}"
    );
    let id = send(&mut h, a, r, b"out of the pocket");
    assert!(h.run_until(h.now + secs(20), |h| delivered_at(h, r, id).is_some()));
    let data = |i: usize| {
        h.nodes[i]
            .sent
            .iter()
            .filter(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Data(_))))
            .map(|(_, f)| f.preamble_symbols)
            .collect::<Vec<_>>()
    };
    assert_eq!(data(a), [LONG_PREAMBLE_SYMBOLS]);
    assert_eq!(data(s), [bark_proto::radio::MIN_PREAMBLE_SYMBOLS]);
    // Each of S's beats goes on A's channel too, just before it.
    let from = h.now;
    h.run_for(beat_period(Config::default().delivery_ttl).mul(6));
    let sent: Vec<_> = beats(&h, s)
        .into_iter()
        .filter(|b| b.0 >= from)
        .map(|b| b.1)
        .collect();
    let on = |c: u8| {
        sent.iter()
            .filter(|&&x| Some(x) == bark_proto::Channel::new(c))
            .count()
    };
    assert!(on(3) >= 6, "{sent:?}");
    assert!(on(9).abs_diff(on(3)) <= 1, "{sent:?}");
}

/// A message that reaches its owner at the hop cap has found its way: the
/// owner delivers it to its node, at the cap, rather than dropping it at
/// the last step and finding it in its delivery queue again
/// (spec "Hop count").
#[test]
fn a_message_at_the_hop_cap_is_still_delivered() {
    use bark_proto::radio::MIN_PREAMBLE_SYMBOLS;
    use bark_proto::{Frame, MAX_FRAME};
    let (r, q, s) = (0, 1, 2);
    let mut h = Harness::new(vec![
        node(0.0, false, 161),
        node(-300.0, false, 162),
        node(50.0, true, 163),
    ]);
    h.link(r, s);
    h.link(q, r);
    h.start_at(s, at(10));
    h.run_until(at(1000), |_| false);
    assert!(h.nodes[s].core.anchor().is_some());
    let text = b"the long way round";
    let (dst_pos, dst_id) = (h.nodes[s].core.address(), h.nodes[s].core.id());
    let data = Data {
        sender_home: h.resting(q).unwrap(),
        dst_hop: h.nodes[r].core.id(),
        src_hop: h.nodes[q].core.id(),
        dst_pos,
        dst_id,
        hops: u8::MAX,
        time_left: u8::MAX,
        payload: text,
    };
    let mut frame = Frame {
        channel: h.resting(r).unwrap(),
        preamble_symbols: MIN_PREAMBLE_SYMBOLS,
        len: 0,
        bytes: [0; MAX_FRAME],
    };
    frame.len = Packet::Data(data).encode(&mut frame.bytes).unwrap() as u8;
    h.inject(q, frame);
    let id = MsgId::of(dst_pos, dst_id, text);
    assert!(h.run_until(h.now + secs(700), |h| delivered_at(h, s, id).is_some()));
    let dropped = h.nodes[r]
        .traces
        .iter()
        .filter(|(_, t)| matches!(t, Trace::Dropped { msg_id, .. } if *msg_id == id))
        .count();
    assert_eq!(dropped, 0);
}

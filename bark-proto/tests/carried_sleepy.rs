//! Sleepy and carried are two properties of a node, each measured on its
//! own and each published as a heartbeat flag (spec "Nodes", "Position",
//! roadmap D72): a node on battery in a pocket is both, and its power
//! changes only whether it is sleepy. A sleepy node with an anchor takes
//! no fix and keeps the flag it had (roadmap D91).

mod common;

use bark_proto::radio::Channel;
use bark_proto::sniff::LONG_PREAMBLE_SYMBOLS;
use bark_proto::wire::{Heartbeat, Packet};
use bark_proto::{AppCommand, Config, Instant, MsgId, Position, Relay, Stored, Trace};
use common::{heartbeat_flags, secs, Harness, ANCHORLESS, CARRIED, RELAY, SLEEPY};

/// A position `east` metres from a point in Kyiv.
fn pos(east: f64) -> Position {
    let (lat, lon) = (50.45, 30.52);
    let m_per_deg = 6_371_000.0 * core::f64::consts::PI / 180.0;
    Position::from_degrees(lat, lon + east / (m_per_deg * lat.to_radians().cos()))
}

/// How long a carried node's fixes must stay in one place in these tests,
/// shorter than the design's four hours to keep them quick.
const STILL: u64 = 600;

/// A node at `east`, and whether it is on external power at boot.
fn node(east: f64, external: bool, seed: u64) -> (Relay, bool) {
    let config = Config {
        position: pos(east),
        fixed_after: secs(STILL),
        ..Config::default()
    };
    (Relay::new(config, seed), external)
}

/// A node at `east` on battery that boots carried, as one whose fixes
/// walked before its reboot does (`store.rs`): a sleepy node with an
/// anchor takes no fix, so this is how a carried sleepy node comes to be
/// in these tests.
fn carried_node(east: f64, seed: u64) -> (Relay, bool) {
    let stored = Stored {
        home: Channel::all().next().expect("a channel"),
        place: pos(east),
        carried: true,
        map_point: false,
        anchor: None,
        relay: false,
        battery_scale: None,
    };
    let config = Config {
        position: pos(east),
        fixed_after: secs(STILL),
        stored: Some(stored),
        ..Config::default()
    };
    (Relay::new(config, seed), false)
}

fn at(s: u64) -> Instant {
    Instant::ZERO + secs(s)
}

fn fix(h: &mut Harness<Relay>, i: usize, east: f64) {
    h.command(
        i,
        AppCommand::Fix {
            position: pos(east),
        },
    );
}

/// Node `i` walks east from `from` at 1.4 m/s, a fix every 10 s, until it
/// is carried, and a second more. Returns where it is then.
fn walk_until_carried(h: &mut Harness<Relay>, i: usize, from: f64) -> f64 {
    let mut east = from;
    for _ in 0..20 {
        fix(h, i, east);
        if h.nodes[i].core.carried() {
            h.run_for(secs(1));
            return east;
        }
        h.run_for(secs(10));
        east += 14.0;
    }
    panic!("node {i} not carried");
}

/// Node `i` stands at `east`, a fix every minute, for `s` seconds.
fn stand(h: &mut Harness<Relay>, i: usize, east: f64, s: u64) {
    for _ in 0..s / 60 {
        fix(h, i, east);
        h.run_for(secs(60));
    }
}

fn traced(h: &Harness<Relay>, i: usize, want: Trace) -> usize {
    h.nodes[i].traces.iter().filter(|(_, t)| *t == want).count()
}

fn power(h: &mut Harness<Relay>, i: usize, external: bool) {
    h.command(i, AppCommand::Power { external });
}

/// The types of the heartbeats node `i` sent after `from`, with the
/// channel and preamble of each.
fn heartbeats(h: &Harness<Relay>, i: usize, from: Instant) -> Vec<(u8, u8, u16)> {
    h.nodes[i]
        .sent
        .iter()
        .filter(|(t, f)| {
            *t >= from && matches!(Packet::decode(f.payload()), Ok(Packet::Heartbeat(_)))
        })
        .map(|(_, f)| {
            (
                heartbeat_flags(f.payload()).unwrap(),
                f.channel.index(),
                f.preamble_symbols,
            )
        })
        .collect()
}

/// The type of node `i`'s last heartbeat.
fn last_type(h: &Harness<Relay>, i: usize) -> u8 {
    heartbeats(h, i, Instant::ZERO)
        .last()
        .expect("a heartbeat")
        .0
}

/// A relay R and two sleepy nodes S and P anchored to it, P booted
/// carried; F, a relay, and L, a sleepy node with no relay in reach; G,
/// a relay R knows. F and L walk until carried. Each of the four
/// combinations heartbeats its own flags: a fixed relay none, a carried
/// relay carried, a fixed sleepy node sleepy, a carried one sleepy and
/// carried, and L all three; and a neighbour keeps both flags as heard.
#[test]
fn the_four_combinations_heartbeat_their_flags() {
    let (r, f, s, p, l, g) = (0, 1, 2, 3, 4, 5);
    // Built one by one on the heap: six nodes at once overflow a debug
    // test thread's stack.
    let nodes = [
        (0.0, true),
        (300.0, true),
        (20.0, false),
        (30.0, false),
        (5000.0, false),
        (-300.0, true),
    ];
    let mut h = Harness::powered(
        (1..)
            .zip(nodes)
            .map(|(seed, (east, external))| {
                if seed == p as u64 + 1 {
                    carried_node(east, seed)
                } else {
                    node(east, external, seed)
                }
            })
            .collect(),
    );
    // Without G, R would know no relay that holds mail once F is carried,
    // and say so with the anchorless flag (spec "Heartbeat").
    h.link(r, g);
    for i in [f, s, p] {
        h.link(r, i);
    }
    h.set_rssi(r, f, -70);
    h.set_rssi(f, p, -110);
    h.link(f, p);
    h.start_at(s, at(10));
    h.start_at(p, at(10));
    h.run_until(at(1000), |_| false);
    let rid = h.nodes[r].core.id();
    assert_eq!(h.nodes[p].core.anchor().map(|a| a.id), Some(rid));
    assert!(h.nodes[p].core.carried());
    assert!(h.nodes[l].core.anchorless());
    walk_until_carried(&mut h, f, 300.0);
    walk_until_carried(&mut h, l, 5000.0);
    let from = h.now;
    h.run_for(secs(700));
    let types = |i| {
        let mut t: Vec<u8> = heartbeats(&h, i, from).iter().map(|b| b.0).collect();
        t.dedup();
        t
    };
    assert_eq!(types(r), [RELAY]);
    assert_eq!(types(f), [CARRIED]);
    assert_eq!(types(s), [SLEEPY]);
    assert_eq!(types(p), [SLEEPY | CARRIED]);
    assert_eq!(types(l), [SLEEPY | ANCHORLESS | CARRIED]);
    let pid = h.nodes[p].core.id();
    let heard = h.nodes[r].core.neighbours().find(|n| n.id == pid).unwrap();
    assert!(heard.sleepy && heard.carried && !heard.anchorless);
    // Being carried changes nothing of how a sleepy node anchors.
    assert_eq!(h.nodes[p].core.anchor().map(|a| a.id), Some(rid));
}

/// A sleepy node with an anchor works by the anchor's position and takes
/// no fix (spec "Beats", roadmap D91): fixes that walk away make it
/// neither carried nor check its anchor, its beats carry the anchor's
/// position, which its holder keeps it at, and its own place stays for
/// when it is a relay again.
#[test]
fn a_sleepy_node_with_an_anchor_takes_no_fix() {
    let (r, s) = (0, 1);
    let mut h = Harness::powered(vec![node(0.0, true, 11), node(20.0, false, 13)]);
    h.link(r, s);
    h.start_at(s, at(10));
    h.run_until(at(1000), |_| false);
    let rid = h.nodes[r].core.id();
    assert_eq!(h.nodes[s].core.anchor().map(|a| a.id), Some(rid));
    assert_eq!(h.nodes[s].core.address(), pos(0.0));
    let before = h.now;
    let mut east = 20.0;
    for _ in 0..40 {
        fix(&mut h, s, east);
        east += 14.0;
        h.run_for(secs(10));
    }
    assert!(!h.nodes[s].core.carried());
    assert_eq!(traced(&h, s, Trace::FarFix), 0);
    assert_eq!(traced(&h, s, Trace::Changed), 0);
    assert_eq!(traced(&h, s, Trace::AnchorCheck), 0);
    assert_eq!(h.nodes[s].core.config().position, pos(20.0));
    assert_eq!(h.nodes[s].core.address(), pos(0.0));
    let beats = heartbeats(&h, s, before);
    assert!(!beats.is_empty() && beats.iter().all(|b| b.0 == SLEEPY));
    let last = last_heartbeat_since(&h, s, before).expect("a beat");
    assert_eq!(last.position(pos(0.0)), pos(0.0));
    let sid = h.nodes[s].core.id();
    let heard = h.nodes[r].core.neighbours().find(|n| n.id == sid).unwrap();
    assert_eq!(heard.position, pos(0.0));
}

/// Without an anchor a sleepy node measures from its fixes as any node
/// does: walking makes it carried and staying makes it fixed again, each
/// said at once with a beat. The flag it has when it takes an anchor
/// stays, since with one it takes no fix (roadmap D91).
#[test]
fn an_anchorless_node_measures_its_fixes_and_keeps_the_flag_at_an_anchor() {
    let (r, l) = (0, 1);
    let mut h = Harness::powered(vec![node(0.0, true, 21), node(20.0, false, 22)]);
    h.start_at(l, at(10));
    h.run_until(at(1000), |_| false);
    assert!(h.nodes[l].core.anchorless());
    let stop = walk_until_carried(&mut h, l, 20.0);
    assert_eq!(traced(&h, l, Trace::Changed), 1);
    let half = STILL / 2;
    stand(&mut h, l, stop + 30.0, half);
    assert!(h.nodes[l].core.carried(), "half the time still");
    stand(&mut h, l, stop + 10.0, half + 60);
    assert!(!h.nodes[l].core.carried());
    assert_eq!(traced(&h, l, Trace::Carried { carried: false }), 1);
    assert_eq!(traced(&h, l, Trace::Changed), 2);
    assert_eq!(last_type(&h, l), SLEEPY | ANCHORLESS);
    // Still anchorless, it takes each fix.
    fix(&mut h, l, stop + 40.0);
    assert_eq!(h.nodes[l].core.config().position, pos(stop + 40.0));
    // Carried again as R comes in reach, it anchors carried, and stays so
    // however long its fixes stand, since it takes none.
    let stop = walk_until_carried(&mut h, l, stop + 40.0);
    h.link(r, l);
    let rid = h.nodes[r].core.id();
    assert!(h.run_until(h.now + secs(700), |h| h.nodes[l].core.anchor().is_some()));
    assert_eq!(h.nodes[l].core.anchor().map(|a| a.id), Some(rid));
    stand(&mut h, l, stop, STILL + 120);
    assert!(h.nodes[l].core.carried());
    assert_eq!(h.nodes[l].core.config().position, pos(stop));
    assert_eq!(h.nodes[l].core.address(), pos(0.0));
    assert_eq!(last_type(&h, l), SLEEPY | CARRIED);
}

/// A carried sleepy node plugged in is a carried relay: it heartbeats
/// carried alone, owns nothing by position, so queues no text for a sleepy node it
/// hears, and no sleepy node anchors to it, however strong; four hours
/// still (here `STILL`) make it fixed, a relay that holds mail.
#[test]
fn a_carried_sleepy_node_plugged_in_is_a_carried_relay() {
    let (r, c, t, o) = (0, 1, 2, 3);
    let mut h = Harness::powered(vec![
        node(0.0, true, 31),
        carried_node(20.0, 32),
        node(200.0, false, 33),
        node(400.0, true, 34),
    ]);
    h.link(r, c);
    h.link(r, o);
    h.link(c, o);
    // C anchors to R, and listens on its channel once plugged in.
    h.set_rssi(r, c, -60);
    h.start_at(c, at(10));
    h.run_until(at(1000), |_| false);
    let stop = 20.0;
    assert!(h.nodes[c].core.carried() && h.nodes[c].core.anchor().is_some());
    power(&mut h, c, true);
    assert!(!h.nodes[c].core.sleepy() && h.nodes[c].core.carried());
    assert_eq!(traced(&h, c, Trace::Role { sleepy: false }), 1);
    // T switches on beside it: C is far the stronger, R the only anchor.
    h.link(c, t);
    h.link(r, t);
    h.set_rssi(c, t, -60);
    h.set_rssi(r, t, -110);
    h.start_at(t, h.now);
    h.run_for(secs(600));
    assert_eq!(last_type(&h, c), CARRIED);
    let rid = h.nodes[r].core.id();
    assert_eq!(h.nodes[t].core.anchor().map(|a| a.id), Some(rid));
    let tid = h.nodes[t].core.id();
    assert!(
        h.nodes[c].core.heard_nodes().get(tid).is_some(),
        "C hears T"
    );
    // A text to C's position for T is searched on, not queued at C.
    let dst = h.nodes[c].core.config().position;
    let text = b"for T at the walker";
    h.command(
        o,
        AppCommand::Send {
            dst_pos: dst,
            dst_id: tid,
            payload: text,
        },
    );
    let id = MsgId::of(dst, tid, text);
    h.run_for(secs(30));
    let queued_at_c = h.nodes[c]
        .traces
        .iter()
        .any(|(_, t)| matches!(t, Trace::Queued { msg_id, .. } if *msg_id == id));
    assert!(!queued_at_c);
    assert_eq!(h.nodes[c].core.delivery_queue().count(), 0);
    // Still on its charger: fixed after STILL, where it stands then.
    stand(&mut h, c, stop + 5.0, STILL + 60);
    assert!(!h.nodes[c].core.carried());
    assert_eq!(h.nodes[c].core.config().position, pos(stop + 5.0));
    h.run_for(secs(30));
    assert_eq!(last_type(&h, c), RELAY);
    let dst = h.nodes[c].core.config().position;
    let text = b"for T at a relay that stays";
    h.command(
        o,
        AppCommand::Send {
            dst_pos: dst,
            dst_id: tid,
            payload: text,
        },
    );
    let id = MsgId::of(dst, tid, text);
    h.run_for(secs(30));
    assert!(h.nodes[c]
        .traces
        .iter()
        .any(|(_, t)| matches!(t, Trace::Queued { msg_id, .. } if *msg_id == id)));
}

/// A sleepy node that is not carried, plugged in, is fixed at once at the
/// place it has, its own and not its anchor's; its first fix as a relay
/// is its place (roadmap D85), which it keeps.
#[test]
fn a_still_sleepy_node_plugged_in_is_fixed_at_once() {
    let (r, s) = (0, 1);
    let mut h = Harness::powered(vec![node(0.0, true, 41), node(20.0, false, 42)]);
    h.link(r, s);
    h.start_at(s, at(10));
    h.run_until(at(1000), |_| false);
    assert_eq!(h.nodes[s].core.address(), pos(0.0));
    power(&mut h, s, true);
    assert!(!h.nodes[s].core.sleepy() && !h.nodes[s].core.carried());
    assert_eq!(h.nodes[s].core.address(), pos(20.0));
    let here = 100.0;
    h.run_for(secs(10));
    fix(&mut h, s, here);
    assert_eq!(h.nodes[s].core.config().position, pos(here));
    for k in 1..=10 {
        h.run_for(secs(10));
        fix(&mut h, s, here + 40.0 * (k % 2) as f64);
    }
    assert!(!h.nodes[s].core.carried());
    assert_eq!(h.nodes[s].core.config().position, pos(here));
    assert_eq!(last_type(&h, s), RELAY);
}

/// A sleepy node plugged in at a place its last fix does not show takes
/// its first fix within a search's limit as its place, and is not
/// carried; one whose first fix comes later keeps the place it had, and
/// a run of far fixes makes it carried (roadmap D85).
#[test]
fn a_sleepy_node_plugged_in_takes_its_first_fix_as_its_place() {
    for (wait, carried) in [(10, false), (70, true)] {
        let (r, s) = (0, 1);
        let mut h = Harness::powered(vec![node(0.0, true, 41), node(20.0, false, 42)]);
        h.link(r, s);
        h.start_at(s, at(10));
        h.run_until(at(1000), |_| false);
        power(&mut h, s, true);
        h.run_for(secs(wait));
        fix(&mut h, s, 300.0);
        let first = if carried { 20.0 } else { 300.0 };
        assert_eq!(h.nodes[s].core.config().position, pos(first));
        for _ in 0..10 {
            h.run_for(secs(10));
            fix(&mut h, s, 300.0);
        }
        assert_eq!(h.nodes[s].core.carried(), carried, "wait: {wait}");
        assert_eq!(traced(&h, s, Trace::FarFix), usize::from(carried));
    }
}

/// A map point fixes any node at once, sleepy or not, and a node whose
/// carried flag it clears says so at once.
#[test]
fn a_map_point_fixes_any_node() {
    for external in [false, true] {
        let (r, n) = (0, 1);
        let carried = if external {
            node(20.0, true, 52)
        } else {
            carried_node(20.0, 52)
        };
        let mut h = Harness::powered(vec![node(0.0, true, 51), carried]);
        h.link(r, n);
        h.start_at(n, at(10));
        h.run_until(at(1000), |_| false);
        if external {
            walk_until_carried(&mut h, n, 20.0);
        }
        assert!(h.nodes[n].core.carried());
        let changes = traced(&h, n, Trace::Changed);
        h.command(
            n,
            AppCommand::SetPosition {
                position: pos(700.0),
            },
        );
        assert!(!h.nodes[n].core.carried(), "external: {external}");
        assert_eq!(h.nodes[n].core.config().position, pos(700.0));
        h.run_for(secs(5));
        assert_eq!(traced(&h, n, Trace::Changed), changes + 1);
        let want = if external { RELAY } else { SLEEPY };
        assert_eq!(last_type(&h, n), want);
        // Its heartbeat says the position is a map point, and the
        // neighbour that heard it keeps that (spec "Packet formats").
        assert!(last_heartbeat(&h, n).map_point);
        let id = h.nodes[n].core.id();
        let pinned = |h: &Harness<Relay>| {
            h.nodes[r]
                .core
                .neighbours()
                .any(|e| e.id == id && e.map_point)
        };
        let until = h.now + secs(400);
        h.run_until(until, pinned);
        assert!(pinned(&h), "external: {external}");
        // No fix replaces a map point (roadmap D84): only the application
        // clears it, and the next accepted fix is the place.
        fix(&mut h, n, 900.0);
        assert_eq!(h.nodes[n].core.config().position, pos(700.0));
        h.command(n, AppCommand::ClearMapPoint);
        let since = h.now;
        h.run_until(at(5000), |h| last_heartbeat_since(h, n, since).is_some());
        let after = last_heartbeat_since(&h, n, since).expect("a heartbeat");
        assert!(!after.map_point, "external: {external}");
        fix(&mut h, n, 900.0);
        assert_eq!(h.nodes[n].core.config().position, pos(900.0));
        assert!(!h.nodes[n].core.carried(), "external: {external}");
    }
}

/// Node `i`'s last heartbeat.
fn last_heartbeat(h: &Harness<Relay>, i: usize) -> Heartbeat {
    last_heartbeat_since(h, i, Instant::ZERO).expect("a heartbeat")
}

/// Node `i`'s last heartbeat sent after `from`, if any.
fn last_heartbeat_since(h: &Harness<Relay>, i: usize, from: Instant) -> Option<Heartbeat> {
    h.nodes[i]
        .sent
        .iter()
        .rev()
        .find_map(|(t, f)| match Packet::decode(f.payload()) {
            Ok(Packet::Heartbeat(hb)) if *t > from => Some(hb),
            _ => None,
        })
}

/// An anchorless sleepy node on the move says so with the long preamble,
/// as all its heartbeats go (spec "Sleepy nodes alone").
#[test]
fn an_anchorless_node_on_the_move_says_so_with_the_long_preamble() {
    let l = 0;
    let mut h = Harness::powered(vec![node(0.0, false, 61)]);
    h.run_until(at(1000), |_| false);
    assert!(h.nodes[l].core.anchorless());
    let before = h.now;
    walk_until_carried(&mut h, l, 0.0);
    h.run_for(secs(1));
    let home = h.nodes[l].core.home().index();
    let first = heartbeats(&h, l, before)
        .into_iter()
        .find(|b| b.0 == SLEEPY | ANCHORLESS | CARRIED)
        .expect("flagged");
    assert_eq!(
        first,
        (SLEEPY | ANCHORLESS | CARRIED, home, LONG_PREAMBLE_SYMBOLS)
    );
}

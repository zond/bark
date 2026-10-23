//! A node takes part from its boot (spec "Channel at boot", "Heartbeat",
//! roadmap D73): it heartbeats from its first slot, its first rotation a
//! burst gap a heartbeat, takes and forwards traffic, and comes back with
//! what it stored before a reboot.

mod common;

use bark_proto::node::{burst_gap, heartbeat_lag};
use bark_proto::radio::{CHANNELS, MIN_PREAMBLE_SYMBOLS};
use bark_proto::sniff::LONG_PREAMBLE_SYMBOLS;
use bark_proto::wire::Packet;
use bark_proto::{AppCommand, AppEvent, Config, Instant, MsgId, Position, Relay, Trace};
use common::{at, secs, Harness};

/// A position `east` metres from a point in Kyiv.
fn pos(east: f64) -> Position {
    let (lat, lon) = (50.45, 30.52);
    let m_per_deg = 6_371_000.0 * core::f64::consts::PI / 180.0;
    Position::from_degrees(lat, lon + east / (m_per_deg * lat.to_radians().cos()))
}

fn config(east: f64) -> Config {
    Config {
        position: pos(east),
        ..Config::default()
    }
}

/// A node at `east`, and whether it is on external power.
fn node(east: f64, external: bool, seed: u64) -> (Relay, bool) {
    (Relay::new(config(east), seed), external)
}

/// Node `i`'s heartbeats from `from` on: when, the type, the channel and
/// the preamble.
fn heartbeats(h: &Harness<Relay>, i: usize, from: Instant) -> Vec<(Instant, u8, u8, u16)> {
    h.nodes[i]
        .sent
        .iter()
        .filter(|(t, f)| {
            *t >= from && matches!(Packet::decode(f.payload()), Ok(Packet::Heartbeat(_)))
        })
        .map(|(t, f)| {
            let kind = f.payload()[0] >> 4;
            (*t, kind, f.channel.index(), f.preamble_symbols)
        })
        .collect()
}

fn knows(h: &Harness<Relay>, a: usize, b: usize) -> bool {
    let id = h.nodes[b].core.id();
    h.nodes[a].core.neighbours().any(|n| n.id == id)
}

fn delivered(h: &Harness<Relay>, i: usize, id: MsgId) -> bool {
    h.nodes[i]
        .app
        .iter()
        .any(|(_, e)| matches!(e, AppEvent::Message { msg_id, .. } if *msg_id == id))
}

fn send(h: &mut Harness<Relay>, from: usize, to: usize, text: &[u8]) -> MsgId {
    let dst_pos = h.nodes[to].core.config().position;
    let dst_id = h.nodes[to].core.id();
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

/// A relay planted beside a running one heartbeats from its first slot:
/// its first rotation's thirteen heartbeats a burst gap apart with the
/// anchorless flag, 0xA, one on each channel, then on its interval. The
/// running relay answers the one on its channel once, with the ordinary
/// preamble, within a backoff and a lag, on the new relay's channel, and
/// each knows the other within the burst.
#[test]
fn a_relay_bursts_and_is_answered() {
    let (a, b) = (0, 1);
    let mut h = Harness::powered(vec![node(0.0, true, 1), node(300.0, true, 2)]);
    h.link(a, b);
    let planted = at(400);
    h.start_at(b, planted);
    h.run_until(planted + secs(60), |_| false);
    let c = Config::default();
    let beats = heartbeats(&h, b, planted);
    let burst: Vec<_> = beats.iter().take(CHANNELS as usize).collect();
    assert!(burst[0].0 < planted + c.heartbeat_max, "first slot");
    let channels: std::collections::HashSet<u8> = burst.iter().map(|b| b.2).collect();
    assert_eq!(channels.len(), CHANNELS as usize, "every channel once");
    for w in burst.windows(2) {
        assert_eq!(w[0].1, 0xA, "flagged");
        let gap = w[1].0.since(w[0].0);
        assert!(
            gap.as_micros().abs_diff(burst_gap(&c).as_micros()) <= 1000,
            "{gap:?}"
        );
    }
    let interval = bark_proto::node::heartbeat_interval(h.nodes[b].core.id(), &c);
    let after = beats[CHANNELS as usize]
        .0
        .since(beats[CHANNELS as usize - 1].0);
    assert!(after.as_micros().abs_diff(interval.as_micros()) <= 1000);
    assert_eq!(beats[CHANNELS as usize].1, 0x8, "the burst is over");
    // A's answer: one heartbeat on B's channel, out of A's slots.
    let home_b = h.nodes[b].core.home().index();
    let heard_at = burst
        .iter()
        .find(|x| x.2 == h.nodes[a].core.home().index())
        .unwrap()
        .0;
    let answers: Vec<_> = heartbeats(&h, a, planted)
        .into_iter()
        .filter(|x| x.2 == home_b && x.0 > heard_at && x.0 < heard_at + secs(2))
        .collect();
    assert_eq!(answers.len(), 1, "{answers:?}");
    assert_eq!(answers[0].3, MIN_PREAMBLE_SYMBOLS);
    assert!(answers[0].0 <= heard_at + c.lbt_backoff_max + heartbeat_lag(&c));
    let end = burst[CHANNELS as usize - 1].0 + secs(2);
    assert!(h.run_until(end, |h| knows(h, a, b) && knows(h, b, a)) || knows(&h, b, a));
    assert!(knows(&h, a, b) && knows(&h, b, a));
    // B's application hears how its boot goes.
    let relays: Vec<u8> = h.nodes[b]
        .boot
        .iter()
        .map(|(_, e)| match e {
            AppEvent::Boot { relays, .. } => *relays,
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(relays[..2], [0, 1]);
}

/// A relay's application may send from the boot. With no relay known yet
/// its text waits, and goes at the first relay heard, not dropped; it is
/// delivered within the burst.
#[test]
fn a_booting_relay_holds_its_text_until_it_hears_a_relay() {
    let (a, b) = (0, 1);
    let mut h = Harness::powered(vec![node(0.0, true, 3), node(300.0, true, 4)]);
    h.link(a, b);
    let planted = at(400);
    h.start_at(b, planted);
    h.run_until(planted, |_| false);
    h.run_until(planted + secs(1), |h| !h.nodes[b].boot.is_empty());
    assert!(h.nodes[b].core.booting());
    assert!(h.accepts(b), "it takes a text at boot");
    let text = send(&mut h, b, a, b"from the first second");
    assert!(h.run_until(planted + secs(40), |h| delivered(h, a, text)));
    let dropped = h.nodes[b]
        .traces
        .iter()
        .any(|(_, t)| matches!(t, Trace::Dropped { .. }));
    assert!(!dropped);
}

/// A sleepy node that hears no relay in its first rotation is anchorless
/// at its end, one worst-case rotation, and beats with the long preamble.
#[test]
fn a_sleepy_node_alone_is_anchorless_after_one_rotation() {
    let mut h = Harness::powered(vec![node(0.0, false, 5)]);
    let rotation = Config::default().boot_listen;
    h.run_until(Instant::ZERO + rotation - secs(1), |_| false);
    assert!(!h.nodes[0].core.anchorless());
    assert!(heartbeats(&h, 0, Instant::ZERO).is_empty(), "silent");
    h.run_until(Instant::ZERO + rotation + secs(60), |_| false);
    assert!(h.nodes[0].core.anchorless());
    let beats = heartbeats(&h, 0, Instant::ZERO);
    assert!(beats
        .iter()
        .all(|b| b.1 == 0xB && b.3 == LONG_PREAMBLE_SYMBOLS));
}

/// A relay restarted with what it stored comes back on its home channel
/// and stays there; its neighbours, which kept its entry, reach it at
/// once. Restarted without, it boots on a channel of its own.
#[test]
fn a_restarted_relay_comes_back_on_its_stored_home() {
    let (a, b) = (0, 1);
    let mut h = Harness::powered(vec![node(0.0, true, 6), node(300.0, true, 7)]);
    h.link(a, b);
    h.run_until(at(600), |_| false);
    let stored = h.nodes[b].stored.expect("kept");
    let home = h.nodes[b].core.home();
    assert_eq!(stored.home, home);
    assert_eq!(stored.place, h.nodes[b].core.config().position);
    let core = Relay::new(
        Config {
            stored: Some(stored),
            ..config(300.0)
        },
        70,
    );
    h.reboot(b, core);
    h.run_until(at(900), |_| false);
    assert_eq!(h.nodes[b].core.home(), home);
    let moved = h.nodes[b]
        .traces
        .iter()
        .any(|(t, tr)| *t >= at(600) && matches!(tr, Trace::HomeChannelMoved { .. }));
    assert!(!moved, "no choice at the end of its first rotation");
    // Its text to A goes at once: A is heard within the burst.
    let text = send(&mut h, b, a, b"back");
    assert!(h.run_until(h.now + secs(5), |h| delivered(h, a, text)));
}

/// A sleepy node restarted with its stored anchor takes it at once and
/// beats to it on its channel, without a first rotation's listen, and
/// keeps it once it hears its heartbeat.
#[test]
fn a_sleepy_node_resumes_its_stored_anchor() {
    let (r, s) = (0, 1);
    let mut h = Harness::powered(vec![node(0.0, true, 8), node(50.0, false, 9)]);
    h.link(r, s);
    h.run_until(at(1000), |_| false);
    let anchor = h.nodes[s].core.anchor().expect("anchored");
    let stored = h.nodes[s].stored.expect("kept");
    assert_eq!(stored.anchor, Some(anchor));
    let back = h.now;
    h.reboot(
        s,
        Relay::new(
            Config {
                stored: Some(stored),
                ..config(50.0)
            },
            90,
        ),
    );
    h.run_until(back + secs(1), |_| false);
    assert_eq!(h.nodes[s].core.anchor(), Some(anchor));
    assert!(!h.nodes[s].core.booting());
    let beats = heartbeats(&h, s, back);
    assert_eq!(beats[0].1, 0x9, "a beat to its anchor at once");
    assert_eq!(beats[0].2, h.nodes[r].core.home().index());
    h.run_until(back + Config::default().boot_listen + secs(10), |_| false);
    assert_eq!(h.nodes[s].core.anchor(), Some(anchor));
}

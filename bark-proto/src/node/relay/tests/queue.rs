//! Where the send queue's entries are counted and how the next is chosen
//! (spec "Send queue"): the gate counts each entry where it was last
//! routed.

use super::*;
use crate::tables::records::Record;
use crate::tables::send_queue::{Message, State};

/// A running relay that sends no heartbeat of its own for an hour.
fn quiet_relay() -> (Relay, Instant) {
    let hour = Duration::from_secs(3600);
    let config = Config {
        boot_listen: Duration::from_secs(1),
        home_override: Channel::new(3),
        sweep_interval: hour,
        heartbeat_min: hour,
        heartbeat_max: hour,
        ..Config::default()
    };
    let mut r = Relay::new(config, 1);
    let mut now = Instant::ZERO;
    drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(2));
    (r, now)
}

/// `r` hears the neighbour `id` at `at`.
fn heard_at(r: &mut Relay, id: NodeId, at: Position, now: Instant) {
    let hb = Heartbeat {
        sleepy: false,
        anchorless: false,
        carried: false,
        home: Channel::new(5).unwrap(),
        format: crate::wire::FORMAT,
        map_point: false,
        leaving: false,
        id,
        short: at.short(),
    };
    r.neighbours.heard(&hb, at, -80, &r.plane, now);
}

/// `r`'s position moved by `lat` and `lon`.
fn off(r: &Relay, lat: i32, lon: i32) -> Position {
    let me = r.config.position;
    Position {
        lat: me.lat + lat,
        lon: me.lon + lon,
    }
}

/// Queue a forward to `dst` received with `hops`, as an arrival queues
/// it: its record, then its entry, routed now. Its message's ID.
fn queue(r: &mut Relay, now: Instant, dst: Position, tag: u8, hops: u32) -> MsgId {
    let m = Message::new(dst, NOBODY, &[tag]);
    let parent = Some((NodeId::from_u64(7), Channel::new(6).unwrap()));
    let at = r.record_place().expect("a record's place");
    r.records
        .put(at, Record::new(m.id(), parent, (hops, 0), now));
    let out = r.onward(m, (hops, 0));
    assert!(r.push_forward(out, 0, now));
    m.id()
}

/// Where entry `i` was last routed.
fn toward(r: &Relay, i: usize) -> Option<NodeId> {
    let o = r.queue.get(i)?;
    let slot = usize::from(o.toward.checked_sub(1)?);
    r.neighbours.slot(slot).map(|n| n.id)
}

/// Neighbours B, north, and C, north-east and less far north, a
/// destination far north, so that B is every message's way on, and a
/// service time of a ban each.
fn two_ways() -> (Relay, Instant, NodeId, NodeId, Position) {
    let (mut r, now) = quiet_relay();
    let (b, c) = (NodeId::from_u64(11), NodeId::from_u64(12));
    let (at_b, at_c) = (off(&r, 10_000, 0), off(&r, 4_000, 4_000));
    heard_at(&mut r, b, at_b, now);
    heard_at(&mut r, c, at_c, now);
    for id in [b, c] {
        r.neighbours.set_service(id, BAN);
    }
    let far = off(&r, 100_000, 0);
    (r, now, b, c, far)
}

/// How many entries the gate counts for `to`, of a service time each.
fn ahead(r: &Relay, to: NodeId, now: Instant) -> u64 {
    ahead_of(r.gate_for(MsgId(0), to, 1, now).0)
}

/// The service times in the gate's `wait`.
fn ahead_of(wait: Duration) -> u64 {
    // A service time is kept in whole units, a little under a ban.
    (wait.as_micros() + BAN.as_micros() / 2) / BAN.as_micros()
}

/// The gate counts each entry where it was routed when it was queued,
/// not where routing would send it now: until it is routed again, the
/// stored way stands.
#[test]
fn the_gate_counts_by_the_route_kept() {
    let (mut r, now, b, c, far) = two_ways();
    queue(&mut r, now, far, 1, 2);
    queue(&mut r, now, far, 2, 2);
    assert_eq!((toward(&r, 0), toward(&r, 1)), (Some(b), Some(b)));
    assert_eq!((ahead(&r, b, now), ahead(&r, c, now)), (2, 0));
    // B moves south: C is now the way on, but both still count at B.
    let south = off(&r, -10_000, 0);
    heard_at(&mut r, b, south, now);
    assert_eq!((ahead(&r, b, now), ahead(&r, c, now)), (2, 0), "stale");
    // Choosing what to send routes only the first again, which may go.
    assert_eq!(r.pick(now), Some(0));
    assert_eq!((toward(&r, 0), toward(&r, 1)), (Some(c), Some(b)));
    assert_eq!((ahead(&r, b, now), ahead(&r, c, now)), (1, 1));
}

/// One set aside, a delivery, and the message the gate is asked for do
/// not count; one in flight counts where it goes.
#[test]
fn the_gate_counts_only_what_goes_on() {
    use crate::tables::send_queue::{Kind, Wait};
    let (mut r, now, b, _, far) = two_ways();
    let ids: Vec<_> = (1..=4).map(|tag| queue(&mut r, now, far, tag, 2)).collect();
    assert_eq!(ahead(&r, b, now), 4);
    r.queue.get_mut(0).unwrap().state = State::Held;
    r.queue.get_mut(1).unwrap().kind = Kind::Direct;
    assert_eq!(ahead(&r, b, now), 2);
    let (wait, _) = r.gate_for(ids[2], b, 1, now);
    assert_eq!(ahead_of(wait), 1, "not itself");
    // In flight, an entry counts by its receiver.
    let o = r.queue.get_mut(3).unwrap();
    o.state = State::Waiting(Wait::Turn);
    o.to = NodeId::from_u64(12);
    o.toward = 0;
    assert_eq!(ahead(&r, b, now), 1);
    assert_eq!(ahead(&r, NodeId::from_u64(12), now), 1);
}

/// A slot that has a new occupant drops every route kept to it: the
/// newcomer has none of the messages that were waiting for the old one.
#[test]
fn a_reoccupied_slot_ends_the_routes_kept_to_it() {
    let (mut r, now, b, c, far) = two_ways();
    queue(&mut r, now, far, 1, 2);
    let slot = r.neighbours.slot_of(b).unwrap();
    r.queue.slot_reoccupied(slot);
    assert_eq!(toward(&r, 0), None);
    assert_eq!((ahead(&r, b, now), ahead(&r, c, now)), (0, 0));
    // Routed again, it counts where it goes.
    assert_eq!(r.pick(now), Some(0));
    assert_eq!(ahead(&r, b, now), 1);
}

/// A message this relay owns when it is queued goes to no neighbour,
/// and counts nowhere.
#[test]
fn a_message_that_ends_here_counts_nowhere() {
    let (mut r, now, b, c, _) = two_ways();
    let here = r.config.position;
    queue(&mut r, now, here, 1, 2);
    assert_eq!(toward(&r, 0), None);
    assert_eq!((ahead(&r, b, now), ahead(&r, c, now)), (0, 0));
}

/// With nothing banned the first entry considered goes, as it did when
/// every entry was routed first: the most travelled that has waited,
/// else the first queued. Only the one chosen is routed again.
#[test]
fn with_nothing_banned_the_order_is_the_queue_s() {
    let (mut r, now, b, c, far) = two_ways();
    for (tag, hops) in [(1, 1), (2, 3), (3, 5), (4, 5), (5, 2)] {
        queue(&mut r, now, far, tag, hops);
    }
    for i in [1, 2, 3] {
        r.queue.get_mut(i).unwrap().waited = true;
    }
    // B moves south: every way kept is stale.
    let south = off(&r, -10_000, 0);
    heard_at(&mut r, b, south, now);
    let mut order = Vec::new();
    while let Some(i) = r.pick(now) {
        assert_eq!(toward(&r, i), Some(c), "routed afresh");
        let gone = r.queue.remove(i).unwrap();
        order.push(gone.message.payload()[0]);
        let rest = (0..r.queue.len()).map(|k| toward(&r, k));
        assert!(rest.into_iter().all(|t| t == Some(b)), "not routed");
    }
    assert_eq!(order, [3, 4, 2, 1, 5]);
}

/// A first candidate whose way is banned is passed over and marked as
/// having waited; the next goes.
#[test]
fn a_banned_candidate_is_passed_and_marked() {
    let (mut r, now, b, c, far) = two_ways();
    queue(&mut r, now, far, 1, 2);
    let east = off(&r, 4_000, 100_000);
    queue(&mut r, now, east, 2, 2);
    assert_eq!((toward(&r, 0), toward(&r, 1)), (Some(b), Some(c)));
    r.neighbours.refused(b, now + BAN);
    assert_eq!(r.pick(now), Some(1));
    assert!(r.queue.get(0).unwrap().waited);
    assert!(!r.queue.get(1).unwrap().waited);
    assert_eq!(r.forwarding.wake, None, "one may go");
}

/// When nothing may go, every due entry has been routed and the wakeup
/// is the earliest end of a ban or a wait of all of them.
#[test]
fn with_everything_banned_the_wakeup_is_the_first_end() {
    let (mut r, now, b, c, far) = two_ways();
    queue(&mut r, now, far, 1, 2);
    let east = off(&r, 4_000, 100_000);
    queue(&mut r, now, east, 2, 2);
    queue(&mut r, now, far, 3, 2);
    let s = Duration::from_secs;
    r.queue.get_mut(2).unwrap().not_before = now + s(9);
    r.neighbours.refused(b, now + s(5));
    r.neighbours.refused(c, now + s(7));
    assert_eq!(r.pick(now), None);
    assert_eq!(r.forwarding.wake, Some(now + s(5)));
    assert!(r.queue.iter().all(|o| o.waited()));
    r.neighbours.refused(b, now + s(10));
    assert_eq!(r.pick(now), None);
    assert_eq!(r.forwarding.wake, Some(now + s(7)));
    r.neighbours.refused(c, now + s(10));
    assert_eq!(r.pick(now), None);
    assert_eq!(r.forwarding.wake, Some(now + s(9)), "its own wait");
}

/// The message chosen at `now`.
fn chosen(r: &mut Relay, now: Instant) -> Option<MsgId> {
    let i = r.pick(now)?;
    r.queue.get(i).map(|o| o.msg_id())
}

/// A message that waits for a ban behind others that go keeps the turn
/// the ban gives it, though it is not routed while it waits: once the ban
/// ends it goes ahead of an older message that has not waited
/// (spec "Overload waits").
#[test]
fn a_message_waiting_behind_others_keeps_its_turn() {
    let (mut r, mut now, b, _, far) = two_ways();
    let east = off(&r, 4_000, 100_000);
    queue(&mut r, now, east, 1, 1);
    queue(&mut r, now, east, 2, 1);
    let a = queue(&mut r, now, far, 3, 1);
    assert_eq!(toward(&r, 2), Some(b));
    let ends = now + BAN;
    r.neighbours.refused(b, ends);
    // The first east goes; A, behind it, is not routed, but its way kept
    // is banned.
    assert_eq!(r.pick(now), Some(0));
    assert!(r.queue.get(2).unwrap().waited);
    r.queue.remove(0);
    now = ends;
    assert_eq!(chosen(&mut r, now), Some(a));
}

/// One that waits for a ban is not left behind while others go: each
/// pick sends one of them, and once the ban ends it goes first, however
/// many keep coming for the open way.
#[test]
fn a_message_waiting_for_a_ban_does_not_starve() {
    let (mut r, mut now, b, _, far) = two_ways();
    let east = off(&r, 4_000, 100_000);
    let a = queue(&mut r, now, far, 0, 1);
    let ends = now + BAN.mul(4);
    r.neighbours.refused(b, ends);
    let mut tag = 1;
    while now < ends {
        queue(&mut r, now, east, tag, 1);
        tag += 1;
        let i = r.pick(now).expect("one for the open way");
        assert_ne!(r.queue.get(i).map(|o| o.msg_id()), Some(a));
        r.queue.remove(i);
        now += BAN.div(2);
    }
    queue(&mut r, now, east, tag, 1);
    assert_eq!(chosen(&mut r, now), Some(a));
}

/// A message whose way kept is banned but that routing now sends to an
/// open way has not waited: it is routed again, once, and keeps its
/// place.
#[test]
fn a_stale_way_that_is_banned_marks_nothing() {
    let (mut r, now, b, c, far) = two_ways();
    let east = off(&r, 4_000, 100_000);
    queue(&mut r, now, east, 1, 1);
    queue(&mut r, now, far, 2, 1);
    let south = off(&r, -10_000, 0);
    heard_at(&mut r, b, south, now);
    r.neighbours.refused(b, now + BAN);
    assert_eq!(r.pick(now), Some(0));
    assert!(!r.queue.get(1).unwrap().waited);
    assert_eq!(toward(&r, 1), Some(c), "routed again");
}

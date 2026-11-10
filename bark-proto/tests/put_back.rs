//! What a node holds at its reboot into a new image is put back where it
//! was, and the node carries on with it as if it had not been away (spec
//! "Updates over the mesh", the install; roadmap D199): a forward keeps
//! its count and its parent, the application's own message stays its own,
//! and a copy from the parent after the reboot is known by its record.

mod common;

use bark_proto::wire::Packet;
use bark_proto::{AppCommand, AppEvent, Config, HeldAt, MsgId, NodeId, Position, Relay, Trace};
use common::{secs, swap, Harness};

/// A position `east` metres from a point in Kyiv.
fn pos(east: f64) -> Position {
    let (lat, lon) = (50.45, 30.52);
    let m_per_deg = 6_371_000.0 * core::f64::consts::PI / 180.0;
    Position::from_degrees(lat, lon + east / (m_per_deg * lat.to_radians().cos()))
}

fn relay(east: f64, seed: u64) -> Relay {
    Relay::new(
        Config {
            position: pos(east),
            boot_listen: secs(5),
            ..Config::default()
        },
        seed,
    )
}

/// P, M and D in a line, 300 m apart, each hearing the next.
fn line() -> Harness<Relay> {
    let mut h = Harness::new(vec![relay(0.0, 1), relay(300.0, 2), relay(600.0, 3)]);
    h.link(0, 1);
    h.link(1, 2);
    let knows = |h: &Harness<Relay>, a: usize, b: usize| {
        let id = h.nodes[b].core.id();
        h.nodes[a].core.neighbours().any(|n| n.id == id)
    };
    let found = h.run_until(h.now + secs(600), |h| {
        [(0, 1), (1, 0), (1, 2), (2, 1)]
            .iter()
            .all(|&(a, b)| knows(h, a, b))
    });
    assert!(found, "neighbours not discovered");
    h
}

/// P sends a message to `dst`; run until M holds it.
fn sent_to_m(h: &mut Harness<Relay>, dst: (Position, NodeId)) -> MsgId {
    let payload = b"across a reboot";
    h.command(
        0,
        AppCommand::Send {
            dst_pos: dst.0,
            dst_id: dst.1,
            payload,
        },
    );
    let id = MsgId::of(dst.0, dst.1, payload);
    let held = |h: &Harness<Relay>| h.nodes[1].core.holding().any(|x| x == id);
    assert!(h.run_until(h.now + secs(30), held), "M never held it");
    id
}

fn traced(h: &Harness<Relay>, node: usize, f: impl Fn(&Trace) -> bool) -> usize {
    h.nodes[node].traces.iter().filter(|(_, t)| f(t)).count()
}

/// M holds a forward from P when it reboots. Back, it hands it to D with
/// the next count it would have sent, 2, not 1 as for a message of its
/// own; D, where the search runs out, returns it, and M returns it to P,
/// its parent, as it would have without the reboot, rather than end the
/// search itself.
#[test]
fn a_forward_carried_across_a_reboot_keeps_its_count_and_its_parent() {
    let mut h = line();
    let (p, m, d) = (0, 1, 2);
    // Beyond D, where nobody is: every search ends.
    let id = sent_to_m(&mut h, (pos(3_000.0), NodeId([9; 6])));
    let own = swap(&mut h, m, relay(300.0, 2), secs(59));
    assert!(own.is_empty(), "not the application's");
    let (pid, did) = (h.nodes[p].core.id(), h.nodes[d].core.id());
    let forwarded = |h: &Harness<Relay>| {
        traced(
            h,
            m,
            |t| matches!(t, Trace::Forwarded { msg_id, to, hops: 2, .. } if *msg_id == id && *to == did),
        ) == 1
    };
    assert!(
        h.run_until(h.now + secs(60), forwarded),
        "not handed to D with count 2"
    );
    let returned = |h: &Harness<Relay>| {
        traced(
            h,
            m,
            |t| matches!(t, Trace::ReturnedToParent { msg_id, to, .. } if *msg_id == id && *to == pid),
        ) == 1
    };
    assert!(h.run_until(h.now + secs(60), returned), "not returned to P");
    let ended_at_m = traced(
        &h,
        m,
        |t| matches!(t, Trace::Dropped { msg_id, .. } if *msg_id == id),
    );
    assert_eq!(ended_at_m, 0, "its search ended at M");
}

/// The application's own message, not yet taken by a first hop when the
/// node reboots, is not put back: the application offers it again, and
/// it is its own, the node taking no other until it has gone, and it
/// arrives.
#[test]
fn the_applications_own_message_stays_its_own() {
    let mut h = line();
    let (s, m) = (1, 2);
    let dst = (h.nodes[m].core.config().position, h.nodes[m].core.id());
    let payload = b"my own";
    h.command(
        s,
        AppCommand::Send {
            dst_pos: dst.0,
            dst_id: dst.1,
            payload,
        },
    );
    let id = MsgId::of(dst.0, dst.1, payload);
    let own = swap(&mut h, s, relay(300.0, 2), secs(59));
    assert_eq!(own.len(), 1);
    assert_eq!((own[0].msg_id, own[0].at), (id, HeldAt::Own));
    assert!(!h.nodes[s].core.knows(id), "its record was kept");
    assert!(h.run_until(h.now + secs(30), |h| h.accepts(s)));
    h.command(
        s,
        AppCommand::Send {
            dst_pos: own[0].dst_pos,
            dst_id: own[0].dst_id,
            payload: &own[0].payload,
        },
    );
    assert!(h.nodes[s].core.knows(id));
    assert!(!h.accepts(s), "took another while its own waits");
    let arrived = |h: &Harness<Relay>| {
        h.nodes[m]
            .app
            .iter()
            .any(|(_, e)| *e == AppEvent::Message { msg_id: id })
    };
    assert!(h.run_until(h.now + secs(60), arrived), "never arrived");
}

/// M takes a message from P, but its ACK is lost, and M reboots. P sends
/// the same packet again; M, back, knows it by the record it kept,
/// answers it, and the message goes on once: D gets one copy, and nothing
/// is dropped.
#[test]
fn the_same_message_again_from_the_parent_is_not_taken_twice() {
    let mut h = line();
    let (p, m, d) = (0, 1, 2);
    let (pid, mid) = (h.nodes[p].core.id(), h.nodes[m].core.id());
    let mut lost = false;
    h.lose = Some(Box::new(move |_, from, to, bytes| {
        let ack =
            matches!(Packet::decode(bytes), Ok(Packet::Ack(a)) if a.from == mid && a.to == pid);
        let lose = ack && from == 1 && to == 0 && !lost;
        lost |= lose;
        lose
    }));
    let dst = (h.nodes[d].core.config().position, h.nodes[d].core.id());
    let id = sent_to_m(&mut h, dst);
    swap(&mut h, m, relay(300.0, 2), secs(2));
    let arrived = |h: &Harness<Relay>| {
        h.nodes[d]
            .app
            .iter()
            .filter(|(_, e)| *e == AppEvent::Message { msg_id: id })
            .count()
    };
    assert!(
        h.run_until(h.now + secs(60), |h| arrived(h) == 1),
        "never arrived"
    );
    h.run_until(h.now + secs(120), |_| false);
    assert_eq!(arrived(&h), 1, "arrived twice");
    let copies = traced(
        &h,
        m,
        |t| matches!(t, Trace::Forwarded { msg_id, .. } if *msg_id == id),
    );
    assert_eq!(copies, 1, "M sent it on twice");
    let repeated = h.nodes[p].sent.iter().filter(
        |(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Data(x)) if x.msg_id() == id),
    );
    assert!(repeated.count() >= 2, "P never sent it again");
    for n in &h.nodes {
        let dropped = n
            .traces
            .iter()
            .any(|(_, t)| matches!(t, Trace::Dropped { msg_id, .. } if *msg_id == id));
        assert!(!dropped, "dropped");
    }
}

//! The node driven on the host: the sizes, the first rotation, the
//! sleepy side, admission and the hand-over.

extern crate std;
use super::*;
use crate::event::{Frame, Trace};
use crate::node::mac;
use crate::sniff::LONG_PREAMBLE_SYMBOLS;
use crate::tables::budget::BAN;
use crate::tables::send_queue::HOP_WAIT;
use crate::wire::Heartbeat;
use std::vec::Vec;

/// The ID a test gives the relay heard at `p`.
fn id_of(p: Position) -> NodeId {
    NodeId::from_u64(p.short().0 as u64)
}

/// The ID numbered `n`.
fn name(n: u32) -> NodeId {
    NodeId::from_u64(n as u64)
}

/// The ID of a node no test builds.
const NOBODY: NodeId = NodeId([9; 6]);

#[test]
fn sizes() {
    // docs/core-interface.md §6: the tables, and around them the medium
    // access, the hand-over, one owed reply and one held message in
    // under 1 KiB, and what the node keeps across a reboot, handed
    // back and last kept.
    let relay = core::mem::size_of::<Relay>();
    let tables = crate::tables::sizes::tables();
    let kept = 2 * core::mem::size_of::<Option<Stored>>();
    // What it keeps of the images spreading
    // (spec "Updates over the mesh"): small beside the tables, most of it
    // a bit for each piece of each of the 48 blocks an image can have.
    let spread = core::mem::size_of::<update::Spread>();
    std::println!("Relay {relay} B, tables {tables} B, kept {kept} B, spread {spread} B");
    assert!(spread <= 2048, "{spread}");
    assert!(relay <= tables + 1024 + kept + spread, "{relay}");
    // It fits the board's RAM: 255 KiB for the application
    // (bark-image/src/layout.rs), less the firmware's other static memory,
    // the BLE stack, Embassy and the USB log, 29 896 B as measured
    // (docs/phone-link-2026-11-05.md, "Does it fit"), and the call
    // stack, estimated at 16 KiB (spec "Implementation notes").
    let ram = 255 * 1024 - 29_896 - 16 * 1024;
    assert!(relay <= ram, "{relay} of {ram}");
    // A node built only to be sleepy spends nothing on the heard table
    // and the delivery queue (spec "Sleepy nodes").
    let sleepy = core::mem::size_of::<SleepyNode>();
    let mailbox = core::mem::size_of::<Heard>() + core::mem::size_of::<DeliveryQueue>();
    std::println!("SleepyNode {sleepy} B, mailbox {mailbox} B");
    assert!(
        sleepy + mailbox <= relay + 64,
        "{sleepy} + {mailbox} of {relay}"
    );
}

/// A sleepy node built without the mailbox does what a full one set
/// up sleepy does.
#[test]
fn a_sleepy_node_without_the_mailbox_is_a_sleepy_relay() {
    let config = Config {
        boot_listen: Duration::from_secs(1),
        ..Config::default()
    };
    let mut full = Relay::new(config, 5);
    let mut bare = SleepyNode::new(config, 5);
    let p_full = on_battery(&mut full);
    let p_bare = on_battery(&mut bare);
    let until = Instant::ZERO + Duration::from_secs(900);
    let (mut t_full, mut t_bare) = (Instant::ZERO, Instant::ZERO);
    let sent_full = drive_from(&mut full, &mut t_full, until, p_full);
    let sent_bare = drive_from(&mut bare, &mut t_bare, until, p_bare);
    assert!(sent_full.len() > 5);
    assert_eq!(sent_full, sent_bare);
}

/// A sleepy node fills its neighbour table from the beats it hears,
/// marked sleepy, and keeps no heard table (spec "Neighbour table",
/// "Heard table").
#[test]
fn a_sleepy_node_keeps_no_heard_table() {
    let config = Config {
        boot_listen: Duration::from_secs(1),
        ..Config::default()
    };
    let mut r = Relay::new(config, 6);
    let pending = on_battery(&mut r);
    let mut now = Instant::ZERO;
    drive_from(
        &mut r,
        &mut now,
        Instant::ZERO + Duration::from_secs(5),
        pending,
    );
    let other = name(77);
    let beat = Heartbeat {
        sleepy: true,
        anchorless: false,
        carried: false,
        home: r.home(),
        format: crate::wire::FORMAT,
        map_point: false,
        id: other,
        short: config.position.short(),
    };
    let mut buf = [0; crate::event::MAX_FRAME];
    let len = Packet::Heartbeat(beat).encode(&mut buf).unwrap();
    let event = Event::Received {
        channel: r.home(),
        rssi_dbm: -80,
        snr_db: 5,
        bytes: &buf[..len],
    };
    r.handle(event, now, &mut |_| {});
    assert!(r.heard_nodes().is_empty());
    let n = r.neighbours().find(|n| n.id == other).copied();
    assert!(n.is_some_and(|n| n.sleepy && n.last_resort()), "{n:?}");
}

/// The application is told once, at the first frame from another node
/// that decodes, and not for bytes that do not decode
/// (spec "Updates over the mesh", the install).
#[test]
fn the_first_frame_heard_is_told_once() {
    // How long a gathered image may wait for it: three worst-case
    // rotations and a heartbeat's lateness (spec "Heartbeat").
    assert_eq!(
        crate::update::heard_within(&Config::default()),
        Duration::from_micros(3 * 13 * 15_000_000 + 872_800)
    );
    let config = Config {
        boot_listen: Duration::from_secs(1),
        ..Config::default()
    };
    let mut r = Relay::new(config, 7);
    let mut now = Instant::ZERO;
    drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(5));
    let heard = |r: &mut Relay, now: Instant, bytes: &[u8]| {
        let event = Event::Received {
            channel: r.home(),
            rssi_dbm: -80,
            snr_db: 5,
            bytes,
        };
        let mut told = 0;
        r.handle(event, now, &mut |a| {
            told += usize::from(matches!(a, Action::App(crate::AppEvent::Heard)))
        });
        told
    };
    // A frame the codec refuses, as a bench frame is (`wire::BENCH_FIRST`).
    assert_eq!(heard(&mut r, now, &[0x8f, 1, 0, 0]), 0);
    let (home, own) = (r.home(), r.id);
    let beat = |id| Heartbeat {
        sleepy: false,
        anchorless: false,
        carried: false,
        home,
        format: crate::wire::FORMAT,
        map_point: false,
        id,
        short: config.position.short(),
    };
    let mut buf = [0; crate::event::MAX_FRAME];
    // Its own ID, as no radio hears its own frame.
    let len = Packet::Heartbeat(beat(own)).encode(&mut buf).unwrap();
    assert_eq!(heard(&mut r, now, &buf[..len]), 0);
    let len = Packet::Heartbeat(beat(name(78))).encode(&mut buf).unwrap();
    assert_eq!(heard(&mut r, now, &buf[..len]), 1);
    let len = Packet::Heartbeat(beat(name(79))).encode(&mut buf).unwrap();
    assert_eq!(heard(&mut r, now, &buf[..len]), 0);
    // A reset is a boot: told again.
    r.reset(config, 7);
    let mut now = Instant::ZERO;
    drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(5));
    assert_eq!(heard(&mut r, now, &buf[..len]), 1);
}

/// A sleepy node holds mail for nobody: a message for another node at
/// its own position is not its to deliver, and waits to go on (spec
/// "Sleepy nodes", "Owner").
#[test]
fn a_sleepy_node_owns_no_other_node_s_message() {
    let config = Config {
        boot_listen: Duration::from_secs(1),
        ..Config::default()
    };
    let mut r = Relay::new(config, 7);
    let pending = on_battery(&mut r);
    let mut now = Instant::ZERO;
    drive_from(
        &mut r,
        &mut now,
        Instant::ZERO + Duration::from_secs(5),
        pending,
    );
    assert!(r.anchorless());
    let send = AppCommand::Send {
        dst_pos: config.position,
        dst_id: name(78),
        payload: b"for another",
    };
    r.handle(Event::App(send), now, &mut |_| {});
    assert_eq!(r.holding().count(), 1);
    assert_eq!(r.load(now).delivery_queue, 0);
}

/// Boot `r` on battery, as its driver reports at boot
/// (`docs/core-interface.md` §4): sleepy. Returns the excursions it
/// asked for, for [`drive_from`].
fn on_battery<const H: usize, const Q: usize>(r: &mut Node<H, Q>) -> Vec<Action> {
    let mut pending = Vec::new();
    let mut keep = |a: Action| {
        if matches!(a, Action::Lbt(_) | Action::Rssi(_) | Action::Transmit(_)) {
            pending.push(a);
        }
    };
    r.handle(Event::Wakeup, Instant::ZERO, &mut keep);
    let power = AppCommand::Power { external: false };
    r.handle(Event::App(power), Instant::ZERO, &mut keep);
    assert!(r.sleepy());
    pending
}

/// Drive `r` from its own wakeups until `until`, completing every radio
/// action at once on a quiet channel. Returns the frames it sent.
fn drive<const H: usize, const Q: usize>(
    r: &mut Node<H, Q>,
    now: &mut Instant,
    until: Instant,
) -> Vec<(Instant, Frame)> {
    drive_from(r, now, until, Vec::new())
}

/// [`drive`], with `pending` the actions `r` has asked for already.
fn drive_from<const H: usize, const Q: usize>(
    r: &mut Node<H, Q>,
    now: &mut Instant,
    until: Instant,
    pending: Vec<Action>,
) -> Vec<(Instant, Frame)> {
    let mut sent = Vec::new();
    drive_all(r, now, until, pending, &mut |t, a| {
        if let Action::Transmit(f) = a {
            sent.push((t, f));
        }
    });
    sent
}

/// [`drive_from`], returning the traces instead of the frames.
fn drive_traces<const H: usize, const Q: usize>(
    r: &mut Node<H, Q>,
    now: &mut Instant,
    until: Instant,
    pending: Vec<Action>,
) -> Vec<Trace> {
    let mut traces = Vec::new();
    drive_all(r, now, until, pending, &mut |_, a| {
        if let Action::Trace(t) = a {
            traces.push(t);
        }
    });
    traces
}

/// Drive `r` as [`drive_from`] does, handing every action with its time
/// to `seen`.
fn drive_all<const H: usize, const Q: usize>(
    r: &mut Node<H, Q>,
    now: &mut Instant,
    until: Instant,
    mut pending: Vec<Action>,
    seen: &mut dyn FnMut(Instant, Action),
) {
    loop {
        let event = match pending.pop() {
            Some(Action::Lbt(channel)) => Event::LbtDone {
                channel,
                busy: false,
                dbm: -130,
            },
            Some(Action::Rssi(channel)) => Event::RssiSample { channel, dbm: -130 },
            Some(Action::Transmit(f)) => {
                seen(*now, Action::Transmit(f));
                *now += mac::airtime(&f);
                Event::TxDone
            }
            Some(_) => continue,
            None => match r.next_wakeup() {
                Some(t) if t <= until => {
                    *now = (*now).max(t);
                    Event::Wakeup
                }
                _ => return,
            },
        };
        let t = *now;
        r.handle(event, *now, &mut |a| match a {
            Action::Lbt(_) | Action::Rssi(_) | Action::Transmit(_) => pending.push(a),
            a => seen(t, a),
        });
    }
}

/// A sleepy node whose anchor is not in its table when the rotation of
/// its first choice ends makes the choice at its time and does not
/// wake at once again. Before D58 a sleepy node beating with the
/// anchor's ID removed the entry; now the entry is pinned, so the test
/// forgets it by hand to keep the invariant: a timer that is due is
/// cleared.
#[test]
fn the_first_choice_ends_without_the_anchor() {
    use crate::wire::Heartbeat;
    let config = Config {
        boot_listen: Duration::from_secs(10),
        ..Config::default()
    };
    let mut s = Relay::new(config, 5);
    let at = |ms: u64| Instant::ZERO + Duration::from_millis(ms);
    on_battery(&mut s);
    let anchor = name(77);
    let beat = || {
        let hb = Heartbeat {
            sleepy: false,
            anchorless: false,
            carried: false,
            home: Channel::new(4).unwrap(),
            format: crate::wire::FORMAT,
            map_point: false,
            id: anchor,
            short: config.position.short(),
        };
        let mut bytes = [0u8; crate::event::MAX_FRAME];
        let n = Packet::Heartbeat(hb).encode(&mut bytes).unwrap();
        (bytes, n)
    };
    let hear = |s: &mut Relay, t: Instant| {
        let (bytes, n) = beat();
        let ev = Event::Received {
            channel: Channel::new(4).unwrap(),
            rssi_dbm: -90,
            snr_db: 0,
            bytes: &bytes[..n],
        };
        s.handle(ev, t, &mut |_| {});
    };
    hear(&mut s, at(1000));
    assert_eq!(s.anchor().map(|a| a.id), Some(anchor));
    s.neighbours.pin(None);
    s.neighbours.forget(anchor);
    assert!(s.neighbours().all(|n| n.id != anchor));
    // Run it, completing its radio actions at once, for as many
    // events as a minute takes it; stuck, it would spend them all at
    // the end of the choice.
    let mut now = at(2000);
    let mut pending: Vec<Action> = Vec::new();
    for _ in 0..10_000 {
        let event = match pending.pop() {
            Some(Action::Lbt(channel)) => Event::LbtDone {
                channel,
                busy: false,
                dbm: -130,
            },
            Some(Action::Rssi(channel)) => Event::RssiSample { channel, dbm: -130 },
            Some(Action::Transmit(f)) => {
                now += mac::airtime(&f);
                Event::TxDone
            }
            Some(_) => continue,
            None => match s.next_wakeup() {
                Some(t) => {
                    now = now.max(t);
                    Event::Wakeup
                }
                None => break,
            },
        };
        if now > at(60_000) {
            break;
        }
        s.handle(event, now, &mut |a| match a {
            Action::Lbt(_) | Action::Rssi(_) | Action::Transmit(_) => pending.push(a),
            _ => {}
        });
    }
    assert!(now > at(60_000), "stuck at {now:?}");
}

/// A relay reset in place, after it has run and holds messages, does
/// what a new one does.
#[test]
fn a_reset_relay_is_a_new_one() {
    use crate::ids::NodeId;
    let config = Config {
        boot_listen: Duration::from_secs(1),
        ..Config::default()
    };
    let mut used = Relay::new(config, 1);
    let mut now = Instant::ZERO;
    drive(&mut used, &mut now, Instant::ZERO + Duration::from_secs(30));
    let send = AppCommand::Send {
        dst_pos: config.position,
        dst_id: NodeId([1; 6]),
        payload: b"queued",
    };
    used.handle(Event::App(send), now, &mut |_| {});
    assert_eq!(used.holding().count(), 1);

    let other = Config {
        position: Position::from_degrees(50.45, 30.52),
        ..config
    };
    used.reset(other, 2);
    let mut fresh = Relay::new(other, 2);
    assert_eq!(used.holding().count(), 0);
    let until = Instant::ZERO + Duration::from_secs(60);
    let (mut t_used, mut t_fresh) = (Instant::ZERO, Instant::ZERO);
    let sent_used = drive(&mut used, &mut t_used, until);
    let sent_fresh = drive(&mut fresh, &mut t_fresh, until);
    assert!(!sent_fresh.is_empty());
    assert_eq!(sent_used, sent_fresh);
    assert_eq!(used.home(), fresh.home());
}

/// A node whose bucket is nearly spent holds one message for a sleepy
/// node that has just beaten, and nothing else happens: it wakes when
/// its bucket will fit the DATA with the reply reserve, within the
/// node's window, and sends it then. The node never answers, and its message
/// waits for its next beat (`relay/delivery.rs`).
#[test]
fn a_delivery_waits_for_the_bucket_within_the_window() {
    use crate::ids::MsgId;
    use crate::tables::budget::REPLY_RESERVE;
    use crate::tables::delivery::Queued;
    use crate::wire::Data;
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
    assert!(!r.booting());
    now = Instant::ZERO + Duration::from_secs(10);
    let node = NodeId([7; 6]);
    r.heard.heard(node, r.home, -80, now);
    // Short of the DATA and the reply reserve by what the bucket refills
    // well within the window.
    let dst = r.config.position;
    let short = Duration::from_millis(20);
    let data = Data {
        sender_home: r.home,
        dst_hop: node,
        src_hop: r.me(),
        dst_pos: dst,
        dst_id: node,
        hops: 1,
        payload: b"later",
    };
    let frame = mac::frame(&Packet::Data(data), r.home, MIN_PREAMBLE_SYMBOLS);
    let need = mac::airtime(&frame) + REPLY_RESERVE - short;
    r.radio
        .budget
        .record(r.radio.budget.available(now) - need, now);
    let id = MsgId::of(dst, node, b"later");
    let mut record = crate::tables::records::Record::new(id, None, 0, now);
    record.set_arrived();
    let at = r.record_place().unwrap();
    r.records.put(at, record);
    let q = Queued::new(id, dst, node, b"later", now).unwrap();
    r.delivery.push(q);
    r.forwarding.feed_due = true;
    r.handle(Event::Wakeup, now, &mut |_| {});
    assert!(r.queue.is_empty(), "taken with a spent bucket");
    let fits = r
        .radio
        .budget
        .allowed_at(mac::airtime(&frame), REPLY_RESERVE, now)
        .unwrap();
    assert_eq!(r.next_wakeup(), Some(fits));
    let window = r.heard.get(node).unwrap().awake_until;
    assert!(fits < window, "the bucket fits it in the window");
    let until = now + Duration::from_secs(60);
    let sent = drive(&mut r, &mut now, until);
    let datas: Vec<_> = sent
        .iter()
        .filter(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Data(d)) if d.dst_hop == node))
        .collect();
    assert_eq!(
        datas.len(),
        3,
        "three attempts at a node that never answers"
    );
    let first = datas[0].0;
    assert!(
        first >= fits && first.since(fits) < Duration::from_millis(1),
        "sent {:?} after the bucket fit it",
        first.since(fits)
    );
    assert!(!r.heard.awake(node, now), "asleep until its next beat");
    assert_eq!(r.delivery.len(), 1, "it waits for the next beat");
}

/// A DATA to a sleepy node goes ahead of everything in the send queue
/// (spec "Delivery"), so it needs only its own frame and the reply
/// reserve in the bucket, not the airtime of the forwards it goes ahead
/// of: a holder at its cap whose queued forwards outweigh its bucket
/// still delivers in the window (spec "Delivery queue", roadmap D55).
#[test]
fn a_delivery_goes_ahead_of_the_queue_in_the_bucket_too() {
    use crate::ids::MsgId;
    use crate::tables::budget::REPLY_RESERVE;
    use crate::tables::delivery::Queued;
    use crate::tables::send_queue::{Kind, Outgoing};
    use crate::wire::{Data, MAX_PAYLOAD};
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
    now = Instant::ZERO + Duration::from_secs(10);
    let node = NodeId([7; 6]);
    r.heard.heard(node, r.home, -80, now);
    let dst = r.config.position;
    // Forwards waiting for a ban, a quarter of the bucket's depth.
    let mut queued = Duration::ZERO;
    for k in 0..10u8 {
        let mut payload = [0u8; MAX_PAYLOAD];
        payload[0] = k;
        let d = Data {
            sender_home: r.home,
            dst_hop: NodeId::NONE,
            src_hop: r.me(),
            dst_pos: dst,
            dst_id: name(99),
            hops: 2,
            payload: &payload,
        };
        let frame = mac::frame(&Packet::Data(d), r.home, MIN_PREAMBLE_SYMBOLS);
        queued = queued + mac::airtime(&frame);
        let out = Outgoing {
            not_before: now + hour,
            ..Outgoing::new(frame, Kind::Forward)
        };
        r.queue.push(out).unwrap();
    }
    let data = Data {
        sender_home: r.home,
        dst_hop: node,
        src_hop: r.me(),
        dst_pos: dst,
        dst_id: node,
        hops: 0,
        payload: b"ahead",
    };
    let frame = mac::frame(&Packet::Data(data), r.home, MIN_PREAMBLE_SYMBOLS);
    // The bucket holds the delivery and the reply reserve, but not the
    // forwards as well.
    let need = mac::airtime(&frame) + REPLY_RESERVE + Duration::from_millis(1);
    assert!(need < queued);
    r.radio
        .budget
        .record(r.radio.budget.available(now) - need, now);
    let id = MsgId::of(dst, node, b"ahead");
    let mut record = crate::tables::records::Record::new(id, None, 0, now);
    record.set_arrived();
    let at = r.record_place().unwrap();
    r.records.put(at, record);
    let q = Queued::new(id, dst, node, b"ahead", now).unwrap();
    r.delivery.push(q);
    r.forwarding.feed_due = true;
    let mut checked = None;
    r.handle(Event::Wakeup, now, &mut |a| {
        if let Action::Lbt(c) = a {
            checked = Some(c);
        }
    });
    assert!(
        r.queue.iter().any(|o| o.kind == Kind::Deliver),
        "held back by the forwards behind it"
    );
    assert_eq!(checked, Some(r.home), "its hand-over starts at once");
}

/// Two packets arrive within one reply time, while a reading of the
/// sweep holds the radio (core-interface §3 rule 8). The second finds
/// the one owed-reply place holding the first one's ACK: it is neither
/// taken nor refused, and gets no answer. Its sender's retry, after the
/// first ACK is gone, is taken (roadmap D29 rule 4).
#[test]
fn a_packet_that_cannot_be_answered_is_not_taken() {
    use crate::event::AppEvent;
    use crate::wire::Data;
    let hour = Duration::from_secs(3600);
    let config = Config {
        boot_listen: Duration::from_secs(1),
        home_override: Channel::new(3),
        sweep_interval: Duration::from_secs(1),
        heartbeat_min: hour,
        heartbeat_max: hour,
        ..Config::default()
    };
    let mut r = Relay::new(config, 1);
    let mut now = Instant::ZERO;
    drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(2));
    assert!(!r.booting());
    let (a_home, b_home) = (Channel::new(5).unwrap(), Channel::new(6).unwrap());
    let packet = |home: Channel, from: u32, text: &'static [u8]| Data {
        sender_home: home,
        dst_hop: r.me(),
        src_hop: name(from),
        dst_pos: r.config.position,
        dst_id: r.me(),
        hops: 1,
        payload: text,
    };
    let (a, b) = (packet(a_home, 1, b"first"), packet(b_home, 2, b"second"));
    let bytes = |d: &Data<'_>| {
        let f = mac::frame(&Packet::Data(*d), r.home, MIN_PREAMBLE_SYMBOLS);
        f.bytes[..f.len as usize].to_vec()
    };
    let (a_bytes, b_bytes) = (bytes(&a), bytes(&b));
    let received = |bytes| Event::Received {
        channel: Channel::new(3).unwrap(),
        rssi_dbm: -80,
        snr_db: 10,
        bytes,
    };
    let taken = |actions: &[Action], d: &Data<'_>| {
        actions.iter().any(
            |x| matches!(x, Action::App(AppEvent::Message { msg_id, .. }) if *msg_id == d.msg_id()),
        )
    };
    let acks = |sent: &[(Instant, Frame)], d: &Data<'_>, home| {
        sent.iter()
            .filter(|(_, f)| {
                f.channel == home
                    && matches!(Packet::decode(f.payload()), Ok(Packet::Ack(k)) if k.msg_id == d.msg_id())
            })
            .count()
    };

    let frames = |got: &[(Instant, Action)]| -> Vec<(Instant, Frame)> {
        got.iter()
            .filter_map(|(t, x)| match x {
                Action::Transmit(f) => Some((*t, *f)),
                _ => None,
            })
            .collect()
    };
    let delivered = |got: &[(Instant, Action)], d: &Data<'_>| {
        got.iter().any(|(_, x)| {
            matches!(x, Action::App(AppEvent::Message { msg_id, .. }) if *msg_id == d.msg_id())
        })
    };

    // Run on until a reading of the sweep is outstanding, and hold it.
    let mut held = None;
    while held.is_none() {
        let t = r.next_wakeup().expect("the sweep is due");
        now = now.max(t);
        let mut pending = Vec::new();
        r.handle(Event::Wakeup, now, &mut |x| pending.push(x));
        for x in pending {
            match x {
                Action::Rssi(c) => held = Some(c),
                Action::Transmit(f) => {
                    now += mac::airtime(&f);
                    r.handle(Event::TxDone, now, &mut |_| {});
                }
                Action::Lbt(channel) => r.handle(
                    Event::LbtDone {
                        channel,
                        busy: false,
                        dbm: -130,
                    },
                    now,
                    &mut |_| {},
                ),
                _ => {}
            }
        }
    }
    now += Duration::from_millis(1);
    let mut actions = Vec::new();
    r.handle(received(&a_bytes), now, &mut |x| actions.push(x));
    assert!(!taken(&actions, &a), "not before its ACK");
    assert!(
        !actions.iter().any(|x| matches!(x, Action::Transmit(_))),
        "the radio is held: {actions:?}"
    );
    assert!(r.radio.replies.pending());

    now += Duration::from_millis(1);
    actions.clear();
    r.handle(received(&b_bytes), now, &mut |x| actions.push(x));
    assert!(
        actions
            .iter()
            .all(|x| matches!(x, Action::Trace(Trace::Silent { owed: true, .. }))),
        "{actions:?}"
    );
    assert!(r.records.get(b.msg_id()).is_none(), "B's packet was taken");

    // The reading completes: A's ACK goes, and A is taken with it.
    now += Duration::from_millis(1);
    actions.clear();
    r.handle(
        Event::RssiSample {
            channel: held.unwrap(),
            dbm: -130,
        },
        now,
        &mut |x| actions.push(x),
    );
    let until = now + Duration::from_millis(100);
    let mut got = Vec::new();
    drive_all(&mut r, &mut now, until, actions, &mut |t, x| {
        got.push((t, x))
    });
    let sent = frames(&got);
    assert_eq!(acks(&sent, &a, a_home), 1);
    assert_eq!(sent.len(), 1, "B's packet was answered");
    assert!(delivered(&got, &a));

    // B's retry, after the retry wait (spec "Retry wait").
    now += Duration::from_millis(200);
    let mut actions = Vec::new();
    r.handle(received(&b_bytes), now, &mut |x| actions.push(x));
    let until = now + Duration::from_millis(100);
    let mut got = Vec::new();
    drive_all(&mut r, &mut now, until, actions, &mut |t, x| {
        got.push((t, x))
    });
    assert_eq!(acks(&frames(&got), &b, b_home), 1);
    assert!(delivered(&got, &b));
}

/// The application may send only while the relay can take a message
/// (roadmap D29 rule 6): not while it boots, not while it holds one of
/// the application's, and not while its bucket cannot hold a full
/// frame on top of what it has queued; then the relay wakes when it
/// can. A send offered anyway is ignored. A message for a sleepy node
/// this relay holds for that finds the delivery queue full is held
/// until it has a place.
#[test]
fn the_application_waits_for_room() {
    use crate::ids::NodeId;
    use crate::tables::budget::DEPTH;
    use crate::tables::delivery::DELIVERY_QUEUE;
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
    r.handle(Event::Wakeup, now, &mut |_| {});
    // From its boot (roadmap D73).
    assert!(r.accepts_send(now), "booting");
    drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(2));
    assert!(r.accepts_send(now));

    // Messages for a sleepy node at this relay's position that is not
    // heard fill the delivery queue.
    let me = r.config.position;
    let send = |r: &mut Relay, k: u8, now| {
        let cmd = AppCommand::Send {
            dst_pos: me,
            dst_id: NodeId([1; 6]),
            payload: &[k],
        };
        r.handle(Event::App(cmd), now, &mut |_| {});
    };
    for k in 0..=DELIVERY_QUEUE as u8 {
        assert!(r.accepts_send(now), "message {k}");
        send(&mut r, k, now);
    }
    assert_eq!(r.holding().count(), DELIVERY_QUEUE + 1, "one held");
    assert!(!r.accepts_send(now));
    send(&mut r, 254, now);
    assert_eq!(
        r.holding().count(),
        DELIVERY_QUEUE + 1,
        "taken while holding"
    );
    // A delivery TTL on the oldest is stale and makes way for the held
    // one; the others stay, and the application may send again.
    let ttl = r.config.delivery_ttl;
    let until = now + ttl + Duration::from_secs(1);
    drive(&mut r, &mut now, until);
    assert!(!r.queue.iter().any(|o| o.held()));
    assert_eq!(
        r.holding().count(),
        DELIVERY_QUEUE,
        "the held one, now queued"
    );
    assert!(r.accepts_send(now));

    // A bucket spent down to a full frame keeps the application
    // waiting until it has refilled that much, and wakes the relay then.
    let mut r = Relay::new(config, 1);
    let mut now = Instant::ZERO;
    drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(2));
    r.radio.budget.record(DEPTH, now);
    r.handle(Event::Wakeup, now, &mut |_| {});
    assert!(!r.accepts_send(now));
    let ready = r.next_wakeup().expect("a wakeup for the application");
    assert!(ready > now);
    assert!(!r.accepts_send(Instant::from_micros(ready.as_micros() - 1)));
    assert!(r.accepts_send(ready));
}

/// With the regular room closed by the bucket, a packet that has come
/// two hops takes hop place 2, then 1, whatever the bucket holds; a
/// sleepy node's own, or a third from two hops out, is refused. A place frees
/// when its frame leaves (roadmap D31). The relay has no neighbours, so
/// each packet, from a sender of its own, would go back its own way.
#[test]
fn a_packet_that_has_travelled_takes_a_hop_place() {
    use crate::event::DropReason;
    use crate::wire::{Data, MAX_PAYLOAD};
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
    now = Instant::ZERO + Duration::from_secs(10);
    // Room for replies, not for a full frame on top of the reserve.
    let spend = r.radio.budget.available(now) - Duration::from_millis(300);
    r.radio.budget.record(spend, now);
    let far = Position {
        lat: r.config.position.lat + 100_000,
        ..r.config.position
    };
    let home = Channel::new(5).unwrap();
    let offer = |r: &mut Relay, now: &mut Instant, tag: u8, hops: u8| {
        let payload = [tag; MAX_PAYLOAD];
        let d = Data {
            sender_home: home,
            dst_hop: r.me(),
            src_hop: name(tag.into()),
            dst_pos: far,
            dst_id: NOBODY,
            hops,
            payload: &payload,
        };
        let f = mac::frame(&Packet::Data(d), r.home, MIN_PREAMBLE_SYMBOLS);
        let event = Event::Received {
            channel: r.home,
            rssi_dbm: -80,
            snr_db: 10,
            bytes: &f.bytes[..f.len as usize],
        };
        let mut refused = None;
        let mut pending = Vec::new();
        r.handle(event, *now, &mut |a| match a {
            Action::Trace(Trace::Refused { reason, .. }) => refused = Some(reason),
            Action::Lbt(_) | Action::Rssi(_) | Action::Transmit(_) => pending.push(a),
            _ => {}
        });
        // Send the reply, so the next packet can be answered.
        let until = *now + Duration::from_millis(100);
        drive_from(r, now, until, pending);
        let place = r
            .queue
            .iter()
            .find(|o| o.msg_id() == Some(d.msg_id()))
            .map(|o| o.place);
        (place, refused)
    };
    assert_eq!(
        offer(&mut r, &mut now, 1, 0),
        (None, Some(DropReason::Budget))
    );
    assert_eq!(offer(&mut r, &mut now, 2, 2), (Some(2), None));
    assert_eq!(offer(&mut r, &mut now, 3, 2), (Some(1), None));
    assert_eq!(
        offer(&mut r, &mut now, 4, 2),
        (None, Some(DropReason::Budget))
    );
    r.queue.retain(|_, o| o.place != 1);
    assert_eq!(offer(&mut r, &mut now, 5, 2), (Some(1), None));
    assert_eq!(r.queue.regular(), 0);
}

/// A full record table gives up the least recently active record of a
/// message this relay does not hold: a message waiting in the send
/// queue keeps its record however old (spec "Message records", roadmap D31).
#[test]
fn a_waiting_message_keeps_its_record() {
    use crate::ids::MsgId;
    use crate::tables::records::{Record, HELD, RECORDS};
    use crate::wire::{Data, MAX_PAYLOAD};
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
    now = Instant::ZERO + Duration::from_secs(10);
    // Room for replies only: what is taken waits in a hop place.
    let spend = r.radio.budget.available(now) - Duration::from_millis(300);
    r.radio.budget.record(spend, now);
    let far = Position {
        lat: r.config.position.lat + 100_000,
        ..r.config.position
    };
    let offer = |r: &mut Relay, now: &mut Instant, tag: u8, hops: u8| {
        let payload = [tag; MAX_PAYLOAD];
        let d = Data {
            sender_home: Channel::new(5).unwrap(),
            dst_hop: r.me(),
            src_hop: name(1),
            dst_pos: far,
            dst_id: NOBODY,
            hops,
            payload: &payload,
        };
        let f = mac::frame(&Packet::Data(d), r.home, MIN_PREAMBLE_SYMBOLS);
        let event = Event::Received {
            channel: r.home,
            rssi_dbm: -80,
            snr_db: 10,
            bytes: &f.bytes[..f.len as usize],
        };
        let mut pending = Vec::new();
        r.handle(event, *now, &mut |a| {
            if matches!(a, Action::Lbt(_) | Action::Rssi(_) | Action::Transmit(_)) {
                pending.push(a)
            }
        });
        let until = *now + Duration::from_millis(100);
        drive_from(r, now, until, pending);
        d.msg_id()
    };
    let waiting = offer(&mut r, &mut now, 1, 2);
    assert!(r.queue.iter().any(|o| o.msg_id() == Some(waiting)));
    // Every other record newer.
    let later = now + Duration::from_secs(1);
    for i in 1..RECORDS as u32 {
        let at = r.records.place(|_| false).unwrap();
        r.records.put(at, Record::new(MsgId(i), None, 0, later));
    }
    assert!(r.records.is_full());
    now = later + Duration::from_secs(1);
    let next = offer(&mut r, &mut now, 2, 3);
    assert!(
        r.records.get(waiting).is_some(),
        "the waiting message's record went"
    );
    assert!(r.records.get(next).is_some());
    assert!(
        r.records.get(MsgId(1)).is_none(),
        "the oldest not held went"
    );
    // What a relay holds stays within its places, so a record place
    // is always found.
    assert!(r.queue.len() <= HELD);
    assert!(r.record_place().is_some());
}

/// Entries waiting for one busy neighbour keep their arrival order
/// through bans that end and recur, and of equals the oldest is
/// offered first each time (roadmap D31 rule 4). Choosing an entry
/// used to move it to the front, which reversed the waiting entries
/// at every ban and left the oldest waiting for minutes.
#[test]
fn waiting_entries_keep_arrival_order_through_bans() {
    use crate::ids::MsgId;
    use crate::tables::budget::BAN;
    use crate::tables::records::Record;
    use crate::tables::send_queue::{Kind, Outgoing};
    use crate::wire::{Ack, Data, Heartbeat};
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
    let me = r.config.position;
    let b = Position {
        lat: me.lat + 5_000,
        ..me
    };
    let far = Position {
        lat: me.lat + 100_000,
        ..me
    };
    let (home, b_home) = (r.home, Channel::new(5).unwrap());
    let hear = |r: &mut Relay, now: Instant, p: &Packet<'_>| {
        let f = mac::frame(p, home, MIN_PREAMBLE_SYMBOLS);
        let event = Event::Received {
            channel: home,
            rssi_dbm: -80,
            snr_db: 10,
            bytes: &f.bytes[..f.len as usize],
        };
        let mut pending = Vec::new();
        r.handle(event, now, &mut |a| pending.push(a));
        pending
    };
    let hb = Heartbeat {
        sleepy: false,
        anchorless: false,
        carried: false,
        home: b_home,
        format: crate::wire::FORMAT,
        map_point: false,
        id: id_of(b),
        short: b.short(),
    };
    hear(&mut r, now, &Packet::Heartbeat(hb));
    // Four forwards for B, queued in this order.
    let mut ids = Vec::new();
    for tag in 0..4u8 {
        let payload = [tag];
        let d = Data {
            sender_home: home,
            dst_hop: name(0),
            src_hop: r.me(),
            dst_pos: far,
            dst_id: NOBODY,
            hops: 2,
            payload: &payload,
        };
        let parent = Some((name(7), Channel::new(6).unwrap()));
        let at = r.record_place().unwrap();
        r.records.put(at, Record::new(d.msg_id(), parent, 1, now));
        let f = mac::frame(&Packet::Data(d), home, MIN_PREAMBLE_SYMBOLS);
        r.queue.push(Outgoing::new(f, Kind::Forward)).unwrap();
        ids.push(d.msg_id());
    }
    // B refuses the first `refusals` offers, then takes everything.
    let refusals = 3;
    let mut offers: Vec<(Instant, MsgId)> = Vec::new();
    let until = now + Duration::from_secs(20);
    let mut pending: Vec<Action> = Vec::new();
    r.handle(Event::Wakeup, now, &mut |a| pending.push(a));
    loop {
        let event = match pending.pop() {
            Some(Action::Lbt(channel)) => Event::LbtDone {
                channel,
                busy: false,
                dbm: -130,
            },
            Some(Action::Rssi(channel)) => Event::RssiSample { channel, dbm: -130 },
            Some(Action::Transmit(f)) => {
                now += mac::airtime(&f);
                r.handle(Event::TxDone, now, &mut |a| pending.push(a));
                if let Ok(Packet::Data(d)) = Packet::decode(f.payload()) {
                    let id = d.msg_id();
                    offers.push((now, id));
                    now += Duration::from_millis(5);
                    let reply = Packet::Ack(Ack {
                        msg_id: id,
                        from: id_of(b),
                        to: r.me(),
                        refused: offers.len() <= refusals,
                        wait: 0,
                        seen: false,
                    });
                    pending.extend(hear(&mut r, now, &reply));
                }
                continue;
            }
            Some(_) => continue,
            None => match r.next_wakeup() {
                Some(t) if t <= until => {
                    now = now.max(t);
                    Event::Wakeup
                }
                _ => break,
            },
        };
        r.handle(event, now, &mut |a| pending.push(a));
        if offers.len() == refusals {
            // Every ban has ended and recurred: the queue is as it came.
            let queued: Vec<_> = r.queue.iter().filter_map(|o| o.msg_id()).collect();
            assert_eq!(queued, ids, "arrival order");
        }
    }
    let offered: Vec<_> = offers.iter().map(|(_, id)| *id).collect();
    // The oldest is offered at every ban's end, then each in turn.
    let mut expect = std::vec![ids[0]; refusals];
    expect.extend(&ids);
    assert_eq!(offered, expect);
    for w in offers[..=refusals].windows(2) {
        assert!(w[1].0.since(w[0].0) >= BAN, "offered within the ban");
    }
    assert!(r.queue.is_empty());
}

/// A message that has waited for a ban goes ahead of those that have
/// not, the most travelled first, for as long as it is queued (spec
/// "Overload waits"). A, far travelled, waits for D's ban behind K and M,
/// which go to E; when D's ban ends A goes ahead of M, D refuses it, and
/// A goes round D to E, still ahead of M, which has waited for nothing.
#[test]
fn a_message_that_waited_keeps_its_turn() {
    use crate::ids::MsgId;
    use crate::tables::records::Record;
    use crate::tables::send_queue::{Kind, Outgoing};
    use crate::wire::{Ack, Data};
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
    let me = r.config.position;
    // D is A's best way north; E, north-east, its second, and the only
    // way east.
    let d_pos = Position {
        lat: me.lat + 10_000,
        ..me
    };
    let e_pos = Position {
        lat: me.lat + 4_000,
        lon: me.lon + 4_000,
    };
    for (p, home) in [(d_pos, 5), (e_pos, 7)] {
        let hb = Heartbeat {
            sleepy: false,
            anchorless: false,
            carried: false,
            home: Channel::new(home).unwrap(),
            format: crate::wire::FORMAT,
            map_point: false,
            id: id_of(p),
            short: p.short(),
        };
        hear(&mut r, &mut now, &Packet::Heartbeat(hb));
    }
    let (d, e) = (id_of(d_pos), id_of(e_pos));
    let north = Position {
        lat: me.lat + 100_000,
        ..me
    };
    let east = Position {
        lon: me.lon + 100_000,
        ..me
    };
    // K, M and A, queued in this order.
    let home = r.home;
    let mut ids = Vec::new();
    for (tag, dst_pos, hops) in [(0u8, east, 1u8), (1, east, 2), (2, north, 9)] {
        let payload = [tag];
        let data = Data {
            sender_home: home,
            dst_hop: name(0),
            src_hop: r.me(),
            dst_pos,
            dst_id: NOBODY,
            hops,
            payload: &payload,
        };
        let parent = Some((name(7), Channel::new(6).unwrap()));
        let at = r.record_place().unwrap();
        r.records
            .put(at, Record::new(data.msg_id(), parent, 1, now));
        let f = mac::frame(&Packet::Data(data), home, MIN_PREAMBLE_SYMBOLS);
        r.queue.push(Outgoing::new(f, Kind::Forward)).unwrap();
        ids.push(data.msg_id());
    }
    // D's ban ends while K is handed over.
    r.neighbours.refused(d, now + Duration::from_millis(1));
    let answer = |r: &mut Relay, now: Instant, p: &Packet<'_>| {
        let f = mac::frame(p, home, MIN_PREAMBLE_SYMBOLS);
        let event = Event::Received {
            channel: home,
            rssi_dbm: -80,
            snr_db: 10,
            bytes: &f.bytes[..f.len as usize],
        };
        let mut pending = Vec::new();
        r.handle(event, now, &mut |a| pending.push(a));
        pending
    };
    // D refuses everything, E takes everything.
    let mut offers: Vec<(NodeId, MsgId)> = Vec::new();
    let until = now + Duration::from_secs(20);
    let mut pending: Vec<Action> = Vec::new();
    r.handle(Event::Wakeup, now, &mut |a| pending.push(a));
    loop {
        let event = match pending.pop() {
            Some(Action::Lbt(channel)) => Event::LbtDone {
                channel,
                busy: false,
                dbm: -130,
            },
            Some(Action::Rssi(channel)) => Event::RssiSample { channel, dbm: -130 },
            Some(Action::Transmit(f)) => {
                now += mac::airtime(&f);
                r.handle(Event::TxDone, now, &mut |a| pending.push(a));
                if let Ok(Packet::Data(data)) = Packet::decode(f.payload()) {
                    let id = data.msg_id();
                    offers.push((data.dst_hop, id));
                    now += Duration::from_millis(5);
                    let from = data.dst_hop;
                    let reply = Packet::Ack(Ack {
                        msg_id: id,
                        from,
                        to: r.me(),
                        refused: from == d,
                        wait: 8,
                        seen: false,
                    });
                    pending.extend(answer(&mut r, now, &reply));
                }
                continue;
            }
            Some(_) => continue,
            None => match r.next_wakeup() {
                Some(t) if t <= until => {
                    now = now.max(t);
                    Event::Wakeup
                }
                _ => break,
            },
        };
        r.handle(event, now, &mut |a| pending.push(a));
    }
    let [k, m, a] = [ids[0], ids[1], ids[2]];
    assert_eq!(offers, [(e, k), (d, a), (e, a), (e, m)]);
    assert!(r.queue.is_empty());
}

/// The mark that a message waited for a ban outlasts its offer (spec
/// "Overload waits").
#[test]
fn the_offer_keeps_the_mark() {
    use crate::tables::records::Record;
    use crate::tables::send_queue::{Kind, Outgoing};
    use crate::wire::Data;
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
    let me = r.config.position;
    let d_pos = Position {
        lat: me.lat + 10_000,
        ..me
    };
    let hb = Heartbeat {
        sleepy: false,
        anchorless: false,
        carried: false,
        home: Channel::new(5).unwrap(),
        format: crate::wire::FORMAT,
        map_point: false,
        id: id_of(d_pos),
        short: d_pos.short(),
    };
    hear(&mut r, &mut now, &Packet::Heartbeat(hb));
    let home = r.home;
    let payload = [0];
    let data = Data {
        sender_home: home,
        dst_hop: name(0),
        src_hop: r.me(),
        dst_pos: Position {
            lat: me.lat + 100_000,
            ..me
        },
        dst_id: NOBODY,
        hops: 9,
        payload: &payload,
    };
    let at = r.record_place().unwrap();
    let parent = Some((name(7), Channel::new(6).unwrap()));
    r.records
        .put(at, Record::new(data.msg_id(), parent, 1, now));
    let f = mac::frame(&Packet::Data(data), home, MIN_PREAMBLE_SYMBOLS);
    r.queue.push(Outgoing::new(f, Kind::Forward)).unwrap();
    r.neighbours
        .refused(id_of(d_pos), now + Duration::from_millis(1));
    r.handle(Event::Wakeup, now, &mut |_| {});
    assert!(r.queue.get(0).unwrap().waited, "it waits for D's ban");
    now = r.next_wakeup().unwrap();
    r.handle(Event::Wakeup, now, &mut |_| {});
    assert!(r.queue.current().is_some(), "its hand-over has started");
    assert!(r.queue.get(0).unwrap().waited);
}

/// Two relays offer one message to the same neighbour D at once, as a
/// retry after a lost ACK and a return can (roadmap D32). D's answers
/// to the other sender, C, taking or refusing, neither end this
/// relay's hand-over nor ban D here; only D's answer to this relay
/// does. When an ACK named only the message and D, D's ACK to C ended
/// this hand-over too, and the message, which D had not taken from
/// this relay, was lost.
#[test]
fn an_answer_to_another_sender_is_not_ours() {
    use crate::tables::records::Record;
    use crate::tables::send_queue::{Kind, Outgoing};
    use crate::wire::{Ack, Data, Heartbeat};
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
    let me = r.config.position;
    let d_pos = Position {
        lat: me.lat + 5_000,
        ..me
    };
    let far = Position {
        lat: me.lat + 100_000,
        ..me
    };
    let home = r.home;
    let hear = |r: &mut Relay, now: Instant, p: &Packet<'_>| {
        let f = mac::frame(p, home, MIN_PREAMBLE_SYMBOLS);
        let event = Event::Received {
            channel: home,
            rssi_dbm: -80,
            snr_db: 10,
            bytes: &f.bytes[..f.len as usize],
        };
        let mut actions = Vec::new();
        r.handle(event, now, &mut |a| actions.push(a));
        actions
    };
    let hb = Heartbeat {
        sleepy: false,
        anchorless: false,
        carried: false,
        home: Channel::new(5).unwrap(),
        format: crate::wire::FORMAT,
        map_point: false,
        id: id_of(d_pos),
        short: d_pos.short(),
    };
    hear(&mut r, now, &Packet::Heartbeat(hb));
    let data = Data {
        sender_home: home,
        dst_hop: name(0),
        src_hop: r.me(),
        dst_pos: far,
        dst_id: NOBODY,
        hops: 2,
        payload: b"one message, two senders",
    };
    let id = data.msg_id();
    let parent = Some((name(7), Channel::new(6).unwrap()));
    let at = r.record_place().unwrap();
    r.records.put(at, Record::new(id, parent, 1, now));
    let f = mac::frame(&Packet::Data(data), home, MIN_PREAMBLE_SYMBOLS);
    r.queue.push(Outgoing::new(f, Kind::Forward)).unwrap();

    // Run until the offer to D has gone out.
    let mut pending = Vec::new();
    r.handle(Event::Wakeup, now, &mut |a| pending.push(a));
    loop {
        let event = match pending.pop().expect("the offer is sent") {
            Action::Lbt(channel) => Event::LbtDone {
                channel,
                busy: false,
                dbm: -130,
            },
            Action::Rssi(channel) => Event::RssiSample { channel, dbm: -130 },
            Action::Transmit(f) => {
                assert!(matches!(Packet::decode(f.payload()), Ok(Packet::Data(_))));
                now += mac::airtime(&f);
                r.handle(Event::TxDone, now, &mut |_| {});
                break;
            }
            _ => continue,
        };
        r.handle(event, now, &mut |a| pending.push(a));
    }
    let to_me = Ack {
        msg_id: id,
        from: id_of(d_pos),
        to: r.me(),
        refused: false,
        wait: 0,
        seen: false,
    };
    let to_c = Ack {
        to: name(0x0c0c_0c0c),
        ..to_me
    };
    let banned = |r: &Relay, now| {
        r.neighbours()
            .any(|n| n.position == d_pos && r.neighbours.is_banned(n, now))
    };
    now += Duration::from_millis(5);
    for answer in [
        to_c,
        Ack {
            refused: true,
            ..to_c
        },
    ] {
        let actions = hear(&mut r, now, &Packet::Ack(answer));
        assert!(r.awaiting_ack(), "{answer:?} ended the hand-over");
        assert!(!banned(&r, now), "{answer:?} banned D");
        assert!(!actions.iter().any(|a| matches!(a, Action::Trace(_))));
    }
    let actions = hear(&mut r, now, &Packet::Ack(to_me));
    assert!(r.handover_idle() && r.queue.is_empty());
    assert!(actions.iter().any(|a| matches!(
        a,
        Action::Trace(Trace::Forwarded { msg_id, .. }) if *msg_id == id
    )));
}

/// A neighbour whose heartbeats this relay stops hearing, but which
/// answers its hand-overs, taking or refusing them, is alive and stays
/// in the table past the 390 s a heartbeat alone keeps it (spec "Neighbour table", "The ACK").
/// Without this, a relay whose only neighbour's heartbeats were lost
/// for a few rotations dropped it while it was carrying every message.
#[test]
fn a_neighbour_that_answers_stays_live() {
    use crate::tables::records::Record;
    use crate::tables::send_queue::{Kind, Outgoing};
    use crate::wire::{Ack, Data};
    let hour = Duration::from_secs(3600);
    let config = Config {
        boot_listen: Duration::from_secs(1),
        home_override: Channel::new(3),
        sweep_interval: hour,
        ..Config::default()
    };
    let mut r = Relay::new(config, 1);
    let mut now = Instant::ZERO;
    drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(2));
    let me = r.config.position;
    let d_pos = Position {
        lat: me.lat + 5_000,
        ..me
    };
    let far = Position {
        lat: me.lat + 100_000,
        ..me
    };
    let home = r.home;
    let hear = |r: &mut Relay, now: Instant, p: &Packet<'_>| {
        let f = mac::frame(p, home, MIN_PREAMBLE_SYMBOLS);
        let event = Event::Received {
            channel: home,
            rssi_dbm: -80,
            snr_db: 10,
            bytes: &f.bytes[..f.len as usize],
        };
        let mut actions = Vec::new();
        r.handle(event, now, &mut |a| actions.push(a));
        actions
    };
    let heard_at = now;
    hear(
        &mut r,
        now,
        &Packet::Heartbeat(Heartbeat {
            sleepy: false,
            anchorless: false,
            carried: false,
            home: Channel::new(5).unwrap(),
            format: crate::wire::FORMAT,
            map_point: false,
            id: id_of(d_pos),
            short: d_pos.short(),
        }),
    );
    let expiry = r.neighbours.params().expire_after;
    assert_eq!(expiry, Duration::from_secs(390));
    let live = |r: &Relay, now| r.neighbours.live(now).any(|(_, n)| n.position == d_pos);

    // Hand D a message and let D answer, taking it, then refusing one.
    let answer = |r: &mut Relay, now: &mut Instant, tag: u8, refused: bool| {
        let payload = [tag];
        let data = Data {
            sender_home: home,
            dst_hop: name(0),
            src_hop: r.me(),
            dst_pos: far,
            dst_id: NOBODY,
            hops: 2,
            payload: &payload,
        };
        let id = data.msg_id();
        let parent = Some((name(7), Channel::new(6).unwrap()));
        let at = r.record_place().unwrap();
        r.records.put(at, Record::new(id, parent, 1, *now));
        let f = mac::frame(&Packet::Data(data), home, MIN_PREAMBLE_SYMBOLS);
        r.queue.push(Outgoing::new(f, Kind::Forward)).unwrap();
        let until = *now + Duration::from_millis(100);
        let mut pending = Vec::new();
        r.handle(Event::Wakeup, *now, &mut |a| pending.push(a));
        let sent = drive_from(r, now, until, pending);
        assert!(sent
            .iter()
            .any(|(_, f)| f.channel == Channel::new(5).unwrap()));
        let ack = Ack {
            msg_id: id,
            from: id_of(d_pos),
            to: r.me(),
            refused,
            wait: 0,
            seen: false,
        };
        let pending = hear(r, *now, &Packet::Ack(ack));
        r.queue.retain(|_, _| false);
        let until = *now + Duration::from_millis(100);
        drive_from(r, now, until, pending);
    };
    now = heard_at + (expiry - Duration::from_secs(10));
    answer(&mut r, &mut now, 1, false);
    let taken_at = now;
    assert!(live(&r, heard_at + expiry), "taken: alive");
    now = taken_at + (expiry - Duration::from_secs(10));
    answer(&mut r, &mut now, 2, true);
    let refused_at = now;
    assert!(live(&r, taken_at + expiry), "refused: alive");
    assert!(!live(&r, refused_at + expiry), "silent: expired");
}

/// A running relay with two neighbours, both banned for one [`BAN`]
/// from the returned instant, and a destination far beyond them.
fn with_every_neighbour_banned() -> (Relay, Instant, Position) {
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
    let me = r.config.position;
    for k in 1..=2 {
        let at = Position {
            lat: me.lat + 5_000 * k,
            ..me
        };
        let hb = Heartbeat {
            sleepy: false,
            anchorless: false,
            carried: false,
            home: Channel::new(5).unwrap(),
            format: crate::wire::FORMAT,
            map_point: false,
            id: id_of(at),
            short: at.short(),
        };
        r.neighbours.heard(&hb, at, -80, &r.plane, now);
        r.neighbours.refused(hb.id, now + BAN);
    }
    let far = Position {
        lat: me.lat + 100_000,
        ..me
    };
    (r, now, far)
}

/// Offer `r` a full DATA to `dst` received with `hops`, a sleepy node's
/// own at 0, and send its reply. Returns the place it took and why it was
/// refused, if it was.
fn offer_data(
    r: &mut Relay,
    now: &mut Instant,
    dst: Position,
    tag: u8,
    hops: u8,
) -> (Option<u8>, Option<crate::event::DropReason>) {
    let (place, refused, _) = offer_dropped(r, now, dst, tag, hops);
    (place, refused)
}

/// [`offer_data`], also returning why the message was dropped, if it
/// was.
fn offer_dropped(
    r: &mut Relay,
    now: &mut Instant,
    dst: Position,
    tag: u8,
    hops: u8,
) -> (
    Option<u8>,
    Option<crate::event::DropReason>,
    Option<crate::event::DropReason>,
) {
    use crate::wire::{Data, MAX_PAYLOAD};
    let payload = [tag; MAX_PAYLOAD];
    let d = Data {
        sender_home: Channel::new(5).unwrap(),
        dst_hop: r.me(),
        src_hop: name(1),
        dst_pos: dst,
        dst_id: NOBODY,
        hops,
        payload: &payload,
    };
    let f = mac::frame(&Packet::Data(d), r.home, MIN_PREAMBLE_SYMBOLS);
    let event = Event::Received {
        channel: r.home,
        rssi_dbm: -80,
        snr_db: 10,
        bytes: &f.bytes[..f.len as usize],
    };
    let (mut refused, mut dropped) = (None, None);
    let mut pending = Vec::new();
    r.handle(event, *now, &mut |a| match a {
        Action::Trace(Trace::Refused { reason, .. }) => refused = Some(reason),
        Action::Trace(Trace::Dropped { reason, .. }) => dropped = Some(reason),
        Action::Lbt(_) | Action::Rssi(_) | Action::Transmit(_) => pending.push(a),
        _ => {}
    });
    // The take, and a drop at the take, come with the ACK (roadmap D93).
    let until = *now + Duration::from_millis(100);
    for t in drive_traces(r, now, until, pending) {
        if let Trace::Dropped { reason, .. } = t {
            dropped = Some(reason);
        }
    }
    let place = r
        .queue
        .iter()
        .find(|o| o.msg_id() == Some(d.msg_id()))
        .map(|o| o.place);
    (place, refused, dropped)
}

/// No message ends by age on its way (spec "Overload waits", roadmap
/// D106): packets wait for their neighbours' bans, in the regular room
/// or a hop place, as long as the bans last.
#[test]
fn a_message_waits_whatever_its_age() {
    let (mut r, start, far) = with_every_neighbour_banned();
    let mut now = start;
    assert_eq!(
        offer_dropped(&mut r, &mut now, far, 1, 2),
        (Some(0), None, None)
    );
    let spend = r.radio.budget.available(now) - Duration::from_millis(300);
    r.radio.budget.record(spend, now);
    // A count higher than the first's passes the gate (spec "Receiving DATA").
    assert_eq!(
        offer_dropped(&mut r, &mut now, far, 2, 3),
        (Some(3), None, None)
    );
    // The neighbours do not answer: the messages wait for their bans
    // until three silences in a row make each neighbour tried for them
    // (spec "Next hop").
    let until = start + Duration::from_secs(10);
    let sent = drive(&mut r, &mut now, until);
    let offers = sent
        .iter()
        .filter(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Data(_))))
        .count();
    assert!(offers >= 3, "{offers}");
    assert_eq!(r.holding().count(), 2, "both kept");
}

/// A packet already in the network is taken under the same bans, into
/// the regular room and, with the bucket spent, a hop place: the hop
/// places' argument against gridlock needs every such packet to move
/// on (spec "Overload waits", roadmap D31, D34).
#[test]
fn a_packet_in_the_network_is_taken_whatever_the_bans() {
    let (mut r, mut now, far) = with_every_neighbour_banned();
    assert_eq!(offer_data(&mut r, &mut now, far, 1, 2), (Some(0), None));
    let spend = r.radio.budget.available(now) - Duration::from_millis(300);
    r.radio.budget.record(spend, now);
    // A count higher than the first's passes the gate (spec "Receiving DATA").
    assert_eq!(offer_data(&mut r, &mut now, far, 2, 3), (Some(3), None));
}

/// A hand-over carries the record's highest count plus one, stamped at
/// its first transmission and repeated by its retries; readdressed
/// after a failure it carries the count of its new first transmission;
/// one that succeeds raises the highest to its count (spec "Hop count", roadmap
/// D35). Stamped when queued, the frame would carry 2 throughout.
#[test]
fn a_hand_over_stamps_its_count_at_its_first_transmission() {
    use crate::tables::records::Record;
    use crate::tables::send_queue::{Kind, Outgoing};
    use crate::wire::{Ack, Data};
    let (mut r, start, far) = with_every_neighbour_banned();
    let mut now = start + BAN;
    let me = r.config.position;
    let [near, nearest] = [1, 2].map(|k| Position {
        lat: me.lat + 5_000 * k,
        ..me
    });
    let data = Data {
        sender_home: r.home,
        dst_hop: name(0),
        src_hop: r.me(),
        dst_pos: far,
        dst_id: NOBODY,
        hops: 2,
        payload: b"count",
    };
    let id = data.msg_id();
    let parent = Some((name(7), Channel::new(6).unwrap()));
    let at = r.record_place().unwrap();
    r.records.put(at, Record::new(id, parent, 1, now));
    let f = mac::frame(&Packet::Data(data), r.home, MIN_PREAMBLE_SYMBOLS);
    r.queue.push(Outgoing::new(f, Kind::Forward)).unwrap();
    // A copy with a higher count arrived since it was queued.
    r.records.get_mut(id).unwrap().received(name(8), 6, now);

    let counts = |sent: &[(Instant, Frame)], to: Position| -> Vec<u8> {
        sent.iter()
            .filter_map(|(_, f)| match Packet::decode(f.payload()) {
                Ok(Packet::Data(d)) if d.dst_hop == id_of(to) => Some(d.hops),
                _ => None,
            })
            .collect()
    };
    let mut pending = Vec::new();
    r.handle(Event::Wakeup, now, &mut |a| pending.push(a));
    let until = now;
    let mut sent = drive_from(&mut r, &mut now, until, pending);
    while counts(&sent, nearest).len() < 3 {
        let until = now + Duration::from_millis(10);
        sent.extend(drive(&mut r, &mut now, until));
        now = now.max(until);
        assert!(now < start + Duration::from_secs(10), "no third attempt");
    }
    assert_eq!(counts(&sent, nearest), [7, 7, 7], "stamped, then repeated");
    // Another copy arrives during the last attempt.
    r.records.get_mut(id).unwrap().received(name(8), 9, now);
    while counts(&sent, near).is_empty() {
        let until = now + Duration::from_millis(10);
        sent.extend(drive(&mut r, &mut now, until));
        now = now.max(until);
        assert!(now < start + Duration::from_secs(10), "not readdressed");
    }
    assert_eq!(counts(&sent, near), [10], "readdressed: stamped again");
    let ack = Ack {
        msg_id: id,
        from: id_of(near),
        to: r.me(),
        refused: false,
        wait: 0,
        seen: false,
    };
    let f = mac::frame(&Packet::Ack(ack), r.home, MIN_PREAMBLE_SYMBOLS);
    let event = Event::Received {
        channel: r.home,
        rssi_dbm: -80,
        snr_db: 10,
        bytes: &f.bytes[..f.len as usize],
    };
    r.handle(event, now, &mut |_| {});
    assert_eq!(r.records.get(id).unwrap().highest_hops(), 10);
}

/// A retransmission, the same sender and count again, is ACKed and not
/// taken; so is another copy while this relay holds one (spec "Receiving DATA" cases
/// 3 and 4, roadmap D35). Here the relay holds the message, waiting
/// for its banned neighbours.
#[test]
fn a_retransmission_or_a_copy_while_holding_is_only_acked() {
    use crate::wire::Data;
    let (mut r, mut now, far) = with_every_neighbour_banned();
    let offer = |r: &mut Relay, now: &mut Instant, from: u32, hops: u8| {
        let d = Data {
            sender_home: Channel::new(5).unwrap(),
            dst_hop: r.me(),
            src_hop: name(from),
            dst_pos: far,
            dst_id: NOBODY,
            hops,
            payload: b"again",
        };
        let f = mac::frame(&Packet::Data(d), r.home, MIN_PREAMBLE_SYMBOLS);
        let event = Event::Received {
            channel: r.home,
            rssi_dbm: -80,
            snr_db: 10,
            bytes: &f.bytes[..f.len as usize],
        };
        let mut pending = Vec::new();
        r.handle(event, *now, &mut |a| pending.push(a));
        let until = *now + Duration::from_millis(100);
        let acks: Vec<_> = drive_from(r, now, until, pending)
            .into_iter()
            .filter_map(|(_, f)| match Packet::decode(f.payload()) {
                Ok(Packet::Ack(a)) => Some(a),
                _ => None,
            })
            .collect();
        let held = r.queue.iter().filter(|o| o.msg_id() == Some(d.msg_id()));
        (acks, held.count())
    };
    let taken = |acks: &[crate::wire::Ack]| acks.len() == 1 && !acks[0].refused;
    let (acks, held) = offer(&mut r, &mut now, 1, 2);
    assert!(taken(&acks) && held == 1, "the first is taken");
    let (acks, held) = offer(&mut r, &mut now, 1, 2);
    assert!(
        taken(&acks) && held == 1,
        "its retransmission is only ACKed"
    );
    let (acks, held) = offer(&mut r, &mut now, 9, 2);
    assert!(taken(&acks) && held == 1, "another copy too, while held");
}

/// Once the relay a message is addressed to has taken it from this
/// one, the message has arrived: a later copy is only ACKed, wherever
/// it comes from (spec "Receiving DATA" case 1, roadmap D35).
#[test]
fn a_copy_after_the_addressed_relay_took_it_is_only_acked() {
    use crate::tables::records::Record;
    use crate::tables::send_queue::{Kind, Outgoing};
    use crate::wire::{Ack, Data};
    let (mut r, start, _) = with_every_neighbour_banned();
    let mut now = start + BAN;
    let me = r.config.position;
    let addressed = Position {
        lat: me.lat + 10_000,
        ..me
    };
    let data = Data {
        sender_home: r.home,
        dst_hop: name(0),
        src_hop: r.me(),
        dst_pos: addressed,
        dst_id: id_of(addressed),
        hops: 2,
        payload: b"arrived",
    };
    let id = data.msg_id();
    let parent = Some((name(7), Channel::new(6).unwrap()));
    let at = r.record_place().unwrap();
    r.records.put(at, Record::new(id, parent, 1, now));
    let f = mac::frame(&Packet::Data(data), r.home, MIN_PREAMBLE_SYMBOLS);
    r.queue.push(Outgoing::new(f, Kind::Forward)).unwrap();
    let mut pending = Vec::new();
    r.handle(Event::Wakeup, now, &mut |a| pending.push(a));
    let until = now + Duration::from_millis(50);
    let sent = drive_from(&mut r, &mut now, until, pending);
    assert!(sent.iter().any(|(_, f)| matches!(
        Packet::decode(f.payload()),
        Ok(Packet::Data(d)) if d.dst_hop == id_of(addressed)
    )));
    let hear = |r: &mut Relay, now: &mut Instant, p: &Packet<'_>| {
        let f = mac::frame(p, r.home, MIN_PREAMBLE_SYMBOLS);
        let event = Event::Received {
            channel: r.home,
            rssi_dbm: -80,
            snr_db: 10,
            bytes: &f.bytes[..f.len as usize],
        };
        let mut pending = Vec::new();
        r.handle(event, *now, &mut |a| pending.push(a));
        let until = *now + Duration::from_millis(100);
        drive_from(r, now, until, pending)
    };
    let ack = Ack {
        msg_id: id,
        from: id_of(addressed),
        to: r.me(),
        refused: false,
        wait: 0,
        seen: false,
    };
    hear(&mut r, &mut now, &Packet::Ack(ack));
    assert!(r.records.get(id).unwrap().arrived());
    assert!(r.queue.is_empty());
    let copy = Data {
        sender_home: Channel::new(5).unwrap(),
        dst_hop: r.me(),
        src_hop: name(9),
        hops: 5,
        ..data
    };
    let sent = hear(&mut r, &mut now, &Packet::Data(copy));
    let acks: Vec<_> = sent
        .iter()
        .filter_map(|(_, f)| match Packet::decode(f.payload()) {
            Ok(Packet::Ack(a)) => Some(a),
            _ => None,
        })
        .collect();
    assert!(acks.len() == 1 && !acks[0].refused, "{acks:?}");
    assert!(r.queue.is_empty(), "taken");
}

/// The relay's own application waits while every neighbour is banned,
/// and the relay wakes when the first ban ends (roadmap D34).
#[test]
fn the_application_waits_while_every_neighbour_is_banned() {
    let (mut r, now, _) = with_every_neighbour_banned();
    r.handle(Event::Wakeup, now, &mut |_| {});
    assert!(!r.accepts_send(now));
    assert_eq!(r.next_wakeup(), Some(now + BAN));
    assert!(r.accepts_send(now + BAN));
}

/// A relay with two neighbours toward a far destination, D nearer to it
/// than E, holding a forward of a message whose record has highest
/// count 1, with its hand-over to D on the air (spec "Next hop"). The
/// relay, the time, the message and the two neighbours' positions.
fn handing_to_the_nearer(text: &[u8]) -> (Relay, Instant, MsgId, Position, Position) {
    use crate::tables::records::Record;
    use crate::tables::send_queue::{Kind, Outgoing};
    use crate::wire::Data;
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
    let me = r.config.position;
    let [d_pos, e_pos] = [10_000, 5_000].map(|k| Position {
        lat: me.lat + k,
        ..me
    });
    let far = Position {
        lat: me.lat + 100_000,
        ..me
    };
    for (p, home) in [(d_pos, 5), (e_pos, 7)] {
        let hb = Heartbeat {
            sleepy: false,
            anchorless: false,
            carried: false,
            home: Channel::new(home).unwrap(),
            format: crate::wire::FORMAT,
            map_point: false,
            id: id_of(p),
            short: p.short(),
        };
        hear(&mut r, &mut now, &Packet::Heartbeat(hb));
    }
    let data = Data {
        sender_home: r.home,
        dst_hop: name(0),
        src_hop: r.me(),
        dst_pos: far,
        dst_id: NOBODY,
        hops: 2,
        payload: text,
    };
    let id = data.msg_id();
    let parent = Some((name(7), Channel::new(6).unwrap()));
    let at = r.record_place().unwrap();
    r.records.put(at, Record::new(id, parent, 1, now));
    let f = mac::frame(&Packet::Data(data), r.home, MIN_PREAMBLE_SYMBOLS);
    r.queue.push(Outgoing::new(f, Kind::Forward)).unwrap();
    let sent = until_sent(&mut r, &mut now, Vec::new());
    assert_eq!(data_to(&sent), [(id_of(d_pos), 2)], "D first");
    (r, now, id, d_pos, e_pos)
}

/// `r` hears `p` on its home channel at `now`, and runs until the
/// actions that follow are done; the frames it sent.
fn hear(r: &mut Relay, now: &mut Instant, p: &Packet<'_>) -> Vec<(Instant, Frame)> {
    let f = mac::frame(p, r.home, MIN_PREAMBLE_SYMBOLS);
    let event = Event::Received {
        channel: r.home,
        rssi_dbm: -80,
        snr_db: 10,
        bytes: &f.bytes[..f.len as usize],
    };
    let mut pending = Vec::new();
    r.handle(event, *now, &mut |a| pending.push(a));
    let until = *now;
    drive_from(r, now, until, pending)
}

/// Run `r` from the actions `pending` it has asked for, then from its
/// wakeups, until the next DATA frame has gone out, on a quiet channel;
/// the frames it sent. It is woken first: a test that queued a frame
/// itself did so outside any event, which the core looks at the queue
/// after.
fn until_sent(r: &mut Relay, now: &mut Instant, mut pending: Vec<Action>) -> Vec<(Instant, Frame)> {
    let mut woken = Vec::new();
    r.handle(Event::Wakeup, *now, &mut |a| woken.push(a));
    pending.splice(0..0, woken);
    let start = *now;
    let mut sent = drive_from(r, now, start, pending);
    while data_to(&sent).is_empty() {
        let until = *now + Duration::from_millis(10);
        sent.extend(drive(r, now, until));
        *now = (*now).max(until);
        assert!(*now < start + Duration::from_secs(10), "nothing sent");
    }
    sent
}

/// The receiver and count of each DATA frame in `sent`.
fn data_to(sent: &[(Instant, Frame)]) -> Vec<(NodeId, u8)> {
    sent.iter()
        .filter_map(|(_, f)| match Packet::decode(f.payload()) {
            Ok(Packet::Data(d)) => Some((d.dst_hop, d.hops)),
            _ => None,
        })
        .collect()
}

/// A seen ACK tries its sender and sends the forward on at once (spec
/// "The ACK", "Hand-over", roadmap D90). D answers that it has seen the
/// message: the relay marks D tried, spends the count D recorded, bans
/// nothing and counts no failure, and its next frame goes to E with the
/// next count, as it would after D returned the message.
#[test]
fn a_seen_ack_tries_the_neighbour_and_sends_on() {
    use crate::wire::Ack;
    let (mut r, mut now, id, d_pos, e_pos) = handing_to_the_nearer(b"seen");
    let seen = Ack {
        msg_id: id,
        from: id_of(d_pos),
        to: r.me(),
        refused: false,
        wait: 0,
        seen: true,
    };
    now += Duration::from_millis(5);
    let f = mac::frame(&Packet::Ack(seen), r.home, MIN_PREAMBLE_SYMBOLS);
    let event = Event::Received {
        channel: r.home,
        rssi_dbm: -80,
        snr_db: 10,
        bytes: &f.bytes[..f.len as usize],
    };
    let mut actions = Vec::new();
    r.handle(event, now, &mut |a| actions.push(a));
    assert!(actions.contains(&Action::Trace(Trace::Seen {
        msg_id: id,
        by: id_of(d_pos)
    })));
    assert!(!actions
        .iter()
        .any(|a| matches!(a, Action::Trace(Trace::Dropped { .. }))));
    let record = r.records.get(id).unwrap();
    let slot = r.neighbours.slot_of(id_of(d_pos)).unwrap();
    assert!(record.handed(slot), "D is tried");
    assert_eq!(record.highest_hops(), 2, "the count D recorded is spent");
    let d = r.neighbours.by_id(id_of(d_pos)).unwrap().1;
    assert!(!r.neighbours.is_banned(d, now) && d.fail_count == 0);
    assert!(r.holding().any(|m| m == id), "the message stays here");
    let sent = until_sent(&mut r, &mut now, actions);
    assert_eq!(data_to(&sent), [(id_of(e_pos), 3)], "E next, one count up");
    assert!(r.awaiting_ack());
}

/// A return answered seen ends the message: the parent has gone on
/// without this copy, and a return has nowhere else to go
/// (spec "Return to parent", roadmap D90).
#[test]
fn a_return_answered_seen_is_dropped() {
    use crate::event::DropReason;
    use crate::tables::records::Record;
    use crate::tables::send_queue::{Kind, Outgoing};
    use crate::wire::{Ack, Data};
    let (mut r, mut now, far) = with_every_neighbour_banned();
    now += BAN;
    let parent = name(7);
    let home = Channel::new(6).unwrap();
    let data = Data {
        sender_home: r.home,
        dst_hop: parent,
        src_hop: r.me(),
        dst_pos: far,
        dst_id: NOBODY,
        hops: 2,
        payload: b"back",
    };
    let id = data.msg_id();
    let at = r.record_place().unwrap();
    r.records
        .put(at, Record::new(id, Some((parent, home)), 1, now));
    let f = mac::frame(&Packet::Data(data), home, MIN_PREAMBLE_SYMBOLS);
    r.queue.push(Outgoing::new(f, Kind::Return)).unwrap();
    let sent = until_sent(&mut r, &mut now, Vec::new());
    assert_eq!(data_to(&sent), [(parent, 2)]);
    let seen = Ack {
        msg_id: id,
        from: parent,
        to: r.me(),
        refused: false,
        wait: 0,
        seen: true,
    };
    now += Duration::from_millis(5);
    let f = mac::frame(&Packet::Ack(seen), r.home, MIN_PREAMBLE_SYMBOLS);
    let event = Event::Received {
        channel: r.home,
        rssi_dbm: -80,
        snr_db: 10,
        bytes: &f.bytes[..f.len as usize],
    };
    let mut actions = Vec::new();
    r.handle(event, now, &mut |a| actions.push(a));
    assert!(actions.contains(&Action::Trace(Trace::Seen {
        msg_id: id,
        by: parent
    })));
    assert!(actions.contains(&Action::Trace(Trace::Dropped {
        msg_id: id,
        reason: DropReason::ReturnSeen
    })));
    assert!(r.queue.is_empty() && r.handover_idle());
    assert!(r.holding().next().is_none());
}

/// A node, sleepy or not, that receives a DATA for itself answers it
/// at once and hands it to its application when the ACK has gone out,
/// not before (roadmap D93).
#[test]
fn a_message_for_this_node_is_taken_when_its_ack_has_gone_out() {
    use crate::event::AppEvent;
    use crate::wire::Data;
    for external in [true, false] {
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
        r.handle(Event::App(AppCommand::Power { external }), now, &mut |_| {});
        let until = now + Duration::from_secs(2);
        drive(&mut r, &mut now, until);
        let d = Data {
            sender_home: Channel::new(5).unwrap(),
            dst_hop: r.me(),
            src_hop: name(1),
            dst_pos: r.config.position,
            dst_id: r.me(),
            hops: 1,
            payload: b"for me",
        };
        let f = mac::frame(&Packet::Data(d), r.home, MIN_PREAMBLE_SYMBOLS);
        let mut pending = Vec::new();
        let mut early = false;
        r.handle(
            Event::Received {
                channel: Channel::new(3).unwrap(),
                rssi_dbm: -80,
                snr_db: 10,
                bytes: &f.bytes[..f.len as usize],
            },
            now,
            &mut |a| match a {
                Action::App(AppEvent::Message { .. }) => early = true,
                Action::Lbt(_) | Action::Rssi(_) | Action::Transmit(_) => pending.push(a),
                _ => {}
            },
        );
        assert!(!early, "external {external}: not before the ACK");
        assert!(
            matches!(pending.as_slice(), [Action::Transmit(f)] if f.channel == d.sender_home),
            "external {external}: the ACK at once: {pending:?}"
        );
        let mut got = Vec::new();
        let until = now + Duration::from_millis(200);
        drive_all(&mut r, &mut now, until, pending, &mut |_, a| got.push(a));
        assert!(
            got.iter().any(|a| matches!(a, Action::App(AppEvent::Message { msg_id, .. }) if *msg_id == d.msg_id())),
            "external {external}: {got:?}"
        );
    }
}

/// A neighbour's heartbeat due on this channel and not heard is a
/// reading at the floor, with the weight of the share of its window the
/// radio listened through: nearly a whole reading on a quiet channel,
/// less by the share a frame received in the window took; and one heard
/// in its window is a reading of itself and no miss (spec "Heartbeat",
/// "Neighbour table").
#[test]
fn a_heartbeat_due_and_not_heard_is_a_reading_at_the_floor() {
    use crate::node::heartbeat_interval;
    use crate::radio::CHANNELS;
    use crate::wire::Data;
    let config = Config {
        boot_listen: Duration::from_secs(1),
        home_override: Channel::new(3),
        sweep_interval: Duration::from_secs(3600),
        ..Config::default()
    };
    let mut r = Relay::new(config, 1);
    let mut now = Instant::ZERO;
    drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(2));
    let me = r.config.position;
    let f_pos = Position {
        lat: me.lat + 5_000,
        ..me
    };
    let f = id_of(f_pos);
    let home = r.home;
    let hear = |r: &mut Relay, now: &mut Instant, p: &Packet<'_>, rssi: i16| {
        let frame = mac::frame(p, home, MIN_PREAMBLE_SYMBOLS);
        let event = Event::Received {
            channel: home,
            rssi_dbm: rssi,
            snr_db: 10,
            bytes: &frame.bytes[..frame.len as usize],
        };
        let mut pending = Vec::new();
        r.handle(event, *now, &mut |a| pending.push(a));
        let until = *now;
        drive_from(r, now, until, pending);
    };
    let heartbeat = Packet::Heartbeat(Heartbeat {
        sleepy: false,
        anchorless: false,
        carried: false,
        home: Channel::new(5).unwrap(),
        format: crate::wire::FORMAT,
        map_point: false,
        id: f,
        short: f_pos.short(),
    });
    hear(&mut r, &mut now, &heartbeat, -100);
    let entry = |r: &Relay| *r.neighbours.get(f).unwrap();
    let n = entry(&r);
    assert!(n.slotted && n.rssi == -100.0);
    let interval = heartbeat_interval(f, &r.config);
    let rotation = interval.mul(CHANNELS as u64);
    let (from, to) = r.slot_window(&n).unwrap();
    assert!(from < n.beat_at + rotation && n.beat_at + rotation < to);
    assert!(
        r.next_wakeup().unwrap() <= from,
        "awake when the window opens"
    );
    let floor = r.radio.busy.noise_floor_dbm() as f32;
    // The weight the last reading at the floor had, from what it moved.
    let weight = |before: f32, after: f32| 2.0 * (after - before) / (floor - before);

    // A quiet channel: the radio was away only for its own heartbeats.
    drive(&mut r, &mut now, to + Duration::from_millis(1));
    let n = entry(&r);
    let w = weight(-100.0, n.rssi);
    assert!(w > 0.95 && w <= 1.0, "{w}: {} -> {}", -100.0, n.rssi);
    assert_eq!(
        n.beat_at,
        from + to.since(from).div(2) - heartbeat_airtime_half(),
        "the slot passed"
    );

    // A full frame for someone else received in the window: the radio
    // could not have heard a heartbeat under it.
    let before = n.rssi;
    let (from, to) = r.slot_window(&n).unwrap();
    drive(&mut r, &mut now, from + Duration::from_millis(1));
    let payload = [0u8; crate::wire::MAX_PAYLOAD];
    let other = Packet::Data(Data {
        sender_home: home,
        dst_hop: NOBODY,
        src_hop: name(4),
        dst_pos: me,
        dst_id: NOBODY,
        hops: 3,
        payload: &payload,
    });
    let airtime = mac::airtime(&mac::frame(&other, home, MIN_PREAMBLE_SYMBOLS));
    now = from + airtime + Duration::from_millis(1);
    hear(&mut r, &mut now, &other, -90);
    drive(&mut r, &mut now, to + Duration::from_millis(1));
    let n = entry(&r);
    let w = weight(before, n.rssi);
    let share = airtime.as_micros() as f32 / to.since(from).as_micros() as f32;
    assert!(
        (1.0 - share - w).abs() < 0.03,
        "{w} against {} listened",
        1.0 - share
    );

    // Heard in its window: a reading of itself, and no miss.
    let before = n.rssi;
    let (from, to) = r.slot_window(&n).unwrap();
    let slot = from + to.since(from).div(2);
    drive(&mut r, &mut now, slot);
    now = slot + Duration::from_millis(5);
    hear(&mut r, &mut now, &heartbeat, -100);
    let heard = entry(&r).rssi;
    assert_eq!(heard, (before - 100.0) / 2.0);
    drive(&mut r, &mut now, to + Duration::from_secs(1));
    assert_eq!(entry(&r).rssi, heard, "no reading for a heartbeat heard");
}

/// Half a heartbeat's airtime: the window is centred on the slot, so the
/// slot that passed is the window's middle less the airtime's half.
fn heartbeat_airtime_half() -> Duration {
    use crate::radio::time_on_air;
    use crate::wire::HEARTBEAT_LEN;
    time_on_air(HEARTBEAT_LEN as u8, MIN_PREAMBLE_SYMBOLS).div(2)
}

/// Any answer to this relay lifts a demotion (spec "Neighbour table",
/// roadmap D106): a refusal from the neighbour, or a DATA it addresses to
/// this relay. A DATA it addresses to another node, or a heartbeat, does
/// not: neither says it hears us.
#[test]
fn an_answer_to_this_relay_lifts_a_demotion() {
    use crate::wire::{Ack, Data};
    let (mut r, mut now, id, d_pos, _) = handing_to_the_nearer(b"answer");
    let d = id_of(d_pos);
    let demote = |r: &mut Relay, now: Instant| {
        for _ in 0..crate::tables::DEMOTE_AFTER_FAILURES {
            r.neighbours.failed(d, now);
        }
        let (_, n) = r.neighbours.by_id(d).unwrap();
        assert!(r.neighbours.is_demoted(n, now));
    };
    let demoted = |r: &Relay, now: Instant| {
        let (_, n) = r.neighbours.by_id(d).unwrap();
        r.neighbours.is_demoted(n, now)
    };

    demote(&mut r, now);
    let refusal = Ack {
        msg_id: id,
        from: d,
        to: r.me(),
        refused: true,
        wait: 1,
        seen: false,
    };
    hear(&mut r, &mut now, &Packet::Ack(refusal));
    assert!(!demoted(&r, now), "a refusal is an answer");

    let offer = |to: NodeId, payload: &'static [u8]| Data {
        sender_home: Channel::new(5).unwrap(),
        dst_hop: to,
        src_hop: d,
        dst_pos: d_pos,
        dst_id: NOBODY,
        hops: 3,
        payload,
    };
    demote(&mut r, now);
    hear(
        &mut r,
        &mut now,
        &Packet::Data(offer(name(9), b"to another")),
    );
    assert!(
        demoted(&r, now),
        "a DATA to another node says nothing of us"
    );
    let hb = Heartbeat {
        sleepy: false,
        anchorless: false,
        carried: false,
        home: Channel::new(5).unwrap(),
        format: crate::wire::FORMAT,
        map_point: false,
        id: d,
        short: d_pos.short(),
    };
    hear(&mut r, &mut now, &Packet::Heartbeat(hb));
    assert!(demoted(&r, now), "nor does a heartbeat");
    let me = r.me();
    hear(&mut r, &mut now, &Packet::Data(offer(me, b"to us")));
    assert!(!demoted(&r, now), "a DATA to this relay is an answer");
}

/// A refused forward goes round the refuser for the wait it told, to the
/// next candidate at once, not after the ban (spec "Bans", "Next hop",
/// roadmap D110).
#[test]
fn a_refused_forward_goes_round_the_refuser() {
    use crate::tables::budget::BAN;
    use crate::wire::Ack;
    let (mut r, mut now, id, d_pos, e_pos) = handing_to_the_nearer(b"refused");
    let refusal = Ack {
        msg_id: id,
        from: id_of(d_pos),
        to: r.me(),
        refused: true,
        wait: 8,
        seen: false,
    };
    now += Duration::from_millis(5);
    let refused_at = now;
    let mut sent = hear(&mut r, &mut now, &Packet::Ack(refusal));
    if data_to(&sent).is_empty() {
        sent = until_sent(&mut r, &mut now, Vec::new());
    }
    let first = sent
        .iter()
        .find(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Data(_))))
        .unwrap()
        .0;
    assert_eq!(data_to(&sent), [(id_of(e_pos), 2)], "E");
    assert!(first < refused_at + BAN, "at once: {first:?}");
}

/// Routing is a view (spec "Next hop", "Send queue"): a message waiting
/// for its banned next hop keeps nothing of that choice, so a neighbour
/// heard meanwhile that ranks before it takes the message at once, before
/// the ban ends (roadmap D106).
#[test]
fn a_message_waiting_for_a_ban_goes_to_a_better_newcomer_at_once() {
    let (mut r, mut now, far) = with_every_neighbour_banned();
    let ends = now + Duration::from_secs(30);
    let ids: Vec<NodeId> = r.neighbours.iter().map(|n| n.id).collect();
    for &id in &ids {
        r.neighbours.refused(id, ends);
    }
    assert_eq!(offer_data(&mut r, &mut now, far, 1, 2), (Some(0), None));
    let until = now + Duration::from_secs(5);
    let sent = drive(&mut r, &mut now, until);
    assert!(data_to(&sent).is_empty(), "it waits for the ban");
    let at = Position {
        lat: far.lat - 20_000,
        ..far
    };
    let hb = Heartbeat {
        sleepy: false,
        anchorless: false,
        carried: false,
        home: Channel::new(6).unwrap(),
        format: crate::wire::FORMAT,
        map_point: false,
        id: id_of(at),
        short: at.short(),
    };
    let sent = hear(&mut r, &mut now, &Packet::Heartbeat(hb));
    let sent = if data_to(&sent).is_empty() {
        until_sent(&mut r, &mut now, Vec::new())
    } else {
        sent
    };
    assert_eq!(data_to(&sent)[0].0, id_of(at));
    assert!(now < ends, "before the ban ended");
}

/// The application's message passes the same gate as a sleepy node's own
/// (spec "Receiving DATA"): with messages queued for its way on that take
/// longer than a hop's budget to go, the relay gives it back with a wait
/// of at least a ban, and takes none meanwhile (roadmap D106).
#[test]
fn the_application_s_message_is_given_back_while_its_way_on_is_taken() {
    use crate::event::AppEvent;
    let (mut r, mut now, far) = with_every_neighbour_banned();
    let ids: Vec<NodeId> = r.neighbours.iter().map(|n| n.id).collect();
    for &id in &ids {
        r.neighbours.refused(id, now);
        r.neighbours.set_service(id, HOP_WAIT);
    }
    for tag in 1..=2 {
        assert_eq!(offer_data(&mut r, &mut now, far, tag, 2), (Some(0), None));
    }
    // Hold the queued ones at their neighbour: no hand-over is driven.
    let payload = [7u8; 20];
    let send = AppCommand::Send {
        dst_pos: far,
        dst_id: NOBODY,
        payload: &payload,
    };
    assert!(r.accepts_send(now));
    let mut back = None;
    r.handle(Event::App(send), now, &mut |a| {
        if let Action::App(AppEvent::GivenBack { retry_at, .. }) = a {
            back = Some(retry_at);
        }
    });
    let retry_at = back.expect("given back");
    assert!(retry_at >= now + BAN, "{retry_at:?}");
    assert!(!r.accepts_send(now));
    assert_eq!(r.queue.len(), 2, "not taken");
}

/// The gate of the send queue (spec "Send queue"): a message that would
/// wait here longer than a hop's budget behind the messages queued for
/// the same next hop, with no higher count, is refused, told no less than
/// the rest of that neighbour's ban; one with a higher count, or for
/// another next hop, is taken (roadmap D106).
#[test]
fn a_second_message_for_a_banned_neighbour_waits_at_its_sender() {
    use crate::event::DropReason;
    use crate::tables::budget::told;
    use crate::wire::{Ack, Data, MAX_PAYLOAD};
    let (mut r, mut now, far) = with_every_neighbour_banned();
    let ends = now + Duration::from_secs(60);
    let ids: Vec<NodeId> = r.neighbours.iter().map(|n| n.id).collect();
    // Four service times fill a hop's budget.
    let service = HOP_WAIT.div(4);
    for &id in &ids {
        r.neighbours.refused(id, ends);
        r.neighbours.set_service(id, service);
    }
    // The nearer neighbour's own place: the other next hop.
    let near = r
        .neighbours
        .iter()
        .map(|n| n.position)
        .min_by_key(|p| p.lat);
    let near = near.unwrap();
    let offer = |r: &mut Relay, now: &mut Instant, tag: u8, hops: u8, dst: Position| {
        let payload = [tag; MAX_PAYLOAD];
        let d = Data {
            sender_home: Channel::new(5).unwrap(),
            dst_hop: r.me(),
            src_hop: name(1),
            dst_pos: dst,
            dst_id: NOBODY,
            hops,
            payload: &payload,
        };
        let f = mac::frame(&Packet::Data(d), r.home, MIN_PREAMBLE_SYMBOLS);
        let event = Event::Received {
            channel: r.home,
            rssi_dbm: -80,
            snr_db: 10,
            bytes: &f.bytes[..f.len as usize],
        };
        let mut refused = None;
        let mut pending = Vec::new();
        r.handle(event, *now, &mut |a| match a {
            Action::Trace(Trace::Refused { reason, .. }) => refused = Some(reason),
            Action::Lbt(_) | Action::Rssi(_) | Action::Transmit(_) => pending.push(a),
            _ => {}
        });
        let until = *now + Duration::from_millis(100);
        let sent = drive_from(r, now, until, pending);
        let wait = sent
            .iter()
            .find_map(|(_, f)| match Packet::decode(f.payload()) {
                Ok(Packet::Ack(Ack {
                    refused: true,
                    wait,
                    ..
                })) => Some(told(wait)),
                _ => None,
            });
        (refused, wait)
    };
    // Up to four ahead, the wait is within the budget.
    let places = 5;
    for tag in 1..=places {
        assert_eq!(offer(&mut r, &mut now, tag, 2, far), (None, None));
    }
    let (why, wait) = offer(&mut r, &mut now, places + 1, 2, far);
    assert_eq!(why, Some(DropReason::SameHop));
    // The excess, a service time, rounded up, or the rest of the ban,
    // rounded down, whichever is longer.
    let wait = wait.expect("refused, with a wait");
    let ban = ends.since(now);
    assert!(wait >= service || wait + BAN > ban, "{wait:?}");
    assert!(
        wait <= service + service.div(8) + BAN || wait <= ban,
        "{wait:?}"
    );
    assert_eq!(
        offer(&mut r, &mut now, places + 2, 3, far),
        (None, None),
        "higher"
    );
    assert_eq!(
        offer(&mut r, &mut now, places + 3, 2, near),
        (None, None),
        "elsewhere"
    );
    assert_eq!(r.queue.len(), places as usize + 2);
}

/// Senders refused at the gate stand in the way's line (spec "Bans"):
/// with the queue for the next hop a service time past the budget, the
/// first refused is told that service time, until its place is within
/// the budget, and each next one a service time more than the one before;
/// the waits are never shorter than the rest of the neighbour's ban
/// (roadmap D106).
#[test]
fn refusals_at_the_gate_are_told_a_line() {
    use crate::event::DropReason;
    use crate::tables::budget::told;
    use crate::wire::{Ack, Data, MAX_PAYLOAD};
    let (mut r, mut now, far) = with_every_neighbour_banned();
    let ids: Vec<NodeId> = r.neighbours.iter().map(|n| n.id).collect();
    let service = HOP_WAIT.div(4);
    for &id in &ids {
        r.neighbours.refused(id, now);
        r.neighbours.set_service(id, service);
    }
    let offer = |r: &mut Relay, now: &mut Instant, tag: u8| {
        let payload = [tag; MAX_PAYLOAD];
        let d = Data {
            sender_home: Channel::new(5).unwrap(),
            dst_hop: r.me(),
            src_hop: name(u32::from(tag)),
            dst_pos: far,
            dst_id: NOBODY,
            hops: 2,
            payload: &payload,
        };
        let f = mac::frame(&Packet::Data(d), r.home, MIN_PREAMBLE_SYMBOLS);
        let event = Event::Received {
            channel: r.home,
            rssi_dbm: -80,
            snr_db: 10,
            bytes: &f.bytes[..f.len as usize],
        };
        let mut refused = None;
        let mut pending = Vec::new();
        r.handle(event, *now, &mut |a| match a {
            Action::Trace(Trace::Refused { reason, .. }) => refused = Some(reason),
            Action::Lbt(_) | Action::Rssi(_) | Action::Transmit(_) => pending.push(a),
            _ => {}
        });
        let until = *now + Duration::from_millis(100);
        let sent = drive_from(r, now, until, pending);
        let wait = sent
            .iter()
            .find_map(|(_, f)| match Packet::decode(f.payload()) {
                Ok(Packet::Ack(Ack {
                    refused: true,
                    wait,
                    ..
                })) => Some(told(wait)),
                _ => None,
            });
        (refused, wait)
    };
    let places = 5;
    for tag in 1..=places {
        assert_eq!(offer(&mut r, &mut now, tag), (None, None));
    }
    let start = now;
    for (k, tag) in (places + 1..places + 5).enumerate() {
        let (why, wait) = offer(&mut r, &mut now, tag);
        assert_eq!(why, Some(DropReason::SameHop));
        // A hundred milliseconds pass between offers; each is told when
        // its place is within the budget, rounded up to a code.
        let wait = wait.expect("told a wait");
        let excess = service.mul(k as u64 + 1) - now.since(start);
        assert!(
            wait >= excess && wait <= excess + excess.div(8) + BAN,
            "{k}: {wait:?}"
        );
    }
    // A ban on the way on longer than the line is told instead.
    let to = r.in_flight().map(|o| {
        let Ok(Packet::Data(d)) = Packet::decode(o.frame.payload()) else {
            unreachable!()
        };
        d.dst_hop
    });
    let to = to.expect("the first is being handed over");
    let ban = service.mul(20);
    r.neighbours.refused(to, now + ban);
    let (_, wait) = offer(&mut r, &mut now, places + 6);
    let wait = wait.expect("told a wait");
    assert!(wait <= ban && wait + ban.div(8) + BAN > ban, "{wait:?}");
}

/// What E is in [`handing_between`].
#[derive(Clone, Copy, Debug)]
enum Peer {
    Relay,
    /// Carried; `stale` if not heard for its heartbeat interval, which
    /// makes it a last resort (spec "Next hop").
    Carried {
        stale: bool,
    },
    /// Sleepy with an anchor: a last resort.
    Sleepy,
}

/// A running relay with neighbours D and E north-east and north-west of
/// it, E a `peer`, and a forward with a parent to a destination 500
/// counts north of it, D's position being the nearer, addressed to E if
/// `to_e`. The relay, the time, the message, D and E, and the
/// neighbour the forward was first handed to (spec "Next hop").
fn handing_between(peer: Peer, to_e: bool) -> (Relay, Instant, MsgId, NodeId, NodeId, NodeId) {
    let (r, now, id, d, e, sent) = handing_between_with(peer, to_e, |_, _| {});
    let [(first, 2)] = data_to(&sent)[..] else {
        panic!("{:?}", data_to(&sent));
    };
    (r, now, id, d, e, first)
}

/// [`handing_between`] with `prep` done to the relay before the forward is
/// queued; the frames it sent up to the first DATA in place of its
/// receiver.
fn handing_between_with(
    peer: Peer,
    to_e: bool,
    prep: impl FnOnce(&mut Relay, Instant),
) -> (Relay, Instant, MsgId, NodeId, NodeId, Vec<(Instant, Frame)>) {
    use crate::tables::records::Record;
    use crate::tables::send_queue::{Kind, Outgoing};
    use crate::wire::Data;
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
    assert!(!r.booting() && r.placed());
    let me = r.config.position;
    let at = |n: i32, e: i32| Position {
        lat: me.lat + n,
        lon: me.lon + e,
    };
    // About 600 m north and 400 m east and west: D a little nearer.
    let (d_pos, e_pos) = (at(1_000, 900), at(1_000, -1_000));
    let beat = |p: Position, home: u8, peer: Peer| {
        Packet::Heartbeat(Heartbeat {
            sleepy: matches!(peer, Peer::Sleepy),
            anchorless: false,
            carried: matches!(peer, Peer::Carried { .. }),
            home: Channel::new(home).unwrap(),
            format: crate::wire::FORMAT,
            map_point: false,
            id: id_of(p),
            short: p.short(),
        })
    };
    hear(&mut r, &mut now, &beat(e_pos, 7, peer));
    if let Peer::Carried { stale: true } = peer {
        let until = now + crate::node::heartbeat_interval(id_of(e_pos), &r.config);
        drive(&mut r, &mut now, until);
        now = until;
    }
    hear(&mut r, &mut now, &beat(d_pos, 5, Peer::Relay));
    let data = Data {
        sender_home: r.home,
        dst_hop: name(0),
        src_hop: r.me(),
        dst_pos: at(500, 0),
        dst_id: if to_e { id_of(e_pos) } else { NOBODY },
        hops: 2,
        payload: b"gone",
    };
    let id = data.msg_id();
    let parent = Some((name(7), Channel::new(6).unwrap()));
    let place = r.record_place().unwrap();
    r.records.put(place, Record::new(id, parent, 1, now));
    prep(&mut r, now);
    let f = mac::frame(&Packet::Data(data), r.home, MIN_PREAMBLE_SYMBOLS);
    r.queue.push(Outgoing::new(f, Kind::Forward)).unwrap();
    let sent = until_sent(&mut r, &mut now, Vec::new());
    (r, now, id, id_of(d_pos), id_of(e_pos), sent)
}

/// The receiver and preamble of each DATA frame in `sent`.
fn data_preambles(sent: &[(Instant, Frame)]) -> Vec<(NodeId, u16)> {
    sent.iter()
        .filter_map(|(_, f)| match Packet::decode(f.payload()) {
            Ok(Packet::Data(d)) => Some((d.dst_hop, f.preamble_symbols)),
            _ => None,
        })
        .collect()
}

/// `from` answers the hand-over of `id` under way, taking it or, if
/// `seen`, having seen it; the actions that follow at once.
fn answered(r: &mut Relay, now: &mut Instant, id: MsgId, from: NodeId, seen: bool) -> Vec<Action> {
    use crate::wire::Ack;
    let ack = Ack {
        msg_id: id,
        from,
        to: r.me(),
        refused: false,
        wait: 0,
        seen,
    };
    *now += Duration::from_millis(5);
    let f = mac::frame(&Packet::Ack(ack), r.home, MIN_PREAMBLE_SYMBOLS);
    let event = Event::Received {
        channel: r.home,
        rssi_dbm: -80,
        snr_db: 10,
        bytes: &f.bytes[..f.len as usize],
    };
    let mut actions = Vec::new();
    r.handle(event, *now, &mut |a| actions.push(a));
    actions
}

/// Run `r` from `pending` until it sends a DATA frame or drops a
/// message, for at most ten seconds; the receiver of the frame, if any,
/// and the drops it traced.
fn settle(
    r: &mut Relay,
    now: &mut Instant,
    pending: Vec<Action>,
) -> (Option<NodeId>, Vec<crate::event::DropReason>) {
    let mut woken = Vec::new();
    r.handle(Event::Wakeup, *now, &mut |a| woken.push(a));
    let (mut seen, mut all): (Vec<_>, Vec<_>) = pending
        .into_iter()
        .chain(woken)
        .partition(|a| matches!(a, Action::Trace(_)));
    let outcome = |seen: &[Action]| {
        let to = seen.iter().find_map(|a| match a {
            Action::Transmit(f) => match Packet::decode(f.payload()) {
                Ok(Packet::Data(d)) => Some(d.dst_hop),
                _ => None,
            },
            _ => None,
        });
        let drops: Vec<_> = seen
            .iter()
            .filter_map(|a| match a {
                Action::Trace(Trace::Dropped { reason, .. }) => Some(*reason),
                _ => None,
            })
            .collect();
        (to, drops)
    };
    let end = *now + Duration::from_secs(10);
    let mut until = *now;
    loop {
        drive_all(r, now, until, core::mem::take(&mut all), &mut |_, a| {
            seen.push(a)
        });
        let (to, drops) = outcome(&seen);
        if to.is_some() || !drops.is_empty() || until >= end {
            return (to, drops);
        }
        until += Duration::from_millis(10);
    }
}

/// The addressed node goes first where it is a neighbour, though another
/// is nearer its position, and though it is carried and stale; answered
/// seen, the search goes on as before (spec "Next hop").
#[test]
fn the_addressed_node_goes_first() {
    for peer in [
        Peer::Relay,
        Peer::Carried { stale: false },
        Peer::Carried { stale: true },
    ] {
        let (mut r, mut now, id, d, e, first) = handing_between(peer, true);
        assert_eq!(first, e, "{peer:?}");
        let a = answered(&mut r, &mut now, id, e, true);
        let (to, drops) = settle(&mut r, &mut now, a);
        assert_eq!((to, drops), (Some(d), std::vec![]), "{peer:?}");
    }
}

/// A sleepy addressed node with an anchor is sent the message at once with
/// the long preamble while the relay's bucket spares it, though D is
/// nearer its position (spec "Next hop").
#[test]
fn an_addressed_sleepy_node_is_sent_long_at_once() {
    let (_, _, _, _, e, sent) = handing_between_with(Peer::Sleepy, true, |_, _| {});
    assert_eq!(data_preambles(&sent), [(e, LONG_PREAMBLE_SYMBOLS)]);
}

/// The node's channel being busier than the others does not hold the
/// message back: the long frame needs only the relay's bucket (spec
/// "Next hop").
#[test]
fn a_busy_channel_does_not_hold_a_sleepy_node_back() {
    let busy_7 = |r: &mut Relay, _: Instant| {
        // Channel 7, E's, busy one sweep in ten; the others quiet.
        for i in 0..20 {
            for c in Channel::all() {
                let loud = c.index() == 7 && i % 10 == 0;
                r.radio.busy.sample(c, if loud { -60 } else { -125 });
            }
        }
    };
    let (_, _, _, _, e, sent) = handing_between_with(Peer::Sleepy, true, busy_7);
    assert_eq!(data_preambles(&sent), [(e, LONG_PREAMBLE_SYMBOLS)]);
}

/// With a bucket that holds a short DATA beyond the reply reserve but not
/// a full long one, the message goes on toward its anchor's position
/// instead (spec "Next hop", "Duty cycle cap").
#[test]
fn a_spent_bucket_leaves_a_sleepy_node_to_its_anchor() {
    let spend = |r: &mut Relay, now: Instant| {
        let spend = r.radio.budget.available(now) - Duration::from_millis(1_000);
        r.radio.budget.record(spend, now);
    };
    let (_, _, _, d, _, sent) = handing_between_with(Peer::Sleepy, true, spend);
    assert_eq!(data_preambles(&sent), [(d, MIN_PREAMBLE_SYMBOLS)]);
}

/// A hand-over to the sleepy node with the long preamble, unanswered in all
/// its attempts, leaves the message to its anchor: the next DATA goes to
/// the nearer D, and none to the node before it is heard again (spec
/// "Next hop").
#[test]
fn an_unanswered_sleepy_node_is_left_to_its_anchor() {
    let (mut r, mut now, _, d, e, mut sent) = handing_between_with(Peer::Sleepy, true, |_, _| {});
    let end = now + Duration::from_secs(30);
    while !data_to(&sent).iter().any(|&(to, _)| to == d) {
        let until = now + Duration::from_millis(10);
        sent.extend(drive(&mut r, &mut now, until));
        now = now.max(until);
        assert!(now < end, "{:?}", data_preambles(&sent));
    }
    let attempts = usize::from(r.config.hop_attempts);
    let mut want = std::vec![(e, LONG_PREAMBLE_SYMBOLS); attempts];
    want.push((d, MIN_PREAMBLE_SYMBOLS));
    assert_eq!(data_preambles(&sent), want);
    assert!(r.neighbours.get(e).is_some_and(|n| n.unanswered));
}

/// A relay that owns the position of a forward addressed to relay C,
/// C's own heartbeat placing it farther from that position than the
/// relay (spec "Owner"), and so would queue it for C's beat on its
/// channel: what it does in its first two seconds, if it has C in its
/// neighbour table or not. The receiver of a DATA it sent, if any, and
/// whether it queued the message for delivery.
fn owner_with(c_live: bool) -> (Option<NodeId>, bool) {
    use crate::tables::records::Record;
    use crate::tables::send_queue::{Kind, Outgoing};
    use crate::wire::Data;
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
    let me = r.config.position;
    let at = |n: i32| Position {
        lat: me.lat + n,
        lon: me.lon,
    };
    // About 6 m north of the relay, and C 30 m: the relay is nearer.
    let (dst, c_pos) = (at(10), at(50));
    let c = id_of(c_pos);
    if c_live {
        let hb = Heartbeat {
            sleepy: false,
            anchorless: false,
            carried: false,
            home: Channel::new(7).unwrap(),
            format: crate::wire::FORMAT,
            map_point: false,
            id: c,
            short: c_pos.short(),
        };
        hear(&mut r, &mut now, &Packet::Heartbeat(hb));
    }
    let data = Data {
        sender_home: r.home,
        dst_hop: name(0),
        src_hop: r.me(),
        dst_pos: dst,
        dst_id: c,
        hops: 2,
        payload: b"here",
    };
    let id = data.msg_id();
    let parent = Some((name(7), Channel::new(6).unwrap()));
    let place = r.record_place().unwrap();
    r.records.put(place, Record::new(id, parent, 1, now));
    let f = mac::frame(&Packet::Data(data), r.home, MIN_PREAMBLE_SYMBOLS);
    r.queue.push(Outgoing::new(f, Kind::Forward)).unwrap();
    let mut seen = Vec::new();
    let mut woken = Vec::new();
    r.handle(Event::Wakeup, now, &mut |a| woken.push(a));
    let until = now + Duration::from_secs(2);
    let (traces, pending): (Vec<_>, Vec<_>) = woken
        .into_iter()
        .partition(|a| matches!(a, Action::Trace(_)));
    seen.extend(traces);
    drive_all(&mut r, &mut now, until, pending, &mut |_, a| seen.push(a));
    let to = seen.iter().find_map(|a| match a {
        Action::Transmit(f) => match Packet::decode(f.payload()) {
            Ok(Packet::Data(d)) => Some(d.dst_hop),
            _ => None,
        },
        _ => None,
    });
    let queued = seen
        .iter()
        .any(|a| matches!(a, Action::Trace(Trace::Queued { msg_id, .. }) if *msg_id == id));
    (to, queued)
}

/// The owner of a position hands a message to the node it is addressed
/// to at once where that is a live neighbour, and queues it for the
/// node's beat only where it is not (spec "Owner", "Next hop").
#[test]
fn an_owner_hands_to_the_addressed_neighbour_at_once() {
    let me = Relay::new(Config::default(), 1).config.position;
    let c = id_of(Position {
        lat: me.lat + 50,
        lon: me.lon,
    });
    assert_eq!(owner_with(true), (Some(c), false));
    assert_eq!(owner_with(false), (None, true));
}

/// A relay with neighbours at `lats` north of it, each that far nearer
/// a far destination, whose application's message has gone out to the
/// nearest and been refused by it, told a wait of `bans` bans. The
/// relay, the time of the refusal and the frames sent after it.
fn own_refused(lats: &[i32], bans: u8) -> (Relay, Instant, Vec<(Instant, Frame)>) {
    use crate::wire::Ack;
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
    let me = r.config.position;
    let at = |k: i32| Position {
        lat: me.lat + k,
        ..me
    };
    for (i, &k) in lats.iter().enumerate() {
        let hb = Heartbeat {
            sleepy: false,
            anchorless: false,
            carried: false,
            home: Channel::new(5 + i as u8).unwrap(),
            format: crate::wire::FORMAT,
            map_point: false,
            id: id_of(at(k)),
            short: at(k).short(),
        };
        hear(&mut r, &mut now, &Packet::Heartbeat(hb));
    }
    let payload = [7u8; 20];
    let send = AppCommand::Send {
        dst_pos: at(100_000),
        dst_id: NOBODY,
        payload: &payload,
    };
    assert!(r.accepts_send(now));
    let mut pending = Vec::new();
    r.handle(Event::App(send), now, &mut |a| pending.push(a));
    let sent = until_sent(&mut r, &mut now, pending);
    let nearest = id_of(at(*lats.iter().max().unwrap()));
    assert_eq!(data_to(&sent), [(nearest, 1)], "the nearest first");
    let id = sent
        .iter()
        .find_map(|(_, f)| match Packet::decode(f.payload()) {
            Ok(Packet::Data(d)) => Some(d.msg_id()),
            _ => None,
        })
        .unwrap();
    now += Duration::from_millis(5);
    let refusal = Ack {
        msg_id: id,
        from: nearest,
        to: r.me(),
        refused: true,
        wait: bans,
        seen: false,
    };
    let refused_at = now;
    let mut sent = hear(&mut r, &mut now, &Packet::Ack(refusal));
    if data_to(&sent).is_empty() {
        sent = until_sent(&mut r, &mut now, Vec::new());
    }
    (r, refused_at, sent)
}

/// The time of the first DATA in `sent`.
fn first_data(sent: &[(Instant, Frame)]) -> Instant {
    sent.iter()
        .find(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Data(_))))
        .unwrap()
        .0
}

/// The application's message, refused, goes round the refuser to the
/// next neighbour at once, as a forward does (spec "Bans").
#[test]
fn the_application_s_refused_message_goes_round_the_refuser() {
    let (lats, bans) = ([10_000, 5_000], 8);
    let (r, refused_at, sent) = own_refused(&lats, bans);
    let e = Position {
        lat: r.config.position.lat + 5_000,
        ..r.config.position
    };
    assert_eq!(data_to(&sent), [(id_of(e), 1)], "E");
    assert!(first_data(&sent) < refused_at + BAN, "at once");
}

/// With no other neighbour the application's refused message waits for
/// the refuser, on its hold (spec "Bans").
#[test]
fn the_application_s_refused_message_alone_waits_for_the_refuser() {
    use crate::tables::budget::told;
    let bans = 4;
    let (r, refused_at, sent) = own_refused(&[10_000], bans);
    let d = Position {
        lat: r.config.position.lat + 10_000,
        ..r.config.position
    };
    assert_eq!(data_to(&sent), [(id_of(d), 1)], "D again");
    assert!(first_data(&sent) >= refused_at + told(bans));
}

/// The inbox follows the delivery queue's rule (spec "Delivery queue",
/// "Receiving DATA"): a message for this node's application waits until
/// the application confirms it, the oldest first; a full inbox of fresh
/// messages refuses the next with a wait, and once the oldest is stale a
/// newer message takes its place.
#[test]
fn the_inbox_keeps_messages_by_the_delivery_queue_s_rule() {
    use crate::event::{AppEvent, DropReason};
    use crate::tables::delivery::INBOX;
    use crate::wire::Data;
    let (mut r, mut now, _) = with_every_neighbour_banned();
    let me = r.me();
    let at = r.config.position;
    // Offer a message for this node's application; whether it was taken
    // and, refused, why.
    let offer = |r: &mut Relay, now: &mut Instant, tag: u8| {
        let payload = [tag; 8];
        let d = Data {
            sender_home: Channel::new(5).unwrap(),
            dst_hop: me,
            src_hop: name(1),
            dst_pos: at,
            dst_id: me,
            hops: 1,
            payload: &payload,
        };
        let f = mac::frame(&Packet::Data(d), r.home, MIN_PREAMBLE_SYMBOLS);
        let event = Event::Received {
            channel: r.home,
            rssi_dbm: -80,
            snr_db: 10,
            bytes: &f.bytes[..f.len as usize],
        };
        let (mut refused, mut arrived) = (None, false);
        let mut pending = Vec::new();
        r.handle(event, *now, &mut |a| match a {
            Action::Trace(Trace::Refused { reason, .. }) => refused = Some(reason),
            Action::App(AppEvent::Message { msg_id }) if msg_id == d.msg_id() => arrived = true,
            Action::Lbt(_) | Action::Rssi(_) | Action::Transmit(_) => pending.push(a),
            _ => {}
        });
        let until = *now + Duration::from_millis(100);
        let mut taken = arrived;
        let start = *now;
        drive_all(r, now, until, pending, &mut |_, a| {
            taken |= matches!(a, Action::App(AppEvent::Message { msg_id }) if msg_id == d.msg_id());
        });
        *now = (*now).max(start + Duration::from_secs(2));
        (d.msg_id(), taken, refused)
    };
    let mut ids = Vec::new();
    for tag in 0..INBOX as u8 {
        let (id, taken, refused) = offer(&mut r, &mut now, tag);
        assert!(taken && refused.is_none(), "message {tag}");
        ids.push(id);
    }
    assert_eq!(r.inbox_len(), INBOX);
    let (_, taken, refused) = offer(&mut r, &mut now, 100);
    assert!(!taken);
    assert_eq!(refused, Some(DropReason::DeliveryFull));
    // Only the oldest is confirmed.
    assert!(!r.confirm(ids[1]));
    assert_eq!(r.inbox().map(|(id, p)| (id, p[0])), Some((ids[0], 0)));
    assert!(r.confirm(ids[0]));
    let (_, taken, _) = offer(&mut r, &mut now, 101);
    assert!(taken, "a confirmed message makes room");
    // Once the oldest is stale, a newer message takes its place.
    now += r.config.delivery_ttl;
    let (_, taken, refused) = offer(&mut r, &mut now, 102);
    assert!(taken && refused.is_none());
    assert_eq!(r.inbox_len(), INBOX);
    assert_eq!(r.inbox().map(|(id, _)| id), Some(ids[2]));
}

/// The counts a person sees: every live neighbour heard, those with
/// margin as strong, and as both ways only one that has taken a message.
#[test]
fn neighbour_counts_tell_heard_strong_and_proven() {
    let hour = Duration::from_secs(3600);
    let config = Config {
        boot_listen: Duration::from_secs(1),
        sweep_interval: hour,
        heartbeat_min: hour,
        heartbeat_max: hour,
        ..Config::default()
    };
    let mut r = Relay::new(config, 1);
    let mut now = Instant::ZERO;
    drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(2));
    let me = r.config.position;
    let margin = Neighbours::margin_dbm(r.radio.busy.noise_floor_dbm());
    let mut ids = Vec::new();
    for (k, rssi) in [
        (1, margin as i16 + 10),
        (2, margin as i16 + 10),
        (3, margin as i16 - 10),
    ] {
        let at = Position {
            lat: me.lat + 5_000 * k,
            ..me
        };
        let hb = Heartbeat {
            sleepy: false,
            anchorless: false,
            carried: false,
            home: Channel::new(5).unwrap(),
            format: crate::wire::FORMAT,
            map_point: false,
            id: id_of(at),
            short: at.short(),
        };
        r.neighbours.heard(&hb, at, rssi, &r.plane, now);
        ids.push(hb.id);
    }
    let counts = r.neighbour_counts(now);
    assert_eq!((counts.heard, counts.strong, counts.both_ways), (3, 2, 0));
    r.neighbours.took(ids[2], now, now + Duration::from_secs(1));
    let counts = r.neighbour_counts(now);
    assert_eq!((counts.heard, counts.strong, counts.both_ways), (3, 2, 1));
}

/// A node that turns sleepy hands what it held on into its send queue as
/// forwards, admitted as any forward with its count; one that finds no
/// room waits in the delivery queue until room comes, never dropped
/// (spec "Sleepy nodes"). It goes on with no parent and not arrived, so
/// a copy that comes back is searched on.
#[test]
fn what_a_node_held_waits_for_room_when_it_turns_sleepy() {
    use crate::ids::MsgId;
    use crate::tables::delivery::Queued;
    use crate::tables::send_queue::{Kind, Outgoing, SEND_QUEUE};
    use crate::wire::{Data, Packet};
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
    now = Instant::ZERO + Duration::from_secs(10);
    let node = NodeId([7; 6]);
    let dst = r.config.position;
    // Held with count 0, its node's own: no hop place takes it.
    let id = MsgId::of(dst, node, b"held");
    let mut record = crate::tables::records::Record::new(id, None, 0, now);
    record.set_arrived();
    let at = r.record_place().unwrap();
    r.records.put(at, record);
    r.delivery
        .push(Queued::new(id, dst, node, b"held", now).unwrap());
    // The regular room full of others' forwards.
    for k in 0..SEND_QUEUE {
        let payload = [k as u8];
        let d = Data {
            sender_home: r.home,
            dst_hop: NodeId::NONE,
            src_hop: r.me(),
            dst_pos: dst,
            dst_id: NodeId([9; 6]),
            hops: 1,
            payload: &payload,
        };
        let frame = mac::frame(&Packet::Data(d), r.home, MIN_PREAMBLE_SYMBOLS);
        r.queue.push(Outgoing::new(frame, Kind::Forward)).unwrap();
    }
    r.mode = Mode::Sleepy(Sleep::new(false));
    let mut traces = Vec::new();
    let mut emit = |a: Action| {
        if let Action::Trace(t) = a {
            traces.push(t);
        }
    };
    r.hand_on(now, &mut emit);
    assert_eq!(r.delivery.len(), 1, "it waits for room");
    assert_eq!(r.queue.len(), SEND_QUEUE);
    r.queue.remove(0);
    r.hand_on(now, &mut emit);
    assert!(r.delivery.is_empty());
    let out = r.queue.iter().last().unwrap();
    assert_eq!(
        (out.kind, out.place, out.msg_id()),
        (Kind::Forward, 0, Some(id))
    );
    assert!(matches!(Packet::decode(out.frame.payload()), Ok(Packet::Data(d)) if d.hops == 1));
    let record = r.records.get(id).unwrap();
    assert!(!record.arrived() && record.parent().is_none());
    assert!(traces.contains(&Trace::HandedOn { msg_id: id }));
    assert!(!traces.iter().any(|t| matches!(t, Trace::Dropped { .. })));
}

/// A relay on channel 3 that runs image version 9 and hears two
/// neighbours, A, a relay at home on channel 5, and S, a sleepy node at
/// home on channel 7 (spec "Neighbour status").
fn with_two_neighbours() -> (Relay, Instant, NodeId, NodeId) {
    let hour = Duration::from_secs(3600);
    let config = Config {
        boot_listen: Duration::from_secs(1),
        home_override: Channel::new(3),
        sweep_interval: hour,
        heartbeat_min: hour,
        heartbeat_max: hour,
        version: 9,
        ..Config::default()
    };
    let mut r = Relay::new(config, 1);
    let mut now = Instant::ZERO;
    drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(2));
    let me = r.config.position;
    let mut ids = [NOBODY; 2];
    for (i, (k, home, sleepy)) in [(1, 5, false), (2, 7, true)].into_iter().enumerate() {
        let at = Position {
            lat: me.lat + 5_000 * k,
            ..me
        };
        let hb = Heartbeat {
            sleepy,
            anchorless: false,
            carried: false,
            home: Channel::new(home).unwrap(),
            format: crate::wire::FORMAT,
            map_point: false,
            id: id_of(at),
            short: at.short(),
        };
        hear(&mut r, &mut now, &Packet::Heartbeat(hb));
        ids[i] = hb.id;
    }
    (r, now, ids[0], ids[1])
}

/// The status frames in `sent`, with their channels and preambles.
fn status_frames(sent: &[(Instant, Frame)]) -> Vec<(u8, u16, crate::wire::StatusFrame)> {
    sent.iter()
        .filter_map(|(_, f)| match Packet::decode(f.payload()) {
            Ok(Packet::Status(s)) => Some((f.channel.index(), f.preamble_symbols, s)),
            _ => None,
        })
        .collect()
}

/// A node's status tells the image it gathers: its version and blocks,
/// the blocks it holds from the first and the pieces of the next
/// (spec "Neighbour status").
#[test]
fn a_status_tells_the_image_gathered() {
    use crate::status::Gathering;
    use crate::update::{Image, ImageTag};
    let (mut r, now, _, _) = with_two_neighbours();
    assert_eq!(r.status(now).gathering, Gathering::default());
    let image = Image {
        name: ImageTag([3; 4]),
        version: 14,
        blocks: 31,
        last: 20,
        key: 0,
    };
    let gather = AppCommand::Gather { image, from: 14 };
    r.handle(Event::App(gather), now, &mut |_| {});
    assert_eq!(
        r.status(now).gathering,
        Gathering {
            version: 14,
            blocks: 31,
            complete: 14,
            have: 0,
        }
    );
}

/// A node asked for its status tells it once, on the asker's home
/// channel, with what its screen shows (spec "Neighbour status").
#[test]
fn a_status_asked_is_told_on_the_asker_s_channel() {
    use crate::status::{Battery, Status};
    use crate::wire::StatusFrame::{Ask, Tell};
    let (mut r, mut now, a, _) = with_two_neighbours();
    let me = r.me();
    let sent = hear(&mut r, &mut now, &Packet::Status(Ask { to: me, from: a }));
    let status = Status {
        placed: true,
        sleepy: false,
        anchored: false,
        carried: false,
        battery: Battery::NotRead,
        neighbours: NeighbourCounts {
            heard: 2,
            strong: 2,
            both_ways: 0,
        },
        version: 9,
        gathering: Default::default(),
    };
    let told = Tell {
        to: a,
        from: r.me(),
        status,
    };
    assert_eq!(
        status_frames(&sent),
        [(5, MIN_PREAMBLE_SYMBOLS, told)],
        "told once, on A's channel"
    );
    assert_eq!(r.status(now), status);
    let later = now + Duration::from_secs(10);
    assert!(status_frames(&drive(&mut r, &mut now, later)).is_empty());
}

/// A status asked of another node, or by a node this one does not hear,
/// goes unanswered.
#[test]
fn a_status_asked_of_another_node_is_not_told() {
    use crate::wire::StatusFrame::Ask;
    let (mut r, mut now, a, _) = with_two_neighbours();
    let asks = [
        Ask { to: a, from: a },
        Ask {
            to: r.me(),
            from: NOBODY,
        },
    ];
    for ask in asks {
        let mut sent = hear(&mut r, &mut now, &Packet::Status(ask));
        let later = now + Duration::from_secs(10);
        sent.extend(drive(&mut r, &mut now, later));
        assert!(status_frames(&sent).is_empty(), "{ask:?}");
    }
}

/// The application asks a neighbour, the sleepy one with the long
/// preamble on its channel, and is handed the answer addressed to this
/// node, not one to another.
#[test]
fn the_application_asks_and_hears_the_answer() {
    use crate::status::{Battery, Status};
    use crate::wire::StatusFrame::{Ask, Tell};
    let (mut r, mut now, a, s) = with_two_neighbours();
    let mut pending = Vec::new();
    for neighbour in [s, NOBODY] {
        let ask = AppCommand::Ask { neighbour };
        r.handle(Event::App(ask), now, &mut |x| pending.push(x));
    }
    let until = now;
    let sent = drive_from(&mut r, &mut now, until, pending);
    let asked = Ask {
        to: s,
        from: r.me(),
    };
    assert_eq!(status_frames(&sent), [(7, LONG_PREAMBLE_SYMBOLS, asked)]);
    let status = Status {
        sleepy: true,
        anchored: true,
        battery: Battery::Percent(57),
        version: 8,
        ..Status::default()
    };
    let mut told = Vec::new();
    for to in [r.me(), a] {
        let f = mac::frame(
            &Packet::Status(Tell {
                to,
                from: s,
                status,
            }),
            r.home,
            LONG_PREAMBLE_SYMBOLS,
        );
        let event = Event::Received {
            channel: r.home,
            rssi_dbm: -80,
            snr_db: 10,
            bytes: f.payload(),
        };
        r.handle(event, now, &mut |x| {
            if let Action::App(e) = x {
                told.push(e)
            }
        });
    }
    assert_eq!(told, [crate::event::AppEvent::Status { from: s, status }]);
}

/// The battery as the status tells it: USB on external power, else the
/// charge of the last reading, not read before the first, low at or
/// below the low level.
#[test]
fn the_status_tells_the_battery() {
    use crate::status::Battery;
    let (mut r, now, _, _) = with_two_neighbours();
    let cmd = |r: &mut Relay, c: AppCommand<'_>| r.handle(Event::App(c), now, &mut |_| {});
    assert_eq!(r.status(now).battery, Battery::NotRead);
    cmd(&mut r, AppCommand::Battery { millivolts: 3900 });
    let p = crate::node::battery_percent(3900).unwrap();
    assert_eq!(r.status(now).battery, Battery::Percent(p));
    cmd(&mut r, AppCommand::Battery { millivolts: 3700 });
    assert_eq!(r.status(now).battery, Battery::Low);
    cmd(&mut r, AppCommand::Power { external: true });
    assert_eq!(r.status(now).battery, Battery::Usb);
}

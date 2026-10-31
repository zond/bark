//! The node driven on the host: the sizes, the first rotation, the
//! sleepy side, admission and the hand-over.

extern crate std;
use super::*;
use crate::event::{Frame, Trace};
use crate::node::mac;
use crate::tables::budget::BAN;
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
    std::println!("Relay {relay} B, tables {tables} B, kept {kept} B");
    assert!(relay <= tables + 1024 + kept, "{relay}");
    // It fits the board's RAM: 255 KiB for the application
    // (bark-image/src/layout.rs), less the BLE controller's buffer, under 5 KiB
    // (docs/implementation-research-2026-10-14.md), and Embassy and the
    // stacks, about 16 KiB (spec "Implementation notes").
    let ram = (255 - 5 - 16) * 1024;
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
    mut pending: Vec<Action>,
) -> Vec<(Instant, Frame)> {
    let mut sent = Vec::new();
    loop {
        let event = match pending.pop() {
            Some(Action::Lbt(channel)) => Event::LbtDone {
                channel,
                busy: false,
                dbm: -130,
            },
            Some(Action::Rssi(channel)) => Event::RssiSample { channel, dbm: -130 },
            Some(Action::Transmit(f)) => {
                sent.push((*now, f));
                *now += mac::airtime(&f);
                Event::TxDone
            }
            Some(_) => continue,
            None => match r.next_wakeup() {
                Some(t) if t <= until => {
                    *now = (*now).max(t);
                    Event::Wakeup
                }
                _ => return sent,
            },
        };
        r.handle(event, *now, &mut |a| match a {
            Action::Lbt(_) | Action::Rssi(_) | Action::Transmit(_) => pending.push(a),
            _ => {}
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
        time_left: u8::MAX,
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
    let q = Queued::new(id, dst, node, b"later", u8::MAX, now).unwrap();
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
            time_left: u8::MAX,
            payload: &payload,
        };
        let frame = mac::frame(&Packet::Data(d), r.home, MIN_PREAMBLE_SYMBOLS);
        queued = queued + mac::airtime(&frame);
        let out = Outgoing {
            not_before: now + hour,
            expires: now + hour,
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
        time_left: u8::MAX,
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
    let q = Queued::new(id, dst, node, b"ahead", u8::MAX, now).unwrap();
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

/// Two packets arrive within one reply time. The second finds the one
/// owed-reply place holding the first one's ACK, which waits out a
/// busy channel: it is neither taken nor refused, and gets no answer.
/// Its sender's retry, after the first ACK is gone, is taken (roadmap
/// D29 rule 4).
#[test]
fn a_packet_that_cannot_be_answered_is_not_taken() {
    use crate::event::AppEvent;
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
    let (a_home, b_home) = (Channel::new(5).unwrap(), Channel::new(6).unwrap());
    let packet = |home: Channel, from: u32, text: &'static [u8]| Data {
        sender_home: home,
        dst_hop: r.me(),
        src_hop: name(from),
        dst_pos: r.config.position,
        dst_id: r.me(),
        hops: 1,
        time_left: u8::MAX,
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

    now = Instant::ZERO + Duration::from_secs(10);
    let mut actions = Vec::new();
    r.handle(received(&a_bytes), now, &mut |x| actions.push(x));
    assert!(taken(&actions, &a));
    assert!(actions.contains(&Action::Lbt(a_home)), "{actions:?}");
    // A's channel is busy: its ACK waits a backoff.
    now += Duration::from_millis(1);
    r.handle(
        Event::LbtDone {
            channel: a_home,
            busy: true,
            dbm: -130,
        },
        now,
        &mut |_| {},
    );
    assert!(r.radio.replies.pending());

    now += Duration::from_millis(1);
    actions.clear();
    r.handle(received(&b_bytes), now, &mut |x| actions.push(x));
    assert!(actions.is_empty(), "{actions:?}");
    assert!(r.records.get(b.msg_id()).is_none(), "B's packet was taken");

    let until = now + Duration::from_millis(100);
    let sent = drive(&mut r, &mut now, until);
    assert_eq!(acks(&sent, &a, a_home), 1);
    assert_eq!(sent.len(), 1, "B's packet was answered");

    // B's retry, after the retry wait (spec "Retry wait").
    now += Duration::from_millis(200);
    actions.clear();
    r.handle(received(&b_bytes), now, &mut |x| actions.push(x));
    assert!(taken(&actions, &b));
    let until = now + Duration::from_millis(100);
    let sent = drive_from(&mut r, &mut now, until, actions);
    assert_eq!(acks(&sent, &b, b_home), 1);
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
/// when its frame leaves (roadmap D31).
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
            src_hop: name(1),
            dst_pos: far,
            dst_id: NOBODY,
            hops,
            time_left: u8::MAX,
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
            time_left: u8::MAX,
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
            time_left: u8::MAX,
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
        time_left: u8::MAX,
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
            time_left: u8::MAX,
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
    let (place, refused, _) = offer_timed(r, now, dst, tag, hops, u8::MAX);
    (place, refused)
}

/// [`offer_data`] with `time_left` units left; also returns why the
/// message was dropped, if it was.
fn offer_timed(
    r: &mut Relay,
    now: &mut Instant,
    dst: Position,
    tag: u8,
    hops: u8,
    time_left: u8,
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
        time_left,
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
    let until = *now + Duration::from_millis(100);
    drive_from(r, now, until, pending);
    let place = r
        .queue
        .iter()
        .find(|o| o.msg_id() == Some(d.msg_id()))
        .map(|o| o.place);
    (place, refused, dropped)
}

/// A packet whose time is gone, one unit left that the time on air has
/// eaten into, is no work: taken, not refused, and the message ends
/// here. With two units it is queued (spec "Receiving DATA").
#[test]
fn a_packet_whose_time_is_gone_is_taken_and_ends() {
    use crate::event::DropReason;
    let (mut r, mut now, far) = with_every_neighbour_banned();
    assert_eq!(
        offer_timed(&mut r, &mut now, far, 1, 2, 1),
        (None, None, Some(DropReason::Expired))
    );
    assert_eq!(
        offer_timed(&mut r, &mut now, far, 2, 2, 2),
        (Some(0), None, None)
    );
}

/// A message waiting here carries its time as it is at each attempt,
/// rounded down, and is dropped once no whole unit is left, waiting for
/// a ban or not; a hop place frees with it (spec "Time to live").
#[test]
fn a_waiting_message_ends_with_its_time() {
    use crate::wire::{Data, TTL_UNIT};
    let (mut r, start, far) = with_every_neighbour_banned();
    let mut now = start;
    let units = 8;
    let (place, _, _) = offer_timed(&mut r, &mut now, far, 1, 2, units);
    assert_eq!(place, Some(0));
    // At least the time on air of the frame is gone already.
    let deadline = start + TTL_UNIT.mul(units as u64);
    // The neighbours do not answer: the message waits for their bans.
    let sent = drive(&mut r, &mut now, deadline);
    let offers: Vec<_> = sent
        .iter()
        .filter_map(|(t, f)| match Packet::decode(f.payload()) {
            Ok(Packet::Data(d)) => Some((*t, d.time_left)),
            _ => None,
        })
        .collect();
    assert!(offers.len() >= 3, "{offers:?}");
    for (t, left) in &offers {
        assert!(*left >= 1, "sent with no time left");
        assert!(
            *left <= Data::units(deadline.since(*t)),
            "{left} units at {t:?}"
        );
    }
    assert!(r.holding().next().is_none(), "kept past its time");
    assert!(offers.iter().all(|(t, _)| *t < deadline - TTL_UNIT));

    // In a hop place, the same.
    let spend = r.radio.budget.available(now) - Duration::from_millis(300);
    r.radio.budget.record(spend, now);
    let (place, _, _) = offer_timed(&mut r, &mut now, far, 2, 2, 3);
    assert_eq!(place, Some(2));
    let until = now + TTL_UNIT.mul(2);
    drive(&mut r, &mut now, until);
    assert!(r.holding().next().is_none());
    assert_eq!(r.lowest_free_hop_place(), Some(1));
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
    assert_eq!(offer_data(&mut r, &mut now, far, 2, 2), (Some(2), None));
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
        time_left: u8::MAX,
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
            time_left: u8::MAX,
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
        time_left: u8::MAX,
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
        time_left: u8::MAX,
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
        time_left: u8::MAX,
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
/// the frames it sent.
fn until_sent(r: &mut Relay, now: &mut Instant, pending: Vec<Action>) -> Vec<(Instant, Frame)> {
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
        time_left: u8::MAX,
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

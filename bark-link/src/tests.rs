extern crate std;

use super::*;
use bark_proto::Duration;
use proptest::prelude::*;

fn pos() -> impl Strategy<Value = Position> {
    (-(1i32 << 24)..(1 << 24), -(1i32 << 25)..(1 << 25))
        .prop_map(|(lat, lon)| Position { lat, lon })
}

fn id() -> impl Strategy<Value = NodeId> {
    any::<[u8; 6]>().prop_map(NodeId)
}

fn payload() -> impl Strategy<Value = std::vec::Vec<u8>> {
    proptest::collection::vec(any::<u8>(), 0..=MAX_PAYLOAD)
}

proptest! {
    #[test]
    fn node_round_trips(version: u8, oldest: u8, id in id(), image: u32, room: u8) {
        let n = Node { version, oldest, id, image, room };
        prop_assert_eq!(Node::decode(&n.encode()), Ok(n));
    }

    #[test]
    fn status_round_trips(flags: u8, address in pos(), battery_mv: u16, relays: u8, heard: u8, boot_left_s: u16, installing: u16, inbox: u8) {
        let bit = |b: u8| flags & (1 << b) != 0;
        let placed = bit(5);
        let s = Status {
            ready: bit(0), external_power: bit(1), sleepy: bit(2), carried: bit(3),
            map_point: bit(4), placed, booting: bit(6), anchorless: bit(7),
            address: if placed { address } else { Position::default() },
            battery_mv, relays, heard, boot_left_s, installing, inbox,
        };
        let b = s.encode();
        prop_assert_eq!(b[0], flags);
        prop_assert_eq!(Status::decode(&b), Ok(s));
    }

    #[test]
    fn send_round_trips(dst_pos in pos(), dst_id in id(), payload in payload()) {
        let s = Send { dst_pos, dst_id, payload: &payload };
        let mut b = [0; Send::MAX_LEN];
        let n = s.encode(&mut b);
        prop_assert_eq!(n, Send::HEADER + payload.len());
        prop_assert_eq!(Send::decode(&b[..n]), Ok(s));
    }

    #[test]
    fn message_round_trips(id: u32, payload in payload()) {
        let m = Message { msg_id: MsgId(id), payload: &payload };
        let mut b = [0; Message::MAX_LEN];
        let n = m.encode(&mut b);
        prop_assert_eq!(Message::decode(&b[..n]), Ok(Some(m)));
    }

    #[test]
    fn confirm_and_given_back_round_trip(id: u32, wait_s: u16) {
        let c = Confirm(MsgId(id));
        prop_assert_eq!(Confirm::decode(&c.encode()), Ok(c));
        let g = GivenBack { msg_id: MsgId(id), wait_s };
        prop_assert_eq!(GivenBack::decode(&g.encode()), Ok(Some(g)));
    }

    #[test]
    fn clock_round_trips(utc_s: u32, offset_min in OFFSETS_MIN, source in 0u8..3) {
        let c = Clock { utc_s, offset_min };
        prop_assert_eq!(Clock::decode(&c.encode()), Ok(c));
        let source = [TimeSource::None, TimeSource::Gps, TimeSource::Phone][source as usize];
        let t = NodeTime { utc_s: if source == TimeSource::None { 0 } else { utc_s }, offset_min, source };
        prop_assert_eq!(NodeTime::decode(&t.encode()), Ok(t));
    }

    #[test]
    fn neighbours_round_trip(
        floor_dbm: i8, next: u8, own in pos(),
        page in proptest::collection::vec((id(), pos(), any::<i16>(), any::<u16>(), any::<u8>()), 0..=neighbours::PER_PAGE),
    ) {
        let h = neighbours::Header { floor_dbm, next, own };
        let page: std::vec::Vec<_> = page
            .into_iter()
            .map(|(id, position, rssi_ddbm, age_s, f)| {
                let bit = |b: u8| f & (1 << b) != 0;
                neighbours::Neighbour {
                    id, position, rssi_ddbm, age_s,
                    sleepy: bit(0), map_point: bit(1), demoted: bit(2), banned: bit(3),
                    anchor: bit(4), carried: bit(5), anchorless: bit(6), proven: bit(7),
                }
            })
            .collect();
        let mut b = [0; neighbours::MAX_LEN];
        let n = neighbours::encode(&h, &page, &mut b);
        prop_assert_eq!(n, neighbours::HEADER + page.len() * neighbours::ENTRY);
        let (got, read) = neighbours::decode(&b[..n]).unwrap();
        prop_assert_eq!(got, h);
        prop_assert_eq!(read.collect::<std::vec::Vec<_>>(), page);
    }

    #[test]
    fn map_point_round_trips(p in pos()) {
        let mut b = [0; MapPoint::MAX_LEN];
        for m in [MapPoint::Clear, MapPoint::Set(p)] {
            let n = m.encode(&mut b);
            prop_assert_eq!(MapPoint::decode(&b[..n]), Ok(m));
        }
    }
}

/// The bytes an app writes and reads, spelled out, for an app written in
/// another language to test against.
#[test]
fn vectors() {
    let kyiv = Position::from_degrees(50.45, 30.52);
    assert_eq!(kyiv.encode(), [0x47, 0xc0, 0x49, 0x0a, 0xd9, 0xff, 0x80]);
    let to = NodeId([1, 2, 3, 4, 5, 6]);
    let send = Send {
        dst_pos: kyiv,
        dst_id: to,
        payload: b"hello",
    };
    let mut b = [0; Send::MAX_LEN];
    let n = send.encode(&mut b);
    assert_eq!(
        &b[..n],
        [
            0x47, 0xc0, 0x49, 0x0a, 0xd9, 0xff, 0x80, 1, 2, 3, 4, 5, 6, b'h', b'e', b'l', b'l',
            b'o'
        ]
    );
    assert_eq!(send.msg_id(), MsgId::of(kyiv, to, b"hello"));
    assert_eq!(send.msg_id(), MsgId(0x357b_6010));

    let status = Status {
        ready: true,
        external_power: true,
        placed: true,
        booting: true,
        relays: 3,
        boot_left_s: 100,
        address: kyiv,
        inbox: 2,
        heard: 5,
        installing: 0x0118,
        ..Status::default()
    };
    assert_eq!(
        status.encode(),
        [0x63, 0x47, 0xc0, 0x49, 0x0a, 0xd9, 0xff, 0x80, 0, 0, 3, 5, 0, 100, 0x01, 0x18, 2]
    );

    let given = GivenBack {
        msg_id: MsgId(0x357b_6010),
        wait_s: 200,
    };
    assert_eq!(given.encode(), [0x35, 0x7b, 0x60, 0x10, 0, 200]);

    let node = Node {
        version: VERSION,
        oldest: OLDEST,
        id: to,
        image: 7,
        room: 3,
    };
    assert_eq!(node.encode(), [1, 1, 1, 2, 3, 4, 5, 6, 0, 0, 0, 7, 3]);

    // 2026-11-05 12:35:19 UTC, at UTC+01:00 and UTC−03:30.
    let clock = Clock {
        utc_s: 1_793_882_119,
        offset_min: 60,
    };
    assert_eq!(clock.encode(), [0x6a, 0xec, 0x78, 0x07, 0, 60]);
    let west = Clock {
        offset_min: -210,
        ..clock
    };
    assert_eq!(west.encode(), [0x6a, 0xec, 0x78, 0x07, 0xff, 0x2e]);
    let time = NodeTime {
        utc_s: 1_793_882_119,
        offset_min: 60,
        source: TimeSource::Gps,
    };
    assert_eq!(time.encode(), [0x6a, 0xec, 0x78, 0x07, 0, 60, 1]);
    let none = NodeTime {
        source: TimeSource::None,
        ..time
    };
    assert_eq!(none.encode(), [0, 0, 0, 0, 0, 60, 0]);
    assert_eq!(&name(to), b"bark-010203040506");

    // The app's vector: a sleepy node anchored, placed, at 57 %, hearing
    // 5, 3 strong, 2 proven, running version 0x01020304, gathering
    // version 14 of 31 blocks, holding 14 and 40 pieces of the next.
    assert_eq!(Ask(to).encode(), [1, 2, 3, 4, 5, 6]);
    let answer = Answer {
        from: to,
        status: bark_proto::status::Status {
            placed: true,
            sleepy: true,
            anchored: true,
            carried: false,
            battery: bark_proto::status::Battery::Percent(57),
            neighbours: bark_proto::NeighbourCounts {
                heard: 5,
                strong: 3,
                both_ways: 2,
            },
            version: 0x0102_0304,
            gathering: bark_proto::status::Gathering {
                version: 14,
                blocks: 31,
                complete: 14,
                have: 40,
            },
        },
    };
    assert_eq!(
        answer.encode(),
        [1, 2, 3, 4, 5, 6, 0x07, 57, 5, 3, 2, 1, 2, 3, 4, 0, 0, 0, 14, 31, 14, 40]
    );
    assert_eq!(Answer::decode(&answer.encode()), Ok(Some(answer)));
    // A notification's 20 bytes at the default MTU, and the least it may
    // carry: gathering none.
    let gathers_none = Answer {
        status: bark_proto::status::Status {
            gathering: Default::default(),
            ..answer.status
        },
        ..answer
    };
    for cut in [15, 20] {
        assert_eq!(
            Answer::decode(&answer.encode()[..cut]),
            Ok(Some(gathers_none))
        );
    }
}

#[test]
fn uuids_are_one_base_counted() {
    let all = [
        uuid::SERVICE,
        uuid::NODE,
        uuid::STATUS,
        uuid::SEND,
        uuid::INBOX,
        uuid::GIVEN_BACK,
        uuid::MAP_POINT,
        uuid::CLOCK,
        uuid::HOME,
        uuid::NEIGHBOURS,
        uuid::UPDATE,
        uuid::IMAGE,
        uuid::RECIPIENT,
        uuid::PUBLIC,
        uuid::GUEST,
        uuid::ASK,
        uuid::PHONES,
    ];
    for (i, u) in all.into_iter().enumerate() {
        assert_eq!(u & !(0xffff << 96), uuid::SERVICE & !(0xffff << 96));
        assert_eq!((u >> 96) & 0xffff, i as u128 + 1);
    }
    // Version 4, the RFC 4122 variant.
    assert_eq!((uuid::SERVICE >> 76) & 0xf, 4);
    assert_eq!((uuid::SERVICE >> 62) & 0b11, 0b10);
    assert_eq!(le_bytes(uuid::SERVICE)[15], 0x28);
    assert_eq!(le_bytes(uuid::SERVICE)[0], 0x44);
}

#[test]
fn writes_are_taken_only_whole() {
    assert_eq!(Send::decode(&[0; 12]), Err(Malformed::Length));
    assert!(Send::decode(&[0; 13]).is_ok());
    assert!(Send::decode(&[0; Send::MAX_LEN]).is_ok());
    assert_eq!(
        Send::decode(&[0; Send::MAX_LEN + 1]),
        Err(Malformed::Length)
    );
    assert_eq!(Confirm::decode(&[0; 5]), Err(Malformed::Length));
    assert_eq!(MapPoint::decode(&[]), Err(Malformed::Length));
    assert_eq!(MapPoint::decode(&[0, 0]), Err(Malformed::Length));
    assert_eq!(MapPoint::decode(&[1; 7]), Err(Malformed::Length));
    assert_eq!(MapPoint::decode(&[2]), Err(Malformed::Value));
    assert_eq!(Clock::decode(&[0; 5]), Err(Malformed::Length));
    assert_eq!(Clock::decode(&[0; 7]), Err(Malformed::Length));
    assert_eq!(
        Clock::decode(&[0, 0, 0, 0, 0x03, 0x49]),
        Err(Malformed::Value)
    );
    assert_eq!(
        Clock::decode(&[0, 0, 0, 0, 0x03, 0x48]).map(|c| c.offset_min),
        Ok(840)
    );
    assert_eq!(
        Clock::decode(&[0, 0, 0, 0, 0xfd, 0x30]).map(|c| c.offset_min),
        Ok(-720)
    );
    assert_eq!(
        Clock::decode(&[0, 0, 0, 0, 0xfd, 0x2f]),
        Err(Malformed::Value)
    );
    assert_eq!(Ask::decode(&[0; 5]), Err(Malformed::Length));
    assert_eq!(Ask::decode(&[0; 7]), Err(Malformed::Length));
    assert_eq!(Malformed::Length.att_error(), 0x0D);
    assert_eq!(Malformed::Value.att_error(), 0x13);
}

#[test]
fn reads_ignore_what_a_later_version_appends() {
    let s = Status {
        relays: 9,
        ..Status::default()
    };
    let mut longer = [0xAA; Status::LEN + 3];
    longer[..Status::LEN].copy_from_slice(&s.encode());
    assert_eq!(Status::decode(&longer), Ok(s));
    assert_eq!(
        Status::decode(&longer[..Status::LEN - 1]),
        Err(Malformed::Length)
    );
    let n = Node {
        version: VERSION,
        oldest: OLDEST,
        id: NodeId([4; 6]),
        image: 9,
        room: 2,
    };
    let mut longer = [0xAA; Node::LEN + 1];
    longer[..Node::LEN].copy_from_slice(&n.encode());
    assert_eq!(Node::decode(&longer), Ok(n));
    assert_eq!(
        Node::decode(&longer[..Node::LEN - 1]),
        Err(Malformed::Length)
    );
    let g = GivenBack {
        msg_id: MsgId(1),
        wait_s: 2,
    };
    let mut longer = [0xAA; GivenBack::LEN + 1];
    longer[..GivenBack::LEN].copy_from_slice(&g.encode());
    assert_eq!(GivenBack::decode(&longer), Ok(Some(g)));
    assert_eq!(GivenBack::decode(&[]), Ok(None));
    assert_eq!(Message::decode(&[]), Ok(None));
    assert_eq!(Message::decode(&[1, 2, 3]), Err(Malformed::Length));
    let t = NodeTime {
        utc_s: 5,
        offset_min: -60,
        source: TimeSource::Phone,
    };
    let mut longer = [0xAA; NodeTime::LEN + 2];
    longer[..NodeTime::LEN].copy_from_slice(&t.encode());
    assert_eq!(NodeTime::decode(&longer), Ok(t));
    assert_eq!(NodeTime::decode(&longer[..6]), Err(Malformed::Length));
    let a = Answer {
        from: NodeId([3; 6]),
        status: bark_proto::status::Status::default(),
    };
    let mut longer = [0xAA; Answer::LEN + 2];
    longer[..Answer::LEN].copy_from_slice(&a.encode());
    assert_eq!(Answer::decode(&longer), Ok(Some(a)));
    assert_eq!(Answer::decode(&[]), Ok(None));
    assert_eq!(Answer::decode(&longer[..14]), Err(Malformed::Length));
}

#[test]
fn seconds_round_up_and_saturate() {
    let t = Instant::from_micros(10_000_000);
    assert_eq!(seconds_left(t, t), 0);
    assert_eq!(seconds_left(t, Instant::from_micros(5)), 0);
    assert_eq!(seconds_left(t, t + Duration::from_micros(1)), 1);
    assert_eq!(seconds_left(t, t + Duration::from_secs(1)), 1);
    assert_eq!(seconds_left(t, t + Duration::from_millis(1580)), 2);
    assert_eq!(seconds_left(t, Instant::MAX), u16::MAX);
}

/// Each side's version at least the other's oldest; the newer side's
/// declaration decides.
#[test]
fn versions_serve_each_other_when_each_meets_the_other_s_oldest() {
    assert!(compatible(VERSION, OLDEST));
    assert!(
        !compatible(OLDEST - 1, 0),
        "an app older than the node serves"
    );
    assert!(
        !compatible(VERSION + 1, VERSION + 1),
        "an app that serves only newer nodes"
    );
    assert!(
        compatible(VERSION + 5, VERSION),
        "a newer app that still serves this node"
    );
}

#[test]
fn every_value_fits_its_pdu() {
    assert_eq!(value_room(ATT_MTU), 244);
    assert_eq!(value_room(DEFAULT_ATT_MTU), 20);
    assert_eq!(Status::LEN, 17);
    assert_eq!(Node::LEN, 13);
    assert_eq!(home::Home::LEN, 8);
}

#[test]
fn the_empty_status_is_the_default() {
    assert_eq!(Status::EMPTY, Status::default());
}

#[test]
fn public_writes() {
    assert_eq!(
        PublicWrite::decode(&[0]),
        Ok(PublicWrite::Set(Public(false)))
    );
    assert_eq!(
        PublicWrite::decode(&[1]),
        Ok(PublicWrite::Set(Public(true)))
    );
    assert_eq!(PublicWrite::decode(&[2]), Ok(PublicWrite::Unpair));
    assert_eq!(PublicWrite::Unpair.encode(), [2]);
    assert_eq!(PublicWrite::decode(&[3]), Err(Malformed::Value));
    assert_eq!(PublicWrite::decode(&[2, 0]), Err(Malformed::Length));
    // The read stays a setting.
    assert_eq!(Public::decode(&[2]), Err(Malformed::Value));
}

#[test]
fn recipient_and_public_are_taken_whole() {
    // The tag, the versions and the person's name; a guest's, no name.
    let named = Recipient {
        tag: 0xab,
        version: 1,
        oldest: 1,
        name: phones::Name::new("Åsa".as_bytes()).unwrap(),
    };
    assert!(named.compatible());
    let mut b = [0; Recipient::MAX_LEN];
    let n = named.encode(&mut b);
    assert_eq!(&b[..n], &[0xab, 1, 1, 0xc3, 0x85, b's', b'a']);
    assert_eq!(Recipient::decode(&b[..n]), Ok(named));
    assert_eq!(
        Recipient::decode(&b[..3]).map(|r| r.name),
        Ok(phones::Name::EMPTY)
    );
    assert_eq!(Recipient::decode(&[]), Err(Malformed::Length));
    assert_eq!(Recipient::decode(&[0xab]), Err(Malformed::Length));
    assert_eq!(Recipient::decode(&[1, 1]), Err(Malformed::Length));
    assert_eq!(Recipient::decode(&[1; 36]), Err(Malformed::Length));
    // Not UTF-8: 0xFF never is, so a name never reads as erased flash.
    assert_eq!(Recipient::decode(&[1, 1, 1, 0xff]), Err(Malformed::Value));
    assert_eq!(Public::decode(&Public(true).encode()), Ok(Public(true)));
    assert_eq!(Public::decode(&[0]), Ok(Public(false)));
    assert_eq!(Public::decode(&[2]), Err(Malformed::Value));
    assert_eq!(Public::decode(&[1, 1]), Err(Malformed::Length));
}

#[test]
fn a_guest_item_round_trips_and_after_is_three_bytes() {
    assert_eq!(After(0x01_0203).encode(), [1, 2, 3]);
    assert_eq!(After::decode(&[1, 2, 3]), Ok(After(0x01_0203)));
    assert_eq!(After::decode(&[1, 2]), Err(Malformed::Length));
    let item = GuestItem {
        number: 7,
        msg_id: MsgId(0x0102_0304),
        payload: b"hi",
    };
    let mut b = [0u8; GuestItem::MAX_LEN];
    let n = item.encode(&mut b);
    assert_eq!(&b[..n], &[0, 0, 7, 1, 2, 3, 4, b'h', b'i']);
    assert_eq!(GuestItem::decode(&b[..n]), Ok(Some(item)));
    assert_eq!(GuestItem::decode(&[]), Ok(None));
    assert_eq!(GuestItem::decode(&b[..6]), Err(Malformed::Length));
}

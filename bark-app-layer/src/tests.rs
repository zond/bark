use std::format;
use std::string::String;
use std::vec::Vec;

use crypto_box::{PublicKey, SecretKey};

use crate::card::{self, Card};
use crate::keys::{self, Keys};
use crate::plain::{self, Header, Record, Writer};
use crate::seal::{self, Fetch, Form, Opened};
use crate::text::{self, Encoding};
use crate::vectors::{self, fixture, hex};
use crate::{Error, MAX_APP_PAYLOAD, MAX_PAYLOAD};

#[test]
fn vectors_file_is_current() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/vectors.json");
    let made = vectors::json();
    if std::env::var_os("BARK_WRITE_VECTORS").is_some() {
        std::fs::write(path, &made).unwrap();
    }
    let kept = std::fs::read_to_string(path).unwrap_or_default();
    assert!(
        kept == made,
        "vectors.json is not what this code makes; BARK_WRITE_VECTORS=1 rewrites it"
    );
}

#[test]
fn the_sizes_are_the_notes() {
    assert_eq!(MAX_APP_PAYLOAD, 207);
    assert_eq!(seal::BOX_OVERHEAD, 41);
    assert_eq!(seal::INTRO_OVERHEAD, 49);
    assert_eq!(seal::REPLY_OVERHEAD, 20);
    assert_eq!(seal::NOTHING_NEW_LEN, 17);
    assert_eq!(Fetch::LEN, 61);
    assert_eq!(card::FIXED_LEN, 38);
    assert_eq!(card::MAX_LEN, 83);
    assert_eq!(card::MAX_TEXT_LEN, 5 + 133);
    // The longest text: 207 less the box, the header and the record's type.
    assert_eq!(seal::MAX_BOX_PLAIN - plain::HEADER_LEN - 1, 160);
}

#[test]
fn the_word_list_is_sorted_and_whole() {
    let w: Vec<&str> = keys::words().collect();
    assert_eq!(w.len(), 2048);
    assert!(w.windows(2).all(|p| p[0] < p[1]));
}

#[test]
fn a_phrase_gives_back_its_seed() {
    for seed in [
        [0u8; 16],
        [0xff; 16],
        fixture().alice_seed,
        fixture().bob_seed,
    ] {
        let p = keys::phrase(&seed);
        assert_eq!(keys::seed_of(p), Ok(seed));
        let upper: Vec<String> = p.iter().map(|w| w.to_uppercase()).collect();
        assert_eq!(keys::seed_of(upper.iter().map(|s| s.as_str())), Ok(seed));
    }
}

#[test]
fn a_phrase_with_a_wrong_word_is_refused() {
    let seed = fixture().alice_seed;
    let mut p = keys::phrase(&seed);
    // The last word holds the checksum: another one breaks it, but for
    // the one in 16 with the same top 7 bits.
    p[11] = if p[11] == "zoo" { "abandon" } else { "zoo" };
    assert_eq!(keys::seed_of(p), Err(Error::Malformed));
    assert_eq!(
        keys::seed_of(p[..11].iter().copied()),
        Err(Error::Malformed)
    );
    assert_eq!(
        keys::seed_of(["notaword"; 12].iter().copied()),
        Err(Error::Malformed)
    );
}

#[test]
fn keys_differ_by_seed() {
    let f = fixture();
    assert_ne!(f.alice.identity, f.bob.identity);
    assert_ne!(f.alice.mailbox, f.bob.mailbox);
    assert_ne!(f.alice.mailbox, f.alice.secret);
}

fn boxed_text(from: &Keys, to: &Keys, s: &str) -> Vec<u8> {
    let mut b = [0u8; MAX_PAYLOAD];
    let mut t = [0u8; MAX_PAYLOAD];
    let (encoding, n) = text::encode(s, &mut t).unwrap();
    let mut w = Writer::new(
        &mut b,
        &Header {
            greater: plain::greater(&from.identity, &to.identity),
            sent: 7,
            address: None,
        },
    )
    .unwrap();
    w.push(&Record::Text {
        encoding,
        bytes: &t[..n],
    })
    .unwrap();
    let len = w.finish();
    let pair = seal::pair(&PublicKey::from(to.identity.0), &from.secret_key());
    let mut out = [0u8; MAX_PAYLOAD];
    let m = seal::seal_box(&pair, &[9; 24], &b[..len], &mut out).unwrap();
    out[..m].to_vec()
}

#[test]
fn a_box_opens_for_its_pair_only() {
    let f = fixture();
    let carol = Keys::from_seed(&[3; 16]);
    let payload = boxed_text(&f.alice, &f.bob, "hi");
    let mut out = [0u8; MAX_PAYLOAD];
    let bob_from_alice = seal::pair(&PublicKey::from(f.alice.identity.0), &f.bob.secret_key());
    let n = seal::open_box(&bob_from_alice, &payload, &mut out).unwrap();
    let (h, mut r) = plain::read(&out[..n]).unwrap();
    assert_eq!(
        h.greater,
        plain::greater(&f.alice.identity, &f.bob.identity)
    );
    assert_eq!(
        r.next(),
        Some(Ok(Record::Text {
            encoding: Encoding::Windows1251,
            bytes: b"hi"
        }))
    );
    let bob_from_carol = seal::pair(&PublicKey::from(carol.identity.0), &f.bob.secret_key());
    assert_eq!(
        seal::open_box(&bob_from_carol, &payload, &mut out),
        Err(Error::Open)
    );
}

#[test]
fn a_header_names_where_its_sender_writes_from_not_their_cards_home() {
    let f = fixture();
    let mut b = [0u8; MAX_PAYLOAD];
    let h = Header {
        greater: false,
        sent: 9,
        address: Some(f.away),
    };
    let w = Writer::new(&mut b, &h).unwrap();
    let n = w.finish();
    assert_eq!(n, plain::HEADER_LEN + 13);
    assert_eq!(b[0], 1);
    assert_eq!(&b[5..n], &f.away.encode());
    assert_eq!(plain::read(&b[..n]).unwrap().0, h);
    // The vectors' text is written from away from the home on the card.
    let v = vectors::json();
    assert!(v.contains(&format!("\"address\": \"{}\"", hex(&f.away.encode()))));
}

#[test]
fn a_changed_byte_does_not_open() {
    let f = fixture();
    let pair = seal::pair(&PublicKey::from(f.alice.identity.0), &f.bob.secret_key());
    let payload = boxed_text(&f.alice, &f.bob, "hi");
    for i in 1..payload.len() {
        let mut p = payload.clone();
        p[i] ^= 1;
        let mut out = [0u8; MAX_PAYLOAD];
        assert_eq!(
            seal::open_box(&pair, &p, &mut out),
            Err(Error::Open),
            "byte {i}"
        );
    }
}

#[test]
fn the_direction_bit_tells_a_reflected_message() {
    let f = fixture();
    // Alice's message to Bob, sent back to Alice, opens under the same
    // pair key; its bit says its sender was Alice.
    let payload = boxed_text(&f.alice, &f.bob, "hi");
    let alice_from_bob = seal::pair(&PublicKey::from(f.bob.identity.0), &f.alice.secret_key());
    let mut out = [0u8; MAX_PAYLOAD];
    let n = seal::open_box(&alice_from_bob, &payload, &mut out).unwrap();
    let (h, _) = plain::read(&out[..n]).unwrap();
    assert_ne!(
        h.greater,
        plain::greater(&f.bob.identity, &f.alice.identity)
    );
}

#[test]
fn our_sealed_box_is_the_librarys() {
    let f = fixture();
    let bob = f.bob.secret_key();
    let mut out = [0u8; MAX_PAYLOAD];
    let n = seal::seal_intro(
        &bob.public_key(),
        &SecretKey::from([5; 32]),
        b"abc",
        &mut out,
    )
    .unwrap();
    assert_eq!(bob.unseal(&out[1..n]).unwrap(), b"abc");
    let theirs = bob
        .public_key()
        .seal(&mut crypto_box::aead::OsRng, b"xyz")
        .unwrap();
    let mut p = Vec::from([Form::Introduction.format()]);
    p.extend_from_slice(&theirs);
    let n = seal::open_intro(&bob, &p, &mut out).unwrap();
    assert_eq!(&out[..n], b"xyz");
}

#[test]
fn too_long_is_refused() {
    let f = fixture();
    let pair = seal::pair(&PublicKey::from(f.bob.identity.0), &f.alice.secret_key());
    let mut out = [0u8; MAX_PAYLOAD];
    let plain = [0u8; seal::MAX_BOX_PLAIN + 1];
    assert_eq!(
        seal::seal_box(&pair, &[0; 24], &plain, &mut out),
        Err(Error::TooLong)
    );
    let n = seal::seal_box(&pair, &[0; 24], &plain[1..], &mut out).unwrap();
    assert_eq!(n, MAX_APP_PAYLOAD);
    assert_eq!(
        seal::seal_reply(
            &f.alice.mailbox,
            1,
            1,
            1,
            &[0; MAX_APP_PAYLOAD + 1],
            &mut out
        ),
        Err(Error::TooLong)
    );
}

#[test]
fn a_fetch_opens_under_its_key_only() {
    let f = fixture();
    let fetch = Fetch {
        sent: 3,
        address: f.away,
        since: 0x12_3456,
    };
    let p = fetch.seal(&f.alice.mailbox, &[1; 24]).unwrap();
    assert_eq!(Fetch::open(&f.alice.mailbox, &p), Ok(fetch));
    assert_eq!(Fetch::open(&f.bob.mailbox, &p), Err(Error::Open));
    let big = Fetch {
        since: seal::MAX_NUMBER + 1,
        ..fetch
    };
    assert_eq!(big.seal(&f.alice.mailbox, &[1; 24]), Err(Error::TooLong));
}

#[test]
fn a_reply_opens_in_its_round_only_and_tells_its_number() {
    let k = fixture().alice.mailbox;
    let mut out = [0u8; MAX_PAYLOAD];
    let n = seal::seal_reply(&k, 10, seal::MAX_REPLIES, 0x12_3456, b"item", &mut out).unwrap();
    assert_eq!(n, 4 + seal::REPLY_OVERHEAD);
    let p = out[..n].to_vec();
    let mut got = [0u8; MAX_PAYLOAD];
    assert_eq!(
        seal::open_reply(&k, 10, &p, &mut got),
        Ok(Opened::Item {
            number: 0x12_3456,
            place: seal::MAX_REPLIES,
            len: 4
        })
    );
    assert_eq!(&got[..4], b"item");
    assert_eq!(seal::open_reply(&k, 11, &p, &mut got), Err(Error::Open));
    let other = fixture().bob.mailbox;
    assert_eq!(seal::open_reply(&other, 10, &p, &mut got), Err(Error::Open));
    for place in [0, seal::MAX_REPLIES + 1] {
        assert_eq!(
            seal::seal_reply(&k, 10, place, 1, b"item", &mut out),
            Err(Error::TooLong)
        );
    }
}

#[test]
fn nothing_new_opens_in_its_round_only() {
    let k = fixture().alice.mailbox;
    let p = seal::seal_nothing_new(&k, 10);
    assert_eq!(p[0], Form::Reply.format());
    let mut got = [0u8; MAX_PAYLOAD];
    assert_eq!(
        seal::open_reply(&k, 10, &p, &mut got),
        Ok(Opened::NothingNew)
    );
    assert_eq!(seal::open_reply(&k, 11, &p, &mut got), Err(Error::Open));
    // Between nothing new and the shortest item: no reply.
    assert_eq!(
        seal::open_reply(&k, 10, &[p[0]; seal::REPLY_OVERHEAD - 1], &mut got),
        Err(Error::Malformed)
    );
}

#[test]
fn a_card_reads_back_as_bytes_and_as_text() {
    let f = fixture();
    let c = Card {
        identity: f.alice.identity,
        home: Some(f.home),
        time: 42,
        name: "Ім'я",
    };
    let mut b = [0u8; card::MAX_LEN];
    let n = c.encode(&mut b).unwrap();
    assert_eq!(n, 51 + 7);
    assert_eq!(&b[..2], &[2, 1]);
    assert_eq!(Card::decode(&b[..n]), Ok(c));
    let homeless = Card { home: None, ..c };
    let n = homeless.encode(&mut b).unwrap();
    assert_eq!(n, 38 + 7);
    assert_eq!(b[1], 0);
    assert_eq!(Card::decode(&b[..n]), Ok(homeless));
    let mut t = [0u8; card::MAX_TEXT_LEN];
    let tn = c.to_text(&mut t).unwrap();
    let s = core::str::from_utf8(&t[..tn]).unwrap();
    assert!(s.starts_with("BARK:"));
    let mut buf = [0u8; card::MAX_LEN];
    assert_eq!(Card::parse(s, &mut buf), Ok(c));
    let lower = String::from(s).to_lowercase();
    let mut buf = [0u8; card::MAX_LEN];
    assert_eq!(Card::parse(&format!(" {lower}\n"), &mut buf), Ok(c));
}

#[test]
fn a_card_that_is_not_one_is_refused() {
    let mut buf = [0u8; card::MAX_LEN];
    for s in ["", "BARK:", "hello", "BARK:A1", "BARK:AAAA"] {
        assert_eq!(Card::parse(s, &mut buf), Err(Error::Malformed), "{s}");
    }
    let f = fixture();
    let long = Card {
        identity: f.alice.identity,
        home: Some(f.home),
        time: 0,
        name: "a name of more than thirty-two bytes",
    };
    assert_eq!(long.encode(&mut [0u8; 100]), Err(Error::TooLong));
    // A version 1 card, before the flags, and a flag this version does
    // not know.
    let short = Card { name: "", ..long };
    let mut b = [0u8; card::MAX_LEN];
    let n = short.encode(&mut b).unwrap();
    for (at, v) in [(0, 1), (1, 2)] {
        let mut bad = b;
        bad[at] = v;
        assert_eq!(Card::decode(&bad[..n]), Err(Error::Malformed), "{at}");
    }
    // A home flag with no room for the address, and a name too long for
    // a card without one.
    assert_eq!(
        Card::decode(&b[..card::FIXED_LEN + 12]),
        Err(Error::Malformed)
    );
    let mut too_long = [b'a'; card::FIXED_LEN + card::MAX_NAME + 1];
    too_long[..card::FIXED_LEN].copy_from_slice(&b[..card::FIXED_LEN]);
    too_long[1] = 0;
    assert_eq!(Card::decode(&too_long), Err(Error::Malformed));
}

#[test]
fn text_takes_windows_1251_when_it_can() {
    let mut b = [0u8; 64];
    assert_eq!(text::encode("Їжак", &mut b), Ok((Encoding::Windows1251, 4)));
    assert_eq!(&b[..4], &[0xaf, 0xe6, 0xe0, 0xea]);
    let s: String = text::decode(Encoding::Windows1251, &b[..4])
        .unwrap()
        .collect();
    assert_eq!(s, "Їжак");
    assert_eq!(text::encode("Їжак 🦔", &mut b).unwrap().0, Encoding::Utf8);
    assert_eq!(
        text::decode(Encoding::Windows1251, &[0x98]).err(),
        Some(Error::Malformed)
    );
    assert_eq!(
        text::decode(Encoding::Utf8, &[0xff]).err(),
        Some(Error::Malformed)
    );
}

#[test]
fn records_read_back_in_order() {
    let f = fixture();
    let c = Card {
        identity: f.bob.identity,
        home: None,
        time: 1,
        name: "",
    };
    let mut b = [0u8; MAX_PAYLOAD];
    let h = Header {
        greater: true,
        sent: 0xdead_beef,
        address: Some(f.home),
    };
    let mut w = Writer::new(&mut b, &h).unwrap();
    w.push(&Record::Acknowledge { sent: 5 }).unwrap();
    w.push(&Record::Card(c)).unwrap();
    let t = Record::Text {
        encoding: Encoding::Utf8,
        bytes: b"end",
    };
    w.push(&t).unwrap();
    assert_eq!(w.push(&t), Err(Error::Malformed));
    let n = w.finish();
    assert_eq!(n, 5 + 13 + 5 + 2 + 38 + 4);
    let (got, r) = plain::read(&b[..n]).unwrap();
    assert_eq!(got, h);
    let rs: Vec<_> = r.collect();
    assert_eq!(
        rs,
        [
            Ok(Record::Acknowledge { sent: 5 }),
            Ok(Record::Card(c)),
            Ok(t)
        ]
    );
    assert!(plain::read(&[0x80, 0, 0, 0, 0]).is_err());
    let (_, mut r) = plain::read(&[0, 0, 0, 0, 0, 9]).unwrap();
    assert_eq!(r.next(), Some(Err(Error::Malformed)));
    assert_eq!(r.next(), None);
}

mod mailbox {
    use std::vec::Vec;

    use bark_proto::MsgId;

    use crate::mailbox::{Arrival, Mailbox, RamRing, Ring};
    use crate::seal::{self, Fetch, Opened};
    use crate::vectors::fixture;
    use crate::MAX_PAYLOAD;

    type Small = RamRing<32>;

    fn home() -> Mailbox<Box<Small>> {
        let mut m = Mailbox::new(Box::new(Small::new()));
        m.set_key(Some(fixture().alice.mailbox));
        m
    }

    impl<R: Ring> Ring for Box<R> {
        fn pages(&self) -> u32 {
            (**self).pages()
        }
        fn read(&self, slot: u32, out: &mut [u8; 256]) {
            (**self).read(slot, out)
        }
        fn write(&mut self, slot: u32, bytes: &[u8; 256]) {
            (**self).write(slot, bytes)
        }
        fn erase(&mut self, page: u32) {
            (**self).erase(page)
        }
        fn state(&self) -> Option<[u8; 64]> {
            (**self).state()
        }
        fn keep_state(&mut self, state: &[u8; 64]) {
            (**self).keep_state(state)
        }
    }

    use std::boxed::Box;

    fn fetch(key: &[u8; 32], sent: u32, since: u32) -> [u8; Fetch::LEN] {
        Fetch {
            sent,
            address: fixture().away,
            since,
        }
        .seal(key, &[sent as u8; 24])
        .unwrap()
    }

    /// What a requester gets from a reply.
    #[derive(Debug, PartialEq)]
    enum Got {
        Item(u32, Vec<u8>),
        NothingNew,
    }

    /// Every reply due now, opened as the requester opens them, in order.
    fn drain<R: Ring>(m: &mut Mailbox<R>, round: u32) -> Vec<Got> {
        let key = fixture().alice.mailbox;
        let mut got = Vec::new();
        let mut out = [0u8; MAX_PAYLOAD];
        while let Some(o) = m.next(&mut out) {
            assert_eq!(o.address, fixture().away);
            let mut item = [0u8; MAX_PAYLOAD];
            got.push(
                match seal::open_reply(&key, round, &out[..o.len], &mut item) {
                    Ok(Opened::Item { number, len, .. }) => {
                        assert_eq!(number, o.number);
                        Got::Item(number, item[..len].to_vec())
                    }
                    Ok(Opened::NothingNew) => {
                        assert_eq!(o.number, 0);
                        Got::NothingNew
                    }
                    Err(e) => panic!("a reply the requester cannot open: {e:?}"),
                },
            );
        }
        assert!(!m.reply_due());
        got
    }

    fn numbers(got: &[Got]) -> Vec<u32> {
        got.iter()
            .map(|g| match g {
                Got::Item(n, _) => *n,
                Got::NothingNew => 0,
            })
            .collect()
    }

    fn item(i: u32) -> Vec<u8> {
        std::format!("item {i}").into_bytes()
    }

    #[test]
    fn a_fetch_gets_every_item_after_since_once() {
        let mut m = home();
        for i in 1..=5 {
            assert_eq!(m.arrive(MsgId(i), &item(i)), Arrival::Kept(i));
        }
        let k = fixture().alice.mailbox;
        assert_eq!(m.arrive(MsgId(0), &fetch(&k, 100, 2)), Arrival::Fetch);
        assert!(m.reply_due());
        let got = drain(&mut m, 100);
        assert_eq!(
            got,
            [
                Got::Item(3, item(3)),
                Got::Item(4, item(4)),
                Got::Item(5, item(5))
            ]
        );
    }

    #[test]
    fn a_fetch_with_nothing_new_gets_one_reply_that_says_so() {
        let mut m = home();
        m.arrive(MsgId(1), &item(1));
        let k = fixture().alice.mailbox;
        assert_eq!(m.arrive(MsgId(0), &fetch(&k, 100, 1)), Arrival::Fetch);
        assert_eq!(drain(&mut m, 100), [Got::NothingNew]);
        // And nothing after it.
        let mut out = [0u8; MAX_PAYLOAD];
        assert_eq!(m.next(&mut out), None);
        // An empty ring answers the same.
        let mut m = home();
        m.arrive(MsgId(0), &fetch(&k, 100, 0));
        assert_eq!(drain(&mut m, 100), [Got::NothingNew]);
    }

    #[test]
    fn an_item_after_the_fetch_waits_for_the_next() {
        let mut m = home();
        m.arrive(MsgId(1), &item(1));
        let k = fixture().alice.mailbox;
        m.arrive(MsgId(0), &fetch(&k, 100, 0));
        let mut out = [0u8; MAX_PAYLOAD];
        assert_eq!(m.next(&mut out).map(|o| o.number), Some(1));
        m.arrive(MsgId(2), &item(2));
        assert_eq!(m.next(&mut out), None);
        assert!(!m.reply_due());
        m.arrive(MsgId(3), &item(3));
        m.arrive(MsgId(0), &fetch(&k, 101, 1));
        assert_eq!(numbers(&drain(&mut m, 101)), [2, 3]);
    }

    #[test]
    fn since_0_gets_all_the_ring_holds() {
        let mut m = Mailbox::new(Box::new(RamRing::<512>::new()));
        m.set_key(Some(fixture().alice.mailbox));
        for i in 1..=300 {
            m.arrive(MsgId(i), &item(i));
        }
        let k = fixture().alice.mailbox;
        m.arrive(MsgId(0), &fetch(&k, 100, 0));
        // More than a round holds: the newest MAX_REPLIES.
        let got = numbers(&drain(&mut m, 100));
        assert_eq!(got, (45..=300).collect::<Vec<_>>());
    }

    #[test]
    fn a_fetch_under_another_key_is_refused_and_not_kept() {
        let mut m = home();
        m.arrive(MsgId(1), &item(1));
        let other = fixture().bob.mailbox;
        assert_eq!(m.arrive(MsgId(0), &fetch(&other, 100, 0)), Arrival::Refused);
        assert_eq!(m.newest(), 1);
        assert!(drain(&mut m, 100).is_empty());
    }

    #[test]
    fn a_replayed_fetch_is_refused_even_after_a_reboot() {
        let mut m = home();
        m.arrive(MsgId(1), &item(1));
        let k = fixture().alice.mailbox;
        let f = fetch(&k, 100, 0);
        assert_eq!(m.arrive(MsgId(0), &f), Arrival::Fetch);
        assert_eq!(m.arrive(MsgId(0), &f), Arrival::Refused);
        assert_eq!(m.arrive(MsgId(0), &fetch(&k, 99, 0)), Arrival::Refused);
        let ring = Box::new(clone_ring(m.ring()));
        let mut m = Mailbox::new(ring);
        assert!(m.is_home());
        assert_eq!(m.newest(), 1);
        assert_eq!(m.arrive(MsgId(0), &f), Arrival::Refused);
        assert_eq!(m.arrive(MsgId(0), &fetch(&k, 101, 0)), Arrival::Fetch);
    }

    #[test]
    fn a_round_starts_again_after_a_reboot_with_the_same_bytes() {
        let mut m = home();
        m.arrive(MsgId(1), &item(1));
        m.arrive(MsgId(2), &item(2));
        let k = fixture().alice.mailbox;
        m.arrive(MsgId(0), &fetch(&k, 100, 0));
        let mut out = [0u8; MAX_PAYLOAD];
        let first = m.next(&mut out).unwrap();
        let mut m = Mailbox::new(Box::new(clone_ring(m.ring())));
        m.arrive(MsgId(3), &item(3));
        let mut again = [0u8; MAX_PAYLOAD];
        assert_eq!(m.next(&mut again), Some(first));
        assert_eq!(out[..first.len], again[..first.len]);
        assert_eq!(numbers(&drain(&mut m, 100)), [2]);
    }

    fn clone_ring(r: &Small) -> Small {
        let mut c = Small::new();
        let mut b = [0u8; 256];
        for s in 0..32 {
            r.read(s, &mut b);
            c.write(s, &b);
        }
        if let Some(st) = r.state() {
            c.keep_state(&st);
        }
        c
    }

    #[test]
    fn the_ring_overwrites_the_oldest() {
        let mut m = home();
        for i in 1..=40 {
            m.arrive(MsgId(i), &item(i));
        }
        // 32 slots in two pages: writing item 33 erased the page of 1 to 16.
        assert_eq!(m.oldest(), 17);
        let mut b = [0u8; MAX_PAYLOAD];
        assert_eq!(m.item(16, &mut b), None);
        assert_eq!(m.item(17, &mut b), Some(item(17).len()));
        let k = fixture().alice.mailbox;
        m.arrive(MsgId(0), &fetch(&k, 100, 3));
        assert_eq!(numbers(&drain(&mut m, 100)), (17..=40).collect::<Vec<_>>());
    }

    #[test]
    fn a_reply_given_back_goes_again() {
        let mut m = home();
        m.arrive(MsgId(1), &item(1));
        m.arrive(MsgId(2), &item(2));
        let k = fixture().alice.mailbox;
        m.arrive(MsgId(0), &fetch(&k, 100, 0));
        let mut out = [0u8; MAX_PAYLOAD];
        let first = m.next(&mut out).unwrap();
        m.given_back(first.msg_id);
        let again = m.next(&mut out).unwrap();
        assert_eq!(again, first);
        // Nothing new, given back, goes again too.
        m.arrive(MsgId(0), &fetch(&k, 101, 2));
        let nothing = m.next(&mut out).unwrap();
        assert_eq!(nothing.number, 0);
        assert_eq!(m.next(&mut out), None);
        m.given_back(nothing.msg_id);
        assert_eq!(m.next(&mut out), Some(nothing));
    }

    #[test]
    fn a_node_that_is_no_home_keeps_nothing() {
        let mut m = Mailbox::new(Box::new(Small::new()));
        assert_eq!(m.arrive(MsgId(1), &item(1)), Arrival::NotHome);
        assert_eq!(m.newest(), 0);
    }

    /// The phone's Inbox: the oldest item not confirmed, with the ID it
    /// arrived with, the same at every read until it is confirmed.
    #[test]
    fn unread_is_the_oldest_unconfirmed_with_its_arrival_id() {
        let mut m = home();
        m.arrive(MsgId(0xa1), &item(1));
        m.arrive(MsgId(0xa2), &item(2));
        let mut b = [0u8; MAX_PAYLOAD];
        let first = (1, MsgId(0xa1), item(1).len());
        assert_eq!(m.unread(&mut b), Some(first));
        assert_eq!(m.unread(&mut b), Some(first));
        m.confirm_read(1);
        assert_eq!(m.unread(&mut b), Some((2, MsgId(0xa2), item(2).len())));
        assert_eq!(&b[..item(2).len()], item(2).as_slice());
        m.confirm_read(2);
        assert_eq!(m.unread(&mut b), None);
    }
}

/// The mailbox on its ring in flash, the board's pages in RAM: what it
/// keeps survives a reboot, and a power cut at any write or erase leaves
/// every item whole or gone and the state the last whole one.
mod flash_ring {
    use std::vec::Vec;

    use bark_proto::MsgId;

    use crate::flash_ring::{FlashRing, Op, Region, PAGE_LEN};
    use crate::mailbox::{Arrival, Mailbox};
    use crate::seal::Fetch;
    use crate::vectors::fixture;
    use crate::MAX_PAYLOAD;

    /// The region's pages: the state's and four of the ring.
    const PAGES: u32 = 5;

    /// Flash in RAM, as NOR flash behaves: a write clears bits, an erase
    /// sets a page's.
    #[derive(Clone)]
    struct Ram(Vec<u8>);

    impl Ram {
        fn erased() -> Ram {
            Ram(std::vec![0xff; (PAGES * PAGE_LEN) as usize])
        }

        /// Do `op`, or as a power cut leaves it: of a write its first
        /// `cut` bytes, of an erase nothing.
        fn apply(&mut self, op: &Op, cut: Option<usize>) {
            match *op {
                Op::Erase { offset } => {
                    if cut.is_none() {
                        let o = offset as usize;
                        self.0[o..o + PAGE_LEN as usize].fill(0xff);
                    }
                }
                Op::Write { offset, .. } => {
                    let d = op.data();
                    let n = cut.unwrap_or(d.len()).min(d.len());
                    for (i, b) in d[..n].iter().enumerate() {
                        self.0[offset as usize + i] &= b;
                    }
                }
            }
        }
    }

    impl Region for &Ram {
        fn pages(&self) -> u32 {
            PAGES
        }
        fn read(&self, offset: u32, out: &mut [u8]) {
            let o = offset as usize;
            out.copy_from_slice(&self.0[o..o + out.len()]);
        }
    }

    type Home<'a> = Mailbox<FlashRing<&'a Ram>>;

    fn item(i: u32) -> Vec<u8> {
        std::format!("item {i}").into_bytes()
    }

    fn fetch(sent: u32, since: u32) -> [u8; Fetch::LEN] {
        Fetch {
            sent,
            address: fixture().away,
            since,
        }
        .seal(&fixture().alice.mailbox, &[sent as u8; 24])
        .unwrap()
    }

    fn home(m: &mut Home<'_>) {
        m.set_key(Some(fixture().alice.mailbox));
    }

    /// The ops the mailbox has queued, taken from it in order.
    fn take_ops(m: &mut Home<'_>) -> Vec<Op> {
        let mut q = Vec::new();
        while let Some(op) = m.ring().op() {
            q.push(op);
            m.ring_mut().op_done();
        }
        assert_eq!(m.ring().lost(), 0);
        q
    }

    /// Boot a mailbox from `flash`, do `f`, and do every op it queued.
    fn step(flash: &mut Ram, f: impl FnOnce(&mut Home<'_>)) {
        let view = flash.clone();
        let mut m = Mailbox::new(FlashRing::open(&view));
        f(&mut m);
        for op in take_ops(&mut m) {
            flash.apply(&op, None);
        }
    }

    #[test]
    fn items_and_state_survive_a_reboot() {
        let mut flash = Ram::erased();
        step(&mut flash, home);
        for i in 1..=70 {
            step(&mut flash, |m| {
                assert_eq!(m.arrive(MsgId(i), &item(i)), Arrival::Kept(i))
            });
        }
        step(&mut flash, |m| {
            assert_eq!(m.arrive(MsgId(0), &fetch(100, 0)), Arrival::Fetch)
        });
        step(&mut flash, |m| m.confirm_read(60));
        let mut m = Mailbox::new(FlashRing::open(&flash));
        assert!(m.is_home());
        assert_eq!(m.newest(), 70);
        // Four pages of 16: item 65 erased the page of 1 to 16.
        assert_eq!(m.oldest(), 17);
        let mut b = [0u8; MAX_PAYLOAD];
        assert_eq!(m.item(17, &mut b), Some(item(17).len()));
        assert_eq!(&b[..item(17).len()], item(17).as_slice());
        assert_eq!(m.unread(&mut b).map(|u| (u.0, u.1)), Some((61, MsgId(61))));
        assert_eq!(m.unread_count(), 10);
        assert_eq!(
            m.arrive(MsgId(0), &fetch(100, 0)),
            Arrival::Refused,
            "replayed"
        );
    }

    /// The state page fills, is erased and starts again, and the state
    /// read back is always the last kept.
    #[test]
    fn the_state_log_wraps() {
        let mut flash = Ram::erased();
        step(&mut flash, home);
        for i in 1..=100u32 {
            step(&mut flash, |m| {
                m.arrive(MsgId(i), &item(i));
                m.confirm_read(i);
            });
        }
        let m = Mailbox::new(FlashRing::open(&flash));
        assert!(m.is_home());
        assert_eq!(m.unread_count(), 0);
        assert_eq!(m.newest(), 100);
    }

    /// A power cut at every op of a run, at every eighth byte of a
    /// write: the mailbox comes back with every item it reads whole and
    /// right, its key, and no number above what was kept.
    #[test]
    fn a_cut_anywhere_leaves_items_whole_or_gone() {
        // The run: home, 40 items, a fetch, a read confirmed; one mailbox
        // throughout, its ops collected in order.
        let mut all: Vec<Op> = Vec::new();
        {
            let view = Ram::erased();
            let mut m = Mailbox::new(FlashRing::open(&view));
            home(&mut m);
            all.extend(take_ops(&mut m));
            for i in 1..=40 {
                m.arrive(MsgId(i), &item(i));
                all.extend(take_ops(&mut m));
            }
            m.arrive(MsgId(0), &fetch(100, 0));
            all.extend(take_ops(&mut m));
            m.confirm_read(30);
            all.extend(take_ops(&mut m));
        }
        let mut flash = Ram::erased();
        let mut cuts = 0;
        for (k, op) in all.iter().enumerate() {
            let points: Vec<usize> = match op {
                Op::Erase { .. } => std::vec![0],
                Op::Write { .. } => (0..op.data().len()).step_by(8).collect(),
            };
            for cut in points {
                let mut f = flash.clone();
                f.apply(op, Some(cut));
                let m = Mailbox::new(FlashRing::open(&f));
                cuts += 1;
                let mut b = [0u8; MAX_PAYLOAD];
                for n in 1..=m.newest() {
                    if let Some(l) = m.item(n, &mut b) {
                        assert_eq!(&b[..l], item(n).as_slice(), "op {k} cut {cut}");
                    }
                }
                assert!(m.newest() <= 40);
                // The key is lost only by a cut in its own first record.
                if k > 0 {
                    assert!(m.is_home(), "op {k} cut {cut}");
                }
            }
            flash.apply(op, None);
        }
        assert!(cuts > 1000, "{cuts}");
        let m = Mailbox::new(FlashRing::open(&flash));
        assert_eq!((m.newest(), m.unread_count()), (40, 10));
    }

    #[test]
    fn reads_see_what_is_queued() {
        let flash = Ram::erased();
        let mut m = Mailbox::new(FlashRing::open(&flash));
        home(&mut m);
        m.arrive(MsgId(1), &item(1));
        assert!(!m.ring().idle());
        let mut b = [0u8; MAX_PAYLOAD];
        assert_eq!(m.item(1, &mut b), Some(item(1).len()));
    }
}

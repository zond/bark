use std::format;
use std::string::String;
use std::vec::Vec;

use crypto_box::{PublicKey, SecretKey};

use crate::card::{self, Card};
use crate::keys::{self, Keys};
use crate::plain::{self, Header, Record, Writer};
use crate::seal::{self, Fetch, Form};
use crate::text::{self, Encoding};
use crate::vectors::{self, fixture};
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
    assert_eq!(MAX_APP_PAYLOAD, 210);
    assert_eq!(seal::BOX_OVERHEAD, 41);
    assert_eq!(seal::INTRO_OVERHEAD, 49);
    assert_eq!(seal::REPLY_OVERHEAD, 17);
    assert_eq!(Fetch::LEN, 62);
    assert_eq!(card::FIXED_LEN, 50);
    assert_eq!(card::MAX_LEN, 82);
    assert_eq!(card::MAX_TEXT_LEN, 5 + 132);
    // The longest text: 210 less the box, the header and the record's type.
    assert_eq!(seal::MAX_BOX_PLAIN - plain::HEADER_LEN - 1, 163);
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
        seal::seal_reply(&f.alice.mailbox, 1, 1, &[0; MAX_APP_PAYLOAD + 1], &mut out),
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
        most: 23,
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
fn a_reply_opens_at_its_number_and_round_only() {
    let k = fixture().alice.mailbox;
    let mut out = [0u8; MAX_PAYLOAD];
    let n = seal::seal_reply(&k, 10, 4, b"item", &mut out).unwrap();
    let p = out[..n].to_vec();
    let mut got = [0u8; MAX_PAYLOAD];
    assert_eq!(seal::open_reply(&k, 10, 4, &p, &mut got), Ok(4));
    assert_eq!(&got[..4], b"item");
    assert_eq!(seal::open_reply(&k, 10, 5, &p, &mut got), Err(Error::Open));
    assert_eq!(seal::open_reply(&k, 11, 4, &p, &mut got), Err(Error::Open));
}

#[test]
fn a_card_reads_back_as_bytes_and_as_text() {
    let f = fixture();
    let c = Card {
        identity: f.alice.identity,
        address: f.home,
        time: 42,
        name: "Ім'я",
    };
    let mut b = [0u8; card::MAX_LEN];
    let n = c.encode(&mut b).unwrap();
    assert_eq!(Card::decode(&b[..n]), Ok(c));
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
        address: f.home,
        time: 0,
        name: "a name of more than thirty-two bytes",
    };
    assert_eq!(long.encode(&mut [0u8; 100]), Err(Error::TooLong));
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
        address: f.away,
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
    assert_eq!(n, 5 + 13 + 5 + 2 + 50 + 4);
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

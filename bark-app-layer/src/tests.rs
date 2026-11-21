use std::format;
use std::string::String;
use std::vec::Vec;

use crypto_box::aead::{AeadInPlace, KeyInit};
use crypto_box::{PublicKey, SecretKey};

use crate::card::{self, Card};
use crate::command::Summary;
use crate::keys::{self, Keys};
use crate::plain::{self, Header, Part, Record, Writer};
use crate::seal::{self, Fetch, Form};
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
    assert_eq!(MAX_APP_PAYLOAD, 223);
    assert_eq!(seal::BOX_OVERHEAD, 18);
    assert_eq!(seal::INTRO_OVERHEAD, 50);
    assert_eq!(seal::REPLY_OVERHEAD, 1);
    assert_eq!(seal::SUMMARY_LEN, 22);
    assert_eq!(Fetch::LEN, 36);
    assert_eq!(Fetch::MAX_LEN, 36 + 66);
    assert_eq!(crate::command::LIST_LEN, 201);
    assert_eq!(card::FIXED_LEN, 38);
    assert_eq!(card::MAX_LEN, 100);
    assert_eq!(card::MAX_TEXT_LEN, 5 + 160);
    // The longest text: 223 less the box, the header and the record's type.
    assert_eq!(seal::MAX_BOX_PLAIN - plain::HEADER_LEN - 1, 199);
    // A part's: one byte more, its number and the count.
    assert_eq!(seal::MAX_BOX_PLAIN - plain::HEADER_LEN - 2, 198);
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
        part: None,
        bytes: &t[..n],
    })
    .unwrap();
    let len = w.finish();
    let pair = seal::pair(&PublicKey::from(to.identity.0), &from.secret_key()).unwrap();
    let mut out = [0u8; MAX_PAYLOAD];
    let m = seal::seal_box(&pair, 7, &b[..len], &mut out).unwrap();
    out[..m].to_vec()
}

#[test]
fn a_box_opens_for_its_pair_only() {
    let f = fixture();
    let carol = Keys::from_seed(&[3; 16]);
    let payload = boxed_text(&f.alice, &f.bob, "hi");
    let mut out = [0u8; MAX_PAYLOAD];
    let bob_from_alice =
        seal::pair(&PublicKey::from(f.alice.identity.0), &f.bob.secret_key()).unwrap();
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
            part: None,
            bytes: b"hi"
        }))
    );
    let bob_from_carol =
        seal::pair(&PublicKey::from(carol.identity.0), &f.bob.secret_key()).unwrap();
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
    let pair = seal::pair(&PublicKey::from(f.alice.identity.0), &f.bob.secret_key()).unwrap();
    let payload = boxed_text(&f.alice, &f.bob, "hi");
    // The recipient tag is not sealed: changed, it only sends the payload
    // to a phone that cannot open it.
    for i in (1..payload.len()).filter(|&i| i != seal::RECIPIENT_AT) {
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
    let alice_from_bob =
        seal::pair(&PublicKey::from(f.bob.identity.0), &f.alice.secret_key()).unwrap();
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
        7,
        &SecretKey::from([5; 32]),
        b"abc",
        &mut out,
    )
    .unwrap();
    assert_eq!(bob.unseal(&out[seal::HEAD_LEN..n]).unwrap(), b"abc");
    let theirs = bob
        .public_key()
        .seal(&mut crypto_box::aead::OsRng, b"xyz")
        .unwrap();
    let mut p = Vec::from([Form::Introduction.format(), 7]);
    p.extend_from_slice(&theirs);
    let n = seal::open_intro(&bob, &p, &mut out).unwrap();
    assert_eq!(&out[..n], b"xyz");
}

#[test]
fn too_long_is_refused() {
    let f = fixture();
    let pair = seal::pair(&PublicKey::from(f.bob.identity.0), &f.alice.secret_key()).unwrap();
    let mut out = [0u8; MAX_PAYLOAD];
    let plain = [0u8; seal::MAX_BOX_PLAIN + 1];
    assert_eq!(
        seal::seal_box(&pair, 7, &plain, &mut out),
        Err(Error::TooLong)
    );
    let n = seal::seal_box(&pair, 7, &plain[1..], &mut out).unwrap();
    assert_eq!(n, MAX_APP_PAYLOAD);
    // The longest box goes on as a reply of the longest payload.
    let item = out;
    let mut reply = [0u8; MAX_PAYLOAD];
    let r = seal::seal_item(&f.alice.mailbox, 7, 1, 1, &item[..n], &mut reply);
    assert_eq!(r, Ok(MAX_PAYLOAD));
    let mut long = [0u8; MAX_APP_PAYLOAD + 1];
    long[0] = Form::Boxed.format();
    assert_eq!(
        seal::seal_item(&f.alice.mailbox, 7, 1, 1, &long, &mut reply),
        Err(Error::TooLong)
    );
}

#[test]
fn a_fetch_opens_under_its_key_only() {
    let f = fixture();
    let fetch = Fetch {
        sent: 3,
        address: f.away,
        recipient: 9,
        command: None,
    };
    let p = fetch.seal(&f.alice.mailbox, f.home.node, 7);
    assert_eq!(seal::recipient_of(&p), Some(7));
    assert_eq!(Fetch::open(&f.alice.mailbox, f.home.node, &p), Ok(fetch));
    assert_eq!(
        Fetch::open(&f.bob.mailbox, f.home.node, &p),
        Err(Error::Open)
    );
    // Another node under the same key, an earlier home, cannot take it.
    assert_eq!(
        Fetch::open(&f.alice.mailbox, f.away.node, &p),
        Err(Error::Open)
    );
    // A bit changed anywhere but the recipient tag, `siv` too, is refused.
    for i in (1..p.len()).filter(|&i| i != seal::RECIPIENT_AT) {
        let mut q = p;
        q[i] ^= 0x80;
        assert_eq!(
            Fetch::open(&f.alice.mailbox, f.home.node, &q),
            Err(Error::Open),
            "{i}"
        );
    }
}

/// Our `crypto_box_beforenm` is the one the crypto_box crate, checked
/// against libsodium, keeps inside: its box and a secretbox under our key
/// make the same bytes.
#[test]
fn our_shared_key_is_crypto_box_s() {
    let f = fixture();
    let (theirs, mine) = (PublicKey::from(f.bob.identity.0), f.alice.secret_key());
    let key = seal::beforenm(&theirs, &mine).unwrap();
    let nonce = [7u8; seal::NONCE_LEN];
    let mut a = *b"the same bytes";
    let mut b = a;
    let ta = crypto_box::SalsaBox::new(&theirs, &mine)
        .encrypt_in_place_detached((&nonce).into(), &[], &mut a)
        .unwrap();
    let tb = crypto_secretbox::XSalsa20Poly1305::new((&key).into())
        .encrypt_in_place_detached((&nonce).into(), &[], &mut b)
        .unwrap();
    assert_eq!((a, ta), (b, tb));
}

/// Both people of a pair derive the same two keys, and they differ from
/// each other and from the shared key.
#[test]
fn both_sides_of_a_pair_have_the_same_keys() {
    let f = fixture();
    let ab = seal::pair(&PublicKey::from(f.bob.identity.0), &f.alice.secret_key()).unwrap();
    let ba = seal::pair(&PublicKey::from(f.alice.identity.0), &f.bob.secret_key()).unwrap();
    assert_eq!((ab.mac, ab.enc), (ba.mac, ba.enc));
    let shared = seal::beforenm(&PublicKey::from(f.bob.identity.0), &f.alice.secret_key()).unwrap();
    assert!(ab.mac != ab.enc && ab.mac != shared && ab.enc != shared);
}

/// A public key of small order gives no shared key, as libsodium's
/// `crypto_box_beforenm` refuses it.
#[test]
fn a_public_key_of_small_order_has_no_pair() {
    let me = fixture().alice.secret_key();
    for small in [[0u8; 32], {
        let mut one = [0u8; 32];
        one[0] = 1;
        one
    }] {
        assert!(seal::pair(&PublicKey::from(small), &me).is_err());
    }
}

/// One plaintext makes one payload: no randomness enters. The direction
/// bit makes a message and its reflection two plaintexts.
#[test]
fn a_box_is_its_plaintext_s_alone() {
    let f = fixture();
    assert_eq!(
        boxed_text(&f.alice, &f.bob, "hi"),
        boxed_text(&f.alice, &f.bob, "hi")
    );
    let ab = boxed_text(&f.alice, &f.bob, "hi");
    let ba = boxed_text(&f.bob, &f.alice, "hi");
    assert_ne!(
        ab[seal::HEAD_LEN..seal::BOX_OVERHEAD],
        ba[seal::HEAD_LEN..seal::BOX_OVERHEAD]
    );
}

/// A box that does not check leaves nothing of its plaintext behind.
#[test]
fn a_forged_siv_opens_nothing() {
    let f = fixture();
    let pair = seal::pair(&PublicKey::from(f.alice.identity.0), &f.bob.secret_key()).unwrap();
    let mut p = boxed_text(&f.alice, &f.bob, "hi");
    p[seal::HEAD_LEN] ^= 1;
    let mut out = [0xaau8; MAX_PAYLOAD];
    assert_eq!(seal::open_box(&pair, &p, &mut out), Err(Error::Open));
    let len = p.len() - seal::BOX_OVERHEAD;
    assert!(out[..len].iter().all(|&b| b == 0));
}

/// A summary of the counts alone, as a fetch without a command gets
/// from a person who is not the owner.
fn counts(items: u8, remaining: u16) -> Summary {
    Summary {
        newest: crate::VERSION,
        items,
        remaining,
        owner: false,
        outcome: None,
    }
}

/// That summary, sealed.
fn summary_of(key: &[u8; 32], to: u8, round: u32, items: u8, remaining: u16) -> Vec<u8> {
    let mut out = [0u8; MAX_PAYLOAD];
    let n = seal::seal_summary(key, to, round, &counts(items, remaining), &mut out);
    out[..n].to_vec()
}

#[test]
fn a_summary_opens_in_its_round_only_and_tells_what_comes_and_what_remains() {
    let k = fixture().alice.mailbox;
    let p = summary_of(&k, 7, 10, seal::MAX_REPLIES, 1000);
    assert_eq!(
        (p[0], seal::recipient_of(&p)),
        (Form::Summary.format(), Some(7))
    );
    assert_eq!(
        seal::open_summary(&k, 10, &p),
        Ok(counts(seal::MAX_REPLIES, 1000))
    );
    assert_eq!(seal::open_summary(&k, 11, &p), Err(Error::Open));
    assert_eq!(
        seal::open_summary(&fixture().bob.mailbox, 10, &p),
        Err(Error::Open)
    );
    // Under its own key, derived: the mailbox key itself encrypts nothing.
    let r = seal::reply_key(&k);
    let fetch = seal::SivKeys::of(&k);
    assert!(r != k && r != fetch.mac && r != fetch.enc);
    assert_eq!(
        seal::open_summary(&r, 10, &summary_of(&r, 7, 10, 1, 0)),
        Ok(counts(1, 0))
    );
    assert_ne!(summary_of(&r, 7, 10, 1, 0), summary_of(&k, 7, 10, 1, 0));
    let mut q = p.clone();
    q[seal::SUMMARY_LEN - 1] ^= 1;
    assert_eq!(seal::open_summary(&k, 10, &q), Err(Error::Open));
    assert_eq!(
        seal::open_summary(&k, 10, &p[..seal::SUMMARY_LEN - 1]),
        Err(Error::Malformed)
    );
}

/// A reply gives back the item the mailbox kept, its form from the
/// reply's, its tag the reply's; at another round or place, or under
/// another key, it gives bytes that do not open.
#[test]
fn a_reply_gives_back_its_item_which_proves_itself() {
    let f = fixture();
    let k = f.alice.mailbox;
    let item = boxed_text(&f.bob, &f.alice, "kept");
    let alice = seal::pair(&PublicKey::from(f.bob.identity.0), &f.alice.secret_key()).unwrap();
    let mut out = [0u8; MAX_PAYLOAD];
    let n = seal::seal_item(&k, 9, 10, seal::MAX_REPLIES, &item, &mut out).unwrap();
    assert_eq!(n, item.len() + seal::REPLY_OVERHEAD);
    let reply = out[..n].to_vec();
    assert_eq!(
        (Form::of(&reply), seal::recipient_of(&reply)),
        (Ok(Form::BoxedItem), Some(9))
    );
    let mut got = [0u8; MAX_PAYLOAD];
    let m = seal::open_item(&k, 10, &reply, &mut got).unwrap();
    assert_eq!(got[0], item[0]);
    assert_eq!(got[seal::RECIPIENT_AT], 9);
    assert_eq!(got[seal::HEAD_LEN..m], item[seal::HEAD_LEN..]);
    let mut plain = [0u8; MAX_PAYLOAD];
    assert!(seal::open_box(&alice, &got[..m], &mut plain).is_ok());
    let other_place = {
        let mut r = reply.clone();
        r[seal::HEAD_LEN] = 1;
        r
    };
    for (key, round, r) in [
        (k, 11, &reply),
        (f.bob.mailbox, 10, &reply),
        (k, 10, &other_place),
    ] {
        let m = seal::open_item(&key, round, r, &mut got).unwrap();
        assert_eq!(
            seal::open_box(&alice, &got[..m], &mut plain),
            Err(Error::Open)
        );
    }
    // Place 0 is the summary's, and no place passes the batch.
    for place in [0, seal::MAX_REPLIES + 1] {
        assert_eq!(
            seal::seal_item(&k, 9, 10, place, &item, &mut out),
            Err(Error::TooLong)
        );
        let mut r = reply.clone();
        r[seal::HEAD_LEN] = place;
        assert_eq!(seal::open_item(&k, 10, &r, &mut got), Err(Error::Malformed));
    }
    // An introduction goes as its own form; nothing else goes at all.
    let mut intro = [0u8; MAX_PAYLOAD];
    let i = seal::seal_intro(
        &PublicKey::from(f.alice.identity.0),
        7,
        &SecretKey::from([5; 32]),
        b"abc",
        &mut intro,
    )
    .unwrap();
    let n = seal::seal_item(&k, 9, 10, 2, &intro[..i], &mut out).unwrap();
    assert_eq!(Form::of(&out[..n]), Ok(Form::IntroductionItem));
    let m = seal::open_item(&k, 10, &out[..n], &mut got).unwrap();
    assert!(seal::open_intro(&f.alice.secret_key(), &got[..m], &mut plain).is_ok());
    let summary = summary_of(&k, 9, 10, 1, 0);
    let fetch = Fetch {
        sent: 1,
        address: f.away,
        recipient: 9,
        command: None,
    }
    .seal(&k, f.home.node, 7);
    for not in [&summary[..], &fetch[..], b"item"] {
        assert!(!seal::forwardable(not));
        assert_eq!(
            seal::seal_item(&k, 9, 10, 1, not, &mut out),
            Err(Error::Malformed)
        );
    }
}

#[test]
fn a_card_reads_back_as_bytes_and_as_text() {
    let f = fixture();
    let c = Card {
        reads: crate::VERSION,
        identity: f.alice.identity,
        home: Some(f.home),
        now: None,
        time: 42,
        name: "Ім'я",
    };
    let mut b = [0u8; card::MAX_LEN];
    let n = c.encode(&mut b).unwrap();
    assert_eq!(n, 51 + 7);
    // The flags' high four bits: the newest payload version Alice reads.
    assert_eq!(&b[..2], &[3, 0x21]);
    assert_eq!(Card::decode(&b[..n]), Ok(c));
    let later = Card { reads: 15, ..c };
    let m = later.encode(&mut b).unwrap();
    assert_eq!(
        (b[1], Card::decode(&b[..m]).map(|c| c.reads)),
        (0xf1, Ok(15))
    );
    assert_eq!(
        Card { reads: 16, ..c }.encode(&mut b),
        Err(Error::Malformed)
    );
    c.encode(&mut b).unwrap();
    let away = card::Linked {
        address: f.away,
        at: 0x0102_0304,
    };
    let both = Card {
        now: Some(away),
        ..c
    };
    let n = both.encode(&mut b).unwrap();
    assert_eq!(n, 68 + 7);
    assert_eq!(b[1], 0x23);
    assert_eq!(&b[51..64], &f.away.encode());
    assert_eq!(&b[64..68], &[1, 2, 3, 4]);
    assert_eq!(Card::decode(&b[..n]), Ok(both));
    let temporary = Card {
        home: None,
        now: Some(away),
        ..c
    };
    let n = temporary.encode(&mut b).unwrap();
    assert_eq!(n, 55 + 7);
    // A node flag without room for the time.
    assert_eq!(
        Card::decode(&b[..card::FIXED_LEN + 15]),
        Err(Error::Malformed)
    );
    assert_eq!(b[1], 0x22);
    assert_eq!(Card::decode(&b[..n]), Ok(temporary));
    let homeless = Card { home: None, ..c };
    let n = homeless.encode(&mut b).unwrap();
    assert_eq!(n, 38 + 7);
    assert_eq!(b[1], 0x20);
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
        reads: crate::VERSION,
        identity: f.alice.identity,
        home: Some(f.home),
        now: None,
        time: 0,
        name: "a name of more than thirty-two bytes",
    };
    assert_eq!(long.encode(&mut [0u8; 100]), Err(Error::TooLong));
    // A card of another version, and a flag this version does not know.
    let short = Card { name: "", ..long };
    let mut b = [0u8; card::MAX_LEN];
    let n = short.encode(&mut b).unwrap();
    for (at, v) in [(0, 1), (0, 2), (0, 4), (1, 4), (1, 0x28)] {
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
        reads: crate::VERSION,
        identity: f.bob.identity,
        home: None,
        now: Some(card::Linked {
            address: f.away,
            at: 1,
        }),
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
    w.push(&Record::Card(c)).unwrap();
    let t = Record::Text {
        encoding: Encoding::Utf8,
        part: None,
        bytes: b"end",
    };
    w.push(&t).unwrap();
    assert_eq!(w.push(&t), Err(Error::Malformed));
    let n = w.finish();
    assert_eq!(n, 5 + 13 + 2 + 38 + 17 + 4);
    let (got, r) = plain::read(&b[..n]).unwrap();
    assert_eq!(got, h);
    let rs: Vec<_> = r.collect();
    assert_eq!(rs, [Ok(Record::Card(c)), Ok(t)]);
    assert!(plain::read(&[0x80, 0, 0, 0, 0]).is_err());
    // A home given: its address and key, 46 bytes with its type, and
    // records may follow it.
    let home = Record::Home {
        address: f.home,
        key: [7; 32],
    };
    let mut w = Writer::new(&mut b, &Header { address: None, ..h }).unwrap();
    w.push(&home).unwrap();
    w.push(&t).unwrap();
    let n = w.finish();
    assert_eq!(n, 5 + 46 + 4);
    assert_eq!(b[5], 7);
    let (_, r) = plain::read(&b[..n]).unwrap();
    assert_eq!(r.collect::<Vec<_>>(), [Ok(home), Ok(t)]);
    let (_, mut r) = plain::read(&b[..5 + 45]).unwrap();
    assert_eq!(r.next(), Some(Err(Error::Malformed)), "cut short");
    let (_, mut r) = plain::read(&[0, 0, 0, 0, 0, 12]).unwrap();
    assert_eq!(r.next(), Some(Err(Error::Malformed)));
    assert_eq!(r.next(), None);
}

#[test]
fn a_reaction_is_the_text_it_answers_and_one_emoji() {
    let h = Header {
        greater: false,
        sent: 7,
        address: None,
    };
    let kiss = "🧑🏻\u{200d}❤️\u{200d}💋\u{200d}🧑🏿";
    assert_eq!(kiss.len(), plain::MAX_REACTION);
    // A flag, a keycap, a family and a letter are each one character.
    for emoji in ["👍", "❤️", "🇺🇦", "#️⃣", "👨\u{200d}👩\u{200d}👧", "é", kiss]
    {
        let r = Record::React { sent: 5, emoji };
        let mut b = [0u8; MAX_PAYLOAD];
        let mut w = Writer::new(&mut b, &h).unwrap();
        w.push(&r).unwrap();
        // It runs to the end, so nothing follows it.
        assert_eq!(w.push(&r), Err(Error::Malformed));
        let n = w.finish();
        assert_eq!(
            &b[plain::HEADER_LEN..plain::HEADER_LEN + 5],
            &[3, 0, 0, 0, 5]
        );
        assert_eq!(&b[plain::HEADER_LEN + 5..n], emoji.as_bytes());
        let (_, mut rs) = plain::read(&b[..n]).unwrap();
        assert_eq!(rs.next(), Some(Ok(r)));
        assert_eq!(rs.next(), None);
    }
    // None, two, or one too long is not written and does not read.
    let mut kiss_more = String::from(kiss);
    kiss_more.push('\u{fe0f}');
    assert_eq!(kiss_more.len(), plain::MAX_REACTION + 3);
    for emoji in ["", "👍👍", "ok", kiss_more.as_str()] {
        let mut b = [0u8; MAX_PAYLOAD];
        let mut w = Writer::new(&mut b, &h).unwrap();
        assert_eq!(
            w.push(&Record::React { sent: 5, emoji }),
            Err(Error::Malformed)
        );
        let mut bad = std::vec![0, 0, 0, 0, 0, 3, 0, 0, 0, 5];
        bad.extend_from_slice(emoji.as_bytes());
        let (_, mut rs) = plain::read(&bad).unwrap();
        assert_eq!(rs.next(), Some(Err(Error::Malformed)));
    }
    // Cut short, not UTF-8, or of a type no longer used.
    for bad in [
        &[0, 0, 0, 0, 0, 3, 0, 0, 0][..],
        &[0, 0, 0, 0, 0, 3, 0, 0, 0, 5, 0xff],
        &[0, 0, 0, 0, 0, 7, 0, 0, 0, 5],
    ] {
        let (_, mut rs) = plain::read(bad).unwrap();
        assert_eq!(rs.next(), Some(Err(Error::Malformed)));
    }
}

#[test]
fn a_text_part_carries_its_number_and_count_in_one_byte() {
    let h = Header {
        greater: false,
        sent: 7,
        address: None,
    };
    for (number, of, byte) in [(1, 2, 0x01), (2, 3, 0x12), (16, 16, 0xff), (1, 16, 0x0f)] {
        let t = Record::Text {
            encoding: Encoding::Windows1251,
            part: Some(Part { number, of }),
            bytes: b"ab",
        };
        let mut b = [0u8; MAX_PAYLOAD];
        let mut w = Writer::new(&mut b, &h).unwrap();
        w.push(&t).unwrap();
        let n = w.finish();
        assert_eq!(&b[plain::HEADER_LEN..n], &[6, byte, b'a', b'b']);
        let (_, mut r) = plain::read(&b[..n]).unwrap();
        assert_eq!(r.next(), Some(Ok(t)));
        assert_eq!(r.next(), None);
    }
    // A UTF-8 part is type 5, and a whole text keeps types 1 and 2.
    let mut b = [0u8; MAX_PAYLOAD];
    let mut w = Writer::new(&mut b, &h).unwrap();
    w.push(&Record::Text {
        encoding: Encoding::Utf8,
        part: Some(Part { number: 1, of: 2 }),
        bytes: b"x",
    })
    .unwrap();
    let n = w.finish();
    assert_eq!(&b[plain::HEADER_LEN..n], &[5, 0x01, b'x']);
}

#[test]
fn a_part_out_of_its_count_is_refused() {
    let h = Header {
        greater: false,
        sent: 7,
        address: None,
    };
    for (number, of) in [(1, 1), (0, 2), (3, 2), (1, 17)] {
        let mut b = [0u8; MAX_PAYLOAD];
        let mut w = Writer::new(&mut b, &h).unwrap();
        let t = Record::Text {
            encoding: Encoding::Utf8,
            part: Some(Part { number, of }),
            bytes: b"x",
        };
        assert_eq!(w.push(&t), Err(Error::Malformed), "{number} of {of}");
    }
    // Read: one of one, a number past the count, no part byte.
    for rec in [&[5u8, 0x00, b'x'][..], &[6, 0x21, b'x'], &[5]] {
        let mut p = std::vec![0, 0, 0, 0, 7];
        p.extend_from_slice(rec);
        let (_, mut r) = plain::read(&p).unwrap();
        assert_eq!(r.next(), Some(Err(Error::Malformed)), "{rec:?}");
    }
}

#[test]
fn the_longest_parts_fill_a_box() {
    let f = fixture();
    let pair = seal::pair(&PublicKey::from(f.bob.identity.0), &f.alice.secret_key()).unwrap();
    let h = Header {
        greater: true,
        sent: 1,
        address: None,
    };
    let part = seal::MAX_BOX_PLAIN - plain::HEADER_LEN - 2;
    for (len, fits) in [(part, true), (part + 1, false)] {
        let mut b = [0u8; MAX_PAYLOAD];
        let mut w = Writer::new(&mut b, &h).unwrap();
        w.push(&Record::Text {
            encoding: Encoding::Utf8,
            part: Some(Part { number: 1, of: 2 }),
            bytes: &[b'a'; MAX_PAYLOAD][..len],
        })
        .unwrap();
        let n = w.finish();
        let mut out = [0u8; MAX_PAYLOAD];
        let r = seal::seal_box(&pair, 7, &b[..n], &mut out);
        assert_eq!(
            r.map(|m| m == MAX_APP_PAYLOAD),
            if fits { Ok(true) } else { Err(Error::TooLong) }
        );
    }
}

mod mailbox {
    use std::boxed::Box;
    use std::vec::Vec;

    use bark_link::phones::HomeKey;
    use bark_proto::MsgId;

    use crate::mailbox::{Arrival, Mailbox, Person, RamRing, Ring};
    use crate::seal::{self, Fetch, Form};
    use crate::vectors::fixture;
    use crate::MAX_PAYLOAD;

    type Small = RamRing<32>;

    /// The tag `item` payloads carry: their second byte, `t` of "item".
    const ITEM_TAG: u8 = b't';

    /// Alice, whose home the node is, at place 0, her tag the items'.
    fn alice() -> Person {
        Person {
            number: 1,
            tag: Some(ITEM_TAG),
            key: HomeKey::Home(fixture().alice.mailbox),
            ..Person::default()
        }
    }

    fn home() -> Mailbox<Box<Small>> {
        let mut m = Mailbox::new(Box::new(Small::new()), fixture().home.node);
        m.set_person(0, Some(alice()));
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
        fn clear(&mut self, slot: u32, at: usize, bits: u8) {
            (**self).clear(slot, at, bits)
        }
        fn erase(&mut self, page: u32) {
            (**self).erase(page)
        }
    }

    fn fetch(key: &[u8; 32], sent: u32) -> crate::seal::FetchPayload {
        Fetch {
            sent,
            address: fixture().away,
            recipient: 9,
            command: None,
        }
        .seal(key, fixture().home.node, 7)
    }

    /// What a requester gets from a reply.
    #[derive(Debug, PartialEq)]
    enum Got {
        /// The summary: the round's items and those unsent after them.
        Summary(u8, u16),
        /// The item's number, as the node handed it over, and the item as
        /// the requester rebuilds it.
        Item(u32, Vec<u8>),
    }

    fn open(key: &[u8; 32], round: u32, reply: &[u8], number: u32) -> Got {
        if Form::of(reply) == Ok(Form::Summary) {
            assert_eq!(number, 0);
            let s = seal::open_summary(key, round, reply).unwrap();
            return Got::Summary(s.items, s.remaining);
        }
        let mut item = [0u8; MAX_PAYLOAD];
        let len = seal::open_item(key, round, reply, &mut item).unwrap();
        Got::Item(number, item[..len].to_vec())
    }

    /// Item `i` as the requester rebuilds it: with the fetch's recipient
    /// tag in place of the one it was kept with.
    fn opened(i: u32) -> Vec<u8> {
        let mut b = item(i);
        b[seal::RECIPIENT_AT] = 9;
        b
    }

    /// Every reply due now, opened as Alice opens them, in order.
    fn drain<R: Ring>(m: &mut Mailbox<R>, round: u32) -> Vec<Got> {
        let key = fixture().alice.mailbox;
        let mut got = Vec::new();
        let mut out = [0u8; MAX_PAYLOAD];
        while let Some(o) = m.next(&mut out) {
            assert_eq!(o.address, fixture().away);
            assert_eq!(o.person, 0);
            // The fetch's recipient tag, for the node it is answered at.
            assert_eq!(seal::recipient_of(&out[..o.len]), Some(9));
            got.push(open(&key, round, &out[..o.len], o.number));
        }
        assert!(!m.reply_due());
        got
    }

    /// A payload of the format's version for the recipient tag `tag`.
    fn tagged(tag: u8, i: u8) -> Vec<u8> {
        std::vec![seal::Form::Boxed.format(), tag, i, i, i]
    }

    /// A node nobody is paired with keeps everything, for guests: no fetch
    /// opens, and no person's inbox holds it.
    #[test]
    fn a_node_with_no_people_keeps_everything_for_guests() {
        let mut m = Mailbox::new(Small::new(), fixture().home.node);
        assert_eq!(m.arrive(MsgId(1), &tagged(3, 1)), Arrival::Kept(1));
        assert!(!m.is_home());
        let f = fetch(&fixture().alice.mailbox, 5);
        assert_eq!(m.arrive(MsgId(3), &f), Arrival::Refused);
        assert!(!m.reply_due());
        let mut out = [0u8; MAX_PAYLOAD];
        assert_eq!((m.unread_count(0), m.unread(0, &mut out)), (0, None));
        assert_eq!(m.for_tag(3, 0, &mut out).map(|u| u.0), Some(1));
    }

    /// A fetch sealed for one node is refused at another that holds the
    /// same person's key, as an earlier home does, and taken at its own:
    /// two homes never answer one round.
    #[test]
    fn a_fetch_for_one_home_is_refused_at_another_with_the_same_key() {
        let mut earlier = Mailbox::new(Small::new(), fixture().away.node);
        earlier.set_person(0, Some(alice()));
        earlier.arrive(MsgId(1), &item(1));
        let f = fetch(&fixture().alice.mailbox, 5);
        assert_eq!(earlier.arrive(MsgId(2), &f), Arrival::Refused);
        assert!(!earlier.reply_due());
        assert_eq!(earlier.person(0).map(|p| p.last_fetch), Some(0));
        let mut m = home();
        m.arrive(MsgId(1), &item(1));
        assert_eq!(
            m.arrive(MsgId(2), &f),
            Arrival::Fetch {
                person: 0,
                command: None,
            }
        );
        assert!(m.reply_due());
    }

    /// Each person's inbox holds the items with their tag, read after the
    /// last they confirmed; a guest reads its own by tag, and reading takes
    /// nothing.
    #[test]
    fn each_person_and_each_guest_reads_by_tag() {
        let mut m = Mailbox::new(Small::new(), fixture().home.node);
        m.set_person(
            0,
            Some(Person {
                tag: Some(1),
                ..alice()
            }),
        );
        m.set_person(
            5,
            Some(Person {
                number: 2,
                tag: Some(6),
                key: HomeKey::None,
                ..alice()
            }),
        );
        for (i, tag) in [(1u8, 1u8), (2, 5), (3, 6), (4, 1), (5, 5)] {
            assert_eq!(
                m.arrive(MsgId(i.into()), &tagged(tag, i)),
                Arrival::Kept(i.into())
            );
        }
        let mut out = [0u8; MAX_PAYLOAD];
        assert_eq!(m.unread_count(0), 2);
        assert_eq!(m.unread(0, &mut out).map(|u| u.0), Some(1));
        assert!(m.confirm_read(0, 1));
        assert!(!m.confirm_read(0, 1), "confirmed already");
        assert_eq!(m.person(0).map(|p| p.read), Some(1));
        assert_eq!(m.unread(0, &mut out).map(|u| u.0), Some(4));
        assert_eq!(m.unread_head(0), Some((4, MsgId(4))));
        assert_eq!(m.unread_count(0), 1);
        // One more for her, counted without a scan.
        m.arrive(MsgId(6), &tagged(1, 6));
        assert_eq!(
            (m.unread_count(0), m.unread_head(0)),
            (2, Some((4, MsgId(4))))
        );
        m.confirm_read(0, 4);
        assert_eq!(
            (m.unread_count(0), m.unread_head(0)),
            (1, Some((6, MsgId(6))))
        );
        m.confirm_read(0, 6);
        assert_eq!((m.unread_count(0), m.unread_head(0)), (0, None));
        assert_eq!(
            (m.unread_count(5), m.unread(5, &mut out).map(|u| u.0)),
            (1, Some(3))
        );
        assert_eq!(m.for_tag(1, 0, &mut out).map(|u| u.0), Some(1));
        // Nobody's at place 2.
        assert_eq!(m.unread_count(2), 0);
        assert!(!m.confirm_read(2, 1));
        // A guest reads its own after the number it keeps.
        assert_eq!(m.for_tag(5, 0, &mut out).map(|u| u.0), Some(2));
        assert_eq!(&out[..5], &tagged(5, 2)[..]);
        assert_eq!(m.for_tag(5, 2, &mut out).map(|u| u.0), Some(5));
        assert_eq!(m.for_tag(5, 5, &mut out), None);
        assert_eq!(m.for_tag(5, 0, &mut out).map(|u| u.0), Some(2));
        assert_eq!(m.for_tag(9, 0, &mut out), None);
        // A number from before the ring began again reads as 0.
        assert_eq!(m.for_tag(5, 900, &mut out).map(|u| u.0), Some(2));
        // A fetch sends its person's alone, and of those the unconfirmed:
        // hers are all confirmed, and the guests' are not hers.
        let f = fetch(&fixture().alice.mailbox, 50);
        assert_eq!(
            m.arrive(MsgId(9), &f),
            Arrival::Fetch {
                person: 0,
                command: None,
            }
        );
        assert_eq!(m.person(0).map(|p| p.last_fetch), Some(50));
        assert_eq!(drain(&mut m, 50), [Got::Summary(0, 0)]);
    }

    /// One person's reading and fetching clears their own bit alone: the
    /// other person's fetch still sends the item they share.
    #[test]
    fn one_person_s_reading_marks_nothing_of_another_s() {
        let mut m = Mailbox::new(Small::new(), fixture().home.node);
        let bob = Person {
            number: 2,
            tag: Some(ITEM_TAG),
            key: HomeKey::Home(fixture().bob.mailbox),
            ..Person::default()
        };
        m.set_person(0, Some(alice()));
        m.set_person(3, Some(bob));
        for i in 1..=3 {
            m.arrive(MsgId(i), &item(i));
        }
        m.confirm_read(0, 2);
        assert_eq!(numbers(&fetch_all(&mut m, 100)), [3, 1]);
        // Bob's fetch gets all three, his bits untouched.
        let k = fixture().bob.mailbox;
        assert_eq!(
            m.arrive(MsgId(0), &fetch(&k, 7)),
            Arrival::Fetch {
                person: 3,
                command: None,
            }
        );
        let mut out = [0u8; MAX_PAYLOAD];
        let mut got = Vec::new();
        while let Some(o) = m.next(&mut out) {
            assert_eq!(o.person, 3);
            got.push(open(&k, 7, &out[..o.len], o.number));
        }
        assert_eq!(numbers(&got), [3, 2, 1]);
    }

    /// Two people's rounds take the send slot in turn.
    #[test]
    fn rounds_take_turns() {
        let mut m = Mailbox::new(Small::new(), fixture().home.node);
        let bob = Person {
            number: 2,
            tag: Some(ITEM_TAG),
            key: HomeKey::Home(fixture().bob.mailbox),
            ..Person::default()
        };
        m.set_person(1, Some(alice()));
        m.set_person(4, Some(bob));
        for i in 1..=3 {
            m.arrive(MsgId(i), &item(i));
        }
        assert_eq!(
            m.arrive(MsgId(0), &fetch(&fixture().alice.mailbox, 9)),
            Arrival::Fetch {
                person: 1,
                command: None,
            }
        );
        assert_eq!(
            m.arrive(MsgId(0), &fetch(&fixture().bob.mailbox, 9)),
            Arrival::Fetch {
                person: 4,
                command: None,
            }
        );
        let mut out = [0u8; MAX_PAYLOAD];
        let mut order = Vec::new();
        while let Some(o) = m.next(&mut out) {
            order.push((o.person, o.number));
        }
        assert_eq!(
            order,
            [
                (1, 0),
                (4, 0),
                (1, 3),
                (4, 3),
                (1, 2),
                (4, 2),
                (1, 1),
                (4, 1)
            ]
        );
    }

    /// A record made anew claims nothing kept before it for a fetch: a
    /// bit left set by the person before it at its place counts for
    /// nothing. Its phone reads every item with its tag, as a person who
    /// pairs again wants the mail that came for them meanwhile.
    #[test]
    fn a_new_person_claims_no_older_item_but_reads_it() {
        let mut m = home();
        for i in 1..=3 {
            m.arrive(MsgId(i), &item(i));
        }
        let newcomer = Person {
            number: 7,
            from: 3,
            ..alice()
        };
        m.set_person(0, Some(newcomer));
        m.arrive(MsgId(4), &item(4));
        assert_eq!(numbers(&fetch_all(&mut m, 100)), [4]);
        // The inbox goes by tag: the newcomer's phone reads the older ones.
        let mut out = [0u8; MAX_PAYLOAD];
        assert_eq!(m.unread(0, &mut out).map(|u| u.0), Some(1));
        assert_eq!(m.unread_count(0), 4);
        assert!(m.confirm_read(0, 1));
        assert_eq!(m.unread(0, &mut out).map(|u| u.0), Some(2));
    }

    /// Two phones of one person: the same key and tag at two places. A
    /// fetch opens under the first and clears its bit alone.
    #[test]
    fn two_phones_of_one_person() {
        let mut m = Mailbox::new(Small::new(), fixture().home.node);
        m.set_person(2, Some(alice()));
        m.set_person(
            6,
            Some(Person {
                number: 9,
                ..alice()
            }),
        );
        m.arrive(MsgId(1), &item(1));
        let mut out = [0u8; MAX_PAYLOAD];
        assert_eq!(m.unread(2, &mut out).map(|u| u.0), Some(1));
        assert_eq!(m.unread(6, &mut out).map(|u| u.0), Some(1));
        assert_eq!(
            m.arrive(MsgId(0), &fetch(&fixture().alice.mailbox, 5)),
            Arrival::Fetch {
                person: 2,
                command: None,
            }
        );
        while m.next(&mut out).is_some() {}
        // The second place's bit is still set: its own fetch gets it.
        m.set_person(2, None);
        assert_eq!(
            m.arrive(MsgId(0), &fetch(&fixture().alice.mailbox, 6)),
            Arrival::Fetch {
                person: 6,
                command: None,
            }
        );
        assert_eq!(m.next(&mut out).map(|o| (o.person, o.number)), Some((6, 0)));
        assert_eq!(m.next(&mut out).map(|o| (o.person, o.number)), Some((6, 1)));
    }

    #[test]
    fn a_guest_s_item_leaves_only_as_the_ring_wraps() {
        // Two pages of 16.
        let mut m = Mailbox::new(RamRing::<32>::new(), fixture().home.node);
        for i in 1..=32u8 {
            m.arrive(MsgId(i.into()), &tagged(7, i));
        }
        let mut out = [0u8; MAX_PAYLOAD];
        assert_eq!(m.for_tag(7, 0, &mut out).map(|u| u.0), Some(1));
        m.arrive(MsgId(33), &tagged(7, 33));
        // The page of 1 to 16 made room.
        assert_eq!(m.for_tag(7, 0, &mut out).map(|u| u.0), Some(17));
    }

    /// The erased slots of the page a wrapped ring makes room in read tag
    /// 0xFF: a person whose tag that is has no item there.
    #[test]
    fn an_erased_slot_is_nobody_s_item() {
        let mut m = Mailbox::new(RamRing::<32>::new(), fixture().home.node);
        let ff = Person {
            number: 1,
            tag: Some(0xFF),
            ..Person::default()
        };
        m.set_person(0, Some(ff));
        for i in 1..=33u8 {
            m.arrive(MsgId(i.into()), &tagged(7, i));
        }
        assert_eq!(m.unread_count(0), 0);
        assert_eq!(m.unread_head(0), None);
    }

    /// The numbers of the items a round's replies carried, the summary
    /// left out.
    fn numbers(got: &[Got]) -> Vec<u32> {
        got.iter()
            .filter_map(|g| match g {
                Got::Item(n, _) => Some(*n),
                Got::Summary(..) => None,
            })
            .collect()
    }

    /// A boxed payload for Alice's tag, `t`, numbered `i`.
    fn item(i: u32) -> Vec<u8> {
        let mut b = std::format!("item {i}").into_bytes();
        b[0] = Form::Boxed.format();
        b
    }

    /// The items unsent after the batch, as its summary, first, says.
    fn remaining(got: &[Got]) -> u16 {
        match got.first() {
            Some(Got::Summary(_, r)) => *r,
            g => panic!("no summary first: {g:?}"),
        }
    }

    /// A fetch from Alice as `round`, and every reply it gets: the summary
    /// first, counting the items after it.
    fn fetch_all<R: Ring>(m: &mut Mailbox<R>, round: u32) -> Vec<Got> {
        let k = fixture().alice.mailbox;
        assert_eq!(
            m.arrive(MsgId(0), &fetch(&k, round)),
            Arrival::Fetch {
                person: 0,
                command: None,
            }
        );
        let got = drain(m, round);
        let items = numbers(&got).len();
        assert!(matches!(got[0], Got::Summary(n, _) if usize::from(n) == items));
        got
    }

    #[test]
    fn a_fetch_gets_the_unsent_newest_first_once() {
        let mut m = home();
        for i in 1..=5 {
            assert_eq!(m.arrive(MsgId(i), &item(i)), Arrival::Kept(i));
        }
        assert_eq!(
            fetch_all(&mut m, 100),
            [
                Got::Summary(5, 0),
                Got::Item(5, opened(5)),
                Got::Item(4, opened(4)),
                Got::Item(3, opened(3)),
                Got::Item(2, opened(2)),
                Got::Item(1, opened(1))
            ]
        );
        // Sent, they stay in the ring.
        assert_eq!(m.oldest(), 1);
        assert_eq!(fetch_all(&mut m, 101), [Got::Summary(0, 0)]);
    }

    #[test]
    fn a_fetch_with_nothing_new_gets_one_reply_that_says_so() {
        let mut m = home();
        assert_eq!(fetch_all(&mut m, 100), [Got::Summary(0, 0)]);
        // And nothing after it.
        let mut out = [0u8; MAX_PAYLOAD];
        assert_eq!(m.next(&mut out), None);
    }

    #[test]
    fn an_item_after_the_fetch_waits_for_the_next() {
        let mut m = home();
        m.arrive(MsgId(1), &item(1));
        let k = fixture().alice.mailbox;
        m.arrive(MsgId(0), &fetch(&k, 100));
        let mut out = [0u8; MAX_PAYLOAD];
        assert_eq!(m.next(&mut out).map(|o| o.number), Some(0));
        assert_eq!(m.next(&mut out).map(|o| o.number), Some(1));
        m.arrive(MsgId(2), &item(2));
        assert_eq!(m.next(&mut out), None);
        assert!(!m.reply_due());
        m.arrive(MsgId(3), &item(3));
        assert_eq!(numbers(&fetch_all(&mut m, 101)), [3, 2]);
    }

    /// What no reply carries, a box longer than the app writes, a
    /// mailbox's reply delivered here or a payload of another version, is
    /// kept and read at the node, as a guest's item is, and no fetch
    /// counts or sends it.
    #[test]
    fn an_item_no_reply_carries_is_read_at_the_node_alone() {
        let mut m = home();
        m.arrive(MsgId(1), &item(1));
        let mut long = [0x42; MAX_PAYLOAD];
        long[0] = Form::Boxed.format();
        long[seal::RECIPIENT_AT] = ITEM_TAG;
        let summary = super::summary_of(&fixture().alice.mailbox, ITEM_TAG, 5, 1, 0);
        let mut other = item(4);
        other[0] = 0x30;
        for (n, p) in [(2, &long[..]), (3, &summary[..]), (4, &other[..])] {
            assert!(!seal::forwardable(p));
            assert_eq!(m.arrive(MsgId(n), p), Arrival::Kept(n));
        }
        assert_eq!(m.unread_count(0), 4);
        assert_eq!(
            fetch_all(&mut m, 100),
            [Got::Summary(1, 0), Got::Item(1, opened(1))]
        );
        let mut b = [0u8; MAX_PAYLOAD];
        m.confirm_read(0, 1);
        assert_eq!(m.unread(0, &mut b), Some((2, MsgId(2), MAX_PAYLOAD)));
    }

    #[test]
    fn a_backlog_comes_in_batches_newest_first_down_to_none_remaining() {
        let mut m = Mailbox::new(Box::new(RamRing::<128>::new()), fixture().home.node);
        m.set_person(0, Some(alice()));
        for i in 1..=70 {
            m.arrive(MsgId(i), &item(i));
        }
        let mut batches = Vec::new();
        for round in 100.. {
            let got = fetch_all(&mut m, round);
            let n = numbers(&got);
            assert!(n.windows(2).all(|w| w[0] == w[1] + 1));
            batches.push((n[0], *n.last().unwrap(), remaining(&got)));
            if remaining(&got) == 0 {
                break;
            }
        }
        assert_eq!(batches, [(70, 39, 38), (38, 7, 6), (6, 1, 0)]);
    }

    #[test]
    fn a_ring_larger_than_a_batch_sends_its_newest_and_overwrites_by_age() {
        let mut m = Mailbox::new(Box::new(RamRing::<64>::new()), fixture().home.node);
        m.set_person(0, Some(alice()));
        for i in 1..=70 {
            m.arrive(MsgId(i), &item(i));
        }
        // Item 65 erased the page of 1 to 16: 17 to 70 are unsent.
        let got = fetch_all(&mut m, 100);
        assert_eq!(numbers(&got), (39..=70).rev().collect::<Vec<_>>());
        assert_eq!(remaining(&got), 22);
        // Items 81 on erase the page of 17 to 32, sent or not.
        for i in 71..=81 {
            m.arrive(MsgId(i), &item(i));
        }
        let got = fetch_all(&mut m, 101);
        let want: Vec<u32> = (71..=81).rev().chain((33..=38).rev()).collect();
        assert_eq!(numbers(&got), want);
        assert_eq!(remaining(&got), 0);
    }

    #[test]
    fn a_fetch_under_another_key_is_refused_and_not_kept() {
        let mut m = home();
        m.arrive(MsgId(1), &item(1));
        let other = fixture().bob.mailbox;
        assert_eq!(m.arrive(MsgId(0), &fetch(&other, 100)), Arrival::Refused);
        assert_eq!(m.newest(), 1);
        assert!(drain(&mut m, 100).is_empty());
        assert_eq!(numbers(&fetch_all(&mut m, 100)), [1]);
    }

    /// The last fetch is the record's: after a reboot the node gives it
    /// back with the person, and a replay is refused.
    #[test]
    fn a_replayed_fetch_is_refused_even_after_a_reboot() {
        let mut m = home();
        m.arrive(MsgId(1), &item(1));
        let k = fixture().alice.mailbox;
        let f = fetch(&k, 100);
        assert_eq!(
            m.arrive(MsgId(0), &f),
            Arrival::Fetch {
                person: 0,
                command: None,
            }
        );
        assert_eq!(m.arrive(MsgId(0), &f), Arrival::Refused);
        assert_eq!(m.arrive(MsgId(0), &fetch(&k, 99)), Arrival::Refused);
        let kept = m.person(0);
        let ring = Box::new(clone_ring(m.ring()));
        let mut m = Mailbox::new(ring, fixture().home.node);
        m.set_person(0, kept);
        assert!(m.is_home());
        assert_eq!(m.newest(), 1);
        assert_eq!(m.arrive(MsgId(0), &f), Arrival::Refused);
        assert_eq!(
            m.arrive(MsgId(0), &fetch(&k, 101)),
            Arrival::Fetch {
                person: 0,
                command: None,
            }
        );
    }

    #[test]
    fn a_reboot_between_two_fetches_keeps_the_marks() {
        let mut m = Mailbox::new(Box::new(RamRing::<64>::new()), fixture().home.node);
        m.set_person(0, Some(alice()));
        for i in 1..=40 {
            m.arrive(MsgId(i), &item(i));
        }
        let got = fetch_all(&mut m, 100);
        assert_eq!(numbers(&got), (9..=40).rev().collect::<Vec<_>>());
        assert_eq!(remaining(&got), 8);
        let kept = m.person(0);
        let mut m = Mailbox::new(Box::new(clone_ring(m.ring())), fixture().home.node);
        m.set_person(0, kept);
        assert_eq!(m.newest(), 40);
        let got = fetch_all(&mut m, 101);
        assert_eq!(numbers(&got), (1..=8).rev().collect::<Vec<_>>());
        assert_eq!(remaining(&got), 0);
    }

    /// A reboot ends the round: what it had handed over is sent, the rest
    /// waits for the next fetch, and no place seals a second item.
    #[test]
    fn a_reboot_in_a_round_leaves_the_rest_for_the_next_fetch() {
        let mut m = home();
        for i in 1..=3 {
            m.arrive(MsgId(i), &item(i));
        }
        let k = fixture().alice.mailbox;
        m.arrive(MsgId(0), &fetch(&k, 100));
        let mut out = [0u8; MAX_PAYLOAD];
        assert_eq!(m.next(&mut out).map(|o| o.number), Some(0));
        assert_eq!(m.next(&mut out).map(|o| o.number), Some(3));
        let kept = m.person(0);
        let mut m = Mailbox::new(Box::new(clone_ring(m.ring())), fixture().home.node);
        m.set_person(0, kept);
        assert!(!m.reply_due());
        assert_eq!(numbers(&fetch_all(&mut m, 101)), [2, 1]);
    }

    /// A key changed ends the round under the old one.
    #[test]
    fn a_new_key_ends_the_round() {
        let mut m = home();
        m.arrive(MsgId(1), &item(1));
        m.arrive(MsgId(0), &fetch(&fixture().alice.mailbox, 100));
        assert!(m.reply_due());
        m.set_person(
            0,
            Some(Person {
                key: HomeKey::None,
                ..alice()
            }),
        );
        assert!(!m.reply_due());
        let mut out = [0u8; MAX_PAYLOAD];
        assert_eq!(m.next(&mut out), None);
    }

    /// A reply lost on the way is still in the ring: the phone reads it at
    /// home, after the last item it confirmed.
    #[test]
    fn a_lost_reply_s_item_is_still_read_at_home() {
        let mut m = home();
        m.arrive(MsgId(0xa1), &item(1));
        m.arrive(MsgId(0xa2), &item(2));
        assert_eq!(numbers(&fetch_all(&mut m, 100)), [2, 1]);
        let mut b = [0u8; MAX_PAYLOAD];
        assert_eq!(m.unread(0, &mut b), Some((1, MsgId(0xa1), item(1).len())));
        m.confirm_read(0, 1);
        assert_eq!(m.unread(0, &mut b), Some((2, MsgId(0xa2), item(2).len())));
        assert_eq!(&b[..item(2).len()], item(2).as_slice());
    }

    /// What the phone at the node confirms is not sent to a fetch.
    #[test]
    fn an_item_read_at_home_counts_as_sent() {
        let mut m = home();
        for i in 1..=3 {
            m.arrive(MsgId(i), &item(i));
        }
        m.confirm_read(0, 1);
        m.confirm_read(0, 2);
        assert_eq!(numbers(&fetch_all(&mut m, 100)), [3]);
    }

    fn clone_ring<const N: usize>(r: &RamRing<N>) -> RamRing<N> {
        let mut c = RamRing::<N>::new();
        let mut b = [0u8; 256];
        for s in 0..N as u32 {
            r.read(s, &mut b);
            c.write(s, &b);
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
        assert_eq!(m.unread_count(0), 24);
        let mut b = [0u8; MAX_PAYLOAD];
        assert_eq!(m.item(16, &mut b), None);
        assert_eq!(m.item(17, &mut b), Some(item(17).len()));
        let got = fetch_all(&mut m, 100);
        assert_eq!(numbers(&got), (17..=40).rev().collect::<Vec<_>>());
        assert_eq!(remaining(&got), 0);
    }

    #[test]
    fn a_reply_given_back_goes_again() {
        let mut m = home();
        m.arrive(MsgId(1), &item(1));
        m.arrive(MsgId(2), &item(2));
        let k = fixture().alice.mailbox;
        m.arrive(MsgId(0), &fetch(&k, 100));
        let mut out = [0u8; MAX_PAYLOAD];
        // The summary first, again first when given back.
        let summary = m.next(&mut out).unwrap();
        assert_eq!(summary.number, 0);
        m.given_back(summary.msg_id);
        assert_eq!(m.next(&mut out), Some(summary));
        let first = m.next(&mut out).unwrap();
        assert_eq!(first.number, 2);
        m.given_back(first.msg_id);
        let again = m.next(&mut out).unwrap();
        assert_eq!(again, first);
        assert_eq!(numbers(&drain(&mut m, 100)), [1]);
        // Nothing new, given back, goes again too.
        m.arrive(MsgId(0), &fetch(&k, 101));
        let nothing = m.next(&mut out).unwrap();
        assert_eq!(nothing.number, 0);
        assert_eq!(m.next(&mut out), None);
        m.given_back(nothing.msg_id);
        assert_eq!(m.next(&mut out), Some(nothing));
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
        assert_eq!(m.unread(0, &mut b), Some(first));
        assert_eq!(m.unread(0, &mut b), Some(first));
        m.confirm_read(0, 1);
        assert_eq!(m.unread(0, &mut b), Some((2, MsgId(0xa2), item(2).len())));
        assert_eq!(&b[..item(2).len()], item(2).as_slice());
        m.confirm_read(0, 2);
        assert_eq!(m.unread(0, &mut b), None);
    }
}

/// The mailbox on its ring in flash, the board's pages in RAM: what it
/// keeps survives a reboot, and a power cut at any write or erase leaves
/// every item whole or gone.
mod flash_ring {
    use std::vec::Vec;

    use bark_proto::MsgId;

    use crate::flash_ring::{FlashRing, Op, Region, PAGE_LEN};
    use crate::mailbox::{Arrival, Mailbox, Person};
    use crate::seal::Fetch;
    use crate::vectors::fixture;
    use crate::MAX_PAYLOAD;
    use bark_link::phones::HomeKey;

    /// The ring's pages.
    const PAGES: u32 = 4;

    /// Flash in RAM, as NOR flash behaves: a write clears bits, an erase
    /// sets a page's.
    #[derive(Clone)]
    struct Ram(Vec<u8>);

    impl Ram {
        fn erased(pages: u32) -> Ram {
            Ram(std::vec![0xff; (pages * PAGE_LEN) as usize])
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

        fn open(&self) -> Home<'_> {
            Mailbox::new(FlashRing::open(self), fixture().home.node)
        }
    }

    impl Region for &Ram {
        fn pages(&self) -> u32 {
            self.0.len() as u32 / PAGE_LEN
        }
        fn read(&self, offset: u32, out: &mut [u8]) {
            let o = offset as usize;
            out.copy_from_slice(&self.0[o..o + out.len()]);
        }
    }

    type Home<'a> = Mailbox<FlashRing<&'a Ram>>;

    /// A boxed payload for Alice's tag, `t`, numbered `i`.
    fn item(i: u32) -> Vec<u8> {
        let mut b = std::format!("item {i}").into_bytes();
        b[0] = crate::seal::Form::Boxed.format();
        b
    }

    fn fetch(sent: u32) -> crate::seal::FetchPayload {
        Fetch {
            sent,
            address: fixture().away,
            recipient: 9,
            command: None,
        }
        .seal(&fixture().alice.mailbox, fixture().home.node, 7)
    }

    fn alice() -> Person {
        Person {
            number: 1,
            tag: Some(b't'),
            key: HomeKey::Home(fixture().alice.mailbox),
            ..Person::default()
        }
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

    /// Boot a mailbox from `flash` with Alice as `person` left her, do
    /// `f`, do every op it queued, and give back what Alice's record keeps.
    fn step(flash: &mut Ram, person: Person, f: impl FnOnce(&mut Home<'_>)) -> Person {
        let view = flash.clone();
        let mut m = view.open();
        m.set_person(0, Some(person));
        f(&mut m);
        let kept = m.person(0).unwrap();
        for op in take_ops(&mut m) {
            flash.apply(&op, None);
        }
        kept
    }

    #[test]
    fn items_survive_a_reboot() {
        let mut flash = Ram::erased(PAGES);
        let mut alice = alice();
        for i in 1..=70 {
            alice = step(&mut flash, alice, |m| {
                assert_eq!(m.arrive(MsgId(i), &item(i)), Arrival::Kept(i))
            });
        }
        alice = step(&mut flash, alice, |m| {
            assert_eq!(
                m.arrive(MsgId(0), &fetch(100)),
                Arrival::Fetch {
                    person: 0,
                    command: None,
                }
            )
        });
        alice = step(&mut flash, alice, |m| assert!(m.confirm_read(0, 60)));
        let mut m = flash.open();
        m.set_person(0, Some(alice));
        assert_eq!(m.newest(), 70);
        // Four pages of 16: item 65 erased the page of 1 to 16.
        assert_eq!(m.oldest(), 17);
        let mut b = [0u8; MAX_PAYLOAD];
        assert_eq!(m.item(17, &mut b), Some(item(17).len()));
        assert_eq!(&b[..item(17).len()], item(17).as_slice());
        assert_eq!(
            m.unread(0, &mut b).map(|u| (u.0, u.1)),
            Some((61, MsgId(61)))
        );
        assert_eq!(m.unread_count(0), 10);
        assert_eq!(
            m.arrive(MsgId(0), &fetch(100)),
            Arrival::Refused,
            "replayed"
        );
    }

    /// An item shows to a phone only once its slot is written: until then
    /// it cannot be confirmed, nor a guest's number pass it, so a reboot
    /// that loses its slot gives its number to the next item, which the
    /// phone and the guest then read, rather than skip as read.
    #[test]
    fn an_item_shows_once_its_slot_is_written() {
        let mut flash = Ram::erased(PAGES);
        let mut alice = alice();
        alice.tag = Some(b'm');
        let mine = |i: u32| {
            let mut p = item(i);
            p[crate::seal::RECIPIENT_AT] = b'm';
            p
        };
        let mut b = [0u8; MAX_PAYLOAD];
        // Item 1 kept, its slot still queued, and the node reboots.
        let view = flash.clone();
        let mut m = view.open();
        m.set_person(0, Some(alice));
        assert_eq!(m.arrive(MsgId(1), &mine(1)), Arrival::Kept(1));
        assert_eq!(m.unread_head(0), None);
        assert_eq!(m.unread_count(0), 0);
        assert!(m.for_tag(b'm', 0, &mut b).is_none());
        assert_eq!(m.person(0).unwrap().read, 0);
        // After the reboot the next item takes number 1, and shows once
        // written.
        alice = step(&mut flash, alice, |m| {
            assert_eq!(m.arrive(MsgId(2), &mine(2)), Arrival::Kept(1));
            assert_eq!(m.unread_head(0), None);
        });
        // One written and one queued: the queued one waits.
        alice = step(&mut flash, alice, |m| {
            assert_eq!(m.unread_head(0), Some((1, MsgId(2))));
            assert_eq!(m.for_tag(b'm', 0, &mut b).map(|f| f.1), Some(MsgId(2)));
            assert_eq!(m.arrive(MsgId(3), &mine(3)), Arrival::Kept(2));
            assert_eq!(m.unread_count(0), 1);
            assert!(m.for_tag(b'm', 1, &mut b).is_none());
        });
        step(&mut flash, alice, |m| {
            assert_eq!(m.unread_count(0), 2);
            assert!(m.confirm_read(0, 2));
        });
    }

    /// Items a page away from their place, as a ring laid out otherwise
    /// before leaves them, are not taken for items, and new ones are kept
    /// whole from the first slot.
    #[test]
    fn items_out_of_place_are_ignored() {
        let mut flash = Ram::erased(PAGES);
        for i in 1..=20 {
            step(&mut flash, alice(), |m| {
                m.arrive(MsgId(i), &item(i));
            });
        }
        flash.0.rotate_right(PAGE_LEN as usize);
        let m = flash.open();
        assert_eq!(m.newest(), 0);
        let mut kept = flash.clone();
        for i in 1..=3 {
            step(&mut kept, alice(), |m| {
                assert_eq!(m.arrive(MsgId(i), &item(i)), Arrival::Kept(i))
            });
        }
        let m = kept.open();
        assert_eq!((m.oldest(), m.newest()), (1, 3));
        let mut b = [0u8; MAX_PAYLOAD];
        assert_eq!(m.item(3, &mut b), Some(item(3).len()));
    }

    /// A power cut at every op of a run, at every eighth byte of a
    /// write: the mailbox comes back with every item it reads whole and
    /// right, and no number above what was kept.
    #[test]
    fn a_cut_anywhere_leaves_items_whole_or_gone() {
        // The run: 40 items, a fetch, a read confirmed; one mailbox
        // throughout, its ops collected in order.
        let mut all: Vec<Op> = Vec::new();
        let view = Ram::erased(PAGES);
        let mut m = view.open();
        m.set_person(0, Some(alice()));
        for i in 1..=40 {
            m.arrive(MsgId(i), &item(i));
            all.extend(take_ops(&mut m));
        }
        m.arrive(MsgId(0), &fetch(100));
        let mut out = [0u8; MAX_PAYLOAD];
        while m.next(&mut out).is_some() {
            all.extend(take_ops(&mut m));
        }
        m.confirm_read(0, 30);
        all.extend(take_ops(&mut m));
        let alice = m.person(0).unwrap();
        let mut flash = Ram::erased(PAGES);
        let mut cuts = 0;
        for (k, op) in all.iter().enumerate() {
            let points: Vec<usize> = match op {
                Op::Erase { .. } => std::vec![0],
                Op::Write { .. } => (0..op.data().len()).step_by(8).collect(),
            };
            for cut in points {
                let mut f = flash.clone();
                f.apply(op, Some(cut));
                let m = f.open();
                cuts += 1;
                let mut b = [0u8; MAX_PAYLOAD];
                for n in 1..=m.newest() {
                    if let Some(l) = m.item(n, &mut b) {
                        assert_eq!(&b[..l], item(n).as_slice(), "op {k} cut {cut}");
                    }
                }
                assert!(m.newest() <= 40);
            }
            flash.apply(op, None);
        }
        assert!(cuts > 1000, "{cuts}");
        let mut m = flash.open();
        m.set_person(0, Some(alice));
        assert_eq!((m.newest(), m.unread_count(0)), (40, 10));
    }

    /// A guest's item cut short by a power cut is not read; written whole,
    /// it is read after the reboot, by the number the guest keeps.
    #[test]
    fn a_cut_between_a_guest_s_write_and_its_read() {
        let payload = std::vec![crate::seal::Form::Boxed.format(), 5, 1, 2, 3];
        let view = Ram::erased(PAGES);
        let mut m = view.open();
        assert_eq!(m.arrive(MsgId(1), &payload), Arrival::Kept(1));
        let ops = take_ops(&mut m);
        let write = ops
            .iter()
            .position(|o| matches!(o, Op::Write { .. }))
            .expect("the slot's write");
        let mut b = [0u8; MAX_PAYLOAD];
        for cut in (0..ops[write].data().len()).step_by(8) {
            let mut f = Ram::erased(PAGES);
            for op in &ops[..write] {
                f.apply(op, None);
            }
            f.apply(&ops[write], Some(cut));
            assert_eq!(f.open().for_tag(5, 0, &mut b), None, "cut {cut}");
        }
        let mut f = Ram::erased(PAGES);
        for op in &ops {
            f.apply(op, None);
        }
        let m = f.open();
        assert_eq!(
            m.for_tag(5, 0, &mut b).map(|u| (u.0, u.1)),
            Some((1, MsgId(1)))
        );
        assert_eq!(&b[..payload.len()], &payload[..]);
        assert_eq!(m.for_tag(5, 1, &mut b), None);
    }

    /// A mark is a write of one byte over a written slot, a bit cleared:
    /// it is kept across a reboot, and the item stays whole.
    #[test]
    fn marks_survive_a_reboot_in_flash() {
        let mut flash = Ram::erased(PAGES);
        let mut alice = alice();
        for i in 1..=40 {
            alice = step(&mut flash, alice, |m| {
                m.arrive(MsgId(i), &item(i));
            });
        }
        let view = flash.clone();
        let mut m = view.open();
        m.set_person(0, Some(alice));
        assert_eq!(
            m.arrive(MsgId(0), &fetch(100)),
            Arrival::Fetch {
                person: 0,
                command: None,
            }
        );
        let mut ops = take_ops(&mut m);
        let mut out = [0u8; MAX_PAYLOAD];
        let mut sent = Vec::new();
        while let Some(o) = m.next(&mut out) {
            sent.extend((o.number != 0).then_some(o.number));
            ops.extend(take_ops(&mut m));
        }
        assert_eq!(sent, (9..=40).rev().collect::<Vec<_>>());
        let alice = m.person(0).unwrap();
        // Each a byte with one bit clear.
        assert!(ops
            .iter()
            .filter(|o| o.data().len() == 1)
            .all(|o| o.data() == [!1]));
        for op in &ops {
            flash.apply(op, None);
        }
        let mut m = flash.open();
        m.set_person(0, Some(alice));
        let mut b = [0u8; MAX_PAYLOAD];
        assert_eq!(m.item(40, &mut b), Some(item(40).len()));
        assert_eq!(&b[..item(40).len()], item(40).as_slice());
        m.arrive(MsgId(0), &fetch(101));
        let mut out = [0u8; MAX_PAYLOAD];
        assert_eq!(m.next(&mut out).map(|o| o.number), Some(0));
        assert_eq!(m.next(&mut out).map(|o| o.number), Some(8));
    }

    #[test]
    fn a_mark_queued_is_read_through_the_queue() {
        let flash = Ram::erased(PAGES);
        let mut m = flash.open();
        m.set_person(0, Some(alice()));
        m.arrive(MsgId(1), &item(1));
        m.arrive(MsgId(2), &item(2));
        m.confirm_read(0, 1);
        assert!(!m.ring().idle());
        m.arrive(MsgId(0), &fetch(100));
        let mut out = [0u8; MAX_PAYLOAD];
        assert_eq!(m.next(&mut out).map(|o| o.number), Some(0));
        assert_eq!(m.next(&mut out).map(|o| o.number), Some(2));
        assert_eq!(m.next(&mut out), None);
    }

    #[test]
    fn reads_see_what_is_queued() {
        let flash = Ram::erased(PAGES);
        let mut m = flash.open();
        m.arrive(MsgId(1), &item(1));
        assert!(!m.ring().idle());
        let mut b = [0u8; MAX_PAYLOAD];
        assert_eq!(m.item(1, &mut b), Some(item(1).len()));
    }
}

/// The owner's commands, on a fetch, run on the node's records, answered
/// in the summary (spec "Mailbox", The owner's commands).
mod commands {
    use std::vec::Vec;

    use bark_link::phones::{HomeKey, Keys, Name, Phones, RECORDS};
    use bark_proto::MsgId;

    use crate::command::{self, Code, Command, Outcome, Summary};
    use crate::mailbox::{Arrival, Mailbox, Person, RamRing};
    use crate::seal::{self, Fetch, Form};
    use crate::vectors::fixture;
    use crate::{Error, MAX_PAYLOAD};

    type Home = Mailbox<RamRing<32>>;

    fn bond(n: u8) -> Keys {
        Keys {
            kind: 1,
            addr: [n, 2, 3, 4, 5, 0xc6],
            irk: None,
            ltk: [n; 16],
            seen: 1,
            service_changed: false,
        }
    }

    /// A node whose records are Alice's, the owner, paired, at place 0,
    /// and Bob's, paired and homed, at place 1.
    fn node() -> (Home, Phones) {
        let f = fixture();
        let mut p = Phones::EMPTY;
        p.pair(bond(1), 0).unwrap();
        p.set_recipient(0, 0x11, Name::new(b"Alice").unwrap());
        p.set_home(0, Some(f.alice.mailbox), 0);
        p.pair(bond(2), 0).unwrap();
        p.set_recipient(1, 0x22, Name::new(b"Bob").unwrap());
        p.set_home(1, Some(f.bob.mailbox), 0);
        let mut m = Mailbox::new(RamRing::new(), f.home.node);
        for i in 0..RECORDS {
            m.set_person(i, p.get(i).map(Person::of));
        }
        (m, p)
    }

    fn fetch(key: &[u8; 32], sent: u32, command: Option<Command>) -> seal::FetchPayload {
        Fetch {
            sent,
            address: fixture().away,
            recipient: 9,
            command,
        }
        .seal(key, fixture().home.node, 7)
    }

    /// The fetch arrives, its command runs on the records, and the
    /// summary and the items come out, opened under `key`.
    fn ask(
        m: &mut Home,
        p: &mut Phones,
        key: &[u8; 32],
        sent: u32,
        c: Option<Command>,
    ) -> (Summary, usize) {
        let Arrival::Fetch {
            person, command, ..
        } = m.arrive(MsgId(sent), &fetch(key, sent, c))
        else {
            panic!("the fetch is taken");
        };
        assert_eq!(command, c);
        command::obey(m, p, person, command, 300, 0);
        drain(m, key, sent)
    }

    /// The round's summary and its count of items, opened under `key`.
    fn drain(m: &mut Home, key: &[u8; 32], sent: u32) -> (Summary, usize) {
        let mut out = [0u8; MAX_PAYLOAD];
        let mut summary = None;
        let mut items = 0;
        while let Some(o) = m.next(&mut out) {
            match Form::of(&out[..o.len]) {
                Ok(Form::Summary) => {
                    assert!(summary.is_none() && items == 0, "the summary first");
                    summary = Some(seal::open_summary(key, sent, &out[..o.len]).unwrap());
                }
                _ => items += 1,
            }
        }
        (summary.expect("a summary"), items)
    }

    fn done(code: Code) -> Option<Outcome> {
        Some(Outcome::Done(code))
    }

    #[test]
    fn a_command_is_read_whole_or_not_at_all() {
        let mut b = [0u8; Command::MAX_LEN];
        let add = Command::Add {
            key: [7; 32],
            tag: 0x42,
            name: Name::new("Åsa".as_bytes()).unwrap(),
        };
        for c in [
            add,
            Command::Remove(0x0102),
            Command::List,
            Command::HandOver(9),
            Command::Leave,
        ] {
            let n = c.encode(&mut b);
            assert_eq!(Command::decode(&b[..n]), Ok(c));
        }
        let n = add.encode(&mut b);
        assert_eq!((n, b[0], b[33]), (38, 1, 0x42));
        let n = Command::Remove(0x0102).encode(&mut b);
        assert_eq!(&b[..n], &[2, 1, 2]);
        for bad in [
            &[][..],
            &[0],
            &[6],
            &[2, 1],
            &[2, 1, 2, 3],
            &[3, 0],
            &[5, 0],
            &[1; 33],
        ] {
            assert_eq!(Command::decode(bad), Err(Error::Malformed), "{bad:?}");
        }
        // A name that is not UTF-8, or longer than a card's.
        let mut bad = [1u8; 2 + 32 + 33];
        assert_eq!(
            Command::decode(&bad[..35]),
            Ok(Command::Add {
                key: [1; 32],
                tag: 1,
                name: Name::new(&[1]).unwrap()
            })
        );
        bad[34] = 0xff;
        assert_eq!(Command::decode(&bad[..35]), Err(Error::Malformed));
        bad[34] = b'a';
        assert_eq!(Command::decode(&bad), Err(Error::Malformed));
    }

    /// A fetch with a command opens as one without; one whose command the
    /// node does not read opens as nothing.
    #[test]
    fn a_fetch_carries_its_command_under_its_proof() {
        let f = fixture();
        let c = Some(Command::HandOver(3));
        let p = fetch(&f.alice.mailbox, 5, c);
        assert_eq!(p.len(), Fetch::LEN + 3);
        let got = Fetch::open(&f.alice.mailbox, f.home.node, &p).unwrap();
        assert_eq!((got.sent, got.command), (5, c));
        let longest = fetch(
            &f.alice.mailbox,
            5,
            Some(Command::Add {
                key: [1; 32],
                tag: 1,
                name: Name::new(&[b'a'; 32]).unwrap(),
            }),
        );
        assert_eq!(longest.len(), Fetch::MAX_LEN);
        assert!(Fetch::open(&f.alice.mailbox, f.home.node, &longest).is_ok());
        let mut q = p;
        let last = q.len() - 1;
        q[last] ^= 1;
        assert_eq!(
            Fetch::open(&f.alice.mailbox, f.home.node, &q),
            Err(Error::Open)
        );
    }

    #[test]
    fn a_summary_carries_the_owner_and_what_a_command_did() {
        let k = fixture().alice.mailbox;
        let mut out = [0u8; MAX_PAYLOAD];
        for s in [
            Summary {
                newest: crate::VERSION,
                items: 3,
                remaining: 700,
                owner: true,
                outcome: None,
            },
            Summary {
                newest: crate::VERSION,
                items: 0,
                remaining: 0,
                owner: false,
                outcome: done(Code::NoSuchHome),
            },
        ] {
            let n = seal::seal_summary(&k, 7, 10, &s, &mut out);
            assert_eq!(n, seal::SUMMARY_LEN + usize::from(s.outcome.is_some()));
            assert_eq!(seal::open_summary(&k, 10, &out[..n]), Ok(s));
        }
        // The flags: the owner in bit 0, the home's newest payload version
        // in the high four bits.
        let s = Summary {
            items: 0,
            remaining: 0,
            owner: true,
            newest: 9,
            outcome: None,
        };
        let mut plain = [0u8; command::SUMMARY_PLAIN_LEN + Outcome::MAX_LEN];
        assert_eq!(s.encode(&mut plain), command::SUMMARY_PLAIN_LEN);
        assert_eq!(plain[3], 0x91);
        assert_eq!(Summary::decode(&plain[..4]), Ok(s));
        // A home's mailbox says its own.
        let (mut m, mut p) = node();
        let (s, _) = ask(&mut m, &mut p, &fixture().alice.mailbox, 1, None);
        assert_eq!(s.newest, crate::VERSION);
    }

    /// Eight records with names of 32 bytes: the list all but fills a
    /// payload.
    #[test]
    fn the_longest_list_fits_one_summary() {
        let (mut m, mut p) = node();
        for k in 2..RECORDS as u8 {
            p.add([k; 32], k, Name::new(&[b'n'; 32]).unwrap(), 0, 0, 0)
                .unwrap();
        }
        p.set_recipient(0, 0x11, Name::new(&[b'a'; 32]).unwrap());
        p.set_recipient(1, 0x22, Name::new(&[b'b'; 32]).unwrap());
        for i in 0..RECORDS {
            m.set_person(i, p.get(i).map(Person::of));
        }
        let k = fixture().alice.mailbox;
        let Arrival::Fetch {
            person, command, ..
        } = m.arrive(MsgId(1), &fetch(&k, 1, Some(Command::List)))
        else {
            panic!()
        };
        command::obey(&mut m, &mut p, person, command, 0, 0);
        let mut out = [0u8; MAX_PAYLOAD];
        let o = m.next(&mut out).unwrap();
        // Eight records of 25 bytes take 200 of the list's 201.
        assert_eq!(o.len, MAX_PAYLOAD - 1);
        let s = seal::open_summary(&k, 1, &out[..o.len]).unwrap();
        let Some(Outcome::List(list)) = s.outcome else {
            panic!("a list")
        };
        let mut listed = [None; RECORDS];
        assert_eq!(list.records(&mut listed), Ok(RECORDS));
        assert!(listed.iter().flatten().all(|l| l.name.len() == 18));
        assert!(listed[0].unwrap().mine && listed[0].unwrap().owner);
    }

    /// The owner adds a person with no phone at the node, lists, removes
    /// a home and hands the mark over; a person not the owner is not
    /// allowed, and still gets their mail.
    #[test]
    fn the_owner_manages_the_node_over_the_mesh() {
        let f = fixture();
        let (alice, bob) = (f.alice.mailbox, f.bob.mailbox);
        let carol = [3u8; 32];
        let (mut m, mut p) = node();
        let add = Command::Add {
            key: carol,
            tag: 0x33,
            name: Name::new(b"Carol").unwrap(),
        };
        let (s, _) = ask(&mut m, &mut p, &alice, 10, Some(add));
        assert_eq!((s.owner, s.outcome), (true, done(Code::Ok)));
        let r = *p.get(2).unwrap();
        assert_eq!(
            (r.bond, r.key, r.tag, r.day),
            (None, HomeKey::Home(carol), Some(0x33), 300)
        );
        assert_eq!(m.person(2).map(|p| p.key), Some(HomeKey::Home(carol)));
        // A retry, its answer lost, changes nothing.
        let (s, _) = ask(&mut m, &mut p, &alice, 11, Some(add));
        assert_eq!(s.outcome, done(Code::Already));
        // Bob is not the owner, but his fetch is answered and his mail
        // comes.
        let mut item = std::vec![Form::Boxed.format(), 0x22, 1, 2, 3];
        m.arrive(MsgId(100), &item);
        item[4] = 4;
        m.arrive(MsgId(101), &item);
        let (s, items) = ask(&mut m, &mut p, &bob, 12, Some(Command::Remove(1)));
        assert_eq!(
            (s.owner, s.outcome, s.items, items),
            (false, done(Code::NotAllowed), 2, 2)
        );
        // Carol fetches, as anyone homed does.
        let (s, _) = ask(&mut m, &mut p, &carol, 13, None);
        assert_eq!((s.owner, s.outcome), (false, None));
        // The list.
        let (s, _) = ask(&mut m, &mut p, &alice, 14, Some(Command::List));
        let Some(Outcome::List(list)) = s.outcome else {
            panic!("a list")
        };
        let mut listed = [None; RECORDS];
        assert_eq!(list.records(&mut listed), Ok(3));
        let names: Vec<_> = listed.iter().flatten().map(|l| l.name).collect();
        assert_eq!(names, [&b"Alice"[..], b"Bob", b"Carol"]);
        assert!(listed[0].unwrap().mine && listed[0].unwrap().owner);
        assert!(!listed[2].unwrap().paired && listed[2].unwrap().home);
        // Bob's home goes; his bond stays.
        let bob_number = p.get(1).unwrap().number;
        let (s, _) = ask(
            &mut m,
            &mut p,
            &alice,
            15,
            Some(Command::Remove(bob_number)),
        );
        assert_eq!(s.outcome, done(Code::Ok));
        assert_eq!(
            p.get(1).map(|r| (r.bond.is_some(), r.key.home())),
            Some((true, None))
        );
        let (s, _) = ask(
            &mut m,
            &mut p,
            &alice,
            16,
            Some(Command::Remove(bob_number)),
        );
        assert_eq!(s.outcome, done(Code::NoSuchHome));
        let (s, _) = ask(
            &mut m,
            &mut p,
            &alice,
            17,
            Some(Command::HandOver(bob_number)),
        );
        assert_eq!(s.outcome, done(Code::NoSuchHome));
        // Carol is the owner now, and Alice stays homed.
        let carol_number = p.get(2).unwrap().number;
        let (s, _) = ask(
            &mut m,
            &mut p,
            &alice,
            18,
            Some(Command::HandOver(carol_number)),
        );
        assert_eq!((s.owner, s.outcome), (false, done(Code::Ok)));
        let (s, _) = ask(&mut m, &mut p, &alice, 19, Some(Command::List));
        assert_eq!((s.owner, s.outcome), (false, done(Code::NotAllowed)));
        let (s, _) = ask(&mut m, &mut p, &carol, 20, None);
        assert!(s.owner);
    }

    /// A leave takes the key the round is under: the summary still goes,
    /// under it, alone, counting what the person leaves behind, and no
    /// later fetch of theirs is answered.
    #[test]
    fn a_leave_is_answered_by_its_summary_alone() {
        let f = fixture();
        let (mut m, mut p) = node();
        let mut item = std::vec![Form::Boxed.format(), 0x22, 1, 2, 3];
        m.arrive(MsgId(100), &item);
        item[4] = 4;
        m.arrive(MsgId(101), &item);
        let (s, items) = ask(&mut m, &mut p, &f.bob.mailbox, 10, Some(Command::Leave));
        assert_eq!(
            (s.owner, s.outcome, s.items, s.remaining, items),
            (false, done(Code::Ok), 0, 2, 0)
        );
        assert_eq!(
            p.get(1).map(|r| (r.bond.is_some(), r.key)),
            Some((true, HomeKey::None))
        );
        assert_eq!(
            m.arrive(MsgId(11), &fetch(&f.bob.mailbox, 11, None)),
            Arrival::Refused
        );
        // The owner who leaves leaves the seat empty, and the next home set
        // takes it.
        let (s, _) = ask(&mut m, &mut p, &f.alice.mailbox, 12, Some(Command::Leave));
        assert_eq!((s.owner, s.outcome), (false, done(Code::Ok)));
        assert_eq!(p.owner(), None);
        p.set_home(1, Some(f.bob.mailbox), 0);
        assert_eq!(p.owner(), Some(f.bob.mailbox));
    }

    /// A summary given back goes again; an orphan's too.
    #[test]
    fn a_leave_s_summary_given_back_goes_again() {
        let f = fixture();
        let (mut m, mut p) = node();
        let key = f.bob.mailbox;
        let Arrival::Fetch {
            person, command, ..
        } = m.arrive(MsgId(1), &fetch(&key, 1, Some(Command::Leave)))
        else {
            panic!()
        };
        // Nothing goes before the command has run.
        assert!(!m.reply_due());
        command::obey(&mut m, &mut p, person, command, 0, 0);
        let mut out = [0u8; MAX_PAYLOAD];
        let o = m.next(&mut out).unwrap();
        assert!(m.next(&mut out).is_none());
        m.given_back(o.msg_id);
        let again = m.next(&mut out).unwrap();
        let s = seal::open_summary(&key, 1, &out[..again.len]).unwrap();
        assert_eq!(s.outcome, done(Code::Ok));
    }

    /// The people as the records say them, as the firmware hands them
    /// over after any change.
    fn sync(m: &mut Home, p: &Phones) {
        for i in 0..RECORDS {
            m.set_person(i, p.get(i).map(Person::of));
        }
    }

    /// A key set again takes no fetch from before it was set: a captured
    /// leave, replayed once the person has made the node their home again,
    /// is refused (spec "Settings", Home; roadmap D236).
    #[test]
    fn a_captured_leave_is_refused_once_the_key_is_set_again() {
        let f = fixture();
        let bob = f.bob.mailbox;
        let (mut m, mut p) = node();
        let leave = fetch(&bob, 10, Some(Command::Leave));
        let Arrival::Fetch {
            person, command, ..
        } = m.arrive(MsgId(1), &leave)
        else {
            panic!("taken")
        };
        command::obey(&mut m, &mut p, person, command, 0, 0);
        assert_eq!(p.get(1).map(|r| r.key), Some(HomeKey::None));
        // Home again, from his phone, at its `sent` of 50.
        p.set_home(1, Some(bob), 50);
        sync(&mut m, &p);
        assert_eq!(m.arrive(MsgId(2), &leave), Arrival::Refused);
        assert_eq!(p.get(1).map(|r| r.key), Some(HomeKey::Home(bob)));
        let (s, _) = ask(&mut m, &mut p, &bob, 51, None);
        assert_eq!(s.outcome, None, "a later fetch is taken");
    }

    /// An owner's command captured before the owner left, replayed once
    /// they are home and the owner again, is refused.
    #[test]
    fn a_captured_owner_command_is_refused_once_the_key_is_set_again() {
        let f = fixture();
        let alice = f.alice.mailbox;
        let (mut m, mut p) = node();
        let bob_number = p.get(1).unwrap().number;
        // Captured on its way, never delivered.
        let remove = fetch(&alice, 10, Some(Command::Remove(bob_number)));
        let (s, _) = ask(&mut m, &mut p, &alice, 11, Some(Command::Leave));
        assert_eq!(s.outcome, done(Code::Ok));
        assert_eq!(p.owner(), None);
        p.set_home(0, Some(alice), 40);
        sync(&mut m, &p);
        assert_eq!(p.owner(), Some(alice), "the empty seat");
        assert_eq!(m.arrive(MsgId(3), &remove), Arrival::Refused);
        assert_eq!(p.get(1).map(|r| r.key), Some(HomeKey::Home(f.bob.mailbox)));
    }

    /// A person added again after a leave takes none of their fetches from
    /// before the add: the record starts at the add's own `sent`.
    #[test]
    fn an_add_after_a_leave_refuses_the_old_fetches() {
        let f = fixture();
        let alice = f.alice.mailbox;
        let carol = [3u8; 32];
        let (mut m, mut p) = node();
        let add = Command::Add {
            key: carol,
            tag: 0x33,
            name: Name::EMPTY,
        };
        let (s, _) = ask(&mut m, &mut p, &alice, 20, Some(add));
        assert_eq!(s.outcome, done(Code::Ok));
        assert_eq!(p.get(2).map(|r| r.last_fetch), Some(20));
        let old = fetch(&carol, 25, None);
        let (s, _) = ask(&mut m, &mut p, &carol, 30, Some(Command::Leave));
        assert_eq!(s.outcome, done(Code::Ok));
        let (s, _) = ask(&mut m, &mut p, &alice, 40, Some(add));
        assert_eq!(s.outcome, done(Code::Ok));
        assert_eq!(p.get(2).map(|r| r.last_fetch), Some(40));
        assert_eq!(m.arrive(MsgId(4), &old), Arrival::Refused);
        assert_eq!(
            m.arrive(MsgId(5), &fetch(&carol, 30, Some(Command::Leave))),
            Arrival::Refused
        );
        let (s, _) = ask(&mut m, &mut p, &carol, 41, None);
        assert_eq!(s.outcome, None);
    }

    /// A fetch under a removed home's key: taken as any other, its command
    /// not run, and answered by a summary alone that says so.
    fn told(
        m: &mut Home,
        p: &mut Phones,
        key: &[u8; 32],
        sent: u32,
        c: Option<Command>,
    ) -> Summary {
        let Arrival::Fetch {
            person, command, ..
        } = m.arrive(MsgId(sent), &fetch(key, sent, c))
        else {
            panic!("the fetch is taken");
        };
        assert_eq!(command, None, "no command runs");
        command::obey(m, p, person, command, 300, 0);
        let (s, items) = drain(m, key, sent);
        assert_eq!(items, 0, "no item goes");
        s
    }

    /// The person whose home was removed is told at their next fetch, once
    /// for each later `sent`, and never sent their mail; their command
    /// does not run (spec "Mailbox", The answer; roadmap D239).
    #[test]
    fn a_fetch_under_a_removed_home_is_told_so() {
        let f = fixture();
        let (alice, bob) = (f.alice.mailbox, f.bob.mailbox);
        let (mut m, mut p) = node();
        let mut item = std::vec![Form::Boxed.format(), 0x22, 1, 2, 3];
        m.arrive(MsgId(100), &item);
        item[4] = 4;
        m.arrive(MsgId(101), &item);
        let bob_number = p.get(1).unwrap().number;
        let (s, _) = ask(&mut m, &mut p, &bob, 10, None);
        assert_eq!(s.items, 2);
        m.arrive(MsgId(102), &item);
        let (s, _) = ask(
            &mut m,
            &mut p,
            &alice,
            11,
            Some(Command::Remove(bob_number)),
        );
        assert_eq!(s.outcome, done(Code::Ok));
        assert!(
            m.person(1)
                .is_some_and(|p| matches!(p.key, HomeKey::Removed(_)))
                && m.is_home()
        );
        let removed = Summary {
            items: 0,
            remaining: 0,
            owner: false,
            newest: crate::VERSION,
            outcome: done(Code::Removed),
        };
        assert_eq!(told(&mut m, &mut p, &bob, 20, None), removed);
        assert_eq!(p.get(1).map(|r| r.last_fetch), Some(20));
        // Once for each later `sent`.
        assert_eq!(m.arrive(MsgId(1), &fetch(&bob, 20, None)), Arrival::Refused);
        assert_eq!(m.arrive(MsgId(2), &fetch(&bob, 19, None)), Arrival::Refused);
        assert_eq!(told(&mut m, &mut p, &bob, 21, None), removed);
        // A command is not run, a leave nor the owner's.
        assert_eq!(
            told(&mut m, &mut p, &bob, 22, Some(Command::Leave)),
            removed
        );
        assert_eq!(p.get(1).map(|r| r.key), Some(HomeKey::Removed(bob)));
        let alice_number = p.get(0).unwrap().number;
        assert_eq!(
            told(
                &mut m,
                &mut p,
                &bob,
                23,
                Some(Command::Remove(alice_number))
            ),
            removed
        );
        assert_eq!(p.get(0).and_then(|r| r.key.home()), Some(alice));
        // Home again from his phone, from that write's `sent`: his mail
        // waited.
        p.set_home(1, Some(bob), 30);
        sync(&mut m, &p);
        assert_eq!(m.arrive(MsgId(3), &fetch(&bob, 30, None)), Arrival::Refused);
        let (s, items) = ask(&mut m, &mut p, &bob, 31, None);
        assert_eq!((s.outcome, s.items, items), (None, 1, 1));
    }

    /// The owner who removes their own home is answered as any command
    /// that takes the sender's key, and told so at their next fetch,
    /// without the owner's flag; their list is not given.
    #[test]
    fn the_owner_s_own_removed_home_is_told_so() {
        let f = fixture();
        let alice = f.alice.mailbox;
        let (mut m, mut p) = node();
        let alice_number = p.get(0).unwrap().number;
        let (s, _) = ask(
            &mut m,
            &mut p,
            &alice,
            10,
            Some(Command::Remove(alice_number)),
        );
        assert_eq!((s.owner, s.outcome), (false, done(Code::Ok)));
        assert_eq!(p.owner(), None);
        let s = told(&mut m, &mut p, &alice, 11, Some(Command::List));
        assert_eq!((s.owner, s.outcome), (false, done(Code::Removed)));
    }

    /// A key a removed record and a home both hold opens as the home's.
    #[test]
    fn a_home_s_key_opens_before_a_removed_one() {
        let f = fixture();
        let bob = f.bob.mailbox;
        let (mut m, mut p) = node();
        p.remove(
            p.get(1).unwrap().number,
            bark_link::phones::Parts {
                bond: false,
                home: true,
            },
        )
        .unwrap();
        p.pair(bond(3), 0).unwrap();
        p.set_home(2, Some(bob), 0);
        sync(&mut m, &p);
        let (s, _) = ask(&mut m, &mut p, &bob, 5, None);
        assert_eq!(s.outcome, None);
    }
}

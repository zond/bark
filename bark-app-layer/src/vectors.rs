//! `vectors.json`: bytes made here that the app's tests check libsodium
//! against (`bark-app/test/app_layer_test.dart`). Every input is fixed, so
//! the file is the same at every run; a change to it is a change of the
//! format. `BARK_WRITE_VECTORS=1 cargo test -p bark-app-layer` rewrites it.

use std::format;
use std::string::String;
use std::vec::Vec;

use bark_proto::{NodeId, Position};
use crypto_box::{PublicKey, SecretKey};

use crate::card::{self, Card};
use crate::keys::{self, Keys};
use crate::plain::{self, Header, Record, Writer};
use crate::seal::{self, Fetch};
use crate::text;
use crate::{Address, MAX_PAYLOAD};

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Fixed inputs, for the file and the tests.
pub struct Fixture {
    pub alice_seed: [u8; 16],
    pub bob_seed: [u8; 16],
    pub alice: Keys,
    pub bob: Keys,
    pub home: Address,
    pub away: Address,
}

pub fn fixture() -> Fixture {
    let alice_seed: [u8; 16] = core::array::from_fn(|i| i as u8);
    let bob_seed: [u8; 16] = core::array::from_fn(|i| 0xf0 - i as u8);
    Fixture {
        alice: Keys::from_seed(&alice_seed),
        bob: Keys::from_seed(&bob_seed),
        alice_seed,
        bob_seed,
        home: Address {
            position: Position::from_degrees(50.45, 30.52),
            node: NodeId([0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f]),
        },
        away: Address {
            position: Position::from_degrees(50.40, 30.60),
            node: NodeId([1, 2, 3, 4, 5, 6]),
        },
    }
}

fn plaintext(header: &Header, records: &[Record]) -> Vec<u8> {
    let mut b = [0u8; MAX_PAYLOAD];
    let mut w = Writer::new(&mut b, header).unwrap();
    for r in records {
        w.push(r).unwrap();
    }
    let n = w.finish();
    b[..n].to_vec()
}

fn text_record<'a>(s: &str, buf: &'a mut [u8; MAX_PAYLOAD]) -> Record<'a> {
    let (encoding, n) = text::encode(s, buf).unwrap();
    Record::Text {
        encoding,
        part: None,
        bytes: &buf[..n],
    }
}

/// A box from `from` to `to`, who is at the node `at`.
fn boxed(from: &Keys, to: &Keys, at: NodeId, nonce: [u8; 24], plain: &[u8]) -> Vec<u8> {
    let pair = seal::pair(&PublicKey::from(to.identity.0), &from.secret_key());
    let mut out = [0u8; MAX_PAYLOAD];
    let recipient = seal::recipient(&to.identity, at);
    let n = seal::seal_box(&pair, recipient, &nonce, plain, &mut out).unwrap();
    out[..n].to_vec()
}

fn entry(name: &str, fields: &[(&str, String)]) -> String {
    let body: Vec<String> = fields
        .iter()
        .map(|(k, v)| format!("    \"{k}\": {v}"))
        .collect();
    format!("  \"{name}\": {{\n{}\n  }}", body.join(",\n"))
}

fn q(s: &str) -> String {
    format!("\"{s}\"")
}

fn h(b: &[u8]) -> String {
    q(&hex(b))
}

pub fn json() -> String {
    let f = fixture();
    let mut parts = Vec::new();

    for (name, seed, k) in [
        ("alice", &f.alice_seed, &f.alice),
        ("bob", &f.bob_seed, &f.bob),
    ] {
        parts.push(entry(
            name,
            &[
                ("seed", h(seed)),
                ("phrase", q(&keys::phrase(seed).join(" "))),
                ("secret", h(&k.secret)),
                ("identity", h(&k.identity.0)),
                ("mailbox", h(&k.mailbox)),
                (
                    "recipient_home",
                    format!("{}", seal::recipient(&k.identity, f.home.node)),
                ),
                (
                    "recipient_away",
                    format!("{}", seal::recipient(&k.identity, f.away.node)),
                ),
            ],
        ));
    }

    let alice_card = Card {
        identity: f.alice.identity,
        home: Some(f.home),
        now: Some(card::Linked {
            address: f.away,
            at: 90_000,
        }),
        time: 86_400,
        name: "Аліса",
    };
    let bob_card = Card {
        identity: f.bob.identity,
        home: None,
        now: Some(card::Linked {
            address: f.away,
            at: 3_600,
        }),
        time: 0,
        name: "Bob",
    };
    for (name, c) in [("card", &alice_card), ("card_no_home", &bob_card)] {
        let mut cb = [0u8; card::MAX_LEN];
        let n = c.encode(&mut cb).unwrap();
        let mut ct = [0u8; card::MAX_TEXT_LEN];
        let tn = c.to_text(&mut ct).unwrap();
        parts.push(entry(
            name,
            &[
                ("identity", h(&c.identity.0)),
                ("home", c.home.map_or("null".into(), |a| h(&a.encode()))),
                (
                    "now",
                    c.now.map_or("null".into(), |n| h(&n.address.encode())),
                ),
                (
                    "now_at",
                    c.now.map_or("null".into(), |n| format!("{}", n.at)),
                ),
                ("time", format!("{}", c.time)),
                ("name", q(c.name)),
                ("bytes", h(&cb[..n])),
                ("text", q(core::str::from_utf8(&ct[..tn]).unwrap())),
            ],
        ));
    }

    // A text in windows-1251, from Alice to Bob, written from a node away
    // from the home on her card, and naming it.
    let mut tb = [0u8; MAX_PAYLOAD];
    let words = "Привіт, Боб! Ґанок їжака — «є».";
    let header = Header {
        greater: plain::greater(&f.alice.identity, &f.bob.identity),
        sent: 1_000_000,
        address: Some(f.away),
    };
    let p = plaintext(&header, &[text_record(words, &mut tb)]);
    let nonce = core::array::from_fn(|i| 0x40 + i as u8);
    let payload = boxed(&f.alice, &f.bob, f.home.node, nonce, &p);
    parts.push(entry(
        "box_1251",
        &[
            ("text", q(words)),
            ("sent", format!("{}", header.sent)),
            ("greater", format!("{}", header.greater)),
            ("address", h(&f.away.encode())),
            ("plaintext", h(&p)),
            ("nonce", h(&nonce)),
            ("payload", h(&payload)),
        ],
    ));
    let box_payload = payload;

    // A text in UTF-8, from Bob to Alice.
    let words = "ok 👍";
    let header = Header {
        greater: plain::greater(&f.bob.identity, &f.alice.identity),
        sent: 1_000_060,
        address: None,
    };
    let p = plaintext(&header, &[text_record(words, &mut tb)]);
    let nonce = core::array::from_fn(|i| 0x80 + i as u8);
    parts.push(entry(
        "box_utf8",
        &[
            ("text", q(words)),
            ("sent", format!("{}", header.sent)),
            ("greater", format!("{}", header.greater)),
            ("plaintext", h(&p)),
            ("nonce", h(&nonce)),
            (
                "payload",
                h(&boxed(&f.bob, &f.alice, f.away.node, nonce, &p)),
            ),
        ],
    ));

    // Reactions, Bob to Alice: a thumbs up, a heart of two code points,
    // and the longest, a kiss with two skin tones.
    for (name, emoji, sent, nonce) in [
        ("box_reaction", "👍", 1_000_120, 0xc0),
        ("box_heart", "❤️", 1_000_180, 0xe0),
        (
            "box_kiss",
            "🧑🏻\u{200d}❤️\u{200d}💋\u{200d}🧑🏿",
            1_000_190,
            0xd0,
        ),
    ] {
        let header = Header {
            greater: plain::greater(&f.bob.identity, &f.alice.identity),
            sent,
            address: None,
        };
        let p = plaintext(
            &header,
            &[Record::React {
                sent: 1_000_000,
                emoji,
            }],
        );
        let nonce: [u8; 24] = core::array::from_fn(|i| nonce + i as u8);
        parts.push(entry(
            name,
            &[
                ("sent", format!("{}", header.sent)),
                ("reacts_to", "1000000".into()),
                ("reaction", q(emoji)),
                ("plaintext", h(&p)),
                ("nonce", h(&nonce)),
                (
                    "payload",
                    h(&boxed(&f.bob, &f.alice, f.away.node, nonce, &p)),
                ),
            ],
        ));
    }

    // A text too long for one box, Alice to Bob, in two parts of
    // consecutive `sent`: cut after the last space that leaves the first
    // in a part's room, which it fills in windows-1251; the second, with
    // a character the page lacks, in UTF-8.
    let words = "Привіт! Ми на місці біля старої школи, вода є, світла немає з \
                 ранку. Генератор працює дві години ввечері, тож заряджаємо \
                 телефони по черзі. Приходьте до сьомої, візьміть ковдри й \
                 ліхтарик. Дорогою обходьте міст — там ями 🙏";
    let room = seal::MAX_BOX_PLAIN - plain::HEADER_LEN - 2;
    let head: String = words.chars().take(room).collect();
    let cut = head.rfind(' ').unwrap() + 1;
    let mut fields = std::vec![("text", q(words))];
    let mut items = Vec::new();
    for (i, piece) in [&words[..cut], &words[cut..]].into_iter().enumerate() {
        let header = Header {
            greater: plain::greater(&f.alice.identity, &f.bob.identity),
            sent: 1_000_200 + i as u32,
            address: None,
        };
        let (encoding, n) = text::encode(piece, &mut tb).unwrap();
        let record = Record::Text {
            encoding,
            part: Some(plain::Part {
                number: i as u8 + 1,
                of: 2,
            }),
            bytes: &tb[..n],
        };
        let p = plaintext(&header, &[record]);
        let nonce = core::array::from_fn(|j| 0x20 + (i * 24 + j) as u8);
        items.push(format!(
            "{{\"text\": {}, \"sent\": {}, \"plaintext\": {}, \"nonce\": {}, \"payload\": {}}}",
            q(piece),
            header.sent,
            h(&p),
            h(&nonce),
            h(&boxed(&f.alice, &f.bob, f.home.node, nonce, &p))
        ));
    }
    fields.push(("room", format!("{room}")));
    fields.push((
        "parts",
        format!("[\n      {}\n    ]", items.join(",\n      ")),
    ));
    parts.push(entry("box_parts", &fields));

    // A first message, Alice to Bob, with her card.
    let words = "Hello, it is Alice.";
    let header = Header {
        greater: plain::greater(&f.alice.identity, &f.bob.identity),
        sent: 999_000,
        address: None,
    };
    let p = plaintext(
        &header,
        &[Record::Card(alice_card), text_record(words, &mut tb)],
    );
    let ephemeral = [0x77u8; 32];
    let mut out = [0u8; MAX_PAYLOAD];
    let n = seal::seal_intro(
        &PublicKey::from(f.bob.identity.0),
        seal::recipient(&f.bob.identity, f.home.node),
        &SecretKey::from(ephemeral),
        &p,
        &mut out,
    )
    .unwrap();
    parts.push(entry(
        "intro",
        &[
            ("text", q(words)),
            ("sent", format!("{}", header.sent)),
            ("plaintext", h(&p)),
            ("ephemeral_secret", h(&ephemeral)),
            ("payload", h(&out[..n])),
        ],
    ));

    // Alice, away, fetches from her home node.
    let fetch = Fetch {
        sent: 1_000_500,
        address: f.away,
        recipient: seal::recipient(&f.alice.identity, f.away.node),
    };
    let nonce = core::array::from_fn(|i| 0x10 + i as u8);
    let home = seal::recipient(&f.alice.identity, f.home.node);
    let payload = fetch.seal(&f.alice.mailbox, home, &nonce).unwrap();
    parts.push(entry(
        "fetch",
        &[
            ("sent", format!("{}", fetch.sent)),
            ("address", h(&fetch.address.encode())),
            ("recipient", format!("{}", fetch.recipient)),
            ("home", format!("{home}")),
            ("nonce", h(&nonce)),
            ("payload", h(&payload)),
        ],
    ));

    // Her home node sends her item 6, the first text above, the first of
    // the round, with none unsent after it.
    let mut out = [0u8; MAX_PAYLOAD];
    let n = seal::seal_reply(
        &f.alice.mailbox,
        fetch.recipient,
        fetch.sent,
        1,
        6,
        0,
        &box_payload,
        &mut out,
    )
    .unwrap();
    parts.push(entry(
        "reply",
        &[
            ("round", format!("{}", fetch.sent)),
            ("place", "1".into()),
            ("number", "6".into()),
            ("remaining", "0".into()),
            ("nonce", h(&seal::reply_nonce(fetch.sent, 1))),
            ("item", h(&box_payload)),
            ("payload", h(&out[..n])),
        ],
    ));

    // Her next fetch finds nothing new.
    let round = fetch.sent + 60;
    parts.push(entry(
        "nothing_new",
        &[
            ("round", format!("{round}")),
            ("nonce", h(&seal::reply_nonce(round, 0))),
            (
                "payload",
                h(&seal::seal_nothing_new(
                    &f.alice.mailbox,
                    fetch.recipient,
                    round,
                )),
            ),
        ],
    ));

    format!("{{\n{}\n}}\n", parts.join(",\n"))
}

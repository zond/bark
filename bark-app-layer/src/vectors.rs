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

fn hex(b: &[u8]) -> String {
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
        bytes: &buf[..n],
    }
}

fn boxed(from: &Keys, to: &Keys, nonce: [u8; 24], plain: &[u8]) -> Vec<u8> {
    let pair = seal::pair(&PublicKey::from(to.identity.0), &from.secret_key());
    let mut out = [0u8; MAX_PAYLOAD];
    let n = seal::seal_box(&pair, &nonce, plain, &mut out).unwrap();
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
            ],
        ));
    }

    let alice_card = Card {
        identity: f.alice.identity,
        address: f.home,
        time: 86_400,
        name: "Аліса",
    };
    let mut cb = [0u8; card::MAX_LEN];
    let n = alice_card.encode(&mut cb).unwrap();
    let mut ct = [0u8; card::MAX_TEXT_LEN];
    let tn = alice_card.to_text(&mut ct).unwrap();
    parts.push(entry(
        "card",
        &[
            ("identity", h(&alice_card.identity.0)),
            ("address", h(&alice_card.address.encode())),
            ("time", format!("{}", alice_card.time)),
            ("name", q(alice_card.name)),
            ("bytes", h(&cb[..n])),
            ("text", q(core::str::from_utf8(&ct[..tn]).unwrap())),
        ],
    ));

    // A text in windows-1251, from Alice to Bob, with her address.
    let mut tb = [0u8; MAX_PAYLOAD];
    let words = "Привіт, Боб! Ґанок їжака — «є».";
    let header = Header {
        greater: plain::greater(&f.alice.identity, &f.bob.identity),
        sent: 1_000_000,
        address: Some(f.home),
    };
    let p = plaintext(&header, &[text_record(words, &mut tb)]);
    let nonce = core::array::from_fn(|i| 0x40 + i as u8);
    let payload = boxed(&f.alice, &f.bob, nonce, &p);
    parts.push(entry(
        "box_1251",
        &[
            ("text", q(words)),
            ("sent", format!("{}", header.sent)),
            ("greater", format!("{}", header.greater)),
            ("address", h(&f.home.encode())),
            ("plaintext", h(&p)),
            ("nonce", h(&nonce)),
            ("payload", h(&payload)),
        ],
    ));
    let box_payload = payload;

    // A text in UTF-8, from Bob to Alice, with an acknowledgement before it.
    let words = "ok 👍";
    let header = Header {
        greater: plain::greater(&f.bob.identity, &f.alice.identity),
        sent: 1_000_060,
        address: None,
    };
    let p = plaintext(
        &header,
        &[
            Record::Acknowledge { sent: 1_000_000 },
            text_record(words, &mut tb),
        ],
    );
    let nonce = core::array::from_fn(|i| 0x80 + i as u8);
    parts.push(entry(
        "box_utf8_ack",
        &[
            ("text", q(words)),
            ("sent", format!("{}", header.sent)),
            ("greater", format!("{}", header.greater)),
            ("acknowledges", "1000000".into()),
            ("plaintext", h(&p)),
            ("nonce", h(&nonce)),
            ("payload", h(&boxed(&f.bob, &f.alice, nonce, &p))),
        ],
    ));

    // An acknowledgement alone, Bob to Alice.
    let header = Header {
        greater: plain::greater(&f.bob.identity, &f.alice.identity),
        sent: 1_000_120,
        address: None,
    };
    let p = plaintext(&header, &[Record::Acknowledge { sent: 1_000_000 }]);
    let nonce = core::array::from_fn(|i| 0xc0 + i as u8);
    parts.push(entry(
        "box_ack",
        &[
            ("sent", format!("{}", header.sent)),
            ("acknowledges", "1000000".into()),
            ("plaintext", h(&p)),
            ("nonce", h(&nonce)),
            ("payload", h(&boxed(&f.bob, &f.alice, nonce, &p))),
        ],
    ));

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
        since: 5,
        most: 23,
    };
    let nonce = core::array::from_fn(|i| 0x10 + i as u8);
    let payload = fetch.seal(&f.alice.mailbox, &nonce).unwrap();
    parts.push(entry(
        "fetch",
        &[
            ("sent", format!("{}", fetch.sent)),
            ("address", h(&fetch.address.encode())),
            ("since", format!("{}", fetch.since)),
            ("most", format!("{}", fetch.most)),
            ("nonce", h(&nonce)),
            ("payload", h(&payload)),
        ],
    ));

    // Her home node sends her item 6, the first text above, in reply.
    let mut out = [0u8; MAX_PAYLOAD];
    let n = seal::seal_reply(&f.alice.mailbox, fetch.sent, 6, &box_payload, &mut out).unwrap();
    parts.push(entry(
        "reply",
        &[
            ("round", format!("{}", fetch.sent)),
            ("number", "6".into()),
            ("nonce", h(&seal::reply_nonce(fetch.sent, 6))),
            ("item", h(&box_payload)),
            ("payload", h(&out[..n])),
        ],
    ));

    format!("{{\n{}\n}}\n", parts.join(",\n"))
}

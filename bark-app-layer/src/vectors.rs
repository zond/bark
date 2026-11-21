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
use crate::command::{self, Code, Command, Outcome, Summary};
use crate::keys::{self, Keys};
use crate::plain::{self, Header, Record, Writer};
use crate::seal::{self, Fetch, SivKeys};
use crate::text;
use crate::{Address, MAX_PAYLOAD};
use bark_link::phones::{self, Name, Phones};

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The seed of Carol, a third person whose pair with Bob is not Alice's.
pub const CAROL_SEED: [u8; 16] = [3; 16];

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
fn boxed(from: &Keys, to: &Keys, at: NodeId, plain: &[u8]) -> Vec<u8> {
    let pair = seal::pair(&PublicKey::from(to.identity.0), &from.secret_key()).unwrap();
    let mut out = [0u8; MAX_PAYLOAD];
    let recipient = seal::recipient(&to.identity, at);
    let n = seal::seal_box(&pair, recipient, plain, &mut out).unwrap();
    out[..n].to_vec()
}

/// `payload` with bit 0 of byte `at` flipped.
fn flipped(payload: &[u8], at: usize) -> Vec<u8> {
    let mut p = payload.to_vec();
    p[at] ^= 1;
    p
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

    // The pair's keys, the same from either side, and Alice's mailbox's.
    let ab = PublicKey::from(f.bob.identity.0);
    let shared = seal::beforenm(&ab, &f.alice.secret_key()).unwrap();
    let pair = SivKeys::of(&shared);
    let mbx = SivKeys::of(&f.alice.mailbox);
    parts.push(entry(
        "keys",
        &[
            ("pair_shared", h(&shared)),
            ("pair_mac", h(&pair.mac)),
            ("pair_enc", h(&pair.enc)),
            ("mailbox_mac", h(&mbx.mac)),
            ("mailbox_enc", h(&mbx.enc)),
            ("mailbox_reply", h(&seal::reply_key(&f.alice.mailbox))),
        ],
    ));

    let alice_card = Card {
        reads: crate::VERSION,
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
        reads: crate::VERSION,
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
                ("reads", format!("{}", c.reads)),
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
    let payload = boxed(&f.alice, &f.bob, f.home.node, &p);
    parts.push(entry(
        "box_1251",
        &[
            ("text", q(words)),
            ("sent", format!("{}", header.sent)),
            ("greater", format!("{}", header.greater)),
            ("address", h(&f.away.encode())),
            ("plaintext", h(&p)),
            ("payload", h(&payload)),
        ],
    ));

    // What Bob opens under his pair with Alice and must refuse: that box
    // with a bit of `siv` or of the ciphertext flipped, and the same
    // plaintext from Carol, whose pair with Bob opens it.
    let carol = Keys::from_seed(&CAROL_SEED);
    parts.push(entry(
        "box_refused",
        &[
            ("forged_siv", h(&flipped(&payload, seal::HEAD_LEN))),
            ("flipped", h(&flipped(&payload, payload.len() - 1))),
            ("carol_seed", h(&CAROL_SEED)),
            ("other_pair", h(&boxed(&carol, &f.bob, f.home.node, &p))),
        ],
    ));

    // A text in UTF-8, from Bob to Alice.
    let words = "ok 👍";
    let header = Header {
        greater: plain::greater(&f.bob.identity, &f.alice.identity),
        sent: 1_000_060,
        address: None,
    };
    let p = plaintext(&header, &[text_record(words, &mut tb)]);
    parts.push(entry(
        "box_utf8",
        &[
            ("text", q(words)),
            ("sent", format!("{}", header.sent)),
            ("greater", format!("{}", header.greater)),
            ("plaintext", h(&p)),
            ("payload", h(&boxed(&f.bob, &f.alice, f.away.node, &p))),
        ],
    ));

    // A home given, Bob to Alice: his home node, and the mailbox key it
    // took for her, with which she may make it her home.
    let given: [u8; 32] = core::array::from_fn(|i| 0x80 + i as u8);
    let header = Header {
        greater: plain::greater(&f.bob.identity, &f.alice.identity),
        sent: 1_000_090,
        address: None,
    };
    let p = plaintext(
        &header,
        &[Record::Home {
            address: f.home,
            key: given,
        }],
    );
    parts.push(entry(
        "home_record",
        &[
            ("address", h(&f.home.encode())),
            ("key", h(&given)),
            ("sent", format!("{}", header.sent)),
            ("greater", format!("{}", header.greater)),
            ("plaintext", h(&p)),
            ("payload", h(&boxed(&f.bob, &f.alice, f.away.node, &p))),
        ],
    ));

    // Reactions, Bob to Alice: a thumbs up, a heart of two code points,
    // and the longest, a kiss with two skin tones.
    for (name, emoji, sent) in [
        ("box_reaction", "👍", 1_000_120),
        ("box_heart", "❤️", 1_000_180),
        ("box_kiss", "🧑🏻\u{200d}❤️\u{200d}💋\u{200d}🧑🏿", 1_000_190),
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
        parts.push(entry(
            name,
            &[
                ("sent", format!("{}", header.sent)),
                ("reacts_to", "1000000".into()),
                ("reaction", q(emoji)),
                ("plaintext", h(&p)),
                ("payload", h(&boxed(&f.bob, &f.alice, f.away.node, &p))),
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
        items.push(format!(
            "{{\"text\": {}, \"sent\": {}, \"plaintext\": {}, \"payload\": {}}}",
            q(piece),
            header.sent,
            h(&p),
            h(&boxed(&f.alice, &f.bob, f.home.node, &p))
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
        command: None,
    };
    let home = seal::recipient(&f.alice.identity, f.home.node);
    let payload = fetch.seal(&f.alice.mailbox, f.home.node, home);
    parts.push(entry(
        "fetch",
        &[
            ("sent", format!("{}", fetch.sent)),
            ("address", h(&fetch.address.encode())),
            ("recipient", format!("{}", fetch.recipient)),
            ("home", format!("{home}")),
            ("payload", h(&payload)),
        ],
    ));
    // What her home must refuse under her key: that fetch with a bit of
    // `siv` or of the ciphertext flipped, the same fetch under Bob's, and
    // the same fetch under hers sealed for another node.
    parts.push(entry(
        "fetch_refused",
        &[
            ("forged_siv", h(&flipped(&payload, seal::HEAD_LEN))),
            ("flipped", h(&flipped(&payload, payload.len() - 1))),
            (
                "other_key",
                h(&fetch.seal(&f.bob.mailbox, f.home.node, home)),
            ),
            (
                "other_node",
                h(&fetch.seal(&f.alice.mailbox, f.away.node, home)),
            ),
        ],
    ));

    // Her home answers: first the summary, two items and none left after
    // them, and that she is its owner, then a box and an introduction
    // from Bob that waited for her there, at places 1 and 2.
    let round = fetch.sent;
    let to = fetch.recipient;
    let summary = |round: u32, items: u8, owner: bool, outcome: Option<Outcome>| {
        let s = Summary {
            newest: crate::VERSION,
            items,
            remaining: 0,
            owner,
            outcome,
        };
        let mut out = [0u8; MAX_PAYLOAD];
        let n = seal::seal_summary(&f.alice.mailbox, to, round, &s, &mut out);
        out[..n].to_vec()
    };
    parts.push(entry(
        "summary",
        &[
            ("round", format!("{round}")),
            ("items", "2".into()),
            ("remaining", "0".into()),
            ("owner", "true".into()),
            ("newest", format!("{}", crate::VERSION)),
            ("nonce", h(&seal::reply_nonce(round, 0))),
            ("payload", h(&summary(round, 2, true, None))),
        ],
    ));
    let header = Header {
        greater: plain::greater(&f.bob.identity, &f.alice.identity),
        sent: 1_000_400,
        address: None,
    };
    let p = plaintext(&header, &[text_record("Waiting at home.", &mut tb)]);
    let boxed_item = boxed(&f.bob, &f.alice, f.home.node, &p);
    let p = plaintext(
        &header,
        &[
            Record::Card(bob_card),
            text_record("Hi, Bob here.", &mut tb),
        ],
    );
    let mut ib = [0u8; MAX_PAYLOAD];
    let n = seal::seal_intro(
        &PublicKey::from(f.alice.identity.0),
        seal::recipient(&f.alice.identity, f.home.node),
        &SecretKey::from([0x55u8; 32]),
        &p,
        &mut ib,
    )
    .unwrap();
    for (name, place, item) in [
        ("reply_box", 1u8, &boxed_item[..]),
        ("reply_intro", 2, &ib[..n]),
    ] {
        let mut out = [0u8; MAX_PAYLOAD];
        let len = seal::seal_item(&f.alice.mailbox, to, round, place, item, &mut out).unwrap();
        let mut opened = [0u8; MAX_PAYLOAD];
        let m = seal::open_item(&f.alice.mailbox, round, &out[..len], &mut opened).unwrap();
        parts.push(entry(
            name,
            &[
                ("round", format!("{round}")),
                ("place", format!("{place}")),
                ("nonce", h(&seal::reply_nonce(round, place))),
                ("item", h(item)),
                ("payload", h(&out[..len])),
                ("opened", h(&opened[..m])),
            ],
        ));
    }

    // Her next fetch finds nothing new.
    let round = fetch.sent + 60;
    parts.push(entry(
        "nothing_new",
        &[
            ("round", format!("{round}")),
            ("nonce", h(&seal::reply_nonce(round, 0))),
            ("payload", h(&summary(round, 0, true, None))),
        ],
    ));

    // As the owner she adds Bob, who has no phone at the node, by his
    // mailbox key, his tag there and his name; then lists the records.
    let mut records = Phones::EMPTY;
    let bond = phones::Keys {
        kind: 1,
        addr: [1, 2, 3, 4, 5, 0xc6],
        irk: None,
        ltk: [7; 16],
        seen: 1,
        service_changed: false,
    };
    records.pair(bond, 0).unwrap();
    records.set_recipient(0, home, Name::new(b"Alice").unwrap());
    records.set_home(0, Some(f.alice.mailbox), 1_000_000);
    let add = Command::Add {
        key: f.bob.mailbox,
        tag: seal::recipient(&f.bob.identity, f.home.node),
        name: Name::new(b"Bob").unwrap(),
    };
    let mut cb = [0u8; Command::MAX_LEN];
    for (name, sent, c) in [
        ("command_add", fetch.sent + 120, add),
        ("command_list", fetch.sent + 180, Command::List),
    ] {
        let n = c.encode(&mut cb);
        let fetch = Fetch {
            sent,
            command: Some(c),
            ..fetch
        };
        let outcome = command::run(&mut records, 0, &c, sent, 0, 330, 1);
        let mut ob = [0u8; Outcome::MAX_LEN];
        let o = outcome.encode(&mut ob);
        parts.push(entry(
            name,
            &[
                ("sent", format!("{sent}")),
                ("command", h(&cb[..n])),
                ("fetch", h(&fetch.seal(&f.alice.mailbox, f.home.node, home))),
                ("outcome", h(&ob[..o])),
                ("summary", h(&summary(sent, 0, true, Some(outcome)))),
            ],
        ));
    }
    // The rest of the commands, as bytes, and a refusal.
    let mut rest = Vec::new();
    for (name, c) in [
        ("remove", Command::Remove(2)),
        ("hand_over", Command::HandOver(2)),
        ("leave", Command::Leave),
    ] {
        let n = c.encode(&mut cb);
        rest.push((name, h(&cb[..n])));
    }
    let round = fetch.sent + 240;
    let refused = Some(Outcome::Done(Code::NotAllowed));
    rest.push(("not_allowed_round", format!("{round}")));
    rest.push(("not_allowed_summary", h(&summary(round, 0, false, refused))));
    // Her fetch once her home there was removed.
    let round = fetch.sent + 300;
    let removed = Some(Outcome::Done(Code::Removed));
    rest.push(("removed_round", format!("{round}")));
    rest.push(("removed_summary", h(&summary(round, 0, false, removed))));
    parts.push(entry("commands", &rest));

    format!("{{\n{}\n}}\n", parts.join(",\n"))
}

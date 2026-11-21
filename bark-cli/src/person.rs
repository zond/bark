//! The person the tool is a phone for: their identity and card, their
//! contacts, and the payloads they send and receive (spec "The payload"),
//! made and opened with `bark-app-layer` as the app makes and opens them.

use std::time::Duration;

use bark_app_layer::card::{self, Card, Linked};
use bark_app_layer::keys::{Keys, KEY_LEN, SEED_LEN};
use bark_app_layer::plain::{self, Header, Part, Record, Writer, HEADER_LEN};
use bark_app_layer::seal::{self, Form, BOX_OVERHEAD, INTRO_OVERHEAD};
use bark_app_layer::text::{self, Encoding};
use bark_app_layer::{Address, Identity, EPOCH_UNIX_S, MAX_APP_PAYLOAD, MAX_PAYLOAD};
use bark_proto::NodeId;
use crypto_box::{PublicKey, SecretKey};

use crate::link::{Error, Result};

/// `sent` of a Unix time: seconds since 2026-01-01 (spec "The payload").
pub fn sent_of(unix_s: u64) -> u32 {
    unix_s.saturating_sub(EPOCH_UNIX_S) as u32
}

/// The Unix time of a `sent`.
pub fn unix_of(sent: u32) -> u64 {
    EPOCH_UNIX_S + u64::from(sent)
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

fn address_hex(a: &Address) -> String {
    hex(&a.encode())
}

fn address_of_hex(s: &str) -> Option<Address> {
    Address::decode(&unhex(s)?).ok()
}

/// Random bytes from the kernel.
pub fn random<const N: usize>() -> Result<[u8; N]> {
    use std::io::Read;
    let mut b = [0u8; N];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .map_err(|e| Error::Other(format!("random bytes: {e}")))?;
    Ok(b)
}

/// The person: their identity's seed and what their card says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Me {
    pub seed: [u8; SEED_LEN],
    pub name: String,
    /// Their home node, and when it was set or cleared, as `sent`.
    pub home: Option<Address>,
    pub home_time: u32,
    /// The node the tool was last linked to, and when, as `sent`.
    pub linked: Option<(Address, u32)>,
    /// The last `sent` given, so that each is later than any before.
    pub last_sent: u32,
}

impl Me {
    pub fn new(seed: [u8; SEED_LEN]) -> Me {
        Me {
            seed,
            name: String::new(),
            home: None,
            home_time: 0,
            linked: None,
            last_sent: 0,
        }
    }

    pub fn keys(&self) -> Keys {
        Keys::from_seed(&self.seed)
    }

    /// The next `sent` at Unix time `now_s`: later than any before.
    pub fn next_sent(&mut self, now_s: u64) -> u32 {
        self.last_sent = sent_of(now_s).max(self.last_sent + 1);
        self.last_sent
    }

    /// The card: the home, and the node last linked unless it is the home
    /// (spec "The payload", The card).
    pub fn card(&self) -> Vec<u8> {
        let now = self
            .linked
            .filter(|(a, _)| self.home.is_none_or(|h| h.node != a.node))
            .map(|(address, at)| Linked { address, at });
        let c = Card {
            reads: bark_app_layer::VERSION,
            identity: self.keys().identity,
            home: self.home,
            now,
            time: self.home_time,
            name: &self.name,
        };
        let mut b = [0u8; card::MAX_LEN];
        let n = c.encode(&mut b).expect("a name of at most 32 bytes");
        b[..n].to_vec()
    }

    pub fn card_text(&self) -> String {
        let bytes = self.card();
        let c = Card::decode(&bytes).expect("its own card");
        let mut t = [0u8; card::MAX_TEXT_LEN];
        let n = c.to_text(&mut t).expect("room for the longest");
        String::from_utf8_lossy(&t[..n]).into_owned()
    }

    /// The stored form: a line a field.
    pub fn save(&self) -> String {
        let mut s = format!(
            "seed {}\nname {}\nsent {}\n",
            hex(&self.seed),
            self.name,
            self.last_sent
        );
        if let Some(h) = &self.home {
            s += &format!("home {} {}\n", address_hex(h), self.home_time);
        } else if self.home_time != 0 {
            s += &format!("home - {}\n", self.home_time);
        }
        if let Some((a, at)) = &self.linked {
            s += &format!("linked {} {at}\n", address_hex(a));
        }
        s
    }

    pub fn load(lines: &str) -> Option<Me> {
        let mut me: Option<Me> = None;
        for line in lines.lines() {
            let (k, v) = line.split_once(' ').unwrap_or((line, ""));
            match k {
                "seed" => me = Some(Me::new(unhex(v)?.try_into().ok()?)),
                "name" => me.as_mut()?.name = v.to_string(),
                "sent" => me.as_mut()?.last_sent = v.parse().ok()?,
                "home" => {
                    let (a, t) = v.split_once(' ')?;
                    let m = me.as_mut()?;
                    m.home = (a != "-").then(|| address_of_hex(a)).flatten();
                    m.home_time = t.parse().ok()?;
                }
                "linked" => {
                    let (a, t) = v.split_once(' ')?;
                    me.as_mut()?.linked = Some((address_of_hex(a)?, t.parse().ok()?));
                }
                _ => return None,
            }
        }
        me
    }
}

/// How long where a contact wrote from is taken as where they are: the
/// app's (`bark-app/lib/contacts.dart`), a minute, in which a person on
/// foot stays within a relay's reach.
const FRESH: Duration = Duration::from_secs(60);

/// A person the tool can write to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Contact {
    /// What the person here calls them.
    pub name: String,
    /// Their card's bytes.
    pub card: Vec<u8>,
    /// A message of theirs has opened here: they hold this person's card.
    pub has_mine: bool,
    /// A boxed message of theirs has opened here.
    pub verified: bool,
    /// The address this person wrote from that they have been seen to
    /// know, and the one last sent to them.
    pub knows_mine: Option<Address>,
    pub sent_mine: Option<Address>,
    /// Where they were last known, and when, as `sent`.
    pub place: Option<(Address, u32)>,
}

impl Contact {
    pub fn of_card(name: &str, bytes: &[u8]) -> Result<Contact> {
        let c = Card::decode(bytes).map_err(|_| Error::Other("not a card".into()))?;
        let name = if name.is_empty() { c.name } else { name };
        Ok(Contact {
            name: name.to_string(),
            card: bytes.to_vec(),
            has_mine: false,
            verified: false,
            knows_mine: None,
            sent_mine: None,
            place: c.now.map(|n| (n.address, n.at)),
        })
    }

    pub fn card(&self) -> Card<'_> {
        Card::decode(&self.card).expect("a card checked when kept")
    }

    pub fn identity(&self) -> Identity {
        self.card().identity
    }

    /// A newer card of theirs: its node is their place if newer than the
    /// one known.
    pub fn take_card(&mut self, bytes: &[u8]) -> Result<()> {
        let c = Card::decode(bytes).map_err(|_| Error::Other("not a card".into()))?;
        if c.time < self.card().time {
            return Ok(());
        }
        if let Some(n) = c.now {
            if self.place.is_none_or(|(_, at)| n.at > at) {
                self.place = Some((n.address, n.at));
            }
        }
        self.card = bytes.to_vec();
        Ok(())
    }

    /// Where a text to them goes at `now`, as the app chooses with nobody
    /// asked: where they were last known while that is fresh, else their
    /// home's mailbox, which keeps it, else that place, as nothing else is.
    pub fn destination(&self, now: u32) -> Option<Address> {
        let home = self.card().home;
        match (self.place, home) {
            (None, home) => home,
            (Some((at, _)), None) => Some(at),
            (Some((at, when)), Some(home)) => {
                let fresh = now.saturating_sub(when) < FRESH.as_secs() as u32;
                Some(if fresh || at.node == home.node {
                    at
                } else {
                    home
                })
            }
        }
    }

    pub fn save(&self) -> String {
        let opt = |a: &Option<Address>| a.as_ref().map_or("-".into(), address_hex);
        let place = self
            .place
            .map_or("- 0".into(), |(a, t)| format!("{} {t}", address_hex(&a)));
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{place}",
            self.name.replace(['\t', '\n'], " "),
            hex(&self.card),
            u8::from(self.has_mine),
            u8::from(self.verified),
            opt(&self.knows_mine),
            opt(&self.sent_mine),
        )
    }

    pub fn load(line: &str) -> Option<Contact> {
        let f: Vec<&str> = line.split('\t').collect();
        let [name, card, has, ver, knows, sent, place] = f[..] else {
            return None;
        };
        let mut c = Contact::of_card(name, &unhex(card)?).ok()?;
        c.has_mine = has == "1";
        c.verified = ver == "1";
        c.knows_mine = address_of_hex(knows);
        c.sent_mine = address_of_hex(sent);
        let (a, t) = place.split_once(' ')?;
        c.place = address_of_hex(a).and_then(|a| Some((a, t.parse().ok()?)));
        Some(c)
    }
}

/// A text as the messages that carry it: the plaintext room of a message
/// to `c` less the record's type, and its part byte for one of several,
/// each part in the encoding that holds it shortest (spec "The payload",
/// Long texts).
fn split(text: &str, room: usize) -> Result<Vec<(Encoding, Vec<u8>)>> {
    let mut b = vec![0u8; 4 * text.len() + 1];
    let (e, n) = text::encode(text, &mut b).map_err(|_| Error::Other("not a text".into()))?;
    if n <= room {
        return Ok(vec![(e, b[..n].to_vec())]);
    }
    let room = room - 1;
    let mut parts = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    while start < chars.len() {
        // The most characters from `start` whose bytes fit.
        let mut end = start;
        let mut got = None;
        while end < chars.len() {
            let to = chars.get(end + 1).map_or(text.len(), |c| c.0);
            let piece = &text[chars[start].0..to];
            let mut pb = vec![0u8; 4 * piece.len()];
            let (e, n) = text::encode(piece, &mut pb).expect("bytes enough");
            if n > room {
                break;
            }
            got = Some((e, pb[..n].to_vec()));
            end += 1;
        }
        parts.push(got.ok_or(Error::Other("a character too long for a message".into()))?);
        start = end;
    }
    if parts.len() > usize::from(plain::MAX_PARTS) {
        return Err(Error::Other(format!(
            "the text is too long: {} messages, {} at most",
            parts.len(),
            plain::MAX_PARTS
        )));
    }
    Ok(parts)
}

/// A text to `c` at `dst`, from the node at `here`, as its payloads, each
/// with its `sent` (spec "The payload"). Boxed once `c` holds this
/// person's card, else an introduction carrying it; the header names
/// `here` until `c` has shown they know it, as the app does.
pub fn seal_text(
    me: &mut Me,
    c: &mut Contact,
    dst: Address,
    here: Option<Address>,
    now_s: u64,
    text: &str,
) -> Result<Vec<(Vec<u8>, u32)>> {
    let keys = me.keys();
    let my_card = me.card();
    let address = here.filter(|h| c.knows_mine != Some(*h));
    let over = if c.has_mine {
        BOX_OVERHEAD
    } else {
        INTRO_OVERHEAD + 2 + my_card.len()
    };
    let room = MAX_APP_PAYLOAD - over - HEADER_LEN - address.map_or(0, |_| Address::LEN) - 1;
    let parts = split(text, room)?;
    let of = parts.len() as u8;
    let tag = seal::recipient(&c.identity(), dst.node);
    let their = PublicKey::from(c.identity().0);
    let mut out = Vec::new();
    for (i, (encoding, bytes)) in parts.iter().enumerate() {
        let sent = me.next_sent(now_s);
        let header = Header {
            greater: plain::greater(&keys.identity, &c.identity()),
            sent,
            address,
        };
        let mut buf = [0u8; MAX_PAYLOAD];
        let mut w = Writer::new(&mut buf, &header).map_err(|_| Error::Other("too long".into()))?;
        if !c.has_mine {
            let card = Card::decode(&my_card).expect("its own card");
            w.push(&Record::Card(card))
                .map_err(|_| Error::Other("too long".into()))?;
        }
        let part = (of > 1).then_some(Part {
            number: i as u8 + 1,
            of,
        });
        w.push(&Record::Text {
            encoding: *encoding,
            part,
            bytes,
        })
        .map_err(|_| Error::Other("too long".into()))?;
        let n = w.finish();
        let mut payload = [0u8; MAX_PAYLOAD];
        let len = if c.has_mine {
            let pair = seal::pair(&their, &keys.secret_key())
                .map_err(|_| Error::Other("a contact's key is not one".into()))?;
            seal::seal_box(&pair, tag, &buf[..n], &mut payload)
        } else {
            let ephemeral = SecretKey::from(random::<KEY_LEN>()?);
            seal::seal_intro(&their, tag, &ephemeral, &buf[..n], &mut payload)
        }
        .map_err(|_| Error::Other("too long".into()))?;
        out.push((payload[..len].to_vec(), sent));
    }
    if address.is_some() {
        c.sent_mine = address;
    }
    Ok(out)
}

/// What a payload said, once opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Opened {
    /// A text, or a part of one, from the contact at `from`.
    Text {
        from: usize,
        sent: u32,
        part: Option<(u8, u8)>,
        text: String,
        /// Boxed: only they could have sent it.
        verified: bool,
    },
    /// A reaction to the text this person sent at `to`.
    Reaction {
        from: usize,
        sent: u32,
        to: u32,
        emoji: String,
    },
    /// A card alone.
    Card { from: usize },
    /// A home given by the contact at `from`: the node that holds a
    /// mailbox key for this person. The key is not kept here: the tool
    /// sets no home it was given.
    Home { from: usize, node: NodeId },
    /// From the person's mailbox: the tool reads none yet.
    Mailbox,
    /// For nobody here, or not well formed.
    Nobody,
}

/// Opens `payload`, which arrived for this person, and keeps what it says
/// of its sender in `contacts`: a box under each contact's keys in turn,
/// an introduction under this person's key, its card a contact
/// (spec "The payload").
pub fn open(me: &Me, contacts: &mut Vec<Contact>, payload: &[u8]) -> Opened {
    let keys = me.keys();
    let mut plain = [0u8; MAX_PAYLOAD];
    let (from, n, verified) = match Form::of(payload) {
        Ok(Form::Boxed) => {
            let opened = contacts.iter().enumerate().find_map(|(i, c)| {
                let pair = seal::pair(&PublicKey::from(c.identity().0), &keys.secret_key()).ok()?;
                seal::open_box(&pair, payload, &mut plain)
                    .ok()
                    .map(|n| (i, n))
            });
            match opened {
                Some((i, n)) => (Some(i), n, true),
                None => return Opened::Nobody,
            }
        }
        Ok(Form::Introduction) => match seal::open_intro(&keys.secret_key(), payload, &mut plain) {
            Ok(n) => (None, n, false),
            Err(_) => return Opened::Nobody,
        },
        Ok(f) if f.is_reply() => return Opened::Mailbox,
        _ => return Opened::Nobody,
    };
    let Ok((header, records)) = plain::read(&plain[..n]) else {
        return Opened::Nobody;
    };
    let mut from = from;
    let mut said = None;
    let mut home = None;
    for r in records {
        let Ok(r) = r else { return Opened::Nobody };
        match r {
            Record::Card(card) => {
                let mut b = [0u8; card::MAX_LEN];
                let Ok(len) = card.encode(&mut b) else {
                    return Opened::Nobody;
                };
                // An introduction's card is its sender's.
                if from.is_none() {
                    // An introduction naming an identity verified here is
                    // dropped: a box would have come.
                    match contacts.iter().position(|c| c.identity() == card.identity) {
                        Some(i) if contacts[i].verified => return Opened::Nobody,
                        Some(i) => {
                            let _ = contacts[i].take_card(&b[..len]);
                            from = Some(i);
                        }
                        None => match Contact::of_card("", &b[..len]) {
                            Ok(c) => {
                                contacts.push(c);
                                from = Some(contacts.len() - 1);
                            }
                            Err(_) => return Opened::Nobody,
                        },
                    }
                } else if let Some(i) = contacts.iter().position(|c| c.identity() == card.identity)
                {
                    let _ = contacts[i].take_card(&b[..len]);
                } else if let Ok(c) = Contact::of_card("", &b[..len]) {
                    contacts.push(c);
                }
            }
            Record::Text {
                encoding,
                part,
                bytes,
            } => {
                let Ok(chars) = text::decode(encoding, bytes) else {
                    return Opened::Nobody;
                };
                said = Some((
                    Some(chars.collect::<String>()),
                    part.map(|p| (p.number, p.of)),
                    None,
                ));
            }
            Record::React { sent, emoji } => {
                said = Some((None, None, Some((sent, emoji.to_string()))))
            }
            Record::Home { address, .. } => home = Some(address.node),
        }
    }
    let Some(i) = from else { return Opened::Nobody };
    let c = &mut contacts[i];
    // The direction bit says who wrote it: one this person sent, come
    // back, is dropped.
    if header.greater != plain::greater(&c.identity(), &keys.identity) {
        return Opened::Nobody;
    }
    c.has_mine = true;
    c.verified |= verified;
    c.knows_mine = c.sent_mine.or(c.knows_mine);
    if let Some(a) = header.address {
        c.place = Some((a, header.sent));
    }
    // A home given is what the message is for; a text beside it is
    // shown by the app.
    if let Some(node) = home {
        return Opened::Home { from: i, node };
    }
    match said {
        Some((Some(text), part, _)) => Opened::Text {
            from: i,
            sent: header.sent,
            part,
            text,
            verified,
        },
        Some((None, _, Some((to, emoji)))) => Opened::Reaction {
            from: i,
            sent: header.sent,
            to,
            emoji,
        },
        _ => Opened::Card { from: i },
    }
}

/// The recipient tag of this person at the node `node`.
pub fn tag(me: &Me, node: NodeId) -> u8 {
    seal::recipient(&me.keys().identity, node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bark_proto::Position;

    pub fn person(n: u8, name: &str) -> Me {
        let mut m = Me::new([n; SEED_LEN]);
        m.name = name.into();
        m.linked = Some((
            Address {
                position: Position::from_degrees(59.2, 18.2),
                node: NodeId::from_u64(0xfa4e_0000_0000 + u64::from(n)),
            },
            1000,
        ));
        m
    }

    const NOW: u64 = EPOCH_UNIX_S + 2000;

    #[test]
    fn a_first_message_introduces_then_answers_box() {
        let mut me = person(1, "Me");
        let mut friend = person(2, "Friend");
        let mut my_contacts = vec![Contact::of_card("", &friend.card()).unwrap()];
        let mut their_contacts = Vec::new();
        let dst = my_contacts[0].destination(sent_of(NOW)).unwrap();
        let here = me.linked.map(|l| l.0);
        let sealed = seal_text(&mut me, &mut my_contacts[0], dst, here, NOW, "hej").unwrap();
        assert_eq!(sealed.len(), 1);
        assert_eq!(Form::of(&sealed[0].0), Ok(Form::Introduction));
        let opened = open(&friend, &mut their_contacts, &sealed[0].0);
        assert!(
            matches!(&opened, Opened::Text { text, verified: false, .. } if text == "hej"),
            "{opened:?}"
        );
        assert_eq!(their_contacts[0].name, "Me");
        // The friend answers in a box, which opens at this person's.
        let dst = their_contacts[0].destination(sent_of(NOW)).unwrap();
        assert_eq!(dst, here.unwrap());
        let back = seal_text(&mut friend, &mut their_contacts[0], dst, None, NOW, "då").unwrap();
        assert_eq!(Form::of(&back[0].0), Ok(Form::Boxed));
        let opened = open(&me, &mut my_contacts, &back[0].0);
        assert!(matches!(&opened, Opened::Text { text, verified: true, .. } if text == "då"));
        assert!(my_contacts[0].has_mine && my_contacts[0].verified);
        // Now known to hold this person's card, the next goes boxed.
        let next = seal_text(&mut me, &mut my_contacts[0], dst, here, NOW, "!").unwrap();
        assert_eq!(Form::of(&next[0].0), Ok(Form::Boxed));
    }

    #[test]
    fn a_long_text_goes_in_parts_with_consecutive_sent() {
        let mut me = person(1, "Me");
        let friend = person(2, "Friend");
        let mut c = Contact::of_card("", &friend.card()).unwrap();
        c.has_mine = true;
        let text = "a".repeat(500);
        let dst = c.destination(0).unwrap();
        let sealed = seal_text(&mut me, &mut c, dst, None, NOW, &text).unwrap();
        assert_eq!(sealed.len(), 3);
        assert_eq!(sealed[1].1, sealed[0].1 + 1);
        let mut theirs = vec![Contact::of_card("", &me.card()).unwrap()];
        let mut got = String::new();
        for (p, _) in &sealed {
            assert!(p.len() <= MAX_APP_PAYLOAD);
            match open(&friend, &mut theirs, p) {
                Opened::Text {
                    text,
                    part: Some(_),
                    ..
                } => got += &text,
                o => panic!("{o:?}"),
            }
        }
        assert_eq!(got, text);
    }

    #[test]
    fn me_and_contacts_last_across_a_save() {
        let mut me = person(3, "Olga");
        me.home = me.linked.map(|l| l.0);
        me.home_time = 5;
        assert_eq!(Me::load(&me.save()), Some(me.clone()));
        let mut c = Contact::of_card("Pal", &person(4, "x").card()).unwrap();
        c.has_mine = true;
        c.knows_mine = me.home;
        assert_eq!(Contact::load(&c.save()), Some(c));
        assert!(me.card_text().starts_with("BARK:"));
    }

    #[test]
    fn a_stale_place_sends_to_the_home() {
        let mut friend = person(2, "Friend");
        let home = Address {
            position: Position::from_degrees(50.0, 30.0),
            node: NodeId::from_u64(0xbbbb),
        };
        friend.home = Some(home);
        let c = Contact::of_card("", &friend.card()).unwrap();
        let at = c.place.unwrap();
        assert_eq!(c.destination(at.1 + 10), Some(at.0));
        assert_eq!(c.destination(at.1 + 120), Some(home));
    }
    /// A home given opens as one line's worth: who gave it and the node,
    /// never the key.
    #[test]
    fn a_home_given_opens_as_its_giver_and_node() {
        let me = person(1, "Me");
        let friend = person(2, "Friend");
        let keys = friend.keys();
        let home = Address {
            position: Position::from_degrees(50.45, 30.52),
            node: NodeId::from_u64(0xfa4e_0000_00aa),
        };
        let header = Header {
            greater: plain::greater(&keys.identity, &me.keys().identity),
            sent: 7,
            address: None,
        };
        let mut buf = [0u8; MAX_PAYLOAD];
        let mut w = Writer::new(&mut buf, &header).unwrap();
        let card = friend.card();
        w.push(&Record::Card(Card::decode(&card).unwrap())).unwrap();
        w.push(&Record::Home {
            address: home,
            key: [0x5a; KEY_LEN],
        })
        .unwrap();
        let n = w.finish();
        let mut payload = [0u8; MAX_PAYLOAD];
        let len = seal::seal_intro(
            &PublicKey::from(me.keys().identity.0),
            0,
            &SecretKey::from([9; KEY_LEN]),
            &buf[..n],
            &mut payload,
        )
        .unwrap();
        let mut contacts = Vec::new();
        let opened = open(&me, &mut contacts, &payload[..len]);
        assert!(
            matches!(opened, Opened::Home { from: 0, node } if node == home.node),
            "{opened:?}"
        );
        assert_eq!(contacts[0].name, "Friend");
    }
}

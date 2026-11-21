//! The person's messages through a node: a text sent to a contact, and the
//! messages waiting at the node read, opened and kept, as the app does
//! (spec "Sending", "Receiving", "The payload").

use std::time::Duration;

use bark_app_layer::card::{self, Card};
use bark_link::Clock;
use bark_proto::MsgId;

use crate::link::{id_text, Error, Result, Transport};
use crate::person::{self, hex, Contact, Opened};
use crate::phone::{self, Select};
use crate::show::utc;
use crate::state::State;

fn msg(id: MsgId) -> String {
    format!("{:08x}", id.0)
}

/// Sends `text` to the contact `to` through the node `sel` names: to where
/// the app would send it with nobody asked ([`Contact::destination`]), in
/// as many messages as it takes. A line for each message the node took.
pub fn send<T: Transport>(
    t: &mut T,
    state: &mut State,
    sel: &Select,
    to: &str,
    text: &str,
    time: Duration,
    clock: Clock,
) -> Result<Vec<String>> {
    let i = state.contact(to)?;
    let (_, mut l) = phone::connect(t, state, sel, time, clock)?;
    let now_s = u64::from(clock.utc_s);
    let c = &state.contacts[i];
    let dst = c.destination(person::sent_of(now_s)).ok_or_else(|| {
        Error::Other(format!(
            "{} has no address yet: their card names no node",
            c.name
        ))
    })?;
    let here = state.me()?.linked.map(|l| l.0);
    let State { me, contacts, .. } = &mut *state;
    let me = me.as_mut().expect("made as the link came up");
    let sealed = person::seal_text(me, &mut contacts[i], dst, here, now_s, text)?;
    // What the sealing moved on, the `sent` and the address told, is kept
    // before the node takes a message, so that no `sent` is used twice.
    state.save()?;
    let mut lines = Vec::new();
    for (payload, sent) in &sealed {
        let id = phone::send(&mut l, dst, payload)?;
        lines.push(format!(
            "sent {} to {:?} at {} sent {}",
            msg(id),
            state.contacts[i].name,
            id_text(dst.node),
            utc(person::unix_of(*sent) as u32)
        ));
    }
    Ok(lines)
}

/// The line that tells what a message said.
fn told(state: &State, id: MsgId, o: &Opened) -> String {
    let name = |i: usize| format!("{:?}", state.contacts[i].name);
    let at = |sent: u32| utc(person::unix_of(sent) as u32);
    match o {
        Opened::Text {
            from,
            sent,
            part,
            text,
            verified,
        } => {
            let part = part.map_or("1/1".into(), |(n, of)| format!("{n}/{of}"));
            format!(
                "message {} from {} sent {} part {part} verified {} text {text:?}",
                msg(id),
                name(*from),
                at(*sent),
                if *verified { "yes" } else { "no" }
            )
        }
        Opened::Reaction {
            from,
            sent,
            to,
            emoji,
        } => format!(
            "reaction {} from {} sent {} to {} emoji {emoji:?}",
            msg(id),
            name(*from),
            at(*sent),
            at(*to)
        ),
        Opened::Card { from } => format!("card {} from {}", msg(id), name(*from)),
        Opened::Mailbox => format!(
            "mailbox {}: a reply from a home's mailbox, which this tool does not read",
            msg(id)
        ),
        Opened::Nobody => format!("unread {}: it opens for nobody here", msg(id)),
    }
}

/// Reads every message waiting for this person at the node `sel` names,
/// opens it and keeps what it says, then confirms it, as a paired phone
/// does, or as a guest moves on the number of the last item taken
/// (spec "Receiving", "Public nodes"). A line for each, and one for a
/// message the node gave back.
pub fn inbox<T: Transport>(
    t: &mut T,
    state: &mut State,
    sel: &Select,
    time: Duration,
    clock: Clock,
) -> Result<Vec<String>> {
    let owned = state.pick(sel).is_ok();
    let (n, mut l) = phone::connect(t, state, sel, time, clock)?;
    let mut lines = Vec::new();
    if let Some(g) = phone::given_back(&mut l)? {
        lines.push(format!("given_back {} wait_s {}", msg(g.msg_id), g.wait_s));
    }
    let mut keep = |state: &mut State, id: MsgId, payload: &[u8]| -> Result<()> {
        let State { me, contacts, .. } = &mut *state;
        let me = me.as_ref().expect("made as the link came up");
        let o = person::open(me, contacts, payload);
        lines.push(told(state, id, &o));
        state.save()
    };
    if owned {
        phone::inbox(&mut l, &mut |id, p| keep(state, id, p))?;
    } else {
        let mut after = state.ring(n.id);
        phone::inbox_guest(&mut l, &mut after, &mut |id, p, number| {
            keep(state, id, p)?;
            state.set_ring(n.id, number);
            state.save()
        })?;
    }
    if lines.is_empty() {
        lines.push("inbox empty".into());
    }
    Ok(lines)
}

/// The person: their identity, name, home and card, which another person
/// adds to reach them.
pub fn me(state: &mut State, name: Option<String>) -> Result<Vec<String>> {
    if let Some(n) = name {
        if n.len() > card::MAX_NAME {
            return Err(Error::Other(format!(
                "a name is {} bytes at most",
                card::MAX_NAME
            )));
        }
        state.me()?.name = n;
        state.save()?;
    }
    let me = state.me()?;
    Ok(vec![
        format!("identity {}", hex(&me.keys().identity.0)),
        format!("name {:?}", me.name),
        format!(
            "home {}",
            me.home.map_or("none".into(), |h| id_text(h.node))
        ),
        format!("card {}", me.card_text()),
    ])
}

/// Adds a contact from the text of their card, or takes a newer card of a
/// contact already kept.
pub fn add_contact(state: &mut State, text: &str, name: Option<String>) -> Result<Vec<String>> {
    let mut buf = [0u8; card::MAX_LEN];
    let c = Card::parse(text, &mut buf).map_err(|_| Error::Other("not a card: BARK:…".into()))?;
    let mut bytes = [0u8; card::MAX_LEN];
    let len = c.encode(&mut bytes).expect("a card just read");
    let bytes = &bytes[..len];
    let i = match state
        .contacts
        .iter()
        .position(|k| k.identity() == c.identity)
    {
        Some(i) => {
            state.contacts[i].take_card(bytes)?;
            if let Some(n) = name {
                state.contacts[i].name = n;
            }
            i
        }
        None => {
            state
                .contacts
                .push(Contact::of_card(name.as_deref().unwrap_or(""), bytes)?);
            state.contacts.len() - 1
        }
    };
    state.save()?;
    Ok(vec![contact_line(&state.contacts[i])])
}

fn contact_line(c: &Contact) -> String {
    let card = c.card();
    format!(
        "contact {:?} identity {} verified {} home {} place {}",
        c.name,
        hex(&card.identity.0),
        if c.verified { "yes" } else { "no" },
        card.home.map_or("none".into(), |h| id_text(h.node)),
        c.place.map_or("none".into(), |(a, _)| id_text(a.node)),
    )
}

pub fn contacts(state: &State) -> Vec<String> {
    state.contacts.iter().map(contact_line).collect()
}

/// Makes the node the person's home, or no longer, and puts it on their
/// card, as the app's setting does (spec "Settings", Home; "Mailbox").
pub fn home<T: Transport>(
    t: &mut T,
    state: &mut State,
    sel: &Select,
    on: bool,
    clock: Clock,
) -> Result<Vec<String>> {
    let (n, mut l) = phone::paired_link(t, state, sel, clock)?;
    let key = on.then(|| {
        state
            .me
            .as_ref()
            .expect("made as the link came up")
            .keys()
            .mailbox
    });
    // One `sent` for the write and the card: when the home was set.
    let time = state.me()?.next_sent(u64::from(clock.utc_s));
    let h = phone::home(&mut l, key, time)?;
    let s = phone::status(&mut l)?;
    let me = state.me()?;
    match (on, phone::here(&n, &s)) {
        (true, Some(a)) => me.home = Some(a),
        (true, None) => {
            return Err(Error::Other(
                "the node has no place yet, so it cannot be a home on a card".into(),
            ))
        }
        (false, _) => me.home = me.home.filter(|h| h.node != n.id),
    }
    me.home_time = time;
    state.save()?;
    Ok(vec![
        format!("home {}", if h.mine { "yes" } else { "no" }),
        "card changed: send it to your contacts (bark me)".into(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::{FakeNode, FakeRadio};
    use crate::person::Me;
    use crate::phone::SCAN_TIME;
    use bark_app_layer::seal::{self, Form};
    use bark_app_layer::Address;
    use bark_proto::{NodeId, Position};

    const CLOCK: Clock = Clock {
        utc_s: 1_795_000_000,
        offset_min: 60,
    };

    fn sel(s: &str) -> Select {
        s.parse().unwrap()
    }

    /// A friend at their own node, whose card the person has added.
    fn friend() -> Me {
        let mut f = Me::new([9; 16]);
        f.name = "Friend".into();
        f.linked = Some((
            Address {
                position: Position::from_degrees(50.4, 30.6),
                node: NodeId::from_u64(0xfa4e_0000_0003),
            },
            person::sent_of(u64::from(CLOCK.utc_s)),
        ));
        f
    }

    fn at_node() -> (FakeRadio, State) {
        let node = FakeNode::new(NodeId::from_u64(0xfa4e_0000_0001));
        let radio = FakeRadio::new(vec![node]);
        let mut state = State::in_memory();
        state.me().unwrap().name = "Me".into();
        add_contact(&mut state, &friend().card_text(), None).unwrap();
        (radio, state)
    }

    #[test]
    fn a_guest_sends_a_first_message_with_its_card() {
        let (mut radio, mut state) = at_node();
        let lines = send(
            &mut radio,
            &mut state,
            &sel("01"),
            "friend",
            "hej",
            SCAN_TIME,
            CLOCK,
        )
        .unwrap();
        assert!(lines[0].starts_with("sent "), "{lines:?}");
        let sent = radio.nodes[0].borrow().sent.clone();
        assert_eq!(sent.len(), 1);
        let v = bark_link::Send::decode(&sent[0]).unwrap();
        assert_eq!(v.dst_id, NodeId::from_u64(0xfa4e_0000_0003));
        assert_eq!(Form::of(v.payload), Ok(Form::Introduction));
        let f = friend();
        assert_eq!(
            seal::recipient_of(v.payload),
            Some(person::tag(&f, v.dst_id))
        );
        // The friend opens it and has the person as a contact.
        let mut theirs = Vec::new();
        let o = person::open(&f, &mut theirs, v.payload);
        assert!(
            matches!(&o, Opened::Text { text, .. } if text == "hej"),
            "{o:?}"
        );
        assert_eq!(theirs[0].name, "Me");
        // The person's card names the node the tool is linked to.
        let here = theirs[0].place.unwrap().0;
        assert_eq!(here.node, NodeId::from_u64(0xfa4e_0000_0001));
    }

    #[test]
    fn a_send_waits_for_ready_when_the_turn_is_another_phones() {
        let (mut radio, mut state) = at_node();
        radio.nodes[0].borrow_mut().busy = 1;
        send(
            &mut radio,
            &mut state,
            &sel("01"),
            "friend",
            "hej",
            SCAN_TIME,
            CLOCK,
        )
        .unwrap();
        assert_eq!(radio.nodes[0].borrow().sent.len(), 1);
    }

    /// A message from the friend, sealed to the person at `node`.
    fn from_friend(state: &mut State, node: NodeId, text: &str) -> Vec<u8> {
        let mut f = friend();
        let mut c = Contact::of_card("", &state.me().unwrap().card()).unwrap();
        let dst = Address {
            position: Position::from_degrees(50.45, 30.52),
            node,
        };
        let here = f.linked.map(|l| l.0);
        person::seal_text(&mut f, &mut c, dst, here, u64::from(CLOCK.utc_s), text)
            .unwrap()
            .remove(0)
            .0
    }

    #[test]
    fn a_guest_reads_its_items_once() {
        let (mut radio, mut state) = at_node();
        let node = NodeId::from_u64(0xfa4e_0000_0001);
        let p = from_friend(&mut state, node, "hello");
        radio.nodes[0].borrow_mut().ring = vec![
            (1, MsgId(0x11), vec![0x20, 0x00, 1, 2]),
            (2, MsgId(0x22), p),
        ];
        let lines = inbox(&mut radio, &mut state, &sel("01"), SCAN_TIME, CLOCK).unwrap();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].contains("from \"Friend\"") && lines[0].contains("\"hello\""),
            "{lines:?}"
        );
        assert_eq!(state.ring(node), 2);
        assert!(state.contacts[0].has_mine);
        let lines = inbox(&mut radio, &mut state, &sel("01"), SCAN_TIME, CLOCK).unwrap();
        assert_eq!(lines, ["inbox empty"]);
    }

    #[test]
    fn a_paired_phone_confirms_each_message_and_answers_in_a_box() {
        let (mut radio, mut state) = at_node();
        crate::phone::pair(
            &mut radio,
            &mut state,
            &sel("01"),
            SCAN_TIME,
            Some(123_456),
            CLOCK,
        )
        .unwrap();
        let node = NodeId::from_u64(0xfa4e_0000_0001);
        let p = from_friend(&mut state, node, "hello");
        radio.nodes[0].borrow_mut().ring = vec![(7, MsgId(0x77), p)];
        let lines = inbox(&mut radio, &mut state, &sel("01"), SCAN_TIME, CLOCK).unwrap();
        assert!(lines[0].starts_with("message 00000077"), "{lines:?}");
        assert_eq!(radio.nodes[0].borrow().confirmed, 7);
        // The friend's message showed they hold the person's card.
        send(
            &mut radio,
            &mut state,
            &sel("01"),
            "Friend",
            "då",
            SCAN_TIME,
            CLOCK,
        )
        .unwrap();
        let sent = radio.nodes[0].borrow().sent.clone();
        let v = bark_link::Send::decode(&sent[0]).unwrap();
        assert_eq!(Form::of(v.payload), Ok(Form::Boxed));
        // The record keeps the person's name and tag.
        let n = radio.nodes[0].borrow();
        let r = n.phones.get(0).unwrap();
        assert_eq!(r.name.as_bytes(), b"Me");
        assert_eq!(r.tag, Some(person::tag(state.me.as_ref().unwrap(), node)));
    }

    #[test]
    fn a_home_goes_on_the_card() {
        let (mut radio, mut state) = at_node();
        crate::phone::pair(
            &mut radio,
            &mut state,
            &sel("01"),
            SCAN_TIME,
            Some(123_456),
            CLOCK,
        )
        .unwrap();
        home(&mut radio, &mut state, &sel("01"), true, CLOCK).unwrap();
        let me = state.me.clone().unwrap();
        assert_eq!(radio.nodes[0].borrow().home, Some(me.keys().mailbox));
        let card = me.card();
        let c = Card::decode(&card).unwrap();
        assert_eq!(c.home.unwrap().node, NodeId::from_u64(0xfa4e_0000_0001));
        assert!(c.now.is_none(), "the home is not named twice");
    }
}

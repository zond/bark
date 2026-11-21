//! The owner's commands, which ride on a fetch, and what they did, which
//! rides on its summary (spec "Mailbox", The owner's commands).
//!
//! A command changes the node's records (`bark_link::phones`), which the
//! mailbox does not hold: [`obey`] runs it on them once the mailbox has
//! taken the fetch, and hands the mailbox the people as the records then
//! say them and the outcome for the summary.

use bark_link::phones::{self, Listed, Name, NotAdded, Parts, Phones, NAME_MAX, RECORDS};

use crate::keys::KEY_LEN;
use crate::mailbox::{Mailbox, Person, Ring};
use crate::{be, Error, MAX_PAYLOAD};

/// What the owner, or for [`Command::Leave`] anyone homed, asks the node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Make a record for a person: a home, with their mailbox key, their
    /// recipient tag at the node and their name, and no bond.
    Add {
        /// Their mailbox key.
        key: [u8; KEY_LEN],
        /// Their recipient tag at the node.
        tag: u8,
        /// Their name, as their card has it.
        name: Name,
    },
    /// Remove the home of the record with this number; a bond stays.
    Remove(u16),
    /// Every record.
    List,
    /// The key of the record with this number, which holds a home, is the
    /// owner's.
    HandOver(u16),
    /// The node forgets the sender's key.
    Leave,
}

impl Command {
    const ADD: u8 = 1;
    const REMOVE: u8 = 2;
    const LIST: u8 = 3;
    const HAND_OVER: u8 = 4;
    const LEAVE: u8 = 5;
    /// Bytes of the longest: an add with the longest name.
    pub const MAX_LEN: usize = 1 + KEY_LEN + 1 + NAME_MAX;

    /// Its first byte, which says which command it is.
    pub fn op(&self) -> u8 {
        match self {
            Command::Add { .. } => Self::ADD,
            Command::Remove(_) => Self::REMOVE,
            Command::List => Self::LIST,
            Command::HandOver(_) => Self::HAND_OVER,
            Command::Leave => Self::LEAVE,
        }
    }

    /// The command into `out`, at least [`Command::MAX_LEN`]; its length.
    pub fn encode(&self, out: &mut [u8]) -> usize {
        match *self {
            Command::Add { key, tag, name } => {
                let name = name.as_bytes();
                out[0] = self.op();
                out[1..1 + KEY_LEN].copy_from_slice(&key);
                out[1 + KEY_LEN] = tag;
                out[2 + KEY_LEN..2 + KEY_LEN + name.len()].copy_from_slice(name);
                2 + KEY_LEN + name.len()
            }
            Command::Remove(n) | Command::HandOver(n) => {
                out[0] = self.op();
                out[1..3].copy_from_slice(&n.to_be_bytes());
                3
            }
            Command::List | Command::Leave => {
                out[0] = self.op();
                1
            }
        }
    }

    /// The command `b` holds whole.
    pub fn decode(b: &[u8]) -> Result<Command, Error> {
        let number = || match b.len() {
            3 => Ok(u16::from_be_bytes([b[1], b[2]])),
            _ => Err(Error::Malformed),
        };
        match *b.first().ok_or(Error::Malformed)? {
            Self::ADD if b.len() >= 2 + KEY_LEN => {
                let mut key = [0u8; KEY_LEN];
                key.copy_from_slice(&b[1..1 + KEY_LEN]);
                let name = Name::new(&b[2 + KEY_LEN..]).map_err(|_| Error::Malformed)?;
                Ok(Command::Add {
                    key,
                    tag: b[1 + KEY_LEN],
                    name,
                })
            }
            Self::REMOVE => number().map(Command::Remove),
            Self::HAND_OVER => number().map(Command::HandOver),
            Self::LIST if b.len() == 1 => Ok(Command::List),
            Self::LEAVE if b.len() == 1 => Ok(Command::Leave),
            _ => Err(Error::Malformed),
        }
    }
}

/// What a command did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    /// Done; for a list, the records follow.
    Ok = 0,
    /// The sender's key is not the owner's.
    NotAllowed = 1,
    /// An add, and no record is free.
    Full = 2,
    /// An add of a key a record holds: nothing changed.
    Already = 3,
    /// No record of that number holds a home.
    NoSuchHome = 4,
    /// The sender's home here was removed; nothing else was done.
    Removed = 5,
}

impl Code {
    fn of(b: u8) -> Result<Code, Error> {
        [
            Code::Ok,
            Code::NotAllowed,
            Code::Full,
            Code::Already,
            Code::NoSuchHome,
            Code::Removed,
        ]
        .into_iter()
        .find(|c| *c as u8 == b)
        .ok_or(Error::Malformed)
    }
}

/// Bytes of a summary's plaintext without an outcome: the round's items
/// (1), the items remaining (2) and the flags (1), whose high four bits
/// are the home's newest payload version.
pub const SUMMARY_PLAIN_LEN: usize = 1 + 2 + 1;

/// Bytes of the summary's head, the tag and that plaintext.
const SUMMARY_FIXED: usize = crate::seal::HEAD_LEN + crate::seal::TAG_LEN + SUMMARY_PLAIN_LEN;

/// Bytes the owner's list may take: what a payload holds after the rest
/// of the summary and the outcome's code.
pub const LIST_LEN: usize = MAX_PAYLOAD - SUMMARY_FIXED - 1;

// Eight records fit, each with a name of 18 bytes.
const _: () = assert!(LIST_LEN / RECORDS - phones::PER_RECORD == 18);

/// The owner's list: every record, as a Phones read gives it, the names
/// cut only as far as all must be to fit ([`Phones::list`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct List {
    len: u8,
    bytes: [u8; LIST_LEN],
}

impl List {
    /// The list's bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }

    /// The records it lists, into `out`; their count.
    pub fn records<'a>(&'a self, out: &mut [Option<Listed<'a>>; RECORDS]) -> Result<usize, Error> {
        phones::decode_list(self.as_bytes(), out).map_err(|_| Error::Malformed)
    }

    fn of(b: &[u8]) -> Result<List, Error> {
        let mut bytes = [0u8; LIST_LEN];
        bytes
            .get_mut(..b.len())
            .ok_or(Error::Malformed)?
            .copy_from_slice(b);
        let list = List {
            len: b.len() as u8,
            bytes,
        };
        list.records(&mut [None; RECORDS])?;
        Ok(list)
    }
}

/// What a command did, as its summary says. A round keeps it until its
/// summary goes, in place: nothing here allocates.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Its code.
    Done(Code),
    /// The owner's list, a list's outcome when it is allowed.
    List(List),
}

impl Outcome {
    /// Bytes of the longest: the code and the longest list.
    pub const MAX_LEN: usize = 1 + LIST_LEN;

    /// The outcome into `out`; its length.
    pub fn encode(&self, out: &mut [u8]) -> usize {
        match self {
            Outcome::Done(c) => {
                out[0] = *c as u8;
                1
            }
            Outcome::List(l) => {
                let b = l.as_bytes();
                out[0] = Code::Ok as u8;
                out[1..1 + b.len()].copy_from_slice(b);
                1 + b.len()
            }
        }
    }

    /// The outcome `b` holds whole: records after an ok are a list.
    pub fn decode(b: &[u8]) -> Result<Outcome, Error> {
        let (&code, rest) = b.split_first().ok_or(Error::Malformed)?;
        match (Code::of(code)?, rest.is_empty()) {
            (c, true) => Ok(Outcome::Done(c)),
            (Code::Ok, false) => List::of(rest).map(Outcome::List),
            _ => Err(Error::Malformed),
        }
    }
}

/// The summary of a round (spec "Mailbox"): what it carries and what
/// stays, whether its requester is the owner, and what a command did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Summary {
    /// The round's items.
    pub items: u8,
    /// The requester's items still unsent after the round.
    pub remaining: u16,
    /// The requester's key is the owner's.
    pub owner: bool,
    /// The newest payload version the home node reads
    /// ([`crate::VERSION`]), so that the app away writes it one it reads
    /// (spec "The payload").
    pub newest: u8,
    /// What the fetch's command did, none for a fetch without one; or,
    /// with or without one, that the sender's home was removed.
    pub outcome: Option<Outcome>,
}

impl Summary {
    /// The plaintext into `out`; its length.
    pub fn encode(&self, out: &mut [u8; SUMMARY_PLAIN_LEN + Outcome::MAX_LEN]) -> usize {
        out[0] = self.items;
        out[1..3].copy_from_slice(&self.remaining.to_be_bytes());
        out[3] = u8::from(self.owner) | (self.newest & 0x0f) << 4;
        SUMMARY_PLAIN_LEN
            + self
                .outcome
                .map_or(0, |o| o.encode(&mut out[SUMMARY_PLAIN_LEN..]))
    }

    /// The plaintext `b` holds whole.
    pub fn decode(b: &[u8]) -> Result<Summary, Error> {
        if b.len() < SUMMARY_PLAIN_LEN {
            return Err(Error::Malformed);
        }
        let rest = &b[SUMMARY_PLAIN_LEN..];
        Ok(Summary {
            items: b[0],
            remaining: be(&b[1..3]) as u16,
            owner: b[3] & 1 != 0,
            newest: b[3] >> 4,
            outcome: match rest.is_empty() {
                true => None,
                false => Some(Outcome::decode(rest)?),
            },
        })
    }
}

/// Runs `command`, of the fetch of `sent`, from the person of the record
/// at `from` on `phones`, the ring's newest item `newest`, today `day`
/// and a bit for each record whose phone is linked in `linked` (spec
/// "Mailbox", The owner's commands). A record added takes no fetch of its
/// key that is not later than `sent`.
pub fn run(
    phones: &mut Phones,
    from: usize,
    command: &Command,
    sent: u32,
    newest: u32,
    day: u16,
    linked: u8,
) -> Outcome {
    if *command != Command::Leave && !phones.is_owner(from) {
        return Outcome::Done(Code::NotAllowed);
    }
    let home = Parts {
        bond: false,
        home: true,
    };
    let done = |ok: bool| Outcome::Done(if ok { Code::Ok } else { Code::NoSuchHome });
    match *command {
        Command::Add { key, tag, name } => match phones.add(key, tag, name, newest, day, sent) {
            Ok(_) => Outcome::Done(Code::Ok),
            Err(NotAdded::Full) => Outcome::Done(Code::Full),
            Err(NotAdded::Already) => Outcome::Done(Code::Already),
        },
        Command::Remove(number) => done(phones.remove(number, home).is_some()),
        Command::HandOver(number) => done(phones.make_owner(number).is_some()),
        Command::List => {
            let mut bytes = [0u8; LIST_LEN];
            let len = phones.list(from, linked, &mut bytes) as u8;
            Outcome::List(List { len, bytes })
        }
        Command::Leave => {
            phones.leave(from);
            Outcome::Done(Code::Ok)
        }
    }
}

/// After [`Mailbox::arrive`] took a fetch for the person at `i`: their
/// record keeps the last fetch and the day `day` they were seen, the
/// fetch's command runs on `phones` ([`run`]), and the mailbox takes the
/// people as the records then say them, and the command's outcome for the
/// summary.
pub fn obey<R: Ring>(
    m: &mut Mailbox<R>,
    phones: &mut Phones,
    i: usize,
    command: Option<Command>,
    day: u16,
    linked: u8,
) {
    // The fetch's `sent` is the person's last fetch now.
    let Some(p) = m.person(i) else {
        return;
    };
    phones.progress(i, p.read, p.last_fetch);
    phones.seen(i, day);
    let outcome = command.map(|c| run(phones, i, &c, p.last_fetch, m.newest(), day, linked));
    for j in 0..RECORDS {
        m.set_person(j, phones.get(j).map(Person::of));
    }
    if let Some(o) = outcome {
        m.answer(i, o);
    }
}

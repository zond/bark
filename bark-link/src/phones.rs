//! The node's phones: its records, how it decides on them and stores them
//! in flash, and the Phones characteristic's values (spec "Pairing",
//! "Phones").
//!
//! The store's page (`bark-fw/src/store.rs`) is a log of 32-byte slots, the
//! last entry of each kind the one that holds. The phones' kinds:
//!
//! - a **record** ([`Record::encode`]), four slots, or that its place is free;
//! - **progress** ([`Progress`]), one slot: what changes with each message
//!   read or fetched, so those writes cost a slot and not four;
//! - the **settings** ([`Settings`]), one slot.
//!
//! Each ends in a check over the rest: the flash writes a word at a time in
//! address order, so an entry cut short by a power cut leaves its last
//! word erased and fails it.

use crate::home::KEY_LEN;
use crate::Malformed;

/// Records a node keeps: a bit for each in a slot's mark, which is a byte
/// (spec "Mailbox").
pub const RECORDS: usize = 8;
/// Bytes of a person's name at most, as their card has it
/// (spec "The payload").
pub const NAME_MAX: usize = 32;

/// The first byte of a record entry.
pub const RECORD_TAG: u8 = 0x83;
/// The first byte of a progress entry.
pub const PROGRESS_TAG: u8 = 0x84;
/// The first byte of the settings entry.
pub const SETTINGS_TAG: u8 = 0x82;
/// Bytes of a store slot (`bark_proto::store::RECORD_LEN`).
pub const SLOT_LEN: usize = 32;
/// Bytes of a record entry: four slots.
pub const RECORD_LEN: usize = 4 * SLOT_LEN;

/// The keys of a bond with a phone, and the phone they belong to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keys {
    /// The phone's identity address type, as the HCI gives it: 0 public, 1
    /// random static (Bluetooth Core Vol 3 Part H §3.6.5).
    pub kind: u8,
    /// The phone's identity address, least significant byte first.
    pub addr: [u8; 6],
    /// The phone's identity resolving key, if it gave one (Vol 3 Part H
    /// §3.6.4).
    pub irk: Option<[u8; 16]>,
    /// The long-term key the link is encrypted with (Vol 3 Part H
    /// §2.3.5.6.5).
    pub ltk: [u8; 16],
    /// The link version whose attribute table the phone last saw, 0 for
    /// none known (spec "Versions").
    pub seen: u8,
    /// The phone's subscription to Service Changed, which a bonded
    /// client's is kept across connections (Vol 3 Part G §3.3.3.3).
    pub service_changed: bool,
}

impl Keys {
    /// Whether these keys are the phone with this identity address.
    pub fn is(&self, kind: u8, addr: &[u8; 6]) -> bool {
        self.kind == kind && self.addr == *addr
    }
}

/// A person's name, as their phone wrote it to Recipient: UTF-8, which
/// never holds the byte `0xFF`, so a stored record's bytes never read as
/// erased flash.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Name {
    len: u8,
    bytes: [u8; NAME_MAX],
}

impl Name {
    /// No name: a guest writes none.
    pub const EMPTY: Name = Name {
        len: 0,
        bytes: [0; NAME_MAX],
    };

    /// The name `b`, if it is UTF-8 of at most [`NAME_MAX`] bytes.
    pub fn new(b: &[u8]) -> Result<Name, Malformed> {
        if b.len() > NAME_MAX {
            return Err(Malformed::Length);
        }
        core::str::from_utf8(b).map_err(|_| Malformed::Value)?;
        let mut bytes = [0; NAME_MAX];
        bytes[..b.len()].copy_from_slice(b);
        Ok(Name {
            len: b.len() as u8,
            bytes,
        })
    }

    /// The name's bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }
}

/// What a record holds of a person's mailbox key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HomeKey {
    /// No key: the node is not their home.
    #[default]
    None,
    /// The node is their home, under this key.
    Home([u8; KEY_LEN]),
    /// Their home was removed: the key stays, with none of a home's
    /// rights, so that a fetch under it is told so (spec "Pairing").
    Removed([u8; KEY_LEN]),
}

impl HomeKey {
    /// The home's key; none for a removed one.
    pub fn home(self) -> Option<[u8; KEY_LEN]> {
        match self {
            HomeKey::Home(k) => Some(k),
            _ => None,
        }
    }

    /// The key a fetch may open under, and whether its home was removed.
    pub fn opens(self) -> Option<([u8; KEY_LEN], bool)> {
        match self {
            HomeKey::None => None,
            HomeKey::Home(k) => Some((k, false)),
            HomeKey::Removed(k) => Some((k, true)),
        }
    }

    /// Whether it holds `key`, a home or removed.
    fn holds(self, key: [u8; KEY_LEN]) -> bool {
        self.opens().is_some_and(|(k, _)| k == key)
    }
}

/// A person paired with the node, or whose home it is, or both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    /// Its number: one more than the last the node gave, so that Phones
    /// names a record that is gone by a number no other takes for long.
    pub number: u16,
    /// The bond with the person's phone; none once the phone unpaired.
    pub bond: Option<Keys>,
    /// Their name, as written to Recipient.
    pub name: Name,
    /// Their recipient tag at the node, as written to Recipient.
    pub tag: Option<u8>,
    /// Their mailbox key, while the node is their home or once was.
    pub key: HomeKey,
    /// The newest item in the ring when the record was made: its bits in
    /// the marks of items up to this one were left by a record freed
    /// before, and count for nothing; its phone still reads those with
    /// its tag (spec "Mailbox").
    pub from: u32,
    /// The number of the last of their items the phone confirmed.
    pub read: u32,
    /// The `sent` of the last fetch the record took (spec "Mailbox"); 0
    /// before any, and again once the key changes.
    pub last_fetch: u32,
    /// The day its person was last seen, from 1 on 2026-01-01 UTC: their
    /// phone linked, a fetch of theirs taken, or the record made; 0 when
    /// the node had no time at any of those.
    pub day: u16,
    /// Its key is the owner's (spec "The node's owner"): every record
    /// that holds the owner's key has the mark, and no other.
    pub owner: bool,
}

impl Record {
    /// A record with the ring's newest item: a phone's just paired, or a
    /// person's the owner adds.
    fn new(number: u16, bond: Option<Keys>, newest: u32) -> Record {
        Record {
            number,
            bond,
            name: Name::EMPTY,
            tag: None,
            key: HomeKey::None,
            from: newest,
            read: 0,
            last_fetch: 0,
            day: 0,
            owner: false,
        }
    }

    /// Whether the record holds anything: a record that holds neither a
    /// bond nor a key is freed.
    fn holds(&self) -> bool {
        self.bond.is_some() || self.key != HomeKey::None
    }

    /// Whether it holds nothing but a removed home: it is never shown,
    /// and is the first to go when the node needs a record
    /// (spec "Pairing").
    pub fn only_removed(&self) -> bool {
        self.bond.is_none() && matches!(self.key, HomeKey::Removed(_))
    }
}

// The record entry's layout. Bytes not named are 0.
const AT_INDEX: usize = 1;
const AT_NUMBER: usize = 2;
const AT_FLAGS: usize = 4;
const AT_KIND: usize = 5;
const AT_ADDR: usize = 6;
const AT_SEEN: usize = 12;
const AT_TAG: usize = 13;
const AT_DAY: usize = 14;
const AT_FROM: usize = 16;
const AT_READ: usize = 19;
const AT_FETCH: usize = 22;
const AT_NAME_LEN: usize = 26;
const AT_NAME: usize = 27;
const AT_LTK: usize = AT_NAME + NAME_MAX;
const AT_IRK: usize = AT_LTK + 16;
const AT_KEY: usize = AT_IRK + 16;
const AT_CHECK: usize = RECORD_LEN - 4;
const _: () = assert!(AT_KEY + KEY_LEN <= AT_CHECK);

const FLAG_BOND: u8 = 0x01;
const FLAG_IRK: u8 = 0x02;
const FLAG_CHANGED: u8 = 0x04;
const FLAG_TAG: u8 = 0x08;
const FLAG_KEY: u8 = 0x10;
const FLAG_OWNER: u8 = 0x20;
const FLAG_REMOVED: u8 = 0x40;
const FLAGS: u8 =
    FLAG_BOND | FLAG_IRK | FLAG_CHANGED | FLAG_TAG | FLAG_KEY | FLAG_OWNER | FLAG_REMOVED;

/// 32-bit FNV-1a, as the store's other entries check theirs.
fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5, |h, &b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

fn be24(b: &[u8]) -> u32 {
    u32::from_be_bytes([0, b[0], b[1], b[2]])
}

fn put24(b: &mut [u8], n: u32) {
    b[..3].copy_from_slice(&n.to_be_bytes()[1..]);
}

fn seal<const N: usize>(mut r: [u8; N]) -> [u8; N] {
    let check = fnv1a(&r[..N - 4]);
    r[N - 4..].copy_from_slice(&check.to_be_bytes());
    r
}

fn whole<const N: usize>(r: &[u8; N], tag: u8) -> bool {
    r[0] == tag && fnv1a(&r[..N - 4]).to_be_bytes() == r[N - 4..]
}

impl Record {
    /// The record entry for place `index`, or the entry that frees it.
    pub fn encode(index: usize, record: Option<&Record>) -> [u8; RECORD_LEN] {
        let mut r = [0u8; RECORD_LEN];
        r[0] = RECORD_TAG;
        r[AT_INDEX] = index as u8;
        if let Some(rec) = record {
            let mut flags = 0;
            r[AT_NUMBER..AT_NUMBER + 2].copy_from_slice(&rec.number.to_be_bytes());
            if let Some(b) = rec.bond {
                flags |= FLAG_BOND;
                r[AT_KIND] = b.kind;
                r[AT_ADDR..AT_ADDR + 6].copy_from_slice(&b.addr);
                r[AT_SEEN] = b.seen;
                r[AT_LTK..AT_LTK + 16].copy_from_slice(&b.ltk);
                if let Some(irk) = b.irk {
                    flags |= FLAG_IRK;
                    r[AT_IRK..AT_IRK + 16].copy_from_slice(&irk);
                }
                if b.service_changed {
                    flags |= FLAG_CHANGED;
                }
            }
            if let Some(t) = rec.tag {
                flags |= FLAG_TAG;
                r[AT_TAG] = t;
            }
            if let Some((k, removed)) = rec.key.opens() {
                flags |= FLAG_KEY;
                r[AT_KEY..AT_KEY + KEY_LEN].copy_from_slice(&k);
                if removed {
                    flags |= FLAG_REMOVED;
                } else if rec.owner {
                    flags |= FLAG_OWNER;
                }
            }
            r[AT_FLAGS] = flags;
            r[AT_DAY..AT_DAY + 2].copy_from_slice(&rec.day.to_be_bytes());
            put24(&mut r[AT_FROM..], rec.from);
            put24(&mut r[AT_READ..], rec.read);
            r[AT_FETCH..AT_FETCH + 4].copy_from_slice(&rec.last_fetch.to_be_bytes());
            r[AT_NAME_LEN] = rec.name.len;
            r[AT_NAME..AT_NAME + NAME_MAX].copy_from_slice(&rec.name.bytes);
        }
        seal(r)
    }

    /// What a whole record entry says: its place, and the record there or
    /// none; `None` for another entry or one cut short.
    pub fn decode(r: &[u8; RECORD_LEN]) -> Option<(usize, Option<Record>)> {
        let index = usize::from(r[AT_INDEX]);
        let flags = r[AT_FLAGS];
        // The mark and the removal are a key's, and the mark a home's.
        let keyed = flags & FLAG_KEY != 0;
        let marked = flags & FLAG_OWNER != 0 && (!keyed || flags & FLAG_REMOVED != 0);
        let removed = flags & FLAG_REMOVED != 0 && !keyed;
        if !whole(r, RECORD_TAG) || index >= RECORDS || flags & !FLAGS != 0 || marked || removed {
            return None;
        }
        if flags & (FLAG_BOND | FLAG_KEY) == 0 {
            return Some((index, None));
        }
        let take16 = |at: usize| -> [u8; 16] { r[at..at + 16].try_into().expect("16 bytes") };
        let key = || r[AT_KEY..AT_KEY + KEY_LEN].try_into().expect("32 bytes");
        let bond = (flags & FLAG_BOND != 0).then(|| Keys {
            kind: r[AT_KIND],
            addr: r[AT_ADDR..AT_ADDR + 6].try_into().expect("6 bytes"),
            irk: (flags & FLAG_IRK != 0).then(|| take16(AT_IRK)),
            ltk: take16(AT_LTK),
            seen: r[AT_SEEN],
            service_changed: flags & FLAG_CHANGED != 0,
        });
        let name_len = usize::from(r[AT_NAME_LEN]);
        let name = Name::new(r[AT_NAME..AT_NAME + NAME_MAX].get(..name_len)?).ok()?;
        Some((
            index,
            Some(Record {
                number: u16::from_be_bytes([r[AT_NUMBER], r[AT_NUMBER + 1]]),
                bond,
                name,
                tag: (flags & FLAG_TAG != 0).then_some(r[AT_TAG]),
                key: match (keyed, flags & FLAG_REMOVED != 0) {
                    (false, _) => HomeKey::None,
                    (true, false) => HomeKey::Home(key()),
                    (true, true) => HomeKey::Removed(key()),
                },
                from: be24(&r[AT_FROM..]),
                read: be24(&r[AT_READ..]),
                last_fetch: u32::from_be_bytes(
                    r[AT_FETCH..AT_FETCH + 4].try_into().expect("4 bytes"),
                ),
                day: u16::from_be_bytes([r[AT_DAY], r[AT_DAY + 1]]),
                owner: flags & FLAG_OWNER != 0,
            }),
        ))
    }
}

/// A progress entry: the last confirmed and the last fetch of the record
/// at `index` whose number is `number`; one of a record since replaced
/// counts for nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    /// The record's place.
    pub index: usize,
    /// The record's number.
    pub number: u16,
    /// [`Record::read`].
    pub read: u32,
    /// [`Record::last_fetch`].
    pub last_fetch: u32,
}

impl Progress {
    /// The entry.
    pub fn encode(&self) -> [u8; SLOT_LEN] {
        let mut r = [0u8; SLOT_LEN];
        r[0] = PROGRESS_TAG;
        r[1] = self.index as u8;
        r[2..4].copy_from_slice(&self.number.to_be_bytes());
        put24(&mut r[4..], self.read);
        r[7..11].copy_from_slice(&self.last_fetch.to_be_bytes());
        seal(r)
    }

    /// A whole entry; `None` for another or one cut short.
    pub fn decode(r: &[u8; SLOT_LEN]) -> Option<Progress> {
        (whole(r, PROGRESS_TAG) && usize::from(r[1]) < RECORDS).then(|| Progress {
            index: usize::from(r[1]),
            number: u16::from_be_bytes([r[2], r[3]]),
            read: be24(&r[4..]),
            last_fetch: u32::from_be_bytes(r[7..11].try_into().expect("4 bytes")),
        })
    }
}

/// The settings entry: the node's own, one for all its phones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// The node's paired phones have made it private (spec "Public nodes").
    pub private: bool,
    /// The number the next record takes.
    pub next: u16,
}

impl Settings {
    /// The entry.
    pub fn encode(&self) -> [u8; SLOT_LEN] {
        let mut r = [0u8; SLOT_LEN];
        r[0] = SETTINGS_TAG;
        r[1] = u8::from(self.private);
        r[2..4].copy_from_slice(&self.next.to_be_bytes());
        seal(r)
    }

    /// A whole entry; `None` for another or one cut short.
    pub fn decode(r: &[u8; SLOT_LEN]) -> Option<Settings> {
        (whole(r, SETTINGS_TAG) && r[1] <= 1).then(|| Settings {
            private: r[1] == 1,
            next: u16::from_be_bytes([r[2], r[3]]),
        })
    }
}

/// Bytes of an entry that starts with `tag`, as the store steps over it:
/// a record's four slots, any other one.
pub fn entry_len(tag: u8) -> usize {
    match tag {
        RECORD_TAG => RECORD_LEN,
        _ => SLOT_LEN,
    }
}

/// The phones as the store's page holds them, read entry by entry in the
/// order written.
#[derive(Clone, Copy, Debug, Default)]
pub struct Load {
    records: [Option<Record>; RECORDS],
    settings: Option<Settings>,
}

impl Load {
    /// An entry of the page, whole or not; others' are passed over.
    pub fn entry(&mut self, bytes: &[u8]) {
        match (bytes.first(), bytes.len()) {
            (Some(&RECORD_TAG), RECORD_LEN) => {
                if let Some((i, rec)) = Record::decode(bytes.try_into().expect("a record")) {
                    self.records[i] = rec;
                }
            }
            (Some(&PROGRESS_TAG), SLOT_LEN) => {
                if let Some(p) = Progress::decode(bytes.try_into().expect("a slot")) {
                    if let Some(r) = self.records[p.index]
                        .as_mut()
                        .filter(|r| r.number == p.number)
                    {
                        r.read = p.read;
                        r.last_fetch = p.last_fetch;
                    }
                }
            }
            (Some(&SETTINGS_TAG), SLOT_LEN) => {
                self.settings =
                    Settings::decode(bytes.try_into().expect("a slot")).or(self.settings);
            }
            _ => {}
        }
    }

    /// The phones the page holds; none from a page without settings, as
    /// every node that keeps a record has written them.
    pub fn phones(&self) -> Phones {
        let Some(s) = self.settings else {
            return Phones::EMPTY;
        };
        let mut p = Phones {
            records: self.records,
            private: s.private,
            next: s.next.max(1),
            dirty: Dirty::default(),
            listed: 0,
        };
        p.private &= p.paired() > 0;
        // A power cut between the records a change of the mark writes
        // leaves it on some of them: it goes with the first one's key.
        p.mark(p.owner());
        p.dirty = Dirty::default();
        p
    }
}

/// What has changed since the store last wrote it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dirty {
    /// Records to write whole, a bit for each place.
    pub records: u8,
    /// Records whose progress alone changed.
    pub progress: u8,
    /// The settings.
    pub settings: bool,
}

/// What a pairing did, or why it was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paired {
    /// The phone's record held a bond with it before: the new keys replace
    /// the old, and the rest of the record stays.
    Again(usize),
    /// A free place took it, or one that held only a removed home.
    Added(usize),
    /// Nobody was paired and every place held a home without a phone: the
    /// one whose person was seen longest ago took it, its home forgotten,
    /// the owner's last.
    Took(usize),
}

impl Paired {
    /// The record's place.
    pub fn index(&self) -> usize {
        match *self {
            Paired::Again(i) | Paired::Added(i) | Paired::Took(i) => i,
        }
    }
}

/// Every record is in use, none holds only a removed home, and a phone is
/// paired: the pairing is refused (spec "Pairing").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;

/// What a removal asks for (spec "Phones").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parts {
    /// The bond.
    pub bond: bool,
    /// The home.
    pub home: bool,
}

/// Why an owner's add made no record (spec "The node's owner").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotAdded {
    /// Every place holds a record, and none only a removed home.
    Full,
    /// A record holds the key as a home already: nothing changed.
    Already,
}

/// What a removal took.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Removed {
    /// The record's place.
    pub index: usize,
    /// The bond it removed, whose phone's link is to end.
    pub bond: Option<Keys>,
}

/// The node's records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Phones {
    /// By place, [`RECORDS`] of them; a place's bit in a slot's mark is its
    /// index.
    pub records: [Option<Record>; RECORDS],
    /// The node is private: only while a phone is paired.
    private: bool,
    /// The number the next record takes.
    next: u16,
    dirty: Dirty,
    /// Moves at each change to what [`Phones::page`] lists of the records.
    listed: u8,
}

impl Phones {
    /// No record: a node never paired.
    pub const EMPTY: Phones = Phones {
        records: [None; RECORDS],
        private: false,
        next: 1,
        dirty: Dirty {
            records: 0,
            progress: 0,
            settings: false,
        },
        listed: 0,
    };

    /// The record at `i`.
    pub fn get(&self, i: usize) -> Option<&Record> {
        self.records.get(i)?.as_ref()
    }

    /// Records that hold a bond: the phones paired.
    pub fn paired(&self) -> usize {
        self.records
            .iter()
            .flatten()
            .filter(|r| r.bond.is_some())
            .count()
    }

    /// Whether the node serves guests (spec "Public nodes").
    pub fn public(&self) -> bool {
        !self.private
    }

    /// Node's `room`: how many more phones may pair; 1 on a node nobody is
    /// paired with whose every place holds a home left behind.
    pub fn room(&self) -> u8 {
        let free = self
            .records
            .iter()
            .filter(|r| r.is_none_or(|r| r.only_removed()))
            .count() as u8;
        match (free, self.paired()) {
            (0, 0) => 1,
            (f, _) => f,
        }
    }

    /// The place of the record whose bond is the phone with this identity
    /// address.
    pub fn with_phone(&self, kind: u8, addr: &[u8; 6]) -> Option<usize> {
        self.records
            .iter()
            .position(|r| r.and_then(|r| r.bond).is_some_and(|b| b.is(kind, addr)))
    }

    /// Whether a pairing from the phone with this identity address, if the
    /// host knows it, would be taken: it holds a record, or one is free, or
    /// nobody is paired.
    pub fn takes(&self, phone: Option<(u8, [u8; 6])>) -> bool {
        self.room() > 0 || phone.is_some_and(|(k, a)| self.with_phone(k, &a).is_some())
    }

    /// The place a new record takes: a free one, else the one that holds
    /// only a removed home whose person was seen longest ago
    /// (spec "Pairing").
    fn free(&self) -> Option<usize> {
        self.records.iter().position(Option::is_none).or_else(|| {
            (0..RECORDS)
                .filter(|&i| self.records[i].is_some_and(|r| r.only_removed()))
                .min_by_key(|&i| self.records[i].map(|r| (r.day, r.number)))
        })
    }

    fn number(&mut self) -> u16 {
        let n = self.next;
        self.next = match n.wrapping_add(1) {
            0 => 1,
            n => n,
        };
        self.dirty.settings = true;
        n
    }

    fn touch(&mut self, i: usize) {
        self.dirty.records |= 1 << i;
    }

    /// What a Phones read lists changed.
    fn relisted(&mut self) {
        self.listed = self.listed.wrapping_add(1);
    }

    /// Moves, wrapping, at each change to what [`Phones::page`] lists but
    /// which phones are linked, which the link knows (spec "Phone link",
    /// Status).
    pub fn listed(&self) -> u8 {
        self.listed
    }

    /// Take a phone's authenticated pairing that bonds, the ring's newest
    /// item being `newest` (spec "Pairing").
    pub fn pair(&mut self, keys: Keys, newest: u32) -> Result<Paired, Full> {
        if let Some(i) = self.with_phone(keys.kind, &keys.addr) {
            let r = self.records[i].as_mut().expect("it holds the bond");
            r.bond = Some(keys);
            self.touch(i);
            return Ok(Paired::Again(i));
        }
        let (i, how) = match self.free() {
            Some(i) => (i, Paired::Added(i)),
            None if self.paired() == 0 => {
                let i = (0..RECORDS)
                    .min_by_key(|&i| self.records[i].map(|r| (r.owner, r.day, r.number)))
                    .expect("eight places");
                (i, Paired::Took(i))
            }
            None => return Err(Full),
        };
        let owner = self.owner();
        let number = self.number();
        self.records[i] = Some(Record::new(number, Some(keys), newest));
        self.touch(i);
        self.relisted();
        self.keep_mark(owner, None);
        Ok(how)
    }

    /// The record at `i` with its bond's keys changed, as the phone's view
    /// of the table changed (spec "Versions").
    pub fn set_keys(&mut self, i: usize, keys: Keys) {
        if let Some(r) = self.records[i].as_mut() {
            if r.bond != Some(keys) {
                r.bond = Some(keys);
                self.touch(i);
            }
        }
    }

    /// The phone at `i` wrote Recipient: its person's tag at the node, and
    /// their name.
    pub fn set_recipient(&mut self, i: usize, tag: u8, name: Name) {
        if let Some(r) = self.records[i].as_mut() {
            if r.tag != Some(tag) || r.name != name {
                r.tag = Some(tag);
                r.name = name;
                self.touch(i);
                self.relisted();
            }
        }
    }

    /// The person of the record at `i` is seen on day `day`, their phone
    /// linked or their fetch taken; 0 when the node has no time: written
    /// when the day changes, so at most once a day.
    pub fn seen(&mut self, i: usize, day: u16) {
        if let Some(r) = self.records[i]
            .as_mut()
            .filter(|r| day != 0 && r.day != day)
        {
            r.day = day;
            self.touch(i);
            self.relisted();
        }
    }

    /// The mailbox moved the record at `i` on: the last item confirmed and
    /// the last fetch taken.
    pub fn progress(&mut self, i: usize, read: u32, last_fetch: u32) {
        if let Some(r) = self.records[i].as_mut() {
            if (r.read, r.last_fetch) != (read, last_fetch) {
                r.read = read;
                r.last_fetch = last_fetch;
                self.dirty.progress |= 1 << i;
            }
        }
    }

    /// Make the node private, or public.
    pub fn set_private(&mut self, private: bool) {
        let private = private && self.paired() > 0;
        if self.private != private {
            self.private = private;
            self.dirty.settings = true;
        }
    }

    fn free_if_empty(&mut self, i: usize) {
        if self.records[i].is_some_and(|r| !r.holds()) {
            self.records[i] = None;
        }
        // With the last bond gone the node is public.
        self.set_private(self.private);
    }

    /// The owner's mailbox key: that of the records with the mark.
    pub fn owner(&self) -> Option<[u8; KEY_LEN]> {
        self.records
            .iter()
            .flatten()
            .find(|r| r.owner)
            .and_then(|r| r.key.home())
    }

    /// Whether the record at `i` is the owner's.
    pub fn is_owner(&self, i: usize) -> bool {
        self.get(i).is_some_and(|r| r.owner)
    }

    /// The mark on every record that holds `key`, and on no other.
    fn mark(&mut self, key: Option<[u8; KEY_LEN]>) {
        for i in 0..RECORDS {
            let Some(r) = self.records[i].as_mut() else {
                continue;
            };
            let owner = key.is_some() && r.key.home() == key;
            if r.owner != owner {
                r.owner = owner;
                self.touch(i);
                self.relisted();
            }
        }
    }

    /// After a change to the records' keys: the mark stays with the key
    /// `was`, the owner's before, while a record holds it, and else goes
    /// to `seat`, a key that takes an empty seat (spec "The node's owner").
    fn keep_mark(&mut self, was: Option<[u8; KEY_LEN]>, seat: Option<[u8; KEY_LEN]>) {
        let held = |k: &[u8; KEY_LEN]| {
            self.records
                .iter()
                .flatten()
                .any(|r| r.key.home() == Some(*k))
        };
        let key = was.filter(held).or(seat.filter(held));
        self.mark(key);
    }

    /// The phone at `i` unpairs: its bond goes, its home stays
    /// (spec "Public nodes").
    pub fn unpair(&mut self, i: usize) -> Option<Keys> {
        let r = self.records[i].as_mut()?;
        let bond = r.bond.take();
        self.touch(i);
        self.free_if_empty(i);
        if bond.is_some() {
            self.relisted();
        }
        bond
    }

    /// The phone at `i` writes Home: its person's key, or none; returns the
    /// place that holds the phone's record after it. A home left by this
    /// person's earlier phone, a home or removed, takes this phone's bond
    /// and keeps its place, whose bits in the ring's marks are their
    /// unfetched mail (spec "Settings"). A key set takes the owner's seat if it is empty
    /// (spec "The node's owner"). `sent` is the phone's as it writes: the
    /// record's last fetch is at least it, so that no fetch of the key from
    /// before is taken; it is never lowered.
    pub fn set_home(&mut self, i: usize, key: Option<[u8; KEY_LEN]>, sent: u32) -> usize {
        let owner = self.owner();
        let at = self.set_key(i, key, sent);
        self.keep_mark(owner, key);
        at
    }

    fn set_key(&mut self, i: usize, key: Option<[u8; KEY_LEN]>, sent: u32) -> usize {
        let Some(r) = self.records[i].as_mut() else {
            return i;
        };
        let start = if key.is_some() { sent } else { 0 };
        let new = key.map_or(HomeKey::None, HomeKey::Home);
        if r.key != new {
            // A removed home set again is a home again, from the later of
            // its last fetch and this write's `sent`.
            let revived = key.is_some_and(|k| r.key == HomeKey::Removed(k));
            let home = r.key.home().is_some() != key.is_some();
            r.last_fetch = if revived {
                r.last_fetch.max(start)
            } else {
                start
            };
            r.key = new;
            self.touch(i);
            if home {
                self.relisted();
            }
        } else if start > r.last_fetch {
            r.last_fetch = start;
            self.dirty.progress |= 1 << i;
        }
        let Some(k) = key else {
            return i;
        };
        let left = (0..RECORDS).find(|&j| {
            j != i && self.records[j].is_some_and(|o| o.bond.is_none() && o.key.holds(k))
        });
        let Some(j) = left else {
            return i;
        };
        let phone = self.records[i].take().expect("it was there");
        let home = self.records[j].as_mut().expect("it was there");
        home.bond = phone.bond;
        home.name = phone.name;
        home.tag = phone.tag.or(home.tag);
        home.day = phone.day.max(home.day);
        home.last_fetch = home.last_fetch.max(sent);
        home.key = new;
        self.touch(i);
        self.touch(j);
        self.relisted();
        j
    }

    /// The place of the record numbered `number` if it holds every part
    /// in `parts`.
    pub fn holding(&self, number: u16, parts: Parts) -> Option<usize> {
        let i = self
            .records
            .iter()
            .position(|r| r.is_some_and(|r| r.number == number))?;
        let r = self.records[i].as_ref().expect("found");
        let lacks = (parts.bond && r.bond.is_none()) || (parts.home && r.key.home().is_none());
        (!lacks).then_some(i)
    }

    /// Remove `parts` of the record numbered `number` (spec "Phones"):
    /// `None` when no record has the number or it does not hold a part
    /// asked for. A home removed keeps its key and its last fetch, marked
    /// removed, so that a later fetch under the key is told so.
    pub fn remove(&mut self, number: u16, parts: Parts) -> Option<Removed> {
        let i = self.holding(number, parts)?;
        let owner = self.owner();
        let r = self.records[i].as_mut().expect("found");
        let bond = match parts.bond {
            true => r.bond.take(),
            false => None,
        };
        if let (true, HomeKey::Home(k)) = (parts.home, r.key) {
            r.key = HomeKey::Removed(k);
        }
        self.touch(i);
        self.free_if_empty(i);
        self.relisted();
        self.keep_mark(owner, None);
        Some(Removed { index: i, bond })
    }

    /// The owner adds a person by their mailbox key and their tag at the
    /// node: a record with a home and no bond, made on day `day` with the
    /// ring's newest item `newest`, its last fetch `sent`, the add's own,
    /// so that no fetch of the key from before is taken (spec "Mailbox",
    /// The owner's commands). A record that holds the key removed is a
    /// home again instead, its last fetch the later of its own and
    /// `sent`. Its place.
    pub fn add(
        &mut self,
        key: [u8; KEY_LEN],
        tag: u8,
        name: Name,
        newest: u32,
        day: u16,
        sent: u32,
    ) -> Result<usize, NotAdded> {
        let held = |k: HomeKey| (0..RECORDS).find(|&i| self.records[i].is_some_and(|r| r.key == k));
        if held(HomeKey::Home(key)).is_some() {
            return Err(NotAdded::Already);
        }
        if let Some(i) = held(HomeKey::Removed(key)) {
            let r = self.records[i].as_mut().expect("found");
            r.key = HomeKey::Home(key);
            r.tag = Some(tag);
            r.name = name;
            r.day = r.day.max(day);
            r.last_fetch = r.last_fetch.max(sent);
            self.touch(i);
            self.relisted();
            return Ok(i);
        }
        let i = self.free().ok_or(NotAdded::Full)?;
        let number = self.number();
        self.records[i] = Some(Record {
            name,
            tag: Some(tag),
            key: HomeKey::Home(key),
            day,
            last_fetch: sent,
            ..Record::new(number, None, newest)
        });
        self.touch(i);
        self.relisted();
        Ok(i)
    }

    /// The key of the record numbered `number`, which holds a home, is
    /// the owner's from now (spec "The node's owner", "Phones"). Its
    /// place; `None` when no record of that number holds a home.
    pub fn make_owner(&mut self, number: u16) -> Option<usize> {
        let i = self.holding(
            number,
            Parts {
                bond: false,
                home: true,
            },
        )?;
        self.mark(self.records[i].and_then(|r| r.key.home()));
        Some(i)
    }

    /// The person of the record at `i` leaves: every record that holds
    /// their key, a home or removed, forgets it, and one with no bond is
    /// freed (spec "The node's owner").
    pub fn leave(&mut self, i: usize) {
        let Some(key) = self.get(i).and_then(|r| r.key.home()) else {
            return;
        };
        let owner = self.owner();
        for j in 0..RECORDS {
            if let Some(r) = self.records[j].as_mut().filter(|r| r.key.holds(key)) {
                r.key = HomeKey::None;
                r.last_fetch = 0;
                self.touch(j);
                self.free_if_empty(j);
            }
        }
        self.relisted();
        self.keep_mark(owner, None);
    }

    /// What has changed since the last call, for the store to write.
    pub fn take_dirty(&mut self) -> Dirty {
        core::mem::take(&mut self.dirty)
    }

    /// The settings entry as it stands.
    pub fn settings(&self) -> Settings {
        Settings {
            private: self.private,
            next: self.next,
        }
    }

    /// The progress entry of the record at `i`.
    pub fn progress_of(&self, i: usize) -> Option<Progress> {
        self.get(i).map(|r| Progress {
            index: i,
            number: r.number,
            read: r.read,
            last_fetch: r.last_fetch,
        })
    }

    /// The records a person sees, with their places: all but those that
    /// hold only a removed home.
    fn shown(&self) -> impl Iterator<Item = (usize, &Record)> {
        self.records
            .iter()
            .enumerate()
            .filter_map(|(i, r)| Some((i, r.as_ref().filter(|r| !r.only_removed())?)))
    }

    /// Phones, read, into `out`; returns its length. `reader` is the
    /// reading phone's place and `linked` a bit for each place whose phone
    /// is linked now.
    pub fn page(&self, from: u8, reader: Option<usize>, linked: u8, out: &mut [u8]) -> usize {
        let mut len = 1;
        out[0] = 0;
        for (i, r) in self.shown().filter(|&(i, _)| i >= usize::from(from)) {
            let name = r.name.as_bytes();
            if len + PER_RECORD + name.len() > out.len() {
                out[0] = i as u8;
                break;
            }
            len += put(i, r, name, reader, linked, &mut out[len..]);
        }
        len
    }

    /// Every record, as a Phones read gives them, into `out`, whatever
    /// its length: the owner's list, which a mailbox's summary carries
    /// (spec "Mailbox"). The names are cut, at a character's end, to the
    /// most bytes at which every record fits, so only as far as needed.
    /// Its length.
    pub fn list(&self, reader: usize, linked: u8, out: &mut [u8]) -> usize {
        let names = || self.shown().map(|(_, r)| r.name.as_bytes().len());
        let fits = |most: usize| -> bool {
            names().map(|n| PER_RECORD + n.min(most)).sum::<usize>() <= out.len()
        };
        let most = (0..=NAME_MAX).rev().find(|&m| fits(m)).unwrap_or(0);
        let mut len = 0;
        for (i, r) in self.shown() {
            let name = cut(r.name.as_bytes(), most);
            if len + PER_RECORD + name.len() > out.len() {
                break;
            }
            len += put(i, r, name, Some(reader), linked, &mut out[len..]);
        }
        len
    }
}

/// `name` cut to at most `most` bytes at a character's end: UTF-8 never
/// starts a character with `0b10`.
fn cut(name: &[u8], most: usize) -> &[u8] {
    let mut n = name.len().min(most);
    while n > 0 && n < name.len() && name[n] & 0xc0 == 0x80 {
        n -= 1;
    }
    &name[..n]
}

/// The record at `i` as a Phones read gives it, its name `name`, into
/// `b`; its length.
fn put(
    i: usize,
    r: &Record,
    name: &[u8],
    reader: Option<usize>,
    linked: u8,
    b: &mut [u8],
) -> usize {
    let flag = |set: bool, bit: u8| if set { bit } else { 0 };
    let b = &mut b[..PER_RECORD + name.len()];
    b[..2].copy_from_slice(&r.number.to_be_bytes());
    b[2] = flag(r.bond.is_some(), 0x01)
        | flag(r.key.home().is_some(), 0x02)
        | flag(linked & (1 << i) != 0, 0x04)
        | flag(reader == Some(i), 0x08)
        | flag(r.tag.is_some(), 0x10)
        | flag(r.owner, 0x20);
    b[3..5].copy_from_slice(&r.day.to_be_bytes());
    b[5] = r.tag.unwrap_or(0);
    b[6] = name.len() as u8;
    b[PER_RECORD..].copy_from_slice(name);
    b.len()
}

/// Bytes of a record in a Phones read before its name.
pub const PER_RECORD: usize = 2 + 1 + 2 + 1 + 1;

/// Bytes of a Phones read at most: as many as fill one value at the MTU the
/// node offers.
pub const MAX_LEN: usize = crate::value_room(crate::ATT_MTU);

// Every record fits a read, and eight take two at most.
const _: () = assert!(RECORDS.div_ceil((MAX_LEN - 1) / (PER_RECORD + NAME_MAX)) <= 2);

/// A record as a Phones read gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Listed<'a> {
    /// Its number.
    pub number: u16,
    /// It holds a bond.
    pub paired: bool,
    /// The node is its person's home.
    pub home: bool,
    /// Its phone is linked now.
    pub linked: bool,
    /// It is the reading phone's.
    pub mine: bool,
    /// It is the owner's.
    pub owner: bool,
    /// Its person's tag at the node, if known.
    pub tag: Option<u8>,
    /// The day its person was last seen.
    pub day: u16,
    /// Its person's name.
    pub name: &'a [u8],
}

/// A Phones read, for the app's side and the tests: the place to write
/// next, and the records it lists into `out`, their count.
pub fn decode_page<'a>(
    b: &'a [u8],
    out: &mut [Option<Listed<'a>>; RECORDS],
) -> Result<(u8, usize), Malformed> {
    let (&next, rest) = b.split_first().ok_or(Malformed::Length)?;
    Ok((next, decode_list(rest, out)?))
}

/// The records of a Phones read after its first byte, or of the owner's
/// list ([`Phones::list`]), into `out`; their count.
pub fn decode_list<'a>(
    mut rest: &'a [u8],
    out: &mut [Option<Listed<'a>>; RECORDS],
) -> Result<usize, Malformed> {
    let mut n = 0;
    while !rest.is_empty() {
        let name_len = usize::from(*rest.get(6).ok_or(Malformed::Length)?);
        let end = PER_RECORD + name_len;
        if name_len > NAME_MAX || rest.len() < end || n == RECORDS {
            return Err(Malformed::Length);
        }
        let flags = rest[2];
        out[n] = Some(Listed {
            number: u16::from_be_bytes([rest[0], rest[1]]),
            paired: flags & 0x01 != 0,
            home: flags & 0x02 != 0,
            linked: flags & 0x04 != 0,
            mine: flags & 0x08 != 0,
            owner: flags & 0x20 != 0,
            tag: (flags & 0x10 != 0).then_some(rest[5]),
            day: u16::from_be_bytes([rest[3], rest[4]]),
            name: &rest[PER_RECORD..end],
        });
        n += 1;
        rest = &rest[end..];
    }
    Ok(n)
}

/// Phones, written (spec "Phones").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhonesWrite {
    /// Read from this place on.
    From(u8),
    /// Draw a removal code on the screen.
    Code,
    /// Remove parts of the record with this number, with the code drawn.
    Remove {
        /// The record's number.
        number: u16,
        /// What to remove.
        parts: Parts,
        /// The code, as a number.
        code: u32,
    },
    /// Make the key of the record with this number the owner's, with the
    /// code drawn.
    Owner {
        /// The record's number.
        number: u16,
        /// The code, as a number.
        code: u32,
    },
}

/// A removal code's numbers: six digits.
pub const CODES: u32 = 1_000_000;

impl PhonesWrite {
    const FROM: u8 = 0;
    const CODE: u8 = 1;
    const REMOVE: u8 = 2;
    /// The `what` of a write of [`Self::REMOVE`] that makes an owner.
    const OWNER: u8 = 4;
    /// Bytes in the longest value.
    pub const MAX_LEN: usize = 1 + 2 + 1 + 3;

    /// The value, into `b`; its length.
    pub fn encode(&self, b: &mut [u8; Self::MAX_LEN]) -> usize {
        match *self {
            PhonesWrite::From(s) => {
                b[..2].copy_from_slice(&[Self::FROM, s]);
                2
            }
            PhonesWrite::Code => {
                b[0] = Self::CODE;
                1
            }
            PhonesWrite::Remove {
                number,
                parts,
                code,
            } => {
                b[0] = Self::REMOVE;
                b[1..3].copy_from_slice(&number.to_be_bytes());
                b[3] = u8::from(parts.bond) | u8::from(parts.home) << 1;
                put24(&mut b[4..], code);
                Self::MAX_LEN
            }
            PhonesWrite::Owner { number, code } => {
                b[0] = Self::REMOVE;
                b[1..3].copy_from_slice(&number.to_be_bytes());
                b[3] = Self::OWNER;
                put24(&mut b[4..], code);
                Self::MAX_LEN
            }
        }
    }

    /// Take a written value whole.
    pub fn decode(b: &[u8]) -> Result<PhonesWrite, Malformed> {
        match (b.first(), b.len()) {
            (Some(&Self::FROM), 2) if usize::from(b[1]) < RECORDS => Ok(PhonesWrite::From(b[1])),
            (Some(&Self::CODE), 1) => Ok(PhonesWrite::Code),
            (Some(&Self::REMOVE), Self::MAX_LEN) => {
                let code = be24(&b[4..]);
                let number = u16::from_be_bytes([b[1], b[2]]);
                match b[3] {
                    _ if code >= CODES => Err(Malformed::Value),
                    Self::OWNER => Ok(PhonesWrite::Owner { number, code }),
                    what @ 1..=3 => Ok(PhonesWrite::Remove {
                        number,
                        parts: Parts {
                            bond: what & 1 != 0,
                            home: what & 2 != 0,
                        },
                        code,
                    }),
                    _ => Err(Malformed::Value),
                }
            }
            (Some(&Self::FROM), 2) => Err(Malformed::Value),
            (Some(&(Self::FROM | Self::CODE | Self::REMOVE)), _) | (None, _) => {
                Err(Malformed::Length)
            }
            (Some(_), _) => Err(Malformed::Value),
        }
    }
}

/// How long a removal code is taken after it is drawn: the Security
/// Manager's timeout for a pairing step (Bluetooth Core Vol 3 Part H §3.4).
pub const CODE_MS: u64 = 30_000;

/// A removal code on the screen, and when it was drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Code {
    /// The six digits, as a number.
    pub code: u32,
    /// When it was drawn, ms.
    pub at_ms: u64,
}

impl Code {
    /// A code of a random word, uniform over the six digits: `None` for a
    /// word past the last whole run of [`CODES`], to draw again.
    pub fn draw(random: u32, at_ms: u64) -> Option<Code> {
        const WHOLE: u32 = u32::MAX - u32::MAX % CODES;
        (random < WHOLE).then_some(Code {
            code: random % CODES,
            at_ms,
        })
    }

    /// Whether the code shown, `drawn`, takes `code` at `now_ms`: one try a
    /// code, so it is forgotten whatever the answer.
    pub fn take(drawn: &mut Option<Code>, code: u32, now_ms: u64) -> bool {
        drawn
            .take()
            .is_some_and(|d| d.code == code && now_ms < d.at_ms + CODE_MS)
    }

    /// Whether it still shows at `now_ms`.
    pub fn shows(&self, now_ms: u64) -> bool {
        now_ms < self.at_ms + CODE_MS
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    fn keys(n: u8) -> Keys {
        Keys {
            kind: 1,
            addr: [n, 2, 3, 4, 5, 0xc6],
            irk: Some([n; 16]),
            ltk: [n.wrapping_add(1); 16],
            seen: 13,
            service_changed: true,
        }
    }

    fn full(n: u8) -> Record {
        Record {
            number: 300 + u16::from(n),
            bond: Some(keys(n)),
            name: Name::new("Åsa Öberg".as_bytes()).unwrap(),
            tag: Some(n),
            key: HomeKey::Home([n; 32]),
            from: 70_000,
            read: 0x01_0203,
            last_fetch: 0x0405_0607,
            day: 314,
            owner: true,
        }
    }

    #[test]
    fn a_record_round_trips_and_one_cut_short_is_none() {
        for rec in [
            full(9),
            Record {
                bond: None,
                tag: None,
                name: Name::EMPTY,
                ..full(1)
            },
            Record {
                key: HomeKey::None,
                owner: false,
                bond: Some(Keys {
                    irk: None,
                    service_changed: false,
                    ..keys(4)
                }),
                ..full(4)
            },
            Record {
                owner: false,
                ..full(6)
            },
            Record {
                owner: false,
                key: HomeKey::Removed([7; 32]),
                ..full(7)
            },
            Record {
                owner: false,
                key: HomeKey::Removed([8; 32]),
                bond: None,
                ..full(8)
            },
        ] {
            let r = Record::encode(5, Some(&rec));
            assert_eq!(Record::decode(&r), Some((5, Some(rec))));
            // Cut at any word: the last word is erased.
            for words in 0..RECORD_LEN / 4 {
                let mut cut = r;
                cut[words * 4..].fill(0xff);
                assert_eq!(Record::decode(&cut), None);
            }
        }
        // The mark without a home is no record, nor a removal without a
        // key.
        let mut marked = Record::encode(5, Some(&full(5)));
        marked[AT_FLAGS] &= !FLAG_KEY;
        assert_eq!(Record::decode(&seal(marked)), None);
        let mut marked = Record::encode(5, Some(&full(5)));
        marked[AT_FLAGS] |= FLAG_REMOVED;
        assert_eq!(Record::decode(&seal(marked)), None);
        let mut keyless = Record::encode(
            5,
            Some(&Record {
                key: HomeKey::None,
                ..full(5)
            }),
        );
        keyless[AT_FLAGS] = (keyless[AT_FLAGS] & !FLAG_OWNER) | FLAG_REMOVED;
        assert_eq!(Record::decode(&seal(keyless)), None);
        let freed = Record::encode(7, None);
        assert_eq!(Record::decode(&freed), Some((7, None)));
        let mut far = freed;
        far[1] = RECORDS as u8;
        assert_eq!(Record::decode(&seal(far)), None);
        // No four bytes of a record are erased flash's: a name is UTF-8.
        assert!(Record::encode(0, Some(&full(0xff)))
            .chunks(SLOT_LEN)
            .all(|s| s.iter().any(|&b| b != 0xff)));
    }

    #[test]
    fn progress_and_settings_round_trip() {
        let p = Progress {
            index: 3,
            number: 77,
            read: 0xab_cdef,
            last_fetch: 99,
        };
        assert_eq!(Progress::decode(&p.encode()), Some(p));
        let s = Settings {
            private: true,
            next: 513,
        };
        assert_eq!(Settings::decode(&s.encode()), Some(s));
        let mut cut = s.encode();
        cut[28..].fill(0xff);
        assert_eq!(Settings::decode(&cut), None);
        assert_eq!(Progress::decode(&s.encode()), None);
        assert_eq!(entry_len(RECORD_TAG), 128);
        assert_eq!(entry_len(1), 32);
    }

    #[test]
    fn the_last_entry_of_each_holds_and_progress_names_its_record() {
        // A page without settings holds no phones.
        assert_eq!(Load::default().phones(), Phones::EMPTY);
        let mut load = Load::default();
        load.entry(
            &Settings {
                private: false,
                next: 9,
            }
            .encode(),
        );
        load.entry(&Record::encode(2, Some(&full(2))));
        load.entry(&Record::encode(4, Some(&full(4))));
        let moved = Progress {
            index: 2,
            number: full(2).number,
            read: 5,
            last_fetch: 6,
        };
        load.entry(&moved.encode());
        // One of a record since replaced counts for nothing.
        load.entry(
            &Progress {
                index: 4,
                number: 1,
                ..moved
            }
            .encode(),
        );
        load.entry(&Record::encode(4, None));
        load.entry(
            &Settings {
                private: true,
                next: 11,
            }
            .encode(),
        );
        let p = load.phones();
        assert_eq!(p.get(2).map(|r| (r.read, r.last_fetch)), Some((5, 6)));
        assert_eq!(p.get(4), None);
        assert_eq!(
            p.settings(),
            Settings {
                private: true,
                next: 11
            }
        );
        assert!(!p.public());
    }

    #[test]
    fn pairings_add_replace_and_are_refused_when_full() {
        let mut p = Phones::EMPTY;
        assert_eq!(p.room(), RECORDS as u8);
        for n in 0..RECORDS as u8 {
            assert_eq!(
                p.pair(keys(n), 100 + u32::from(n)),
                Ok(Paired::Added(n.into()))
            );
        }
        assert_eq!(p.get(3).map(|r| (r.number, r.from)), Some((4, 103)));
        assert_eq!(p.room(), 0);
        assert!(!p.takes(None));
        assert_eq!(p.pair(keys(9), 0), Err(Full));
        // The same phone pairs again: its keys change, the rest stays.
        p.set_recipient(5, 0x42, Name::new(b"Bo").unwrap());
        let again = Keys {
            ltk: [0xee; 16],
            ..keys(5)
        };
        assert!(p.takes(Some((again.kind, again.addr))));
        assert_eq!(p.pair(again, 999), Ok(Paired::Again(5)));
        let r = p.get(5).unwrap();
        assert_eq!(
            (r.bond, r.tag, r.number, r.from),
            (Some(again), Some(0x42), 6, 105)
        );
        // Unpaired, it keeps its home; with no home it is freed.
        p.set_home(5, Some([1; 32]), 0);
        assert_eq!(p.unpair(5), Some(again));
        assert_eq!(p.get(5).map(|r| r.bond), Some(None));
        p.unpair(6);
        assert_eq!(p.get(6), None);
        assert_eq!(p.room(), 1);
        // A new record takes the next number.
        assert_eq!(p.pair(keys(20), 0), Ok(Paired::Added(6)));
        assert_eq!(p.get(6).map(|r| r.number), Some(9));
    }

    /// Nobody paired and every place a home left behind: the pairing takes
    /// the one linked longest ago, so a node can always be paired again.
    #[test]
    fn a_node_nobody_is_paired_with_takes_the_oldest_home() {
        let mut p = Phones::EMPTY;
        for n in 0..RECORDS as u8 {
            p.pair(keys(n), 0).unwrap();
            p.seen(n.into(), [50, 40, 10, 30, 20, 60, 70, 80][usize::from(n)]);
            p.set_home(n.into(), Some([n; 32]), 0);
        }
        p.set_private(true);
        for n in 0..RECORDS {
            p.unpair(n);
        }
        assert_eq!(p.paired(), 0);
        assert!(p.public(), "nobody paired, the node is public");
        assert_eq!(p.room(), 1);
        assert_eq!(p.pair(keys(30), 5), Ok(Paired::Took(2)));
        let r = p.get(2).unwrap();
        assert_eq!((r.key, r.from, r.number), (HomeKey::None, 5, 9));
        assert_eq!(p.room(), 0);
    }

    #[test]
    fn the_day_is_written_when_it_changes() {
        let mut p = Phones::EMPTY;
        p.pair(keys(1), 0).unwrap();
        p.take_dirty();
        p.seen(0, 0);
        p.seen(0, 300);
        assert_eq!(p.take_dirty().records, 1);
        p.seen(0, 300);
        p.seen(0, 0);
        assert_eq!(p.take_dirty(), Dirty::default());
        assert_eq!(p.get(0).map(|r| r.day), Some(300));
        p.progress(0, 7, 8);
        let d = p.take_dirty();
        assert_eq!((d.records, d.progress), (0, 1));
        assert_eq!(
            p.progress_of(0),
            Some(Progress {
                index: 0,
                number: 1,
                read: 7,
                last_fetch: 8
            })
        );
    }

    /// A phone that makes the node its person's home finds the home their
    /// earlier phone left: one record, in the home's place.
    #[test]
    fn home_joins_a_home_left_behind() {
        let mut p = Phones::EMPTY;
        let k = [9; 32];
        p.pair(keys(1), 0).unwrap();
        p.set_home(0, Some(k), 0);
        p.progress(0, 30, 31);
        p.unpair(0);
        p.pair(keys(2), 50).unwrap();
        p.set_recipient(1, 0x11, Name::new(b"Eva").unwrap());
        p.seen(1, 400);
        assert_eq!(p.set_home(1, Some(k), 0), 0);
        assert_eq!(p.get(1), None);
        let r = p.get(0).unwrap();
        assert_eq!(
            (r.bond, r.tag, r.key),
            (Some(keys(2)), Some(0x11), HomeKey::Home(k))
        );
        assert_eq!((r.from, r.read, r.last_fetch, r.day), (0, 30, 31, 400));
        assert_eq!(r.name.as_bytes(), b"Eva");
        // Clearing forgets the key and the last fetch, the record stays.
        assert_eq!(p.set_home(0, None, 0), 0);
        assert_eq!(
            p.get(0).map(|r| (r.key, r.last_fetch)),
            Some((HomeKey::None, 0))
        );
    }

    #[test]
    fn a_removal_takes_what_the_record_holds() {
        let mut p = Phones::EMPTY;
        p.pair(keys(1), 0).unwrap();
        p.pair(keys(2), 0).unwrap();
        p.set_home(1, Some([2; 32]), 0);
        let both = Parts {
            bond: true,
            home: true,
        };
        let bond = Parts {
            bond: true,
            home: false,
        };
        let home = Parts {
            bond: false,
            home: true,
        };
        assert_eq!(p.remove(9, bond), None);
        assert_eq!(p.remove(1, both), None, "record 1 holds no home");
        assert_eq!(
            p.remove(2, home),
            Some(Removed {
                index: 1,
                bond: None
            })
        );
        // The key stays, removed, with its last fetch.
        assert_eq!(p.get(1).map(|r| r.key), Some(HomeKey::Removed([2; 32])));
        assert_eq!(p.remove(2, home), None, "removed already");
        p.set_private(true);
        assert_eq!(
            p.remove(2, bond),
            Some(Removed {
                index: 1,
                bond: Some(keys(2))
            })
        );
        assert!(p.get(1).is_some_and(|r| r.only_removed()));
        assert!(!p.public());
        assert_eq!(
            p.remove(1, bond),
            Some(Removed {
                index: 0,
                bond: Some(keys(1))
            })
        );
        assert!(p.public(), "the last bond gone, the node is public");
        assert_eq!(p.room(), RECORDS as u8);
    }

    #[test]
    fn eight_records_take_two_reads() {
        let mut p = Phones::EMPTY;
        for n in 0..RECORDS as u8 {
            p.pair(keys(n), 0).unwrap();
            p.set_recipient(n.into(), n, Name::new(&[b'a' + n; NAME_MAX]).unwrap());
        }
        p.set_home(2, Some([1; 32]), 0);
        p.unpair(2);
        p.seen(3, 9);
        let mut out = [0u8; MAX_LEN];
        let mut listed = [None; RECORDS];
        let len = p.page(0, Some(1), 0b1000_0001, &mut out);
        let (next, n) = decode_page(&out[..len], &mut listed).unwrap();
        assert_eq!((next, n), (6, 6));
        let first = listed[0].unwrap();
        assert_eq!(
            (
                first.number,
                first.paired,
                first.home,
                first.linked,
                first.mine,
                first.tag
            ),
            (1, true, false, true, false, Some(0))
        );
        assert!(listed[1].unwrap().mine);
        let home_left = listed[2].unwrap();
        assert_eq!((home_left.paired, home_left.home), (false, true));
        assert_eq!(
            (listed[3].unwrap().day, listed[3].unwrap().name),
            (9, &[b'd'; 32][..])
        );
        let mut out = [0u8; MAX_LEN];
        let mut listed = [None; RECORDS];
        let len = p.page(next, None, 0b1000_0001, &mut out);
        let (next, n) = decode_page(&out[..len], &mut listed).unwrap();
        assert_eq!((next, n), (0, 2));
        assert_eq!(listed[1].unwrap().number, 8);
        assert!(listed[1].unwrap().linked);
        let mut again = [None; RECORDS];
        assert_eq!(
            decode_page(&out[..len - 1], &mut again),
            Err(Malformed::Length)
        );
        // An empty node lists nothing.
        assert_eq!(Phones::EMPTY.page(0, None, 0, &mut [0u8; MAX_LEN]), 1);
    }

    /// Every read of Phones, place 0 first, as one run of bytes.
    fn every_page(p: &Phones) -> std::vec::Vec<u8> {
        let mut all = std::vec::Vec::new();
        let mut from = 0;
        loop {
            let mut out = [0u8; MAX_LEN];
            let len = p.page(from, None, 0, &mut out);
            all.extend_from_slice(&out[1..len]);
            from = out[0];
            if from == 0 {
                return all;
            }
        }
    }

    /// `listed` moves at each write that changes what Phones lists, and at
    /// no other (spec "Phone link", Status).
    #[test]
    fn listed_moves_with_what_phones_lists() {
        fn step(p: &mut Phones, what: &str, lists: bool, write: impl Fn(&mut Phones)) {
            let (bytes, listed) = (every_page(p), p.listed());
            write(p);
            assert_eq!(every_page(p) != bytes, lists, "{what}: the list");
            assert_eq!(p.listed() != listed, lists, "{what}: listed");
        }
        let mut p = Phones::EMPTY;
        let bo = Name::new(b"Bo").unwrap();
        step(&mut p, "pair", true, |p| {
            assert!(p.pair(keys(1), 0).is_ok())
        });
        step(&mut p, "a second", true, |p| {
            assert!(p.pair(keys(2), 0).is_ok())
        });
        step(&mut p, "again", false, |p| {
            assert!(p.pair(keys(2), 0).is_ok())
        });
        step(&mut p, "keys", false, |p| {
            p.set_keys(0, Keys { seen: 1, ..keys(1) })
        });
        step(&mut p, "tag", true, |p| p.set_recipient(1, 4, Name::EMPTY));
        step(&mut p, "same tag", false, |p| {
            p.set_recipient(1, 4, Name::EMPTY)
        });
        step(&mut p, "name", true, |p| p.set_recipient(1, 4, bo));
        step(&mut p, "day", true, |p| p.seen(1, 300));
        step(&mut p, "same day", false, |p| p.seen(1, 300));
        step(&mut p, "progress", false, |p| p.progress(1, 9, 9));
        step(&mut p, "private", false, |p| p.set_private(true));
        step(&mut p, "home", true, |p| {
            assert_eq!(p.set_home(1, Some([7; 32]), 0), 1)
        });
        step(&mut p, "new key", false, |p| {
            assert_eq!(p.set_home(1, Some([8; 32]), 0), 1)
        });
        step(&mut p, "unpair", true, |p| assert!(p.unpair(1).is_some()));
        step(&mut p, "join", true, |p| {
            assert_eq!(p.set_home(0, Some([8; 32]), 0), 1)
        });
        step(&mut p, "no home", true, |p| {
            assert_eq!(p.set_home(1, None, 0), 1)
        });
        let number = p.get(1).unwrap().number;
        let bond = Parts {
            bond: true,
            home: false,
        };
        step(&mut p, "remove", true, |p| {
            assert!(p.remove(number, bond).is_some())
        });
        step(&mut p, "add", true, |p| {
            assert!(p.add([3; 32], 3, Name::EMPTY, 0, 0, 0).is_ok())
        });
        let at = (0..RECORDS)
            .find(|&i| p.get(i).is_some_and(|r| r.key == HomeKey::Home([3; 32])))
            .unwrap();
        let added = p.get(at).unwrap().number;
        step(&mut p, "add again", false, |p| {
            assert!(p.add([3; 32], 3, Name::EMPTY, 0, 0, 0).is_err())
        });
        step(&mut p, "owner", true, |p| {
            assert!(p.make_owner(added).is_some())
        });
        step(&mut p, "owner again", false, |p| {
            assert!(p.make_owner(added).is_some())
        });
        step(&mut p, "seen", true, |p| p.seen(at, 400));
        step(&mut p, "leave", true, |p| p.leave(at));
    }

    /// The mark is the key's: a Home write takes the empty seat, every
    /// record holding the owner's key has it, and it goes with the last
    /// of them (spec "The node's owner").
    #[test]
    fn the_owner_s_mark_follows_the_key() {
        let mut p = Phones::EMPTY;
        let (ann, bo) = ([1; 32], [2; 32]);
        for n in 0..3 {
            p.pair(keys(n), 0).unwrap();
        }
        assert_eq!(p.owner(), None);
        p.set_home(0, Some(ann), 0);
        assert_eq!(p.owner(), Some(ann));
        p.set_home(1, Some(bo), 0);
        assert!(p.is_owner(0) && !p.is_owner(1), "the seat is taken");
        // Ann's second phone holds her key, and so the mark.
        p.set_home(2, Some(ann), 0);
        assert!(p.is_owner(2));
        p.set_home(0, None, 0);
        assert!(!p.is_owner(0) && p.is_owner(2));
        // Her last home gone, the seat is empty, and Bo's own key, written
        // again, takes it.
        let home = Parts {
            bond: false,
            home: true,
        };
        p.remove(p.get(2).unwrap().number, home).unwrap();
        assert_eq!(p.owner(), None);
        p.set_home(1, Some(bo), 0);
        assert_eq!(p.owner(), Some(bo));
        // Kept in the record, and in the page's flags.
        let r = Record::encode(1, p.get(1));
        assert_eq!(
            Record::decode(&r).map(|(_, r)| r.unwrap().owner),
            Some(true)
        );
        let mut out = [0u8; MAX_LEN];
        let mut listed = [None; RECORDS];
        let len = p.page(0, None, 0, &mut out);
        decode_page(&out[..len], &mut listed).unwrap();
        let owners: std::vec::Vec<_> = listed.iter().flatten().map(|l| l.owner).collect();
        assert_eq!(owners, [false, true, false]);
    }

    /// The owner adds a person with a home and no bond, hands the mark to
    /// another, and a person leaves (spec "The node's owner").
    #[test]
    fn an_owner_adds_hands_over_and_a_person_leaves() {
        let mut p = Phones::EMPTY;
        p.pair(keys(1), 0).unwrap();
        p.set_home(0, Some([1; 32]), 0);
        let eva = Name::new(b"Eva").unwrap();
        assert_eq!(p.add([5; 32], 0x55, eva, 40, 300, 0), Ok(1));
        let r = *p.get(1).unwrap();
        assert_eq!(
            (r.bond, r.key, r.tag, r.from, r.day, r.owner),
            (None, HomeKey::Home([5; 32]), Some(0x55), 40, 300, false)
        );
        assert_eq!(r.name, eva);
        // The same key again changes nothing, full or not.
        assert_eq!(
            p.add([5; 32], 0x56, Name::EMPTY, 41, 301, 0),
            Err(NotAdded::Already)
        );
        assert_eq!(p.get(1), Some(&r));
        for k in 6..12 {
            assert!(p.add([k; 32], k, Name::EMPTY, 0, 0, 0).is_ok());
        }
        assert_eq!(
            p.add([12; 32], 1, Name::EMPTY, 0, 0, 0),
            Err(NotAdded::Full)
        );
        assert_eq!(
            p.add([5; 32], 1, Name::EMPTY, 0, 0, 0),
            Err(NotAdded::Already)
        );
        // Handed over: the old owner stays homed and loses the mark.
        assert_eq!(p.make_owner(r.number), Some(1));
        assert_eq!(p.owner(), Some([5; 32]));
        assert_eq!(
            p.get(0).map(|r| (r.key, r.owner)),
            Some((HomeKey::Home([1; 32]), false))
        );
        // A record without a home cannot take it.
        p.set_home(0, None, 0);
        assert_eq!(p.make_owner(p.get(0).unwrap().number), None);
        assert_eq!(p.make_owner(999), None);
        // Eva leaves: her record, which holds no bond, is freed, and the
        // seat is empty.
        p.leave(1);
        assert_eq!(p.get(1), None);
        assert_eq!(p.owner(), None);
        // A person with a phone who leaves keeps the bond.
        p.set_home(0, Some([1; 32]), 0);
        assert!(p.is_owner(0));
        p.leave(0);
        assert_eq!(
            p.get(0).map(|r| (r.bond.is_some(), r.key, r.owner)),
            Some((true, HomeKey::None, false))
        );
    }

    /// A record the owner added has no phone, and is no more the first
    /// to go than one a phone left: the pairing takes the home whose
    /// person was seen longest ago, the owner's last (spec "Pairing").
    #[test]
    fn a_pairing_on_a_full_node_spares_the_owner_and_the_recently_seen() {
        let mut p = Phones::EMPTY;
        p.pair(keys(0), 0).unwrap();
        p.seen(0, 5);
        p.set_home(0, Some([0; 32]), 0);
        for k in 1..RECORDS as u8 {
            p.add([k; 32], k, Name::EMPTY, 0, 100 + u16::from(k), 0)
                .unwrap();
        }
        // Record 3's person fetched on day 400.
        p.seen(3, 400);
        p.unpair(0);
        assert_eq!(p.paired(), 0);
        assert_eq!(p.pair(keys(9), 0), Ok(Paired::Took(1)));
        assert_eq!(p.owner(), Some([0; 32]), "the owner's home stays");
        // Each phone that pairs, is seen, makes the node its home and
        // unpairs leaves the next to the one seen longest ago.
        let mut took = std::vec::Vec::new();
        let mut at = 1;
        for n in 10..16 {
            p.seen(at, 500);
            p.set_home(at, Some([n; 32]), 0);
            p.unpair(at);
            at = p.pair(keys(n), 0).unwrap().index();
            took.push(at);
        }
        assert_eq!(took, [2, 4, 5, 6, 7, 3]);
        assert!(p.get(0).is_some_and(|r| r.owner));
    }

    /// The owner's list holds every record, cutting the names only as far
    /// as all of them need to fit, at a character's end.
    #[test]
    fn the_list_cuts_names_only_as_far_as_needed() {
        /// The names the list holds in `room` bytes.
        fn names(p: &Phones, room: usize) -> std::vec::Vec<std::vec::Vec<u8>> {
            let mut out = [0u8; 256];
            let mut listed = [None; RECORDS];
            let n = p.list(0, 1, &mut out[..room]);
            let count = decode_list(&out[..n], &mut listed).unwrap();
            listed[..count]
                .iter()
                .map(|l| l.unwrap().name.to_vec())
                .collect()
        }
        let mut p = Phones::EMPTY;
        p.pair(keys(0), 0).unwrap();
        p.set_home(0, Some([0; 32]), 0);
        // Two bytes a letter, but for the space: 31 bytes.
        let long = "Олександра Ковал".as_bytes();
        assert_eq!(long.len(), 31);
        p.set_recipient(0, 1, Name::new(long).unwrap());
        let mut out = [0u8; 200];
        let mut listed = [None; RECORDS];
        let n = p.list(0, 1, &mut out);
        assert_eq!(decode_list(&out[..n], &mut listed), Ok(1));
        let me = listed[0].unwrap();
        assert_eq!(
            (me.name, me.mine, me.owner, me.linked),
            (long, true, true, true)
        );
        for k in 1..RECORDS as u8 {
            p.add([k; 32], k, Name::new(long).unwrap(), 0, 0, 0)
                .unwrap();
        }
        // 200 bytes: 25 a record, 18 of a name, a character's end.
        assert!(names(&p, 200).iter().all(|n| n == &long[..18]));
        assert_eq!(names(&p, 200).len(), RECORDS);
        // One byte less: 17 would split a letter, so 16.
        assert!(names(&p, 199).iter().all(|n| n == &long[..16]));
        // A short name stays whole, and the long ones take its room.
        p.set_recipient(0, 1, Name::new(b"Bo").unwrap());
        let got = names(&p, 200);
        assert_eq!((&got[0][..], &got[1][..]), (&b"Bo"[..], &long[..20]));
    }

    /// A power cut between the records a hand-over writes leaves the mark
    /// on two keys: the first record's holds it.
    #[test]
    fn a_mark_cut_short_goes_with_the_first_key() {
        let mut load = Load::default();
        load.entry(
            &Settings {
                private: false,
                next: 9,
            }
            .encode(),
        );
        let one = Record {
            key: HomeKey::Home([1; 32]),
            ..full(1)
        };
        let two = Record {
            key: HomeKey::Home([2; 32]),
            ..full(2)
        };
        load.entry(&Record::encode(3, Some(&two)));
        load.entry(&Record::encode(1, Some(&one)));
        load.entry(&Record::encode(
            5,
            Some(&Record {
                owner: false,
                ..one
            }),
        ));
        let p = load.phones();
        assert_eq!(p.owner(), Some([1; 32]));
        assert!(p.is_owner(1) && p.is_owner(5) && !p.is_owner(3));
    }

    /// A key set starts its record's last fetch at the write's `sent`, or
    /// the add's, and a write of a key held already never lowers it, so
    /// no fetch of the key from before is taken (spec "Settings", Home).
    #[test]
    fn a_key_set_starts_at_its_sent_and_never_goes_back() {
        let mut p = Phones::EMPTY;
        let k = [4; 32];
        p.pair(keys(1), 0).unwrap();
        p.set_home(0, Some(k), 500);
        assert_eq!(p.get(0).map(|r| r.last_fetch), Some(500));
        p.progress(0, 0, 700);
        p.take_dirty();
        // Again, earlier: nothing moves.
        p.set_home(0, Some(k), 600);
        assert_eq!(p.get(0).map(|r| r.last_fetch), Some(700));
        assert_eq!(p.take_dirty(), Dirty::default());
        // Again, later: it moves, and is kept as progress.
        p.set_home(0, Some(k), 900);
        assert_eq!(p.get(0).map(|r| r.last_fetch), Some(900));
        assert_eq!(p.take_dirty().progress, 1);
        // Left and set again: from the new write, not from 0.
        p.leave(0);
        assert_eq!(p.get(0).map(|r| r.last_fetch), Some(0));
        p.set_home(0, Some(k), 1000);
        assert_eq!(p.get(0).map(|r| r.last_fetch), Some(1000));
        // An add starts at its own fetch's `sent`.
        p.add([5; 32], 5, Name::EMPTY, 0, 0, 1200).unwrap();
        assert_eq!(p.get(1).map(|r| r.last_fetch), Some(1200));
        // A phone joining that record keeps the later of the two.
        p.pair(keys(2), 0).unwrap();
        assert_eq!(p.set_home(2, Some([5; 32]), 1100), 1);
        assert_eq!(p.get(1).map(|r| r.last_fetch), Some(1200));
        p.unpair(1);
        p.pair(keys(3), 0).unwrap();
        assert_eq!(p.set_home(2, Some([5; 32]), 1300), 1);
        assert_eq!(p.get(1).map(|r| r.last_fetch), Some(1300));
    }

    const HOME: Parts = Parts {
        bond: false,
        home: true,
    };
    const BOND: Parts = Parts {
        bond: true,
        home: false,
    };

    /// Every record a Phones read lists: number, paired, home, owner.
    fn listed(p: &Phones) -> std::vec::Vec<(u16, bool, bool, bool)> {
        let all = every_page(p);
        let mut out = [None; RECORDS];
        let n = decode_list(&all, &mut out).unwrap();
        out[..n]
            .iter()
            .flatten()
            .map(|l| (l.number, l.paired, l.home, l.owner))
            .collect()
    }

    /// A removed home keeps its key with none of a home's rights: no
    /// owner's mark, no list shows it alone, and one with a bond shows as
    /// paired only (spec "Phones").
    #[test]
    fn a_removed_home_keeps_its_key_and_shows_nowhere() {
        let mut p = Phones::EMPTY;
        let (ann, bo) = ([1; 32], [2; 32]);
        p.pair(keys(1), 0).unwrap();
        p.set_home(0, Some(ann), 100);
        p.add(bo, 2, Name::EMPTY, 0, 0, 200).unwrap();
        assert!(p.is_owner(0));
        p.progress(0, 0, 150);
        p.remove(1, HOME).unwrap();
        let r = *p.get(0).unwrap();
        assert_eq!(
            (r.key, r.owner, r.last_fetch),
            (HomeKey::Removed(ann), false, 150)
        );
        assert_eq!(p.owner(), None, "the seat is empty");
        assert_eq!(
            listed(&p),
            [(1, true, false, false), (2, false, true, false)]
        );
        // Kept so in flash.
        assert_eq!(
            Record::decode(&Record::encode(0, Some(&r))),
            Some((0, Some(r)))
        );
        // Its bond gone too, it holds only the removed key, and is shown
        // nowhere.
        p.remove(1, BOND).unwrap();
        assert!(p.get(0).is_some_and(|r| r.only_removed()));
        assert_eq!(listed(&p), [(2, false, true, false)]);
        let mut out = [0u8; 200];
        let mut l = [None; RECORDS];
        let n = p.list(1, 0, &mut out);
        assert_eq!(decode_list(&out[..n], &mut l), Ok(1));
        // It is not a home: no hand-over, no removal again, no leave.
        assert_eq!(p.make_owner(1), None);
        assert_eq!(p.remove(1, HOME), None);
        p.leave(0);
        assert_eq!(p.get(0).map(|r| r.key), Some(HomeKey::Removed(ann)));
        assert_eq!(p.room(), RECORDS as u8 - 1, "it is room");
    }

    /// The same key set again, by Home or by the owner's add, makes the
    /// removed record a home again, its last fetch never lowered
    /// (roadmap D236, D239).
    #[test]
    fn a_removed_key_set_again_is_a_home_again() {
        let mut p = Phones::EMPTY;
        let (ann, bo) = ([1; 32], [2; 32]);
        p.pair(keys(1), 0).unwrap();
        p.set_home(0, Some(ann), 100);
        p.progress(0, 0, 300);
        p.remove(1, HOME).unwrap();
        p.set_home(0, Some(ann), 200);
        let r = *p.get(0).unwrap();
        assert_eq!((r.key, r.last_fetch), (HomeKey::Home(ann), 300));
        assert!(p.is_owner(0), "the empty seat");
        p.remove(1, HOME).unwrap();
        p.set_home(0, Some(ann), 400);
        assert_eq!(p.get(0).map(|r| r.last_fetch), Some(400));
        // A removed record without a bond, set again from a new phone,
        // takes the phone's bond.
        p.remove(
            1,
            Parts {
                bond: true,
                home: true,
            },
        )
        .unwrap();
        p.pair(keys(2), 0).unwrap();
        assert_eq!(p.set_home(1, Some(ann), 350), 0);
        let r = *p.get(0).unwrap();
        assert_eq!(
            (r.bond, r.key.home(), r.last_fetch),
            (Some(keys(2)), Some(ann), 400)
        );
        assert_eq!(p.get(1), None);
        // An add of a removed key is not "already there": it is a home again
        // in its record, with the add's tag and name.
        let at = p.add(bo, 2, Name::EMPTY, 0, 10, 500).unwrap();
        p.progress(at, 0, 700);
        let number = p.get(at).unwrap().number;
        p.remove(number, HOME).unwrap();
        let eva = Name::new(b"Eva").unwrap();
        assert_eq!(p.add(bo, 5, eva, 0, 20, 600), Ok(at));
        let r = *p.get(at).unwrap();
        assert_eq!(
            (r.number, r.key.home(), r.tag, r.name, r.day, r.last_fetch),
            (number, Some(bo), Some(5), eva, 20, 700)
        );
        assert_eq!(p.add(bo, 5, eva, 0, 20, 800), Err(NotAdded::Already));
    }

    /// A node that needs a record and has none free takes the one that
    /// holds only a removed key, seen longest ago, and never another
    /// (spec "Pairing").
    #[test]
    fn a_full_node_takes_the_oldest_removed_record_and_no_other() {
        let mut p = Phones::EMPTY;
        p.pair(keys(0), 0).unwrap();
        p.set_home(0, Some([0; 32]), 0);
        for k in 1..RECORDS as u8 {
            p.add([k; 32], k, Name::EMPTY, 0, 100 + u16::from(k), 0)
                .unwrap();
        }
        assert_eq!(p.pair(keys(20), 0), Err(Full));
        assert_eq!(
            p.add([20; 32], 20, Name::EMPTY, 0, 0, 0),
            Err(NotAdded::Full)
        );
        // Records 5 and 3 removed, and 0, which keeps its bond and so its
        // place.
        for at in [5, 3, 0] {
            let number = p.get(at).unwrap().number;
            p.remove(number, HOME).unwrap();
        }
        assert_eq!(p.room(), 2);
        assert_eq!(p.pair(keys(21), 0), Ok(Paired::Added(3)));
        let r = *p.get(3).unwrap();
        assert_eq!((r.bond, r.key), (Some(keys(21)), HomeKey::None));
        assert_eq!(p.add([22; 32], 22, Name::EMPTY, 0, 0, 0), Ok(5));
        assert_eq!(
            p.get(5).map(|r| (r.key.home(), r.bond)),
            Some((Some([22; 32]), None))
        );
        assert_eq!(p.pair(keys(23), 0), Err(Full));
        assert_eq!(
            p.add([23; 32], 23, Name::EMPTY, 0, 0, 0),
            Err(NotAdded::Full)
        );
        let r = *p.get(0).unwrap();
        assert_eq!((r.bond, r.key), (Some(keys(0)), HomeKey::Removed([0; 32])));
        assert!((1..RECORDS).all(|i| p
            .get(i)
            .is_some_and(|r| r.key.home().is_some() || r.bond.is_some())));
    }

    #[test]
    fn phones_writes_are_taken_whole() {
        let mut b = [0u8; PhonesWrite::MAX_LEN];
        for w in [
            PhonesWrite::From(7),
            PhonesWrite::Code,
            PhonesWrite::Remove {
                number: 0x0102,
                parts: Parts {
                    bond: false,
                    home: true,
                },
                code: 999_999,
            },
            PhonesWrite::Owner {
                number: 0x0304,
                code: 5,
            },
            PhonesWrite::Remove {
                number: 0x0102,
                parts: Parts {
                    bond: false,
                    home: true,
                },
                code: 999_999,
            },
        ] {
            let n = w.encode(&mut b);
            assert_eq!(PhonesWrite::decode(&b[..n]), Ok(w));
        }
        assert_eq!(&b, &[2, 1, 2, 2, 0x0f, 0x42, 0x3f]);
        assert_eq!(PhonesWrite::decode(&[0, 8]), Err(Malformed::Value));
        assert_eq!(PhonesWrite::decode(&[0]), Err(Malformed::Length));
        assert_eq!(PhonesWrite::decode(&[1, 0]), Err(Malformed::Length));
        assert_eq!(
            PhonesWrite::decode(&[2, 0, 1, 0, 0, 0, 0]),
            Err(Malformed::Value)
        );
        assert_eq!(
            PhonesWrite::decode(&[2, 0, 1, 4, 0, 0, 9]),
            Ok(PhonesWrite::Owner { number: 1, code: 9 })
        );
        assert_eq!(
            PhonesWrite::decode(&[2, 0, 1, 5, 0, 0, 0]),
            Err(Malformed::Value)
        );
        assert_eq!(
            PhonesWrite::decode(&[2, 0, 1, 4, 0x0f, 0x42, 0x40]),
            Err(Malformed::Value)
        );
        assert_eq!(
            PhonesWrite::decode(&[2, 0, 1, 1, 0x0f, 0x42, 0x40]),
            Err(Malformed::Value)
        );
        assert_eq!(PhonesWrite::decode(&[3]), Err(Malformed::Value));
        assert_eq!(PhonesWrite::decode(&[]), Err(Malformed::Length));
    }

    /// A code is taken once, within 30 s, and forgotten at the first try.
    #[test]
    fn a_code_takes_one_try() {
        assert_eq!(Code::draw(u32::MAX, 0), None);
        assert_eq!(
            Code::draw(u32::MAX - u32::MAX % CODES - 1, 0).map(|c| c.code),
            Some(CODES - 1)
        );
        let drawn = Code::draw(1_234_567, 1_000);
        assert_eq!(drawn.map(|c| c.code), Some(234_567));
        let mut shown = drawn;
        assert!(!Code::take(&mut shown, 234_568, 2_000));
        assert!(!Code::take(&mut shown, 234_567, 2_000), "one try");
        let mut shown = drawn;
        assert!(Code::take(&mut shown, 234_567, 30_999));
        assert_eq!(shown, None);
        let mut shown = drawn;
        assert!(!Code::take(&mut shown, 234_567, 31_000), "past 30 s");
        assert!(drawn.unwrap().shows(30_999) && !drawn.unwrap().shows(31_000));
        assert!(!Code::take(&mut None, 0, 0));
    }
}

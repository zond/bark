//! The node's phones: a record for each person paired with it, at most
//! [`RECORDS`] (spec "Phone link", Pairing, Phones; roadmap D168), as the
//! node keeps them, decides on them and stores them in flash, and the
//! Phones characteristic's values, `28bc0011-…` ([`crate::uuid::PHONES`]).
//!
//! The store's page (`bark-fw/src/store.rs`) is a log of 32-byte slots,
//! the last entry of each kind the one that holds. Three kinds are the
//! phones', each starting with its kind's byte:
//!
//! - a **record**, [`RECORD_TAG`], four slots: everything a record
//!   holds, or that its place is free ([`Record::encode`]);
//! - **progress**, [`PROGRESS_TAG`], one slot: what changes with each
//!   message read or fetched, the last confirmed and the last fetch, so
//!   that those writes cost a slot and not four ([`Progress`]);
//! - the **settings**, [`SETTINGS_TAG`], one slot: whether the node is
//!   private, and the next record's number ([`Settings`]).
//!
//! Each ends in a check over the rest: the flash writes a word at a time in
//! address order, so an entry cut short by a power cut leaves its last
//! word erased and fails it.

use crate::home::KEY_LEN;
use crate::Malformed;

/// Records a node keeps: a bit for each in a slot's mark, which is a byte
/// (spec "Mailbox", The ring).
pub const RECORDS: usize = 8;
/// Bytes of a person's name at most, as their card has it
/// (spec "Phone link", The payload, the card's display name).
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
    /// none known (spec "Phone link", Changes to the table).
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
    /// Their mailbox key, while the node is their home.
    pub key: Option<[u8; KEY_LEN]>,
    /// The newest item in the ring when the record was made: its bits in
    /// the marks of items up to this one were left by a record freed
    /// before, and count for nothing; its phone still reads those with
    /// its tag (spec "Mailbox", The ring).
    pub from: u32,
    /// The number of the last of their items the phone confirmed.
    pub read: u32,
    /// The `sent` of the last fetch the record took (spec "Mailbox", The
    /// fetch); 0 before any, and again once the key changes.
    pub last_fetch: u32,
    /// The day the phone was last linked, from 1 on 2026-01-01 UTC; 0 when
    /// the node had no time at any of its links.
    pub day: u16,
}

impl Record {
    /// A record for a phone just paired, with the ring's newest item.
    fn new(number: u16, bond: Keys, newest: u32) -> Record {
        Record {
            number,
            bond: Some(bond),
            name: Name::EMPTY,
            tag: None,
            key: None,
            from: newest,
            read: 0,
            last_fetch: 0,
            day: 0,
        }
    }

    /// Whether the record holds anything: a record that holds neither a
    /// bond nor a home is freed.
    fn holds(&self) -> bool {
        self.bond.is_some() || self.key.is_some()
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
const FLAG_HOME: u8 = 0x10;
const FLAGS: u8 = FLAG_BOND | FLAG_IRK | FLAG_CHANGED | FLAG_TAG | FLAG_HOME;

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
            if let Some(k) = rec.key {
                flags |= FLAG_HOME;
                r[AT_KEY..AT_KEY + KEY_LEN].copy_from_slice(&k);
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
        if !whole(r, RECORD_TAG) || index >= RECORDS || flags & !FLAGS != 0 {
            return None;
        }
        if flags & (FLAG_BOND | FLAG_HOME) == 0 {
            return Some((index, None));
        }
        let take16 = |at: usize| -> [u8; 16] { r[at..at + 16].try_into().expect("16 bytes") };
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
                key: (flags & FLAG_HOME != 0)
                    .then(|| r[AT_KEY..AT_KEY + KEY_LEN].try_into().expect("32 bytes")),
                from: be24(&r[AT_FROM..]),
                read: be24(&r[AT_READ..]),
                last_fetch: u32::from_be_bytes(
                    r[AT_FETCH..AT_FETCH + 4].try_into().expect("4 bytes"),
                ),
                day: u16::from_be_bytes([r[AT_DAY], r[AT_DAY + 1]]),
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
    /// The node's owners have made it private (spec "Phone link",
    /// Public).
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
        };
        p.private &= p.paired() > 0;
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
    /// A free place took it.
    Added(usize),
    /// Nobody was paired and every place held a home left behind: the one
    /// linked longest ago took it, its home forgotten.
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

/// Every record is in use and a phone is paired: the pairing is refused
/// (spec "Phone link", Pairing).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;

/// What a removal asks for (spec "Phone link", Phones).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parts {
    /// The bond.
    pub bond: bool,
    /// The home.
    pub home: bool,
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

    /// Whether the node serves guests: unless its owners made it private,
    /// and always while nobody is paired (spec "Phone link", Public).
    pub fn public(&self) -> bool {
        !self.private
    }

    /// Node's `room`: how many more phones may pair; 1 on a node nobody is
    /// paired with whose every place holds a home left behind.
    pub fn room(&self) -> u8 {
        let free = self.records.iter().filter(|r| r.is_none()).count() as u8;
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

    /// Take a phone's authenticated pairing that bonds, the ring's newest
    /// item being `newest` (spec "Phone link", Pairing).
    pub fn pair(&mut self, keys: Keys, newest: u32) -> Result<Paired, Full> {
        if let Some(i) = self.with_phone(keys.kind, &keys.addr) {
            let r = self.records[i].as_mut().expect("it holds the bond");
            r.bond = Some(keys);
            self.touch(i);
            return Ok(Paired::Again(i));
        }
        let (i, how) = match self.records.iter().position(Option::is_none) {
            Some(i) => (i, Paired::Added(i)),
            None if self.paired() == 0 => {
                let i = (0..RECORDS)
                    .min_by_key(|&i| self.records[i].map(|r| (r.day, r.number)))
                    .expect("eight places");
                (i, Paired::Took(i))
            }
            None => return Err(Full),
        };
        let number = self.number();
        self.records[i] = Some(Record::new(number, keys, newest));
        self.touch(i);
        Ok(how)
    }

    /// The record at `i` with its bond's keys changed, as the phone's view
    /// of the table changed (spec "Phone link", Changes to the table).
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
            }
        }
    }

    /// The phone at `i` is linked on day `day`, 0 when the node has no
    /// time: written when the day changes, so at most once a day.
    pub fn linked(&mut self, i: usize, day: u16) {
        if let Some(r) = self.records[i]
            .as_mut()
            .filter(|r| day != 0 && r.day != day)
        {
            r.day = day;
            self.touch(i);
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
        // The last bond gone, the node is one nobody has paired with:
        // public.
        self.set_private(self.private);
    }

    /// The phone at `i` unpairs: its bond goes, its home stays (spec
    /// "Phone link", Public).
    pub fn unpair(&mut self, i: usize) -> Option<Keys> {
        let r = self.records[i].as_mut()?;
        let bond = r.bond.take();
        self.touch(i);
        self.free_if_empty(i);
        bond
    }

    /// The phone at `i` writes Home: its person's key, or none; the place
    /// that holds the phone's record after it. A record holding the same
    /// key and no bond was this person's home, left by a phone that
    /// unpaired: it takes this phone's bond, name and tag and keeps its
    /// own place, whose bits in the ring's marks are the person's
    /// unfetched mail, and the phone's place is freed (spec "Phone link",
    /// Home).
    pub fn set_home(&mut self, i: usize, key: Option<[u8; KEY_LEN]>) -> usize {
        let Some(r) = self.records[i].as_mut() else {
            return i;
        };
        if r.key != key {
            r.key = key;
            r.last_fetch = 0;
            self.touch(i);
        }
        let Some(k) = key else {
            return i;
        };
        let left = (0..RECORDS).find(|&j| {
            j != i && self.records[j].is_some_and(|o| o.bond.is_none() && o.key == Some(k))
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
        self.touch(i);
        self.touch(j);
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
        let lacks = (parts.bond && r.bond.is_none()) || (parts.home && r.key.is_none());
        (!lacks).then_some(i)
    }

    /// Remove `parts` of the record numbered `number` (spec "Phone link",
    /// Phones): `None` when no record has the number or it does not hold a
    /// part asked for.
    pub fn remove(&mut self, number: u16, parts: Parts) -> Option<Removed> {
        let i = self.holding(number, parts)?;
        let r = self.records[i].as_mut().expect("found");
        let bond = match parts.bond {
            true => r.bond.take(),
            false => None,
        };
        if parts.home {
            r.key = None;
            r.last_fetch = 0;
        }
        self.touch(i);
        self.free_if_empty(i);
        Some(Removed { index: i, bond })
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

    /// Phones, read: the records from place `from` on, as many whole ones
    /// as fit in `out`, after the place to write next, 0 once the last is
    /// in; `reader` is the reading phone's place and `linked` a bit for
    /// each place whose phone is linked now. Its length.
    pub fn page(&self, from: u8, reader: Option<usize>, linked: u8, out: &mut [u8]) -> usize {
        let mut len = 1;
        out[0] = 0;
        for (i, r) in self.records.iter().enumerate().skip(usize::from(from)) {
            let Some(r) = r else {
                continue;
            };
            let name = r.name.as_bytes();
            let need = PER_RECORD + name.len();
            if len + need > out.len() {
                out[0] = i as u8;
                break;
            }
            let flag = |set: bool, bit: u8| if set { bit } else { 0 };
            let b = &mut out[len..len + need];
            b[..2].copy_from_slice(&r.number.to_be_bytes());
            b[2] = flag(r.bond.is_some(), 0x01)
                | flag(r.key.is_some(), 0x02)
                | flag(linked & (1 << i) != 0, 0x04)
                | flag(reader == Some(i), 0x08)
                | flag(r.tag.is_some(), 0x10);
            b[3..5].copy_from_slice(&r.day.to_be_bytes());
            b[5] = r.tag.unwrap_or(0);
            b[6] = name.len() as u8;
            b[PER_RECORD..].copy_from_slice(name);
            len += need;
        }
        len
    }
}

/// Bytes of a record in a Phones read before its name.
const PER_RECORD: usize = 2 + 1 + 2 + 1 + 1;

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
    /// It holds a mailbox key.
    pub home: bool,
    /// Its phone is linked now.
    pub linked: bool,
    /// It is the reading phone's.
    pub mine: bool,
    /// Its person's tag at the node, if known.
    pub tag: Option<u8>,
    /// The day its phone was last linked.
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
    let (&next, mut rest) = b.split_first().ok_or(Malformed::Length)?;
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
            tag: (flags & 0x10 != 0).then_some(rest[5]),
            day: u16::from_be_bytes([rest[3], rest[4]]),
            name: &rest[PER_RECORD..end],
        });
        n += 1;
        rest = &rest[end..];
    }
    Ok((next, n))
}

/// Phones, written (spec "Phone link", Phones).
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
}

/// A removal code's numbers: six digits.
pub const CODES: u32 = 1_000_000;

impl PhonesWrite {
    const FROM: u8 = 0;
    const CODE: u8 = 1;
    const REMOVE: u8 = 2;
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
        }
    }

    /// Take a written value whole.
    pub fn decode(b: &[u8]) -> Result<PhonesWrite, Malformed> {
        match (b.first(), b.len()) {
            (Some(&Self::FROM), 2) if usize::from(b[1]) < RECORDS => Ok(PhonesWrite::From(b[1])),
            (Some(&Self::CODE), 1) => Ok(PhonesWrite::Code),
            (Some(&Self::REMOVE), Self::MAX_LEN) => {
                let code = be24(&b[4..]);
                if !(1..=3).contains(&b[3]) || code >= CODES {
                    return Err(Malformed::Value);
                }
                Ok(PhonesWrite::Remove {
                    number: u16::from_be_bytes([b[1], b[2]]),
                    parts: Parts {
                        bond: b[3] & 1 != 0,
                        home: b[3] & 2 != 0,
                    },
                    code,
                })
            }
            (Some(&Self::FROM), 2) => Err(Malformed::Value),
            (Some(&(Self::FROM | Self::CODE | Self::REMOVE)), _) | (None, _) => {
                Err(Malformed::Length)
            }
            (Some(_), _) => Err(Malformed::Value),
        }
    }
}

/// How long a removal code is taken after it is drawn: the time a pairing
/// gives its person to type a code, the Security Manager's timeout
/// (Bluetooth Core Vol 3 Part H §3.4).
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
            key: Some([n; 32]),
            from: 70_000,
            read: 0x01_0203,
            last_fetch: 0x0405_0607,
            day: 314,
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
                key: None,
                bond: Some(Keys {
                    irk: None,
                    service_changed: false,
                    ..keys(4)
                }),
                ..full(4)
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
        p.set_home(5, Some([1; 32]));
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
            p.linked(n.into(), [50, 40, 10, 30, 20, 60, 70, 80][usize::from(n)]);
            p.set_home(n.into(), Some([n; 32]));
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
        assert_eq!((r.key, r.from, r.number), (None, 5, 9));
        assert_eq!(p.room(), 0);
    }

    #[test]
    fn the_day_is_written_when_it_changes() {
        let mut p = Phones::EMPTY;
        p.pair(keys(1), 0).unwrap();
        p.take_dirty();
        p.linked(0, 0);
        p.linked(0, 300);
        assert_eq!(p.take_dirty().records, 1);
        p.linked(0, 300);
        p.linked(0, 0);
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
        p.set_home(0, Some(k));
        p.progress(0, 30, 31);
        p.unpair(0);
        p.pair(keys(2), 50).unwrap();
        p.set_recipient(1, 0x11, Name::new(b"Eva").unwrap());
        p.linked(1, 400);
        assert_eq!(p.set_home(1, Some(k)), 0);
        assert_eq!(p.get(1), None);
        let r = p.get(0).unwrap();
        assert_eq!((r.bond, r.tag, r.key), (Some(keys(2)), Some(0x11), Some(k)));
        assert_eq!((r.from, r.read, r.last_fetch, r.day), (0, 30, 31, 400));
        assert_eq!(r.name.as_bytes(), b"Eva");
        // Clearing forgets the key and the last fetch, the record stays.
        assert_eq!(p.set_home(0, None), 0);
        assert_eq!(p.get(0).map(|r| (r.key, r.last_fetch)), Some((None, 0)));
    }

    #[test]
    fn a_removal_takes_what_the_record_holds() {
        let mut p = Phones::EMPTY;
        p.pair(keys(1), 0).unwrap();
        p.pair(keys(2), 0).unwrap();
        p.set_home(1, Some([2; 32]));
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
        assert_eq!(p.get(1).map(|r| r.key), Some(None));
        p.set_private(true);
        assert_eq!(
            p.remove(2, bond),
            Some(Removed {
                index: 1,
                bond: Some(keys(2))
            })
        );
        assert_eq!(p.get(1), None);
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
        p.set_home(2, Some([1; 32]));
        p.unpair(2);
        p.linked(3, 9);
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
            PhonesWrite::decode(&[2, 0, 1, 4, 0, 0, 0]),
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

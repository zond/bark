//! The home node's mailbox, an application on the node beside the phone
//! link (spec "Mailbox"). It reads what the core delivers to the node's
//! application (`AppEvent::Message`, the payload from `Core::inbox`),
//! offers replies as `AppCommand::Send` while `accepts_send` holds, and
//! takes a reply back on `AppEvent::GivenBack`; it changes nothing in the
//! core, and the simulator can run it on a [`Ring`] in RAM.
//!
//! The node holds the mailbox key and cannot open the person's mail: it
//! keeps every payload but a fetch, numbered, and answers a fetch once:
//! the newest batch of the items it has not sent, newest first, each
//! sealed again under the key with the count of those still unsent after
//! the batch (`seal::seal_reply`), or one reply that says nothing is new
//! (`seal::seal_nothing_new`). An item handed over for a reply, or
//! confirmed by the phone at the node, is marked sent in its slot and
//! stays in the ring. An item that arrives later waits for the next
//! fetch.
//!
//! On a public node the ring keeps the guests' mail too, every payload for
//! a recipient tag that is not the owner's, which a guest reads by its tag
//! after a number it keeps itself ([`Mailbox::for_tag`]); such an item is
//! written marked sent, so no fetch sends it and it counts in nothing of
//! the owner's, and it leaves the ring as the ring wraps (spec "Mailbox",
//! "Phone link", Public).

use core::cell::Cell;

use bark_proto::MsgId;

use crate::keys::KEY_LEN;
use crate::seal::{self, Fetch, Form, MAX_NUMBER, MAX_REPLIES};
use crate::{be, Address, MAX_APP_PAYLOAD, MAX_PAYLOAD};

/// Bytes of a slot: an erase page of 4096 holds 16.
pub const SLOT_LEN: usize = 256;
/// Slots in an erase page of 4096 bytes (`flash_ring::PAGE_LEN`).
pub const SLOTS_PER_PAGE: u32 = 4096 / SLOT_LEN as u32;

/// Where a slot's check sits: the last four bytes, over all before them
/// but the mark, which is read as erased.
const CHECK_AT: usize = SLOT_LEN - 4;
/// Where a slot's payload starts: after its number (3) and length (1).
const PAYLOAD_AT: usize = seal::NUMBER_LEN + 1;
/// Where a slot's message ID sits: the four bytes before the check, so
/// that the payload's place is the same in a slot written before it was
/// kept, whose ID reads as `0xFFFF_FFFF`, the erased flash.
const ID_AT: usize = CHECK_AT - 4;
/// The byte that marks an item sent: erased while it is not, cleared to
/// 0 without an erase when it is, so that the item stays. A slot written
/// before marks were kept reads as not sent.
const MARK_AT: usize = ID_AT - 1;
const _: () = assert!(PAYLOAD_AT + MAX_PAYLOAD <= MARK_AT);

/// Bytes of the mailbox's kept state.
pub const STATE_LEN: usize = 64;

/// The flash a mailbox keeps its items and its state in: pages of
/// [`SLOTS_PER_PAGE`] slots, erased to `0xFF` a page at a time, and one
/// record of state. The firmware backs it with flash, the simulator and
/// the tests with RAM.
pub trait Ring {
    /// Pages in the ring, at least two: one is erased to make room.
    fn pages(&self) -> u32;
    /// The bytes of slot `slot`.
    fn read(&self, slot: u32, out: &mut [u8; SLOT_LEN]);
    /// Writes slot `slot`, erased since it was last written.
    fn write(&mut self, slot: u32, bytes: &[u8; SLOT_LEN]);
    /// Clears byte `at` of slot `slot` to 0, without an erase: flash
    /// clears bits in a written byte.
    fn clear(&mut self, slot: u32, at: usize);
    /// Byte `at` of slot `slot`; cheaper than the slot where the flash
    /// is slow to read.
    fn read_byte(&self, slot: u32, at: usize) -> u8 {
        let mut b = [0u8; SLOT_LEN];
        self.read(slot, &mut b);
        b[at]
    }
    /// Erases page `page`.
    fn erase(&mut self, page: u32);
    /// The state last kept, if any.
    fn state(&self) -> Option<[u8; STATE_LEN]>;
    /// Keeps `state` across a reboot.
    fn keep_state(&mut self, state: &[u8; STATE_LEN]);
}

fn fnv1a(bytes: &[u8]) -> u32 {
    fnv1a_on(0x811c_9dc5, bytes)
}

fn fnv1a_on(h: u32, bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(h, |h, &b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193))
}

/// A slot's check, its mark read as erased.
fn slot_check(b: &[u8; SLOT_LEN]) -> u32 {
    let h = fnv1a_on(fnv1a(&b[..MARK_AT]), &[0xff]);
    fnv1a_on(h, &b[MARK_AT + 1..CHECK_AT])
}

/// What a payload delivered to the home node was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrival {
    /// Kept as this item.
    Kept(u32),
    /// A fetch, taken: its answer follows.
    Fetch,
    /// A fetch that does not open under the key, or is not later than the
    /// last taken: dropped.
    Refused,
    /// The node is nobody's home and not public: nothing is kept.
    NotHome,
}

/// Whose the ring's items are, by their recipient tags (spec "Phone link",
/// Public).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// Every item is the owner's: a node that is not public, or one whose
    /// owner's phone has not written its tag yet.
    All,
    /// The owner's are those with this tag; the rest are guests'.
    Tag(u8),
    /// None is: a public node nobody has paired with.
    Nobody,
}

/// The fetch the mailbox answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Reply {
    /// The fetch's `sent`: the replies' round.
    round: u32,
    /// Where they go.
    address: Address,
    /// The requester's recipient tag there.
    recipient: u8,
    /// The next item to look at, going down; the round has none below
    /// `bottom`.
    next: u32,
    /// The batch's oldest item.
    bottom: u32,
    /// Replies with an item handed to the core: the last one's place.
    handed: u32,
    /// Items unsent after the batch when the fetch was taken.
    remaining: u16,
    /// An item given back, with its place, to send again.
    again: Option<(u32, u32)>,
    /// A reply of the round has been handed to the core.
    answered: bool,
}

/// A reply handed to the core and not yet past its first hop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InFlight {
    msg_id: MsgId,
    /// The item, or 0 for the reply that says nothing is new.
    number: u32,
    /// Its place in the round.
    place: u32,
}

/// A reply to offer: send `payload[..len]` to `address`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outgoing {
    /// The fetch's address.
    pub address: Address,
    /// The item it carries, or 0 when it says nothing is new.
    pub number: u32,
    /// Its length in the buffer handed in.
    pub len: usize,
    /// The message's ID, as the core will compute it.
    pub msg_id: MsgId,
}

/// The mailbox of a home node.
pub struct Mailbox<R: Ring> {
    ring: R,
    key: Option<[u8; KEY_LEN]>,
    /// The `sent` of the last fetch taken; 0 before any.
    last_fetch: u32,
    reply: Option<Reply>,
    in_flight: Option<InFlight>,
    /// The highest number kept; 0 before any.
    newest: u32,
    /// Items in the ring not marked sent.
    unsent: u32,
    /// The highest number the phone at the node has confirmed reading;
    /// 0 before any (spec "Phone link", Receiving).
    read: u32,
    /// The node is public ([`Mailbox::keeps`]).
    public: bool,
    /// Whose the items are.
    owner: Owner,
    /// The owner's oldest unread item's number, 0 for none, and the count
    /// of them, while nothing has changed since: found by reading each
    /// later item's tag, which the node's loop asks for at every step.
    owners_unread: Cell<Option<(u32, u32)>>,
}

impl<R: Ring> Mailbox<R> {
    /// The mailbox in `ring`, as it was before a reboot.
    pub fn new(ring: R) -> Mailbox<R> {
        let mut m = Mailbox {
            ring,
            key: None,
            last_fetch: 0,
            reply: None,
            in_flight: None,
            newest: 0,
            unsent: 0,
            read: 0,
            public: false,
            owner: Owner::All,
            owners_unread: Cell::new(None),
        };
        // A whole slot counts only in its own place: one that is not, left
        // by a ring laid out otherwise before, would set the newest number
        // past slots that are not erased, and the next item written over
        // one of them would fail its check.
        for s in 0..m.slots() {
            if let Some((n, _, sent)) = m.slot(s).filter(|&(n, ..)| m.slot_of(n) == s) {
                m.newest = m.newest.max(n);
                m.unsent += u32::from(!sent);
            }
        }
        if let Some(s) = m.ring.state() {
            m.restore(&s);
        }
        m
    }

    /// The ring, for the driver that backs it.
    pub fn ring(&self) -> &R {
        &self.ring
    }

    /// The ring, for a driver that writes it later than it is asked to
    /// (`flash_ring::FlashRing`).
    pub fn ring_mut(&mut self) -> &mut R {
        &mut self.ring
    }

    /// The oldest item the phone at the node has not confirmed: its number,
    /// the message ID it arrived with and its length in `out`. The phone
    /// reads the ring from where it last confirmed; the ring keeps every
    /// item for fetches either way.
    pub fn unread(&self, out: &mut [u8; MAX_PAYLOAD]) -> Option<(u32, MsgId, usize)> {
        let (n, _) = self.owners_unread();
        self.read_item(n, out).map(|(id, l)| (n, id, l))
    }

    /// Items the phone at the node has not confirmed.
    pub fn unread_count(&self) -> u32 {
        self.owners_unread().1
    }

    /// The owner's oldest unread item and the count of them.
    fn owners_unread(&self) -> (u32, u32) {
        if let Some(u) = self.owners_unread.get() {
            return u;
        }
        let from = (self.read + 1).max(self.oldest());
        let mut first = 0;
        let mut count = 0;
        for n in (from..=self.newest).filter(|&n| self.owners(n)) {
            if first == 0 && self.read_item(n, &mut [0u8; MAX_PAYLOAD]).is_some() {
                first = n;
            }
            count += 1;
        }
        self.owners_unread.set(Some((first, count)));
        (first, count)
    }

    /// Whether item `n` is the owner's, by its tag.
    fn owners(&self, n: u32) -> bool {
        match self.owner {
            Owner::All => true,
            Owner::Nobody => false,
            Owner::Tag(t) => {
                self.ring
                    .read_byte(self.slot_of(n), PAYLOAD_AT + seal::RECIPIENT_AT)
                    == t
            }
        }
    }

    /// Whose the items are from now on; items kept before stay as they
    /// were written.
    pub fn set_owner(&mut self, owner: Owner) {
        self.owner = owner;
        self.owners_unread.set(None);
    }

    /// The oldest item after `after` whose recipient tag is `tag`: its
    /// number, the message ID it arrived with and its length in `out`. A
    /// guest reads its mail so, keeping `after` itself, and reading takes
    /// nothing from the ring. A number above the newest is from before
    /// the ring began again, erased or on another board, and reads as 0.
    pub fn for_tag(
        &self,
        tag: u8,
        after: u32,
        out: &mut [u8; MAX_PAYLOAD],
    ) -> Option<(u32, MsgId, usize)> {
        let after = if after > self.newest { 0 } else { after };
        let from = after.saturating_add(1).max(self.lowest());
        (from..=self.newest)
            .filter(|&n| {
                self.ring
                    .read_byte(self.slot_of(n), PAYLOAD_AT + seal::RECIPIENT_AT)
                    == tag
            })
            .find_map(|n| {
                self.read_item(n, out)
                    .filter(|&(_, len)| out[..len].get(seal::RECIPIENT_AT) == Some(&tag))
                    .map(|(id, len)| (n, id, len))
            })
    }

    /// The phone at the node has item `n`, and every one before it. It
    /// counts as sent, so that no fetch sends it again; the phone confirms
    /// the oldest it has not, [`Mailbox::unread`], so those before it are
    /// marked or gone.
    pub fn confirm_read(&mut self, n: u32) {
        if n > self.read && n <= self.newest {
            self.mark(n);
            self.read = n;
            self.owners_unread.set(None);
            self.keep();
        }
    }

    /// Items in the ring not yet sent to a fetch or read by the phone at
    /// the node.
    pub fn unsent(&self) -> u32 {
        self.unsent
    }

    fn slots(&self) -> u32 {
        self.ring.pages() * SLOTS_PER_PAGE
    }

    fn slot_of(&self, n: u32) -> u32 {
        (n - 1) % self.slots()
    }

    /// The lowest number the ring can still hold.
    fn lowest(&self) -> u32 {
        self.newest.saturating_sub(self.slots() - 1).max(1)
    }

    /// The number and the payload's length in slot `s`, and whether it is
    /// marked sent, if it holds a whole item.
    fn slot(&self, s: u32) -> Option<(u32, usize, bool)> {
        let mut b = [0u8; SLOT_LEN];
        self.ring.read(s, &mut b);
        let check = be(&b[CHECK_AT..]);
        let len = usize::from(b[seal::NUMBER_LEN]);
        let n = be(&b[..seal::NUMBER_LEN]);
        (slot_check(&b) == check && len <= MAX_PAYLOAD && n != 0).then_some((
            n,
            len,
            b[MARK_AT] != 0xff,
        ))
    }

    /// Whether item `n` is in the ring and not marked sent. The mark is
    /// read first: most slots a batch passes over are sent.
    fn is_unsent(&self, n: u32) -> bool {
        if n < self.lowest() || n > self.newest {
            return false;
        }
        let s = self.slot_of(n);
        self.ring.read_byte(s, MARK_AT) == 0xff
            && self.slot(s).is_some_and(|(got, _, sent)| got == n && !sent)
    }

    /// Marks item `n` sent, if it is unsent.
    fn mark(&mut self, n: u32) {
        if self.is_unsent(n) {
            self.ring.clear(self.slot_of(n), MARK_AT);
            self.unsent = self.unsent.saturating_sub(1);
        }
    }

    /// Whether the node is someone's home.
    pub fn is_home(&self) -> bool {
        self.key.is_some()
    }

    /// Whether the ring keeps what arrives: on a home, and on a public
    /// node, whose owner's messages it keeps as a home's while guests
    /// come and go (spec "Phone link", Public).
    pub fn keeps(&self) -> bool {
        self.key.is_some() || self.public
    }

    /// The node is public, or no longer; not kept here, as the setting is
    /// kept with the bond (`bark_link::bond`).
    pub fn set_public(&mut self, public: bool) {
        self.public = public;
    }

    /// Makes the node a home with this mailbox key, or no home with
    /// `None` (the phone link's Home characteristic). A new key forgets
    /// the last fetch, which was under the old one.
    pub fn set_key(&mut self, key: Option<[u8; KEY_LEN]>) {
        if self.key != key {
            self.key = key;
            self.last_fetch = 0;
            self.reply = None;
            self.in_flight = None;
            self.keep();
        }
    }

    /// The highest number kept, 0 before any.
    pub fn newest(&self) -> u32 {
        self.newest
    }

    /// The oldest number the ring still holds, 0 when it holds none.
    pub fn oldest(&self) -> u32 {
        (self.lowest()..=self.newest)
            .find(|&n| self.item(n, &mut [0u8; MAX_PAYLOAD]).is_some())
            .unwrap_or(0)
    }

    /// Item `n` into `out`, its length; `None` once it is overwritten.
    pub fn item(&self, n: u32, out: &mut [u8; MAX_PAYLOAD]) -> Option<usize> {
        self.read_item(n, out).map(|(_, len)| len)
    }

    /// Item `n` into `out`: the message ID it arrived with, and its length.
    fn read_item(&self, n: u32, out: &mut [u8; MAX_PAYLOAD]) -> Option<(MsgId, usize)> {
        if n == 0 || n > self.newest {
            return None;
        }
        let s = self.slot_of(n);
        let mut b = [0u8; SLOT_LEN];
        self.ring.read(s, &mut b);
        let (got, len, _) = self.slot(s)?;
        if got != n {
            return None;
        }
        out[..len].copy_from_slice(&b[PAYLOAD_AT..PAYLOAD_AT + len]);
        Some((MsgId(be(&b[ID_AT..CHECK_AT])), len))
    }

    /// A payload the core delivered to this node's application, with its
    /// message ID. The ID is kept with the item: the phone confirms an
    /// item by it, and computed again from the node's address it would
    /// change whenever the address does.
    pub fn arrive(&mut self, msg_id: MsgId, payload: &[u8]) -> Arrival {
        if !self.keeps() {
            return Arrival::NotHome;
        }
        if Form::of(payload) == Ok(Form::Fetch) {
            let Some(key) = self.key else {
                return Arrival::Refused;
            };
            return match Fetch::open(&key, payload) {
                Ok(f) if f.sent > self.last_fetch => {
                    self.take(f);
                    Arrival::Fetch
                }
                _ => Arrival::Refused,
            };
        }
        if payload.len() > MAX_PAYLOAD || self.newest == MAX_NUMBER {
            return Arrival::Refused;
        }
        let n = self.newest + 1;
        let s = self.slot_of(n);
        if s.is_multiple_of(SLOTS_PER_PAGE) {
            for o in s..s + SLOTS_PER_PAGE {
                let gone = self.slot(o).filter(|&(k, ..)| self.slot_of(k) == o);
                let lost = u32::from(gone.is_some_and(|(.., sent)| !sent));
                self.unsent = self.unsent.saturating_sub(lost);
            }
            self.ring.erase(s / SLOTS_PER_PAGE);
        }
        let owners = match self.owner {
            Owner::All => true,
            Owner::Nobody => false,
            Owner::Tag(t) => seal::recipient_of(payload) == Some(t),
        };
        let mut b = [0xffu8; SLOT_LEN];
        b[..seal::NUMBER_LEN].copy_from_slice(&n.to_be_bytes()[1..]);
        b[seal::NUMBER_LEN] = payload.len() as u8;
        b[PAYLOAD_AT..PAYLOAD_AT + payload.len()].copy_from_slice(payload);
        b[ID_AT..CHECK_AT].copy_from_slice(&msg_id.0.to_be_bytes());
        let check = slot_check(&b);
        b[CHECK_AT..].copy_from_slice(&check.to_be_bytes());
        // A guest's item is sent as it is written: no fetch of the
        // owner's sends it. The check reads the mark as erased.
        if !owners {
            b[MARK_AT] = 0;
        }
        self.ring.write(s, &b);
        self.newest = n;
        self.unsent += u32::from(owners);
        self.owners_unread.set(None);
        Arrival::Kept(n)
    }

    fn take(&mut self, f: Fetch) {
        let lowest = self.lowest();
        let mut count = 0;
        let mut bottom = self.newest + 1;
        let mut n = self.newest;
        while count < MAX_REPLIES && n >= lowest {
            if self.is_unsent(n) {
                count += 1;
                bottom = n;
            }
            n -= 1;
        }
        self.last_fetch = f.sent;
        self.reply = Some(Reply {
            round: f.sent,
            address: f.address,
            recipient: f.recipient,
            next: self.newest,
            bottom,
            handed: 0,
            remaining: u16::try_from(self.unsent.saturating_sub(count)).unwrap_or(u16::MAX),
            again: None,
            answered: false,
        });
        self.in_flight = None;
        self.keep();
    }

    /// Whether a reply may be due: [`Mailbox::next`] then gives it, or
    /// finds that the phone at the node has read the rest meanwhile, after
    /// which none is due.
    pub fn reply_due(&self) -> bool {
        self.key.is_some()
            && self
                .reply
                .is_some_and(|r| r.again.is_some() || r.next >= r.bottom || !r.answered)
    }

    /// The next reply to hand the core, sealed into `out`, if one is due;
    /// call it while `accepts_send` holds, and send what it gives. Its
    /// item is marked sent as it is handed over. A round with no item to
    /// send gets one reply that says nothing is new.
    pub fn next(&mut self, out: &mut [u8; MAX_PAYLOAD]) -> Option<Outgoing> {
        let key = self.key?;
        let mut r = self.reply?;
        let mut sealed = None;
        if let Some((n, place)) = r.again.take() {
            sealed = self.seal_item(&key, &r, n, place, out);
        }
        while sealed.is_none() && r.next >= r.bottom {
            let n = r.next;
            r.next -= 1;
            if self.is_unsent(n) {
                sealed = self.seal_item(&key, &r, n, r.handed + 1, out);
                r.handed += 1;
                self.mark(n);
            }
        }
        if sealed.is_none() && !r.answered {
            let nothing = seal::seal_nothing_new(&key, r.recipient, r.round);
            out[..nothing.len()].copy_from_slice(&nothing);
            sealed = Some((0, 0, nothing.len()));
        }
        // Kept with nothing to send too, so that no reply is due after.
        r.answered |= sealed.is_some();
        self.reply = Some(r);
        let (number, place, len) = sealed?;
        let msg_id = MsgId::of(r.address.position, r.address.node, &out[..len]);
        self.in_flight = Some(InFlight {
            msg_id,
            number,
            place,
        });
        Some(Outgoing {
            address: r.address,
            number,
            len,
            msg_id,
        })
    }

    /// Item `n` at `place` in `r`'s round, sealed into `out`: its number,
    /// place and length; `None` once the ring has overwritten it, so that
    /// a place never seals two plaintexts. One longer than the app writes
    /// goes empty, so that its number still arrives.
    fn seal_item(
        &self,
        key: &[u8; KEY_LEN],
        r: &Reply,
        n: u32,
        place: u32,
        out: &mut [u8; MAX_PAYLOAD],
    ) -> Option<(u32, u32, usize)> {
        let mut item = [0u8; MAX_PAYLOAD];
        let len = self.item(n, &mut item)?;
        let len = if len > MAX_APP_PAYLOAD { 0 } else { len };
        seal::seal_reply(
            key,
            r.recipient,
            r.round,
            place,
            n,
            r.remaining,
            &item[..len],
            out,
        )
        .ok()
        .map(|m| (n, place, m))
    }

    /// The core gave the message back: if it is the last reply, it goes
    /// again when the node next takes one.
    pub fn given_back(&mut self, msg_id: MsgId) {
        if let (Some(f), Some(r)) = (self.in_flight, self.reply.as_mut()) {
            if f.msg_id == msg_id {
                if f.number == 0 {
                    r.answered = false;
                } else {
                    r.again = Some((f.number, f.place));
                }
                self.in_flight = None;
            }
        }
    }

    /// The state: the key, the last fetch and what the phone at the node
    /// has read. A round is not kept, to spare a write a reply: after a
    /// reboot its items not yet handed over wait, unsent, for the next
    /// fetch, and no place is sealed twice.
    fn keep(&mut self) {
        let mut s = [0u8; STATE_LEN];
        if let Some(k) = self.key {
            s[0] = 1;
            s[1..33].copy_from_slice(&k);
        }
        s[33..37].copy_from_slice(&self.last_fetch.to_be_bytes());
        s[57..60].copy_from_slice(&self.read.to_be_bytes()[1..]);
        let check = fnv1a(&s[..STATE_LEN - 4]);
        s[STATE_LEN - 4..].copy_from_slice(&check.to_be_bytes());
        self.ring.keep_state(&s);
    }

    fn restore(&mut self, s: &[u8; STATE_LEN]) {
        if fnv1a(&s[..STATE_LEN - 4]) != be(&s[STATE_LEN - 4..]) {
            return;
        }
        if s[0] == 1 {
            let mut k = [0u8; KEY_LEN];
            k.copy_from_slice(&s[1..33]);
            self.key = Some(k);
        }
        self.last_fetch = be(&s[33..37]);
        self.read = be(&s[57..60]).min(self.newest);
    }
}

/// A ring in RAM of `SLOTS` slots, a whole number of pages, for the
/// simulator and the tests.
pub struct RamRing<const SLOTS: usize> {
    slots: [[u8; SLOT_LEN]; SLOTS],
    state: Option<[u8; STATE_LEN]>,
}

impl<const SLOTS: usize> RamRing<SLOTS> {
    /// An erased ring with no state.
    pub fn new() -> Self {
        assert!(
            SLOTS.is_multiple_of(SLOTS_PER_PAGE as usize) && SLOTS >= 2 * SLOTS_PER_PAGE as usize
        );
        RamRing {
            slots: [[0xff; SLOT_LEN]; SLOTS],
            state: None,
        }
    }
}

impl<const SLOTS: usize> Default for RamRing<SLOTS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const SLOTS: usize> Ring for RamRing<SLOTS> {
    fn pages(&self) -> u32 {
        (SLOTS / SLOTS_PER_PAGE as usize) as u32
    }
    fn read(&self, slot: u32, out: &mut [u8; SLOT_LEN]) {
        *out = self.slots[slot as usize];
    }
    fn write(&mut self, slot: u32, bytes: &[u8; SLOT_LEN]) {
        // Flash only clears bits: a slot written twice without an erase
        // would hold the AND of both.
        for (d, s) in self.slots[slot as usize].iter_mut().zip(bytes) {
            *d &= s;
        }
    }
    fn clear(&mut self, slot: u32, at: usize) {
        self.slots[slot as usize][at] = 0;
    }
    fn erase(&mut self, page: u32) {
        let first = (page * SLOTS_PER_PAGE) as usize;
        for s in &mut self.slots[first..first + SLOTS_PER_PAGE as usize] {
            *s = [0xff; SLOT_LEN];
        }
    }
    fn state(&self) -> Option<[u8; STATE_LEN]> {
        self.state
    }
    fn keep_state(&mut self, state: &[u8; STATE_LEN]) {
        self.state = Some(*state);
    }
}

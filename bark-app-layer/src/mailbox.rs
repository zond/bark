//! The node's mailbox (spec "Mailbox"), an application beside the phone
//! link: it reads what the core delivers to the node's application, offers
//! replies as `AppCommand::Send` while `accepts_send` holds and takes back
//! a reply on `AppEvent::GivenBack`. It changes nothing in the core, and
//! the simulator runs it on a [`Ring`] in RAM.
//!
//! What a person's record keeps (the key, the last fetch, the last item
//! confirmed) lives with the record (`bark-link` `phones`), not here: the
//! mailbox says when they change.

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
/// Where a slot's message ID sits: the four bytes before the check.
const ID_AT: usize = CHECK_AT - 4;
/// The slot's mark: a bit for each person, set while the item is theirs
/// and not yet sent to them, and erased, all set, until the slot is
/// written. A bit is cleared without an erase, so that the item stays.
const MARK_AT: usize = ID_AT - 1;
const _: () = assert!(PAYLOAD_AT + MAX_PAYLOAD <= MARK_AT);

/// People a node keeps: a bit for each in a slot's mark, a byte.
pub const PEOPLE: usize = 8;

/// The flash a mailbox keeps its items in: pages of [`SLOTS_PER_PAGE`]
/// slots, erased to `0xFF` a page at a time. The firmware backs it with
/// flash, the simulator and the tests with RAM.
pub trait Ring {
    /// Pages in the ring, at least two: one is erased to make room.
    fn pages(&self) -> u32;
    /// The bytes of slot `slot`.
    fn read(&self, slot: u32, out: &mut [u8; SLOT_LEN]);
    /// Writes slot `slot`, erased since it was last written.
    fn write(&mut self, slot: u32, bytes: &[u8; SLOT_LEN]);
    /// Clears the bits `bits` of byte `at` of slot `slot`, without an
    /// erase: flash clears bits in a written byte.
    fn clear(&mut self, slot: u32, at: usize, bits: u8);
    /// Byte `at` of slot `slot`; cheaper than the slot where the flash
    /// is slow to read.
    fn read_byte(&self, slot: u32, at: usize) -> u8 {
        let mut b = [0u8; SLOT_LEN];
        self.read(slot, &mut b);
        b[at]
    }
    /// Erases page `page`.
    fn erase(&mut self, page: u32);
    /// Whether slot `slot` is on the flash as last written, its write and
    /// any erase before it done: a ring that writes later than asked
    /// (`flash_ring::FlashRing`) holds it only in its queue until then.
    fn written(&self, _slot: u32) -> bool {
        true
    }
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

/// What a payload delivered to the node was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrival {
    /// Kept as this item.
    Kept(u32),
    /// A fetch that opened under this person's key, taken: its answer
    /// follows, and the person's last fetch is its `sent`.
    Fetch(usize),
    /// A fetch that opens under no key, or is not later than the last
    /// its person took, or an item the ring has no number for: dropped.
    Refused,
}

/// A person paired with the node, or whose home it is, as the mailbox
/// needs them: what their record holds of their mail (`bark-link`
/// `phones::Record`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Person {
    /// Their record's number: a new number is a new person.
    pub number: u16,
    /// Their recipient tag at the node, once their phone wrote it: their
    /// items are those with it.
    pub tag: Option<u8>,
    /// Their mailbox key, while the node is their home.
    pub key: Option<[u8; KEY_LEN]>,
    /// The newest item when their record was made: their bit in the
    /// marks of items up to it was another's, and counts for nothing.
    pub from: u32,
    /// The last of their items their phone confirmed.
    pub read: u32,
    /// The `sent` of the last fetch they took.
    pub last_fetch: u32,
}

/// The fetch the mailbox answers for a person.
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
    /// The person's items unsent after the batch when the fetch was taken.
    remaining: u16,
    /// An item given back, with its place, to send again.
    again: Option<(u32, u32)>,
    /// A reply of the round has been handed to the core.
    answered: bool,
}

impl Reply {
    fn due(&self) -> bool {
        self.again.is_some() || self.next >= self.bottom || !self.answered
    }
}

/// A reply handed to the core and not yet past its first hop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InFlight {
    msg_id: MsgId,
    /// Whose round it is of.
    person: usize,
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
    /// Whose round it is of.
    pub person: usize,
}

/// A person's oldest unread item and the count of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Unread {
    /// The item's number and the message ID it arrived with.
    first: Option<(u32, MsgId)>,
    count: u32,
    /// The newest item looked at.
    upto: u32,
}

/// The mailbox of a node.
pub struct Mailbox<R: Ring> {
    ring: R,
    people: [Option<Person>; PEOPLE],
    replies: [Option<Reply>; PEOPLE],
    /// The person whose round last handed a reply to the core: the next
    /// reply is another's, in turn.
    turn: usize,
    in_flight: Option<InFlight>,
    /// The highest number kept; 0 before any.
    newest: u32,
    /// Each person's oldest unread item, its number and ID, and the count
    /// of them, while nothing has changed since: found by reading each
    /// later item's tag, which the node's loop asks for at every step.
    unread: [Cell<Option<Unread>>; PEOPLE],
}

impl<R: Ring> Mailbox<R> {
    /// The mailbox in `ring`, as it was before a reboot; its people come
    /// from their records ([`Mailbox::set_person`]).
    pub fn new(ring: R) -> Mailbox<R> {
        let mut m = Mailbox {
            ring,
            people: [None; PEOPLE],
            replies: [None; PEOPLE],
            turn: PEOPLE - 1,
            in_flight: None,
            newest: 0,
            unread: Default::default(),
        };
        // A whole slot counts only in its own place: one that is not, left
        // by a ring of another size, would set the newest number past slots
        // that are not erased, and the next item written over one of them
        // would fail its check.
        for s in 0..m.slots() {
            if let Some((n, _)) = m.slot(s).filter(|&(n, _)| m.slot_of(n) == s) {
                m.newest = m.newest.max(n);
            }
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

    /// The person at place `i`, from their record, or none. A person of
    /// another number than the one there is another person: their round
    /// ends, as does one whose key changed.
    pub fn set_person(&mut self, i: usize, person: Option<Person>) {
        let was = self.people[i];
        if was == person {
            return;
        }
        let same = was.zip(person).is_some_and(|(w, p)| w.number == p.number);
        let keyed = was.and_then(|w| w.key) == person.and_then(|p| p.key);
        if !same || !keyed {
            self.replies[i] = None;
            if self.in_flight.is_some_and(|f| f.person == i) {
                self.in_flight = None;
            }
        }
        self.people[i] = person;
        self.unread[i].set(None);
    }

    /// The person at place `i`, with what the mailbox has moved on.
    pub fn person(&self, i: usize) -> Option<Person> {
        self.people[i]
    }

    /// The oldest of person `i`'s items their phone has not confirmed: its
    /// number, the message ID it arrived with and its length in `out`
    /// (spec "Receiving").
    pub fn unread(&self, i: usize, out: &mut [u8; MAX_PAYLOAD]) -> Option<(u32, MsgId, usize)> {
        let (n, _) = self.unread_of(i).first?;
        self.read_item(n, out).map(|(id, l)| (n, id, l))
    }

    /// The number and the message ID of person `i`'s oldest unread item,
    /// without reading it.
    pub fn unread_head(&self, i: usize) -> Option<(u32, MsgId)> {
        self.unread_of(i).first
    }

    /// Person `i`'s items their phone has not confirmed.
    pub fn unread_count(&self, i: usize) -> u32 {
        self.unread_of(i).count
    }

    /// Person `i`'s oldest unread item and the count of them.
    /// Items written since it was last found are looked at then.
    fn unread_of(&self, i: usize) -> Unread {
        let shown = self.shown();
        let mut u = self.unread[i].get().unwrap_or(Unread {
            first: None,
            count: 0,
            upto: 0,
        });
        if u.upto == shown {
            return u;
        }
        if let Some(t) = self.people[i].and_then(|p| p.tag.map(|t| (t, p.read))) {
            let (tag, read) = t;
            let from = (read + 1).max(self.lowest()).max(u.upto + 1);
            for n in (from..=shown).filter(|&n| self.holds(n, tag)) {
                if u.first.is_none() {
                    u.first = self
                        .read_item(n, &mut [0u8; MAX_PAYLOAD])
                        .map(|(id, _)| (n, id));
                }
                u.count += 1;
            }
        }
        u.upto = shown;
        self.unread[i].set(Some(u));
        u
    }

    /// The newest item a phone may read: the newest whose slot is
    /// written, those after it held only in the ring's queue. Read before
    /// then, an item could be confirmed, or a guest's number moved past
    /// it, and a reboot that lost its slot would give its number to the
    /// next item, which would then count as read.
    fn shown(&self) -> u32 {
        let mut n = self.newest;
        while n >= self.lowest() && n > 0 && !self.ring.written(self.slot_of(n)) {
            n -= 1;
        }
        n
    }

    /// Whether the ring holds item `n` whole and for `tag`: its tag byte
    /// first, which rules most items out at the cost of a byte, then the
    /// slot, since an erased one reads tag 0xFF.
    fn holds(&self, n: u32, tag: u8) -> bool {
        self.tag_of(n) == tag
            && self
                .slot(self.slot_of(n))
                .is_some_and(|(got, len)| got == n && len > seal::RECIPIENT_AT)
    }

    /// The recipient tag item `n` carries, read from its slot alone.
    fn tag_of(&self, n: u32) -> u8 {
        self.ring
            .read_byte(self.slot_of(n), PAYLOAD_AT + seal::RECIPIENT_AT)
    }

    /// The oldest item after `after` whose recipient tag is `tag`: its
    /// number, the message ID it arrived with and its length in `out`. A
    /// guest reads its mail so, keeping `after` itself. A number above the
    /// newest is from before the ring began again and reads as 0.
    pub fn for_tag(
        &self,
        tag: u8,
        after: u32,
        out: &mut [u8; MAX_PAYLOAD],
    ) -> Option<(u32, MsgId, usize)> {
        let after = if after > self.newest { 0 } else { after };
        let from = after.saturating_add(1).max(self.lowest());
        (from..=self.shown())
            .filter(|&n| self.tag_of(n) == tag)
            .find_map(|n| {
                self.read_item(n, out)
                    .filter(|&(_, len)| out[..len].get(seal::RECIPIENT_AT) == Some(&tag))
                    .map(|(id, len)| (n, id, len))
            })
    }

    /// Person `i`'s phone has item `n` and every one of theirs before it,
    /// so no fetch of theirs sends them. Whether their last confirmed
    /// moved, for their record to keep.
    pub fn confirm_read(&mut self, i: usize, n: u32) -> bool {
        let Some(p) = self.people[i].as_mut() else {
            return false;
        };
        if n <= p.read || n > self.newest {
            return false;
        }
        p.read = n;
        self.mark(i, n);
        self.unread[i].set(None);
        true
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

    /// The number and the payload's length in slot `s`, if it holds a
    /// whole item.
    fn slot(&self, s: u32) -> Option<(u32, usize)> {
        let mut b = [0u8; SLOT_LEN];
        self.ring.read(s, &mut b);
        let check = be(&b[CHECK_AT..]);
        let len = usize::from(b[seal::NUMBER_LEN]);
        let n = be(&b[..seal::NUMBER_LEN]);
        (slot_check(&b) == check && len <= MAX_PAYLOAD && n != 0).then_some((n, len))
    }

    /// Whether item `n` is in the ring, person `i`'s and not yet sent to
    /// them. The mark is read first: most slots a batch passes over are
    /// not.
    fn is_unsent(&self, i: usize, n: u32) -> bool {
        let Some(p) = self.people[i] else {
            return false;
        };
        if n <= p.from || n < self.lowest() || n > self.newest {
            return false;
        }
        let s = self.slot_of(n);
        self.ring.read_byte(s, MARK_AT) & (1 << i) != 0
            && self.slot(s).is_some_and(|(got, _)| got == n)
    }

    /// Clears person `i`'s bit of item `n`, if it is set.
    fn mark(&mut self, i: usize, n: u32) {
        if self.is_unsent(i, n) {
            self.ring.clear(self.slot_of(n), MARK_AT, 1 << i);
        }
    }

    /// Person `i`'s items in the ring not yet sent to them, counted down
    /// from `top`.
    fn unsent_below(&self, i: usize, top: u32) -> u32 {
        (self.lowest()..=top)
            .filter(|&n| self.is_unsent(i, n))
            .count() as u32
    }

    /// Whether the node is anyone's home.
    pub fn is_home(&self) -> bool {
        self.people.iter().flatten().any(|p| p.key.is_some())
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
        let (got, len) = self.slot(s)?;
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
        if Form::of(payload) == Ok(Form::Fetch) {
            return self.fetch(payload);
        }
        if payload.len() > MAX_PAYLOAD || self.newest == MAX_NUMBER {
            return Arrival::Refused;
        }
        let n = self.newest + 1;
        let s = self.slot_of(n);
        if s.is_multiple_of(SLOTS_PER_PAGE) {
            self.ring.erase(s / SLOTS_PER_PAGE);
            for u in &self.unread {
                u.set(None);
            }
        }
        let tag = seal::recipient_of(payload);
        let mark = (0..PEOPLE)
            .filter(|&i| tag.is_some() && self.people[i].is_some_and(|p| p.tag == tag))
            .fold(0u8, |m, i| m | 1 << i);
        let mut b = [0xffu8; SLOT_LEN];
        b[..seal::NUMBER_LEN].copy_from_slice(&n.to_be_bytes()[1..]);
        b[seal::NUMBER_LEN] = payload.len() as u8;
        b[PAYLOAD_AT..PAYLOAD_AT + payload.len()].copy_from_slice(payload);
        b[ID_AT..CHECK_AT].copy_from_slice(&msg_id.0.to_be_bytes());
        let check = slot_check(&b);
        b[CHECK_AT..].copy_from_slice(&check.to_be_bytes());
        // The check reads the mark as erased.
        b[MARK_AT] = mark;
        self.ring.write(s, &b);
        self.newest = n;
        // Each person whose tag it carries has one more unread once it is
        // written ([`Mailbox::shown`]), found then ([`Mailbox::unread_of`]).
        Arrival::Kept(n)
    }

    /// A fetch: taken by the first person whose key opens it, if it is
    /// later than the last they took (spec "Mailbox").
    fn fetch(&mut self, payload: &[u8]) -> Arrival {
        let opened = (0..PEOPLE).find_map(|i| {
            let key = self.people[i]?.key?;
            Fetch::open(&key, payload).ok().map(|f| (i, f))
        });
        match opened {
            Some((i, f)) if self.people[i].is_some_and(|p| f.sent > p.last_fetch) => {
                self.take(i, f);
                Arrival::Fetch(i)
            }
            _ => Arrival::Refused,
        }
    }

    fn take(&mut self, i: usize, f: Fetch) {
        let lowest = self.lowest();
        let mut count = 0;
        let mut bottom = self.newest + 1;
        let mut n = self.newest;
        while count < MAX_REPLIES && n >= lowest {
            if self.is_unsent(i, n) {
                count += 1;
                bottom = n;
            }
            n -= 1;
        }
        let remaining = self.unsent_below(i, bottom.saturating_sub(1));
        if let Some(p) = self.people[i].as_mut() {
            p.last_fetch = f.sent;
        }
        self.replies[i] = Some(Reply {
            round: f.sent,
            address: f.address,
            recipient: f.recipient,
            next: self.newest,
            bottom,
            handed: 0,
            remaining: u16::try_from(remaining).unwrap_or(u16::MAX),
            again: None,
            answered: false,
        });
        if self.in_flight.is_some_and(|f| f.person == i) {
            self.in_flight = None;
        }
    }

    /// Whether a reply may be due: [`Mailbox::next`] then gives it, or
    /// finds that the person's phone has read the rest meanwhile, after
    /// which none is due.
    pub fn reply_due(&self) -> bool {
        (0..PEOPLE).any(|i| self.due(i))
    }

    fn due(&self, i: usize) -> bool {
        self.people[i].is_some_and(|p| p.key.is_some()) && self.replies[i].is_some_and(|r| r.due())
    }

    /// The next reply to hand the core, sealed into `out`, if one is due;
    /// call it while `accepts_send` holds, and send what it gives. The
    /// rounds of several people take turns.
    pub fn next(&mut self, out: &mut [u8; MAX_PAYLOAD]) -> Option<Outgoing> {
        for k in 1..=PEOPLE {
            let i = (self.turn + k) % PEOPLE;
            if !self.due(i) {
                continue;
            }
            if let Some(o) = self.next_of(i, out) {
                self.turn = i;
                return Some(o);
            }
        }
        None
    }

    fn next_of(&mut self, i: usize, out: &mut [u8; MAX_PAYLOAD]) -> Option<Outgoing> {
        let key = self.people[i]?.key?;
        let mut r = self.replies[i]?;
        let mut sealed = None;
        if let Some((n, place)) = r.again.take() {
            sealed = self.seal_item(&key, &r, n, place, out);
        }
        while sealed.is_none() && r.next >= r.bottom {
            let n = r.next;
            r.next -= 1;
            if self.is_unsent(i, n) {
                sealed = self.seal_item(&key, &r, n, r.handed + 1, out);
                r.handed += 1;
                self.mark(i, n);
            }
        }
        if sealed.is_none() && !r.answered {
            let nothing = seal::seal_nothing_new(&key, r.recipient, r.round);
            out[..nothing.len()].copy_from_slice(&nothing);
            sealed = Some((0, 0, nothing.len()));
        }
        // Kept with nothing to send too, so that no reply is due after.
        r.answered |= sealed.is_some();
        self.replies[i] = Some(r);
        let (number, place, len) = sealed?;
        let msg_id = MsgId::of(r.address.position, r.address.node, &out[..len]);
        self.in_flight = Some(InFlight {
            msg_id,
            person: i,
            number,
            place,
        });
        Some(Outgoing {
            address: r.address,
            number,
            len,
            msg_id,
            person: i,
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
        let Some(f) = self.in_flight.filter(|f| f.msg_id == msg_id) else {
            return;
        };
        if let Some(r) = self.replies[f.person].as_mut() {
            if f.number == 0 {
                r.answered = false;
            } else {
                r.again = Some((f.number, f.place));
            }
        }
        self.in_flight = None;
    }
}

/// A ring in RAM of `SLOTS` slots, a whole number of pages, for the
/// simulator and the tests.
pub struct RamRing<const SLOTS: usize> {
    slots: [[u8; SLOT_LEN]; SLOTS],
}

impl<const SLOTS: usize> RamRing<SLOTS> {
    /// An erased ring.
    pub fn new() -> Self {
        assert!(
            SLOTS.is_multiple_of(SLOTS_PER_PAGE as usize) && SLOTS >= 2 * SLOTS_PER_PAGE as usize
        );
        RamRing {
            slots: [[0xff; SLOT_LEN]; SLOTS],
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
    fn clear(&mut self, slot: u32, at: usize, bits: u8) {
        self.slots[slot as usize][at] &= !bits;
    }
    fn erase(&mut self, page: u32) {
        let first = (page * SLOTS_PER_PAGE) as usize;
        for s in &mut self.slots[first..first + SLOTS_PER_PAGE as usize] {
            *s = [0xff; SLOT_LEN];
        }
    }
}

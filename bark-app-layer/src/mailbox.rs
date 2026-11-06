//! The home node's mailbox, an application on the node beside the phone
//! link (spec "Mailbox"). It reads what the core delivers to the node's
//! application (`AppEvent::Message`, the payload from `Core::inbox`),
//! offers replies as `AppCommand::Send` while `accepts_send` holds, and
//! takes a reply back on `AppEvent::GivenBack`; it changes nothing in the
//! core, and the simulator can run it on a [`Ring`] in RAM.
//!
//! The node holds the mailbox key and cannot open the person's mail: it
//! keeps every payload but a fetch, numbered, and answers a fetch once:
//! the items it holds after the fetch's number, each sealed again under
//! the key (`seal::seal_reply`), or one reply that says nothing is new
//! (`seal::seal_nothing_new`). An item that arrives later waits for the
//! next fetch.

use bark_proto::MsgId;

use crate::keys::KEY_LEN;
use crate::seal::{self, Fetch, Form, MAX_NUMBER, MAX_REPLIES};
use crate::{be, Address, MAX_PAYLOAD};

/// Bytes of a slot: an erase page of 4096 holds 16.
pub const SLOT_LEN: usize = 256;
/// Slots in an erase page (nRF52840 Product Specification, NVMC: a page
/// is 4096 bytes).
pub const SLOTS_PER_PAGE: u32 = 4096 / SLOT_LEN as u32;

/// Where a slot's check sits: the last four bytes, over all before them.
const CHECK_AT: usize = SLOT_LEN - 4;
/// Where a slot's payload starts: after its number (3) and length (1).
const PAYLOAD_AT: usize = seal::NUMBER_LEN + 1;
/// Where a slot's message ID sits: the four bytes before the check, so
/// that the payload's place is the same in a slot written before it was
/// kept, whose ID reads as `0xFFFF_FFFF`, the erased flash.
const ID_AT: usize = CHECK_AT - 4;
const _: () = assert!(PAYLOAD_AT + MAX_PAYLOAD <= ID_AT);

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
    /// Erases page `page`.
    fn erase(&mut self, page: u32);
    /// The state last kept, if any.
    fn state(&self) -> Option<[u8; STATE_LEN]>;
    /// Keeps `state` across a reboot.
    fn keep_state(&mut self, state: &[u8; STATE_LEN]);
}

fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5, |h, &b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
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
    /// The node is nobody's home: nothing is kept.
    NotHome,
}

/// The fetch the mailbox answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Reply {
    /// The fetch's `sent`: the replies' round.
    round: u32,
    /// Where they go.
    address: Address,
    /// The round's first item, at place 1.
    first: u32,
    /// The next item to send.
    next: u32,
    /// The newest item when the fetch was taken, the round's last.
    last: u32,
    /// A reply of the round has been handed to the core.
    answered: bool,
}

/// A reply handed to the core and not yet past its first hop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InFlight {
    msg_id: MsgId,
    /// The item, or 0 for the reply that says nothing is new.
    number: u32,
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
    /// The highest number the phone at the node has confirmed reading;
    /// 0 before any (spec "Phone link", Receiving).
    read: u32,
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
            read: 0,
        };
        m.newest = (0..m.slots())
            .filter_map(|s| m.slot(s).map(|(n, _)| n))
            .max()
            .unwrap_or(0);
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
    /// (`flash::FlashRing`).
    pub fn ring_mut(&mut self) -> &mut R {
        &mut self.ring
    }

    /// The oldest item the phone at the node has not confirmed: its number,
    /// the message ID it arrived with and its length in `out`. The phone
    /// reads the ring from where it last confirmed; the ring keeps every
    /// item for fetches either way.
    pub fn unread(&self, out: &mut [u8; MAX_PAYLOAD]) -> Option<(u32, MsgId, usize)> {
        let from = (self.read + 1).max(self.oldest());
        (from..=self.newest).find_map(|n| self.read_item(n, out).map(|(id, l)| (n, id, l)))
    }

    /// Items the phone at the node has not confirmed.
    pub fn unread_count(&self) -> u32 {
        self.newest
            - self
                .read
                .max(self.oldest().saturating_sub(1))
                .min(self.newest)
    }

    /// The phone at the node has item `n`, and every one before it.
    pub fn confirm_read(&mut self, n: u32) {
        if n > self.read && n <= self.newest {
            self.read = n;
            self.keep();
        }
    }

    fn slots(&self) -> u32 {
        self.ring.pages() * SLOTS_PER_PAGE
    }

    /// The number and the payload's length in slot `s`, if it holds a
    /// whole item.
    fn slot(&self, s: u32) -> Option<(u32, usize)> {
        let mut b = [0u8; SLOT_LEN];
        self.ring.read(s, &mut b);
        let check = be(&b[CHECK_AT..]);
        let len = usize::from(b[seal::NUMBER_LEN]);
        let n = be(&b[..seal::NUMBER_LEN]);
        (fnv1a(&b[..CHECK_AT]) == check && len <= MAX_PAYLOAD && n != 0).then_some((n, len))
    }

    /// Whether the node is someone's home.
    pub fn is_home(&self) -> bool {
        self.key.is_some()
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
        let first = self.newest.saturating_sub(self.slots() - 1).max(1);
        (first..=self.newest)
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
        let s = (n - 1) % self.slots();
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
        let Some(key) = self.key else {
            return Arrival::NotHome;
        };
        if Form::of(payload) == Ok(Form::Fetch) {
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
        let s = (n - 1) % self.slots();
        if s.is_multiple_of(SLOTS_PER_PAGE) {
            self.ring.erase(s / SLOTS_PER_PAGE);
        }
        let mut b = [0xffu8; SLOT_LEN];
        b[..seal::NUMBER_LEN].copy_from_slice(&n.to_be_bytes()[1..]);
        b[seal::NUMBER_LEN] = payload.len() as u8;
        b[PAYLOAD_AT..PAYLOAD_AT + payload.len()].copy_from_slice(payload);
        b[ID_AT..CHECK_AT].copy_from_slice(&msg_id.0.to_be_bytes());
        let check = fnv1a(&b[..CHECK_AT]);
        b[CHECK_AT..].copy_from_slice(&check.to_be_bytes());
        self.ring.write(s, &b);
        self.newest = n;
        Arrival::Kept(n)
    }

    fn take(&mut self, f: Fetch) {
        let last = self.newest;
        // A ring larger than a round sends its newest MAX_REPLIES.
        let first = (f.since + 1)
            .max(self.oldest())
            .max((last + 1).saturating_sub(MAX_REPLIES));
        self.last_fetch = f.sent;
        self.reply = Some(Reply {
            round: f.sent,
            address: f.address,
            first,
            next: first,
            last,
            answered: false,
        });
        self.in_flight = None;
        self.keep();
    }

    /// Whether a reply may be due: [`Mailbox::next`] then gives it, or
    /// skips past items no longer in the ring.
    pub fn reply_due(&self) -> bool {
        self.key.is_some() && self.reply.is_some_and(|r| r.next <= r.last || !r.answered)
    }

    /// The next reply to hand the core, sealed into `out`, if one is due;
    /// call it while `accepts_send` holds, and send what it gives. A round
    /// with no item to send gets one reply that says nothing is new.
    pub fn next(&mut self, out: &mut [u8; MAX_PAYLOAD]) -> Option<Outgoing> {
        let key = self.key?;
        let mut r = self.reply?;
        let mut item = [0u8; MAX_PAYLOAD];
        let mut sealed = None;
        while sealed.is_none() && r.next <= r.last {
            let n = r.next;
            r.next += 1;
            let Some(len) = self.item(n, &mut item) else {
                continue;
            };
            let place = n - r.first + 1;
            // An item longer than the app writes cannot come out.
            sealed = seal::seal_reply(&key, r.round, place, n, &item[..len], out)
                .ok()
                .map(|m| (n, m));
        }
        if sealed.is_none() && !r.answered {
            let nothing = seal::seal_nothing_new(&key, r.round);
            out[..nothing.len()].copy_from_slice(&nothing);
            sealed = Some((0, nothing.len()));
        }
        let (number, len) = sealed?;
        r.answered = true;
        self.reply = Some(r);
        let msg_id = MsgId::of(r.address.position, r.address.node, &out[..len]);
        self.in_flight = Some(InFlight { msg_id, number });
        Some(Outgoing {
            address: r.address,
            number,
            len,
            msg_id,
        })
    }

    /// The core gave the message back: if it is the last reply, it goes
    /// again when the node next takes one.
    pub fn given_back(&mut self, msg_id: MsgId) {
        if let (Some(f), Some(r)) = (self.in_flight, self.reply.as_mut()) {
            if f.msg_id == msg_id {
                if f.number == 0 {
                    r.answered = false;
                } else {
                    r.next = r.next.min(f.number);
                }
                self.in_flight = None;
            }
        }
    }

    /// The state: the key, the last fetch and where its replies go. The
    /// next reply is not kept, to spare a write a reply: after a reboot
    /// the round starts again from its first, and the requester drops
    /// what it has.
    fn keep(&mut self) {
        let mut s = [0u8; STATE_LEN];
        if let Some(k) = self.key {
            s[0] = 1;
            s[1..33].copy_from_slice(&k);
        }
        s[33..37].copy_from_slice(&self.last_fetch.to_be_bytes());
        if let Some(r) = self.reply {
            s[37] = 1;
            s[38..38 + Address::LEN].copy_from_slice(&r.address.encode());
            s[51..54].copy_from_slice(&r.first.to_be_bytes()[1..]);
            s[54..57].copy_from_slice(&r.last.to_be_bytes()[1..]);
        }
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
        if s[37] == 1 {
            if let Ok(address) = Address::decode(&s[38..]) {
                let first = be(&s[51..54]);
                self.reply = Some(Reply {
                    round: self.last_fetch,
                    address,
                    first,
                    next: first,
                    last: be(&s[54..57]),
                    answered: false,
                });
            }
        }
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

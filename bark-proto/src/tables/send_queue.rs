//! The send queue (spec "Send queue"): packets waiting for their hand-over, those
//! being handed over included, in the order they arrived.
//!
//! **Regular room.** A relay takes a packet into it only if the airtime of
//! everything queued, that packet included, fits in its duty bucket as it
//! stands (spec "Receiving DATA", roadmap D29). Its length follows: the bucket's depth,
//! 6 s, over the time of one full frame and its ACK, `138.56 + 16.96 =
//! 155.52 ms`: ⌈38.6⌉ = 39. Each packet taken also costs the ACK that
//! takes it, so from a full bucket with the reply reserve left in at most
//! 37 full frames fit, and for full frames the bucket binds first. Short
//! frames fill the 39 places first: 39 hand-overs of them still go out
//! in a few seconds.
//!
//! **Hop places.** Beyond the regular room there is one place for each
//! hop count. A packet received with count `h` that the regular room
//! refuses takes the highest free place numbered `h` or lower, whatever
//! the bucket holds; the bucket still decides when it is sent. A packet
//! from the node's own application, count 0, has none, so
//! overload pushes back where messages enter (roadmap D31). The places
//! keep the network from locking up while nothing is dropped for want of
//! room: take the highest-numbered place held anywhere, `k`. Its message
//! has made at least `k` hops and sends `k + 1` or more, and place `k + 1`
//! is free everywhere, so its next hop can take it, unless the count is at
//! the cap, where the message ends (spec "Hop count"). By induction downwards, every
//! message held moves.
//!
//! Each entry is a frame, what kind of hand-over it is, which decides
//! what happens when the hand-over fails (spec "Hand-over"), and where
//! its hand-over is ([`State`], `node/transfer.rs`). An entry whose
//! receivers are all banned waits in its place until a ban ends (spec
//! "Overload waits"). The next hand-over goes to a DATA to a sleepy node
//! if one is due, since its node listens only briefly (spec "Delivery");
//! else to the first entry that is due, except that one that has waited
//! goes first, the most travelled first: the place one higher is free
//! only for the message that has come farthest, and a lower one offered
//! first would be refused and ban the neighbour again. Choosing an entry
//! does not move it: the hand-over refers to it where it stands, so of
//! equals the one that arrived first is offered first, however often they
//! wait (roadmap D31 rule 4). One set aside for the delivery queue keeps
//! its place and is never due.
//!
//! **Flights.** A hand-over that has begun keeps, beside its place, what
//! the place has no room for ([`Flight`]): until when it waits, since
//! when its checks have found the channel busy, and the neighbours it
//! passed over. At most [`FLIGHTS`] hand-overs are under way at once; one
//! of them is on the radio, the rest wait between their attempts (spec
//! "Waiting hand-overs").

use crate::event::{Frame, MAX_FRAME};
use crate::ids::MsgId;
use crate::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS};
use crate::tables::budget::DEPTH;
use crate::tables::fixed::FixedVec;
use crate::tables::records::MAX_HOPS;
use crate::tables::{retry_window, ACK_TIMEOUT, HOP_ATTEMPTS, RETRY_BACKOFF};
use crate::time::{Duration, Instant};
use crate::wire::Packet;
use crate::wire::ACK_LEN;

/// Queue length for a bucket of `depth`; see the module documentation.
pub const fn queue_for(depth: Duration) -> usize {
    let per_frame = time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS).as_micros()
        + time_on_air(ACK_LEN as u8, MIN_PREAMBLE_SYMBOLS).as_micros();
    depth.as_micros().div_ceil(per_frame) as usize
}

/// Queue length for the design's bucket (spec "Duty cycle cap").
pub const SEND_QUEUE: usize = queue_for(DEPTH);

const _: () = assert!(SEND_QUEUE == 39);

/// Hop places: one for each count a packet can be received with and still
/// be sent on, 1 to 254; a packet received at 255 ends at the hop cap.
pub const HOP_PLACES: usize = MAX_HOPS as usize - 1;

/// How many hand-overs may be under way at once: the one on the radio
/// and those waiting between their attempts (spec "Waiting hand-overs").
/// Enough that the radio has a hand-over to serve through the longest
/// retry wait: that wait is at most `2^(attempts − 1)` retry units,
/// 622 ms, and a hand-over that transmits holds the radio at least a
/// full frame and the ACK timeout, 238.6 ms, so three can be served in
/// one such wait; with the waiter, four.
pub const FLIGHTS: usize = 1 + flights_in(retry_window(RETRY_BACKOFF, HOP_ATTEMPTS - 1));

/// How many hand-overs of a full frame the radio can serve in `wait`.
const fn flights_in(wait: Duration) -> usize {
    let service =
        time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS).as_micros() + ACK_TIMEOUT.as_micros();
    wait.as_micros().div_ceil(service) as usize
}

const _: () = assert!(FLIGHTS == 4);

/// What a queued frame is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A DATA to the next hop toward the destination (spec "Next hop"). Its next
    /// hop is chosen when its hand-over starts, and again after a failure.
    Forward,
    /// A DATA back to the parent (spec "Return to parent"); dropped if it fails.
    Return,
    /// A DATA to a sleepy node this node holds a message for (spec "Delivery"); left in the delivery queue
    /// if it fails.
    Deliver,
    /// A DATA from the delivery queue to its node whether or not it is in
    /// a window: once with the long preamble after a far fix, or with the
    /// ordinary one to a node heard as one that is not sleepy (spec
    /// "Delivery"). Left in the delivery queue if it fails.
    Direct,
}

impl Kind {
    /// Whether it carries a message from the delivery queue to its node,
    /// which goes ahead of the rest (spec "Delivery").
    pub fn delivers(self) -> bool {
        matches!(self, Kind::Deliver | Kind::Direct)
    }
}

/// Where a queued frame is in its hand-over (spec "Hand-over",
/// `node/transfer.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Waiting its turn, due at `not_before`.
    Queued,
    /// The application's message for a node this one holds for, set aside
    /// until the delivery queue has a place (spec "Return to parent"); it
    /// keeps its place and its airtime in admission.
    Held,
    /// Addressed, and waiting for this: until the instant its flight
    /// keeps, or for its channel, until no hand-over under way waits on
    /// it.
    Waiting(Wait),
    /// On the radio: its channel checked, then its frame on air.
    Radio,
    /// Listening at home for its ACK until the instant its flight keeps.
    AwaitAck,
}

/// What a hand-over waits for before its listen-before-talk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wait {
    /// Its turn: nothing, once the radio is free.
    Turn,
    /// A random backoff after a busy check.
    Backoff,
    /// An owed reply, which goes first.
    Reply,
    /// The duty-cycle budget.
    Budget,
    /// The retry wait after a missed ACK.
    Retry,
    /// A channel another hand-over under way waits on, for its backoff
    /// or its retry (spec "Waiting hand-overs"). It has no flight: once
    /// the channel is free it is queued and addressed again.
    Channel,
}

/// A queued frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outgoing {
    /// The frame as it will be sent; a forward's address is filled in when
    /// its hand-over starts, and every frame's channel before each attempt.
    pub frame: Frame,
    /// What it is for.
    pub kind: Kind,
    /// Where its hand-over is.
    pub state: State,
    /// Not handed over before this: it waits for a ban to end.
    pub not_before: Instant,
    /// Transmissions that went unanswered: of the hand-over under way,
    /// and of a return or a DATA to a sleepy node kept while it waits; a
    /// refusal does not count.
    pub tries: u8,
    /// The hop place it holds; 0 for the regular room.
    pub place: u8,
    /// When a DATA's time left reaches zero (spec "Time to live");
    /// [`Instant::MAX`] for a DATA to a sleepy node, which the delivery
    /// queue's TTL ends.
    pub expires: Instant,
}

impl Outgoing {
    /// `frame` for `kind`, due at once.
    pub const fn new(frame: Frame, kind: Kind) -> Outgoing {
        Outgoing {
            frame,
            kind,
            state: State::Queued,
            not_before: Instant::ZERO,
            tries: 0,
            place: 0,
            expires: Instant::MAX,
        }
    }

    /// Whether it is set aside for the delivery queue.
    pub fn held(&self) -> bool {
        self.state == State::Held
    }

    /// Whether its hand-over has begun: addressed and waiting, on the
    /// radio or waiting for its ACK.
    pub fn in_flight(&self) -> bool {
        !matches!(self.state, State::Queued | State::Held)
    }

    /// Whether it waits between the attempts of its hand-over: for a
    /// backoff, a retry, the bucket or its channel. Its turn and an owed
    /// reply are no wait.
    pub fn waits(&self) -> bool {
        matches!(
            self.state,
            State::Waiting(Wait::Backoff | Wait::Retry | Wait::Budget | Wait::Channel)
        )
    }

    /// The hop count the frame carries.
    pub fn hops(&self) -> u8 {
        match Packet::decode(self.frame.payload()) {
            Ok(Packet::Data(d)) => d.hops,
            _ => 0,
        }
    }

    /// Whether it has waited for a ban.
    fn waited(&self) -> bool {
        self.not_before > Instant::ZERO
    }

    /// The message the frame carries: a DATA's computed ID (roadmap D15).
    pub fn msg_id(&self) -> Option<MsgId> {
        match Packet::decode(self.frame.payload()) {
            Ok(Packet::Data(d)) => Some(d.msg_id()),
            _ => None,
        }
    }
}

/// What the queue keeps of a hand-over under way beside its place, which
/// has no room for it: until when it waits or listens for its ACK, since
/// when the checks of its attempt have found the channel busy, and the
/// neighbours it passed over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flight {
    /// The entry whose hand-over this is.
    place: usize,
    /// Until when it waits, or its ACK is due.
    pub until: Instant,
    /// `None` until a check of this attempt.
    pub lbt_since: Option<Instant>,
    /// Neighbour slots a forward was routed past: listen-before-talk gave
    /// up on them, they refused it, or its attempts went unanswered
    /// (spec "Hand-over"); not tried.
    pub skipped: u64,
}

impl Flight {
    /// No flight: the queue's filler, and a test's start.
    pub const NONE: Flight = Flight {
        place: 0,
        until: Instant::ZERO,
        lbt_since: None,
        skipped: 0,
    };

    /// The entry whose hand-over this is.
    pub fn place(&self) -> usize {
        self.place
    }
}

/// The send queue is full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;

/// The send queue.
///
/// Its few fields first, then the items: an event reads the first and
/// seldom the items ([`crate::Node`]'s layout).
#[derive(Clone)]
#[repr(C)]
pub struct SendQueue {
    /// The entry last chosen to be addressed.
    current: Option<usize>,
    /// The hand-overs under way, in the order they began.
    flights: FixedVec<Flight, FLIGHTS>,
    frames: FixedVec<Outgoing, { SEND_QUEUE + HOP_PLACES }>,
}

impl SendQueue {
    /// Empty.
    pub const fn new() -> SendQueue {
        let channel = match Channel::new(0) {
            Some(c) => c,
            None => unreachable!(),
        };
        let frame = Frame {
            channel,
            preamble_symbols: 0,
            len: 0,
            bytes: [0; MAX_FRAME],
        };
        SendQueue {
            frames: FixedVec::new(Outgoing::new(frame, Kind::Return)),
            flights: FixedVec::new(Flight::NONE),
            current: None,
        }
    }

    /// Number of queued frames.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Whether nothing is queued.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Empty again, in place.
    pub fn clear(&mut self) {
        self.frames.clear();
        self.flights.clear();
        self.current = None;
    }

    /// Entries in the regular room.
    pub fn regular(&self) -> usize {
        self.iter().filter(|o| o.place == 0).count()
    }

    /// The highest free hop place for a packet received with `hops`,
    /// counting `freed` as free.
    pub fn free_place(&self, hops: u8, freed: Option<u8>) -> Option<u8> {
        let mut taken = [false; HOP_PLACES + 1];
        for o in self.iter() {
            taken[o.place as usize] = true;
        }
        if let Some(k) = freed {
            taken[k as usize] = false;
        }
        let top = (hops as usize).min(HOP_PLACES);
        (1..=top).rev().find(|&k| !taken[k]).map(|k| k as u8)
    }

    /// The lowest free hop place: a packet received with this count or
    /// higher finds one.
    pub fn lowest_free_place(&self) -> Option<u8> {
        let mut taken = [false; HOP_PLACES + 1];
        for o in self.iter() {
            taken[o.place as usize] = true;
        }
        (1..=HOP_PLACES).find(|&k| !taken[k]).map(|k| k as u8)
    }

    /// The airtime of every queued frame, sent once each.
    pub fn airtime(&self) -> Duration {
        self.iter()
            .map(|o| time_on_air(o.frame.len, o.frame.preamble_symbols))
            .fold(Duration::ZERO, |a, b| a + b)
    }

    /// Queue `out` last. Admission (spec "Receiving DATA") keeps the regular room and the
    /// hop places within bounds, so only a caller that skipped it finds the
    /// queue full.
    pub fn push(&mut self, out: Outgoing) -> Result<(), Full> {
        self.frames.push(out).map_err(|_| Full)
    }

    /// Entry `i`.
    pub fn get(&self, i: usize) -> Option<&Outgoing> {
        self.frames.as_slice().get(i)
    }

    /// Entry `i`, to address or to let wait.
    pub fn get_mut(&mut self, i: usize) -> Option<&mut Outgoing> {
        self.frames.as_mut_slice().get_mut(i)
    }

    /// Make entry `i` the one being addressed. The queue keeps its order.
    pub fn select(&mut self, i: usize) {
        self.current = (i < self.len()).then_some(i);
    }

    /// The entry being addressed, if one was chosen.
    pub fn current_index(&self) -> Option<usize> {
        self.current
    }

    /// No entry is being addressed.
    pub fn deselect(&mut self) {
        self.current = None;
    }

    /// Take entry `i` out, as it stands: its hand-over, if under way, is
    /// over.
    pub fn remove(&mut self, i: usize) -> Option<Outgoing> {
        if i >= self.len() {
            return None;
        }
        let out = self.frames.remove(i);
        self.removed(i);
        Some(out)
    }

    /// Entry `i` is gone: what pointed past it moves down.
    fn removed(&mut self, i: usize) {
        self.current = match self.current {
            Some(c) if c == i => None,
            Some(c) if c > i => Some(c - 1),
            c => c,
        };
        self.flights.retain(|f| f.place != i);
        for f in self.flights.as_mut_slice() {
            if f.place > i {
                f.place -= 1;
            }
        }
    }

    // --- The flights ---

    /// The hand-overs under way, in the order they began.
    pub fn flights(&self) -> impl Iterator<Item = &Flight> {
        self.flights.as_slice().iter()
    }

    /// Whether another hand-over may begin.
    pub fn flight_free(&self) -> bool {
        !self.flights.is_full()
    }

    /// The flight of entry `i`, if its hand-over is under way.
    pub fn flight_of(&self, i: usize) -> Option<&Flight> {
        self.flights().find(|f| f.place == i)
    }

    /// Begin the hand-over of entry `i` at `now`, routed past `skipped`:
    /// false if the flights are full, or it has one.
    pub fn start_flight(&mut self, i: usize, now: Instant, skipped: u64) -> bool {
        if i >= self.len() || self.flight_of(i).is_some() {
            return false;
        }
        self.flights
            .push(Flight {
                place: i,
                until: now,
                lbt_since: None,
                skipped,
            })
            .is_ok()
    }

    /// The hand-over of entry `i` is over, whatever its state says.
    pub fn end_flight(&mut self, i: usize) {
        self.flights.retain(|f| f.place != i);
    }

    /// Entry `i` and its flight, to advance its hand-over.
    pub fn handover(&mut self, i: usize) -> Option<(&mut Outgoing, &mut Flight)> {
        let out = self.frames.as_mut_slice().get_mut(i)?;
        let flight = self
            .flights
            .as_mut_slice()
            .iter_mut()
            .find(|f| f.place == i)?;
        Some((out, flight))
    }

    /// The entry on the radio: its channel being checked, its frame on
    /// air, or listening at home for its ACK.
    pub fn on_radio(&self) -> Option<usize> {
        self.flights().map(|f| f.place).find(|&i| {
            matches!(
                self.get(i).map(|o| o.state),
                Some(State::Radio | State::AwaitAck)
            )
        })
    }

    /// Whether a hand-over under way waits on `channel`, for its backoff
    /// or its retry: another to that channel waits for it (spec "Waiting hand-overs").
    pub fn holds_channel(&self, channel: Channel) -> bool {
        self.flights().any(|f| {
            self.get(f.place).is_some_and(|o| {
                o.frame.channel == channel
                    && matches!(o.state, State::Waiting(Wait::Backoff | Wait::Retry))
            })
        })
    }

    /// The first entry that waited for its channel and may now be queued
    /// again.
    pub fn next_unblocked(&self) -> Option<usize> {
        self.iter().position(|o| {
            o.state == State::Waiting(Wait::Channel) && !self.holds_channel(o.frame.channel)
        })
    }

    /// The entry the next hand-over goes to: a DATA to a sleepy node that
    /// is due, the first; else of those that have waited and are due, the
    /// most travelled, the earliest of equals; else the first that is due.
    pub fn next_due(&self, now: Instant) -> Option<usize> {
        // Asked on every event; mostly there is nothing queued.
        if self.is_empty() {
            return None;
        }
        let due = |o: &Outgoing| o.state == State::Queued && o.not_before <= now;
        if let Some(i) = self.delivery_due(now) {
            return Some(i);
        }
        self.iter()
            .enumerate()
            .filter(|(_, o)| due(o) && o.waited())
            .max_by(|(i, a), (j, b)| a.hops().cmp(&b.hops()).then(j.cmp(i)))
            .map(|(i, _)| i)
            .or_else(|| self.iter().position(due))
    }

    /// The first DATA from the delivery queue that is due to begin.
    pub fn delivery_due(&self, now: Instant) -> Option<usize> {
        self.iter()
            .position(|o| o.kind.delivers() && o.state == State::Queued && o.not_before <= now)
    }

    /// The entry of message `id`, if it waits beyond `now`, is due at
    /// `now`.
    pub fn due_now(&mut self, id: MsgId, now: Instant) {
        for o in self.frames.as_mut_slice() {
            if !o.held() && o.not_before > now && o.msg_id() == Some(id) {
                o.not_before = now;
            }
        }
    }

    /// When the earliest entry whose hand-over has not begun is due.
    pub fn earliest_due(&self) -> Option<Instant> {
        self.iter()
            .filter(|o| o.state == State::Queued)
            .map(|o| o.not_before)
            .min()
    }

    /// Queued entries, in the order they arrived.
    pub fn iter(&self) -> impl Iterator<Item = &Outgoing> {
        self.frames.as_slice().iter()
    }

    /// Keep only the entries for which `keep` holds, in order; `keep` is
    /// told whether the entry's hand-over is under way, which ends with
    /// it.
    pub fn retain(&mut self, mut keep: impl FnMut(bool, &Outgoing) -> bool) {
        let mut i = 0;
        while i < self.len() {
            let under_way = self.flight_of(i).is_some();
            if keep(under_way, &self.frames.as_slice()[i]) {
                i += 1;
                continue;
            }
            self.frames.remove(i);
            self.removed(i);
        }
    }
}

impl Default for SendQueue {
    fn default() -> Self {
        SendQueue::new()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::tables::budget::REPLY_RESERVE;

    fn frame(tag: u8) -> Outgoing {
        let mut bytes = [0; MAX_FRAME];
        bytes[0] = tag;
        let frame = Frame {
            channel: Channel::new(3).unwrap(),
            preamble_symbols: MIN_PREAMBLE_SYMBOLS,
            len: 1,
            bytes,
        };
        Outgoing::new(frame, Kind::Return)
    }

    #[test]
    fn size_follows_the_bucket() {
        assert_eq!(queue_for(Duration::from_secs(6)), SEND_QUEUE);
        assert_eq!(queue_for(Duration::ZERO), 0);
        // What a full bucket takes in full frames, each with the ACK that
        // took it, leaving the reply reserve: fewer than the places.
        let per = time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS)
            + time_on_air(ACK_LEN as u8, MIN_PREAMBLE_SYMBOLS);
        let fit = (DEPTH - REPLY_RESERVE).as_micros() / per.as_micros();
        assert_eq!(fit, 37);
        assert!((fit as usize) < SEND_QUEUE);
    }

    /// The flights follow from the longest retry wait and the least a
    /// hand-over that transmits holds the radio (spec "Waiting hand-overs").
    #[test]
    fn flights_follow_from_the_retry_wait() {
        let wait = retry_window(RETRY_BACKOFF, 2);
        assert_eq!(wait.as_millis(), 622);
        let service = time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS) + ACK_TIMEOUT;
        assert_eq!(service.as_millis(), 238);
        assert_eq!(FLIGHTS, 1 + 3);
        assert_eq!(flights_in(Duration::ZERO), 0);
        assert_eq!(flights_in(service), 1);
    }

    #[test]
    fn airtime_counts_every_frame() {
        let mut q = SendQueue::new();
        assert_eq!(q.airtime(), Duration::ZERO);
        q.push(frame(1)).unwrap();
        q.push(frame(2)).unwrap();
        let one = time_on_air(1, MIN_PREAMBLE_SYMBOLS);
        assert_eq!(q.airtime(), one + one);
    }

    fn tags(q: &SendQueue) -> std::vec::Vec<u8> {
        q.iter().map(|f| f.frame.bytes[0]).collect()
    }

    #[test]
    fn arrival_order_and_bounded() {
        let mut q = SendQueue::new();
        assert!(q.is_empty() && q.current_index().is_none() && q.remove(0).is_none());
        for i in 0..SEND_QUEUE as u8 {
            assert_eq!(q.push(frame(i)), Ok(()));
        }
        assert_eq!(q.regular(), SEND_QUEUE);
        // The chosen entry is changed and taken where it stands.
        q.select(5);
        q.get_mut(5).unwrap().frame.bytes[0] = 50;
        assert_eq!(q.remove(5).unwrap().frame.bytes[0], 50);
        assert!(q.current_index().is_none());
        assert_eq!(&tags(&q)[4..6], &[4, 6]);
        assert_eq!(q.len(), SEND_QUEUE - 1);
        // Removing entries before the chosen one leaves it on its entry.
        q.select(10);
        q.start_flight(10, Instant::ZERO, 0);
        q.retain(|under_way, f| under_way || f.frame.bytes[0] % 2 == 0);
        let c = q.current_index().unwrap();
        assert_eq!(q.get(c).unwrap().frame.bytes[0], 11);
        assert_eq!(q.flight_of(c).map(|f| f.place()), Some(c));
        assert_eq!(q.len(), 21, "20 even tags and the one under way, 11");
        q.retain(|under_way, _| !under_way);
        assert!(q.current_index().is_none() && q.flights().next().is_none());
    }

    /// A flight names its entry through removals before and after it,
    /// ends with its entry, and the flights are bounded.
    #[test]
    fn flights_follow_their_entries() {
        let t = |ms| Instant::ZERO + Duration::from_millis(ms);
        let mut q = SendQueue::new();
        for i in 0..8u8 {
            q.push(frame(i)).unwrap();
        }
        assert!(q.start_flight(3, t(1), 0b100));
        assert!(!q.start_flight(3, t(2), 0), "one flight per entry");
        assert!(q.start_flight(5, t(2), 0));
        assert!(!q.start_flight(9, t(2), 0), "no such entry");
        assert_eq!(q.flight_of(3).unwrap().skipped, 0b100);
        q.remove(0);
        assert_eq!(q.flight_of(2).map(|f| f.until), Some(t(1)));
        assert_eq!(q.flight_of(4).map(|f| f.until), Some(t(2)));
        q.remove(7 - 1);
        assert_eq!(q.flights().count(), 2, "an entry after them");
        q.remove(2);
        assert_eq!(q.flights().count(), 1, "its entry gone, the flight is");
        assert_eq!(q.flight_of(3).map(|f| f.until), Some(t(2)));
        q.end_flight(3);
        assert!(q.flights().next().is_none());
        for i in 0..FLIGHTS {
            assert!(q.flight_free());
            assert!(q.start_flight(i, t(3), 0));
        }
        assert!(!q.flight_free());
        assert!(!q.start_flight(FLIGHTS, t(3), 0), "the flights are full");
        q.clear();
        assert!(q.flights().next().is_none() && q.is_empty());
    }

    /// The entry on the radio, and the channels the waiting ones hold
    /// (spec "Waiting hand-overs").
    #[test]
    fn waiting_flights_hold_their_channels() {
        let t = |ms| Instant::ZERO + Duration::from_millis(ms);
        let (c3, c4) = (Channel::new(3).unwrap(), Channel::new(4).unwrap());
        let mut q = SendQueue::new();
        for i in 0..4u8 {
            q.push(frame(i)).unwrap();
        }
        q.get_mut(1).unwrap().frame.channel = c4;
        q.get_mut(3).unwrap().frame.channel = c4;
        assert!(q.on_radio().is_none() && !q.holds_channel(c3));
        q.start_flight(0, t(0), 0);
        q.get_mut(0).unwrap().state = State::Waiting(Wait::Retry);
        assert!(q.holds_channel(c3) && !q.holds_channel(c4));
        q.start_flight(1, t(0), 0);
        q.get_mut(1).unwrap().state = State::AwaitAck;
        assert_eq!(q.on_radio(), Some(1));
        assert!(!q.holds_channel(c4), "listening for its ACK is no wait");
        q.get_mut(1).unwrap().state = State::Waiting(Wait::Budget);
        assert!(!q.holds_channel(c4), "the bucket is not the channel");
        q.get_mut(1).unwrap().state = State::Waiting(Wait::Backoff);
        assert!(q.holds_channel(c4));
        // Entries that waited for their channel begin once it is free.
        q.get_mut(2).unwrap().state = State::Waiting(Wait::Channel);
        q.get_mut(3).unwrap().state = State::Waiting(Wait::Channel);
        assert_eq!(q.next_unblocked(), None);
        assert_eq!(q.next_due(t(0)), None, "none of them is queued");
        q.end_flight(1);
        q.get_mut(1).unwrap().state = State::Queued;
        assert_eq!(q.next_unblocked(), Some(3));
        assert_eq!(q.next_due(t(0)), Some(1));
        assert_eq!(q.earliest_due(), Some(t(0)));
    }

    #[test]
    fn every_place_fits() {
        let mut q = SendQueue::new();
        for i in 0..SEND_QUEUE {
            q.push(frame(i as u8)).unwrap();
        }
        for k in (1..=HOP_PLACES as u8).rev() {
            assert_eq!(q.free_place(MAX_HOPS, None), Some(k));
            let mut o = frame(0);
            o.place = k;
            q.push(o).unwrap();
        }
        assert_eq!(q.free_place(MAX_HOPS, None), None);
        assert_eq!(q.push(frame(0)), Err(Full));
    }

    #[test]
    fn a_packet_takes_the_highest_free_place_not_above_its_count() {
        let mut q = SendQueue::new();
        assert_eq!(q.free_place(0, None), None, "nothing for count 0");
        assert_eq!(q.free_place(3, None), Some(3));
        for k in [3, 1] {
            let mut o = frame(k);
            o.place = k;
            q.push(o).unwrap();
        }
        assert_eq!(q.regular(), 0);
        assert_eq!(q.free_place(3, None), Some(2));
        assert_eq!(q.free_place(1, None), None);
        assert_eq!(q.free_place(1, Some(1)), Some(1));
        // The place frees when its frame leaves.
        q.remove(0);
        assert_eq!(q.free_place(5, None), Some(5));
        assert_eq!(q.free_place(3, None), Some(3));
    }

    /// A DATA frame with `hops` that has waited until `due` ms, if given.
    fn data(tag: u8, hops: u8, due: Option<u64>) -> Outgoing {
        use crate::ids::NodeId;
        use crate::wire::Data;
        let d = Data {
            sender_home: Channel::new(3).unwrap(),
            dst_hop: NodeId([1; 6]),
            src_hop: NodeId([2; 6]),
            dst_pos: crate::position::Position { lat: 0, lon: 0 },
            dst_id: NodeId([3; 6]),
            hops,
            time_left: 1,
            payload: &[tag],
        };
        let mut bytes = [0; MAX_FRAME];
        let len = Packet::Data(d).encode(&mut bytes).unwrap() as u8;
        let frame = Frame {
            channel: d.sender_home,
            preamble_symbols: MIN_PREAMBLE_SYMBOLS,
            len,
            bytes,
        };
        let mut o = Outgoing::new(frame, Kind::Forward);
        if let Some(ms) = due {
            o.not_before = Instant::ZERO + Duration::from_millis(ms);
        }
        o
    }

    /// When bans end, the most travelled of the frames that waited goes
    /// first, ahead of the queue's order; one that never waited keeps it.
    #[test]
    fn the_most_travelled_waiting_frame_goes_first() {
        let t = |ms| Instant::ZERO + Duration::from_millis(ms);
        let mut q = SendQueue::new();
        q.push(data(0, 1, None)).unwrap();
        q.push(data(1, 2, Some(10))).unwrap();
        q.push(data(2, 5, Some(10))).unwrap();
        q.push(data(3, 5, Some(10))).unwrap();
        q.push(data(4, 9, Some(20))).unwrap();
        assert_eq!(q.next_due(t(5)), Some(0), "none has waited its time");
        assert_eq!(q.next_due(t(10)), Some(2), "the earlier of equals");
        assert_eq!(q.next_due(t(20)), Some(4));
        q.remove(4);
        assert_eq!(q.next_due(t(20)), Some(2));
        q.retain(|_, o| !o.waited());
        assert_eq!(q.next_due(t(20)), Some(0), "the one that never waited");
    }

    #[test]
    fn waiting_entries_keep_their_place() {
        let t = |ms| Instant::ZERO + Duration::from_millis(ms);
        let mut q = SendQueue::new();
        for (tag, due) in [(1, 30), (2, 10), (3, 0)] {
            let mut o = frame(tag);
            o.not_before = t(due);
            q.push(o).unwrap();
        }
        assert_eq!(q.earliest_due(), Some(t(0)));
        assert_eq!(q.next_due(t(5)), Some(2));
        assert_eq!(q.next_due(t(10)), Some(1));
        q.select(1);
        assert_eq!(tags(&q), [1, 2, 3], "chosen, not moved");
        q.retain(|_, _| false);
        assert_eq!(q.next_due(t(100)), None);
    }
}

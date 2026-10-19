//! Delivery to the sleepy nodes a node hears
//! (spec "Delivery", "Delivery queue").
//!
//! A message that reaches the owner of its position and is for another
//! node waits in the delivery queue, whether or not that node is heard,
//! and leaves it when the node ACKs it or, stale after the delivery TTL,
//! when a newer message needs its place (`tables/delivery.rs`).
//! It goes to the node as DATA addressed to its ID, through the send queue
//! like any hand-over, only while the node listens: from each of its beats
//! for the heard table's window, which each message it takes renews
//! (`tables/heard.rs`). So the holder sends on the beat, which is what a
//! node that sleeps between beats needs. A queued message is handed to
//! the send queue one per node at a time and oldest first: when it
//! arrives, when the node beats, when the node takes the one before, and
//! whenever a hand-over ends and frees room in the send queue. In its
//! node's window it needs a place in the regular room and its own frame in
//! the bucket with the reply reserve left, not the airtime of everything
//! queued, which it goes ahead of (spec "Delivery queue"). One the bucket cannot fit yet is
//! offered again the moment it will, as a deferred hand-over is started
//! then (`feed_at`). One whose node has stopped listening by the time its
//! hand-over would start leaves the send queue and waits for the next
//! beat; one whose hand-over goes unanswered marks its node asleep until
//! then.
//!
//! With [`Config::sleepy_listens`], a research switch, a node heard is
//! taken to listen all the time: its messages go at once, and after an
//! unanswered one every `delivery_retry` (60 s) as well as on its beats.
//!
//! Messages for a node this one does not hear wait for it all the same, as
//! spec "Owner" has the owner queue when nobody knows the node.
//!
//! [`Config::sleepy_listens`]: crate::Config::sleepy_listens

use super::Node;
use crate::event::{Action, DropReason, Trace};
use crate::ids::{MsgId, NodeId};
use crate::node::mac;
use crate::radio::{time_on_air, MIN_PREAMBLE_SYMBOLS};
use crate::sniff::LONG_PREAMBLE_SYMBOLS;
use crate::tables::budget::REPLY_RESERVE;
use crate::tables::send_queue::{Kind, Outgoing};
use crate::time::{Duration, Instant};
use crate::wire::{Data, Packet, DATA_HEADER_LEN};

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// Whether the sleepy node `id` may be sent to with the short preamble
    /// at `now`: in the window after a beat, or with `sleepy_listens`
    /// whenever it is heard.
    pub(super) fn listening(&self, id: NodeId, now: Instant) -> bool {
        if self.config.sleepy_listens {
            return self.heard.get(id).is_some();
        }
        self.heard.awake(id, now)
    }

    /// Hand the oldest message queued for `node` to the send queue, if
    /// the node listens and has none there already; with `sleepy_listens`,
    /// a node whose last delivery went unanswered only on a `retry`. If the
    /// send queue has no room for it: `Err(Some(t))` when the bucket will
    /// fit it at `t`, `Err(None)` when only a hand-over ending will make
    /// room.
    ///
    /// Outside its window, the node receiving duty-cycled, the message goes
    /// with the long preamble if the bucket holds it beyond everything this
    /// node has to send: the send queue, the other messages of the delivery
    /// queue in their windows, and the reply reserve. Otherwise, or once a
    /// delivery to the node has gone unanswered, it waits for its beat.
    pub(super) fn try_deliver(
        &mut self,
        node: NodeId,
        retry: bool,
        now: Instant,
    ) -> Result<(), Option<Instant>> {
        let Some(n) = self.heard.get(node).copied() else {
            return Ok(());
        };
        let listens = if self.config.sleepy_listens {
            !n.missed || retry
        } else {
            self.heard.awake(node, now)
        };
        let is_for = |o: &Outgoing| {
            o.kind == Kind::Deliver
                && matches!(Packet::decode(o.frame.payload()), Ok(Packet::Data(d)) if d.dst_hop == node)
        };
        // Outside its window, and not known to be out of reach.
        let early = !listens && !self.config.sleepy_listens && !n.missed;
        if (!listens && !early) || self.queue.iter().any(is_for) {
            return Ok(());
        }
        let home = n.home;
        let Some(q) = self.delivery.iter().find(|q| q.node == node) else {
            return Ok(());
        };
        // The count is stamped when its hand-over starts (spec "Hop count").
        let data = Data {
            sender_home: self.home,
            dst_hop: node,
            src_hop: self.me(),
            dst_pos: q.dst_pos,
            dst_id: node,
            hops: 0,
            time_left: q.time_left,
            payload: q.payload(),
        };
        let preamble = if early {
            LONG_PREAMBLE_SYMBOLS
        } else {
            MIN_PREAMBLE_SYMBOLS
        };
        let (msg_id, frame) = (q.msg_id, mac::frame(&Packet::Data(data), home, preamble));
        let airtime = mac::airtime(&frame);
        if early {
            // The other messages it holds for sleepy nodes, in their windows.
            let held = self
                .delivery
                .iter()
                .filter(|o| o.msg_id != msg_id)
                .map(|o| {
                    time_on_air(
                        (DATA_HEADER_LEN + o.payload().len()) as u8,
                        MIN_PREAMBLE_SYMBOLS,
                    )
                })
                .fold(Duration::ZERO, |a, b| a + b);
            if self.no_room(airtime, held, now).is_none() {
                let _ = self.queue.push(Outgoing::new(frame, Kind::Deliver));
            }
            return Ok(());
        }
        // In its window it goes ahead of everything queued, and needs room
        // for itself only: held back by the forwards behind it, a capped
        // holder's deliveries missed their windows (roadmap D55).
        match self.no_room_to_deliver(airtime, now) {
            None => {}
            Some(DropReason::SendQueueFull) => return Err(None),
            Some(_) => return Err(self.budget.allowed_at(airtime, REPLY_RESERVE, now)),
        }
        let _ = self.queue.push(Outgoing::new(frame, Kind::Deliver));
        Ok(())
    }

    /// Offer the send queue the oldest queued message of each node, oldest
    /// first, until it has no room, and note when to offer again: when the
    /// bucket will fit the message that did not, or else when a hand-over
    /// ends (`feed_due`).
    pub(super) fn feed(&mut self, retry: bool, now: Instant) {
        self.feed_due = false;
        self.feed_at = None;
        let mut i = 0;
        loop {
            let Some(node) = self.delivery.iter().nth(i).map(|q| q.node) else {
                return;
            };
            let first = !self.delivery.iter().take(i).any(|q| q.node == node);
            if first {
                if let Err(at) = self.try_deliver(node, retry, now) {
                    self.feed_at = at;
                    return;
                }
            }
            i += 1;
        }
    }

    /// Hand `node`'s oldest message to the send queue as [`Self::feed`]
    /// would.
    pub(super) fn deliver_now(&mut self, node: NodeId, now: Instant) {
        if let Err(Some(at)) = self.try_deliver(node, false, now) {
            self.feed_at = Some(self.feed_at.map_or(at, |t| t.min(at)));
        }
    }

    /// `node` ACKed the message `msg_id` handed to it: it listens one
    /// hand-over for the next.
    pub(super) fn delivered_to(
        &mut self,
        msg_id: MsgId,
        node: NodeId,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        self.delivery.remove(msg_id);
        self.delivered.insert(msg_id, now);
        self.heard.took(node, now);
        emit(Action::Trace(Trace::Delivered { msg_id, node }));
        self.deliver_now(node, now);
    }

    /// Make a place in the full delivery queue for a message arriving at
    /// `now`, if a stale one can give it up (spec "Delivery queue").
    pub(super) fn make_room(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        let Some(q) = self.delivery.make_room(now) else {
            return;
        };
        // A hand-over of it not yet on its way goes with it.
        let id = q.msg_id;
        let active = !self.handover.idle();
        self.queue.retain(|current, o| {
            (current && active) || o.kind != Kind::Deliver || o.msg_id() != Some(id)
        });
        self.feed_due = true;
        emit(Action::Trace(Trace::Dropped {
            msg_id: id,
            reason: DropReason::QueueExpired,
        }));
    }

    /// Expire the heard table, and with `sleepy_listens` retry the
    /// delivery queue.
    pub(super) fn delivery_timers(&mut self, now: Instant) {
        self.heard.expire(now);
        if self.config.sleepy_listens && now >= self.delivery_retry_at {
            self.delivery_retry_at = now + self.config.delivery_retry;
            self.feed(true, now);
        }
    }

    /// When delivery next needs the core.
    pub(super) fn delivery_wakeup(&self) -> Option<Instant> {
        let retry = (self.config.sleepy_listens && !self.delivery.is_empty())
            .then_some(self.delivery_retry_at);
        [
            retry,
            self.feed_at,
            self.heard.next_expiry(),
            // A message of its own waits for a place (`release_held`).
            self.held.and(self.delivery.stale_from()),
        ]
        .into_iter()
        .flatten()
        .min()
    }
}

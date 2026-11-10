//! The drain before a reboot into a new image (spec "Send queue",
//! "Updates over the mesh", the install): the node takes no message, so
//! that what it holds goes on to its neighbours and its sleepy nodes, and
//! the driver reboots once it holds nothing or the bound has passed. What
//! it still holds then, the driver keeps across the reboot ([`Node::held`])
//! and puts back where it was ([`Node::put_back`]), so that the node
//! carries on with it as if it had not been away.

use super::Node;
use crate::ids::{MsgId, NodeId};
use crate::node::mac;
use crate::position::Position;
use crate::radio::Channel;
use crate::tables::delivery::{Pushed, Queued};
use crate::tables::records::Record;
use crate::tables::send_queue::SEND_QUEUE;
use crate::time::{Duration, Instant};
use crate::wire::Packet;

/// A message the node holds, as a driver keeps it across a reboot: what
/// [`MsgId::of`] is taken of, so that it is the same message after, and
/// where it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Held<'a> {
    /// Its ID.
    pub msg_id: MsgId,
    /// Its destination's position.
    pub dst_pos: Position,
    /// Its destination node.
    pub dst_id: NodeId,
    /// Its payload.
    pub payload: &'a [u8],
    /// The count it goes on from (spec "Hop count"): a forward's highest
    /// received or sent here, or the count a message waiting in a queue
    /// arrived with.
    pub hops: u8,
    /// Where it was.
    pub at: HeldAt,
}

/// Where a held message was, and so where it goes back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeldAt {
    /// The application's own message, not yet taken by a first hop: the
    /// driver offers it again as the application's, which may be given
    /// it back (spec "A sleepy node's own message").
    Own,
    /// A forward or a return in the send queue.
    Forward {
        /// The neighbour it came from and its home channel then, which a
        /// return goes to (spec "Return to parent"); `None` where the
        /// search began here.
        parent: Option<(NodeId, Channel)>,
        /// The send-queue place it held; 0 the regular room.
        place: u8,
    },
    /// In the delivery queue.
    Delivery {
        /// The neighbour leaving the air it waits for, or
        /// [`NodeId::NONE`] for a message to deliver from here.
        waits_for: NodeId,
        /// How long it had been queued.
        age: Duration,
        /// It went once with the long preamble after a far fix.
        flushed: bool,
    },
    /// In the inbox, for the node's own application.
    Inbox {
        /// How long it had been queued.
        age: Duration,
    },
}

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// `AppCommand::Drain`: take no message from now, or take them again.
    pub(super) fn drain(&mut self, on: bool, now: Instant) {
        let s = &mut self.forwarding.drain_from_s;
        *s = match on {
            // Rounded up, so that the bound is never shorter.
            true => (*s).min(now.as_micros().div_ceil(1_000_000) as u32),
            false => u32::MAX,
        };
    }

    /// Whether it drains for a reboot: it takes no message.
    pub fn draining(&self) -> bool {
        self.forwarding.drain_from_s != u32::MAX
    }

    /// Whether it is about to leave the air, for a reboot or to turn
    /// sleepy: its heartbeats say so, and its neighbours rely on it only
    /// where nothing else is left (spec "Packet formats", "Next hop").
    pub fn leaving_air(&self) -> bool {
        self.draining() || self.leaving()
    }

    /// Whether the node, draining, may reboot at `now`: it holds nothing
    /// a reset would lose, in its send queue, its delivery queue, its
    /// inbox or a reply that takes a message, or it has drained for the
    /// delivery TTL from the whole second it began, the wait the delivery
    /// queue gives a node that sleeps or is dark, by when every message
    /// in it has waited that long for its node (spec "Delivery queue")
    /// and a forward has had twenty hops' budgets (spec "Send queue").
    pub fn drained(&self, now: Instant) -> bool {
        if !self.draining() {
            return false;
        }
        let since = Instant::from_micros(u64::from(self.forwarding.drain_from_s) * 1_000_000);
        let empty = self.holding().next().is_none()
            && self.inbox.is_empty()
            && !self.radio.replies.taking();
        empty || now.since(since) >= self.config.delivery_ttl
    }

    /// Every message the node holds at `now`, each once, in order: its send
    /// queue, its delivery queue and its inbox. A message on its way to a
    /// sleepy node is the delivery queue's.
    pub fn held(&self, now: Instant) -> impl Iterator<Item = Held<'_>> + '_ {
        let queued = self
            .queue
            .iter()
            .filter(|o| !o.kind.delivers())
            .filter_map(move |o| {
                let Ok(Packet::Data(d)) = Packet::decode(o.frame.payload()) else {
                    return None;
                };
                let id = d.msg_id();
                let record = self.records.get(id);
                let at = match self.own.id == Some(id) {
                    true => HeldAt::Own,
                    false => HeldAt::Forward {
                        parent: record.and_then(Record::parent),
                        place: o.place,
                    },
                };
                Some(Held {
                    msg_id: id,
                    dst_pos: d.dst_pos,
                    dst_id: d.dst_id,
                    payload: d.payload,
                    // The frame carries the count received plus one until
                    // its hand-over stamps it (`relay/forward.rs`).
                    hops: record.map_or(d.hops.saturating_sub(1), Record::highest_hops),
                    at,
                })
            });
        fn held(q: &Queued, at: HeldAt) -> Held<'_> {
            Held {
                msg_id: q.msg_id,
                dst_pos: q.dst_pos,
                dst_id: q.node,
                payload: q.payload(),
                hops: q.hops,
                at,
            }
        }
        let delivery = self.delivery.iter().map(move |q| {
            let at = HeldAt::Delivery {
                waits_for: q.waits_for,
                age: now.since(q.enqueued),
                flushed: q.flushed,
            };
            held(q, at)
        });
        let inbox = self.inbox.iter().map(move |q| {
            let at = HeldAt::Inbox {
                age: now.since(q.enqueued),
            };
            held(q, at)
        });
        queued.chain(delivery).chain(inbox)
    }

    /// Put back a message [`Node::held`] gave, taken `ago` before `now`,
    /// after the reset and after the tables kept with it
    /// ([`Node::remember`]), before the first event: whether it was. A
    /// forward goes back into the send queue at the place it held, with
    /// its record if the table kept it, else one made from what `m` keeps;
    /// a message in a queue goes back into it, aged by `ago`, as the
    /// tables are. The application's own is not put back: the driver
    /// offers it again as the application's (spec "Updates over the mesh",
    /// the install).
    pub fn put_back(&mut self, m: &Held<'_>, ago: Duration, now: Instant) -> bool {
        let id = m.msg_id;
        let queued = |age: Duration| {
            let enqueued = now.as_micros().saturating_sub((ago + age).as_micros());
            Queued::new(
                id,
                m.dst_pos,
                m.dst_id,
                m.payload,
                Instant::from_micros(enqueued),
            )
            .map(|mut q| {
                q.hops = m.hops;
                q
            })
        };
        match m.at {
            HeldAt::Own => false,
            HeldAt::Forward { parent, place } => {
                let frame = self.onward(m.dst_pos, m.dst_id, m.payload, m.hops);
                let free = match place {
                    0 => self.queue.regular() < SEND_QUEUE,
                    k => self.queue.iter().all(|o| o.place != k),
                };
                let air = mac::airtime(&frame);
                let place = match free {
                    true => place,
                    false => match self.admit(air, Duration::ZERO, m.hops, false, now) {
                        Ok(k) => k,
                        Err(_) => return false,
                    },
                };
                if self.records.get(id).is_none() {
                    // The table outnumbers what a node holds
                    // (`tables/records.rs`).
                    let Some(at) = self.record_place() else {
                        return false;
                    };
                    self.records.put(at, Record::new(id, parent, m.hops, now));
                }
                self.push_forward(frame, place, now)
            }
            HeldAt::Delivery {
                waits_for,
                age,
                flushed,
            } => {
                let Some(mut q) = queued(age) else {
                    return false;
                };
                q.waits_for = waits_for;
                q.flushed = flushed;
                let pushed = self.delivery.push(q) == Pushed::Queued;
                self.forwarding.feed_due |= pushed;
                pushed
            }
            HeldAt::Inbox { age } => {
                let Some(q) = queued(age) else {
                    return false;
                };
                self.delivered.insert(id, q.enqueued);
                self.inbox.push(q) == Pushed::Queued
            }
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::node::Config;
    use crate::Relay;

    fn at(s: u64) -> Instant {
        Instant::ZERO + Duration::from_secs(s)
    }

    /// A message queued for a sleepy node at 1 000 s and kept at 1 100 s,
    /// put back 59 s after on a clock that reads 5 000 s, has been queued
    /// 159 s: it turns stale 441 s later, at the delivery TTL from its
    /// queueing, and makes way for a newer one then and not before.
    #[test]
    fn a_queued_message_put_back_expires_on_time() {
        let config = Config::default();
        let ttl = config.delivery_ttl;
        let mut r = Relay::new(config, 1);
        let node = NodeId([9; 6]);
        let dst_pos = Position::default();
        let q = |id: u32, t: Instant| {
            Queued::new(MsgId(id), dst_pos, node, &id.to_be_bytes(), t).unwrap()
        };
        r.delivery.push(q(1, at(1_000)));
        let held: std::vec::Vec<_> = r.held(at(1_100)).collect();
        let HeldAt::Delivery { age, .. } = held[0].at else {
            panic!("not in the delivery queue");
        };
        assert_eq!(age, Duration::from_secs(100));
        let mut back = Relay::new(config, 2);
        let away = Duration::from_secs(59);
        assert!(back.put_back(&held[0], away, at(5_000)));
        let stale = at(5_000) + ttl - Duration::from_secs(159);
        assert_eq!(back.delivery.stale_from(), Some(stale));
        for i in 2..=crate::tables::delivery::DELIVERY_QUEUE as u32 {
            back.delivery.push(q(i, at(5_000)));
        }
        let just_before = stale - Duration::from_millis(1);
        assert_eq!(back.delivery.make_room(just_before), None);
        let gone = back.delivery.make_room(stale).map(|q| q.msg_id);
        assert_eq!(gone, Some(MsgId(1)));
    }
}

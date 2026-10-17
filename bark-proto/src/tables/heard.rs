//! The heard table: the sleepy nodes a node has heard beat on its home
//! channel, by ID, for delivery (spec "Heard table").
//!
//! A sleepy node heartbeats only on its home channel, at a random time in
//! the second half of each [`beat_period`], and listens for a while after
//! each beat ([`HeardParams::awake`]). A holder of a message for it sends
//! the message then. The table keeps what that needs: the node's home
//! channel, until when it listens, and whether a delivery to it went
//! unanswered since its last beat.
//!
//! An entry is kept for two beat periods, the delivery TTL, after its last
//! beat: one lost beat does not lose it, and a node that has gone is kept
//! no longer than the messages held for it (spec "Sleepy nodes"). When the table
//! is full a newcomer takes the place of an expired entry, else of the
//! weakest if it is heard stronger: the strongest are the nodes most
//! likely to have chosen this one (spec "Relay choice").

use crate::event::MAX_FRAME;
use crate::ids::NodeId;
use crate::node::Config;
use crate::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS};
use crate::tables::fixed::FixedVec;
use crate::tables::send_queue::SEND_QUEUE;
use crate::tables::{attempt_span, retry_waits};
use crate::time::{Duration, Instant};
use crate::wire::ACK_LEN;

/// Table capacity (spec "Heard table": ≈80 sleepy nodes per relay expected
/// at indoor density with 10 % adoption, 400 at 50 %).
pub const HEARD: usize = 256;

/// The longest interval between two heartbeats of a sleepy node: half the
/// delivery TTL, 300 s (spec "Beats"). A
/// holder keeps a node for two periods, a TTL, so one lost beat does not
/// lose it and a node that has gone is kept no longer than its messages;
/// a message waits at most a period for its node to wake.
pub const fn beat_period(delivery_ttl: Duration) -> Duration {
    delivery_ttl.div(2)
}

/// The table's parameters, from a configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeardParams {
    /// How long an entry is kept after its last beat: two beat periods.
    pub keep: Duration,
    /// How long a sleepy node listens after its beat or a message it
    /// took, and so how long a holder may send to it then: the hand-overs
    /// that may be ahead of the next message, a full send queue (spec
    /// "Send queue") of full frames each with its ACK, 6.12 s, and the
    /// message with all its attempts and their waits (spec "Hand-over"),
    /// 3.13 s: 9.25 s (spec "Delivery"). It is the window for one
    /// message; a node that is not told how many wait listens again as long
    /// after each it takes.
    pub awake: Duration,
}

impl HeardParams {
    /// From the protocol configuration.
    pub const fn from_config(config: &Config) -> HeardParams {
        let attempt = attempt_span(config.ack_timeout).as_micros();
        let span = attempt * config.hop_attempts as u64
            + retry_waits(config.retry_backoff, config.hop_attempts).as_micros();
        let full = time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS).as_micros()
            + time_on_air(ACK_LEN as u8, MIN_PREAMBLE_SYMBOLS).as_micros();
        HeardParams {
            keep: beat_period(config.delivery_ttl).mul(2),
            awake: Duration::from_micros(SEND_QUEUE as u64 * full + span),
        }
    }
}

/// A sleepy node heard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeardNode {
    /// Its ID.
    pub id: NodeId,
    /// Its home channel, where it listens.
    pub home: Channel,
    /// Mean RSSI of its last two beats, dBm; the one beat until there are
    /// two.
    pub rssi: i16,
    /// RSSI of its last beat.
    pub last: i16,
    /// When its last beat was heard.
    pub last_heard: Instant,
    /// Until when it listens.
    pub awake_until: Instant,
    /// A delivery to it went unanswered since its last beat: it is not
    /// listening, and its messages wait for its next beat.
    pub missed: bool,
}

/// The heard table.
#[derive(Clone)]
pub struct Heard {
    nodes: FixedVec<HeardNode, HEARD>,
    params: HeardParams,
}

const EMPTY: HeardNode = HeardNode {
    id: NodeId::NONE,
    home: match Channel::new(0) {
        Some(c) => c,
        None => panic!("the plan has channels"),
    },
    rssi: 0,
    last: 0,
    last_heard: Instant::ZERO,
    awake_until: Instant::ZERO,
    missed: false,
};

impl Heard {
    /// An empty table.
    pub const fn from_config(config: &Config) -> Heard {
        Heard {
            nodes: FixedVec::new(EMPTY),
            params: HeardParams::from_config(config),
        }
    }

    /// The parameters.
    pub fn params(&self) -> &HeardParams {
        &self.params
    }

    /// Number of entries, expired ones not yet removed included.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether there are no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The entries.
    pub fn iter(&self) -> impl Iterator<Item = &HeardNode> {
        self.nodes.as_slice().iter()
    }

    /// The entry of `id`.
    pub fn get(&self, id: NodeId) -> Option<&HeardNode> {
        self.iter().find(|n| n.id == id)
    }

    fn get_mut(&mut self, id: NodeId) -> Option<&mut HeardNode> {
        self.nodes.as_mut_slice().iter_mut().find(|n| n.id == id)
    }

    /// A beat of `id` on `home` was heard at `now` with packet RSSI
    /// `rssi`: it listens now. Returns whether it is in the table.
    pub fn heard(&mut self, id: NodeId, home: Channel, rssi: i16, now: Instant) -> bool {
        let awake = now + self.params.awake;
        if let Some(n) = self.get_mut(id) {
            n.rssi = (n.last as i32 + rssi as i32).div_euclid(2) as i16;
            n.last = rssi;
            n.home = home;
            n.last_heard = now;
            n.awake_until = awake;
            n.missed = false;
            return true;
        }
        let new = HeardNode {
            id,
            home,
            rssi,
            last: rssi,
            last_heard: now,
            awake_until: awake,
            missed: false,
        };
        if self.nodes.push(new).is_ok() {
            return true;
        }
        let keep = self.params.keep;
        let slots = self.nodes.as_mut_slice();
        let expired = slots.iter().position(|n| now.since(n.last_heard) >= keep);
        let weakest = || {
            let (i, n) = slots.iter().enumerate().min_by_key(|(_, n)| n.rssi)?;
            (n.rssi < rssi).then_some(i)
        };
        match expired.or_else(weakest) {
            Some(i) => {
                slots[i] = new;
                true
            }
            None => false,
        }
    }

    /// Whether `id` listens at `now`.
    pub fn awake(&self, id: NodeId, now: Instant) -> bool {
        self.get(id)
            .is_some_and(|n| now < n.awake_until && !n.missed)
    }

    /// `id` took a message at `now`: it listens a window longer, for the
    /// next.
    pub fn took(&mut self, id: NodeId, now: Instant) {
        let awake = now + self.params.awake;
        if let Some(n) = self.get_mut(id) {
            n.awake_until = n.awake_until.max(awake);
            n.missed = false;
        }
    }

    /// A delivery to `id` went unanswered: it is not listening.
    pub fn missed(&mut self, id: NodeId) {
        if let Some(n) = self.get_mut(id) {
            n.missed = true;
        }
    }

    /// Remove every entry not heard for two beat periods at `now`.
    pub fn expire(&mut self, now: Instant) {
        let keep = self.params.keep;
        self.nodes.retain(|n| now.since(n.last_heard) < keep);
    }

    /// When the next entry expires, if any.
    pub fn next_expiry(&self) -> Option<Instant> {
        self.iter().map(|n| n.last_heard + self.params.keep).min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: u64) -> Instant {
        Instant::from_micros(s * 1_000_000)
    }

    fn id(i: u32) -> NodeId {
        NodeId((i + 1).to_be_bytes())
    }

    fn ch(i: u8) -> Channel {
        Channel::new(i).unwrap()
    }

    #[test]
    fn params_follow_the_design() {
        let p = HeardParams::from_config(&Config::default());
        assert_eq!(
            beat_period(Config::default().delivery_ttl),
            Duration::from_secs(300)
        );
        assert_eq!(p.keep, Duration::from_secs(600));
        // 40 × 152.96 ms, and 3 × 738.56 ms + 305.92 + 611.84 ms.
        assert_eq!(p.awake, Duration::from_micros(6_118_400 + 3_133_440));
    }

    #[test]
    fn a_beat_wakes_the_node_for_a_window() {
        let mut t = Heard::from_config(&Config::default());
        let awake = t.params().awake;
        assert!(t.heard(id(1), ch(3), -90, at(10)));
        assert!(t.awake(id(1), at(10) + (awake - Duration::from_micros(1))));
        assert!(!t.awake(id(1), at(10) + awake));
        t.took(id(1), at(15));
        assert!(t.awake(id(1), at(15) + (awake - Duration::from_micros(1))));
        t.missed(id(1));
        assert!(!t.awake(id(1), at(16)), "a missed delivery: asleep");
        assert!(t.heard(id(1), ch(4), -80, at(300)));
        let n = *t.get(id(1)).unwrap();
        assert_eq!((n.home, n.rssi, n.last, n.missed), (ch(4), -85, -80, false));
        assert!(t.awake(id(1), at(300)));
    }

    #[test]
    fn entries_last_two_periods_and_the_weakest_makes_way() {
        let mut t = Heard::from_config(&Config::default());
        for i in 0..HEARD as u32 {
            assert!(t.heard(id(i), ch(0), -100 + (i % 20) as i16, at(0)));
        }
        assert!(!t.heard(id(1000), ch(0), -100, at(1)), "no weaker one");
        assert!(t.heard(id(1001), ch(0), -99, at(1)));
        assert!(t.get(id(0)).is_none(), "the weakest went");
        assert_eq!(t.next_expiry(), Some(at(600)));
        t.expire(at(599));
        assert_eq!(t.len(), HEARD);
        t.expire(at(600));
        assert_eq!(t.len(), 1);
    }
}

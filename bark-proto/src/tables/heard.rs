//! The heard table: the sleepy nodes a node has heard beat on its home
//! channel, by ID, for delivery (spec "Heard table").
//!
//! A sleepy node heartbeats only on its home channel, at a random time in
//! the second half of each [`beat_period`], and listens for a while after
//! each beat ([`HeardParams::awake`]) and after each message it takes
//! ([`HeardParams::linger`]). A holder of a message
//! for it sends the message then. The table keeps what that needs: the node's home
//! channel, until when it listens, and whether a delivery to it went
//! unanswered since its last beat.
//!
//! An entry is kept for two beat periods, the delivery TTL, after the last
//! frame heard from the node, its beat, its own DATA or its ACK: one lost
//! beat does not lose it, a node that answers is not lost for its beats
//! going unheard, and a node that has gone is kept no longer than the
//! messages held for it (spec "Sleepy nodes", "Heard table"). When the table
//! is full a newcomer takes the place of an expired entry, else of the one
//! heard longest ago: a node that beats now listens now, so a holder for
//! more nodes than the table keeps still sends each its mail after each
//! beat (roadmap D61).

use crate::event::MAX_FRAME;
use crate::ids::NodeId;
use crate::node::Config;
use crate::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS};
use crate::tables::fixed::FixedVec;
use crate::tables::{attempt_span, retry_waits};
use crate::time::{Duration, Instant};

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
    /// How long an entry is kept after the last frame heard from its node:
    /// two beat periods.
    pub keep: Duration,
    /// How long a sleepy node listens after its beat, and so how long a
    /// holder may start sending to it then: long enough for one hand-over
    /// of a message that goes first to complete (spec "Delivery"), since
    /// one cut short waits a beat period. That is the attempt the holder
    /// may have under way, a full frame and the ACK timeout, 238.56 ms,
    /// and the message's hand-over, as [`linger`](Self::linger):
    /// 3379.68 ms.
    pub awake: Duration,
    /// How long it listens after it has answered a message: one hand-over
    /// with all its attempts and retry waits, 3141.12 ms. Nothing is under
    /// way ahead of the next message then: the holder has just had the
    /// answer.
    pub linger: Duration,
}

impl HeardParams {
    /// From the protocol configuration.
    pub const fn from_config(config: &Config) -> HeardParams {
        let attempt = attempt_span(config.ack_timeout).as_micros();
        let span = attempt * config.hop_attempts as u64
            + retry_waits(config.retry_backoff, config.hop_attempts).as_micros();
        let under_way = time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS).as_micros()
            + config.ack_timeout.as_micros();
        HeardParams {
            keep: beat_period(config.delivery_ttl).mul(2),
            awake: Duration::from_micros(under_way + span),
            linger: Duration::from_micros(span),
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
    /// When the last frame from it was heard: a beat, its DATA or its ACK.
    pub last_heard: Instant,
    /// Until when it listens.
    pub awake_until: Instant,
    /// A delivery to it went unanswered since its last beat: it is not
    /// listening, and its messages wait for its next beat.
    pub missed: bool,
}

/// The heard table, of up to `N` nodes: a sleepy node keeps none (spec
/// "Heard table").
///
/// Its few fields first, then the items: an event reads the first and
/// seldom the items ([`crate::Node`]'s layout).
#[derive(Clone)]
#[repr(C)]
pub struct Heard<const N: usize = HEARD> {
    params: HeardParams,
    nodes: FixedVec<HeardNode, N>,
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

impl<const N: usize> Heard<N> {
    /// An empty table.
    pub const fn from_config(config: &Config) -> Heard<N> {
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
        let oldest = || {
            let (i, _) = slots.iter().enumerate().min_by_key(|(_, n)| n.last_heard)?;
            Some(i)
        };
        match expired.or_else(oldest) {
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

    /// `id` took a message at `now`: it listens one hand-over for the next.
    pub fn took(&mut self, id: NodeId, now: Instant) {
        let awake = now + self.params.linger;
        if let Some(n) = self.get_mut(id) {
            n.awake_until = awake;
            n.missed = false;
            n.last_heard = now;
        }
    }

    /// A DATA of `id`'s own, sent from its home channel `home`, was heard
    /// at `now`: it is in reach, and listens on `home`. Sending says
    /// nothing of whether it listens now.
    pub fn sent(&mut self, id: NodeId, home: Channel, now: Instant) {
        if let Some(n) = self.get_mut(id) {
            n.home = home;
            n.last_heard = now;
        }
    }

    /// `id` now listens on `home`.
    pub fn moved(&mut self, id: NodeId, home: Channel) {
        if let Some(n) = self.get_mut(id) {
            n.home = home;
        }
    }

    /// A delivery to `id` went unanswered: it is not listening.
    pub fn missed(&mut self, id: NodeId) {
        if let Some(n) = self.get_mut(id) {
            n.missed = true;
        }
    }

    /// Empty the table.
    pub fn clear(&mut self) {
        self.nodes.clear();
    }

    /// Remove every entry of a node not heard for two beat periods at `now`.
    pub fn expire(&mut self, now: Instant) {
        // Called on every event, mostly of an empty table.
        if self.is_empty() {
            return;
        }
        let keep = self.params.keep;
        self.nodes.retain(|n| now.since(n.last_heard) < keep);
    }

    /// When the next entry expires, if any.
    pub fn next_expiry(&self) -> Option<Instant> {
        if self.is_empty() {
            return None;
        }
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
        NodeId::from_u64(i as u64 + 1)
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
        // 3 × 738.56 ms + 308.48 + 616.96 ms, after 138.56 + 100 ms.
        assert_eq!(p.linger, Duration::from_micros(3_141_120));
        assert_eq!(p.awake, Duration::from_micros(238_560 + 3_141_120));
    }

    #[test]
    fn a_beat_wakes_the_node_for_a_window() {
        let mut t = Heard::<HEARD>::from_config(&Config::default());
        let awake = t.params().awake;
        assert!(t.heard(id(1), ch(3), -90, at(10)));
        assert!(t.awake(id(1), at(10) + (awake - Duration::from_micros(1))));
        assert!(!t.awake(id(1), at(10) + awake));
        t.took(id(1), at(15));
        let linger = t.params().linger;
        assert!(t.awake(id(1), at(15) + (linger - Duration::from_micros(1))));
        assert!(!t.awake(id(1), at(15) + linger));
        t.took(id(1), at(15));
        t.missed(id(1));
        assert!(!t.awake(id(1), at(16)), "a missed delivery: asleep");
        assert!(t.heard(id(1), ch(4), -80, at(300)));
        let n = *t.get(id(1)).unwrap();
        assert_eq!((n.home, n.rssi, n.last, n.missed), (ch(4), -85, -80, false));
        assert!(t.awake(id(1), at(300)));
    }

    /// A node heard by its own DATA or by taking a message is kept as by
    /// a beat, and its DATA names the channel it listens on.
    #[test]
    fn any_frame_from_a_node_keeps_it() {
        let mut t = Heard::<HEARD>::from_config(&Config::default());
        assert!(t.heard(id(1), ch(3), -90, at(0)));
        t.sent(id(1), ch(5), at(500));
        assert!(!t.awake(id(1), at(500)), "sending is not listening");
        t.expire(at(1000));
        assert_eq!(t.get(id(1)).map(|n| n.home), Some(ch(5)));
        t.took(id(1), at(1050));
        t.expire(at(1600));
        assert!(t.get(id(1)).is_some());
        t.expire(at(1650));
        assert!(t.get(id(1)).is_none());
        t.sent(id(2), ch(5), at(1650));
        assert!(t.is_empty(), "DATA alone does not make a node sleepy");
    }

    /// A full table makes way for every newcomer, weak or strong, in the
    /// place of the node heard longest ago, which listens no longer.
    #[test]
    fn entries_last_two_periods_and_the_longest_unheard_makes_way() {
        let mut t = Heard::<HEARD>::from_config(&Config::default());
        for i in 0..HEARD as u32 {
            assert!(t.heard(id(i), ch(0), -60 - (i % 20) as i16, at(i as u64)));
        }
        assert!(t.heard(id(1000), ch(0), -110, at(300)), "a weak newcomer");
        assert!(t.get(id(0)).is_none(), "the longest unheard went");
        assert!(t.awake(id(1000), at(300)));
        assert!(t.heard(id(1), ch(0), -60, at(301)), "a beat renews");
        assert!(t.heard(id(1001), ch(0), -110, at(302)));
        assert!(t.get(id(2)).is_none() && t.get(id(1)).is_some());
        assert_eq!(t.next_expiry(), Some(at(603)));
        t.expire(at(855));
        assert_eq!(t.len(), HEARD - 255 + 2);
    }
}

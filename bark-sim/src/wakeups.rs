//! Each node's pending core wakeup, earliest first.
//!
//! A core has one wakeup at a time: each one it asks for replaces the
//! last. Pushed on the event queue, every replaced one would stay there
//! until its time came, to be skipped then; a sleepy node that asks for
//! the end of its boot listen whenever its radio is busy leaves one
//! behind at each reading, and in a city-sized run they make up nearly
//! all of the queue. Here each node has one place, in a heap that moves
//! or removes it.
//!
//! An entry is ordered by its time and then by the sequence number the
//! event queue gave it (`sim::Sim::push`), so that a wakeup comes out in
//! the place among the other events it would have had there.

use bark_proto::Instant;

/// Not in the heap.
const NONE: u32 = u32::MAX;

/// Time and sequence number in one integer, ordered as the pair.
pub(crate) fn order(t: Instant, seq: u64) -> u128 {
    (t.as_micros() as u128) << 64 | seq as u128
}

/// The time of an [`order`].
pub(crate) fn time(order: u128) -> Instant {
    Instant::from_micros((order >> 64) as u64)
}

pub(crate) struct Wakeups {
    /// Nodes, a binary min-heap on `order`.
    heap: Vec<u32>,
    /// Per node, its index in `heap`, or [`NONE`].
    at: Vec<u32>,
    /// Per node, the order of its wakeup while it has one.
    order: Vec<u128>,
}

impl Wakeups {
    pub(crate) fn new(nodes: usize) -> Wakeups {
        Wakeups {
            heap: Vec::with_capacity(nodes),
            at: vec![NONE; nodes],
            order: vec![0; nodes],
        }
    }

    /// The earliest wakeup: its order and node.
    pub(crate) fn peek(&self) -> Option<(u128, u32)> {
        let &n = self.heap.first()?;
        Some((self.order[n as usize], n))
    }

    /// Set `node`'s wakeup to `order`, or remove it.
    pub(crate) fn set(&mut self, node: u32, order: Option<u128>) {
        let n = node as usize;
        match (self.at[n], order) {
            (NONE, None) => {}
            (NONE, Some(o)) => {
                self.order[n] = o;
                self.at[n] = self.heap.len() as u32;
                self.heap.push(node);
                self.up(self.heap.len() - 1);
            }
            (i, Some(o)) => {
                let earlier = o < self.order[n];
                self.order[n] = o;
                if earlier {
                    self.up(i as usize);
                } else {
                    self.down(i as usize);
                }
            }
            (i, None) => self.remove(i as usize),
        }
    }

    /// Take the earliest wakeup out: its node.
    pub(crate) fn pop(&mut self) -> Option<u32> {
        let &n = self.heap.first()?;
        self.remove(0);
        Some(n)
    }

    fn remove(&mut self, i: usize) {
        let gone = self.heap.swap_remove(i);
        self.at[gone as usize] = NONE;
        if i < self.heap.len() {
            self.at[self.heap[i] as usize] = i as u32;
            self.up(i);
            self.down(i);
        }
    }

    fn up(&mut self, mut i: usize) {
        while i > 0 {
            let p = (i - 1) / 2;
            if self.key(i) >= self.key(p) {
                break;
            }
            self.swap(i, p);
            i = p;
        }
    }

    fn down(&mut self, mut i: usize) {
        loop {
            let (l, r) = (2 * i + 1, 2 * i + 2);
            let mut least = i;
            if l < self.heap.len() && self.key(l) < self.key(least) {
                least = l;
            }
            if r < self.heap.len() && self.key(r) < self.key(least) {
                least = r;
            }
            if least == i {
                return;
            }
            self.swap(i, least);
            i = least;
        }
    }

    fn key(&self, i: usize) -> u128 {
        self.order[self.heap[i] as usize]
    }

    fn swap(&mut self, i: usize, j: usize) {
        self.heap.swap(i, j);
        self.at[self.heap[i] as usize] = i as u32;
        self.at[self.heap[j] as usize] = j as u32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comes_out_in_order_after_moves_and_removals() {
        let n = 200;
        let mut w = Wakeups::new(n);
        let mut want: Vec<Option<u128>> = vec![None; n];
        let mut x: u64 = 7;
        for seq in 0..20_000 {
            x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let node = (x >> 33) as usize % n;
            // A quarter of the time the wakeup is removed.
            let t = Instant::from_micros((x >> 40) % 1000);
            let o = (!(x >> 20).is_multiple_of(4)).then(|| order(t, seq));
            w.set(node as u32, o);
            want[node] = o;
            if (x >> 10).is_multiple_of(3) {
                let least = want
                    .iter()
                    .enumerate()
                    .filter_map(|(i, o)| o.map(|o| (o, i as u32)))
                    .min();
                assert_eq!(w.peek(), least);
                if let Some((_, i)) = least {
                    assert_eq!(w.pop(), Some(i));
                    want[i as usize] = None;
                }
            }
        }
        let mut rest: Vec<(u128, u32)> = want
            .iter()
            .enumerate()
            .filter_map(|(i, o)| o.map(|o| (o, i as u32)))
            .collect();
        rest.sort();
        for (o, i) in rest {
            assert_eq!(w.peek(), Some((o, i)));
            assert_eq!(w.pop(), Some(i));
        }
        assert_eq!(w.pop(), None);
    }

    #[test]
    fn order_is_time_then_sequence() {
        let a = order(Instant::from_micros(5), u64::MAX);
        let b = order(Instant::from_micros(6), 0);
        assert!(a < b);
        assert_eq!(time(b), Instant::from_micros(6));
    }
}

//! The simulator's queues, earliest first: its events, and apart from
//! them each node's core wakeup.
//!
//! Every entry is ordered by its time and then by a sequence number
//! given when it was queued (`sim::Sim::push`), so that entries due at
//! the same instant come out in the order they were queued. The two make
//! one integer, [`order`]: the queues compare entries tens of times per
//! event, and one integer compares in one step even in a debug build.
//!
//! # Wakeups
//!
//! A core has one wakeup at a time: each one it asks for replaces the
//! last. Pushed on the event queue, every replaced one would stay there
//! until its time came, to be skipped then; a sleepy node that asks for
//! the end of its first rotation whenever its radio is busy leaves one
//! behind at each reading, and in a city-sized run they make up nearly
//! all of the queue. Here each node has one place, in a heap that moves
//! or removes it.

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

/// Events, a binary min-heap on their [`order`].
pub(crate) struct Events<E> {
    heap: Vec<(u128, E)>,
}

impl<E: Copy> Events<E> {
    pub(crate) fn new() -> Events<E> {
        Events { heap: Vec::new() }
    }

    /// The earliest event's order.
    pub(crate) fn peek(&self) -> Option<u128> {
        self.heap.first().map(|e| e.0)
    }

    pub(crate) fn push(&mut self, order: u128, e: E) {
        let mut i = self.heap.len();
        self.heap.push((order, e));
        while i > 0 {
            let p = (i - 1) / 2;
            if order >= self.heap[p].0 {
                break;
            }
            self.heap[i] = self.heap[p];
            i = p;
        }
        self.heap[i] = (order, e);
    }

    /// Take the earliest event out.
    pub(crate) fn pop(&mut self) -> Option<(u128, E)> {
        let last = self.heap.pop()?;
        let Some(&first) = self.heap.first() else {
            return Some(last);
        };
        // The last entry fills the root's place, sinking past smaller
        // children.
        let n = self.heap.len();
        let mut i = 0;
        loop {
            let l = 2 * i + 1;
            if l >= n {
                break;
            }
            let c = if l + 1 < n && self.heap[l + 1].0 < self.heap[l].0 {
                l + 1
            } else {
                l
            };
            if self.heap[c].0 >= last.0 {
                break;
            }
            self.heap[i] = self.heap[c];
            i = c;
        }
        self.heap[i] = last;
        Some(first)
    }
}

pub(crate) struct Wakeups {
    /// Order and node, a binary min-heap on the order.
    heap: Vec<(u128, u32)>,
    /// Per node, its index in `heap`, or [`NONE`].
    at: Vec<u32>,
}

impl Wakeups {
    pub(crate) fn new(nodes: usize) -> Wakeups {
        Wakeups {
            heap: Vec::with_capacity(nodes),
            at: vec![NONE; nodes],
        }
    }

    /// The earliest wakeup: its order and node.
    pub(crate) fn peek(&self) -> Option<(u128, u32)> {
        self.heap.first().copied()
    }

    /// The order of `node`'s wakeup, if it has one.
    pub(crate) fn get(&self, node: u32) -> Option<u128> {
        match self.at[node as usize] {
            NONE => None,
            i => Some(self.heap[i as usize].0),
        }
    }

    /// Set `node`'s wakeup to `order`, or remove it.
    pub(crate) fn set(&mut self, node: u32, order: Option<u128>) {
        match (self.at[node as usize], order) {
            (NONE, None) => {}
            (NONE, Some(o)) => {
                self.heap.push((o, node));
                self.sift(self.heap.len() - 1, (o, node));
            }
            (i, Some(o)) => self.sift(i as usize, (o, node)),
            (i, None) => {
                self.at[node as usize] = NONE;
                let last = self.heap.pop().expect("in the heap");
                if (i as usize) < self.heap.len() {
                    self.sift(i as usize, last);
                }
            }
        }
    }

    /// Take the earliest wakeup out: its node.
    #[cfg(test)]
    fn pop(&mut self) -> Option<u32> {
        let (_, n) = self.peek()?;
        self.set(n, None);
        Some(n)
    }

    /// Put `e` in place `i`, then move it up or down to where it belongs,
    /// shifting the entries it passes.
    fn sift(&mut self, mut i: usize, e: (u128, u32)) {
        let start = i;
        while i > 0 {
            let p = (i - 1) / 2;
            if e.0 >= self.heap[p].0 {
                break;
            }
            self.put(i, self.heap[p]);
            i = p;
        }
        if i == start {
            let n = self.heap.len();
            loop {
                let l = 2 * i + 1;
                if l >= n {
                    break;
                }
                let c = if l + 1 < n && self.heap[l + 1].0 < self.heap[l].0 {
                    l + 1
                } else {
                    l
                };
                if self.heap[c].0 >= e.0 {
                    break;
                }
                self.put(i, self.heap[c]);
                i = c;
            }
        }
        self.put(i, e);
    }

    fn put(&mut self, i: usize, e: (u128, u32)) {
        self.heap[i] = e;
        self.at[e.1 as usize] = i as u32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_come_out_in_order() {
        let mut q = Events::new();
        let mut want = Vec::new();
        let mut x: u64 = 3;
        for seq in 0..20_000u64 {
            x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            if (x >> 20).is_multiple_of(3) {
                want.sort();
                let least = (!want.is_empty()).then(|| want.remove(0));
                assert_eq!(q.peek(), least.map(|e: (u128, u64)| e.0));
                assert_eq!(q.pop(), least);
            } else {
                let o = order(Instant::from_micros((x >> 40) % 1000), seq);
                q.push(o, seq);
                want.push((o, seq));
            }
        }
        want.sort();
        for e in want {
            assert_eq!(q.pop(), Some(e));
        }
        assert_eq!(q.pop(), None);
    }

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

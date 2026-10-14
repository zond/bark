//! A vector with a capacity fixed at compile time, stored inline.

/// Up to `N` values of `T`, inline. Slots past `len` hold a filler value
/// and are never observed.
#[derive(Clone)]
pub(crate) struct FixedVec<T: Copy, const N: usize> {
    items: [T; N],
    len: usize,
}

impl<T: Copy, const N: usize> FixedVec<T, N> {
    /// Empty; `fill` initialises the unused slots.
    pub(crate) const fn new(fill: T) -> Self {
        FixedVec { items: [fill; N], len: 0 }
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn is_full(&self) -> bool {
        self.len == N
    }

    pub(crate) fn as_slice(&self) -> &[T] {
        &self.items[..self.len]
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.items[..self.len]
    }

    /// Append, or give the value back if full.
    pub(crate) fn push(&mut self, value: T) -> Result<(), T> {
        if self.is_full() {
            return Err(value);
        }
        self.items[self.len] = value;
        self.len += 1;
        Ok(())
    }

    /// Remove index `i`, shifting the rest down. Keeps order.
    pub(crate) fn remove(&mut self, i: usize) -> T {
        let v = self.items[i];
        self.items.copy_within(i + 1..self.len, i);
        self.len -= 1;
        v
    }

    /// Keep only the elements for which `keep` is true. Keeps order.
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        let mut w = 0;
        for r in 0..self.len {
            if keep(&self.items[r]) {
                self.items[w] = self.items[r];
                w += 1;
            }
        }
        self.len = w;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operations() {
        let mut v: FixedVec<u8, 4> = FixedVec::new(0);
        for i in 1..=4 {
            assert_eq!(v.push(i), Ok(()));
        }
        assert!(v.is_full());
        assert_eq!(v.push(5), Err(5));
        v.as_mut_slice()[0] = 9;
        v.retain(|&x| x != 3);
        assert_eq!(v.as_slice(), &[9, 2, 4]);
        assert_eq!(v.remove(1), 2);
        assert_eq!(v.as_slice(), &[9, 4]);
        assert_eq!(v.len(), 2);
    }
}

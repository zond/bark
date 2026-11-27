//! A turn at something one task does at a time, which several may wait
//! for at once.
//!
//! An embassy-sync `Mutex` keeps one waker
//! (`WakerRegistration::register`), so two tasks waiting on it wake each
//! other until it is free, and the processor does not sleep meanwhile. A
//! [`Queue`] keeps a waker for each of `N` waiters, and wakes them in the
//! order they came.

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::semaphore::{FairSemaphore, Semaphore as _};

/// A turn that `N` tasks may wait for at once.
pub struct Queue<const N: usize>(FairSemaphore<CriticalSectionRawMutex, N>);

impl<const N: usize> Queue<N> {
    pub const fn new() -> Self {
        Queue(FairSemaphore::new(1))
    }

    /// Wait for the turn; it is given back when the value is dropped.
    pub async fn turn(&'static self) -> impl Drop {
        self.0.acquire(1).await.expect("a place for each waiter")
    }
}

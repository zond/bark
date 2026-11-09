//! The log, to the host over the USB serial port, one whole line at a time.
//!
//! A record is formatted into one line buffer, then copied into the ring
//! the port drains only if the ring has room for all of it; otherwise it
//! is dropped and counted, and the count is logged once a minute as
//! `log: N records dropped` (`bark_driver::record`). Lines never merge and
//! a loss is seen.
//!
//! The line buffer is held by a thread-mode lock while a record is
//! formatted: no interrupt is masked and nothing waits for it, since every
//! task logs from thread mode, one poll at a time. A record logged while
//! the buffer is held, from an interrupt or from inside another record's
//! formatting, is dropped and counted rather than waited for. So the
//! record in the buffer is the ring's only writer, and the room it found
//! only grows while it is copied in, one piece of at most [`PIECE`] bytes
//! at a time: interrupts are masked only for one piece's copy.

use core::cell::RefCell;

use bark_driver::record::{put_whole, Drops, Line};
use cortex_m::peripheral::scb::VectActive;
use cortex_m::peripheral::SCB;
use embassy_futures::join::join3;
use embassy_sync::blocking_mutex::raw::{CriticalSectionRawMutex, ThreadModeRawMutex};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::pipe::Pipe;
use embassy_time::{Duration, Ticker};
use embassy_usb::class::cdc_acm::CdcAcmClass;
use embassy_usb::driver::{Driver, EndpointError};

use bark_proto::MAX_FRAME;

/// Ring size, bytes. Lines logged before the host opens the port wait
/// here; once it is full, new lines are dropped, so the banner survives.
pub const RING: usize = 2048;

/// The longest line, bytes, with its end: the console's report of a
/// received frame, its bytes in hex at two digits each, and 128 for the
/// time, the level and the fields beside them. A longer line is cut and
/// ends in `...`.
pub const RECORD: usize = 2 * MAX_FRAME + 128;

/// What the ring takes under one critical section, and what the port
/// sends in one packet: the bulk endpoint's size.
pub const PIECE: usize = crate::USB_PACKET as usize;

/// How often a loss is reported.
const REPORT: Duration = Duration::from_secs(60);

/// The global logger.
pub struct Logger {
    ring: Pipe<CriticalSectionRawMutex, RING>,
    line: Mutex<ThreadModeRawMutex, RefCell<Line<RECORD>>>,
    drops: Drops,
}

impl Logger {
    /// Empty, nothing dropped.
    pub const fn new() -> Logger {
        Logger {
            ring: Pipe::new(),
            line: Mutex::new(RefCell::new(Line::new())),
            drops: Drops::new(),
        }
    }

    /// Formats `record` and puts it in the ring whole; false if it was
    /// dropped, and then counted.
    fn put(&self, record: &log::Record<'_>) -> bool {
        // The thread-mode lock would panic in an interrupt.
        if SCB::vect_active() != VectActive::ThreadMode {
            self.drops.count();
            return false;
        }
        let whole = self.line.lock(|line| {
            let Ok(mut line) = line.try_borrow_mut() else {
                return false;
            };
            line.clear();
            crate::log_line(record, &mut line);
            put_whole(line.finish(), self.ring.free_capacity(), PIECE, |b| {
                self.ring.try_write(b).unwrap_or(0)
            })
        });
        if !whole {
            self.drops.count();
        }
        whole
    }

    /// Logs the records dropped in the last minute, if any. A report that
    /// is itself dropped puts its count back, with itself.
    fn report(&self) {
        let n = self.drops.take();
        if n == 0 {
            return;
        }
        let args = format_args!("log: {} records dropped", n);
        let record = log::Record::builder()
            .args(args)
            .level(log::Level::Warn)
            .target("log")
            .build();
        if !self.put(&record) {
            // `put` counted the report; add what it reported.
            self.drops.add(n);
        }
    }

    /// Drains the ring to the host while the port is open, hands what the
    /// host sends to `input`, and reports losses. Never returns.
    pub async fn run<'d, D: Driver<'d>>(
        &self,
        class: CdcAcmClass<'d, D>,
        input: impl AsyncFn(&[u8]),
    ) -> ! {
        let (mut sender, mut receiver) = class.split();
        let out = async {
            let mut packet = [0u8; PIECE];
            sender.wait_connection().await;
            loop {
                let len = self.ring.read(&mut packet).await;
                // A full packet is followed by an empty one, so the host
                // sees the transfer end (USB 2.0 §5.8.3).
                let sent = match sender.write_packet(&packet[..len]).await {
                    Ok(()) if len == PIECE => sender.write_packet(&[]).await,
                    r => r,
                };
                if sent == Err(EndpointError::Disabled) {
                    sender.wait_connection().await;
                }
            }
        };
        let r#in = async {
            let mut packet = [0u8; PIECE];
            receiver.wait_connection().await;
            loop {
                match receiver.read_packet(&mut packet).await {
                    Ok(n) => input(&packet[..n]).await,
                    Err(EndpointError::Disabled) => receiver.wait_connection().await,
                    Err(_) => {}
                }
            }
        };
        let losses = async {
            let mut every = Ticker::every(REPORT);
            loop {
                every.next().await;
                self.report();
            }
        };
        join3(out, r#in, losses).await;
        unreachable!("the three loops never end")
    }
}

impl log::Log for Logger {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        self.put(record);
    }

    fn flush(&self) {}
}

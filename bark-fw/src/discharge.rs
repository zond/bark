//! The battery's discharge log on the external flash
//! (`bark_image::discharge`, roadmap D178): the node samples its cell an
//! hour apart on battery and at each boot and change of power or mode,
//! and says at boot what the log holds; the console prints it
//! (`discharge`) and erases it (`discharge erase`).
//!
//! A discharge bench build ([`BENCH`], roadmap D190) empties the cell in
//! about a day to learn the curve's shape: it relays at any battery level
//! (`node.rs`), keeps its GPS tracking as a steady load (`gps.rs`), and
//! samples every ten minutes, marking each sample.

use bark_image::discharge::LEN;
use bark_image::ext_flash::PROGRAM_PAGE;
use bark_image::layout::external::DISCHARGE;
use embedded_storage::nor_flash::ReadNorFlash;

use crate::ext_flash;

/// A discharge bench build, from `BARK_DISCHARGE_BENCH=1` at build time:
/// an open node image only, as it ignores the battery's low level and
/// would flatten a cell in the field. Any other value fails the build.
pub const BENCH: bool = match option_env!("BARK_DISCHARGE_BENCH") {
    None => false,
    Some(v) => {
        assert!(
            matches!(v.as_bytes(), b"1"),
            "BARK_DISCHARGE_BENCH is 1 or unset"
        );
        assert!(
            matches!(crate::protect::BUILD, bark_image::protect::Build::Open),
            "BARK_DISCHARGE_BENCH needs BARK_OPEN=1"
        );
        assert!(
            !cfg!(feature = "console"),
            "BARK_DISCHARGE_BENCH is a node image's, not the console's"
        );
        assert!(
            option_env!("BARK_ON_BATTERY").is_none(),
            "BARK_DISCHARGE_BENCH relays on battery; BARK_ON_BATTERY would make it sleepy"
        );
        true
    }
};
// Checked in every build, the console's too, where nothing reads it.
const _: bool = BENCH;

/// Slots in the region.
const SLOTS: u32 = DISCHARGE.len / LEN as u32;

/// The region's slots as the log reads them, a program page at a time,
/// so that a pass over the region wakes the chip once a page, not once a
/// record. A page that cannot be read reads as erased.
fn reader() -> impl FnMut(u32, &mut [u8; LEN]) {
    let mut page = [0u8; PROGRAM_PAGE as usize];
    let mut held: Option<u32> = None;
    move |offset, out| {
        let start = offset - offset % PROGRAM_PAGE;
        if held != Some(start) {
            if ext_flash::with(|c| c.read(DISCHARGE.start + start, &mut page)) != Some(Ok(())) {
                page.fill(0xff);
            }
            held = Some(start);
        }
        let at = (offset - start) as usize;
        out.copy_from_slice(&page[at..at + LEN]);
    }
}

#[cfg(not(feature = "console"))]
pub use node::Log;

/// The node's side: the samples taken and written.
#[cfg(not(feature = "console"))]
mod node {
    use bark_image::discharge::{Now, Ring, Sampler, Summary};
    use bark_image::layout::external::DISCHARGE;

    use super::{reader, SLOTS};
    use crate::ext_flash;

    /// The log as the node's loop keeps it.
    pub struct Log {
        /// The ring and when to sample, once [`Log::open`] found the chip.
        kept: Option<(Ring, Sampler)>,
        /// A reading of the battery the log has not yet seen.
        reading: Option<u16>,
    }

    impl Log {
        /// At boot, after `ext_flash::init`: find where the next sample goes
        /// and log what the log holds.
        pub fn open() -> Log {
            let mut log = Log {
                kept: None,
                reading: None,
            };
            if ext_flash::with(|_| ()).is_none() {
                return log;
            }
            let s = Summary::of(SLOTS, reader());
            if s.count == 0 {
                log::info!("discharge: no samples");
            } else {
                log::info!(
                "discharge: {} samples over {}.{} h, {} mV to {} mV; the console's `discharge` prints them",
                s.count,
                s.span_s / 3600,
                s.span_s % 3600 / 360,
                s.first_mv,
                s.last_mv
            );
            }
            let ring = Ring::open(SLOTS, reader());
            let sampler = if super::BENCH {
                Sampler::bench()
            } else {
                Sampler::new()
            };
            log.kept = Some((ring, sampler));
            log
        }

        /// The driver read the battery: `mv`, uncalibrated.
        pub fn read(&mut self, mv: u16) {
            self.reading = Some(mv);
        }

        /// After each of the driver's steps: take a sample if one is due, and
        /// write it.
        pub async fn note(&mut self, sleepy: bool) {
            let Some((ring, sampler)) = self.kept.as_mut() else {
                return;
            };
            let now = Now {
                uptime_s: embassy_time::Instant::now().as_secs() as u32,
                utc_s: crate::utc::time().utc_s,
                reading: self.reading.take(),
                external: crate::power::reported(),
                sleepy,
                gps: crate::gps::awake(),
            };
            let sample = sampler.note(now);
            let write = sample.map(|s| ring.append(s));
            let (Some(s), Some(w)) = (sample, write) else {
                return;
            };
            log::info!(
                "discharge: {} mv={} external={} sleepy={} gps={} bench={}",
                s.why.name(),
                s.mv,
                s.external,
                s.sleepy,
                s.gps,
                s.bench
            );
            let written = match w.erase {
                Some(e) => ext_flash::erase(DISCHARGE.start + e).await,
                None => Ok(()),
            };
            if let Err(e) = written.and(ext_flash::program(DISCHARGE.start + w.at, &w.bytes).await)
            {
                log::warn!("discharge: sample not written: {:?}", e);
            }
        }
    }
}

#[cfg(feature = "console")]
pub use console::{erase, print};

/// The console's side: the log printed and erased.
#[cfg(feature = "console")]
mod console {
    use bark_image::discharge::{Oldest, BENCH_EVERY_S, EVERY_S};
    use bark_image::layout::external::DISCHARGE;
    use embassy_time::{Duration, Timer};
    use embedded_storage::nor_flash::NorFlash;

    use super::{reader, SLOTS};
    use crate::ext_flash;

    /// A sample's line's longest form, bytes, with the log's prefix: the
    /// pace at which they are printed, one per 64-byte USB packet and
    /// 1 ms frame, so that the log's ring never fills and drops them.
    const LINE: usize = 160;
    const LINE_DRAIN: Duration = Duration::from_millis(LINE.div_ceil(64) as u64);

    /// Every sample, oldest first, a line each between a header and an
    /// end line; `scale` is the calibration the node image keeps, in
    /// 2^-15, by which a reading is multiplied before the levels read it
    /// (`bark_proto::Stored::battery_scale`). The header gives both
    /// intervals, a node image's and a bench build's, as each sample says
    /// which took it.
    pub async fn print(scale: Option<u16>) {
        let (every, bench) = (EVERY_S, BENCH_EVERY_S);
        match scale {
            Some(k) => log::info!(
                "discharge slots={} every_s={} bench_every_s={} scale={}",
                SLOTS,
                every,
                bench,
                k
            ),
            None => log::info!(
                "discharge slots={} every_s={} bench_every_s={} scale=none",
                SLOTS,
                every,
                bench
            ),
        }
        let mut read = reader();
        let mut it = Oldest::start(SLOTS, &mut read);
        let mut count = 0;
        while let Some(s) = it.next(&mut read) {
            count += 1;
            log::info!(
                "discharge seq={} utc_s={} uptime_s={} mv={} external={} sleepy={} gps={} bench={} why={}",
                s.seq,
                s.utc_s,
                s.uptime_s,
                s.mv,
                u8::from(s.external),
                u8::from(s.sleepy),
                u8::from(s.gps),
                u8::from(s.bench),
                s.why.name()
            );
            Timer::after(LINE_DRAIN).await;
        }
        log::info!("discharge end samples={}", count);
    }

    /// Erase the log.
    pub fn erase() {
        match ext_flash::with(|c| c.erase(DISCHARGE.start, DISCHARGE.end())) {
            Some(Ok(())) => log::info!("discharge erased"),
            Some(Err(e)) => log::warn!("discharge: erase failed: {:?}", e),
            None => log::warn!("discharge: no external flash"),
        }
    }
}

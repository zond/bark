//! The battery's discharge log on the external flash
//! (`bark_image::discharge`, roadmap D178): the node samples its cell an
//! hour apart on battery and at each boot and change of power or mode,
//! and says at boot what the log holds; the console prints it
//! (`discharge`) and erases it (`discharge erase`).

use bark_image::discharge::LEN;
use bark_image::ext_flash::PROGRAM_PAGE;
use bark_image::layout::external::DISCHARGE;
use embedded_storage::nor_flash::ReadNorFlash;

use crate::ext_flash;

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
pub use node::{note, open, read};

/// The node's side: the samples taken and written.
#[cfg(not(feature = "console"))]
mod node {
    use core::cell::Cell;

    use bark_image::discharge::{Now, Ring, Sampler, Summary};
    use bark_image::layout::external::DISCHARGE;
    use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
    use embassy_sync::blocking_mutex::Mutex;

    use super::{reader, SLOTS};
    use crate::ext_flash;

    /// The ring and when to sample, once [`open`] found the chip.
    static LOG: Mutex<ThreadModeRawMutex, Cell<Option<(Ring, Sampler)>>> =
        Mutex::new(Cell::new(None));

    /// A reading of the battery the log has not yet seen.
    static READING: Mutex<ThreadModeRawMutex, Cell<Option<u16>>> = Mutex::new(Cell::new(None));

    /// At boot, after `ext_flash::init`: find where the next sample goes
    /// and log what the log holds.
    pub fn open() {
        if ext_flash::with(|_| ()).is_none() {
            return;
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
        LOG.lock(|c| c.set(Some((ring, Sampler::new()))));
    }

    /// The driver read the battery: `mv`, uncalibrated.
    pub fn read(mv: u16) {
        READING.lock(|c| c.set(Some(mv)));
    }

    /// After each of the driver's steps: take a sample if one is due, and
    /// write it.
    pub async fn note(sleepy: bool) {
        let Some((mut ring, mut sampler)) = LOG.lock(Cell::get) else {
            return;
        };
        let now = Now {
            uptime_s: embassy_time::Instant::now().as_secs() as u32,
            utc_s: crate::utc::time().utc_s,
            reading: READING.lock(Cell::take),
            external: crate::power::reported(),
            sleepy,
            gps: crate::gps::awake(),
        };
        let sample = sampler.note(now);
        let write = sample.map(|s| ring.append(s));
        LOG.lock(|c| c.set(Some((ring, sampler))));
        let (Some(s), Some(w)) = (sample, write) else {
            return;
        };
        log::info!(
            "discharge: {} mv={} external={} sleepy={} gps={}",
            s.why.name(),
            s.mv,
            s.external,
            s.sleepy,
            s.gps
        );
        let written = match w.erase {
            Some(e) => ext_flash::erase(DISCHARGE.start + e).await,
            None => Ok(()),
        };
        if let Err(e) = written.and(ext_flash::program(DISCHARGE.start + w.at, &w.bytes).await) {
            log::warn!("discharge: sample not written: {:?}", e);
        }
    }
}

#[cfg(feature = "console")]
pub use console::{erase, print};

/// The console's side: the log printed and erased.
#[cfg(feature = "console")]
mod console {
    use bark_image::discharge::{Oldest, EVERY_S};
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
    /// (`bark_proto::Stored::battery_scale`).
    pub async fn print(scale: Option<u16>) {
        match scale {
            Some(k) => log::info!("discharge slots={} every_s={} scale={}", SLOTS, EVERY_S, k),
            None => log::info!("discharge slots={} every_s={} scale=none", SLOTS, EVERY_S),
        }
        let mut read = reader();
        let mut it = Oldest::start(SLOTS, &mut read);
        let mut count = 0;
        while let Some(s) = it.next(&mut read) {
            count += 1;
            log::info!(
                "discharge seq={} utc_s={} uptime_s={} mv={} external={} sleepy={} gps={} why={}",
                s.seq,
                s.utc_s,
                s.uptime_s,
                s.mv,
                u8::from(s.external),
                u8::from(s.sleepy),
                u8::from(s.gps),
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

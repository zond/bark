//! The node's neighbour table and message records, kept across the
//! reboot into an image gathered over the mesh
//! (spec "Updates over the mesh", the install; roadmap D180;
//! `bark_app_layer::tables`): written to their region of the external
//! flash just before the reset, and given back to the core at the first
//! boot of the new image, aged by the time the swap took, then erased.
//!
//! The board has no driver for its real-time clock, so the time away is
//! taken from the loader's figures for the image swapped in
//! (`bark_image::boot::time`), with the time from the tables' taking to
//! the reset and from the boot to their return, which the clock measures.

use bark_app_layer::tables::{bytes, Check, Header, HEADER_LEN, MAX_LEN};
use bark_image::ext_flash::PROGRAM_PAGE;
use bark_image::layout::external::{SECTOR, TABLES};
use bark_proto::Duration;
use embedded_storage::nor_flash::ReadNorFlash;

use crate::ext_flash;
use crate::node::Role;

// A full neighbour table and every record fit, so none is left out.
const _: () = assert!(MAX_LEN as u32 <= TABLES.len);

const PAGE: usize = PROGRAM_PAGE as usize;

async fn erase() {
    for k in 0..TABLES.len / SECTOR {
        if let Err(e) = ext_flash::erase(TABLES.start + k * SECTOR).await {
            log::error!("tables: erase {:?}", e);
        }
    }
}

/// Keep `core`'s tables, just before the reset: the entries a page at a
/// time, then the header.
pub async fn keep(core: &Role) {
    erase().await;
    let taken = embassy_time::Instant::now();
    let now = crate::clock::core(taken);
    let mut page = [0xFFu8; PAGE];
    let mut fill = 0;
    let mut at = TABLES.start + HEADER_LEN as u32;
    let mut check = Check::new();
    let mut counts = [0u16; 2];
    let mut failed = false;
    for k in core.remembered(now) {
        counts[usize::from(matches!(k, bark_proto::Kept::Record(_)))] += 1;
        let mut b = bytes(&k);
        check.add(b);
        while !b.is_empty() {
            let n = b.len().min(PAGE - fill);
            page[fill..fill + n].copy_from_slice(&b[..n]);
            (fill, b) = (fill + n, &b[n..]);
            if fill == PAGE {
                failed |= ext_flash::program(at, &page).await.is_err();
                (fill, at) = (0, at + PAGE as u32);
            }
        }
    }
    if fill > 0 {
        failed |= ext_flash::program(at, &page[..fill]).await.is_err();
    }
    if failed {
        log::error!("tables: a write failed; none kept");
        return;
    }
    let written_us = taken.elapsed().as_micros() as u32;
    let header = Header::new(counts[0], counts[1], written_us, check);
    match ext_flash::program(TABLES.start, &header.encode()).await {
        Ok(()) => log::info!(
            "tables: {} neighbours and {} records kept across the reboot",
            counts[0],
            counts[1]
        ),
        Err(e) => log::error!("tables: header {:?}; none kept", e),
    }
}

/// Entry `i` of the tables `h` heads, read into `buf`.
fn read<'a>(h: &Header, i: usize, buf: &'a mut [u8; PAGE]) -> Option<&'a [u8]> {
    let (at, len) = h.place(i);
    let b = &mut buf[..len];
    (ext_flash::with(|c| c.read(TABLES.start + at as u32, b)) == Some(Ok(()))).then_some(b)
}

/// At boot, after `update::started`: give `core` the tables kept, if the
/// boot is the first of an image of `swapped` bytes just swapped in, and
/// erase them. Tables found at any other boot are from a reboot whose
/// time away is not known, and are only erased.
pub async fn restore(core: &mut Role, swapped: Option<u32>) {
    let mut buf = [0u8; PAGE];
    let header = (ext_flash::with(|c| c.read(TABLES.start, &mut buf)) == Some(Ok(())))
        .then(|| Header::decode(&buf))
        .flatten();
    let Some(h) = header else {
        return;
    };
    let Some(length) = swapped else {
        log::info!("tables: kept, but this boot follows no swap; erased unused");
        return erase().await;
    };
    // A torn write fails the check and gives nothing back.
    let mut check = Check::new();
    for i in 0..h.entries() {
        match read(&h, i, &mut buf) {
            Some(b) => check.add(b),
            None => return erase().await,
        }
    }
    if !h.matches(check) {
        log::warn!("tables: the write was cut short; erased unused");
        return erase().await;
    }
    let now = crate::clock::core_now();
    let ago = Duration::from_micros(
        bark_image::boot::time::swap_us(length)
            + u64::from(h.written_us)
            + embassy_time::Instant::now().as_micros(),
    );
    let mut back = [0u16; 2];
    for i in 0..h.entries() {
        let Some(k) = read(&h, i, &mut buf).and_then(|b| h.entry(i, b)) else {
            continue;
        };
        if core.remember(&k, ago, now) {
            back[usize::from(matches!(k, bark_proto::Kept::Record(_)))] += 1;
        }
    }
    log::info!(
        "tables: {} of {} neighbours and {} of {} records back, {} ms away assumed",
        back[0],
        h.neighbours,
        back[1],
        h.records,
        ago.as_millis()
    );
    erase().await;
}

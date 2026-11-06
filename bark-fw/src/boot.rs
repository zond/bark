//! What the node learnt while it booted, kept for one `boot:` line in its
//! first 10 s report (`node.rs`). The lines logged at boot reach the host
//! only if it holds the serial port open by then, which a host that
//! reopens the port after a flash misses by seconds; the report comes
//! after.

use core::cell::Cell;
use core::fmt;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;

use crate::display::Outcome;
use crate::lfclk::Source;

/// The facts, each set once by the part that learns it.
#[derive(Clone, Copy, Default)]
pub struct Facts {
    pub lfclk: Option<Source>,
    pub display: Option<Outcome>,
    /// The BLE controller's memory: what it needs and what it has.
    pub controller: Option<(usize, usize)>,
    /// The GPS's state now (`gps.rs`).
    pub gps: &'static str,
    /// The debug port's read-out protection (`protect.rs`).
    pub protection: Option<crate::protect::State>,
}

static FACTS: Mutex<CriticalSectionRawMutex, Cell<Facts>> = Mutex::new(Cell::new(Facts {
    lfclk: None,
    display: None,
    controller: None,
    gps: "rest",
    protection: None,
}));

/// Note a fact.
pub fn note(f: impl FnOnce(&mut Facts)) {
    FACTS.lock(|c| {
        let mut facts = c.get();
        f(&mut facts);
        c.set(facts);
    });
}

/// The facts so far.
pub fn facts() -> Facts {
    FACTS.lock(Cell::get)
}

extern "C" {
    /// The start of the image's initialised data in flash, and the data's
    /// bounds in RAM (cortex-m-rt's `link.x`): the image ends where the
    /// data's copy in flash does.
    static __sidata: u32;
    static __sdata: u32;
    static __edata: u32;
}

/// Bytes of flash the image takes from the start of its slot.
pub fn image_len() -> u32 {
    use core::ptr::addr_of;
    let data = addr_of!(__edata) as u32 - addr_of!(__sdata) as u32;
    addr_of!(__sidata) as u32 + data - bark_image::layout::ACTIVE.start
}

impl fmt::Display for Facts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.lfclk {
            Some(Source::Crystal { started_ms }) => write!(f, "lfclk=crystal in {started_ms} ms")?,
            Some(Source::Rc) => f.write_str("lfclk=rc")?,
            None => f.write_str("lfclk=?")?,
        }
        match self.display {
            Some(Outcome::Shown { ms }) => write!(f, " display=shown in {ms} ms")?,
            Some(Outcome::Busy(step)) => write!(f, " display=busy at {step}")?,
            Some(Outcome::NoRefresh) => f.write_str(" display=no refresh")?,
            Some(Outcome::Spi) => f.write_str(" display=spi error")?,
            None => f.write_str(" display=pending")?,
        }
        match self.controller {
            Some((need, have)) => write!(f, " controller_memory={need} of {have}")?,
            None => f.write_str(" controller_memory=?")?,
        }
        write!(f, " gps={}", self.gps)?;
        if let Some(p) = self.protection {
            write!(f, " {p}")?;
        }
        let slot = bark_image::layout::ACTIVE.len;
        write!(
            f,
            " image_version={} image={} of {} B",
            crate::update::BARK_IMAGE_VERSION,
            image_len(),
            slot
        )
    }
}

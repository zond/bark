//! The battery's discharge log: samples of the cell's voltage kept on the
//! external flash ([`DISCHARGE`]), so that the cell's curve can be learned
//! from a discharge of days to weeks, where no host reads the USB log.
//!
//! [`Sampler`] says when to sample: every [`EVERY_S`] on battery (or
//! [`BENCH_EVERY_S`] in a discharge bench build), at each boot, and at
//! each change of power or mode. [`Ring`] writes each as a checked record,
//! so a record a power cut tore is skipped. The ring wraps, erasing a
//! sector at a time: the discharge to read is the one that just ended, so
//! the newest samples are the ones to keep.
//!
//! [`DISCHARGE`]: crate::layout::external::DISCHARGE

use crate::layout::external::SECTOR;

/// Bytes of a record ([`Sample::encode`]): 256 to a sector, so none
/// straddles a program page of 256 bytes.
pub const LEN: usize = 16;

/// Records to a sector.
pub const PER_SECTOR: u32 = SECTOR / LEN as u32;

/// Seconds between samples on battery: an hour. A sleepy node's 59 days
/// are 1416 samples, which [`DISCHARGE`] holds; the battery estimate's
/// 15 h (`bark_proto::drain::SAMPLE`) spans 15. A finer interval would
/// show the readings' noise, a step of the mapping being some 5.7 mV.
///
/// [`DISCHARGE`]: crate::layout::external::DISCHARGE
pub const EVERY_S: u32 = 3600;

/// Seconds between samples on battery in a discharge bench build, a relay
/// with its GPS tracking at about 35.4 mA: a sample then spans about as
/// much charge as a relay's hour at 6.41 mA, 3600 s × 6.41 / 35.4 = 652 s,
/// rounded. [`DISCHARGE`] holds 14 days of them.
///
/// [`DISCHARGE`]: crate::layout::external::DISCHARGE
pub const BENCH_EVERY_S: u32 = 600;

/// The erased value of the flash (ZD25WQ16B datasheet, "Sector Erase").
const ERASED: u8 = 0xff;

/// Why a sample was taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Why {
    /// [`Sampler::every_s`] after the last on battery.
    Interval = 0,
    /// The first reading after a boot, or the boot itself on USB.
    Boot = 1,
    /// USB came: the last reading on battery, the end of a discharge.
    Plugged = 2,
    /// USB went: the first reading on battery, the start of a discharge.
    Unplugged = 3,
    /// The node turned sleepy or relay, or its GPS started or stopped.
    Mode = 4,
}

impl Why {
    fn from_bits(b: u8) -> Option<Why> {
        Some(match b {
            0 => Why::Interval,
            1 => Why::Boot,
            2 => Why::Plugged,
            3 => Why::Unplugged,
            4 => Why::Mode,
            _ => return None,
        })
    }

    /// The word the console prints.
    pub fn name(self) -> &'static str {
        match self {
            Why::Interval => "interval",
            Why::Boot => "boot",
            Why::Plugged => "plugged",
            Why::Unplugged => "unplugged",
            Why::Mode => "mode",
        }
    }
}

/// One sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// Its place in the log, one more than the sample before, across
    /// boots; set by [`Ring::append`]. 24 bits, which at a sample an hour
    /// last 1900 years.
    pub seq: u32,
    /// UTC seconds since 1970 from the GPS or the phone, 0 if the node
    /// knew no time. 32 bits last until 2106.
    pub utc_s: u32,
    /// Seconds since the boot.
    pub uptime_s: u32,
    /// The cell's millivolts as the converter read them, uncalibrated; 0
    /// for none read since the boot, as on USB, where the node does not
    /// read the battery.
    pub mv: u16,
    /// On USB power.
    pub external: bool,
    /// Sleepy, else a relay.
    pub sleepy: bool,
    /// The GPS running, searching or tracking.
    pub gps: bool,
    /// Taken by a discharge bench build ([`BENCH_EVERY_S`]).
    pub bench: bool,
    /// Why it was taken.
    pub why: Why,
}

/// CRC-16/CCITT, polynomial 0x1021, from 0xffff: a record torn by a
/// power cut, or programmed all zero, fails it, and whatever the chip
/// held before the log was there passes it once in 65 536 slots.
fn crc16(bytes: &[u8]) -> u16 {
    bytes.iter().fold(0xffff, |mut c, &b| {
        c ^= u16::from(b) << 8;
        for _ in 0..8 {
            c = if c & 0x8000 != 0 {
                (c << 1) ^ 0x1021
            } else {
                c << 1
            };
        }
        c
    })
}

/// The largest sequence number.
const SEQ_MASK: u32 = 0xff_ffff;

/// Where the check lies.
const CHECK: usize = LEN - 2;

impl Sample {
    /// The record, little-endian: `seq` in 3 bytes, `utc_s` and
    /// `uptime_s` in 4, `mv` in 2, a byte of flags (bit 0 external, 1
    /// sleepy, 2 GPS, 3 bench, 4 to 6 why), and the check in 2.
    pub fn encode(&self) -> [u8; LEN] {
        let mut r = [0u8; LEN];
        r[0..3].copy_from_slice(&(self.seq & SEQ_MASK).to_le_bytes()[..3]);
        r[3..7].copy_from_slice(&self.utc_s.to_le_bytes());
        r[7..11].copy_from_slice(&self.uptime_s.to_le_bytes());
        r[11..13].copy_from_slice(&self.mv.to_le_bytes());
        r[13] = u8::from(self.external)
            | u8::from(self.sleepy) << 1
            | u8::from(self.gps) << 2
            | u8::from(self.bench) << 3
            | (self.why as u8) << 4;
        let check = crc16(&r[..CHECK]);
        r[CHECK..].copy_from_slice(&check.to_le_bytes());
        r
    }

    /// The sample a record holds, `None` if it is erased or torn.
    pub fn decode(r: &[u8; LEN]) -> Option<Sample> {
        if crc16(&r[..CHECK]).to_le_bytes() != r[CHECK..] {
            return None;
        }
        let word = |i: usize| u32::from_le_bytes([r[i], r[i + 1], r[i + 2], r[i + 3]]);
        let flags = r[13];
        Some(Sample {
            seq: u32::from_le_bytes([r[0], r[1], r[2], 0]),
            utc_s: word(3),
            uptime_s: word(7),
            mv: u16::from_le_bytes([r[11], r[12]]),
            external: flags & 1 != 0,
            sleepy: flags & 2 != 0,
            gps: flags & 4 != 0,
            bench: flags & 8 != 0,
            why: Why::from_bits(flags >> 4 & 7)?,
        })
    }
}

/// What a slot holds.
enum Slot {
    Erased,
    Torn,
    Kept(Sample),
}

fn slot(read: &mut impl FnMut(u32, &mut [u8; LEN]), i: u32) -> Slot {
    let mut r = [0u8; LEN];
    read(i * LEN as u32, &mut r);
    match Sample::decode(&r) {
        Some(s) => Slot::Kept(s),
        None if r.iter().all(|&b| b == ERASED) => Slot::Erased,
        None => Slot::Torn,
    }
}

/// The newest sample's slot and sample.
fn newest(slots: u32, read: &mut impl FnMut(u32, &mut [u8; LEN])) -> Option<(u32, Sample)> {
    let mut newest: Option<(u32, Sample)> = None;
    for i in 0..slots {
        if let Slot::Kept(s) = slot(read, i) {
            if newest.is_none_or(|(_, n)| s.seq > n.seq) {
                newest = Some((i, s));
            }
        }
    }
    newest
}

/// The samples, oldest first, round the ring from the slot after the
/// newest. A sample an erase cut short left behind is older than every one
/// written after it, so it is passed over where it lies out of order.
#[derive(Clone, Copy, Debug)]
pub struct Oldest {
    slots: u32,
    newest: u32,
    /// Slots read so far after the newest.
    k: u32,
    last: Option<u32>,
}

impl Oldest {
    /// From the oldest of the ring of `slots` slots that `read` reads.
    pub fn start(slots: u32, mut read: impl FnMut(u32, &mut [u8; LEN])) -> Oldest {
        let (newest, k) = match newest(slots, &mut read) {
            Some((at, _)) => (at, 0),
            None => (0, slots),
        };
        Oldest {
            slots,
            newest,
            k,
            last: None,
        }
    }

    /// The next sample, `None` after the newest.
    pub fn next(&mut self, mut read: impl FnMut(u32, &mut [u8; LEN])) -> Option<Sample> {
        while self.k < self.slots {
            self.k += 1;
            if let Slot::Kept(s) = slot(&mut read, (self.newest + self.k) % self.slots) {
                if self.last.is_none_or(|l| s.seq > l) {
                    self.last = Some(s.seq);
                    return Some(s);
                }
            }
        }
        None
    }
}

/// Each sample, oldest first ([`Oldest`]).
pub fn each(slots: u32, mut read: impl FnMut(u32, &mut [u8; LEN]), mut f: impl FnMut(&Sample)) {
    let mut it = Oldest::start(slots, &mut read);
    while let Some(s) = it.next(&mut read) {
        f(&s);
    }
}

/// What the log holds, for the boot's line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    /// Samples.
    pub count: u32,
    /// The oldest reading and the newest, millivolts, 0 for none.
    pub first_mv: u16,
    /// See `first_mv`.
    pub last_mv: u16,
    /// Seconds the samples span: within a boot by the uptime, across one
    /// by the time where both samples have it, else not counted.
    pub span_s: u32,
}

impl Summary {
    /// The summary of the log `read` reads.
    pub fn of(slots: u32, read: impl FnMut(u32, &mut [u8; LEN])) -> Summary {
        let mut sum = Summary::default();
        let mut before: Option<Sample> = None;
        each(slots, read, |s| {
            sum.count += 1;
            if s.mv != 0 {
                if sum.first_mv == 0 {
                    sum.first_mv = s.mv;
                }
                sum.last_mv = s.mv;
            }
            if let Some(b) = before {
                sum.span_s += between(&b, s).unwrap_or(0);
            }
            before = Some(*s);
        });
        sum
    }
}

/// Seconds from sample `a` to the next, `b`, where they tell it.
pub fn between(a: &Sample, b: &Sample) -> Option<u32> {
    if b.why != Why::Boot && b.uptime_s >= a.uptime_s {
        Some(b.uptime_s - a.uptime_s)
    } else if a.utc_s != 0 && b.utc_s >= a.utc_s {
        Some(b.utc_s - a.utc_s)
    } else {
        None
    }
}

/// Where the next sample goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ring {
    slots: u32,
    next: u32,
    seq: u32,
}

/// A sample's write: erase the sector at `erase` first, if any, then
/// program `bytes` at `at`; offsets from the region's start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Write {
    /// The sector to erase first.
    pub erase: Option<u32>,
    /// Where the record goes.
    pub at: u32,
    /// The record.
    pub bytes: [u8; LEN],
}

impl Ring {
    /// The ring of `slots` slots that `read` reads. The next sample goes
    /// after the newest and after anything else written in its sector,
    /// torn or left by a cut erase.
    pub fn open(slots: u32, mut read: impl FnMut(u32, &mut [u8; LEN])) -> Ring {
        assert!(slots >= 2 * PER_SECTOR && slots.is_multiple_of(PER_SECTOR));
        let Some((at, s)) = newest(slots, &mut read) else {
            return Ring {
                slots,
                next: 0,
                seq: 0,
            };
        };
        let end = (at + 1).next_multiple_of(PER_SECTOR);
        let mut next = at + 1;
        for i in at + 1..end {
            if !matches!(slot(&mut read, i), Slot::Erased) {
                next = i + 1;
            }
        }
        Ring {
            slots,
            next: next % slots,
            seq: (s.seq + 1) & SEQ_MASK,
        }
    }

    /// Write `sample` as the next, its `seq` set.
    pub fn append(&mut self, mut sample: Sample) -> Write {
        sample.seq = self.seq;
        let at = self.next * LEN as u32;
        let w = Write {
            erase: self.next.is_multiple_of(PER_SECTOR).then_some(at),
            at,
            bytes: sample.encode(),
        };
        self.next = (self.next + 1) % self.slots;
        self.seq = (self.seq + 1) & SEQ_MASK;
        w
    }
}

/// What the node is doing at a step, for [`Sampler::note`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Now {
    /// Seconds since the boot.
    pub uptime_s: u32,
    /// UTC seconds, if the node knows the time.
    pub utc_s: Option<u64>,
    /// A reading of the battery taken since the last step: the driver
    /// reads it once a minute, on battery only.
    pub reading: Option<u16>,
    /// On USB power.
    pub external: bool,
    /// Sleepy, else a relay.
    pub sleepy: bool,
    /// The GPS running.
    pub gps: bool,
}

/// When to take a sample.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sampler {
    /// A discharge bench build's: a sample every [`BENCH_EVERY_S`].
    bench: bool,
    /// The boot's sample was taken.
    booted: bool,
    /// The power at the last step.
    external: Option<bool>,
    /// USB went and the first reading on battery is not yet taken.
    unplugged: bool,
    /// The last reading since the boot, 0 for none.
    mv: u16,
    /// The last sample's uptime and mode.
    last: Option<(u32, bool, bool)>,
}

impl Sampler {
    /// Nothing sampled since the boot, in a node image.
    pub const fn new() -> Sampler {
        Sampler::with(false)
    }

    /// Nothing sampled since the boot, in a discharge bench build.
    pub const fn bench() -> Sampler {
        Sampler::with(true)
    }

    const fn with(bench: bool) -> Sampler {
        Sampler {
            bench,
            booted: false,
            external: None,
            unplugged: false,
            mv: 0,
            last: None,
        }
    }

    /// The sample to take at this step, if any; its `seq` is the ring's
    /// to set.
    pub fn note(&mut self, now: Now) -> Option<Sample> {
        if let Some(mv) = now.reading {
            self.mv = mv;
        }
        let was = self.external.replace(now.external);
        let why = if !self.booted {
            // On battery the boot's sample waits for the first reading:
            // the cell as it rested before the boot's load.
            (now.external || now.reading.is_some()).then_some(Why::Boot)
        } else if was == Some(false) && now.external {
            // The reading of at most a minute ago, the discharge's last.
            Some(Why::Plugged)
        } else {
            self.unplugged |= was == Some(true) && !now.external;
            let (Some(_), Some((at, sleepy, gps))) = (now.reading, self.last) else {
                return None;
            };
            if self.unplugged {
                Some(Why::Unplugged)
            } else if (sleepy, gps) != (now.sleepy, now.gps) {
                Some(Why::Mode)
            } else if now.uptime_s.saturating_sub(at) >= self.every_s() {
                Some(Why::Interval)
            } else {
                None
            }
        }?;
        self.booted = true;
        self.unplugged = false;
        self.last = Some((now.uptime_s, now.sleepy, now.gps));
        Some(Sample {
            seq: 0,
            utc_s: now.utc_s.and_then(|s| u32::try_from(s).ok()).unwrap_or(0),
            uptime_s: now.uptime_s,
            mv: if now.external && why != Why::Plugged {
                0
            } else {
                self.mv
            },
            external: now.external,
            sleepy: now.sleepy,
            gps: now.gps,
            bench: self.bench,
            why,
        })
    }

    /// Seconds between samples on battery.
    pub const fn every_s(&self) -> u32 {
        if self.bench {
            BENCH_EVERY_S
        } else {
            EVERY_S
        }
    }
}

#[cfg(test)]
mod tests;

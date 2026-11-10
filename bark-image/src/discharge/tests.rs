use std::vec;
use std::vec::Vec;

use super::*;
use crate::layout::external::DISCHARGE;

const SLOTS: u32 = DISCHARGE.len / LEN as u32;

/// A region of NOR flash: erased to ones, programmed by clearing bits.
struct Flash(Vec<u8>);

impl Flash {
    fn new() -> Flash {
        Flash(vec![ERASED; DISCHARGE.len as usize])
    }

    fn read(&self) -> impl FnMut(u32, &mut [u8; LEN]) + '_ {
        |at, out| out.copy_from_slice(&self.0[at as usize..at as usize + LEN])
    }

    fn apply(&mut self, w: Write) {
        if let Some(e) = w.erase {
            self.0[e as usize..(e + SECTOR) as usize].fill(ERASED);
        }
        for (d, b) in self.0[w.at as usize..].iter_mut().zip(w.bytes) {
            *d &= b;
        }
    }

    fn samples(&self) -> Vec<Sample> {
        let mut v = Vec::new();
        each(SLOTS, self.read(), |s| v.push(*s));
        v
    }
}

fn sample(uptime_s: u32, mv: u16) -> Sample {
    Sample {
        seq: 0,
        utc_s: 1_790_000_000 + uptime_s,
        uptime_s,
        mv,
        external: false,
        sleepy: true,
        gps: false,
        bench: false,
        why: Why::Interval,
    }
}

#[test]
fn a_record_round_trips_and_a_torn_or_erased_one_is_none() {
    let s = Sample {
        seq: 70_000,
        utc_s: 1_794_000_000,
        uptime_s: 123_456,
        mv: 3_912,
        external: true,
        sleepy: false,
        gps: true,
        bench: true,
        why: Why::Unplugged,
    };
    let r = s.encode();
    assert_eq!(Sample::decode(&r), Some(s));
    assert_eq!(Sample::decode(&[ERASED; LEN]), None);
    assert_eq!(Sample::decode(&[0; LEN]), None);
    // A program cut short leaves some bits of its bytes still set.
    for cut in 0..LEN {
        let mut torn = r;
        for b in &mut torn[cut..] {
            *b = ERASED;
        }
        assert_eq!(Sample::decode(&torn), None, "cut at {cut}");
    }
}

#[test]
fn the_log_is_read_oldest_first_and_wraps_losing_a_sector() {
    let mut f = Flash::new();
    let total = SLOTS + 10;
    for i in 0..total {
        let mut ring = Ring::open(SLOTS, f.read());
        f.apply(ring.append(sample(i * EVERY_S, 4000 - i as u16 / 4)));
    }
    let v = f.samples();
    // The sector the wrap erased holds only the ten newest.
    assert_eq!(v.len() as u32, SLOTS - PER_SECTOR + 10);
    assert!(v.windows(2).all(|w| w[1].seq == w[0].seq + 1));
    assert_eq!(v.last().unwrap().seq, total - 1);
    let sum = Summary::of(SLOTS, f.read());
    assert_eq!(sum.count, v.len() as u32);
    assert_eq!(sum.first_mv, v[0].mv);
    assert_eq!(sum.last_mv, v.last().unwrap().mv);
    assert_eq!(sum.span_s, (v.len() as u32 - 1) * EVERY_S);
}

#[test]
fn a_torn_record_is_skipped_and_the_next_goes_after_it() {
    let mut f = Flash::new();
    let mut ring = Ring::open(SLOTS, f.read());
    for i in 0..3 {
        f.apply(ring.append(sample(i * EVERY_S, 4000)));
    }
    // The fourth's program cut after its first half.
    let mut w = ring.append(sample(3 * EVERY_S, 3990));
    w.bytes[8..].fill(ERASED);
    f.apply(w);
    let mut ring = Ring::open(SLOTS, f.read());
    let w = ring.append(sample(4 * EVERY_S, 3980));
    assert_eq!(w.at, 4 * LEN as u32);
    assert_eq!(w.erase, None);
    f.apply(w);
    let v = f.samples();
    assert_eq!(
        v.iter().map(|s| s.seq).collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    assert_eq!(v[3].mv, 3980);
}

#[test]
fn a_cut_erase_leaves_old_samples_out_of_order_and_passed_over() {
    let mut f = Flash::new();
    let mut ring = Ring::open(SLOTS, f.read());
    for i in 0..SLOTS {
        f.apply(ring.append(sample(i, 4000)));
    }
    // The wrap's erase of sector 0 cut short: the sector's second half
    // keeps its old samples.
    let w = ring.append(sample(SLOTS, 3900));
    assert_eq!(w.erase, Some(0));
    f.0[..SECTOR as usize / 2].fill(ERASED);
    f.apply(Write { erase: None, ..w });
    // After a reboot the next samples go to the next sector, which they
    // erase, past what the cut left in this one.
    let mut ring = Ring::open(SLOTS, f.read());
    let w = ring.append(sample(SLOTS + 1, 3900));
    assert_eq!(w.erase, Some(SECTOR));
    f.apply(w);
    for i in 2..PER_SECTOR {
        f.apply(ring.append(sample(SLOTS + i, 3900)));
    }
    let v = f.samples();
    assert!(v.windows(2).all(|w| w[1].seq > w[0].seq));
    assert_eq!(v.last().unwrap().seq, SLOTS + PER_SECTOR - 1);
    // The old samples the cut left lie after a newer one and are passed
    // over; those of the six sectors not erased are read.
    assert_eq!(
        v.iter().filter(|s| s.mv == 4000).count() as u32,
        SLOTS - 2 * PER_SECTOR
    );
}

#[test]
fn an_empty_log_starts_at_the_first_sector_and_erases_it() {
    let f = Flash::new();
    assert_eq!(Summary::of(SLOTS, f.read()), Summary::default());
    let mut ring = Ring::open(SLOTS, f.read());
    let w = ring.append(sample(0, 4100));
    assert_eq!((w.erase, w.at), (Some(0), 0));
    assert_eq!(Sample::decode(&w.bytes).unwrap().seq, 0);
}

fn now(uptime_s: u32, reading: Option<u16>, external: bool) -> Now {
    Now {
        uptime_s,
        utc_s: None,
        reading,
        external,
        sleepy: !external,
        gps: false,
    }
}

#[test]
fn a_boot_on_battery_is_sampled_at_its_first_reading_then_hourly() {
    let mut s = Sampler::new();
    assert_eq!(s.note(now(0, None, false)), None);
    let boot = s.note(now(1, Some(4100), false)).unwrap();
    assert_eq!((boot.why, boot.mv), (Why::Boot, 4100));
    for t in (61..3600).step_by(60) {
        assert_eq!(s.note(now(t, Some(4099), false)), None);
    }
    let hour = s.note(now(3601, Some(4090), false)).unwrap();
    assert_eq!(
        (hour.why, hour.mv, hour.uptime_s),
        (Why::Interval, 4090, 3601)
    );
    // Only the readings sample; a step between them does not.
    assert_eq!(s.note(now(7300, None, false)), None);
    assert_eq!(
        s.note(now(7301, Some(4080), false)).unwrap().why,
        Why::Interval
    );
}

#[test]
fn plugging_in_and_out_marks_the_ends_of_a_discharge() {
    let mut s = Sampler::new();
    let boot = s.note(now(0, None, true)).unwrap();
    assert_eq!((boot.why, boot.mv, boot.external), (Why::Boot, 0, true));
    assert_eq!(s.note(now(10, None, true)), None);
    // USB goes; the driver reads at its next step.
    assert_eq!(s.note(now(20, None, false)), None);
    let out = s.note(now(21, Some(4150), false)).unwrap();
    assert_eq!((out.why, out.mv), (Why::Unplugged, 4150));
    assert_eq!(s.note(now(81, Some(4140), false)), None);
    let back = s.note(now(100, None, true)).unwrap();
    assert_eq!(
        (back.why, back.mv, back.external),
        (Why::Plugged, 4140, true)
    );
    assert_eq!(s.note(now(200, None, true)), None);
}

#[test]
fn a_change_of_mode_is_sampled_at_the_next_reading() {
    let mut s = Sampler::new();
    s.note(now(0, Some(3800), false)).unwrap();
    let mut n = now(60, Some(3790), false);
    n.gps = true;
    assert_eq!(s.note(n).unwrap().why, Why::Mode);
    assert_eq!(s.note(n), None);
    n.sleepy = false;
    n.uptime_s = 120;
    let m = s.note(n).unwrap();
    assert_eq!((m.why, m.sleepy, m.gps), (Why::Mode, false, true));
}

#[test]
fn the_time_is_kept_where_known() {
    let mut s = Sampler::new();
    let mut n = now(0, Some(4000), false);
    n.utc_s = Some(1_794_300_000);
    assert_eq!(s.note(n).unwrap().utc_s, 1_794_300_000);
}

#[test]
fn a_bench_build_samples_every_ten_minutes_and_marks_its_samples() {
    let mut s = Sampler::bench();
    let boot = s.note(now(1, Some(4100), false)).unwrap();
    assert!(boot.bench);
    for t in (61..BENCH_EVERY_S).step_by(60) {
        assert_eq!(s.note(now(t, Some(4099), false)), None);
    }
    let next = s.note(now(BENCH_EVERY_S + 1, Some(4095), false)).unwrap();
    assert_eq!((next.why, next.bench), (Why::Interval, true));
    assert_eq!(Sample::decode(&next.encode()), Some(next));
    assert!(
        !Sampler::new()
            .note(now(1, Some(4100), false))
            .unwrap()
            .bench
    );
}

//! The cell's curve against the run it was fitted to
//! (`bark_proto::node::cell_percent`; `docs/discharge-2026-11-11.md`).

use bark_proto::node::cell_percent;

/// The run: board f4f5's discharge at a constant 35.4 mA, from the sample
/// where USB went to the last before the board stopped.
const RUN: &str = include_str!("../../docs/data/discharge-f4f5-2026-11-11.csv");
const FIRST_SEQ: u32 = 49;
const LAST_SEQ: u32 = 179;

/// The largest error the curve's comment states, percent of the charge.
const LARGEST: f64 = 3.1;

/// The run's samples, oldest first: seconds since the boot, reading.
fn samples() -> Vec<(u32, u16)> {
    let mut lines = RUN.lines();
    let header: Vec<&str> = lines.next().unwrap().split(',').collect();
    let col = |name: &str| header.iter().position(|&h| h == name).unwrap();
    let (seq, up, mv) = (col("seq"), col("uptime_s"), col("mv"));
    let mut out = Vec::new();
    for line in lines {
        let f: Vec<&str> = line.split(',').collect();
        let s: u32 = f[seq].parse().unwrap();
        if (FIRST_SEQ..=LAST_SEQ).contains(&s) {
            assert_eq!(s, FIRST_SEQ + out.len() as u32, "a sample missing");
            out.push((f[up].parse().unwrap(), f[mv].parse().unwrap()));
        }
    }
    out
}

/// At a constant current the charge left falls evenly in time, from
/// 100 % at the first sample to 0 at the last. The curve reads every
/// sample within its stated error.
#[test]
fn the_curve_reads_the_run_within_its_error() {
    let run = samples();
    assert_eq!(run.len(), 131);
    let (start, end) = (run[0].0, run[run.len() - 1].0);
    let mut squares = 0.0;
    for &(t, mv) in &run {
        let truth = 100.0 * f64::from(end - t) / f64::from(end - start);
        let e = f64::from(cell_percent(mv)) - truth;
        squares += e * e;
        assert!(e.abs() <= LARGEST, "{mv} mV at {t} s: {e:.2} %");
    }
    let rms = (squares / run.len() as f64).sqrt();
    assert!(rms <= 1.1, "rms {rms:.2} %");
}

//! Scenario runner: `bark-sim SCENARIO.toml [--seed N] [--duration-s S]
//! [--json FILE] [--csv FILE]`. Prints a short table; writes the JSON
//! summary and the CSV time series when asked.

use bark_sim::{Scenario, Sim};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str =
    "usage: bark-sim SCENARIO.toml [--seed N] [--duration-s S] [--json FILE] [--csv FILE]";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("bark-sim: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut path = None;
    let (mut seed, mut duration, mut json, mut csv) = (None, None, None, None);
    while let Some(a) = args.next() {
        let mut val = |name: &str| {
            args.next()
                .ok_or_else(|| format!("{name} needs a value\n{USAGE}"))
        };
        match a.as_str() {
            "--seed" => {
                seed = Some(
                    val("--seed")?
                        .parse::<u64>()
                        .map_err(|e| format!("--seed: {e}"))?,
                )
            }
            "--duration-s" => {
                duration = Some(
                    val("--duration-s")?
                        .parse::<f64>()
                        .map_err(|e| format!("--duration-s: {e}"))?,
                )
            }
            "--json" => json = Some(PathBuf::from(val("--json")?)),
            "--csv" => csv = Some(PathBuf::from(val("--csv")?)),
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            _ if path.is_none() && !a.starts_with('-') => path = Some(PathBuf::from(a)),
            _ => return Err(format!("unexpected argument {a:?}\n{USAGE}")),
        }
    }
    let path = path.ok_or_else(|| USAGE.to_string())?;
    let mut sc = Scenario::load(&path)?;
    if let Some(s) = seed {
        sc.seed = s;
    }
    if let Some(d) = duration {
        sc.duration_s = d;
    }
    if csv.is_some() && sc.metrics.cadence_s.is_none() {
        sc.metrics.cadence_s = Some(10.0);
    }
    let t0 = std::time::Instant::now();
    let mut sim = Sim::new(&sc)?;
    let built = t0.elapsed();
    sim.run();
    let ran = t0.elapsed() - built;
    let summary = sim.summary();
    print!("{}", summary.table());
    println!(
        "wall time: build {:.2} s, run {:.2} s",
        built.as_secs_f64(),
        ran.as_secs_f64()
    );
    if let Some(p) = json {
        std::fs::write(&p, summary.json()).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    if let Some(p) = csv {
        std::fs::write(&p, sim.metrics.csv()).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(())
}

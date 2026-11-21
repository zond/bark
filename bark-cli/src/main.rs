//! `bark`: a phone for a bark node on the command line (`README.md`).

use std::process::ExitCode;
use std::time::Duration;

use bark_cli::bluez::Bluez;
use bark_cli::link::{Link, Result};
use bark_cli::phone::{self, Select};
use bark_cli::show;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "bark",
    version,
    about = "A phone for a bark node, over this computer's Bluetooth"
)]
struct Args {
    /// Log every step on D-Bus, and BlueZ's answers, to stderr.
    #[arg(short, long, global = true)]
    verbose: bool,
    /// The Bluetooth adapter, such as hci0; else the first.
    #[arg(long, global = true)]
    adapter: Option<String>,
    /// Seconds to listen for nodes.
    #[arg(long, global = true, default_value_t = phone::SCAN_TIME.as_secs_f64())]
    scan_time: f64,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// The nodes in reach: ID, RSSI, public or private, address.
    Scan,
    /// What the node is: its ID, link versions, firmware image and room
    /// for more phones.
    Node {
        /// The node's ID, or the last digits of it.
        node: Select,
    },
    /// How the node stands now.
    Status {
        /// The node's ID, or the last digits of it.
        node: Select,
    },
}

fn run(args: Args) -> Result<Vec<String>> {
    let time = Duration::from_secs_f64(args.scan_time.max(0.0));
    let mut radio = Bluez::new(args.adapter.as_deref(), args.verbose)?;
    match args.command {
        Command::Scan => Ok(phone::scan(&mut radio, time)?
            .iter()
            .map(show::found)
            .collect()),
        Command::Node { node } => {
            let found = phone::find(&mut radio, &node, time)?;
            let mut link = phone::guest(&mut radio, &found)?;
            let read = phone::node(&mut link);
            let _ = link.disconnect();
            Ok(show::node(&read?))
        }
        Command::Status { node } => {
            let found = phone::find(&mut radio, &node, time)?;
            let mut link = phone::guest(&mut radio, &found)?;
            let read = phone::status(&mut link);
            let _ = link.disconnect();
            Ok(show::status(&read?))
        }
    }
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(lines) => {
            for l in lines {
                println!("{l}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("bark: {e}");
            ExitCode::FAILURE
        }
    }
}

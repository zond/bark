//! `bark`: a phone for a bark node on the command line (`README.md`).

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use bark_cli::bluez::Bluez;
use bark_cli::link::{Error, Link, Result, Transport};
use bark_cli::messages;
use bark_cli::phone::{self, Change, Select};
use bark_cli::show;
use bark_cli::state::{self, State};
use bark_link::phones::Parts;
use bark_link::Clock;
use bark_proto::Position;
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "bark",
    version,
    about = "A phone for a bark node, over this computer's Bluetooth"
)]
struct Args {
    /// Log every step on D-Bus and BlueZ's answers to stderr; twice, every
    /// device heard too.
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,
    /// The Bluetooth adapter, such as hci0; else the first.
    #[arg(long, global = true)]
    adapter: Option<String>,
    /// Seconds to listen for nodes at most.
    #[arg(long, global = true, default_value_t = phone::SCAN_TIME.as_secs_f64())]
    scan_time: f64,
    /// The directory of the tool's own state: the nodes it is paired with.
    #[arg(long, global = true)]
    dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, ValueEnum)]
enum OnOff {
    On,
    Off,
}

#[derive(Clone, Copy, ValueEnum)]
enum Part {
    Pairing,
    Home,
    Both,
}

#[derive(Subcommand)]
enum MapPointDo {
    /// Set the node's position to a map point.
    Set {
        /// Degrees north.
        #[arg(allow_negative_numbers = true)]
        lat: f64,
        /// Degrees east.
        #[arg(allow_negative_numbers = true)]
        lon: f64,
    },
    /// Clear the map point.
    Clear,
}

#[derive(Subcommand)]
enum Command {
    /// The nodes in reach: ID, RSSI, public or private, paired, address.
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
    /// Pair with the node, by the code its screen shows.
    Pair {
        node: Select,
        /// The six-digit code; else it is asked for when the node shows it.
        #[arg(long)]
        code: Option<u32>,
    },
    /// Have the node forget this computer, and forget it here.
    Unpair { node: Select },
    /// The node's neighbour table.
    Neighbours { node: Select },
    /// Ask a neighbour of the node for its status.
    Ask {
        node: Select,
        /// The neighbour's ID, or the last digits of it.
        neighbour: Select,
    },
    /// The node's time, after giving it this computer's.
    Clock { node: Select },
    /// Set or clear the node's map point.
    MapPoint {
        node: Select,
        #[command(subcommand)]
        what: MapPointDo,
    },
    /// Whether the node serves phones not paired with it; on or off sets it.
    Public { node: Select, set: Option<OnOff> },
    /// The phones paired with the node, and whose home it is.
    Phones { node: Select },
    /// Remove a record's pairing, its home or both, with the code the
    /// node's screen then shows.
    Remove {
        node: Select,
        /// The record's number, as `phones` gives it.
        number: u16,
        what: Part,
    },
    /// Make a record's person the node's owner, with the code the node's
    /// screen then shows.
    Owner {
        node: Select,
        /// The record's number, as `phones` gives it.
        number: u16,
    },
    /// This person: identity, name, home, and the card others add.
    Me {
        /// Sets the name the card shows, 32 bytes at most.
        #[arg(long)]
        name: Option<String>,
    },
    /// Add a contact from their card, or take their newer card.
    Contact {
        #[command(subcommand)]
        what: ContactDo,
    },
    /// The contacts.
    Contacts,
    /// Send a text to a contact through the node.
    Send {
        node: Select,
        /// The contact's name, or the start of their identity.
        contact: String,
        text: String,
    },
    /// Read the messages waiting at the node, and confirm them.
    Inbox { node: Select },
    /// Make the node your home, whose mailbox keeps your mail, or no
    /// longer.
    Home { node: Select, set: OnOff },
    /// Update the paired node's firmware from an image file, such as a
    /// release's bark-fw-<version>.img, and reboot it into the image.
    Update { node: Select, image: PathBuf },
}

#[derive(Subcommand)]
enum ContactDo {
    /// Add the person whose card this is: its BARK: text.
    Add {
        card: String,
        /// What to call them; else their card's name.
        #[arg(long)]
        name: Option<String>,
    },
}

/// This computer's time and UTC offset, for the node's clock
/// (spec "Settings"). The offset is the system's, as `date` gives it:
/// the standard library knows no time zone, and reading the zone files
/// and `TZ` here would take a dependency or a parser of our own.
fn now() -> Clock {
    let utc_s = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as u32);
    let offset_min = std::process::Command::new("date")
        .arg("+%z")
        .output()
        .ok()
        .and_then(|o| {
            let z = String::from_utf8(o.stdout).ok()?;
            let z = z.trim();
            let sign = if z.starts_with('-') { -1 } else { 1 };
            let h: i16 = z.get(1..3)?.parse().ok()?;
            let m: i16 = z.get(3..5)?.parse().ok()?;
            Some(sign * (h * 60 + m))
        })
        .unwrap_or(0);
    Clock { utc_s, offset_min }
}

fn run(args: Args) -> Result<Vec<String>> {
    let time = Duration::from_secs_f64(args.scan_time.max(0.0));
    let dir = args
        .dir
        .or_else(state::default_dir)
        .ok_or_else(|| Error::Other("no state directory: give --dir".into()))?;
    let mut state = State::load(&dir)?;
    // Opened only for a command that needs it: the person's own need none.
    let radio = || Bluez::new(args.adapter.as_deref(), args.verbose);
    let state = &mut state;
    Ok(match args.command {
        Command::Me { name } => messages::me(state, name)?,
        Command::Contact {
            what: ContactDo::Add { card, name },
        } => messages::add_contact(state, &card, name)?,
        Command::Contacts => messages::contacts(state),
        Command::Scan => phone::scan(&mut radio()?, time)?
            .iter()
            .map(show::found)
            .collect(),
        Command::Node { node } => {
            let (n, l) = phone::connect(&mut radio()?, state, &node, time, now())?;
            let _ = l.disconnect();
            show::node(&n)
        }
        Command::Status { node } => {
            let (_, mut l) = phone::connect(&mut radio()?, state, &node, time, now())?;
            let s = phone::status(&mut l);
            let _ = l.disconnect();
            show::status(&s?)
        }
        Command::Send {
            node,
            contact,
            text,
        } => messages::send(&mut radio()?, state, &node, &contact, &text, time, now())?,
        Command::Inbox { node } => messages::inbox(&mut radio()?, state, &node, time, now())?,
        Command::Update { node, image } => {
            let file = std::fs::read(&image)
                .map_err(|e| Error::Other(format!("reading {}: {e}", image.display())))?;
            let image = bark_cli::update::Image::parse(&file)?;
            bark_cli::update::update(&mut radio()?, state, &node, &image, now(), &mut |l| {
                println!("{l}")
            })?;
            Vec::new()
        }
        Command::Home { node, set } => {
            messages::home(&mut radio()?, state, &node, matches!(set, OnOff::On), now())?
        }
        Command::Pair { node, code } => {
            let n = phone::pair(&mut radio()?, state, &node, time, code, now())?;
            let mut lines = vec![format!("paired {}", bark_cli::link::id_text(n.id))];
            lines.extend(show::node(&n));
            lines
        }
        Command::Unpair { node } => {
            let id = phone::unpair(&mut radio()?, state, &node, now())?;
            vec![format!("unpaired {}", bark_cli::link::id_text(id))]
        }
        Command::Neighbours { node } => paired(&mut radio()?, state, &node, |l| {
            let (h, list) = phone::neighbours(l)?;
            Ok(show::neighbours(&h, &list))
        })?,
        Command::Ask { node, neighbour } => paired(&mut radio()?, state, &node, |l| {
            let id = phone::neighbour(l, &neighbour)?;
            Ok(show::answer(&phone::ask(l, id)?))
        })?,
        Command::Clock { node } => paired(&mut radio()?, state, &node, |l| {
            Ok(show::node_time(&phone::clock(l)?))
        })?,
        Command::MapPoint { node, what } => {
            let at = match what {
                MapPointDo::Set { lat, lon } => Some(Position::from_degrees(lat, lon)),
                MapPointDo::Clear => None,
            };
            paired(&mut radio()?, state, &node, |l| {
                Ok(show::status(&phone::map_point(l, at)?))
            })?
        }
        Command::Public { node, set } => {
            let set = set.map(|s| matches!(s, OnOff::On));
            paired(&mut radio()?, state, &node, |l| {
                let public = phone::public(l, set)?;
                Ok(vec![format!(
                    "public {}",
                    if public { "yes" } else { "no" }
                )])
            })?
        }
        Command::Phones { node } => paired(&mut radio()?, state, &node, |l| {
            Ok(show::phones(&phone::phones(l)?))
        })?,
        Command::Remove { node, number, what } => {
            let parts = Parts {
                bond: matches!(what, Part::Pairing | Part::Both),
                home: matches!(what, Part::Home | Part::Both),
            };
            paired(&mut radio()?, state, &node, |l| {
                phone::change(l, number, Change::Remove(parts), typed_code)?;
                Ok(show::phones(&phone::phones(l)?))
            })?
        }
        Command::Owner { node, number } => paired(&mut radio()?, state, &node, |l| {
            phone::change(l, number, Change::Owner, typed_code)?;
            Ok(show::phones(&phone::phones(l)?))
        })?,
    })
}

/// A paired phone's command `f`, on its link to `node`.
fn paired<T: Transport>(
    radio: &mut T,
    state: &mut State,
    node: &Select,
    f: impl FnOnce(&mut T::Link) -> Result<Vec<String>>,
) -> Result<Vec<String>> {
    let (_, mut l) = phone::paired_link(radio, state, node, now())?;
    let lines = f(&mut l);
    let _ = l.disconnect();
    lines
}

/// The code the person reads on the node's screen, typed here.
fn typed_code() -> Result<u32> {
    eprint!("The code on the node's screen: ");
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| Error::Other(format!("reading the code: {e}")))?;
    line.trim()
        .parse()
        .ok()
        .filter(|&c| c < bark_link::phones::CODES)
        .ok_or_else(|| Error::Other("a code is six digits".into()))
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

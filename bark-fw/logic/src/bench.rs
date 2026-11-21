//! The bench build's commands on the serial port (`bark-fw/README.md`,
//! "Bench build"), parsed, and the reset cause its status gives.

use core::fmt;

use bark_app_layer::Address;
use bark_proto::radio::Channel;
use bark_proto::wire::MAX_PAYLOAD;
use bark_proto::{Config, NodeId, Position};

/// The longest carrier: twice what the move rule can take to leave a
/// busy channel, its dwell and its longest wait (spec "Channel moves"), so
/// that a jammed relay has time to move and no longer.
pub const CARRIER_MAX_S: u32 = (2
    * (Config::DEFAULT.move_dwell.as_micros() + Config::DEFAULT.move_wait_max.as_micros())
    / 1_000_000) as u32;

/// The longest command: a payload's, a whole payload in hex behind the
/// name and an address, and the spaces between. The app's first message
/// to a person carries its sender's card, over half a payload.
pub const LINE: usize = "payload".len() + 2 * Address::LEN + 2 * MAX_PAYLOAD + 2;

/// A text's longest: its words a whole payload behind the name, an ID, a
/// latitude and a longitude at their longest, and the spaces between.
const TEXT_LINE: usize =
    "text".len() + 2 * NodeId::LEN + "-90.000000".len() + "-180.000000".len() + 4 + MAX_PAYLOAD;
const _: () = assert!(TEXT_LINE <= LINE);

/// What power the node is told it is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Power {
    Usb,
    Battery,
    /// What USB detection says.
    Real,
}

impl Power {
    /// External power present, or `None` for the real reading.
    pub fn told(self) -> Option<bool> {
        match self {
            Power::Usb => Some(true),
            Power::Battery => Some(false),
            Power::Real => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command<'a> {
    /// Send `payload` to `to` at `at`, as a phone's message.
    Send {
        to: NodeId,
        at: Position,
        payload: Payload<'a>,
    },
    Power(Power),
    /// The transmit power, dBm conducted, before the radio's limits.
    TxPower(i8),
    /// The least cold oscillator delay, µs, before the probed range.
    Tcxo(u32),
    /// Transmit on `channel` for `seconds`.
    Carrier {
        channel: Channel,
        seconds: u32,
    },
    /// Flip the role, as a long press of the button does.
    Flip,
    Reboot,
    Status,
    /// Drop what the node keeps of images other than its own, and reboot.
    Forget,
}

/// A message's payload as it was typed, checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Payload<'a> {
    /// The words' UTF-8 bytes.
    Words(&'a str),
    /// Bytes, two hex digits each.
    Hex(&'a str),
}

impl Payload<'_> {
    /// The bytes into `out`; how many, none for hex that is not.
    pub fn write(&self, out: &mut [u8; MAX_PAYLOAD]) -> usize {
        match *self {
            Payload::Words(w) => {
                out[..w.len()].copy_from_slice(w.as_bytes());
                w.len()
            }
            Payload::Hex(h) => {
                let n = h.len() / 2;
                hex(h, &mut out[..n]).map_or(0, |()| n)
            }
        }
    }
}

/// The commands, for a line that is none of them.
pub const HELP: &str = "commands: text <id> <lat> <lon> <words> | payload <address> <hex> | \
power <usb|battery|real> | txpower <dbm> | tcxo <us> | carrier <k> <seconds> | flip | reboot | status | forget";

pub fn parse(line: &str) -> Result<Command<'_>, &'static str> {
    let line = line.trim();
    let (name, rest) = line.split_once(' ').unwrap_or((line, ""));
    let mut args = rest.split_whitespace();
    let command = match name {
        "text" => return text(rest),
        "payload" => return payload(rest),
        "power" => Command::Power(match args.next() {
            Some("usb") => Power::Usb,
            Some("battery") => Power::Battery,
            Some("real") => Power::Real,
            _ => return Err("power: usb, battery or real"),
        }),
        "txpower" => Command::TxPower(
            args.next()
                .and_then(|w| w.parse().ok())
                .ok_or("txpower: a whole number of dBm")?,
        ),
        "tcxo" => Command::Tcxo(
            args.next()
                .and_then(|w| w.parse().ok())
                .ok_or("tcxo: a whole number of microseconds")?,
        ),
        "carrier" => {
            let channel = args
                .next()
                .and_then(|w| w.parse().ok())
                .and_then(Channel::new)
                .ok_or("carrier: a channel, 0 to 12")?;
            let seconds = args
                .next()
                .and_then(|w| w.parse().ok())
                .filter(|s| (1..=CARRIER_MAX_S).contains(s))
                .ok_or("carrier: seconds, 1 to the longest")?;
            Command::Carrier { channel, seconds }
        }
        "flip" => Command::Flip,
        "reboot" => Command::Reboot,
        "status" => Command::Status,
        "forget" => Command::Forget,
        _ => return Err(HELP),
    };
    match args.next() {
        None => Ok(command),
        Some(_) => Err("too many words"),
    }
}

/// `<id> <lat> <lon> <words>`: the words are the rest of the line as it
/// was typed.
fn text(rest: &str) -> Result<Command<'_>, &'static str> {
    const FORM: &str = "text <id, 12 hex digits> <lat> <lon> <words>";
    let mut rest = rest.trim_start();
    let mut word = || {
        let (w, r) = rest.split_once(' ').ok_or(FORM)?;
        rest = r.trim_start();
        Ok::<_, &'static str>(w)
    };
    let to = node_id(word()?).ok_or(FORM)?;
    let lat = micro_degrees(word()?, 90).ok_or("text: latitude in degrees, -90 to 90")?;
    let lon = micro_degrees(word()?, 180).ok_or("text: longitude in degrees, -180 to 180")?;
    let words = rest.trim_end();
    if words.is_empty() {
        return Err(FORM);
    }
    if words.len() > MAX_PAYLOAD {
        return Err("text: longer than a message");
    }
    let at = Position::from_degrees(lat as f64 / 1e6, lon as f64 / 1e6);
    Ok(Command::Send {
        to,
        at,
        payload: Payload::Words(words),
    })
}

/// `<address> <bytes>`: the address in the app's form, and the payload in
/// hex, as the phone test asks a board to send them.
fn payload(rest: &str) -> Result<Command<'_>, &'static str> {
    const FORM: &str = "payload <address, 26 hex digits> <bytes in hex>";
    let mut words = rest.split_whitespace();
    let (Some(address), Some(bytes), None) = (words.next(), words.next(), words.next()) else {
        return Err(FORM);
    };
    let mut a = [0u8; Address::LEN];
    hex(address, &mut a).ok_or(FORM)?;
    let to = Address::decode(&a).map_err(|_| FORM)?;
    if bytes.len() > 2 * MAX_PAYLOAD {
        return Err("payload: longer than a message");
    }
    // Checked here, decoded once, into the message (`Payload::write`).
    if !bytes.len().is_multiple_of(2) || !bytes.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("payload: bytes in hex, two digits each");
    }
    Ok(Command::Send {
        to: to.node,
        at: to.position,
        payload: Payload::Hex(bytes),
    })
}

/// Twelve hex digits, the form the log gives a node ID in.
fn node_id(digits: &str) -> Option<NodeId> {
    let mut id = [0u8; NodeId::LEN];
    hex(digits, &mut id)?;
    Some(NodeId(id))
}

/// `digits` into `out`, two hex digits a byte, exactly as many as fill it.
fn hex(digits: &str, out: &mut [u8]) -> Option<()> {
    let b = digits.as_bytes();
    if b.len() != 2 * out.len() {
        return None;
    }
    out.fill(0);
    for (i, &c) in b.iter().enumerate() {
        let digit = char::from(c).to_digit(16)? as u8;
        out[i / 2] = out[i / 2] << 4 | digit;
    }
    Some(())
}

/// Degrees with at most six decimals, as the log gives a position, in
/// millionths, within `±limit` degrees; read as integers, so no rounding
/// of a fraction comes between what was typed and the position.
fn micro_degrees(word: &str, limit: i64) -> Option<i64> {
    let (negative, digits) = match word.strip_prefix('-') {
        Some(d) => (true, d),
        None => (false, word),
    };
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    let all_digits = |s: &str| s.bytes().all(|c| c.is_ascii_digit());
    if whole.is_empty() || whole.len() > 3 || fraction.len() > 6 {
        return None;
    }
    if !all_digits(whole) || !all_digits(fraction) {
        return None;
    }
    let mut micro: i64 = whole.parse().ok()?;
    let mut scale = 1_000_000;
    for c in fraction.bytes() {
        scale /= 10;
        micro = micro * 10 + i64::from(c - b'0');
    }
    micro *= scale;
    if micro > limit * 1_000_000 {
        return None;
    }
    Some(if negative { -micro } else { micro })
}

/// The causes the nRF52840 records for the resets since the register was
/// last cleared, a set of bits (Product Specification, POWER, RESETREAS);
/// none is a power-on or brown-out reset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResetCause(pub u32);

const RESET_CAUSES: [(u32, &str); 9] = [
    (1 << 0, "pin"),
    (1 << 1, "watchdog"),
    (1 << 2, "software"),
    (1 << 3, "lockup"),
    (1 << 16, "off-gpio"),
    (1 << 17, "off-lpcomp"),
    (1 << 18, "debug"),
    (1 << 19, "off-nfc"),
    (1 << 20, "off-vbus"),
];

impl fmt::Display for ResetCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut named = RESET_CAUSES.iter().filter(|(bit, _)| self.0 & bit != 0);
        match named.next() {
            None => f.write_str("power-on")?,
            Some((_, name)) => f.write_str(name)?,
        }
        for (_, name) in named {
            write!(f, "+{name}")?;
        }
        let known = RESET_CAUSES.iter().fold(0, |m, (bit, _)| m | bit);
        match self.0 & !known {
            0 => Ok(()),
            other => write!(f, "+0x{other:x}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_longest_carrier_is_half_an_hour() {
        assert_eq!(CARRIER_MAX_S, 1800);
    }

    #[test]
    fn a_text_keeps_its_words_as_typed() {
        let c = parse("text 0a1B2c3d4e5f 59.329300 -18.0686 hello  there ").unwrap();
        assert_eq!(
            c,
            Command::Send {
                to: NodeId([0x0a, 0x1b, 0x2c, 0x3d, 0x4e, 0x5f]),
                at: Position::from_degrees(59.3293, -18.0686),
                payload: Payload::Words("hello  there"),
            }
        );
        let Command::Send { payload, .. } = c else {
            unreachable!()
        };
        let mut out = [0; MAX_PAYLOAD];
        let n = payload.write(&mut out);
        assert_eq!(&out[..n], b"hello  there");
    }

    #[test]
    fn a_text_with_a_part_missing_or_wrong_is_refused() {
        for line in [
            "text",
            "text 0a1b2c3d4e5f 59.3 18.0",
            "text 0a1b2c3d4e5 59.3 18.0 hi",
            "text 0a1b2c3d4e5g 59.3 18.0 hi",
            "text 0a1b2c3d4e5f 90.000001 18.0 hi",
            "text 0a1b2c3d4e5f 59.3 180.5 hi",
            "text 0a1b2c3d4e5f 59.3333333 18.0 hi",
            "text 0a1b2c3d4e5f 59,3 18.0 hi",
            "text 0a1b2c3d4e5f -.5 18.0 hi",
        ] {
            assert!(parse(line).is_err(), "{line}");
        }
        let long = format!("text 0a1b2c3d4e5f 1 2 {}", "x".repeat(MAX_PAYLOAD + 1));
        assert!(parse(&long).is_err());
        let full = format!(
            "text 0a1b2c3d4e5f -90.000000 -180.000000 {}",
            "x".repeat(MAX_PAYLOAD)
        );
        assert_eq!(full.len(), TEXT_LINE);
        assert!(parse(&full).is_ok());
    }

    /// An address as the app writes it, `Address.toString()`.
    fn app_address(a: Address) -> String {
        a.encode().iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn a_payload_goes_to_the_address_the_app_wrote() {
        let to = Address {
            position: Position::from_degrees(59.3293, -18.0686),
            node: NodeId([0x25, 0xf1, 0x61, 0xd7, 0x31, 0x12]),
        };
        let line = format!("payload {} 02E2ff00", app_address(to));
        let c = parse(&line).unwrap();
        assert_eq!(
            c,
            Command::Send {
                to: to.node,
                at: to.position,
                payload: Payload::Hex("02E2ff00"),
            }
        );
        let Command::Send {
            to: id,
            at,
            payload,
        } = c
        else {
            unreachable!()
        };
        let mut out = [0; MAX_PAYLOAD];
        let n = payload.write(&mut out);
        assert_eq!(out[..n], [0x02, 0xe2, 0xff, 0x00]);
        // The ID the board logs is the one the app gave: its FNV-1a of
        // the address's bytes and the payload's.
        let app_id = bark_proto::ids::fnv1a(&[&to.encode(), &out[..n]]);
        assert_eq!(bark_proto::MsgId::of(at, id, &out[..n]).0, app_id);
    }

    #[test]
    fn a_payload_with_a_part_missing_or_wrong_is_refused() {
        let a = "47c0490ad9ff80fa4e00000001";
        for line in [
            "payload".to_string(),
            format!("payload {a}"),
            format!("payload {a} 02e2 00"),
            format!("payload {} 02e2", &a[1..]),
            format!("payload {a}0 02e2"),
            format!("payload {}g 02e2", &a[1..]),
            format!("payload {a} 02e"),
            format!("payload {a} 02g2"),
            format!("payload {a} +2e2"),
        ] {
            assert!(parse(&line).is_err(), "{line}");
        }
        let long = format!("payload {a} {}", "ab".repeat(MAX_PAYLOAD + 1));
        assert_eq!(parse(&long), Err("payload: longer than a message"));
        let full = format!("payload {a} {}", "ab".repeat(MAX_PAYLOAD));
        assert_eq!(full.len(), LINE);
        let Ok(Command::Send { payload, .. }) = parse(&full) else {
            panic!("{full}")
        };
        let mut out = [0; MAX_PAYLOAD];
        assert_eq!(payload.write(&mut out), MAX_PAYLOAD);
        assert!(out.iter().all(|&b| b == 0xab));
    }

    #[test]
    fn degrees_are_read_exactly() {
        assert_eq!(micro_degrees("59.3293", 90), Some(59_329_300));
        assert_eq!(micro_degrees("-0.000001", 90), Some(-1));
        assert_eq!(micro_degrees("180", 180), Some(180_000_000));
        assert_eq!(micro_degrees("7.", 90), Some(7_000_000));
    }

    #[test]
    fn the_other_commands() {
        assert_eq!(parse("power battery"), Ok(Command::Power(Power::Battery)));
        assert_eq!(parse("power real"), Ok(Command::Power(Power::Real)));
        assert_eq!(Power::Real.told(), None);
        assert_eq!(parse("txpower -9"), Ok(Command::TxPower(-9)));
        assert_eq!(parse("tcxo 750"), Ok(Command::Tcxo(750)));
        assert_eq!(
            parse("carrier 12 900"),
            Ok(Command::Carrier {
                channel: Channel::new(12).unwrap(),
                seconds: 900
            })
        );
        assert_eq!(parse(" reboot "), Ok(Command::Reboot));
        assert_eq!(parse("status"), Ok(Command::Status));
        assert_eq!(parse("forget"), Ok(Command::Forget));
        assert_eq!(parse("flip"), Ok(Command::Flip));
        for line in [
            "power",
            "power mains",
            "txpower",
            "txpower 3.5",
            "tcxo",
            "tcxo -250",
            "tcxo 0.75",
            "tcxo 750 us",
            "carrier 13 10",
            "carrier 3 0",
            "carrier 3 1801",
            "reboot now",
            "forget all",
            "home 3",
            "",
        ] {
            assert!(parse(line).is_err(), "{line}");
        }
    }

    #[test]
    fn reset_causes_are_named() {
        let s = |bits| ResetCause(bits).to_string();
        assert_eq!(s(0), "power-on");
        assert_eq!(s(1 << 1), "watchdog");
        assert_eq!(s(1 << 2 | 1 << 3), "software+lockup");
        assert_eq!(s(1 << 0 | 1 << 8), "pin+0x100");
    }
}

//! The tool's output: one fact a line, a name and its value, so that a
//! script greps for the name.

use bark_link::neighbours::{self, Header, Neighbour, Quality};
use bark_link::{Answer, Node, NodeTime, Status, TimeSource};
use bark_proto::status::{Battery, ReadingAge};
use bark_proto::Position;

use crate::link::{id_text, Found};
use crate::phone::Record;

fn yes(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

pub fn found(f: &Found) -> String {
    let rssi = f.rssi.map_or("none".into(), |r| r.to_string());
    let reach = f.public.word();
    format!(
        "{} rssi {rssi} {reach} paired {} address {}",
        id_text(f.id),
        yes(f.bonded),
        f.address
    )
}

/// A position, or `none` for the zero that stands for none.
fn place(p: Position) -> String {
    match p == Position::default() {
        true => "none".into(),
        false => p.to_string(),
    }
}

/// The names of the flags set, or `none`.
fn named(flags: &[(bool, &str)]) -> String {
    let set: Vec<&str> = flags.iter().filter(|f| f.0).map(|f| f.1).collect();
    match set.is_empty() {
        true => "none".into(),
        false => set.join(" "),
    }
}

/// The table's header, then a line for each neighbour: its ID, position,
/// averaged RSSI in dBm, the link's quality (spec "The neighbour map"),
/// seconds since heard, and its flags by name.
pub fn neighbours(h: &Header, list: &[Neighbour]) -> Vec<String> {
    let mut lines = vec![
        format!("floor_dbm {}", h.floor_dbm),
        format!("own {}", place(h.own)),
        format!("neighbours {}", list.len()),
    ];
    for n in list {
        let quality = match neighbours::quality(n.rssi_ddbm, h.floor_dbm) {
            Quality::Good => "good",
            Quality::Fair => "fair",
            Quality::Fragile => "fragile",
        };
        let flags = named(&[
            (n.sleepy, "sleepy"),
            (n.map_point, "map_point"),
            (n.demoted, "demoted"),
            (n.banned, "banned"),
            (n.anchor, "anchor"),
            (n.carried, "carried"),
            (n.anchorless, "anchorless"),
            (n.proven, "proven"),
        ]);
        lines.push(format!(
            "{} position {} rssi {:.1} quality {quality} age_s {} flags {flags}",
            id_text(n.id),
            place(n.position),
            f32::from(n.rssi_ddbm) / 10.0,
            n.age_s,
        ));
    }
    lines
}

/// A neighbour's answer to an ask: what its screen shows
/// (spec "Neighbour status").
pub fn answer(a: &Answer) -> Vec<String> {
    let s = &a.status;
    let battery = match s.battery {
        Battery::Usb => "usb".into(),
        Battery::NotRead => "not_read".into(),
        Battery::Low => "low".into(),
        Battery::Percent(p) => format!("{p}%"),
    };
    let mut lines = vec![
        format!("id {}", id_text(a.from)),
        format!(
            "flags {}",
            named(&[
                (s.placed, "placed"),
                (s.sleepy, "sleepy"),
                (s.anchored, "anchored"),
                (s.carried, "carried"),
            ])
        ),
        format!("battery {battery}"),
        format!("heard {}", s.neighbours.heard),
        format!("strong {}", s.neighbours.strong),
        format!("both_ways {}", s.neighbours.both_ways),
        format!("gps_cn0 {}", s.gps_cn0),
        gps_age(s.gps_age),
        format!("image {}", s.version),
    ];
    let g = &s.gathering;
    if g.version != 0 {
        lines.push(format!(
            "gathering {} blocks {} complete {} have {}",
            g.version, g.blocks, g.complete, g.have
        ));
    }
    lines
}

/// The civil date of `days` since 1970-01-01 (the proleptic Gregorian
/// calendar, counted in eras of 400 years, 146 097 days).
fn date(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// UTC seconds since 1970 as ISO 8601.
pub fn utc(s: u32) -> String {
    let s = i64::from(s);
    let (y, m, d) = date(s.div_euclid(86_400));
    let t = s.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        t / 3600,
        t / 60 % 60,
        t % 60
    )
}

pub fn node_time(t: &NodeTime) -> Vec<String> {
    let source = match t.source {
        TimeSource::None => "none",
        TimeSource::Gps => "gps",
        TimeSource::Phone => "phone",
    };
    let time = match t.source {
        TimeSource::None => "none".into(),
        _ => utc(t.utc_s),
    };
    vec![
        format!("utc {time}"),
        format!("offset_min {}", t.offset_min),
        format!("source {source}"),
    ]
}

/// Days since 1970 of 2026-01-01, the first day a record's day counts,
/// as day 1 (spec "Phones").
const DAY_ONE: i64 = 20_454;

/// A line for each of the node's records.
pub fn phones(list: &[Record]) -> Vec<String> {
    list.iter()
        .map(|r| {
            let day = match r.day {
                0 => "none".into(),
                d => {
                    let (y, m, d) = date(DAY_ONE + i64::from(d) - 1);
                    format!("{y:04}-{m:02}-{d:02}")
                }
            };
            let tag = r.tag.map_or("none".into(), |t| format!("{t:02x}"));
            format!(
                "number {} flags {} tag {tag} day {day} name {}",
                r.number,
                named(&[
                    (r.paired, "paired"),
                    (r.home, "home"),
                    (r.linked, "linked"),
                    (r.mine, "mine"),
                    (r.owner, "owner"),
                ]),
                r.name
            )
        })
        .collect()
}

pub fn node(n: &Node) -> Vec<String> {
    vec![
        format!("id {}", id_text(n.id)),
        format!("link_version {}", n.version),
        format!("oldest {}", n.oldest),
        format!("image {}", n.image),
        format!("room {}", n.room),
    ]
}

/// Status's flags by name, in the order of their bits (spec "Phone link",
/// The service).
fn flags(s: &Status) -> String {
    named(&[
        (s.ready, "ready"),
        (s.external_power, "usb"),
        (s.sleepy, "sleepy"),
        (s.carried, "carried"),
        (s.map_point, "map_point"),
        (s.placed, "placed"),
        (s.booting, "first_rotation"),
        (s.anchorless, "anchorless"),
    ])
}

pub fn status(s: &Status) -> Vec<String> {
    let address = match s.placed {
        true => s.address.to_string(),
        false => "none".into(),
    };
    vec![
        format!("flags {}", flags(s)),
        format!("address {address}"),
        format!("battery_mv {}", s.battery_mv),
        format!("relays {}", s.relays),
        format!("heard {}", s.heard),
        format!("boot_left_s {}", s.boot_left_s),
        format!("installing {}", s.installing),
        format!("inbox {}", s.inbox),
        format!("phones_mark {}", s.phones),
        format!("gps_cn0 {}", s.gps_cn0),
        gps_age(s.gps_age),
    ]
}

/// How long ago the GPS's reading was, as its code tells it: its lower
/// bound, in minutes (spec "Neighbour status").
fn gps_age(age: ReadingAge) -> String {
    match age.minutes() {
        Some(m) => format!("gps_age_min {m}"),
        None => "gps_age none".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bark_proto::NodeId;

    #[test]
    fn status_names_its_flags_and_a_place() {
        let s = Status {
            ready: true,
            placed: true,
            booting: true,
            address: Position::from_degrees(50.45, 30.52),
            battery_mv: 3900,
            gps_cn0: 27,
            gps_age: ReadingAge(37),
            ..Status::EMPTY
        };
        let lines = status(&s);
        assert_eq!(lines[0], "flags ready placed first_rotation");
        assert_eq!(lines[1], format!("address {}", s.address));
        assert_eq!(lines[2], "battery_mv 3900");
        assert_eq!(lines[lines.len() - 2..], ["gps_cn0 27", "gps_age_min 68"]);
        let lines = status(&Status::EMPTY);
        assert_eq!(lines[0], "flags none");
        assert_eq!(lines[1], "address none");
        assert_eq!(lines.last().unwrap(), "gps_age none");
    }

    #[test]
    fn a_found_node_is_one_line() {
        let f = Found {
            id: NodeId::from_u64(0xfa4e_0000_0001),
            address: "E2:49:1C:03:EF:88".into(),
            rssi: Some(-61),
            public: crate::link::Reach::Public,
            bonded: false,
        };
        assert_eq!(
            found(&f),
            "fa4e00000001 rssi -61 public paired no address E2:49:1C:03:EF:88"
        );
    }

    #[test]
    fn days_are_dates() {
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc(1_795_000_000), "2026-11-18T11:06:40Z");
        let r = Record {
            number: 3,
            paired: true,
            home: false,
            linked: true,
            mine: true,
            owner: true,
            tag: Some(0x5a),
            day: 1,
            name: "Olga".into(),
        };
        assert_eq!(
            phones(&[r])[0],
            "number 3 flags paired linked mine owner tag 5a day 2026-01-01 name Olga"
        );
    }
}

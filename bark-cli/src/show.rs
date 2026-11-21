//! The tool's output: one fact a line, a name and its value, so that a
//! script greps for the name.

use bark_link::{Node, Status};

use crate::link::{id_text, Found};

pub fn found(f: &Found) -> String {
    let rssi = f.rssi.map_or("none".into(), |r| r.to_string());
    let reach = if f.public { "public" } else { "private" };
    format!(
        "{} rssi {rssi} {reach} address {}",
        id_text(f.id),
        f.address
    )
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
    let named = [
        (s.ready, "ready"),
        (s.external_power, "usb"),
        (s.sleepy, "sleepy"),
        (s.carried, "carried"),
        (s.map_point, "map_point"),
        (s.placed, "placed"),
        (s.booting, "first_rotation"),
        (s.anchorless, "anchorless"),
    ];
    let set: Vec<&str> = named.iter().filter(|f| f.0).map(|f| f.1).collect();
    match set.is_empty() {
        true => "none".into(),
        false => set.join(" "),
    }
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
        format!("phones {}", s.phones),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use bark_proto::{NodeId, Position};

    #[test]
    fn status_names_its_flags_and_a_place() {
        let s = Status {
            ready: true,
            placed: true,
            booting: true,
            address: Position::from_degrees(50.45, 30.52),
            battery_mv: 3900,
            ..Status::EMPTY
        };
        let lines = status(&s);
        assert_eq!(lines[0], "flags ready placed first_rotation");
        assert_eq!(lines[1], format!("address {}", s.address));
        assert_eq!(lines[2], "battery_mv 3900");
        let lines = status(&Status::EMPTY);
        assert_eq!(lines[0], "flags none");
        assert_eq!(lines[1], "address none");
    }

    #[test]
    fn a_found_node_is_one_line() {
        let f = Found {
            id: NodeId::from_u64(0xfa4e_0000_0001),
            address: "E2:49:1C:03:EF:88".into(),
            rssi: Some(-61),
            public: true,
        };
        assert_eq!(
            found(&f),
            "fa4e00000001 rssi -61 public address E2:49:1C:03:EF:88"
        );
    }
}

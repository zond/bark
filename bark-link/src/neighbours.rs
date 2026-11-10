//! Neighbours: the characteristic by which the app reads the node's
//! neighbour table (spec "Neighbour table"), for a map of the node and the
//! links around it, `28bc000a-…` ([`crate::uuid::NEIGHBOURS`]).
//!
//! | | value | bytes |
//! |---|---|---|
//! | write | the slot to read from | 1 |
//! | read | [`Header`], then up to [`PER_PAGE`] [`Neighbour`]s | 10 + 18 n |
//!
//! The table holds up to 64 neighbours, more than one value holds, so the
//! app reads it in pages: it writes a slot, reads the live neighbours from
//! that slot on, as many as fit, and writes the slot the page names next,
//! until that is 0. A neighbour keeps its slot while it stays in the
//! table, so one that leaves between two reads moves no other, and the
//! pages skip none that stay.

use bark_proto::radio::{
    full_frame_rssi_above_noise_db, full_frame_rssi_through_db, FADE_DEPTH_DB,
};
use bark_proto::{NodeId, Position};

use crate::{node_id, position, u16_at, Malformed, ID, POS};

/// Neighbours, written: read from this slot on. Taken whole.
pub fn decode_from(b: &[u8]) -> Result<u8, Malformed> {
    match b {
        [slot] => Ok(*slot),
        _ => Err(Malformed::Length),
    }
}

/// The page's first bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Header {
    /// The slot to write for the next page; 0 when this page reached the
    /// table's end.
    pub next: u8,
    /// The node's noise floor, dBm (spec "Busy estimate"), against which
    /// a link's margin is read ([`quality`]).
    pub floor_dbm: i8,
    /// The node's own position (spec "Position"), which for a sleepy node
    /// is not its address; zero while it has no place.
    pub own: Position,
}

/// One neighbour, as the table holds it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Neighbour {
    /// Its ID.
    pub id: NodeId,
    /// Its position, from its heartbeat.
    pub position: Position,
    /// The averaged RSSI of its link, in tenths of a dBm
    /// (spec "Neighbour table"): finer than the radio's 0.5 dB step, so
    /// that the app reads the margin as the node does.
    pub rssi_ddbm: i16,
    /// Whole seconds since it was last heard, at most `u16::MAX`.
    pub age_s: u16,
    /// It is sleepy (spec "Nodes").
    pub sleepy: bool,
    /// Its position is a map point (spec "Position").
    pub map_point: bool,
    /// It is demoted (spec "Neighbour table").
    pub demoted: bool,
    /// It is banned (spec "Bans").
    pub banned: bool,
    /// It is this sleepy node's anchor, the relay it sends through and
    /// receives from (spec "Relay choice").
    pub anchor: bool,
    /// It is carried (spec "Nodes").
    pub carried: bool,
    /// Its last heartbeat had the anchorless flag (spec "Heartbeat").
    pub anchorless: bool,
    /// It has taken a message from this node, so it hears this node: a
    /// heartbeat shows only the way to it.
    pub proven: bool,
}

/// Bytes before the first neighbour: the next slot, the noise floor, the
/// node's position, and the bytes of each neighbour.
pub const HEADER: usize = 1 + 1 + POS + 1;
/// Where the header says the bytes of each neighbour.
const AT_ENTRY: usize = HEADER - 1;
/// Bytes of one neighbour in this version: ID, position, RSSI, age and
/// flags. A later version may append fields to each; the header says how
/// long each is.
pub const ENTRY: usize = ID + POS + 2 + 2 + 1;
/// Neighbours in one page: as many as fit one PDU at the node's MTU.
pub const PER_PAGE: usize = (crate::value_room(crate::ATT_MTU) - HEADER) / ENTRY;
/// Bytes in the longest value.
pub const MAX_LEN: usize = HEADER + PER_PAGE * ENTRY;

impl Neighbour {
    fn flags(&self) -> u8 {
        [
            self.sleepy,
            self.map_point,
            self.demoted,
            self.banned,
            self.anchor,
            self.carried,
            self.anchorless,
            self.proven,
        ]
        .iter()
        .enumerate()
        .fold(0, |f, (bit, &set)| f | (u8::from(set) << bit))
    }

    fn encode(&self, b: &mut [u8; ENTRY]) {
        b[..ID].copy_from_slice(&self.id.0);
        b[ID..ID + POS].copy_from_slice(&self.position.encode());
        b[ID + POS..ID + POS + 2].copy_from_slice(&self.rssi_ddbm.to_be_bytes());
        b[ID + POS + 2..ID + POS + 4].copy_from_slice(&self.age_s.to_be_bytes());
        b[ID + POS + 4] = self.flags();
    }

    fn decode(b: &[u8]) -> Neighbour {
        let flag = |bit: u8| b[ID + POS + 4] & (1 << bit) != 0;
        Neighbour {
            id: node_id(b),
            position: position(&b[ID..]),
            rssi_ddbm: u16_at(b, ID + POS) as i16,
            age_s: u16_at(b, ID + POS + 2),
            sleepy: flag(0),
            map_point: flag(1),
            demoted: flag(2),
            banned: flag(3),
            anchor: flag(4),
            carried: flag(5),
            anchorless: flag(6),
            proven: flag(7),
        }
    }
}

/// A page, into `b`; returns its length.
pub fn encode(header: &Header, page: &[Neighbour], b: &mut [u8; MAX_LEN]) -> usize {
    assert!(page.len() <= PER_PAGE, "page too long");
    b[0] = header.next;
    b[1] = header.floor_dbm as u8;
    b[2..AT_ENTRY].copy_from_slice(&header.own.encode());
    b[AT_ENTRY] = ENTRY as u8;
    for (n, chunk) in page.iter().zip(b[HEADER..].as_chunks_mut::<ENTRY>().0) {
        n.encode(chunk);
    }
    HEADER + page.len() * ENTRY
}

/// Read a page: its header and its neighbours, ignoring what a later
/// version appends to each.
pub fn decode(b: &[u8]) -> Result<(Header, impl Iterator<Item = Neighbour> + '_), Malformed> {
    if b.len() < HEADER {
        return Err(Malformed::Length);
    }
    let entry = usize::from(b[AT_ENTRY]);
    if entry < ENTRY || !(b.len() - HEADER).is_multiple_of(entry) {
        return Err(Malformed::Length);
    }
    let header = Header {
        next: b[0],
        floor_dbm: b[1] as i8,
        own: position(&b[2..]),
    };
    Ok((
        header,
        b[HEADER..].chunks_exact(entry).map(Neighbour::decode),
    ))
}

/// How well a link carries a full-length frame, by the margin of its
/// averaged RSSI over the node's noise floor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Quality {
    /// Below the margin the node asks of a next hop
    /// (spec "Neighbour table"): a frame is lost more than one time in a
    /// hundred in a fade of one standard deviation, and the node sends
    /// through it only when no other is left.
    Fragile,
    /// A frame gets through a fade of one standard deviation, not of two.
    Fair,
    /// A frame gets through a fade of two standard deviations, which comes
    /// one time in 44, the convention of the switching margin
    /// (spec "Relay choice").
    Good,
}

/// The margin, dB above the noise floor, from which a link is fair: the
/// node's own margin, about 4.1 dB.
pub fn fair_above_floor_db() -> f32 {
    full_frame_rssi_above_noise_db()
}

/// The margin, dB above the noise floor, from which a link is good: a
/// full frame through a fade of two standard deviations, about 6.9 dB.
pub fn good_above_floor_db() -> f32 {
    full_frame_rssi_through_db(2.0 * FADE_DEPTH_DB)
}

/// The quality of a link of averaged RSSI `rssi_ddbm`, tenths of a dBm,
/// at a node whose noise floor is `floor_dbm`.
pub fn quality(rssi_ddbm: i16, floor_dbm: i8) -> Quality {
    let margin = f32::from(rssi_ddbm) / 10.0 - f32::from(floor_dbm);
    if margin >= good_above_floor_db() {
        Quality::Good
    } else if margin >= fair_above_floor_db() {
        Quality::Fair
    } else {
        Quality::Fragile
    }
}

/// An averaged RSSI in dBm as the value carries it.
pub fn ddbm(rssi_dbm: f32) -> i16 {
    // `as` saturates, and the radio's readings are far inside i16 tenths.
    libm::roundf(rssi_dbm * 10.0) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_fills_one_pdu() {
        assert_eq!(HEADER, 10);
        assert_eq!(ENTRY, 18);
        assert_eq!(PER_PAGE, 13);
        assert_eq!(MAX_LEN, 244);
        // 64 slots in five reads.
        assert_eq!(
            bark_proto::tables::neighbours::NEIGHBOURS.div_ceil(PER_PAGE),
            5
        );
    }

    #[test]
    fn vectors() {
        let kyiv = Position::from_degrees(50.45, 30.52);
        let h = Header {
            floor_dbm: -118,
            next: 20,
            own: kyiv,
        };
        let n = Neighbour {
            id: NodeId([1, 2, 3, 4, 5, 6]),
            position: kyiv,
            rssi_ddbm: -1105,
            age_s: 300,
            sleepy: true,
            anchor: true,
            proven: true,
            ..Neighbour::default()
        };
        let mut b = [0; MAX_LEN];
        let len = encode(&h, &[n], &mut b);
        assert_eq!(
            &b[..len],
            [
                20, 0x8a, 0x47, 0xc0, 0x49, 0x0a, 0xd9, 0xff, 0x80, 18, //
                1, 2, 3, 4, 5, 6, 0x47, 0xc0, 0x49, 0x0a, 0xd9, 0xff, 0x80, 0xfb, 0xaf, 0x01, 0x2c,
                0x91,
            ]
        );
        let (got, mut page) = decode(&b[..len]).unwrap();
        assert_eq!(got, h);
        assert_eq!(page.next(), Some(n));
        assert_eq!(page.next(), None);
        assert_eq!(decode_from(&[7]), Ok(7));
        assert_eq!(decode_from(&[]), Err(Malformed::Length));
        assert_eq!(decode_from(&[1, 2]), Err(Malformed::Length));
    }

    #[test]
    fn reads_ignore_what_a_later_version_appends() {
        let n = Neighbour {
            rssi_ddbm: -900,
            banned: true,
            ..Neighbour::default()
        };
        let mut b = [0; MAX_LEN];
        let len = encode(&Header::default(), &[n, n], &mut b);
        // The same two neighbours, each with three bytes more.
        let mut longer = [0xAA; HEADER + 2 * (ENTRY + 3)];
        longer[..HEADER].copy_from_slice(&b[..HEADER]);
        longer[AT_ENTRY] = (ENTRY + 3) as u8;
        for i in 0..2 {
            let at = HEADER + i * (ENTRY + 3);
            longer[at..at + ENTRY]
                .copy_from_slice(&b[HEADER + i * ENTRY..HEADER + (i + 1) * ENTRY]);
        }
        let (_, page) = decode(&longer).unwrap();
        assert!(page.eq([n, n]));
        assert!(decode(&b[..len - 1]).is_err());
        assert!(decode(&b[..HEADER - 1]).is_err());
        let mut short = b;
        short[AT_ENTRY] = (ENTRY - 1) as u8;
        assert!(decode(&short[..HEADER]).is_err());
    }

    #[test]
    fn quality_follows_the_margin() {
        assert!((fair_above_floor_db() - 4.08).abs() < 0.01);
        assert!((good_above_floor_db() - 6.92).abs() < 0.01);
        assert_eq!(quality(-1140, -118), Quality::Fragile);
        assert_eq!(quality(-1139, -118), Quality::Fair);
        assert_eq!(quality(-1112, -118), Quality::Fair);
        assert_eq!(quality(-1110, -118), Quality::Good);
        assert_eq!(quality(-600, -118), Quality::Good);
        assert_eq!(ddbm(-110.54), -1105);
        assert_eq!(ddbm(-110.56), -1106);
    }
}

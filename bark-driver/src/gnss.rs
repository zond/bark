//! What the node reads from its GNSS receiver: the NMEA 0183 sentences
//! GGA, GSA, GSV and RMC, and of them only what the node uses (spec
//! "Position"): whether there is a fix, the satellites in it and their
//! carrier-to-noise densities (C/N0), the position, and the UTC date and
//! time.
//!
//! [`Nmea`] takes the receiver's bytes as they come and gives a [`Fix`]
//! at each RMC sentence, the one that carries the date. The receiver
//! sends each second's sentences together, so the satellites a fix used
//! (GSA), their C/N0 (GSV) and their count (GGA) are those of the last
//! group of each before it, of this second or the one before. A sentence
//! whose checksum fails is ignored. Nothing is allocated.
//!
//! The receiver sends the sentences it is told to: [`OUTPUT`] asks for
//! these four and no others.

use bark_proto::Position;

/// The longest sentence: 82 characters from `$` to the line end (NMEA
/// 0183, §5.3); a longer line is not one.
const LINE: usize = 82;

/// Satellites a GSA sentence names as used: twelve fields (NMEA 0183,
/// GSA).
const GSA_SLOTS: usize = 12;

/// Satellites the node keeps of those used: the GSA sentences of four
/// systems, GPS, GLONASS, Galileo and BeiDou, the most the receiver
/// reports.
const USED: usize = 4 * GSA_SLOTS;

/// Satellites in view the node keeps the C/N0 of: as many as it keeps in
/// use, since only those are looked up.
const IN_VIEW: usize = USED;

/// The command that sets the receiver's sentences, each second: GGA,
/// GSA, GSV and RMC, and not GLL, VTG, ZDA or the antenna's TXT (Quectel
/// L76K&L26K GNSS Protocol Specification V1.2, 2021-12-16, PCAS03). The
/// receiver may have been told another set before; this one is told at
/// each start. The others would only fill the line: at 9600 bps it
/// carries 960 bytes a second, and the default set of GPS and BeiDou with
/// 28 satellites in view, the specification's example, is about 930.
pub const OUTPUT: &[u8] = b"$PCAS03,1,0,1,1,1,0,0,0,0,0,,,0,0*02\r\n";

/// Sentence addresses kept for the bench, the kinds the receiver sent.
const ADDRESSES: usize = 8;

/// GSA and GSV sentences kept as they came, for the bench: one per
/// talker and kind, and of a combined talker's GSA one per system, so a
/// receiver of GPS, GLONASS and BeiDou fills six.
const RAW: usize = 8;

/// The C/N0 band of a real sky, dB-Hz: open item O18 of the spec, a
/// bench number. Until it is measured the band is the receiver's whole
/// scale, 0 to 99 dB-Hz (NMEA 0183, GSV), and no fix is dropped by it;
/// the node logs the C/N0 of each fix's satellites for the bench.
pub const CN0_BAND: core::ops::RangeInclusive<u8> = 0..=99;

/// A satellite: its system (the talker's, [`system`]) and its number.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Sat {
    system: u8,
    prn: u8,
}

/// QZSS's satellite numbers. The L76K reports QZSS under GPS's talker in
/// GSV and as system 5 in GSA (L76K&L26K GNSS Protocol Specification
/// V1.2, Tables 2 and 16), so its number names its system.
const QZSS: core::ops::RangeInclusive<u32> = 193..=197;

impl Sat {
    fn new(system: u8, prn: u32) -> Sat {
        let system = if QZSS.contains(&prn) && system <= 1 {
            5
        } else {
            system
        };
        Sat {
            system,
            prn: prn.min(u8::MAX.into()) as u8,
        }
    }
}

/// The C/N0 of the satellites a fix used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cn0 {
    /// Satellites used that report a C/N0.
    pub count: u8,
    /// The lowest, dB-Hz; 0 with none.
    pub min: u8,
    /// The mean, dB-Hz; 0 with none.
    pub mean: u8,
    /// The highest, dB-Hz; 0 with none.
    pub max: u8,
    /// Satellites the GSA sentences name as used: with `count`, where a
    /// lookup fails.
    pub listed: u8,
    /// Satellites the GSV sentences report a C/N0 for.
    pub in_view: u8,
}

impl Cn0 {
    /// Whether every satellite lies in [`CN0_BAND`] (spec "Position").
    pub fn real(&self) -> bool {
        self.count == 0 || (CN0_BAND.contains(&self.min) && CN0_BAND.contains(&self.max))
    }
}

/// What one second's sentences say.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fix {
    /// The position, if the receiver has a fix: RMC's status `A`.
    pub position: Option<Position>,
    /// Seconds since 1970-01-01 00:00 UTC, if the fix gives the date and
    /// time.
    pub utc_s: Option<u64>,
    /// Satellites used, GGA's count.
    pub satellites: u8,
    /// Their C/N0.
    pub cn0: Cn0,
}

/// The parser's state between bytes.
pub struct Nmea {
    line: [u8; LINE],
    len: usize,
    /// The bytes since `$` overran [`LINE`]: the line is dropped.
    overrun: bool,
    /// The kind of the last sentence taken, to tell a new group of GSA or
    /// GSV from the rest of one.
    last: [u8; 3],
    used: [Sat; USED],
    n_used: usize,
    seen: [(Sat, u8); IN_VIEW],
    n_seen: usize,
    satellites: u8,
    /// The last GSA and GSV sentences, as they came, for the bench: the
    /// sentence's address and, for a GSA, its system field's first byte;
    /// the line; its length.
    raw: [([u8; 6], [u8; LINE], usize); RAW],
    /// The addresses of the sentences taken, in the order first seen.
    addresses: [[u8; 5]; ADDRESSES],
    /// Sentences dropped for their checksum or their length.
    pub bad: u32,
}

impl Default for Nmea {
    fn default() -> Self {
        Nmea {
            line: [0; LINE],
            len: 0,
            overrun: false,
            last: [0; 3],
            used: [Sat::default(); USED],
            n_used: 0,
            seen: [(Sat::default(), 0); IN_VIEW],
            n_seen: 0,
            satellites: 0,
            raw: [([0; 6], [0; LINE], 0); RAW],
            addresses: [[0; 5]; ADDRESSES],
            bad: 0,
        }
    }
}

/// The system a talker names: 1 GPS, 2 GLONASS, 3 Galileo, 4 BeiDou,
/// 5 QZSS, as GSA's system field numbers them (NMEA 0183 4.11); 0 for
/// `GN`, the combined talker, whose GSA says its system in that field.
fn system(talker: &[u8]) -> u8 {
    match talker {
        b"GP" => 1,
        b"GL" => 2,
        b"GA" => 3,
        b"GB" | b"BD" => 4,
        b"GQ" => 5,
        _ => 0,
    }
}

/// A field's decimal digits, ignoring a fraction; `None` if empty or not
/// digits.
fn number(f: &[u8]) -> Option<u32> {
    let whole = f.split(|&c| c == b'.').next()?;
    if whole.is_empty() || whole.len() > 9 {
        return None;
    }
    whole.iter().try_fold(0u32, |n, &c| {
        c.is_ascii_digit().then(|| n * 10 + u32::from(c - b'0'))
    })
}

/// Millionths of a minute in half the circle, 180°, which a position's
/// counts divide into 2²⁵ (`bark_proto::position`).
const HALF_CIRCLE_UMIN: u64 = 180 * 60 * 1_000_000;

/// A position's counts from `dddmm.mmmm` and its hemisphere letter,
/// rounded to the nearest count and clamped to a field of `bits`, as
/// `Position::from_degrees` does. In integers: the board's processor has
/// no double-precision unit.
fn counts(f: &[u8], hemisphere: &[u8], negative: u8, bits: u32) -> Option<i32> {
    let dot = f.iter().position(|&c| c == b'.').unwrap_or(f.len());
    if dot < 3 {
        return None;
    }
    let deg = u64::from(number(&f[..dot - 2])?);
    let mut umin = (deg * 60 + u64::from(number(&f[dot - 2..dot])?)) * 1_000_000;
    let mut scale = 100_000;
    for &c in f.get(dot + 1..).unwrap_or(&[]) {
        if !c.is_ascii_digit() {
            return None;
        }
        umin += u64::from(c - b'0') * scale;
        scale /= 10;
    }
    let n = ((umin << 26) + HALF_CIRCLE_UMIN) / (2 * HALF_CIRCLE_UMIN);
    let half = 1u64 << (bits - 1);
    match hemisphere {
        [c] if *c == negative => Some(-(n.min(half) as i64) as i32),
        [_] => Some(n.min(half - 1) as i32),
        _ => None,
    }
}

/// Days from 1970-01-01 to a date of the Gregorian calendar.
fn days(year: i64, month: u32, day: u32) -> i64 {
    // Counted from 1 March, so that the leap day ends a year.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Seconds since 1970 from RMC's `hhmmss.ss` and `ddmmyy`. The two-digit
/// year is 2000 to 2099.
fn utc(time: &[u8], date: &[u8]) -> Option<u64> {
    if time.len() < 6 || date.len() != 6 {
        return None;
    }
    let (h, m, s) = (
        number(&time[..2])?,
        number(&time[2..4])?,
        number(&time[4..6])?,
    );
    let (dd, mm, yy) = (
        number(&date[..2])?,
        number(&date[2..4])?,
        number(&date[4..])?,
    );
    if h > 23 || m > 59 || s > 60 || !(1..=12).contains(&mm) || !(1..=31).contains(&dd) {
        return None;
    }
    let d = days(2000 + i64::from(yy), mm, dd);
    u64::try_from(d * 86_400 + i64::from(h * 3600 + m * 60 + s)).ok()
}

impl Nmea {
    /// Take the receiver's bytes; `fix` is called with each second's
    /// [`Fix`].
    pub fn feed(&mut self, bytes: &[u8], fix: &mut impl FnMut(Fix)) {
        for &b in bytes {
            match b {
                b'$' => {
                    self.len = 0;
                    self.overrun = false;
                    self.push(b);
                }
                b'\r' | b'\n' => {
                    if self.len > 0 && !self.overrun {
                        let line = self.line;
                        if let Some(f) = self.sentence(&line[..self.len]) {
                            fix(f);
                        }
                    }
                    self.len = 0;
                }
                _ if self.len > 0 => self.push(b),
                _ => {}
            }
        }
    }

    fn push(&mut self, b: u8) {
        if self.len == LINE {
            self.overrun = true;
            self.bad += 1;
            self.len = 0;
            return;
        }
        self.line[self.len] = b;
        self.len += 1;
    }

    /// One line from `$`; a [`Fix`] if it is an RMC.
    fn sentence(&mut self, line: &[u8]) -> Option<Fix> {
        // `$`, the body, `*` and two hex digits of the XOR of the body.
        let star = line.iter().rposition(|&c| c == b'*');
        let checked = star.and_then(|s| {
            let hex = core::str::from_utf8(line.get(s + 1..s + 3)?).ok()?;
            let sum = u8::from_str_radix(hex, 16).ok()?;
            (line[1..s].iter().fold(0, |x, &c| x ^ c) == sum).then_some(&line[1..s])
        });
        let Some(body) = checked else {
            self.bad += 1;
            return None;
        };
        let mut fields = body.split(|&c| c == b',');
        let head = fields.next()?;
        if head.len() != 5 {
            return None;
        }
        self.note(head);
        let (talker, kind) = head.split_at(2);
        let mut f = [&[][..]; 20];
        let mut n = 0;
        for field in fields.take(f.len()) {
            f[n] = field;
            n += 1;
        }
        let new_group = self.last != kind;
        self.last.copy_from_slice(kind);
        match kind {
            b"GGA" => {
                self.satellites = number(f[6]).unwrap_or(0).min(u8::MAX.into()) as u8;
                None
            }
            b"GSA" => {
                if new_group {
                    self.n_used = 0;
                }
                self.keep(head, f[17].first().copied().unwrap_or(0), line);
                let system = match (system(talker), number(f[17])) {
                    (0, Some(s)) => s as u8,
                    (s, _) => s,
                };
                for slot in &f[2..2 + GSA_SLOTS] {
                    let Some(prn) = number(slot) else { continue };
                    if self.n_used < USED {
                        self.used[self.n_used] = Sat::new(system, prn);
                        self.n_used += 1;
                    }
                }
                None
            }
            b"GSV" => {
                if new_group {
                    self.n_seen = 0;
                }
                self.keep(head, 0, line);
                let system = system(talker);
                // After the three counts, a satellite is four fields:
                // number, elevation, azimuth, C/N0; a field may follow,
                // the signal (NMEA 0183 4.10).
                let mut at = 3;
                while at + 4 <= n {
                    let prn = number(f[at]);
                    let cn0 = number(f[at + 3]);
                    if let (Some(prn), Some(cn0)) = (prn, cn0) {
                        if self.n_seen < IN_VIEW {
                            let sat = Sat::new(system, prn);
                            self.seen[self.n_seen] = (sat, cn0.min(u8::MAX.into()) as u8);
                            self.n_seen += 1;
                        }
                    }
                    at += 4;
                }
                None
            }
            b"RMC" => {
                let position = match f[1] {
                    b"A" => counts(f[2], f[3], b'S', 25)
                        .zip(counts(f[4], f[5], b'W', 26))
                        .map(|(lat, lon)| Position { lat, lon }),
                    _ => None,
                };
                Some(Fix {
                    position,
                    utc_s: position.and(utc(f[0], f[8])),
                    satellites: self.satellites,
                    cn0: self.cn0(),
                })
            }
            _ => None,
        }
    }

    /// Note a sentence's address among those seen.
    fn note(&mut self, head: &[u8]) {
        if self.addresses.iter().any(|a| a == head) {
            return;
        }
        if let Some(a) = self.addresses.iter_mut().find(|a| a[0] == 0) {
            a.copy_from_slice(head);
        }
    }

    /// The addresses of the sentences taken, such as `GNRMC`, in the
    /// order first seen.
    pub fn addresses(&self) -> impl Iterator<Item = &[u8]> + Clone {
        self.addresses.iter().filter(|a| a[0] != 0).map(|a| &a[..])
    }

    /// Keep `line` as the last of its address and `system`.
    fn keep(&mut self, head: &[u8], system: u8, line: &[u8]) {
        let mut key = [0; 6];
        key[..5].copy_from_slice(head);
        key[5] = system;
        let slot = self.raw.iter().position(|r| r.0 == key);
        let Some(i) = slot.or_else(|| self.raw.iter().position(|r| r.2 == 0)) else {
            return;
        };
        let r = &mut self.raw[i];
        r.0 = key;
        r.1[..line.len()].copy_from_slice(line);
        r.2 = line.len();
    }

    /// The last GSA and GSV sentences of each talker, from `$` to the
    /// checksum, in the order first seen.
    pub fn last_gsa_gsv(&self) -> impl Iterator<Item = &[u8]> + Clone {
        self.raw.iter().filter(|r| r.2 > 0).map(|r| &r.1[..r.2])
    }

    /// The C/N0 of the satellites used, as last reported.
    fn cn0(&self) -> Cn0 {
        let lists = Cn0 {
            listed: self.n_used as u8,
            in_view: self.n_seen as u8,
            ..Cn0::default()
        };
        let mut c = Cn0 {
            min: u8::MAX,
            ..Cn0::default()
        };
        let mut sum = 0u32;
        for used in &self.used[..self.n_used] {
            let found = self.seen[..self.n_seen]
                .iter()
                .find(|(s, _)| s == used || (used.system == 0 && s.prn == used.prn));
            if let Some(&(_, cn0)) = found {
                c.count += 1;
                c.min = c.min.min(cn0);
                c.max = c.max.max(cn0);
                sum += u32::from(cn0);
            }
        }
        if c.count == 0 {
            return lists;
        }
        c.mean = (sum / u32::from(c.count)) as u8;
        Cn0 {
            listed: lists.listed,
            in_view: lists.in_view,
            ..c
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    /// A sentence with its checksum.
    fn s(body: &str) -> std::string::String {
        let sum = body.bytes().fold(0u8, |x, c| x ^ c);
        std::format!("${body}*{sum:02X}\r\n")
    }

    fn fixes(n: &mut Nmea, text: &str) -> Vec<Fix> {
        let mut out = Vec::new();
        // In pieces, as the UART hands them.
        for chunk in text.as_bytes().chunks(7) {
            n.feed(chunk, &mut |f| out.push(f));
        }
        out
    }

    /// One second of a receiver with a fix from GPS and BeiDou satellites,
    /// in NMEA 4.10 with the combined talker.
    fn second() -> std::string::String {
        [
            s("GNGGA,123519.000,5027.0060,N,03031.4040,E,1,05,1.2,180.0,M,27.0,M,,"),
            s("GNGSA,A,3,04,07,09,,,,,,,,,,2.1,1.2,1.7,1"),
            s("GNGSA,A,3,13,21,,,,,,,,,,,2.1,1.2,1.7,4"),
            s("GPGSV,2,1,05,04,60,100,44,07,40,200,38,09,20,300,31,11,10,050,,1"),
            s("GPGSV,2,2,05,16,05,010,22,1"),
            s("BDGSV,1,1,02,13,50,120,40,21,30,220,35,1"),
            s("GNRMC,123519.000,A,5027.0060,N,03031.4040,E,0.0,0.0,051126,,,A,V"),
        ]
        .concat()
    }

    #[test]
    fn a_second_with_a_fix() {
        let mut n = Nmea::default();
        let got = fixes(&mut n, &second());
        assert_eq!(got.len(), 1);
        let f = got[0];
        let p = f.position.expect("a fix");
        let degrees = Position::from_degrees(50.0 + 27.006 / 60.0, 30.0 + 31.404 / 60.0);
        assert_eq!(p, degrees, "as the core rounds");
        let (lat, lon) = p.micro_degrees();
        assert!((lat - 50_450_100).abs() <= 3, "{lat}");
        assert!((lon - 30_523_400).abs() <= 3, "{lon}");
        // 2026-11-05 12:35:19 UTC.
        assert_eq!(f.utc_s, Some(1_793_882_119));
        assert_eq!(f.satellites, 5);
        // 04, 07, 09 of GPS and 13, 21 of BeiDou; 11 and 16 not used.
        assert_eq!(
            f.cn0,
            Cn0 {
                count: 5,
                min: 31,
                mean: 37,
                max: 44,
                listed: 5,
                in_view: 6,
            }
        );
        assert!(f.cn0.real());
        assert_eq!(n.bad, 0);
        // A GSA of each system, the last GSV of each talker.
        let raw: Vec<_> = n.last_gsa_gsv().map(|l| l[..6].to_vec()).collect();
        assert_eq!(raw, [b"$GNGSA", b"$GNGSA", b"$GPGSV", b"$BDGSV"]);
        // The next second starts its groups afresh.
        let again = fixes(&mut n, &second());
        assert_eq!(again[0].cn0, f.cn0);
    }

    /// The L76K's default sentences of one second, verbatim with their
    /// checksums from the examples of the L76K&L26K GNSS Protocol
    /// Specification V1.2 (2021-12-16), §2.2, in the order of PCAS03's
    /// fields. The examples are not of one second (ZDA's time differs),
    /// but each is what the receiver sends.
    const L76K_SECOND: &str = "\
$GNGGA,071556.000,3149.29103,N,11706.92916,E,1,21,0.7,75.7,M,-5.0,M,,*69\r\n\
$GNGLL,3149.29103,N,11706.92916,E,071556.000,A,A*45\r\n\
$GNGSA,A,3,10,13,15,20,,,,,,,,,2.5,2.0,1.5,1*35\r\n\
$GPGSV,3,1,12,02,49,123,43,05,60,005,43,06,06,127,29,07,05,042,33,0*66\r\n\
$GPGSV,3,2,12,13,79,139,44,15,55,225,44,18,12,314,36,20,44,055,42,0*6C\r\n\
$GPGSV,3,3,12,29,48,275,42,30,20,069,38,193,69,062,42,195,46,160,42,0*6A\r\n\
$BDGSV,4,1,16,01,43,135,36,03,54,193,36,04,31,120,34,06,43,189,35,0*7B\r\n\
$BDGSV,4,2,16,07,06,196,28,08,63,008,35,09,30,194,32,10,09,210,29,0*74\r\n\
$BDGSV,4,3,16,13,59,342,38,16,50,184,37,27,57,183,40,30,53,295,39,0*77\r\n\
$BDGSV,4,4,16,32,65,305,41,38,68,046,40,39,59,181,38,41,40,042,38,0*78\r\n\
$GNRMC,071556.000,A,3149.29103,N,11706.92916,E,0.00,0.00,250420,,,A,V*09\r\n\
$GNVTG,0.00,T,,M,0.00,N,0.00,K,A*23\r\n\
$GNZDA,053712.000,21,10,2021,00,00*49\r\n\
$GPTXT,01,01,01,ANTENNA OPEN*25\r\n";

    #[test]
    fn the_l76k_specification_second() {
        let mut n = Nmea::default();
        let got = fixes(&mut n, L76K_SECOND);
        assert_eq!(n.bad, 0, "the specification's checksums hold");
        assert_eq!(got.len(), 1);
        let f = got[0];
        let p = f.position.expect("a fix");
        let (lat, lon) = p.micro_degrees();
        assert!((lat - 31_821_517).abs() <= 3, "{lat}");
        assert!((lon - 117_115_486).abs() <= 3, "{lon}");
        // 2020-04-25 07:15:56 UTC.
        assert_eq!(f.utc_s, Some(1_587_798_956));
        assert_eq!(f.satellites, 21);
        // GPS 10, 13, 15, 20 used; 10 is not in view in this example.
        assert_eq!(
            f.cn0,
            Cn0 {
                count: 3,
                min: 42,
                mean: 43,
                max: 44,
                listed: 4,
                in_view: 28,
            }
        );
        let addresses: Vec<_> = n.addresses().collect();
        assert_eq!(addresses.len(), 8, "every kind noted");
    }

    #[test]
    fn a_second_cut_anywhere() {
        let mut whole = Nmea::default();
        let want = fixes(&mut whole, L76K_SECOND);
        let text = L76K_SECOND.as_bytes();
        for cut in 0..=text.len() {
            let mut n = Nmea::default();
            let mut got = Vec::new();
            n.feed(&text[..cut], &mut |f| got.push(f));
            n.feed(&text[cut..], &mut |f| got.push(f));
            assert_eq!(got, want, "cut at {cut}");
            assert_eq!(n.bad, 0);
        }
        let mut n = Nmea::default();
        let mut got = Vec::new();
        for b in text.chunks(1) {
            n.feed(b, &mut |f| got.push(f));
        }
        assert_eq!(got, want, "a byte at a time");
    }

    /// One second of GPS, GLONASS, BeiDou and QZSS, with or without
    /// NMEA 4.10's system field in GSA and signal field in GSV.
    fn four_systems(fields_4_10: bool) -> std::string::String {
        let sig = if fields_4_10 { ",1" } else { "" };
        let gsa = |prns: &str, id: &str| {
            let tail = if fields_4_10 {
                std::format!(",{id}")
            } else {
                std::string::String::new()
            };
            s(&std::format!("GNGSA,A,3,{prns},1.6,0.9,1.3{tail}"))
        };
        [
            s("GNGGA,101500.000,5912.9071,N,01812.7088,E,1,07,0.9,30.0,M,24.0,M,,"),
            gsa("05,13,,,,,,,,,,", "1"),
            gsa("70,,,,,,,,,,,", "2"),
            gsa("13,21,,,,,,,,,,", "4"),
            gsa("194,,,,,,,,,,,", "5"),
            s(&std::format!(
                "GPGSV,2,1,05,05,60,100,41,13,40,200,38,15,20,300,,194,30,150,33{sig}"
            )),
            s(&std::format!("GPGSV,2,2,05,20,10,050,22{sig}")),
            s(&std::format!("GLGSV,1,1,02,70,50,120,36,71,10,220,{sig}")),
            s(&std::format!("BDGSV,1,1,02,13,50,120,29,21,30,220,35{sig}")),
            s("GNRMC,101500.000,A,5912.9071,N,01812.7088,E,0.0,0.0,061126,,,A,V"),
        ]
        .concat()
    }

    #[test]
    fn used_satellites_matched_by_system_and_number() {
        let mut n = Nmea::default();
        let f = fixes(&mut n, &four_systems(true))[0];
        assert_eq!(n.bad, 0);
        // GPS 05 41 and 13 38, GLONASS 70 36, BeiDou 13 29 and 21 35, QZSS
        // 194 33: GPS 13 and BeiDou 13 each their own.
        assert_eq!(
            f.cn0,
            Cn0 {
                count: 6,
                min: 29,
                mean: 35,
                max: 41,
                listed: 6,
                in_view: 7,
            }
        );
    }

    #[test]
    fn without_the_system_and_signal_fields() {
        let mut n = Nmea::default();
        let f = fixes(&mut n, &four_systems(false))[0];
        assert_eq!(n.bad, 0);
        assert_eq!(f.cn0.listed, 6);
        // A combined GSA without its system matches by number alone, so
        // its GPS 13 and BeiDou 13 each take the first 13 in view, GPS's.
        assert_eq!(f.cn0.count, 6);
        assert_eq!((f.cn0.min, f.cn0.max), (33, 41));
    }

    #[test]
    fn the_output_command_is_a_sentence() {
        let line = core::str::from_utf8(OUTPUT).unwrap();
        let body = line.trim_end().trim_start_matches('$');
        let (body, sum) = body.split_once('*').unwrap();
        assert_eq!(s(body), line, "checksum {sum}");
        assert_eq!(body.split(',').count(), 15, "PCAS03 and its 14 fields");
    }

    #[test]
    fn no_fix_no_position_no_time() {
        let mut n = Nmea::default();
        let text = [
            s("GPGGA,,,,,,0,00,99.9,,,,,,"),
            s("GPRMC,,V,,,,,,,,,,N,V"),
            s("GPRMC,235959.000,V,,,,,,,311226,,,N,V"),
        ]
        .concat();
        let got = fixes(&mut n, &text);
        assert_eq!(got.len(), 2);
        assert!(got
            .iter()
            .all(|f| f.position.is_none() && f.utc_s.is_none()));
        assert_eq!(got[0].satellites, 0);
    }

    #[test]
    fn a_bad_checksum_or_a_long_line_is_dropped() {
        let mut n = Nmea::default();
        let good = s("GPRMC,000000.00,A,0000.0000,S,00000.0000,W,0,0,010100,,,A");
        let mut bad = good.clone().into_bytes();
        bad[10] ^= 1;
        let long = std::format!("${}\r\n", "A".repeat(100));
        let text = std::format!("{}{long}{good}", std::str::from_utf8(&bad).unwrap());
        let got = fixes(&mut n, &text);
        assert_eq!(got.len(), 1);
        assert_eq!(n.bad, 2);
        assert_eq!(got[0].utc_s, Some(946_684_800), "2000-01-01");
        assert_eq!(got[0].position, Some(Position::from_degrees(0.0, 0.0)));
    }

    #[test]
    fn southern_and_western_hemispheres() {
        let mut n = Nmea::default();
        let text = s("GPRMC,010203,A,3352.1280,S,15112.5600,W,0,0,290224,,,A");
        let f = fixes(&mut n, &text)[0];
        let (lat, lon) = f.position.unwrap().micro_degrees();
        assert!((lat + 33_868_800).abs() <= 3, "{lat}");
        assert!((lon + 151_209_333).abs() <= 3, "{lon}");
        // 2024-02-29 01:02:03 UTC, a leap day.
        assert_eq!(f.utc_s, Some(1_709_168_523));
    }

    #[test]
    fn days_from_the_epoch() {
        assert_eq!(days(1970, 1, 1), 0);
        assert_eq!(days(2000, 3, 1), 11_017);
        assert_eq!(days(2100, 3, 1), 47_541);
    }
}

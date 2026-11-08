//! The bond: the keys a node keeps for the one phone paired with it
//! (spec "Phone link", Pairing), as the node stores them in flash, two of
//! the stored-state page's 32-byte slots (`bark-fw/src/store.rs`).
//!
//! | bytes | |
//! |---|---|
//! | 0 | [`Bond::TAG`] |
//! | 1 | the phone's identity address type: 0 public, 1 random static (Vol 3 Part H §3.6.5) |
//! | 2–7 | the identity address, least significant byte first, as the HCI carries it |
//! | 8 | 1 if an identity resolving key follows, else 0 |
//! | 9 | the link version whose attribute table the phone last saw, 0 for none known |
//! | 10 | 1 if the phone has asked for Service Changed indications, else 0 |
//! | 11 | 1 if the node is public, else 0 (spec "Phone link", Public) |
//! | 12 | 1 if the paired phone's recipient tag follows, else 0 |
//! | 13 | the paired phone's recipient tag at the node, or 0 |
//! | 14–15 | 0 |
//! | 16–31 | the long-term key the pairing made, least significant byte first (Vol 3 Part H §2.3.5.6.5) |
//! | 32–47 | the phone's identity resolving key, or 0 (Vol 3 Part H §3.6.4) |
//! | 48–59 | 0 |
//! | 60–63 | the complement of bytes 0–3 |
//!
//! The flash writes a record a word at a time in address order, so a
//! record cut short by a power cut leaves its last word erased, which is
//! no complement of its first.

/// The keys of a bond, and the phone they belong to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bond {
    /// The phone's identity address type, as the HCI gives it.
    pub kind: u8,
    /// The phone's identity address.
    pub addr: [u8; 6],
    /// The phone's identity resolving key, if it gave one.
    pub irk: Option<[u8; 16]>,
    /// The long-term key the link is encrypted with.
    pub ltk: [u8; 16],
    /// The link version ([`crate::VERSION`]) whose attribute table the
    /// phone last saw; 0 in a record from before this field, whose phone
    /// saw a table with no way to tell it of a change.
    pub seen: u8,
    /// The phone's subscription to Service Changed, which a bonded
    /// client's is kept across connections (Bluetooth Core Vol 3 Part G
    /// §3.3.3.3).
    pub service_changed: bool,
    /// The node is public: false in a record from before this field.
    pub public: bool,
    /// The paired phone's recipient tag at the node, which it writes at
    /// every connection; kept so that the node knows its owner's payloads
    /// while the owner is away. `None` until the phone writes one.
    pub owner: Option<u8>,
}

impl Bond {
    /// Bytes of the record.
    pub const LEN: usize = 64;
    /// The record's first byte: the stored-state page's other records
    /// start with their layout's version, counted from 1, or with 0x80,
    /// the UTC offset's (`bark-fw/src/store.rs`).
    pub const TAG: u8 = 0x81;

    /// The record.
    pub fn record(&self) -> [u8; Self::LEN] {
        let mut r = [0u8; Self::LEN];
        r[0] = Self::TAG;
        r[1] = self.kind;
        r[2..8].copy_from_slice(&self.addr);
        if let Some(irk) = self.irk {
            r[8] = 1;
            r[32..48].copy_from_slice(&irk);
        }
        r[9] = self.seen;
        r[10] = u8::from(self.service_changed);
        r[11] = u8::from(self.public);
        if let Some(t) = self.owner {
            r[12] = 1;
            r[13] = t;
        }
        r[16..32].copy_from_slice(&self.ltk);
        let check = [!r[0], !r[1], !r[2], !r[3]];
        r[60..].copy_from_slice(&check);
        r
    }

    /// The bond a whole record holds; `None` for another record or one
    /// cut short.
    pub fn from_record(r: &[u8; Self::LEN]) -> Option<Bond> {
        let whole = r[0] == Self::TAG && (0..4).all(|i| r[60 + i] == !r[i]);
        if !whole || r[8] > 1 || r[10] > 1 || r[11] > 1 || r[12] > 1 {
            return None;
        }
        let mut addr = [0u8; 6];
        addr.copy_from_slice(&r[2..8]);
        let mut ltk = [0u8; 16];
        ltk.copy_from_slice(&r[16..32]);
        let irk = (r[8] == 1).then(|| {
            let mut k = [0u8; 16];
            k.copy_from_slice(&r[32..48]);
            k
        });
        Some(Bond {
            kind: r[1],
            addr,
            irk,
            ltk,
            seen: r[9],
            service_changed: r[10] == 1,
            public: r[11] == 1,
            owner: (r[12] == 1).then_some(r[13]),
        })
    }
}

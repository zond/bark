//! The payload's four forms (spec "Phone link"). The
//! first byte, in the clear, is the format: the version in the high four
//! bits, the form in the low four.
//!
//! | form | after the format byte |
//! |---|---|
//! | 0, boxed | nonce (24), `crypto_box_easy`: tag (16), ciphertext |
//! | 1, introduction | `crypto_box_seal`: ephemeral key (32), tag (16), ciphertext |
//! | 2, fetch | nonce (24), `crypto_secretbox_easy`: tag (16), ciphertext |
//! | 3, from the mailbox | `crypto_secretbox_easy`: tag (16), ciphertext of the item's number (3) and the item, or of nothing; nonce derived, not sent |

use blake2::digest::consts::U24;
use blake2::{Blake2b, Digest};
use crypto_box::aead::generic_array::GenericArray;
use crypto_box::aead::AeadCore;
use crypto_box::aead::{AeadInPlace, KeyInit};
use crypto_box::{PublicKey, SalsaBox, SecretKey};
use crypto_secretbox::XSalsa20Poly1305;

use crate::keys::KEY_LEN;
use crate::{be, Address, Error, MAX_APP_PAYLOAD, MAX_PAYLOAD, VERSION};

/// The forms, the format byte's low four bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    /// To a contact, `crypto_box`.
    Boxed = 0,
    /// To someone who may not know the sender, a sealed box.
    Introduction = 1,
    /// To one's own mailbox, `crypto_secretbox` under the mailbox key.
    Fetch = 2,
    /// From the mailbox to the person, under the mailbox key.
    Reply = 3,
}

impl Form {
    /// The format byte of this form.
    pub const fn format(self) -> u8 {
        VERSION << 4 | self as u8
    }

    /// The form of `payload`, if it is one this crate reads.
    pub fn of(payload: &[u8]) -> Result<Form, Error> {
        let first = *payload.first().ok_or(Error::Malformed)?;
        if first >> 4 != VERSION {
            return Err(Error::Malformed);
        }
        match first & 0xf {
            0 => Ok(Form::Boxed),
            1 => Ok(Form::Introduction),
            2 => Ok(Form::Fetch),
            3 => Ok(Form::Reply),
            _ => Err(Error::Malformed),
        }
    }
}

/// Bytes of a nonce.
pub const NONCE_LEN: usize = 24;
/// Bytes of a tag.
pub const TAG_LEN: usize = 16;

/// Bytes a boxed payload adds to its plaintext.
pub const BOX_OVERHEAD: usize = 1 + NONCE_LEN + TAG_LEN;
/// Bytes an introduction adds to its plaintext.
pub const INTRO_OVERHEAD: usize = 1 + KEY_LEN + TAG_LEN;
/// Bytes a reply adds to the item it carries: the format byte, the tag
/// and the item's number.
pub const REPLY_OVERHEAD: usize = 1 + TAG_LEN + NUMBER_LEN;
/// Bytes of the reply that says nothing is new.
pub const NOTHING_NEW_LEN: usize = 1 + TAG_LEN;

/// The longest plaintext of a boxed payload.
pub const MAX_BOX_PLAIN: usize = MAX_APP_PAYLOAD - BOX_OVERHEAD;
/// The longest plaintext of an introduction.
pub const MAX_INTRO_PLAIN: usize = MAX_APP_PAYLOAD - INTRO_OVERHEAD;

/// The shared key of two identities, which `crypto_box_beforenm` makes
/// once per contact: the receiver tries each contact's.
pub fn pair(their: &PublicKey, mine: &SecretKey) -> SalsaBox {
    SalsaBox::new(their, mine)
}

/// Writes `plain` sealed into `out` after `head`, the tag first as
/// libsodium writes it; returns the length.
fn seal_into<A: AeadInPlace + AeadCore<NonceSize = U24>>(
    cipher: &A,
    nonce: &[u8; NONCE_LEN],
    head: &[u8],
    plain: &[u8],
    out: &mut [u8],
) -> Result<usize, Error> {
    let at = head.len() + TAG_LEN;
    let len = at + plain.len();
    if out.len() < len {
        return Err(Error::TooLong);
    }
    out[..head.len()].copy_from_slice(head);
    out[at..len].copy_from_slice(plain);
    let tag = cipher
        .encrypt_in_place_detached(GenericArray::from_slice(nonce), &[], &mut out[at..len])
        .map_err(|_| Error::TooLong)?;
    out[head.len()..at].copy_from_slice(&tag);
    Ok(len)
}

/// Opens `sealed`, the tag first, into `out`; returns the plaintext's
/// length.
fn open_into<A: AeadInPlace + AeadCore<NonceSize = U24>>(
    cipher: &A,
    nonce: &[u8; NONCE_LEN],
    sealed: &[u8],
    out: &mut [u8],
) -> Result<usize, Error> {
    let len = sealed.len().checked_sub(TAG_LEN).ok_or(Error::Malformed)?;
    if out.len() < len {
        return Err(Error::TooLong);
    }
    out[..len].copy_from_slice(&sealed[TAG_LEN..]);
    cipher
        .decrypt_in_place_detached(
            GenericArray::from_slice(nonce),
            &[],
            &mut out[..len],
            GenericArray::from_slice(&sealed[..TAG_LEN]),
        )
        .map_err(|_| Error::Open)?;
    Ok(len)
}

fn nonce_at(payload: &[u8]) -> Result<[u8; NONCE_LEN], Error> {
    let mut n = [0u8; NONCE_LEN];
    n.copy_from_slice(payload.get(1..1 + NONCE_LEN).ok_or(Error::Malformed)?);
    Ok(n)
}

/// A boxed payload (form 0) of `plain` under a pair's key, with a random
/// `nonce`, into `out`; its length.
pub fn seal_box(
    pair: &SalsaBox,
    nonce: &[u8; NONCE_LEN],
    plain: &[u8],
    out: &mut [u8],
) -> Result<usize, Error> {
    if plain.len() > MAX_BOX_PLAIN {
        return Err(Error::TooLong);
    }
    let mut head = [0u8; 1 + NONCE_LEN];
    head[0] = Form::Boxed.format();
    head[1..].copy_from_slice(nonce);
    seal_into(pair, nonce, &head, plain, out)
}

/// The plaintext of a boxed payload under a pair's key, into `out`; its
/// length, or [`Error::Open`] if the pair is not the one.
pub fn open_box(pair: &SalsaBox, payload: &[u8], out: &mut [u8]) -> Result<usize, Error> {
    if Form::of(payload)? != Form::Boxed || payload.len() > MAX_PAYLOAD {
        return Err(Error::Malformed);
    }
    open_into(pair, &nonce_at(payload)?, &payload[1 + NONCE_LEN..], out)
}

/// `crypto_box_seal`'s nonce: BLAKE2b-192 of the ephemeral key and the
/// receiver's.
fn seal_nonce(ephemeral: &PublicKey, to: &PublicKey) -> [u8; NONCE_LEN] {
    let mut h = Blake2b::<U24>::new();
    h.update(ephemeral.as_bytes());
    h.update(to.as_bytes());
    h.finalize().into()
}

/// An introduction (form 1) of `plain` to `to`, sealed with a fresh
/// `ephemeral` key, into `out`; its length.
pub fn seal_intro(
    to: &PublicKey,
    ephemeral: &SecretKey,
    plain: &[u8],
    out: &mut [u8],
) -> Result<usize, Error> {
    if plain.len() > MAX_INTRO_PLAIN {
        return Err(Error::TooLong);
    }
    let epk = ephemeral.public_key();
    let mut head = [0u8; 1 + KEY_LEN];
    head[0] = Form::Introduction.format();
    head[1..].copy_from_slice(epk.as_bytes());
    seal_into(
        &SalsaBox::new(to, ephemeral),
        &seal_nonce(&epk, to),
        &head,
        plain,
        out,
    )
}

/// The plaintext of an introduction to `me`, into `out`; its length.
pub fn open_intro(me: &SecretKey, payload: &[u8], out: &mut [u8]) -> Result<usize, Error> {
    if Form::of(payload)? != Form::Introduction || payload.len() > MAX_PAYLOAD {
        return Err(Error::Malformed);
    }
    let mut e = [0u8; KEY_LEN];
    e.copy_from_slice(payload.get(1..1 + KEY_LEN).ok_or(Error::Malformed)?);
    let epk = PublicKey::from(e);
    let nonce = seal_nonce(&epk, &me.public_key());
    open_into(
        &SalsaBox::new(&epk, me),
        &nonce,
        &payload[1 + KEY_LEN..],
        out,
    )
}

/// A fetch (spec "Mailbox"): the requester asks its home node for every
/// item after `since`, to be sent to `address`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fetch {
    /// The requester's clock, as a text's `sent`; a fetch is taken only
    /// if later than the last one taken.
    pub sent: u32,
    /// The requester's current address.
    pub address: Address,
    /// The number up to which it has every item; 0 asks for all the
    /// ring holds.
    pub since: u32,
}

impl Fetch {
    /// Bytes of its plaintext.
    pub const PLAIN_LEN: usize = 4 + Address::LEN + NUMBER_LEN;
    /// Bytes of its payload.
    pub const LEN: usize = BOX_OVERHEAD + Self::PLAIN_LEN;

    fn plain(&self) -> [u8; Self::PLAIN_LEN] {
        let mut b = [0u8; Self::PLAIN_LEN];
        b[..4].copy_from_slice(&self.sent.to_be_bytes());
        b[4..4 + Address::LEN].copy_from_slice(&self.address.encode());
        b[4 + Address::LEN..].copy_from_slice(&self.since.to_be_bytes()[1..]);
        b
    }

    /// The fetch as a payload (form 2) under the mailbox `key`, with a
    /// random `nonce`.
    pub fn seal(
        &self,
        key: &[u8; KEY_LEN],
        nonce: &[u8; NONCE_LEN],
    ) -> Result<[u8; Self::LEN], Error> {
        if self.since > MAX_NUMBER {
            return Err(Error::TooLong);
        }
        let mut head = [0u8; 1 + NONCE_LEN];
        head[0] = Form::Fetch.format();
        head[1..].copy_from_slice(nonce);
        let mut out = [0u8; Self::LEN];
        seal_into(&secretbox(key), nonce, &head, &self.plain(), &mut out)?;
        Ok(out)
    }

    /// The fetch in `payload`, if it is one under `key`.
    pub fn open(key: &[u8; KEY_LEN], payload: &[u8]) -> Result<Fetch, Error> {
        if Form::of(payload)? != Form::Fetch || payload.len() != Self::LEN {
            return Err(Error::Malformed);
        }
        let mut b = [0u8; Self::PLAIN_LEN];
        open_into(
            &secretbox(key),
            &nonce_at(payload)?,
            &payload[1 + NONCE_LEN..],
            &mut b,
        )?;
        Ok(Fetch {
            sent: be(&b[..4]),
            address: Address::decode(&b[4..])?,
            since: be(&b[4 + Address::LEN..]),
        })
    }
}

/// Bytes of an item's number in a fetch and in a reply's nonce.
pub const NUMBER_LEN: usize = 3;
/// The largest number an item takes.
pub const MAX_NUMBER: u32 = (1 << (8 * NUMBER_LEN)) - 1;

fn secretbox(key: &[u8; KEY_LEN]) -> XSalsa20Poly1305 {
    XSalsa20Poly1305::new(key.into())
}

/// The most replies a round has, so the most places a requester tries:
/// every item of the board's ring, 16 pages of 16 (`bark-image` layout,
/// `MAILBOX`). A ring that held more would send the newest this many.
pub const MAX_REPLIES: u32 = 256;

/// A reply's nonce: the fetch's `sent` (the round), the reply's place in
/// it, then zeros. Never sent; unique under the mailbox key as long as
/// rounds rise, and apart from the phones' random fetch nonces. Place 0
/// says nothing is new; an item's place is its number less the round's
/// first number, plus one, so the same item has the same place whenever
/// it is sent again in its round.
pub fn reply_nonce(round: u32, place: u32) -> [u8; NONCE_LEN] {
    let mut n = [0u8; NONCE_LEN];
    n[..4].copy_from_slice(&round.to_be_bytes());
    n[4..4 + NUMBER_LEN].copy_from_slice(&place.to_be_bytes()[1..]);
    n
}

/// A reply (form 3) carrying item `number` at `place` in the round, sealed
/// under the mailbox `key` for the fetch of `round`, into `out`; its
/// length.
pub fn seal_reply(
    key: &[u8; KEY_LEN],
    round: u32,
    place: u32,
    number: u32,
    item: &[u8],
    out: &mut [u8],
) -> Result<usize, Error> {
    if item.len() > MAX_APP_PAYLOAD || number > MAX_NUMBER || !(1..=MAX_REPLIES).contains(&place) {
        return Err(Error::TooLong);
    }
    let mut plain = [0u8; NUMBER_LEN + MAX_APP_PAYLOAD];
    plain[..NUMBER_LEN].copy_from_slice(&number.to_be_bytes()[1..]);
    plain[NUMBER_LEN..NUMBER_LEN + item.len()].copy_from_slice(item);
    seal_into(
        &secretbox(key),
        &reply_nonce(round, place),
        &[Form::Reply.format()],
        &plain[..NUMBER_LEN + item.len()],
        out,
    )
}

/// The reply that says the mailbox holds nothing the fetch of `round`
/// asks for: form 3 sealing nothing at place 0, [`NOTHING_NEW_LEN`] bytes.
pub fn seal_nothing_new(key: &[u8; KEY_LEN], round: u32) -> [u8; NOTHING_NEW_LEN] {
    let mut out = [0u8; NOTHING_NEW_LEN];
    // Nothing to seal cannot be too long.
    let _ = seal_into(
        &secretbox(key),
        &reply_nonce(round, 0),
        &[Form::Reply.format()],
        &[],
        &mut out,
    );
    out
}

/// What a reply says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opened {
    /// Item `number`, its `len` bytes written out.
    Item {
        /// The item's number in the ring.
        number: u32,
        /// Its place in the round: the round's first item is
        /// `number - place + 1`, and the ring held none after the fetch's
        /// `since` below it.
        place: u32,
        /// Its length.
        len: usize,
    },
    /// The mailbox holds nothing the fetch asked for.
    NothingNew,
}

/// A reply to the fetch of `round`, the item into `out`; [`Error::Open`]
/// if it is not one of that round's. The length tells nothing new from an
/// item, and an item's place is tried from 1 to [`MAX_REPLIES`].
pub fn open_reply(
    key: &[u8; KEY_LEN],
    round: u32,
    payload: &[u8],
    out: &mut [u8],
) -> Result<Opened, Error> {
    if Form::of(payload)? != Form::Reply || payload.len() > MAX_PAYLOAD {
        return Err(Error::Malformed);
    }
    let cipher = secretbox(key);
    if payload.len() == NOTHING_NEW_LEN {
        open_into(&cipher, &reply_nonce(round, 0), &payload[1..], &mut [])?;
        return Ok(Opened::NothingNew);
    }
    let len = payload
        .len()
        .checked_sub(REPLY_OVERHEAD)
        .ok_or(Error::Malformed)?;
    if out.len() < len {
        return Err(Error::TooLong);
    }
    let mut plain = [0u8; MAX_PAYLOAD];
    for place in 1..=MAX_REPLIES {
        if open_into(
            &cipher,
            &reply_nonce(round, place),
            &payload[1..],
            &mut plain,
        )
        .is_ok()
        {
            out[..len].copy_from_slice(&plain[NUMBER_LEN..NUMBER_LEN + len]);
            return Ok(Opened::Item {
                number: be(&plain[..NUMBER_LEN]),
                place,
                len,
            });
        }
    }
    Err(Error::Open)
}

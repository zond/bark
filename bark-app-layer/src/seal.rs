//! The payload's four forms, their format byte and the recipient tag
//! (spec "The payload").

use blake2::digest::consts::{U16, U24};
use blake2::digest::Mac;
use blake2::{Blake2b, Blake2bMac, Digest};
use crypto_box::aead::generic_array::GenericArray;
use crypto_box::aead::AeadCore;
use crypto_box::aead::{AeadInPlace, KeyInit};
use crypto_box::{PublicKey, SalsaBox, SecretKey};
use crypto_secretbox::XSalsa20Poly1305;

use crate::keys::KEY_LEN;
use crate::{be, Address, Error, Identity, MAX_APP_PAYLOAD, MAX_PAYLOAD, VERSION};
use bark_proto::NodeId;

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

/// Where the recipient tag sits: after the format byte.
pub const RECIPIENT_AT: usize = 1;
/// Bytes before a form's own: the format byte and the recipient tag.
pub const HEAD_LEN: usize = RECIPIENT_AT + 1;

/// The recipient tag of `identity` at the node `node` (spec "The payload").
pub fn recipient(identity: &Identity, node: NodeId) -> u8 {
    let mut mac =
        <Blake2bMac<U16> as Mac>::new_from_slice(&identity.0).expect("a 32-byte key is BLAKE2b's");
    mac.update(&node.0);
    mac.finalize().into_bytes()[0]
}

/// The recipient tag of `payload`, the byte after its format.
pub fn recipient_of(payload: &[u8]) -> Option<u8> {
    payload.get(RECIPIENT_AT).copied()
}

/// Bytes of a nonce.
pub const NONCE_LEN: usize = 24;
/// Bytes of a tag.
pub const TAG_LEN: usize = 16;

/// Bytes a boxed payload adds to its plaintext.
pub const BOX_OVERHEAD: usize = HEAD_LEN + NONCE_LEN + TAG_LEN;
/// Bytes an introduction adds to its plaintext.
pub const INTRO_OVERHEAD: usize = HEAD_LEN + KEY_LEN + TAG_LEN;
/// Bytes of a reply's count of the items still unsent after its batch.
pub const REMAINING_LEN: usize = 2;
/// Bytes a reply adds to the item it carries: the format byte, the
/// recipient tag, the tag, the item's number and the items remaining.
pub const REPLY_OVERHEAD: usize = HEAD_LEN + TAG_LEN + NUMBER_LEN + REMAINING_LEN;
/// Bytes of the reply that says nothing is new.
pub const NOTHING_NEW_LEN: usize = HEAD_LEN + TAG_LEN;

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
    n.copy_from_slice(
        payload
            .get(HEAD_LEN..HEAD_LEN + NONCE_LEN)
            .ok_or(Error::Malformed)?,
    );
    Ok(n)
}

/// A form's head: its format byte, the recipient tag and `rest`.
fn head<const N: usize>(form: Form, recipient: u8, rest: &[u8]) -> [u8; N] {
    let mut head = [0u8; N];
    head[0] = form.format();
    head[RECIPIENT_AT] = recipient;
    head[HEAD_LEN..].copy_from_slice(rest);
    head
}

/// A boxed payload (form 0) of `plain` under a pair's key, for the
/// `recipient` tag, with a random `nonce`, into `out`; its length.
pub fn seal_box(
    pair: &SalsaBox,
    recipient: u8,
    nonce: &[u8; NONCE_LEN],
    plain: &[u8],
    out: &mut [u8],
) -> Result<usize, Error> {
    if plain.len() > MAX_BOX_PLAIN {
        return Err(Error::TooLong);
    }
    let head: [u8; HEAD_LEN + NONCE_LEN] = head(Form::Boxed, recipient, nonce);
    seal_into(pair, nonce, &head, plain, out)
}

/// The plaintext of a boxed payload under a pair's key, into `out`; its
/// length, or [`Error::Open`] if the pair is not the one.
pub fn open_box(pair: &SalsaBox, payload: &[u8], out: &mut [u8]) -> Result<usize, Error> {
    if Form::of(payload)? != Form::Boxed || payload.len() > MAX_PAYLOAD {
        return Err(Error::Malformed);
    }
    open_into(
        pair,
        &nonce_at(payload)?,
        &payload[HEAD_LEN + NONCE_LEN..],
        out,
    )
}

/// `crypto_box_seal`'s nonce: BLAKE2b-192 of the ephemeral key and the
/// receiver's.
fn seal_nonce(ephemeral: &PublicKey, to: &PublicKey) -> [u8; NONCE_LEN] {
    let mut h = Blake2b::<U24>::new();
    h.update(ephemeral.as_bytes());
    h.update(to.as_bytes());
    h.finalize().into()
}

/// An introduction (form 1) of `plain` to `to`, for the `recipient` tag,
/// sealed with a fresh `ephemeral` key, into `out`; its length.
pub fn seal_intro(
    to: &PublicKey,
    recipient: u8,
    ephemeral: &SecretKey,
    plain: &[u8],
    out: &mut [u8],
) -> Result<usize, Error> {
    if plain.len() > MAX_INTRO_PLAIN {
        return Err(Error::TooLong);
    }
    let epk = ephemeral.public_key();
    let head: [u8; HEAD_LEN + KEY_LEN] = head(Form::Introduction, recipient, epk.as_bytes());
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
    e.copy_from_slice(
        payload
            .get(HEAD_LEN..HEAD_LEN + KEY_LEN)
            .ok_or(Error::Malformed)?,
    );
    let epk = PublicKey::from(e);
    let nonce = seal_nonce(&epk, &me.public_key());
    open_into(
        &SalsaBox::new(&epk, me),
        &nonce,
        &payload[HEAD_LEN + KEY_LEN..],
        out,
    )
}

/// A fetch (spec "Mailbox"): the requester asks its home node for the
/// newest batch of the items it has not sent, to be sent to `address`
/// for the `recipient` tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fetch {
    /// The requester's clock, as a text's `sent`; a fetch is taken only
    /// if later than the last one taken.
    pub sent: u32,
    /// The requester's current address.
    pub address: Address,
    /// The requester's recipient tag at that address's node, which the
    /// home node cannot make, as it does not know the identity.
    pub recipient: u8,
}

impl Fetch {
    /// Bytes of its plaintext.
    pub const PLAIN_LEN: usize = 4 + Address::LEN + 1;
    /// Bytes of its payload.
    pub const LEN: usize = BOX_OVERHEAD + Self::PLAIN_LEN;

    fn plain(&self) -> [u8; Self::PLAIN_LEN] {
        let mut b = [0u8; Self::PLAIN_LEN];
        b[..4].copy_from_slice(&self.sent.to_be_bytes());
        b[4..4 + Address::LEN].copy_from_slice(&self.address.encode());
        b[4 + Address::LEN] = self.recipient;
        b
    }

    /// The fetch as a payload (form 2) under the mailbox `key`, with a
    /// random `nonce`, for `home`, the requester's recipient tag at its
    /// home node.
    pub fn seal(
        &self,
        key: &[u8; KEY_LEN],
        home: u8,
        nonce: &[u8; NONCE_LEN],
    ) -> Result<[u8; Self::LEN], Error> {
        let head: [u8; HEAD_LEN + NONCE_LEN] = head(Form::Fetch, home, nonce);
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
            &payload[HEAD_LEN + NONCE_LEN..],
            &mut b,
        )?;
        Ok(Fetch {
            sent: be(&b[..4]),
            address: Address::decode(&b[4..])?,
            recipient: b[4 + Address::LEN],
        })
    }
}

/// Bytes of an item's number in a reply.
pub const NUMBER_LEN: usize = 3;
/// The largest number an item takes.
pub const MAX_NUMBER: u32 = (1 << (8 * NUMBER_LEN)) - 1;

fn secretbox(key: &[u8; KEY_LEN]) -> XSalsa20Poly1305 {
    XSalsa20Poly1305::new(key.into())
}

/// The most items a fetch is answered with, so the most places a requester
/// tries: under the 33 full frames a relay's full bucket sends with the
/// reply reserve left, so a batch never waits for a refill
/// (spec "Send queue", "Mailbox").
pub const MAX_REPLIES: u32 = 32;

/// A reply's nonce: the round (the fetch's `sent`), the place, then zeros.
/// Never sent; unique under the mailbox key while rounds rise. Place 0
/// says nothing is new; an item keeps its place when its round sends it
/// again.
pub fn reply_nonce(round: u32, place: u32) -> [u8; NONCE_LEN] {
    let mut n = [0u8; NONCE_LEN];
    n[..4].copy_from_slice(&round.to_be_bytes());
    n[4..4 + NUMBER_LEN].copy_from_slice(&place.to_be_bytes()[1..]);
    n
}

/// A reply (form 3) carrying item `number` at `place` in the round, with
/// `remaining` items still unsent after the round's batch, sealed under
/// the mailbox `key` for the fetch of `round`, for its `recipient` tag,
/// into `out`; its length.
#[allow(clippy::too_many_arguments)]
pub fn seal_reply(
    key: &[u8; KEY_LEN],
    recipient: u8,
    round: u32,
    place: u32,
    number: u32,
    remaining: u16,
    item: &[u8],
    out: &mut [u8],
) -> Result<usize, Error> {
    if item.len() > MAX_APP_PAYLOAD || number > MAX_NUMBER || !(1..=MAX_REPLIES).contains(&place) {
        return Err(Error::TooLong);
    }
    const HEAD: usize = NUMBER_LEN + REMAINING_LEN;
    let mut plain = [0u8; HEAD + MAX_APP_PAYLOAD];
    plain[..NUMBER_LEN].copy_from_slice(&number.to_be_bytes()[1..]);
    plain[NUMBER_LEN..HEAD].copy_from_slice(&remaining.to_be_bytes());
    plain[HEAD..HEAD + item.len()].copy_from_slice(item);
    let head: [u8; HEAD_LEN] = head(Form::Reply, recipient, &[]);
    seal_into(
        &secretbox(key),
        &reply_nonce(round, place),
        &head,
        &plain[..HEAD + item.len()],
        out,
    )
}

/// The reply that says the mailbox holds nothing the fetch of `round`
/// asks for, for its `recipient` tag: form 3 sealing nothing at place 0,
/// [`NOTHING_NEW_LEN`] bytes.
pub fn seal_nothing_new(key: &[u8; KEY_LEN], recipient: u8, round: u32) -> [u8; NOTHING_NEW_LEN] {
    let mut out = [0u8; NOTHING_NEW_LEN];
    let head: [u8; HEAD_LEN] = head(Form::Reply, recipient, &[]);
    // Nothing to seal cannot be too long.
    let _ = seal_into(
        &secretbox(key),
        &reply_nonce(round, 0),
        &head,
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
        /// Its place in the round.
        place: u32,
        /// Items still unsent after the round's batch, when the fetch was
        /// taken.
        remaining: u16,
        /// Its length.
        len: usize,
    },
    /// The mailbox holds no item it has not sent, so none remains.
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
        open_into(
            &cipher,
            &reply_nonce(round, 0),
            &payload[HEAD_LEN..],
            &mut [],
        )?;
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
            &payload[HEAD_LEN..],
            &mut plain,
        )
        .is_ok()
        {
            const HEAD: usize = NUMBER_LEN + REMAINING_LEN;
            out[..len].copy_from_slice(&plain[HEAD..HEAD + len]);
            return Ok(Opened::Item {
                number: be(&plain[..NUMBER_LEN]),
                place,
                remaining: u16::from_be_bytes([plain[NUMBER_LEN], plain[NUMBER_LEN + 1]]),
                len,
            });
        }
    }
    Err(Error::Open)
}

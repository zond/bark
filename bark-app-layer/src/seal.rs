//! The payload's forms, their format byte and the recipient tag
//! (spec "The payload").

use blake2::digest::consts::{U10, U16, U24};
use blake2::digest::Mac;
use blake2::{Blake2b, Blake2bMac, Digest};
use crypto_box::aead::generic_array::GenericArray;
use crypto_box::aead::AeadCore;
use crypto_box::aead::{AeadInPlace, KeyInit};
use crypto_box::{PublicKey, SalsaBox, SecretKey};
use crypto_secretbox::XSalsa20Poly1305;
use curve25519_dalek::montgomery::MontgomeryPoint;
use subtle::ConstantTimeEq;

use crate::keys::{kdf, KEY_LEN};
use crate::{be, Address, Error, Identity, MAX_APP_PAYLOAD, MAX_PAYLOAD, VERSION};
use bark_proto::NodeId;

/// The forms, the format byte's low four bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    /// To a contact, under the keys of the pair's `crypto_box_beforenm`.
    Boxed = 0,
    /// To someone who may not know the sender, a sealed box.
    Introduction = 1,
    /// To one's own mailbox, under the mailbox key's keys.
    Fetch = 2,
    /// From the mailbox, first in a round: what the round carries and
    /// what stays, `crypto_secretbox` under the reply key.
    Summary = 3,
    /// From the mailbox, a boxed payload it kept.
    BoxedItem = 4,
    /// From the mailbox, an introduction it kept.
    IntroductionItem = 5,
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
            3 => Ok(Form::Summary),
            4 => Ok(Form::BoxedItem),
            5 => Ok(Form::IntroductionItem),
            _ => Err(Error::Malformed),
        }
    }

    /// Whether the mailbox sent this form: a summary or an item.
    pub fn is_reply(self) -> bool {
        matches!(
            self,
            Form::Summary | Form::BoxedItem | Form::IntroductionItem
        )
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

/// Bytes of a nonce of XSalsa20, the stream every form uses.
pub const NONCE_LEN: usize = 24;
/// Bytes of a tag.
pub const TAG_LEN: usize = 16;
/// Bytes of `siv`, the one value that is a boxed payload's or a fetch's
/// nonce and its proof.
pub const SIV_LEN: usize = 16;

/// Bytes a boxed payload or a fetch adds to its plaintext.
pub const BOX_OVERHEAD: usize = HEAD_LEN + SIV_LEN;
/// Bytes an introduction adds to its plaintext.
pub const INTRO_OVERHEAD: usize = HEAD_LEN + KEY_LEN + TAG_LEN;

/// The longest plaintext of a boxed payload.
pub const MAX_BOX_PLAIN: usize = MAX_APP_PAYLOAD - BOX_OVERHEAD;
/// The longest plaintext of an introduction.
pub const MAX_INTRO_PLAIN: usize = MAX_APP_PAYLOAD - INTRO_OVERHEAD;

/// The two keys a boxed payload or a fetch is made with, both derived
/// from one shared key: `siv` is hashed under one and the stream runs
/// under the other (spec "The payload").
#[derive(Clone)]
pub struct SivKeys {
    pub(crate) mac: [u8; KEY_LEN],
    pub(crate) enc: [u8; KEY_LEN],
}

impl SivKeys {
    /// The keys derived from `shared`: a pair's shared key, or the
    /// mailbox key.
    pub fn of(shared: &[u8; KEY_LEN]) -> SivKeys {
        SivKeys {
            mac: kdf(shared, 1, b"bark-mac"),
            enc: kdf(shared, 2, b"bark-enc"),
        }
    }

    /// The hash keyed with the MAC key, as `crypto_generichash` makes it
    /// to 16 bytes, of the format byte, `bound` and the plaintext: `siv`.
    fn siv(&self, format: u8, bound: &[u8], plain: &[u8]) -> [u8; SIV_LEN] {
        let mut mac = <Blake2bMac<U16> as Mac>::new_from_slice(&self.mac)
            .expect("a 32-byte key is BLAKE2b's");
        mac.update(&[format]);
        mac.update(bound);
        mac.update(plain);
        mac.finalize().into_bytes().into()
    }

    /// `siv` and zeros as the stream's nonce.
    fn nonce(siv: &[u8]) -> [u8; NONCE_LEN] {
        let mut n = [0u8; NONCE_LEN];
        n[..SIV_LEN].copy_from_slice(siv);
        n
    }
}

/// The shared key of two identities, `crypto_box_beforenm`: X25519 of
/// the two keys, then HSalsa20 of it over 16 zero bytes. A public key
/// of small order gives the zero point, which libsodium refuses.
pub(crate) fn beforenm(their: &PublicKey, mine: &SecretKey) -> Result<[u8; KEY_LEN], Error> {
    let shared = MontgomeryPoint(*their.as_bytes()).mul_clamped(mine.to_bytes());
    if bool::from(shared.as_bytes().ct_eq(&[0u8; KEY_LEN])) {
        return Err(Error::Malformed);
    }
    Ok(salsa20::hsalsa::<U10>(shared.as_bytes().into(), &GenericArray::default()).into())
}

/// The keys of two identities' boxed payloads, made once per contact
/// from `crypto_box_beforenm`: the receiver tries each contact's. Both
/// directions of a pair have the same keys.
pub fn pair(their: &PublicKey, mine: &SecretKey) -> Result<SivKeys, Error> {
    beforenm(their, mine).map(|k| SivKeys::of(&k))
}

/// XSalsa20 over `buf` under `key` with `nonce`, as
/// `crypto_secretbox_detached` encrypts, its tag discarded: the keystream
/// from its 33rd byte, the first 32 being the tag's key. Its own inverse.
pub fn stream(key: &[u8; KEY_LEN], nonce: &[u8; NONCE_LEN], buf: &mut [u8]) {
    // It fails only for a message longer than XSalsa20's counter reaches,
    // and a payload is far shorter.
    let _ = secretbox(key).encrypt_in_place_detached(GenericArray::from_slice(nonce), &[], buf);
}

/// `plain` as a payload of `form` under `keys`, for the `recipient` tag,
/// into `out`: the format byte, the tag, `siv`, the ciphertext; its length.
/// `bound` is hashed into `siv` and not sent: what the receiver knows
/// itself, so that the payload opens there alone.
fn siv_seal(
    keys: &SivKeys,
    form: Form,
    recipient: u8,
    bound: &[u8],
    plain: &[u8],
    out: &mut [u8],
) -> Result<usize, Error> {
    let at = HEAD_LEN + SIV_LEN;
    let len = at + plain.len();
    if out.len() < len {
        return Err(Error::TooLong);
    }
    let siv = keys.siv(form.format(), bound, plain);
    out[0] = form.format();
    out[RECIPIENT_AT] = recipient;
    out[HEAD_LEN..at].copy_from_slice(&siv);
    out[at..len].copy_from_slice(plain);
    stream(&keys.enc, &SivKeys::nonce(&siv), &mut out[at..len]);
    Ok(len)
}

/// The plaintext of `payload` under `keys`, sealed with `bound`, into
/// `out`; its length. The plaintext is taken only once `siv` checks, and
/// is wiped if it does not.
fn siv_open(keys: &SivKeys, bound: &[u8], payload: &[u8], out: &mut [u8]) -> Result<usize, Error> {
    let at = HEAD_LEN + SIV_LEN;
    let len = payload.len().checked_sub(at).ok_or(Error::Malformed)?;
    if out.len() < len {
        return Err(Error::TooLong);
    }
    let siv = &payload[HEAD_LEN..at];
    let plain = &mut out[..len];
    plain.copy_from_slice(&payload[at..]);
    stream(&keys.enc, &SivKeys::nonce(siv), plain);
    if !bool::from(keys.siv(payload[0], bound, plain).ct_eq(siv)) {
        plain.fill(0);
        return Err(Error::Open);
    }
    Ok(len)
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

/// A form's head: its format byte, the recipient tag and `rest`.
fn head<const N: usize>(form: Form, recipient: u8, rest: &[u8]) -> [u8; N] {
    let mut head = [0u8; N];
    head[0] = form.format();
    head[RECIPIENT_AT] = recipient;
    head[HEAD_LEN..].copy_from_slice(rest);
    head
}

/// A boxed payload (form 0) of `plain` under a pair's keys, for the
/// `recipient` tag, into `out`; its length.
pub fn seal_box(
    pair: &SivKeys,
    recipient: u8,
    plain: &[u8],
    out: &mut [u8],
) -> Result<usize, Error> {
    if plain.len() > MAX_BOX_PLAIN {
        return Err(Error::TooLong);
    }
    siv_seal(pair, Form::Boxed, recipient, &[], plain, out)
}

/// The plaintext of a boxed payload under a pair's keys, into `out`; its
/// length, or [`Error::Open`] if the pair is not the one.
pub fn open_box(pair: &SivKeys, payload: &[u8], out: &mut [u8]) -> Result<usize, Error> {
    if Form::of(payload)? != Form::Boxed || payload.len() > MAX_PAYLOAD {
        return Err(Error::Malformed);
    }
    siv_open(pair, &[], payload, out)
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

    /// The fetch as a payload (form 2) under the keys of the mailbox
    /// `key`, for the home node `home`, with `tag`, the requester's
    /// recipient tag there. The home's ID is hashed into `siv`, so that
    /// another node holding the same key, an earlier home, cannot take it
    /// and answer the round a second time under the same reply nonces.
    pub fn seal(&self, key: &[u8; KEY_LEN], home: NodeId, tag: u8) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        // A plaintext of its own length fits.
        let _ = siv_seal(
            &SivKeys::of(key),
            Form::Fetch,
            tag,
            &home.0,
            &self.plain(),
            &mut out,
        );
        out
    }

    /// The fetch in `payload`, if it is one under the mailbox `key` for
    /// the node `me`.
    pub fn open(key: &[u8; KEY_LEN], me: NodeId, payload: &[u8]) -> Result<Fetch, Error> {
        if Form::of(payload)? != Form::Fetch || payload.len() != Self::LEN {
            return Err(Error::Malformed);
        }
        let mut b = [0u8; Self::PLAIN_LEN];
        siv_open(&SivKeys::of(key), &me.0, payload, &mut b)?;
        Ok(Fetch {
            sent: be(&b[..4]),
            address: Address::decode(&b[4..])?,
            recipient: b[4 + Address::LEN],
        })
    }
}

fn secretbox(key: &[u8; KEY_LEN]) -> XSalsa20Poly1305 {
    XSalsa20Poly1305::new(key.into())
}

/// The most items a fetch is answered with: under the 33 full frames a
/// relay's full bucket sends with the reply reserve left, so a batch never
/// waits for a refill (spec "Send queue", "Mailbox").
pub const MAX_REPLIES: u8 = 32;

/// Bytes of a reply's place in its round, sent in the clear before an
/// item.
pub const PLACE_LEN: usize = 1;

/// Bytes of a summary's count of the round's items.
pub const ITEMS_LEN: usize = 1;
/// Bytes of a summary's count of the items still unsent after the round.
pub const REMAINING_LEN: usize = 2;

/// Bytes of the summary: the format byte, the recipient tag, the tag, the
/// round's items and the items remaining.
pub const SUMMARY_LEN: usize = HEAD_LEN + TAG_LEN + ITEMS_LEN + REMAINING_LEN;

/// Bytes a reply adds to the item it carries: its own format byte and
/// recipient tag stand for the item's, and the place is added.
pub const REPLY_OVERHEAD: usize = PLACE_LEN;

/// The key a mailbox's replies are under, the summary's and the items'
/// stream, derived from the mailbox key as the fetch's two are, so that
/// the mailbox key itself encrypts nothing.
pub fn reply_key(mailbox: &[u8; KEY_LEN]) -> [u8; KEY_LEN] {
    kdf(mailbox, 3, b"bark-rpl")
}

/// A reply's nonce: the round (the fetch's `sent`), the place, then zeros.
/// Never sent; unique under the reply key while rounds rise. Place 0 is
/// the summary's; an item keeps its place when its round sends it again.
pub fn reply_nonce(round: u32, place: u8) -> [u8; NONCE_LEN] {
    let mut n = [0u8; NONCE_LEN];
    n[..4].copy_from_slice(&round.to_be_bytes());
    n[4] = place;
    n
}

/// Whether the mailbox can send `item` on: a boxed payload or an
/// introduction of this version that fits a reply.
pub fn forwardable(item: &[u8]) -> bool {
    matches!(Form::of(item), Ok(Form::Boxed | Form::Introduction))
        && (HEAD_LEN..=MAX_APP_PAYLOAD).contains(&item.len())
}

/// The summary of the fetch of `round`, for its `recipient` tag: how many
/// items the round carries and how many of the person's stay unsent
/// after it, sealed under the mailbox `key` at place 0. Zero items says
/// nothing is new.
pub fn seal_summary(
    key: &[u8; KEY_LEN],
    recipient: u8,
    round: u32,
    items: u8,
    remaining: u16,
) -> [u8; SUMMARY_LEN] {
    let mut out = [0u8; SUMMARY_LEN];
    let mut plain = [0u8; ITEMS_LEN + REMAINING_LEN];
    plain[0] = items;
    plain[ITEMS_LEN..].copy_from_slice(&remaining.to_be_bytes());
    let head: [u8; HEAD_LEN] = head(Form::Summary, recipient, &[]);
    // Three bytes fit their own length.
    let _ = seal_into(
        &secretbox(&reply_key(key)),
        &reply_nonce(round, 0),
        &head,
        &plain,
        &mut out,
    );
    out
}

/// A summary of the fetch of `round`, if it is one under `key`: the
/// round's items and the items remaining after it.
pub fn open_summary(key: &[u8; KEY_LEN], round: u32, payload: &[u8]) -> Result<(u8, u16), Error> {
    if Form::of(payload)? != Form::Summary || payload.len() != SUMMARY_LEN {
        return Err(Error::Malformed);
    }
    let mut plain = [0u8; ITEMS_LEN + REMAINING_LEN];
    open_into(
        &secretbox(&reply_key(key)),
        &reply_nonce(round, 0),
        &payload[HEAD_LEN..],
        &mut plain,
    )?;
    Ok((
        plain[0],
        u16::from_be_bytes([plain[ITEMS_LEN], plain[ITEMS_LEN + 1]]),
    ))
}

/// A reply carrying `item` at `place` in the round of `round`, for the
/// fetch's `recipient` tag, into `out`; its length. Its form says the
/// item's, the place goes in the clear, and the item without its format
/// byte and recipient tag goes under the reply key's stream alone:
/// the item proves itself.
pub fn seal_item(
    key: &[u8; KEY_LEN],
    recipient: u8,
    round: u32,
    place: u8,
    item: &[u8],
    out: &mut [u8],
) -> Result<usize, Error> {
    let form = match Form::of(item) {
        Ok(Form::Boxed) => Form::BoxedItem,
        Ok(Form::Introduction) => Form::IntroductionItem,
        _ => return Err(Error::Malformed),
    };
    if !forwardable(item) || !(1..=MAX_REPLIES).contains(&place) {
        return Err(Error::TooLong);
    }
    let at = HEAD_LEN + PLACE_LEN;
    let len = at + item.len() - HEAD_LEN;
    if out.len() < len {
        return Err(Error::TooLong);
    }
    out[0] = form.format();
    out[RECIPIENT_AT] = recipient;
    out[HEAD_LEN] = place;
    out[at..len].copy_from_slice(&item[HEAD_LEN..]);
    stream(
        &reply_key(key),
        &reply_nonce(round, place),
        &mut out[at..len],
    );
    Ok(len)
}

/// The item a reply carries, as the mailbox's round of `round` sent it,
/// into `out`: its format byte from the reply's form and the reply's
/// recipient tag in place of the item's, which no receiver checks; its
/// length. Nothing here tells one round from another: the item does, by
/// opening under the receiver's keys or not.
pub fn open_item(
    key: &[u8; KEY_LEN],
    round: u32,
    payload: &[u8],
    out: &mut [u8],
) -> Result<usize, Error> {
    let form = match Form::of(payload)? {
        Form::BoxedItem => Form::Boxed,
        Form::IntroductionItem => Form::Introduction,
        _ => return Err(Error::Malformed),
    };
    let at = HEAD_LEN + PLACE_LEN;
    if payload.len() < at || payload.len() > MAX_PAYLOAD {
        return Err(Error::Malformed);
    }
    let place = payload[HEAD_LEN];
    if !(1..=MAX_REPLIES).contains(&place) {
        return Err(Error::Malformed);
    }
    let len = payload.len() - REPLY_OVERHEAD;
    if out.len() < len {
        return Err(Error::TooLong);
    }
    out[0] = form.format();
    out[RECIPIENT_AT] = payload[RECIPIENT_AT];
    out[HEAD_LEN..len].copy_from_slice(&payload[at..]);
    stream(
        &reply_key(key),
        &reply_nonce(round, place),
        &mut out[HEAD_LEN..len],
    );
    Ok(len)
}

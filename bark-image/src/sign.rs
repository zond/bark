//! The signatures in a header, and the check the running firmware makes
//! of them before it stages an image (`bark-fw/README.md`, "Updates").
//!
//! A signature covers the header's first [`SIGNED_LEN`] bytes: the magic,
//! the format, the version, the length and the image's SHA-256. The hash
//! stands for the image, which the firmware hashes anyway, so the
//! signature is checked against 48 bytes and not the image's 388 KB,
//! and before a byte of the image has come.

#[cfg(feature = "verify")]
use ed25519_compact::{PublicKey, Signature};

use crate::HEADER_LEN;

/// The header's bytes a signature covers.
pub const SIGNED_LEN: usize = 48;

/// Bytes of a public key (RFC 8032, 5.1.5).
pub const KEY_LEN: usize = 32;

/// Bytes of a signature (RFC 8032, 5.1.6).
pub const SIGNATURE_LEN: usize = 64;

/// Bytes of a slot: the byte naming the key, then the signature.
pub const SLOT_LEN: usize = 1 + SIGNATURE_LEN;

/// Slots in a header, and keys a firmware holds: one to sign with, and
/// room to bring in the next before the last is retired.
pub const SLOTS: usize = 3;

/// The byte of an empty slot, or of no key.
pub const NONE: u8 = 0;

const _: () = assert!(SIGNED_LEN + SLOTS * SLOT_LEN <= HEADER_LEN);

/// A public key and the byte that names it in a slot. [`NONE`] marks no
/// key. Laid out as 33 bytes, the byte first, so that the build scripts
/// read a firmware's keys from its image.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    /// The byte naming the key, 1 to 255.
    pub id: u8,
    /// The Ed25519 public key.
    pub public: [u8; KEY_LEN],
}

impl Key {
    /// No key.
    pub const NONE: Key = Key {
        id: NONE,
        public: [0; KEY_LEN],
    };
}

/// Why a header's signatures were refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignatureError {
    /// No slot holds a signature.
    Unsigned,
    /// No slot names a key this firmware holds.
    UnknownKey,
    /// A slot names a key this firmware holds, and its signature does not
    /// verify under it.
    Bad,
}

/// The id of the first key of `keys` under which a signature in `header`
/// verifies.
#[cfg(feature = "verify")]
pub fn verify(header: &[u8; HEADER_LEN], keys: &[Key; SLOTS]) -> Result<u8, SignatureError> {
    let message = &header[..SIGNED_LEN];
    let mut found = SignatureError::Unsigned;
    for slot in header[SIGNED_LEN..]
        .as_chunks::<SLOT_LEN>()
        .0
        .iter()
        .take(SLOTS)
    {
        let id = slot[0];
        if id == NONE {
            continue;
        }
        let Some(key) = keys.iter().find(|k| k.id == id) else {
            if found == SignatureError::Unsigned {
                found = SignatureError::UnknownKey;
            }
            continue;
        };
        found = SignatureError::Bad;
        let mut signature = [0u8; SIGNATURE_LEN];
        signature.copy_from_slice(&slot[1..]);
        // Refuses a weak key and a signature not in its one canonical
        // form (RFC 8032, 5.1.7), which no honest signer makes.
        if PublicKey::new(key.public)
            .verify(message, &Signature::new(signature))
            .is_ok()
        {
            return Ok(id);
        }
    }
    Err(found)
}

#[cfg(all(test, feature = "verify"))]
mod tests {
    use super::*;
    use crate::{Header, HASH_LEN};
    use ed25519_compact::{KeyPair, Seed};

    fn signer(n: u8) -> KeyPair {
        KeyPair::from_seed(Seed::new([n; 32]))
    }

    fn key(id: u8, n: u8) -> Key {
        Key {
            id,
            public: *signer(n).pk,
        }
    }

    fn header() -> [u8; HEADER_LEN] {
        Header {
            version: 7,
            length: 167_996,
            hash: [0x5a; HASH_LEN],
        }
        .encode()
    }

    fn sign(header: &mut [u8; HEADER_LEN], slot: usize, id: u8, n: u8) {
        let signature = *signer(n).sk.sign(&header[..SIGNED_LEN], None);
        let at = SIGNED_LEN + slot * SLOT_LEN;
        header[at] = id;
        header[at + 1..at + SLOT_LEN].copy_from_slice(&signature);
    }

    fn keys(list: &[Key]) -> [Key; SLOTS] {
        let mut k = [Key::NONE; SLOTS];
        k[..list.len()].copy_from_slice(list);
        k
    }

    #[test]
    fn a_signature_by_a_held_key_verifies() {
        let mut h = header();
        sign(&mut h, 0, 1, 1);
        assert_eq!(verify(&h, &keys(&[key(1, 1)])), Ok(1));
    }

    #[test]
    fn any_slot_and_any_held_key_will_do() {
        let mut h = header();
        sign(&mut h, 0, 9, 9);
        sign(&mut h, 2, 2, 2);
        assert_eq!(verify(&h, &keys(&[key(1, 1), key(2, 2)])), Ok(2));
    }

    #[test]
    fn an_unsigned_header_is_refused() {
        assert_eq!(
            verify(&header(), &keys(&[key(1, 1)])),
            Err(SignatureError::Unsigned)
        );
    }

    #[test]
    fn a_signature_by_a_key_not_held_is_refused() {
        let mut h = header();
        sign(&mut h, 0, 3, 3);
        assert_eq!(
            verify(&h, &keys(&[key(1, 1)])),
            Err(SignatureError::UnknownKey)
        );
    }

    #[test]
    fn a_signature_under_the_wrong_key_is_refused() {
        let mut h = header();
        // Names key 1, made with another.
        sign(&mut h, 0, 1, 2);
        assert_eq!(verify(&h, &keys(&[key(1, 1)])), Err(SignatureError::Bad));
    }

    #[test]
    fn every_signed_byte_counts() {
        let mut signed = header();
        sign(&mut signed, 0, 1, 1);
        for at in 0..SIGNED_LEN {
            let mut h = signed;
            h[at] ^= 1;
            assert_eq!(
                verify(&h, &keys(&[key(1, 1)])),
                Err(SignatureError::Bad),
                "byte {at}"
            );
        }
    }

    /// The bench key's public key, `bark-fw/keys/bench.pub`, and its
    /// signature of [`header`] as `scripts/image.py sign` makes it: the
    /// script and the firmware agree.
    #[test]
    fn a_signature_from_the_script_verifies() {
        let hex = |s: &str, out: &mut [u8]| {
            for (i, b) in out.iter_mut().enumerate() {
                *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap();
            }
        };
        let mut bench = Key::NONE;
        bench.id = 1;
        hex(
            "71bd2f005d6b0bc8ce9c04c9e6cd089f9ccee6f837ca413d20fe85fdc0225313",
            &mut bench.public,
        );
        let mut h = header();
        h[SIGNED_LEN] = 1;
        hex(
            "2440a85feda31b832383d539ed36e67a91cd0b75c57df61a38829278da3d8523\
             5886b3a397da3512671993bc28bdb0780650ae3384681e163ea9550795c90604",
            &mut h[SIGNED_LEN + 1..SIGNED_LEN + SLOT_LEN],
        );
        assert_eq!(verify(&h, &keys(&[bench])), Ok(1));
        h[8] ^= 1;
        assert_eq!(verify(&h, &keys(&[bench])), Err(SignatureError::Bad));
    }

    #[test]
    fn a_key_is_its_byte_then_its_public_key() {
        assert_eq!(core::mem::size_of::<[Key; SLOTS]>(), SLOTS * (1 + KEY_LEN));
    }
}

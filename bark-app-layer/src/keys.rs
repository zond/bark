//! Every key of an identity from one 128-bit seed, and the seed's backup
//! as 12 words (spec "The payload").

use blake2::digest::consts::U32;
use blake2::digest::{Mac, Update, VariableOutput};
use blake2::{Blake2bMac, Blake2bVar};
use crypto_box::SecretKey;
use sha2::{Digest, Sha256, Sha512};

use crate::{Error, Identity};

/// Bytes in a seed: 128 bits, the security of X25519 itself (RFC 7748 §1).
pub const SEED_LEN: usize = 16;

/// Words in the backup: 128 bits of seed and 4 of checksum, 11 bits a word.
pub const PHRASE_WORDS: usize = 12;

/// Bits a word carries: the list has 2¹¹ words.
const WORD_BITS: u32 = 11;

/// The list, one word a line, sorted; fixed forever, since a changed list
/// loses every backup.
const LIST: &str = include_str!("words.txt");

/// The words of the list, in order.
pub fn words() -> impl Iterator<Item = &'static str> {
    LIST.lines()
}

/// Bytes of a symmetric key and of an X25519 key.
pub const KEY_LEN: usize = 32;

/// An identity's keys, all from its seed.
#[derive(Clone)]
pub struct Keys {
    /// The X25519 secret key, as `crypto_box_seed_keypair` makes it.
    pub secret: [u8; KEY_LEN],
    /// The public key: the identity.
    pub identity: Identity,
    /// The mailbox key, which a home node holds (spec "Mailbox").
    pub mailbox: [u8; KEY_LEN],
}

/// `crypto_kdf_derive_from_key`: BLAKE2b keyed with the master key, the
/// subkey's number little-endian as the salt and the context as the
/// personalisation, each padded with zeros to 16 bytes.
fn kdf(master: &[u8; KEY_LEN], id: u64, context: &[u8; 8]) -> [u8; KEY_LEN] {
    let mut salt = [0u8; 16];
    salt[..8].copy_from_slice(&id.to_le_bytes());
    let mut personal = [0u8; 16];
    personal[..8].copy_from_slice(context);
    let mac = Blake2bMac::<U32>::new_with_salt_and_personal(master, &salt, &personal)
        .expect("a 32-byte key and 16-byte salt and personalisation are BLAKE2b's");
    mac.finalize().into_bytes().into()
}

impl Keys {
    /// The keys of `seed`.
    pub fn from_seed(seed: &[u8; SEED_LEN]) -> Keys {
        let mut master = [0u8; KEY_LEN];
        let mut h = Blake2bVar::new(KEY_LEN).expect("32 bytes is a BLAKE2b length");
        h.update(seed);
        h.finalize_variable(&mut master)
            .expect("the buffer is the length asked for");
        // crypto_box_seed_keypair: the secret key is the first half of the
        // seed's SHA-512.
        let digest = Sha512::digest(kdf(&master, 1, b"bark-key"));
        let mut secret = [0u8; KEY_LEN];
        secret.copy_from_slice(&digest[..KEY_LEN]);
        let identity = Identity(*SecretKey::from(secret).public_key().as_bytes());
        Keys {
            secret,
            identity,
            mailbox: kdf(&master, 2, b"bark-mbx"),
        }
    }

    /// The secret key as `crypto_box` takes it.
    pub fn secret_key(&self) -> SecretKey {
        SecretKey::from(self.secret)
    }
}

/// The 4 checksum bits: the first of the seed's SHA-256 (FIPS 180-4).
fn checksum(seed: &[u8; SEED_LEN]) -> u32 {
    u32::from(Sha256::digest(seed)[0] >> 4)
}

/// The phrase's 132 bits: the seed, then the checksum in the high half
/// of a last byte.
const PHRASE_BYTES: usize = SEED_LEN + 1;

/// The 12 words of `seed`: its 128 bits and the checksum's 4, 11 bits a
/// word, most significant first.
pub fn phrase(seed: &[u8; SEED_LEN]) -> [&'static str; PHRASE_WORDS] {
    let mut bits = [0u8; PHRASE_BYTES];
    bits[..SEED_LEN].copy_from_slice(seed);
    bits[SEED_LEN] = (checksum(seed) << 4) as u8;
    let mut out = [""; PHRASE_WORDS];
    for (k, w) in out.iter_mut().enumerate() {
        let i = (0..WORD_BITS).fold(0, |n, j| {
            let b = k as u32 * WORD_BITS + j;
            n << 1 | u32::from(bits[(b / 8) as usize] >> (7 - b % 8) & 1)
        });
        *w = words().nth(i as usize).expect("the list has 2048 words");
    }
    out
}

/// The seed of a phrase: 12 words of the list, whatever their case,
/// whose checksum holds.
pub fn seed_of<'a>(phrase: impl IntoIterator<Item = &'a str>) -> Result<[u8; SEED_LEN], Error> {
    let mut bits = [0u8; PHRASE_BYTES];
    let mut count = 0u32;
    for w in phrase {
        if count as usize == PHRASE_WORDS {
            return Err(Error::Malformed);
        }
        let i = words()
            .position(|x| x.eq_ignore_ascii_case(w))
            .ok_or(Error::Malformed)? as u32;
        for j in 0..WORD_BITS {
            let b = count * WORD_BITS + j;
            if i >> (WORD_BITS - 1 - j) & 1 == 1 {
                bits[(b / 8) as usize] |= 0x80 >> (b % 8);
            }
        }
        count += 1;
    }
    let mut seed = [0u8; SEED_LEN];
    seed.copy_from_slice(&bits[..SEED_LEN]);
    if count as usize != PHRASE_WORDS || u32::from(bits[SEED_LEN] >> 4) != checksum(&seed) {
        return Err(Error::Malformed);
    }
    Ok(seed)
}

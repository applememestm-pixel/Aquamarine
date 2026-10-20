//! Encrypted account backups.
//!
//! The server stores a backup it cannot read. Layout of the blob:
//!
//! ```text
//! "AQBK1" ‖ u32 header_len (BE) ‖ header (JSON) ‖ nonce[12] ‖ ciphertext
//! ciphertext = ChaCha20-Poly1305(DEK, nonce, AAD = "AQBK1", DEFLATE(payload))
//! ```
//!
//! The 32-byte data key (DEK) is random and wrapped twice in the header:
//!
//! * `pw` — under `Argon2id(password, salt, m = 64 MiB, t = 3, p = 1)`;
//! * `rk` — under `HKDF-SHA256(recovery entropy, info = "aquamarine backup recovery v1")`.
//!
//! The recovery key is 32 random bytes shown to the user once as 24 BIP-39
//! words. Either the account password or the recovery key opens a backup.
//!
//! The server never learns the password either: what the client sends at
//! sign-in is [`auth_secret`] — a separate Argon2id hash with a different
//! salt, from which the backup key can't be derived.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{aead::{Aead, KeyInit, Payload}, ChaCha20Poly1305, Nonce};
use hkdf::Hkdf;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use zeroize::Zeroize;

const MAGIC: &[u8; 5] = b"AQBK1";
const WORDS: &str = include_str!("bip39_english.txt");

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("not an Aquamarine backup")]
    Format,
    #[error("wrong password or recovery key")]
    Key,
    #[error("recovery key: {0}")]
    Phrase(String),
    #[error("backup is damaged")]
    Damaged,
}

/// A wrapped copy of the data key.
#[derive(Clone, Serialize, Deserialize)]
pub struct Wrap {
    #[serde(with = "hex_bytes")]
    pub nonce: Vec<u8>,
    #[serde(with = "hex_bytes")]
    pub ct: Vec<u8>,
}

/// Argon2id parameters (kept in the header so they can be raised later).
#[derive(Clone, Serialize, Deserialize)]
pub struct Kdf {
    #[serde(with = "hex_bytes")]
    pub salt: Vec<u8>,
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Header {
    pub kdf: Kdf,
    pub pw: Wrap,
    pub rk: Wrap,
    /// Seconds since the epoch.
    pub created: u64,
}

/// What a device keeps to make new backups without asking again.
#[derive(Clone, Serialize, Deserialize)]
pub struct Keys {
    #[serde(with = "hex_bytes")]
    pub dek: Vec<u8>,
    pub header: Header,
}

impl Drop for Keys {
    fn drop(&mut self) {
        self.dek.zeroize();
    }
}

mod hex_bytes {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(v))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        hex::decode(String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn argon(password: &str, salt: &[u8], m_kib: u32, t: u32, p: u32) -> [u8; 32] {
    let params = Params::new(m_kib, t, p, Some(32)).expect("argon2 params");
    let mut out = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, &mut out)
        .expect("argon2");
    out
}

fn wrap(kek: &[u8; 32], dek: &[u8], label: &[u8]) -> Wrap {
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let ct = ChaCha20Poly1305::new(kek.into())
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: dek, aad: label })
        .expect("encrypt");
    Wrap { nonce: nonce.to_vec(), ct }
}

fn unwrap(kek: &[u8; 32], w: &Wrap, label: &[u8]) -> Result<Vec<u8>, BackupError> {
    if w.nonce.len() != 12 {
        return Err(BackupError::Format);
    }
    ChaCha20Poly1305::new(kek.into())
        .decrypt(Nonce::from_slice(&w.nonce), Payload { msg: &w.ct, aad: label })
        .map_err(|_| BackupError::Key)
}

fn recovery_kek(entropy: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    Hkdf::<Sha256>::new(None, entropy).expand(b"aquamarine backup recovery v1", &mut out).expect("hkdf");
    out
}

/// What the client sends to the server instead of the password, at sign-up
/// and sign-in. Argon2id with a salt bound to the e-mail; the backup key
/// uses another, random salt, so the server can't get from one to the other.
pub fn auth_secret(password: &str, email: &str) -> String {
    let salt = Sha256::digest(format!("aquamarine/auth/v1|{}", email.trim().to_lowercase()).as_bytes());
    format!("aqv2${}", hex::encode(argon(password, &salt[..16], 19 * 1024, 2, 1)))
}

// ---------------------------------------------------------------- recovery key

/// What the relay keeps to recognise the recovery key at a password reset
/// (it stores only a hash of this). HKDF with its own label, so it can't
/// open the backup. Same 24 words → same check.
pub fn recovery_check(phrase: &str) -> Result<String, BackupError> {
    let mut e = phrase_to_entropy(phrase)?;
    let mut out = [0u8; 32];
    Hkdf::<Sha256>::new(None, &e).expand(b"aquamarine account recovery v1", &mut out).expect("hkdf");
    e.zeroize();
    Ok(hex::encode(out))
}

/// A fresh 24-word recovery key.
pub fn new_phrase() -> String {
    let mut rk = [0u8; 32];
    OsRng.fill_bytes(&mut rk);
    let p = entropy_to_phrase(&rk);
    rk.zeroize();
    p
}

fn wordlist() -> Vec<&'static str> {
    WORDS.lines().map(str::trim).filter(|w| !w.is_empty()).collect()
}

/// 32 bytes → 24 BIP-39 words (8-bit SHA-256 checksum).
pub fn entropy_to_phrase(entropy: &[u8; 32]) -> String {
    let words = wordlist();
    let check = Sha256::digest(entropy)[0];
    let mut bits = Vec::with_capacity(264);
    for b in entropy.iter().chain(std::iter::once(&check)) {
        for i in (0..8).rev() {
            bits.push((b >> i) & 1);
        }
    }
    bits.chunks(11)
        .map(|c| words[c.iter().fold(0usize, |a, b| (a << 1) | *b as usize)])
        .collect::<Vec<_>>()
        .join(" ")
}

/// 24 words → 32 bytes, checking the checksum.
pub fn phrase_to_entropy(phrase: &str) -> Result<[u8; 32], BackupError> {
    let words = wordlist();
    let given: Vec<String> = phrase.split_whitespace().map(|w| w.to_lowercase()).collect();
    if given.len() != 24 {
        return Err(BackupError::Phrase(format!("expected 24 words, got {}", given.len())));
    }
    let mut bits = Vec::with_capacity(264);
    for w in &given {
        let idx = words.iter().position(|x| x == w).ok_or_else(|| BackupError::Phrase(format!("unknown word “{w}”")))?;
        for i in (0..11).rev() {
            bits.push(((idx >> i) & 1) as u8);
        }
    }
    let bytes: Vec<u8> = bits.chunks(8).map(|c| c.iter().fold(0u8, |a, b| (a << 1) | b)).collect();
    let mut e = [0u8; 32];
    e.copy_from_slice(&bytes[..32]);
    if Sha256::digest(e)[0] != bytes[32] {
        return Err(BackupError::Phrase("a word is mistyped (checksum)".into()));
    }
    Ok(e)
}

// ---------------------------------------------------------------- keys

/// New backup keys for this account. Returns the keys to keep and the
/// 24-word recovery key to show the user once.
pub fn new_keys(password: &str) -> (Keys, String) {
    let mut rk = [0u8; 32];
    OsRng.fill_bytes(&mut rk);
    new_keys_with(password, rk)
}

/// Like [`new_keys`], keeping a recovery key the user already has.
pub fn new_keys_with_phrase(password: &str, phrase: &str) -> Result<(Keys, String), BackupError> {
    Ok(new_keys_with(password, phrase_to_entropy(phrase)?))
}

fn new_keys_with(password: &str, mut rk: [u8; 32]) -> (Keys, String) {
    let mut dek = vec![0u8; 32];
    OsRng.fill_bytes(&mut dek);
    let mut salt = vec![0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let kdf = Kdf { salt, m_kib: 64 * 1024, t: 3, p: 1 };
    let pw_kek = argon(password, &kdf.salt, kdf.m_kib, kdf.t, kdf.p);
    let header = Header {
        pw: wrap(&pw_kek, &dek, b"pw"),
        rk: wrap(&recovery_kek(&rk), &dek, b"rk"),
        kdf,
        created: now(),
    };
    let phrase = entropy_to_phrase(&rk);
    rk.zeroize();
    (Keys { dek, header }, phrase)
}

/// The password changed: wrap the same data key under the new one.
pub fn rewrap_password(keys: &mut Keys, password: &str) {
    let mut salt = vec![0u8; 16];
    OsRng.fill_bytes(&mut salt);
    keys.header.kdf.salt = salt;
    let kek = argon(password, &keys.header.kdf.salt, keys.header.kdf.m_kib, keys.header.kdf.t, keys.header.kdf.p);
    keys.header.pw = wrap(&kek, &keys.dek, b"pw");
}

/// Wrap the data key under another recovery key (the device lost the old one).
pub fn rewrap_recovery(keys: &mut Keys, phrase: &str) -> Result<(), BackupError> {
    let mut e = phrase_to_entropy(phrase)?;
    keys.header.rk = wrap(&recovery_kek(&e), &keys.dek, b"rk");
    e.zeroize();
    Ok(())
}

// ---------------------------------------------------------------- blobs

pub fn seal(keys: &Keys, payload: &[u8]) -> Vec<u8> {
    let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(3));
    let _ = enc.write_all(payload);
    let packed = enc.finish().unwrap_or_default();
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let dek: &[u8; 32] = keys.dek.as_slice().try_into().expect("dek is 32 bytes");
    let ct = ChaCha20Poly1305::new(dek.into())
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: &packed, aad: MAGIC })
        .expect("encrypt");
    let header = serde_json::to_vec(&keys.header).expect("header json");
    let mut out = Vec::with_capacity(5 + 4 + header.len() + 12 + ct.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(header.len() as u32).to_be_bytes());
    out.extend_from_slice(&header);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    out
}

pub fn header_of(blob: &[u8]) -> Result<(Header, usize), BackupError> {
    if blob.len() < 9 || &blob[..5] != MAGIC {
        return Err(BackupError::Format);
    }
    let hl = u32::from_be_bytes(blob[5..9].try_into().unwrap()) as usize;
    if blob.len() < 9 + hl + 12 {
        return Err(BackupError::Format);
    }
    let header: Header = serde_json::from_slice(&blob[9..9 + hl]).map_err(|_| BackupError::Format)?;
    Ok((header, 9 + hl))
}

fn open_with_dek(blob: &[u8], body_at: usize, dek: &[u8]) -> Result<Vec<u8>, BackupError> {
    let dek: &[u8; 32] = dek.try_into().map_err(|_| BackupError::Format)?;
    let (nonce, ct) = blob[body_at..].split_at(12);
    let packed = ChaCha20Poly1305::new(dek.into())
        .decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad: MAGIC })
        .map_err(|_| BackupError::Damaged)?;
    let mut out = Vec::new();
    flate2::read::DeflateDecoder::new(&packed[..]).read_to_end(&mut out).map_err(|_| BackupError::Damaged)?;
    Ok(out)
}

/// Open with the account password. Returns the payload and the keys, so
/// this device can keep making backups with the same recovery key.
pub fn open_with_password(blob: &[u8], password: &str) -> Result<(Vec<u8>, Keys), BackupError> {
    let (header, at) = header_of(blob)?;
    let k = &header.kdf;
    if k.m_kib > 1024 * 1024 || k.t > 64 || k.p > 16 {
        return Err(BackupError::Format); // refuse absurd parameters
    }
    let dek = unwrap(&argon(password, &k.salt, k.m_kib, k.t, k.p), &header.pw, b"pw")?;
    let payload = open_with_dek(blob, at, &dek)?;
    Ok((payload, Keys { dek, header }))
}

/// Open with the 24-word recovery key (the password is forgotten).
pub fn open_with_recovery(blob: &[u8], phrase: &str) -> Result<(Vec<u8>, Keys), BackupError> {
    let (header, at) = header_of(blob)?;
    let mut e = phrase_to_entropy(phrase)?;
    let dek = unwrap(&recovery_kek(&e), &header.rk, b"rk");
    e.zeroize();
    let dek = dek?;
    let payload = open_with_dek(blob, at, &dek)?;
    Ok((payload, Keys { dek, header }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_check_and_rewrap() {
        let p = new_phrase();
        assert_eq!(recovery_check(&p).unwrap(), recovery_check(&format!("  {}  ", p.to_uppercase())).unwrap());
        assert_ne!(recovery_check(&p).unwrap(), recovery_check(&new_phrase()).unwrap());
        let (mut keys, p1) = new_keys_with_phrase("pw", &p).unwrap();
        assert_eq!(p1, p);
        let p2 = new_phrase();
        rewrap_recovery(&mut keys, &p2).unwrap();
        let blob = seal(&keys, b"x");
        assert!(open_with_recovery(&blob, &p).is_err());
        assert_eq!(open_with_recovery(&blob, &p2).unwrap().0, b"x");
        assert_eq!(open_with_password(&blob, "pw").unwrap().0, b"x");
    }

    #[test]
    fn wordlist_is_bip39() {
        let w = wordlist();
        assert_eq!(w.len(), 2048);
        assert_eq!((w[0], w[2047]), ("abandon", "zoo"));
    }

    #[test]
    fn bip39_vector() {
        // Trezor test vector: 32 bytes of 0x00 → "abandon ×23 art".
        let p = entropy_to_phrase(&[0u8; 32]);
        assert_eq!(p, format!("{}art", "abandon ".repeat(23)));
        assert_eq!(phrase_to_entropy(&p).unwrap(), [0u8; 32]);
        // 0x7f… vector
        let e = [0x7fu8; 32];
        let p = entropy_to_phrase(&e);
        assert_eq!(p, "legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth title");
        assert!(matches!(phrase_to_entropy(&p.replace("title", "zoo")), Err(BackupError::Phrase(_))));
    }

    #[test]
    fn seal_open_with_password_and_recovery() {
        let (keys, phrase) = new_keys("correct horse");
        let payload = b"chat history goes here ".repeat(200);
        let blob = seal(&keys, &payload);
        assert!(blob.len() < payload.len(), "compressed");
        assert!(!blob.windows(12).any(|w| w == b"chat history"), "plaintext leaked");
        let (p1, k1) = open_with_password(&blob, "correct horse").unwrap();
        assert_eq!(p1, payload);
        assert_eq!(k1.dek, keys.dek);
        assert!(matches!(open_with_password(&blob, "wrong"), Err(BackupError::Key)));
        let (p2, _) = open_with_recovery(&blob, &phrase).unwrap();
        assert_eq!(p2, payload);
        // A new password keeps the recovery key working.
        let mut k = keys.clone();
        rewrap_password(&mut k, "new pass");
        let blob2 = seal(&k, b"x");
        assert!(open_with_password(&blob2, "correct horse").is_err());
        assert_eq!(open_with_password(&blob2, "new pass").unwrap().0, b"x");
        assert_eq!(open_with_recovery(&blob2, &phrase).unwrap().0, b"x");
    }

    #[test]
    fn auth_secret_is_stable_and_separate() {
        let a = auth_secret("pw", "Me@Example.com");
        assert_eq!(a, auth_secret("pw", " me@example.com "));
        assert_ne!(a, auth_secret("pw", "other@example.com"));
        assert!(a.starts_with("aqv2$") && a.len() == 5 + 64);
    }
}

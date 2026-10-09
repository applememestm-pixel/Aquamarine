//! Thin, opinionated wrapper around the crypto primitives we depend on.
//!
//! The rest of the crate never touches `x25519-dalek`, `ed25519-dalek`,
//! `chacha20poly1305`, `hkdf` or `hmac` directly. That keeps the surface
//! area small and lets us swap implementations later without churning the
//! protocol code.

use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Nonce,
};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use thiserror::Error;
use x25519_dalek::{PublicKey as X25519Public, StaticSecret};
use zeroize::{Zeroize, ZeroizeOnDrop};

// ---------- Errors -----------------------------------------------------------

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("AEAD encryption failed")]
    AeadEncrypt,
    #[error("AEAD decryption failed (bad tag, wrong key, or tampered ciphertext)")]
    AeadDecrypt,
    #[error("HKDF expand output too large")]
    HkdfExpand,
    #[error("bad Ed25519 signature")]
    BadSignature,
    #[error("invalid key length: expected {expected}, got {got}")]
    BadKeyLength { expected: usize, got: usize },
    #[error("invalid key encoding")]
    BadKeyEncoding,
}

// ---------- X25519 -----------------------------------------------------------

/// Length of an X25519 public key / shared secret / HKDF chain key.
pub const X25519_LEN: usize = 32;

/// An X25519 secret. Zeroized on drop.
#[derive(Clone, ZeroizeOnDrop)]
pub struct DhSecret(StaticSecret);

impl core::fmt::Debug for DhSecret {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("DhSecret(<redacted>)")
    }
}

/// An X25519 public key.
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct DhPublic(pub [u8; X25519_LEN]);

impl core::fmt::Debug for DhPublic {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "DhPublic({})", hex_short(&self.0))
    }
}

// Serde impls for DhSecret. Only ever used inside the crate to serialize
// session state that is then encrypted at rest by `aqua-client-core::vault`.
// Never send this over the wire.
impl Serialize for DhSecret {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        serde_bytes::serialize(&self.0.to_bytes()[..], s)
    }
}
impl<'de> Deserialize<'de> for DhSecret {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v: Vec<u8> = serde_bytes::deserialize(d)?;
        let arr: [u8; X25519_LEN] = v.as_slice().try_into().map_err(|_| {
            serde::de::Error::invalid_length(v.len(), &"32-byte X25519 secret")
        })?;
        Ok(DhSecret::from_bytes(arr))
    }
}

impl DhSecret {
    /// Generate a fresh X25519 secret using the OS RNG.
    pub fn generate() -> Self {
        DhSecret(StaticSecret::random_from_rng(OsRng))
    }

    /// Derive the public component.
    pub fn public(&self) -> DhPublic {
        DhPublic(X25519Public::from(&self.0).to_bytes())
    }

    /// Diffie-Hellman with a peer's public key. Returns the raw 32-byte
    /// shared secret. Zeroized on drop by the return type.
    pub fn dh(&self, peer: &DhPublic) -> SharedSecret {
        let peer_pub = X25519Public::from(peer.0);
        let ss = self.0.diffie_hellman(&peer_pub);
        SharedSecret(ss.to_bytes())
    }

    /// Serialize the secret. NEVER send this over the wire — used only
    /// for on-disk persistence, and only after wrapping with a KEK.
    pub fn to_bytes(&self) -> [u8; X25519_LEN] {
        self.0.to_bytes()
    }

    pub fn from_bytes(bytes: [u8; X25519_LEN]) -> Self {
        DhSecret(StaticSecret::from(bytes))
    }
}

impl DhPublic {
    pub fn as_bytes(&self) -> &[u8; X25519_LEN] {
        &self.0
    }

    pub fn from_bytes(bytes: [u8; X25519_LEN]) -> Self {
        DhPublic(bytes)
    }

    pub fn from_slice(slice: &[u8]) -> Result<Self, CryptoError> {
        if slice.len() != X25519_LEN {
            return Err(CryptoError::BadKeyLength {
                expected: X25519_LEN,
                got: slice.len(),
            });
        }
        let mut out = [0u8; X25519_LEN];
        out.copy_from_slice(slice);
        Ok(DhPublic(out))
    }
}

/// A DH-derived 32-byte shared secret. Kept in its own type so callers
/// cannot mix it up with an HKDF output or a message key.
#[derive(Clone, ZeroizeOnDrop)]
pub struct SharedSecret(pub(crate) [u8; X25519_LEN]);

impl core::fmt::Debug for SharedSecret {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("SharedSecret(<redacted>)")
    }
}

impl SharedSecret {
    pub fn as_bytes(&self) -> &[u8; X25519_LEN] {
        &self.0
    }
}

// ---------- Ed25519 ----------------------------------------------------------

pub const ED25519_PUB_LEN: usize = 32;
pub const ED25519_SIG_LEN: usize = 64;

/// An Ed25519 signing key. Used for identity assertions and prekey
/// signatures. Distinct from the X25519 DH key.
#[derive(Clone, ZeroizeOnDrop)]
pub struct SignSecret(SigningKey);

impl core::fmt::Debug for SignSecret {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("SignSecret(<redacted>)")
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct SignPublic(pub [u8; ED25519_PUB_LEN]);

impl core::fmt::Debug for SignPublic {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "SignPublic({})", hex_short(&self.0))
    }
}

// Serde impls for SignSecret. Same at-rest-only rules apply as for DhSecret.
impl Serialize for SignSecret {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        serde_bytes::serialize(&self.0.to_bytes()[..], s)
    }
}
impl<'de> Deserialize<'de> for SignSecret {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v: Vec<u8> = serde_bytes::deserialize(d)?;
        let arr: [u8; 32] = v.as_slice().try_into().map_err(|_| {
            serde::de::Error::invalid_length(v.len(), &"32-byte Ed25519 seed")
        })?;
        Ok(SignSecret::from_bytes(arr))
    }
}

impl SignSecret {
    pub fn generate() -> Self {
        let mut seed = [0u8; 32];
        OsRng.fill_bytes(&mut seed);
        let sk = SigningKey::from_bytes(&seed);
        seed.zeroize();
        SignSecret(sk)
    }

    pub fn public(&self) -> SignPublic {
        SignPublic(self.0.verifying_key().to_bytes())
    }

    pub fn sign(&self, msg: &[u8]) -> [u8; ED25519_SIG_LEN] {
        self.0.sign(msg).to_bytes()
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        SignSecret(SigningKey::from_bytes(&bytes))
    }
}

impl SignPublic {
    pub fn verify(
        &self,
        msg: &[u8],
        sig: &[u8; ED25519_SIG_LEN],
    ) -> Result<(), CryptoError> {
        let vk = VerifyingKey::from_bytes(&self.0).map_err(|_| CryptoError::BadKeyEncoding)?;
        let sig = Signature::from_bytes(sig);
        vk.verify(msg, &sig).map_err(|_| CryptoError::BadSignature)
    }

    pub fn as_bytes(&self) -> &[u8; ED25519_PUB_LEN] {
        &self.0
    }

    pub fn from_bytes(bytes: [u8; ED25519_PUB_LEN]) -> Self {
        SignPublic(bytes)
    }
}

// ---------- HKDF-SHA256 ------------------------------------------------------

/// HKDF-Extract-then-Expand over SHA-256. `salt` may be empty.
///
/// Returns exactly `out_len` bytes; caller decides how many they need.
pub fn hkdf_sha256(
    salt: &[u8],
    ikm: &[u8],
    info: &[u8],
    out_len: usize,
) -> Result<Vec<u8>, CryptoError> {
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
    let mut out = vec![0u8; out_len];
    hk.expand(info, &mut out).map_err(|_| CryptoError::HkdfExpand)?;
    Ok(out)
}

// ---------- HMAC-SHA256 ------------------------------------------------------

pub const HMAC_TAG_LEN: usize = 32;

/// HMAC-SHA256. Used to derive chain keys and message keys in the ratchet.
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; HMAC_TAG_LEN] {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key)
        .expect("HMAC-SHA256 accepts any key length");
    mac.update(data);
    let out = mac.finalize().into_bytes();
    let mut arr = [0u8; HMAC_TAG_LEN];
    arr.copy_from_slice(&out);
    arr
}

// ---------- ChaCha20-Poly1305 AEAD -------------------------------------------

pub const AEAD_KEY_LEN: usize = 32;
pub const AEAD_NONCE_LEN: usize = 12;

/// Encrypt with ChaCha20-Poly1305. `nonce` must not be reused with the
/// same key — callers derive it deterministically per-message or draw
/// it at random.
pub fn aead_encrypt(
    key: &[u8; AEAD_KEY_LEN],
    nonce: &[u8; AEAD_NONCE_LEN],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let cipher = ChaCha20Poly1305::new(key.into());
    cipher
        .encrypt(Nonce::from_slice(nonce), Payload { msg: plaintext, aad })
        .map_err(|_| CryptoError::AeadEncrypt)
}

pub fn aead_decrypt(
    key: &[u8; AEAD_KEY_LEN],
    nonce: &[u8; AEAD_NONCE_LEN],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let cipher = ChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(Nonce::from_slice(nonce), Payload { msg: ciphertext, aad })
        .map_err(|_| CryptoError::AeadDecrypt)
}

// ---------- Constant-time equality -------------------------------------------

pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

// ---------- Utility ----------------------------------------------------------

pub(crate) fn hex_short(bytes: &[u8]) -> String {
    // Debug-only helper: first 4 bytes as hex, so logs and asserts stay
    // readable without leaking whole keys.
    let n = bytes.len().min(4);
    let mut s = String::with_capacity(n * 2 + 2);
    for b in &bytes[..n] {
        use core::fmt::Write;
        let _ = write!(s, "{:02x}", b);
    }
    s.push('…');
    s
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x25519_dh_agrees() {
        let a = DhSecret::generate();
        let b = DhSecret::generate();
        let s_ab = a.dh(&b.public());
        let s_ba = b.dh(&a.public());
        assert_eq!(s_ab.as_bytes(), s_ba.as_bytes());
    }

    #[test]
    fn x25519_dh_differs_between_peers() {
        let a = DhSecret::generate();
        let b = DhSecret::generate();
        let c = DhSecret::generate();
        assert_ne!(a.dh(&b.public()).as_bytes(), a.dh(&c.public()).as_bytes());
    }

    #[test]
    fn ed25519_sign_verify_roundtrip() {
        let sk = SignSecret::generate();
        let pk = sk.public();
        let msg = b"aquamarine identity assertion";
        let sig = sk.sign(msg);
        pk.verify(msg, &sig).expect("valid signature");
    }

    #[test]
    fn ed25519_rejects_wrong_message() {
        let sk = SignSecret::generate();
        let pk = sk.public();
        let sig = sk.sign(b"hello");
        assert!(matches!(
            pk.verify(b"HELLO", &sig),
            Err(CryptoError::BadSignature)
        ));
    }

    #[test]
    fn ed25519_rejects_wrong_signer() {
        let sk = SignSecret::generate();
        let other = SignSecret::generate();
        let sig = sk.sign(b"msg");
        assert!(matches!(
            other.public().verify(b"msg", &sig),
            Err(CryptoError::BadSignature)
        ));
    }

    #[test]
    fn hkdf_deterministic_and_info_sensitive() {
        let ikm = [0x11u8; 32];
        let a = hkdf_sha256(b"salt", &ikm, b"info-1", 64).unwrap();
        let b = hkdf_sha256(b"salt", &ikm, b"info-1", 64).unwrap();
        let c = hkdf_sha256(b"salt", &ikm, b"info-2", 64).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn aead_roundtrip_and_tamper_detection() {
        let key = [0x42u8; AEAD_KEY_LEN];
        let nonce = [0x00u8; AEAD_NONCE_LEN];
        let aad = b"header";
        let pt = b"top secret message";
        let ct = aead_encrypt(&key, &nonce, aad, pt).unwrap();
        assert_eq!(aead_decrypt(&key, &nonce, aad, &ct).unwrap(), pt);

        // Tampered ciphertext must fail.
        let mut bad = ct.clone();
        bad[0] ^= 0x01;
        assert!(aead_decrypt(&key, &nonce, aad, &bad).is_err());

        // Wrong AAD must fail.
        assert!(aead_decrypt(&key, &nonce, b"other", &ct).is_err());
    }

    #[test]
    fn hmac_deterministic() {
        let k = b"k";
        assert_eq!(hmac_sha256(k, b"x"), hmac_sha256(k, b"x"));
        assert_ne!(hmac_sha256(k, b"x"), hmac_sha256(k, b"y"));
    }
}

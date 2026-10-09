//! Long-term and short-term key material, and the bundle a party
//! publishes on the server so others can start a session with them.
//!
//! An Aquamarine identity is composed of two independent primitives:
//!
//! * an **Ed25519 signing key** used to sign identity assertions and
//!   prekeys, and
//! * an **X25519 Diffie-Hellman key** used for the initial X3DH
//!   agreement.
//!
//! Keeping the two separate (rather than deriving one from the other via
//! XEdDSA the way Signal does) makes the code smaller and easier to
//! audit at the cost of an extra 32 bytes per identity on the wire.

use serde::{Deserialize, Serialize};

use crate::crypto::{
    CryptoError, DhPublic, DhSecret, SignPublic, SignSecret, ED25519_SIG_LEN,
};

// ---------- IDs --------------------------------------------------------------

/// Identifier for a prekey inside a user's bundle. Small integers; the
/// server increments them per-upload.
pub type PreKeyId = u32;

// ---------- Identity ---------------------------------------------------------

/// A party's long-term identity: signing key + DH key. Never sent as-is
/// over the wire; only [`IdentityPublic`] leaves the device.
///
/// The inner secrets self-zero on drop, so we don't need an outer
/// `ZeroizeOnDrop` derive (which would also block destructuring and
/// cloning).
///
/// `Serialize`/`Deserialize` are only ever used inside
/// `aqua-client-core::vault` for encrypted-at-rest storage. Do NOT send
/// this type over the network. `Clone` is provided so long-running
/// tasks can hold their own copy of the identity for signing without
/// keeping a lock on the app state.
#[derive(Clone, Serialize, Deserialize)]
pub struct IdentityKeyPair {
    pub sign: SignSecret,
    pub dh: DhSecret,
}

impl core::fmt::Debug for IdentityKeyPair {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("IdentityKeyPair")
            .field("sign_pub", &self.sign.public())
            .field("dh_pub", &self.dh.public())
            .finish_non_exhaustive()
    }
}

impl IdentityKeyPair {
    pub fn generate() -> Self {
        Self {
            sign: SignSecret::generate(),
            dh: DhSecret::generate(),
        }
    }

    pub fn public(&self) -> IdentityPublic {
        IdentityPublic {
            sign_pub: self.sign.public(),
            dh_pub: self.dh.public(),
        }
    }
}

/// Public half of an identity. Included in every prekey bundle and every
/// initial X3DH message.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityPublic {
    pub sign_pub: SignPublic,
    pub dh_pub: DhPublic,
}

// ---------- Signed prekey ----------------------------------------------------

/// A medium-lived X25519 keypair, signed with the owner's identity
/// signing key. Rotated periodically (typical policy: once a week).
///
/// Not `ZeroizeOnDrop` on purpose: the inner [`DhSecret`] self-zeroes on
/// drop, and skipping the outer `Drop` lets callers destructure the
/// struct (e.g. move `.secret` into a ratchet).
#[derive(Serialize, Deserialize)]
pub struct SignedPreKey {
    pub id: PreKeyId,
    pub secret: DhSecret,
    #[serde(with = "serde_bytes_array")]
    pub signature: [u8; ED25519_SIG_LEN],
}

impl core::fmt::Debug for SignedPreKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SignedPreKey")
            .field("id", &self.id)
            .field("public", &self.secret.public())
            .finish_non_exhaustive()
    }
}

impl SignedPreKey {
    pub fn generate(id: PreKeyId, identity: &IdentityKeyPair) -> Self {
        let secret = DhSecret::generate();
        let pub_bytes = secret.public();
        // The signature covers exactly the 32-byte DH public key.
        let signature = identity.sign.sign(pub_bytes.as_bytes());
        SignedPreKey { id, secret, signature }
    }

    pub fn public(&self) -> SignedPreKeyPublic {
        SignedPreKeyPublic {
            id: self.id,
            dh_pub: self.secret.public(),
            signature: self.signature,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedPreKeyPublic {
    pub id: PreKeyId,
    pub dh_pub: DhPublic,
    #[serde(with = "serde_bytes_array")]
    pub signature: [u8; ED25519_SIG_LEN],
}

impl SignedPreKeyPublic {
    /// Verify the SPK signature under the identity's signing key.
    pub fn verify(&self, identity: &IdentityPublic) -> Result<(), CryptoError> {
        identity
            .sign_pub
            .verify(self.dh_pub.as_bytes(), &self.signature)
    }
}

// ---------- One-time prekey --------------------------------------------------

/// A single-use X25519 keypair. The server hands out one OPK per new
/// session and destroys it afterwards, giving new sessions forward
/// secrecy even before the first Double Ratchet DH step.
///
/// Not `ZeroizeOnDrop` on purpose (see [`SignedPreKey`] for rationale).
#[derive(Serialize, Deserialize)]
pub struct OneTimePreKey {
    pub id: PreKeyId,
    pub secret: DhSecret,
}

impl core::fmt::Debug for OneTimePreKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OneTimePreKey")
            .field("id", &self.id)
            .field("public", &self.secret.public())
            .finish_non_exhaustive()
    }
}

impl OneTimePreKey {
    pub fn generate(id: PreKeyId) -> Self {
        OneTimePreKey { id, secret: DhSecret::generate() }
    }

    pub fn public(&self) -> OneTimePreKeyPublic {
        OneTimePreKeyPublic { id: self.id, dh_pub: self.secret.public() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneTimePreKeyPublic {
    pub id: PreKeyId,
    pub dh_pub: DhPublic,
}

// ---------- Prekey bundle ----------------------------------------------------

/// The blob a client publishes to the server and other clients fetch
/// when they want to start a session with them. All fields are public.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreKeyBundle {
    pub identity: IdentityPublic,
    pub signed_prekey: SignedPreKeyPublic,
    /// Optional — the server pops a fresh OPK off the stack when
    /// producing this bundle for a specific requester. If none is
    /// available the session still works, just with slightly weaker
    /// initial secrecy properties.
    pub one_time_prekey: Option<OneTimePreKeyPublic>,
}

impl PreKeyBundle {
    /// Consistency check: the SPK is really signed by this identity.
    /// Callers MUST invoke this before running X3DH.
    pub fn verify(&self) -> Result<(), CryptoError> {
        self.signed_prekey.verify(&self.identity)
    }
}

// ---------- Serde helper: fixed-size byte arrays -----------------------------

mod serde_bytes_array {
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer, const N: usize>(
        bytes: &[u8; N],
        s: S,
    ) -> Result<S::Ok, S::Error> {
        serde_bytes::serialize(&bytes[..], s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>, const N: usize>(
        d: D,
    ) -> Result<[u8; N], D::Error> {
        let v: Vec<u8> = serde_bytes::deserialize(d)?;
        v.as_slice().try_into().map_err(|_| {
            serde::de::Error::invalid_length(v.len(), &"fixed-size byte array")
        })
    }
}

// ---------- Serde: DhPublic / SignPublic as raw bytes ------------------------

impl Serialize for DhPublic {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        serde_bytes::serialize(&self.0[..], s)
    }
}

impl<'de> Deserialize<'de> for DhPublic {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v: Vec<u8> = serde_bytes::deserialize(d)?;
        let arr: [u8; 32] = v.as_slice().try_into().map_err(|_| {
            serde::de::Error::invalid_length(v.len(), &"32-byte X25519 public key")
        })?;
        Ok(DhPublic(arr))
    }
}

impl Serialize for SignPublic {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        serde_bytes::serialize(&self.0[..], s)
    }
}

impl<'de> Deserialize<'de> for SignPublic {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v: Vec<u8> = serde_bytes::deserialize(d)?;
        let arr: [u8; 32] = v.as_slice().try_into().map_err(|_| {
            serde::de::Error::invalid_length(v.len(), &"32-byte Ed25519 public key")
        })?;
        Ok(SignPublic(arr))
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_prekey_verifies() {
        let id = IdentityKeyPair::generate();
        let spk = SignedPreKey::generate(1, &id);
        spk.public().verify(&id.public()).expect("valid SPK signature");
    }

    #[test]
    fn signed_prekey_rejects_wrong_identity() {
        let id = IdentityKeyPair::generate();
        let other = IdentityKeyPair::generate();
        let spk = SignedPreKey::generate(1, &id);
        assert!(spk.public().verify(&other.public()).is_err());
    }

    #[test]
    fn bundle_verify_ok() {
        let id = IdentityKeyPair::generate();
        let spk = SignedPreKey::generate(7, &id);
        let opk = OneTimePreKey::generate(42);
        let bundle = PreKeyBundle {
            identity: id.public(),
            signed_prekey: spk.public(),
            one_time_prekey: Some(opk.public()),
        };
        bundle.verify().expect("bundle should verify");
    }

    #[test]
    fn bundle_verify_rejects_tampering() {
        let id = IdentityKeyPair::generate();
        let spk = SignedPreKey::generate(7, &id);
        let mut spub = spk.public();
        // Flip one bit in the SPK dh_pub — signature must no longer verify.
        spub.dh_pub.0[0] ^= 0x01;
        let bundle = PreKeyBundle {
            identity: id.public(),
            signed_prekey: spub,
            one_time_prekey: None,
        };
        assert!(bundle.verify().is_err());
    }

    #[test]
    fn bundle_serde_roundtrip() {
        let id = IdentityKeyPair::generate();
        let spk = SignedPreKey::generate(1, &id);
        let bundle = PreKeyBundle {
            identity: id.public(),
            signed_prekey: spk.public(),
            one_time_prekey: Some(OneTimePreKey::generate(99).public()),
        };
        let bytes = bincode::serialize(&bundle).unwrap();
        let back: PreKeyBundle = bincode::deserialize(&bytes).unwrap();
        back.verify().unwrap();
        assert_eq!(back.identity.dh_pub, bundle.identity.dh_pub);
        assert_eq!(back.signed_prekey.id, bundle.signed_prekey.id);
    }
}

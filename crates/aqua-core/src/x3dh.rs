//! Extended Triple Diffie-Hellman (X3DH) initial key agreement.
//!
//! Based on Moxie Marlinspike's original X3DH spec. Adapted for our
//! separate-signing-and-DH-key identity model (see [`crate::keys`]):
//!
//! ```text
//! DH1 = DH(IK_A,  SPK_B)         DH involving identity + signed prekey
//! DH2 = DH( EK_A, IK_B )         DH involving ephemeral + identity
//! DH3 = DH( EK_A, SPK_B)         DH involving ephemeral + signed prekey
//! DH4 = DH( EK_A, OPK_B)         optional; used only if OPK present
//!
//! SK  = HKDF-SHA256(salt = zeroes(32),
//!                   ikm  = F || DH1 || DH2 || DH3 [|| DH4],
//!                   info = "Aquamarine/X3DH v1",
//!                   len  = 32)
//! ```
//!
//! where `F` is 32 bytes of 0xFF, following the spec to make the X25519
//! branch of X3DH domain-separate from Ed25519 X3DH.
//!
//! Associated data `AD` for the first ciphertext is:
//!
//! ```text
//! AD = IK_A_dh_pub || IK_B_dh_pub
//! ```

use thiserror::Error;

use crate::{
    crypto::{hkdf_sha256, CryptoError, DhPublic, DhSecret, SharedSecret},
    keys::{IdentityKeyPair, IdentityPublic, OneTimePreKey, PreKeyBundle, SignedPreKey},
};

const X3DH_INFO: &[u8] = b"Aquamarine/X3DH v1";
const X3DH_F: [u8; 32] = [0xFFu8; 32];

#[derive(Debug, Error)]
pub enum X3DHError {
    #[error("prekey bundle failed signature check: {0}")]
    BadBundle(#[from] CryptoError),
}

/// Output of the initiator side of X3DH.
#[derive(Debug)]
pub struct InitiatorOutput {
    /// The 32-byte shared secret. Fed into the Double Ratchet as the
    /// initial root key.
    pub shared_secret: SharedSecret,
    /// The ephemeral public key Alice generated. Sent to Bob.
    pub ephemeral_pub: DhPublic,
    /// The one-time-prekey id Alice consumed, if any. Sent to Bob so he
    /// can look up the matching OPK secret in his local store.
    pub used_opk_id: Option<crate::keys::PreKeyId>,
}

/// Alice's side: derive the initial shared secret from her identity and
/// Bob's freshly-fetched prekey bundle. `bundle` is verified before use.
pub fn x3dh_initiator(
    alice: &IdentityKeyPair,
    bundle: &PreKeyBundle,
) -> Result<InitiatorOutput, X3DHError> {
    bundle.verify()?;

    let ephemeral = DhSecret::generate();

    let dh1 = alice.dh.dh(&bundle.signed_prekey.dh_pub);
    let dh2 = ephemeral.dh(&bundle.identity.dh_pub);
    let dh3 = ephemeral.dh(&bundle.signed_prekey.dh_pub);
    let dh4 = bundle
        .one_time_prekey
        .as_ref()
        .map(|opk| ephemeral.dh(&opk.dh_pub));

    let sk = derive_sk(&dh1, &dh2, &dh3, dh4.as_ref())?;

    Ok(InitiatorOutput {
        shared_secret: sk,
        ephemeral_pub: ephemeral.public(),
        used_opk_id: bundle.one_time_prekey.as_ref().map(|o| o.id),
    })
}

/// Bob's side: given his own identity + prekeys and Alice's public
/// values (from her initial message), reconstruct the same shared
/// secret. `used_opk` must be the OneTimePreKey Alice consumed (looked
/// up by id) or `None` if she reported no OPK.
pub fn x3dh_responder(
    bob: &IdentityKeyPair,
    bob_signed_prekey: &SignedPreKey,
    used_opk: Option<&OneTimePreKey>,
    alice_identity: &IdentityPublic,
    alice_ephemeral_pub: &DhPublic,
) -> Result<SharedSecret, X3DHError> {
    let dh1 = bob_signed_prekey.secret.dh(&alice_identity.dh_pub);
    let dh2 = bob.dh.dh(alice_ephemeral_pub);
    let dh3 = bob_signed_prekey.secret.dh(alice_ephemeral_pub);
    let dh4 = used_opk.map(|opk| opk.secret.dh(alice_ephemeral_pub));

    derive_sk(&dh1, &dh2, &dh3, dh4.as_ref()).map_err(X3DHError::BadBundle)
}

/// Associated data for the first X3DH-encrypted message, per spec.
pub fn x3dh_associated_data(
    alice_identity: &IdentityPublic,
    bob_identity: &IdentityPublic,
) -> [u8; 64] {
    let mut ad = [0u8; 64];
    ad[..32].copy_from_slice(alice_identity.dh_pub.as_bytes());
    ad[32..].copy_from_slice(bob_identity.dh_pub.as_bytes());
    ad
}

// ---------- internals --------------------------------------------------------

fn derive_sk(
    dh1: &SharedSecret,
    dh2: &SharedSecret,
    dh3: &SharedSecret,
    dh4: Option<&SharedSecret>,
) -> Result<SharedSecret, CryptoError> {
    let mut ikm = Vec::with_capacity(32 + 32 * 4);
    ikm.extend_from_slice(&X3DH_F);
    ikm.extend_from_slice(dh1.as_bytes());
    ikm.extend_from_slice(dh2.as_bytes());
    ikm.extend_from_slice(dh3.as_bytes());
    if let Some(d4) = dh4 {
        ikm.extend_from_slice(d4.as_bytes());
    }
    let out = hkdf_sha256(&[0u8; 32], &ikm, X3DH_INFO, 32)?;
    let mut sk = [0u8; 32];
    sk.copy_from_slice(&out);
    Ok(SharedSecret(sk))
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{PreKeyBundle, SignedPreKey};

    fn make_bundle(bob: &IdentityKeyPair, use_opk: bool) -> (SignedPreKey, Option<OneTimePreKey>, PreKeyBundle) {
        let spk = SignedPreKey::generate(1, bob);
        let opk = if use_opk { Some(OneTimePreKey::generate(42)) } else { None };
        let bundle = PreKeyBundle {
            identity: bob.public(),
            signed_prekey: spk.public(),
            one_time_prekey: opk.as_ref().map(|o| o.public()),
        };
        (spk, opk, bundle)
    }

    #[test]
    fn initiator_and_responder_agree_with_opk() {
        let alice = IdentityKeyPair::generate();
        let bob = IdentityKeyPair::generate();
        let (bob_spk, bob_opk, bundle) = make_bundle(&bob, true);

        let init = x3dh_initiator(&alice, &bundle).unwrap();
        let bob_sk = x3dh_responder(
            &bob,
            &bob_spk,
            bob_opk.as_ref(),
            &alice.public(),
            &init.ephemeral_pub,
        )
        .unwrap();

        assert_eq!(init.shared_secret.as_bytes(), bob_sk.as_bytes());
        assert_eq!(init.used_opk_id, Some(42));
    }

    #[test]
    fn initiator_and_responder_agree_without_opk() {
        let alice = IdentityKeyPair::generate();
        let bob = IdentityKeyPair::generate();
        let (bob_spk, bob_opk, bundle) = make_bundle(&bob, false);
        assert!(bob_opk.is_none());

        let init = x3dh_initiator(&alice, &bundle).unwrap();
        let bob_sk = x3dh_responder(
            &bob,
            &bob_spk,
            None,
            &alice.public(),
            &init.ephemeral_pub,
        )
        .unwrap();

        assert_eq!(init.shared_secret.as_bytes(), bob_sk.as_bytes());
        assert_eq!(init.used_opk_id, None);
    }

    #[test]
    fn different_sessions_produce_different_keys() {
        let alice = IdentityKeyPair::generate();
        let bob = IdentityKeyPair::generate();
        let (_, _, b1) = make_bundle(&bob, true);
        let (_, _, b2) = make_bundle(&bob, true);
        let i1 = x3dh_initiator(&alice, &b1).unwrap();
        let i2 = x3dh_initiator(&alice, &b2).unwrap();
        assert_ne!(i1.shared_secret.as_bytes(), i2.shared_secret.as_bytes());
    }

    #[test]
    fn tampered_bundle_rejected() {
        let alice = IdentityKeyPair::generate();
        let bob = IdentityKeyPair::generate();
        let (_spk, _opk, mut bundle) = make_bundle(&bob, true);
        // Swap in a fresh, unsigned DH public.
        bundle.signed_prekey.dh_pub = DhSecret::generate().public();
        assert!(x3dh_initiator(&alice, &bundle).is_err());
    }

    #[test]
    fn associated_data_is_pair_of_dh_pubs() {
        let a = IdentityKeyPair::generate();
        let b = IdentityKeyPair::generate();
        let ad = x3dh_associated_data(&a.public(), &b.public());
        assert_eq!(&ad[..32], a.public().dh_pub.as_bytes());
        assert_eq!(&ad[32..], b.public().dh_pub.as_bytes());
    }
}

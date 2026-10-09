//! High-level convenience: run X3DH + Double Ratchet as one object.
//!
//! `Session` is what an application actually holds per conversation.

use thiserror::Error;

use crate::{
    keys::{IdentityKeyPair, IdentityPublic, OneTimePreKey, PreKeyBundle, SignedPreKey},
    ratchet::{DoubleRatchet, RatchetError},
    wire::{Envelope, InitialMessage},
    x3dh::{x3dh_associated_data, x3dh_initiator, x3dh_responder, X3DHError},
};

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("X3DH failed: {0}")]
    X3DH(#[from] X3DHError),
    #[error("ratchet failed: {0}")]
    Ratchet(#[from] RatchetError),
}

/// A live E2EE session between exactly two identities.
///
/// The X3DH associated-data blob is fixed for the lifetime of the
/// session and mixed into every ciphertext's AAD.
///
/// `Serialize`/`Deserialize` are intended for encrypted-at-rest
/// persistence only (see `aqua-client-core::vault`); never send a
/// serialized session over the wire.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Session {
    ratchet: DoubleRatchet,
    ad: Vec<u8>,
    /// Peer's long-term identity. Kept for UI display / key verification.
    pub peer: IdentityPublic,
}

impl Session {
    /// Start a new session as the initiator, given the peer's prekey
    /// bundle.
    ///
    /// Returns the session plus an [`InitialMessage`] to hand to the
    /// server for delivery. The initial message already contains the
    /// first plaintext, AEAD-encrypted.
    pub fn start_as_initiator(
        own_identity: &IdentityKeyPair,
        peer_bundle: &PreKeyBundle,
        first_plaintext: &[u8],
    ) -> Result<(Self, InitialMessage), SessionError> {
        let init = x3dh_initiator(own_identity, peer_bundle)?;
        let ad = x3dh_associated_data(&own_identity.public(), &peer_bundle.identity).to_vec();

        let mut ratchet = DoubleRatchet::init_initiator(
            init.shared_secret,
            peer_bundle.signed_prekey.dh_pub,
        )?;
        let (header, ciphertext) = ratchet.encrypt(first_plaintext, &ad)?;

        let initial = InitialMessage {
            sender_identity: own_identity.public(),
            sender_ephemeral: init.ephemeral_pub,
            used_opk_id: init.used_opk_id,
            header,
            ciphertext,
        };

        Ok((
            Self {
                ratchet,
                ad,
                peer: peer_bundle.identity,
            },
            initial,
        ))
    }

    /// Accept an incoming [`InitialMessage`] as the responder. Consumes
    /// the matching one-time prekey (if any). Returns the new session
    /// plus the decrypted first plaintext.
    pub fn accept_initial(
        own_identity: &IdentityKeyPair,
        own_signed_prekey: SignedPreKey,
        matching_opk: Option<OneTimePreKey>,
        initial: &InitialMessage,
    ) -> Result<(Self, Vec<u8>), SessionError> {
        let sk = x3dh_responder(
            own_identity,
            &own_signed_prekey,
            matching_opk.as_ref(),
            &initial.sender_identity,
            &initial.sender_ephemeral,
        )?;
        let ad = x3dh_associated_data(&initial.sender_identity, &own_identity.public()).to_vec();

        // The ratchet consumes the SPK secret as its first DHs.
        let mut ratchet = DoubleRatchet::init_responder(sk, own_signed_prekey.secret);
        let plaintext = ratchet.decrypt(&initial.header, &initial.ciphertext, &ad)?;

        Ok((
            Self {
                ratchet,
                ad,
                peer: initial.sender_identity,
            },
            plaintext,
        ))
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Envelope, SessionError> {
        let (header, ciphertext) = self.ratchet.encrypt(plaintext, &self.ad)?;
        Ok(Envelope { header, ciphertext })
    }

    pub fn decrypt(&mut self, envelope: &Envelope) -> Result<Vec<u8>, SessionError> {
        Ok(self.ratchet.decrypt(&envelope.header, &envelope.ciphertext, &self.ad)?)
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{OneTimePreKey, PreKeyBundle, SignedPreKey};

    fn setup() -> (
        IdentityKeyPair,
        IdentityKeyPair,
        SignedPreKey,
        OneTimePreKey,
        PreKeyBundle,
    ) {
        let alice = IdentityKeyPair::generate();
        let bob = IdentityKeyPair::generate();
        let bob_spk = SignedPreKey::generate(1, &bob);
        let bob_opk = OneTimePreKey::generate(1);
        let bundle = PreKeyBundle {
            identity: bob.public(),
            signed_prekey: bob_spk.public(),
            one_time_prekey: Some(bob_opk.public()),
        };
        (alice, bob, bob_spk, bob_opk, bundle)
    }

    #[test]
    fn end_to_end_conversation() {
        let (alice, bob, bob_spk, bob_opk, bundle) = setup();

        let (mut a_sess, initial) =
            Session::start_as_initiator(&alice, &bundle, b"hi bob!").unwrap();
        let (mut b_sess, first) =
            Session::accept_initial(&bob, bob_spk, Some(bob_opk), &initial).unwrap();
        assert_eq!(first, b"hi bob!");
        assert_eq!(b_sess.peer.dh_pub, alice.public().dh_pub);
        assert_eq!(a_sess.peer.dh_pub, bob.public().dh_pub);

        // Ten-turn conversation, alternating.
        for i in 0..10 {
            let sent = format!("A#{i}");
            let env = a_sess.encrypt(sent.as_bytes()).unwrap();
            assert_eq!(b_sess.decrypt(&env).unwrap(), sent.as_bytes());

            let sent = format!("B#{i}");
            let env = b_sess.encrypt(sent.as_bytes()).unwrap();
            assert_eq!(a_sess.decrypt(&env).unwrap(), sent.as_bytes());
        }
    }

    #[test]
    fn wrong_opk_id_is_still_handled_if_server_lied() {
        // If the server delivers a bundle claiming an OPK but we hand
        // the responder the wrong OPK secret, decryption must fail
        // rather than silently succeed with a different shared secret.
        let (alice, bob, bob_spk, bob_opk, bundle) = setup();
        let (_a_sess, initial) =
            Session::start_as_initiator(&alice, &bundle, b"hi").unwrap();
        // Replace the OPK we hand Bob with an unrelated one.
        let wrong = OneTimePreKey::generate(1);
        let _ = bob_opk; // consumed conceptually
        let res = Session::accept_initial(&bob, bob_spk, Some(wrong), &initial);
        assert!(res.is_err(), "must not decrypt with wrong OPK secret");
    }

    #[test]
    fn envelope_serde_roundtrip() {
        let (alice, bob, bob_spk, bob_opk, bundle) = setup();
        let (mut a_sess, initial) =
            Session::start_as_initiator(&alice, &bundle, b"hi").unwrap();
        let (mut b_sess, _first) =
            Session::accept_initial(&bob, bob_spk, Some(bob_opk), &initial).unwrap();

        let env = a_sess.encrypt(b"round-trip me").unwrap();
        let bytes = bincode::serialize(&env).unwrap();
        let env2: Envelope = bincode::deserialize(&bytes).unwrap();
        assert_eq!(b_sess.decrypt(&env2).unwrap(), b"round-trip me");
    }
}

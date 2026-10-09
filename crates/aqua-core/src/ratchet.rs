//! The Double Ratchet.
//!
//! Follows Trevor Perrin & Moxie Marlinspike's specification, using
//! X25519 for the DH ratchet, HKDF-SHA256 for the root ratchet,
//! HMAC-SHA256 for the symmetric chain ratchet, and ChaCha20-Poly1305
//! for message encryption.
//!
//! Key ideas the implementation preserves:
//!
//! * **Forward secrecy**: each message key is derived from a chain key
//!   that is immediately replaced; the message key itself is dropped
//!   after use. Compromising a device tomorrow does not decrypt today's
//!   messages.
//! * **Post-compromise security**: whenever the peer sends a message
//!   with a new DH public key, a fresh Diffie-Hellman is mixed into the
//!   root key. Even if an attacker briefly captured the state, they
//!   lose the ability to decrypt as soon as one honest DH ratchet
//!   happens.
//! * **Out-of-order tolerance**: skipped message keys are kept in a
//!   bounded cache so late-arriving messages can still be decrypted.
//!   The cache is capped at [`MAX_SKIP`] entries; anything beyond is
//!   dropped and later attempts to decrypt that message fail explicitly.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::Zeroize;

use crate::crypto::{
    aead_decrypt, aead_encrypt, hkdf_sha256, hmac_sha256, CryptoError, DhPublic, DhSecret,
    SharedSecret, AEAD_KEY_LEN, AEAD_NONCE_LEN, HMAC_TAG_LEN,
};

// ---------- Constants --------------------------------------------------------

const ROOT_INFO: &[u8] = b"Aquamarine/RootKDF v1";
const MSG_KEY_INFO: &[u8] = b"Aquamarine/MsgKey v1";

/// Byte used inside HMAC to derive the message key from a chain key.
const CK_MSG_BYTE: u8 = 0x01;
/// Byte used inside HMAC to advance the chain key.
const CK_NEXT_BYTE: u8 = 0x02;

/// Default upper bound on how many message keys we're willing to skip
/// forward in a single chain. Prevents an attacker from forcing us to
/// derive an unbounded number of keys by claiming a huge message number.
pub const MAX_SKIP: u32 = 1000;

// ---------- Errors -----------------------------------------------------------

#[derive(Debug, Error)]
pub enum RatchetError {
    #[error("would need to skip {requested} message keys, MAX_SKIP is {max}")]
    TooManySkipped { requested: u32, max: u32 },
    #[error("message key for (dh, n={n}) already used or evicted from skip cache")]
    NoMessageKey { n: u32 },
    #[error("crypto error: {0}")]
    Crypto(#[from] CryptoError),
    #[error("first-message state is inconsistent (no receiving chain established yet)")]
    NoReceivingChain,
}

// ---------- Wire header ------------------------------------------------------

/// Per-message header. Sent in the clear alongside the ciphertext.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Header {
    /// Sender's current DH ratchet public key.
    pub dh_pub: DhPublic,
    /// Length of the sender's previous sending chain (so receiver knows
    /// how many trailing messages of that chain to skip if any are
    /// missing).
    pub pn: u32,
    /// Message number in the current sending chain.
    pub n: u32,
}

impl Header {
    /// Encode as a compact byte string for use as AEAD associated data
    /// (binding the ciphertext to its header).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(32 + 4 + 4);
        out.extend_from_slice(self.dh_pub.as_bytes());
        out.extend_from_slice(&self.pn.to_be_bytes());
        out.extend_from_slice(&self.n.to_be_bytes());
        out
    }
}

// ---------- Ratchet state ----------------------------------------------------

/// A live Double Ratchet session. Not `Clone` on purpose — cloning would
/// duplicate the sending chain and let an attacker who compromises one
/// copy derive keys that another copy has already discarded.
///
/// `Serialize`/`Deserialize` are intended for encrypted-at-rest
/// persistence only (see `aqua-client-core::vault`); never send a
/// serialized ratchet over the wire.
#[derive(Debug, Serialize, Deserialize)]
pub struct DoubleRatchet {
    root_key: [u8; 32],

    dhs: DhSecret,
    dhr: Option<DhPublic>,

    ck_send: Option<[u8; HMAC_TAG_LEN]>,
    ck_recv: Option<[u8; HMAC_TAG_LEN]>,

    n_send: u32,
    n_recv: u32,
    pn: u32,

    /// Skipped message keys, keyed by (peer_dh_pub, n).
    /// LinkedHashMap would give us FIFO eviction; a plain HashMap plus a
    /// separate insertion queue is enough for now.
    skipped: HashMap<(DhPublic, u32), [u8; HMAC_TAG_LEN]>,
    skipped_order: Vec<(DhPublic, u32)>,
    max_skip: u32,
    max_cache: usize,
}

impl Drop for DoubleRatchet {
    fn drop(&mut self) {
        self.root_key.zeroize();
        if let Some(mut ck) = self.ck_send.take() {
            ck.zeroize();
        }
        if let Some(mut ck) = self.ck_recv.take() {
            ck.zeroize();
        }
        for (_, mut mk) in self.skipped.drain() {
            mk.zeroize();
        }
    }
}

impl DoubleRatchet {
    /// Initialize the ratchet on the party that ran X3DH as initiator.
    /// `peer_initial_dh_pub` is the responder's signed-prekey public,
    /// which acts as the peer's first ratchet public.
    pub fn init_initiator(
        shared_secret: SharedSecret,
        peer_initial_dh_pub: DhPublic,
    ) -> Result<Self, RatchetError> {
        let dhs = DhSecret::generate();
        let dh_out = dhs.dh(&peer_initial_dh_pub);
        let (root_key, ck_send) = kdf_rk(shared_secret.as_bytes(), &dh_out)?;
        Ok(Self {
            root_key,
            dhs,
            dhr: Some(peer_initial_dh_pub),
            ck_send: Some(ck_send),
            ck_recv: None,
            n_send: 0,
            n_recv: 0,
            pn: 0,
            skipped: HashMap::new(),
            skipped_order: Vec::new(),
            max_skip: MAX_SKIP,
            max_cache: MAX_SKIP as usize,
        })
    }

    /// Initialize the ratchet on the responder. `own_initial_dh` is the
    /// party's signed-prekey secret; the ratchet consumes it as the
    /// first sending DH keypair. On the first receive, the peer's DH
    /// public will be extracted from the header and a DH ratchet step
    /// will bootstrap `ck_recv`.
    pub fn init_responder(shared_secret: SharedSecret, own_initial_dh: DhSecret) -> Self {
        let mut rk = [0u8; 32];
        rk.copy_from_slice(shared_secret.as_bytes());
        Self {
            root_key: rk,
            dhs: own_initial_dh,
            dhr: None,
            ck_send: None,
            ck_recv: None,
            n_send: 0,
            n_recv: 0,
            pn: 0,
            skipped: HashMap::new(),
            skipped_order: Vec::new(),
            max_skip: MAX_SKIP,
            max_cache: MAX_SKIP as usize,
        }
    }

    /// Encrypt one message.
    pub fn encrypt(
        &mut self,
        plaintext: &[u8],
        associated_data: &[u8],
    ) -> Result<(Header, Vec<u8>), RatchetError> {
        let ck = self.ck_send.as_mut().ok_or(RatchetError::NoReceivingChain)?;
        let (mk, next_ck) = advance_chain(ck);
        *ck = next_ck;

        let header = Header {
            dh_pub: self.dhs.public(),
            pn: self.pn,
            n: self.n_send,
        };
        self.n_send = self.n_send.wrapping_add(1);

        let mut aad = header.to_bytes();
        aad.extend_from_slice(associated_data);

        let (aead_key, nonce) = derive_message_key(&mk)?;
        let ct = aead_encrypt(&aead_key, &nonce, &aad, plaintext)?;
        Ok((header, ct))
    }

    /// Decrypt one message. Handles DH ratchet steps and out-of-order
    /// delivery transparently.
    pub fn decrypt(
        &mut self,
        header: &Header,
        ciphertext: &[u8],
        associated_data: &[u8],
    ) -> Result<Vec<u8>, RatchetError> {
        // 1) If we have a stashed key from a previous skip, try it first.
        let key_id = (header.dh_pub, header.n);
        if let Some(mut mk) = self.skipped.remove(&key_id) {
            self.skipped_order.retain(|k| k != &key_id);
            let out = decrypt_with_mk(&mk, header, ciphertext, associated_data);
            mk.zeroize();
            return out;
        }

        // 2) If the peer has advanced their DH ratchet, do a DH step.
        let is_new_peer_dh = match &self.dhr {
            Some(current) => current != &header.dh_pub,
            None => true,
        };
        if is_new_peer_dh {
            // Skip any leftover messages in the previous chain, up to
            // header.pn.
            self.skip_message_keys(header.pn)?;
            self.dh_ratchet_step(header.dh_pub)?;
        }

        // 3) Skip forward inside the current receiving chain if needed.
        self.skip_message_keys(header.n)?;

        // 4) Derive the message key for this exact position.
        let ck = self.ck_recv.as_mut().ok_or(RatchetError::NoReceivingChain)?;
        let (mut mk, next_ck) = advance_chain(ck);
        *ck = next_ck;
        self.n_recv = self.n_recv.wrapping_add(1);

        let out = decrypt_with_mk(&mk, header, ciphertext, associated_data);
        mk.zeroize();
        out
    }

    // -------- internals -----------------------------------------------------

    /// Advance the receiving chain up to (but not including) `until_n`,
    /// stashing each derived message key in the skip cache so late
    /// messages can still be decrypted.
    fn skip_message_keys(&mut self, until_n: u32) -> Result<(), RatchetError> {
        let ck = match &mut self.ck_recv {
            Some(c) => c,
            None => return Ok(()), // Nothing to skip; first message of a chain.
        };
        if until_n < self.n_recv {
            return Ok(()); // Already past this point.
        }
        let to_skip = until_n - self.n_recv;
        if to_skip > self.max_skip {
            return Err(RatchetError::TooManySkipped {
                requested: to_skip,
                max: self.max_skip,
            });
        }
        let dhr = match self.dhr {
            Some(k) => k,
            None => return Ok(()),
        };
        for _ in 0..to_skip {
            let (mk, next_ck) = advance_chain(ck);
            *ck = next_ck;
            let id = (dhr, self.n_recv);
            self.n_recv = self.n_recv.wrapping_add(1);
            // Evict oldest if we're at capacity.
            if self.skipped.len() >= self.max_cache {
                if let Some(oldest) = self.skipped_order.first().cloned() {
                    self.skipped_order.remove(0);
                    if let Some(mut old_mk) = self.skipped.remove(&oldest) {
                        old_mk.zeroize();
                    }
                }
            }
            self.skipped.insert(id, mk);
            self.skipped_order.push(id);
        }
        Ok(())
    }

    fn dh_ratchet_step(&mut self, new_peer_dh: DhPublic) -> Result<(), RatchetError> {
        self.pn = self.n_send;
        self.n_send = 0;
        self.n_recv = 0;
        self.dhr = Some(new_peer_dh);

        // Receiving side of the new root ratchet.
        let dh_recv = self.dhs.dh(&new_peer_dh);
        let (new_rk, new_ckr) = kdf_rk(&self.root_key, &dh_recv)?;
        self.root_key = new_rk;
        if let Some(mut old) = self.ck_recv.take() {
            old.zeroize();
        }
        self.ck_recv = Some(new_ckr);

        // Rotate our sending DH and derive a fresh sending chain.
        self.dhs = DhSecret::generate();
        let dh_send = self.dhs.dh(&new_peer_dh);
        let (new_rk, new_cks) = kdf_rk(&self.root_key, &dh_send)?;
        self.root_key = new_rk;
        if let Some(mut old) = self.ck_send.take() {
            old.zeroize();
        }
        self.ck_send = Some(new_cks);

        Ok(())
    }
}

// ---------- KDF helpers ------------------------------------------------------

/// Root key derivation: mixes a DH output into the current root key,
/// producing a new root key and a new chain key.
fn kdf_rk(
    rk: &[u8],
    dh_out: &SharedSecret,
) -> Result<([u8; 32], [u8; HMAC_TAG_LEN]), CryptoError> {
    let out = hkdf_sha256(rk, dh_out.as_bytes(), ROOT_INFO, 64)?;
    let mut new_rk = [0u8; 32];
    let mut new_ck = [0u8; HMAC_TAG_LEN];
    new_rk.copy_from_slice(&out[..32]);
    new_ck.copy_from_slice(&out[32..]);
    Ok((new_rk, new_ck))
}

/// Symmetric chain ratchet: produces the next message key and next chain
/// key from the current chain key.
fn advance_chain(ck: &[u8; HMAC_TAG_LEN]) -> ([u8; HMAC_TAG_LEN], [u8; HMAC_TAG_LEN]) {
    let mk = hmac_sha256(ck, &[CK_MSG_BYTE]);
    let next_ck = hmac_sha256(ck, &[CK_NEXT_BYTE]);
    (mk, next_ck)
}

fn derive_message_key(
    mk: &[u8; HMAC_TAG_LEN],
) -> Result<([u8; AEAD_KEY_LEN], [u8; AEAD_NONCE_LEN]), CryptoError> {
    let out = hkdf_sha256(&[0u8; 32], mk, MSG_KEY_INFO, AEAD_KEY_LEN + AEAD_NONCE_LEN)?;
    let mut aead_key = [0u8; AEAD_KEY_LEN];
    let mut nonce = [0u8; AEAD_NONCE_LEN];
    aead_key.copy_from_slice(&out[..AEAD_KEY_LEN]);
    nonce.copy_from_slice(&out[AEAD_KEY_LEN..]);
    Ok((aead_key, nonce))
}

fn decrypt_with_mk(
    mk: &[u8; HMAC_TAG_LEN],
    header: &Header,
    ciphertext: &[u8],
    associated_data: &[u8],
) -> Result<Vec<u8>, RatchetError> {
    let (aead_key, nonce) = derive_message_key(mk)?;
    let mut aad = header.to_bytes();
    aad.extend_from_slice(associated_data);
    Ok(aead_decrypt(&aead_key, &nonce, &aad, ciphertext)?)
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        keys::{IdentityKeyPair, OneTimePreKey, PreKeyBundle, SignedPreKey},
        x3dh::{x3dh_associated_data, x3dh_initiator, x3dh_responder},
    };

    /// Build a full X3DH-derived pair of ratchets for Alice and Bob.
    /// Returns (alice_dr, bob_dr, ad).
    fn establish_session() -> (DoubleRatchet, DoubleRatchet, Vec<u8>) {
        let alice_id = IdentityKeyPair::generate();
        let bob_id = IdentityKeyPair::generate();
        let bob_spk = SignedPreKey::generate(1, &bob_id);
        let bob_opk = OneTimePreKey::generate(1);
        let bundle = PreKeyBundle {
            identity: bob_id.public(),
            signed_prekey: bob_spk.public(),
            one_time_prekey: Some(bob_opk.public()),
        };

        let init = x3dh_initiator(&alice_id, &bundle).unwrap();
        let bob_sk = x3dh_responder(
            &bob_id,
            &bob_spk,
            Some(&bob_opk),
            &alice_id.public(),
            &init.ephemeral_pub,
        )
        .unwrap();
        assert_eq!(init.shared_secret.as_bytes(), bob_sk.as_bytes());

        let ad = x3dh_associated_data(&alice_id.public(), &bob_id.public()).to_vec();

        let alice_dr = DoubleRatchet::init_initiator(
            init.shared_secret,
            bob_spk.public().dh_pub,
        )
        .unwrap();
        // Bob's ratchet consumes his SPK secret. Move it in.
        let bob_dr = DoubleRatchet::init_responder(bob_sk, bob_spk.secret);
        (alice_dr, bob_dr, ad)
    }

    #[test]
    fn simple_send_and_receive() {
        let (mut alice, mut bob, ad) = establish_session();
        let (h, ct) = alice.encrypt(b"hello bob", &ad).unwrap();
        let pt = bob.decrypt(&h, &ct, &ad).unwrap();
        assert_eq!(pt, b"hello bob");
    }

    #[test]
    fn full_ping_pong() {
        let (mut alice, mut bob, ad) = establish_session();
        for i in 0..10 {
            let msg = format!("a->b #{i}");
            let (h, ct) = alice.encrypt(msg.as_bytes(), &ad).unwrap();
            assert_eq!(bob.decrypt(&h, &ct, &ad).unwrap(), msg.as_bytes());

            let reply = format!("b->a #{i}");
            let (h, ct) = bob.encrypt(reply.as_bytes(), &ad).unwrap();
            assert_eq!(alice.decrypt(&h, &ct, &ad).unwrap(), reply.as_bytes());
        }
    }

    #[test]
    fn out_of_order_within_chain() {
        let (mut alice, mut bob, ad) = establish_session();
        let mut msgs = Vec::new();
        for i in 0..5 {
            let (h, ct) = alice.encrypt(format!("msg {i}").as_bytes(), &ad).unwrap();
            msgs.push((h, ct));
        }
        // Deliver 2, 4, 0, 3, 1
        for i in [2, 4, 0, 3, 1] {
            let (h, ct) = &msgs[i];
            let pt = bob.decrypt(h, ct, &ad).unwrap();
            assert_eq!(pt, format!("msg {i}").as_bytes());
        }
    }

    #[test]
    fn out_of_order_across_dh_ratchets() {
        let (mut alice, mut bob, ad) = establish_session();

        // Alice sends 3 messages (chain 1).
        let mut alice_chain1 = Vec::new();
        for i in 0..3 {
            alice_chain1.push(alice.encrypt(format!("a1-{i}").as_bytes(), &ad).unwrap());
        }
        // Bob receives msg 0 only (triggers DH ratchet on Bob's side).
        let (h0, ct0) = &alice_chain1[0];
        assert_eq!(bob.decrypt(h0, ct0, &ad).unwrap(), b"a1-0");
        // Bob replies (starts a new chain from Bob).
        let (hb, ctb) = bob.encrypt(b"b-reply", &ad).unwrap();
        // Alice receives Bob's reply — DH ratchet on Alice, new chains.
        assert_eq!(alice.decrypt(&hb, &ctb, &ad).unwrap(), b"b-reply");
        // Alice sends chain-2 messages.
        let a2_0 = alice.encrypt(b"a2-0", &ad).unwrap();
        let a2_1 = alice.encrypt(b"a2-1", &ad).unwrap();
        // Bob now receives a2-1 first, then the two lost chain-1 messages,
        // then a2-0.
        assert_eq!(bob.decrypt(&a2_1.0, &a2_1.1, &ad).unwrap(), b"a2-1");
        // The two lost chain-1 messages should still decrypt via skipped keys.
        for i in [1, 2] {
            let (h, ct) = &alice_chain1[i];
            let pt = bob.decrypt(h, ct, &ad).unwrap();
            assert_eq!(pt, format!("a1-{i}").as_bytes());
        }
        // And the earlier a2-0.
        assert_eq!(bob.decrypt(&a2_0.0, &a2_0.1, &ad).unwrap(), b"a2-0");
    }

    #[test]
    fn tampered_ciphertext_rejected() {
        let (mut alice, mut bob, ad) = establish_session();
        let (h, mut ct) = alice.encrypt(b"secret", &ad).unwrap();
        ct[0] ^= 0x01;
        assert!(bob.decrypt(&h, &ct, &ad).is_err());
    }

    #[test]
    fn replay_after_delivery_fails() {
        let (mut alice, mut bob, ad) = establish_session();
        let (h, ct) = alice.encrypt(b"once", &ad).unwrap();
        bob.decrypt(&h, &ct, &ad).unwrap();
        // Second delivery: message key was consumed, so decrypt must fail.
        // (Either NoMessageKey or Crypto, depending on state; either way not Ok.)
        assert!(bob.decrypt(&h, &ct, &ad).is_err());
    }

    #[test]
    fn wrong_associated_data_fails() {
        let (mut alice, mut bob, ad) = establish_session();
        let (h, ct) = alice.encrypt(b"secret", &ad).unwrap();
        let mut bad_ad = ad.clone();
        bad_ad[0] ^= 0xff;
        assert!(bob.decrypt(&h, &ct, &bad_ad).is_err());
    }

    #[test]
    fn max_skip_enforced() {
        let (mut alice, mut bob, ad) = establish_session();
        // Alice sends many messages; we'll only deliver the last one.
        let n = MAX_SKIP + 1;
        let mut last = None;
        for i in 0..=n {
            let (h, ct) = alice.encrypt(format!("m{i}").as_bytes(), &ad).unwrap();
            if i == n {
                last = Some((h, ct));
            }
        }
        let (h, ct) = last.unwrap();
        let err = bob.decrypt(&h, &ct, &ad).unwrap_err();
        assert!(matches!(err, RatchetError::TooManySkipped { .. }));
    }
}

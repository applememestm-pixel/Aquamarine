//! Prekey lifecycle: rotation of the signed prekey and top-up of the
//! one-time prekey pool.
//!
//! Policy (see the constants below):
//!
//! * **Signed prekey** rotates every 7 days. On the first check with an
//!   empty vault, a fresh SPK is generated too.
//! * **One-time prekeys** are kept above `OPK_MIN_POOL` local secrets.
//!   When the count drops below that, we generate up to `OPK_TARGET`
//!   fresh OPKs and hand the publics off to the caller for upload.
//!
//! This module never talks to the network itself. `check_and_refresh`
//! returns a [`RefreshOutcome`] describing what changed; the transport
//! layer is responsible for pushing the resulting publics to the
//! server.

use std::time::{Duration, SystemTime};

use aqua_core::{
    keys::{
        IdentityKeyPair, OneTimePreKey, OneTimePreKeyPublic, SignedPreKey, SignedPreKeyPublic,
    },
};
use thiserror::Error;

use crate::vault::{Vault, VaultError};

// ---------- Policy ---------------------------------------------------------

/// Rotate the signed prekey once per week.
pub const SPK_ROTATION_PERIOD: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// If the local pool of unused OPK secrets falls below this, refill.
pub const OPK_MIN_POOL: usize = 10;

/// Refill target when we do refill.
pub const OPK_TARGET: usize = 50;

// ---------- Errors ---------------------------------------------------------

#[derive(Debug, Error)]
pub enum PrekeyError {
    #[error("vault: {0}")]
    Vault(#[from] VaultError),
}

// ---------- Outcome --------------------------------------------------------

/// What `check_and_refresh` did on this call. Everything here should be
/// pushed to the server (a) atomically if possible, (b) in this order:
/// SPK first, then OPKs — so the server's bundle is never missing an
/// SPK.
#[derive(Debug, Default, Clone)]
pub struct RefreshOutcome {
    /// A new signed prekey was generated. Publish its public half.
    pub new_signed_prekey: Option<SignedPreKeyPublic>,
    /// One-time prekeys generated to refill the pool. Publish these.
    pub new_one_time_prekeys: Vec<OneTimePreKeyPublic>,
}

impl RefreshOutcome {
    pub fn is_empty(&self) -> bool {
        self.new_signed_prekey.is_none() && self.new_one_time_prekeys.is_empty()
    }
}

// ---------- Manager --------------------------------------------------------

pub struct PrekeyManager<'v> {
    vault: &'v Vault,
    identity: &'v IdentityKeyPair,
}

impl<'v> PrekeyManager<'v> {
    pub fn new(vault: &'v Vault, identity: &'v IdentityKeyPair) -> Self {
        Self { vault, identity }
    }

    /// Bring the local prekey material up to policy for the given wall-
    /// clock `now`. Returns the publics of anything freshly generated.
    /// Safe to call as often as you like; a no-op when everything is
    /// already current.
    pub fn check_and_refresh(&self, now: SystemTime) -> Result<RefreshOutcome, PrekeyError> {
        let mut out = RefreshOutcome::default();

        // -- Signed prekey --
        let needs_new_spk = match self.vault.latest_signed_prekey()? {
            None => true,
            Some((_, generated_at)) => {
                now.duration_since(generated_at)
                    .map(|age| age >= SPK_ROTATION_PERIOD)
                    // If clock went backwards, don't rotate: refuse to
                    // burn a prekey on a machine whose clock is wrong.
                    .unwrap_or(false)
            }
        };
        if needs_new_spk {
            let next_id = self.next_spk_id()?;
            let spk = SignedPreKey::generate(next_id, self.identity);
            let spk_public = spk.public();
            self.vault.save_signed_prekey(&spk, now)?;
            out.new_signed_prekey = Some(spk_public);
        }

        // -- One-time prekeys --
        let count = self.vault.opk_count()?;
        if count < OPK_MIN_POOL {
            let to_make = OPK_TARGET - count;
            let mut next_id = self.vault.next_opk_id()?;
            for _ in 0..to_make {
                let opk = OneTimePreKey::generate(next_id);
                out.new_one_time_prekeys.push(opk.public());
                self.vault.save_one_time_prekey(&opk)?;
                next_id = next_id.saturating_add(1);
            }
        }

        Ok(out)
    }

    /// Consume an incoming OPK reference from an initial message.
    pub fn consume_one_time_prekey(
        &self,
        id: aqua_core::keys::PreKeyId,
    ) -> Result<Option<OneTimePreKey>, PrekeyError> {
        Ok(self.vault.take_one_time_prekey(id)?)
    }

    fn next_spk_id(&self) -> Result<u32, PrekeyError> {
        // SPKs are keyed by monotonically-increasing ids too. The vault
        // stores them under the id we assign, so we look up the max and
        // add one. If there are none yet, start at 1 (the id 0 is
        // reserved for "no prekey" in some serialization corners).
        let latest = self.vault.latest_signed_prekey()?;
        Ok(latest.map(|(spk, _)| spk.id).unwrap_or(0).saturating_add(1))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::{MasterKey, Vault};

    fn setup() -> (Vault, IdentityKeyPair) {
        let vault = Vault::open_in_memory(MasterKey::generate()).unwrap();
        let id = IdentityKeyPair::generate();
        vault.save_identity(&id).unwrap();
        (vault, id)
    }

    #[test]
    fn first_call_generates_spk_and_opks() {
        let (vault, id) = setup();
        let pm = PrekeyManager::new(&vault, &id);
        let out = pm.check_and_refresh(SystemTime::UNIX_EPOCH).unwrap();
        assert!(out.new_signed_prekey.is_some());
        assert_eq!(out.new_one_time_prekeys.len(), OPK_TARGET);
        assert_eq!(vault.opk_count().unwrap(), OPK_TARGET);
    }

    #[test]
    fn second_call_shortly_after_is_noop() {
        let (vault, id) = setup();
        let pm = PrekeyManager::new(&vault, &id);
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        pm.check_and_refresh(t0).unwrap();

        // Same moment: nothing should change.
        let out = pm.check_and_refresh(t0).unwrap();
        assert!(out.is_empty());

        // A day later: still nothing.
        let out = pm.check_and_refresh(t0 + Duration::from_secs(24 * 3600)).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn spk_rotates_after_a_week() {
        let (vault, id) = setup();
        let pm = PrekeyManager::new(&vault, &id);
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let out = pm.check_and_refresh(t0).unwrap();
        let first_spk_id = out.new_signed_prekey.unwrap().id;

        let out = pm.check_and_refresh(t0 + SPK_ROTATION_PERIOD).unwrap();
        assert!(out.new_signed_prekey.is_some(), "should rotate at the boundary");
        assert!(
            out.new_signed_prekey.as_ref().unwrap().id > first_spk_id,
            "new SPK gets a fresh id"
        );

        // Verify the new SPK is signed by our identity.
        out.new_signed_prekey.unwrap().verify(&id.public()).unwrap();
    }

    #[test]
    fn spk_does_not_rotate_if_clock_went_backwards() {
        let (vault, id) = setup();
        let pm = PrekeyManager::new(&vault, &id);
        let t_future = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000_000);
        pm.check_and_refresh(t_future).unwrap();

        // Now a "clock skew" call from the past. Should NOT rotate.
        let out = pm.check_and_refresh(SystemTime::UNIX_EPOCH).unwrap();
        assert!(out.new_signed_prekey.is_none());
    }

    #[test]
    fn opks_refill_when_pool_drops_below_threshold() {
        let (vault, id) = setup();
        let pm = PrekeyManager::new(&vault, &id);
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let out = pm.check_and_refresh(t0).unwrap();
        let first_batch = out.new_one_time_prekeys;
        assert_eq!(first_batch.len(), OPK_TARGET);
        let highest_id = first_batch.iter().map(|o| o.id).max().unwrap();

        // Consume until we drop below MIN_POOL.
        let mut consumed = 0;
        for opk in &first_batch {
            if vault.opk_count().unwrap() < OPK_MIN_POOL {
                break;
            }
            vault.take_one_time_prekey(opk.id).unwrap();
            consumed += 1;
        }
        assert!(vault.opk_count().unwrap() < OPK_MIN_POOL);
        assert!(consumed > 0);

        // Next tick refills to TARGET.
        let out = pm.check_and_refresh(t0).unwrap();
        assert!(!out.new_one_time_prekeys.is_empty());
        assert_eq!(vault.opk_count().unwrap(), OPK_TARGET);

        // Newly-generated OPK ids are all above the previous max — no
        // id collisions with what's still in the pool.
        for opk in &out.new_one_time_prekeys {
            assert!(
                opk.id > highest_id,
                "expected new id {} > previous max {highest_id}",
                opk.id
            );
        }
    }

    #[test]
    fn consume_via_manager_delegates_to_vault() {
        let (vault, id) = setup();
        let pm = PrekeyManager::new(&vault, &id);
        pm.check_and_refresh(SystemTime::UNIX_EPOCH).unwrap();
        let some_id = vault.next_opk_id().unwrap() - 1; // last id we just wrote
        let taken = pm.consume_one_time_prekey(some_id).unwrap();
        assert!(taken.is_some());
        assert!(pm.consume_one_time_prekey(some_id).unwrap().is_none());
    }
}

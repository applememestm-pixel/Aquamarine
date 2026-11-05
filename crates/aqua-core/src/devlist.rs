//! Signed device lists (1.2.0-5): which devices an account really has.
//!
//! The relay keeps every account's device list, and a sender encrypts each
//! message once per device on it. So a relay could quietly add a device of
//! its own and read along. A signed list stops that: the account's own
//! devices sign the list, and senders only encrypt to devices on a list
//! they could check.
//!
//! * A list names its devices by their full public identity (X25519 + the
//!   Ed25519 key that signs).
//! * The first list a sender sees is taken on trust (like the first key of
//!   a contact). Every later one must have a higher version and be signed by
//!   a device that was on the list the sender already accepted — the relay
//!   can't make that signature.
//! * A **reset** (every old device is lost; the account is back with its
//!   recovery key) is accepted, but the sender is told — the same warning as
//!   a changed safety code. A relay that fakes a reset is therefore seen.
//!
//! Wire format (JSON):
//! `{"v":1,"user","ver","at","reset","devices":[{"dh","sign"}],"signer":{"dh","sign"},"sig"}`
//! with keys in hex and the signature over [`SignedDeviceList::signed_bytes`].

use serde::{Deserialize, Serialize};

use crate::crypto::{DhPublic, SignPublic, ED25519_SIG_LEN};
use crate::keys::{IdentityKeyPair, IdentityPublic};

/// One device of the list: its identity keys in hex.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DeviceKey {
    pub dh: String,
    pub sign: String,
}

impl DeviceKey {
    pub fn of(id: &IdentityPublic) -> Self {
        Self { dh: hex::encode(id.dh_pub.as_bytes()), sign: hex::encode(id.sign_pub.as_bytes()) }
    }

    fn sign_pub(&self) -> Option<SignPublic> {
        let b: [u8; 32] = hex::decode(&self.sign).ok()?.try_into().ok()?;
        Some(SignPublic::from_bytes(b))
    }

    /// The short device id the relay uses (first 6 bytes of the DH key).
    pub fn id(&self) -> String {
        self.dh.chars().take(12).collect()
    }

    pub fn dh_pub(&self) -> Option<DhPublic> {
        let b: [u8; 32] = hex::decode(&self.dh).ok()?.try_into().ok()?;
        Some(DhPublic::from_bytes(b))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedDeviceList {
    #[serde(default = "one")]
    pub v: u32,
    pub user: String,
    pub ver: u64,
    /// When it was signed (unix seconds), for people to read.
    #[serde(default)]
    pub at: i64,
    #[serde(default)]
    pub reset: bool,
    pub devices: Vec<DeviceKey>,
    pub signer: DeviceKey,
    pub sig: String,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListError {
    /// The signature doesn't check out, or the keys are malformed.
    BadSignature,
    /// The signer isn't on the list it signed.
    SignerNotListed,
    /// Not newer than the list already accepted.
    Stale,
    /// Signed by a device that wasn't on the accepted list (and it's no reset).
    UnknownSigner,
    /// For another account.
    WrongUser,
    Malformed,
}

impl core::fmt::Display for ListError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            ListError::BadSignature => "the device list's signature doesn't check out",
            ListError::SignerNotListed => "the device list is signed by a device that isn't on it",
            ListError::Stale => "the device list is older than the one already known",
            ListError::UnknownSigner => "the device list is signed by a device the account didn't have",
            ListError::WrongUser => "the device list is for another account",
            ListError::Malformed => "the device list is malformed",
        })
    }
}

/// How a new list was taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accepted {
    /// The first list seen for this account.
    First,
    /// Signed by a device of the previous list.
    Chained,
    /// A reset: all the old devices are gone. Tell the user.
    Reset,
}

impl SignedDeviceList {
    /// What the signature covers.
    pub fn signed_bytes(user: &str, ver: u64, at: i64, reset: bool, devices: &[DeviceKey], signer: &DeviceKey) -> Vec<u8> {
        let mut d: Vec<&DeviceKey> = devices.iter().collect();
        d.sort();
        d.dedup();
        let list: Vec<String> = d.iter().map(|k| format!("{}:{}", k.dh, k.sign)).collect();
        format!(
            "aqua-devlist-v1\n{}\n{ver}\n{at}\n{}\n{}\n{}:{}",
            user.trim().to_lowercase(),
            if reset { 1 } else { 0 },
            list.join(","),
            signer.dh,
            signer.sign
        )
        .into_bytes()
    }

    /// Sign a list with this device's identity (the device must be on it).
    pub fn sign(identity: &IdentityKeyPair, user: &str, ver: u64, at: i64, reset: bool, devices: Vec<DeviceKey>) -> Self {
        let signer = DeviceKey::of(&identity.public());
        let mut devices = devices;
        if !devices.contains(&signer) {
            devices.push(signer.clone());
        }
        devices.sort();
        devices.dedup();
        let user = user.trim().to_lowercase();
        let sig = identity.sign.sign(&Self::signed_bytes(&user, ver, at, reset, &devices, &signer));
        Self { v: 1, user, ver, at, reset, devices, signer, sig: hex::encode(sig) }
    }

    /// The signature is good and the signer is on the list.
    pub fn verify_self(&self) -> Result<(), ListError> {
        if self.devices.is_empty() || self.devices.len() > 64 || self.user.is_empty() {
            return Err(ListError::Malformed);
        }
        if !self.devices.contains(&self.signer) {
            return Err(ListError::SignerNotListed);
        }
        let key = self.signer.sign_pub().ok_or(ListError::Malformed)?;
        let sig: [u8; ED25519_SIG_LEN] = hex::decode(&self.sig).ok().and_then(|b| b.try_into().ok()).ok_or(ListError::Malformed)?;
        key.verify(&Self::signed_bytes(&self.user, self.ver, self.at, self.reset, &self.devices, &self.signer), &sig)
            .map_err(|_| ListError::BadSignature)
    }

    /// May this list replace `prev` (the one already accepted for `user`)?
    pub fn accept(&self, user: &str, prev: Option<&SignedDeviceList>) -> Result<Accepted, ListError> {
        if self.user != user.trim().to_lowercase() {
            return Err(ListError::WrongUser);
        }
        self.verify_self()?;
        let Some(prev) = prev else { return Ok(Accepted::First) };
        if self.ver <= prev.ver {
            return Err(ListError::Stale);
        }
        if prev.devices.contains(&self.signer) {
            return Ok(Accepted::Chained);
        }
        if self.reset {
            return Ok(Accepted::Reset);
        }
        Err(ListError::UnknownSigner)
    }

    /// Is the device with this DH key on the list?
    pub fn has_dh(&self, dh: &[u8]) -> bool {
        let h = hex::encode(dh);
        self.devices.iter().any(|d| d.dh == h)
    }

    /// Is the device with this short id (relay's `user~id`) on the list?
    pub fn has_id(&self, id: &str) -> bool {
        self.devices.iter().any(|d| d.id() == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(i: &IdentityKeyPair) -> DeviceKey {
        DeviceKey::of(&i.public())
    }

    #[test]
    fn a_chain_of_lists_is_accepted_and_a_forged_one_is_not() {
        let phone = IdentityKeyPair::generate();
        let pc = IdentityKeyPair::generate();
        let relay = IdentityKeyPair::generate();

        let l1 = SignedDeviceList::sign(&phone, "Alice", 1, 0, false, vec![k(&phone)]);
        assert_eq!(l1.user, "alice");
        assert_eq!(l1.accept("alice", None), Ok(Accepted::First));

        // The phone approves the PC: version 2, signed by the phone.
        let l2 = SignedDeviceList::sign(&phone, "alice", 2, 0, false, vec![k(&phone), k(&pc)]);
        assert_eq!(l2.accept("alice", Some(&l1)), Ok(Accepted::Chained));
        assert!(l2.has_dh(pc.public().dh_pub.as_bytes()));
        assert!(l2.has_id(&k(&pc).id()));

        // The relay adds its own device and signs with it: refused.
        let fake = SignedDeviceList::sign(&relay, "alice", 3, 0, false, vec![k(&phone), k(&pc), k(&relay)]);
        assert_eq!(fake.accept("alice", Some(&l2)), Err(ListError::UnknownSigner));
        // ...or edits a real list: the signature breaks.
        let mut edited = l2.clone();
        edited.ver = 3;
        edited.devices.push(k(&relay));
        edited.devices.sort();
        assert_eq!(edited.accept("alice", Some(&l2)), Err(ListError::BadSignature));
        // ...or replays an old one.
        assert_eq!(l1.accept("alice", Some(&l2)), Err(ListError::Stale));
        // A list for someone else.
        assert_eq!(l2.accept("bob", None), Err(ListError::WrongUser));

        // A reset is taken, but marked (the user is warned).
        let new_phone = IdentityKeyPair::generate();
        let r = SignedDeviceList::sign(&new_phone, "alice", 3, 0, true, vec![k(&new_phone)]);
        assert_eq!(r.accept("alice", Some(&l2)), Ok(Accepted::Reset));

        // Round trip through JSON.
        let json = serde_json::to_string(&l2).unwrap();
        let back: SignedDeviceList = serde_json::from_str(&json).unwrap();
        assert_eq!(back.accept("alice", Some(&l1)), Ok(Accepted::Chained));
    }
}

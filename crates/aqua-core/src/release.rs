//! Release signatures (AQUA-SEC-012).
//!
//! `SHA256SUMS.txt` lists the hash of every file of a release. Checked only
//! against itself, it proves nothing: whoever can change a file on the
//! release page (or answer instead of the update feed) changes its line too.
//! So the release workflow signs the exact text with an Ed25519 key kept in
//! the `UPDATE_SIGNING_KEY` secret (`SHA256SUMS.txt.sig`: the signature in
//! hex), and the apps check it with the public key built into them.
//!
//! [`UPDATE_PUBKEY`] stays `None` until the owner generates the key pair:
//! with `None` nothing changes (updates are mandatory — a check that refused
//! every unsigned release would stop them all). Setting it is one line.

use crate::crypto::{SignPublic, ED25519_SIG_LEN};

/// The release-signing public key (hex, 32 bytes). `None`: not enabled yet.
pub const UPDATE_PUBKEY: Option<&str> = None;

/// The release asset with the signature over `SHA256SUMS.txt` (hex).
pub const SIG_ASSET: &str = "SHA256SUMS.txt.sig";

/// Whether the sums text may be trusted: always while no key is built in;
/// otherwise only with a valid signature by it over the exact text.
pub fn sums_trusted(sums: &str, sig_hex: Option<&str>) -> bool {
    sums_trusted_with(UPDATE_PUBKEY, sums, sig_hex)
}

/// [`sums_trusted`] with a given key (tests).
pub fn sums_trusted_with(key: Option<&str>, sums: &str, sig_hex: Option<&str>) -> bool {
    let Some(key) = key else { return true };
    let Some(key) = hex::decode(key.trim()).ok().and_then(|b| <[u8; 32]>::try_from(b).ok()) else { return false };
    let Some(sig) = sig_hex.and_then(|s| hex::decode(s.trim()).ok()).and_then(|b| <[u8; ED25519_SIG_LEN]>::try_from(b).ok()) else { return false };
    SignPublic::from_bytes(key).verify(sums.as_bytes(), &sig).is_ok()
}

/// Release downloads come only from this repository's release pages.
pub const DOWNLOAD_PREFIX: &str = "https://github.com/applememestm-pixel/Aquamarine/releases/download/";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::SignSecret;

    #[test]
    fn signed_sums_only_with_the_built_in_key() {
        let k = SignSecret::generate();
        let pk = hex::encode(k.public().as_bytes());
        let sums = "aaaa  app.exe\n";
        let sig = hex::encode(k.sign(sums.as_bytes()));
        assert!(sums_trusted_with(None, sums, None), "not enabled: as before");
        assert!(sums_trusted_with(Some(&pk), sums, Some(&sig)));
        assert!(!sums_trusted_with(Some(&pk), sums, None), "no signature");
        assert!(!sums_trusted_with(Some(&pk), "bbbb  app.exe\n", Some(&sig)), "changed text");
        let other = hex::encode(SignSecret::generate().sign(sums.as_bytes()));
        assert!(!sums_trusted_with(Some(&pk), sums, Some(&other)), "another key");
    }
}

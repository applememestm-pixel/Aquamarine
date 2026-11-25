//! Release signatures (AQUA-SEC-012, REV2-003).
//!
//! `SHA256SUMS.txt` lists the hash of every file of a release. Checked only
//! against itself, it proves nothing: whoever can change a file on the
//! release page (or answer instead of the update feed) changes its line too.
//! So the release workflow signs the exact text with the Ed25519 key in the
//! `UPDATE_SIGNING_KEY` secret (`SHA256SUMS.txt.sig`, the signature in hex),
//! and the apps check it with the public key built into them.
//!
//! **Fail-closed** (owner, 25.11): the public key comes from the build
//! (`AQUA_UPDATE_PUBKEY`, set by release.yml from the repository variable
//! `UPDATE_PUBKEY`; release.yml stops when it's missing). A build without
//! it — a developer build — installs no updates at all, and no release is
//! installed without a valid signature.

use crate::crypto::{SignPublic, ED25519_SIG_LEN};

/// The release-signing public key (hex, 32 bytes) this build trusts.
pub const UPDATE_PUBKEY: Option<&str> = option_env!("AQUA_UPDATE_PUBKEY");

/// The release asset with the signature over `SHA256SUMS.txt` (hex).
pub const SIG_ASSET: &str = "SHA256SUMS.txt.sig";

/// Shown when this build can't check releases.
pub const NO_KEY: &str = "updates are off in this build: it has no release signing key";
/// Shown when a release's sums aren't signed by the release key.
pub const UNSIGNED: &str = "this release isn't signed by the Aquamarine release key — the update wasn't installed";

/// This build has a well-formed release key (updates can be checked).
pub fn signing_enabled() -> bool {
    UPDATE_PUBKEY.and_then(parse_key).is_some()
}

fn parse_key(k: &str) -> Option<[u8; 32]> {
    hex::decode(k.trim()).ok().and_then(|b| <[u8; 32]>::try_from(b).ok())
}

/// The sums text may be trusted: only with a valid signature by the built-in
/// key over the exact text. No key, no signature, a bad one: `false`.
pub fn sums_trusted(sums: &str, sig_hex: Option<&str>) -> bool {
    sums_trusted_with(UPDATE_PUBKEY, sums, sig_hex)
}

/// [`sums_trusted`] with a given key (tests).
pub fn sums_trusted_with(key: Option<&str>, sums: &str, sig_hex: Option<&str>) -> bool {
    let Some(key) = key.and_then(parse_key) else { return false };
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
        assert!(!sums_trusted_with(None, sums, Some(&sig)), "no key built in: nothing is trusted (fail-closed)");
        assert!(!sums_trusted_with(Some("zz"), sums, Some(&sig)), "a malformed key trusts nothing");
        assert!(sums_trusted_with(Some(&pk), sums, Some(&sig)));
        assert!(!sums_trusted_with(Some(&pk), sums, None), "no signature");
        assert!(!sums_trusted_with(Some(&pk), "bbbb  app.exe\n", Some(&sig)), "changed text");
        let other = hex::encode(SignSecret::generate().sign(sums.as_bytes()));
        assert!(!sums_trusted_with(Some(&pk), sums, Some(&other)), "another key");
    }
}

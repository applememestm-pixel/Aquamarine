//! Release signing with the REAL repository key (CI: release-signing-check).
//!
//! Runs only when `RELEASE_SIGNING_DIR` points at a folder the check made:
//! release files, their `SHA256SUMS.txt` (sha256sum format) and
//! `SHA256SUMS.txt.sig` signed by tools/release-sign.sh with the secret
//! `UPDATE_SIGNING_KEY` — and this crate built with `AQUA_UPDATE_PUBKEY`
//! from the variable `UPDATE_PUBKEY`. Elsewhere it does nothing.

use aqua_core::release::{signing_enabled, sums_trusted, sums_trusted_with, UPDATE_PUBKEY};

/// The hash `sums` gives for `name` (what every updater does with it).
fn line(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let h = it.next()?;
        (it.next()?.trim_start_matches('*') == name && h.len() == 64).then(|| h.to_ascii_lowercase())
    })
}

#[test]
fn the_repository_key_signs_and_checks_releases() {
    let Ok(dir) = std::env::var("RELEASE_SIGNING_DIR") else { return };
    if dir.is_empty() { return; }
    let dir = std::path::Path::new(&dir);
    assert!(signing_enabled(), "built with AQUA_UPDATE_PUBKEY (vars.UPDATE_PUBKEY)");
    let sums = std::fs::read_to_string(dir.join("SHA256SUMS.txt")).unwrap();
    let sig = std::fs::read_to_string(dir.join("SHA256SUMS.txt.sig")).unwrap();

    // 1. A correct release: signature good, every file matches its line.
    assert!(sums_trusted(&sums, Some(&sig)), "the secret's signature checks with the built-in public key");
    for name in ["Aquamarine_test_x64-setup.exe", "Aquamarine_test.apk"] {
        let bytes = std::fs::read(dir.join(name)).unwrap();
        assert_eq!(line(&sums, name).as_deref(), Some(aqua_core::hex_sha256(&bytes).as_str()), "{name}");
    }

    // 2. A swapped file (sums untouched): its hash no longer matches.
    let mut swapped = std::fs::read(dir.join("Aquamarine_test.apk")).unwrap();
    swapped[0] ^= 1;
    assert_ne!(line(&sums, "Aquamarine_test.apk").as_deref(), Some(aqua_core::hex_sha256(&swapped).as_str()));

    // 3. Sums changed to fit a swapped file: the signature no longer fits.
    let forged = sums.replace(&line(&sums, "Aquamarine_test.apk").unwrap(), &aqua_core::hex_sha256(&swapped));
    assert!(!sums_trusted(&forged, Some(&sig)), "edited sums are refused");

    // 4. A wrong signature: one changed digit, another key, none.
    let mut bad: Vec<char> = sig.trim().chars().collect();
    bad[10] = if bad[10] == '0' { '1' } else { '0' };
    assert!(!sums_trusted(&sums, Some(&bad.iter().collect::<String>())));
    let other = aqua_core::crypto::SignSecret::generate();
    assert!(!sums_trusted(&sums, Some(&hex::encode(other.sign(sums.as_bytes())))), "another key's signature");
    assert!(!sums_trusted(&sums, None), "no signature");

    // 5. No public key in the build: nothing is trusted (fail-closed);
    // a build with another key refuses this release.
    assert!(!sums_trusted_with(None, &sums, Some(&sig)));
    let other_pub = hex::encode(other.public().as_bytes());
    assert!(!sums_trusted_with(Some(&other_pub), &sums, Some(&sig)));

    // Only the public key goes to the log.
    println!("release key (public): {}", UPDATE_PUBKEY.unwrap_or(""));
}

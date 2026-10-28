//! Two-step verification: time-based one-time codes (TOTP, RFC 6238) — the
//! 6-digit codes of Google Authenticator, Aegis, 2FAS, Bitwarden… — and
//! one-time backup codes for when the phone with the app is gone.
//!
//! The relay keeps the secret (it has to, to check the codes) and asks for a
//! code when the account signs in on a new device.

use hmac::{Hmac, Mac};
use sha2::Digest;

/// 30-second steps, 6 digits, HMAC-SHA1: what every authenticator app does.
const STEP: u64 = 30;
const DIGITS: u32 = 6;
/// How many backup codes a set has.
pub const BACKUP_CODES: usize = 10;

const B32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// RFC 4648 base32, no padding (what `otpauth://` URIs use).
pub fn base32(bytes: &[u8]) -> String {
    let mut out = String::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for &b in bytes {
        buf = (buf << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(B32[((buf >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(B32[((buf << (5 - bits)) & 31) as usize] as char);
    }
    out
}

pub fn unbase32(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in s.chars().filter(|c| !c.is_whitespace() && *c != '=') {
        let v = B32.iter().position(|&b| b as char == c.to_ascii_uppercase())? as u32;
        buf = (buf << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

/// A new secret: 20 random bytes, base32.
pub fn new_secret() -> String {
    let mut b = [0u8; 20];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut b);
    base32(&b)
}

/// The code for time step `step`.
pub fn code_at(secret: &[u8], step: u64) -> u32 {
    let mut mac = <Hmac<sha1::Sha1> as Mac>::new_from_slice(secret).expect("any key length");
    mac.update(&step.to_be_bytes());
    let h = mac.finalize().into_bytes();
    let off = (h[h.len() - 1] & 0x0f) as usize;
    let n = u32::from_be_bytes([h[off] & 0x7f, h[off + 1], h[off + 2], h[off + 3]]);
    n % 10u32.pow(DIGITS)
}

pub fn now_step() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) / STEP
}

/// Check `code` against `secret` (base32) at the current step ±1 (clocks
/// drift). Returns the matching step, which must be newer than `last` — a
/// code works once.
pub fn verify(secret_b32: &str, code: &str, last: u64) -> Option<u64> {
    let secret = unbase32(secret_b32)?;
    let code: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
    if code.len() != DIGITS as usize {
        return None;
    }
    let want: u32 = code.parse().ok()?;
    let now = now_step();
    let mut hit = None;
    for step in [now.saturating_sub(1), now, now + 1] {
        // Compare every candidate (no early exit on the first match).
        if subtle::ConstantTimeEq::ct_eq(&code_at(&secret, step).to_be_bytes()[..], &want.to_be_bytes()[..]).into() && step > last {
            hit = Some(step);
        }
    }
    hit
}

/// `otpauth://` link for the authenticator app (shown as a QR code).
pub fn uri(secret_b32: &str, account: &str, issuer: &str) -> String {
    let enc = |s: &str| form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>().replace('+', "%20");
    format!(
        "otpauth://totp/{}:{}?secret={secret_b32}&issuer={}&algorithm=SHA1&digits={DIGITS}&period={STEP}",
        enc(issuer),
        enc(account),
        enc(issuer)
    )
}

/// A fresh set of backup codes: (what the person writes down, what is stored).
pub fn new_backup_codes() -> (Vec<String>, Vec<String>) {
    const ABC: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut shown = Vec::new();
    let mut stored = Vec::new();
    for _ in 0..BACKUP_CODES {
        let mut b = [0u8; 8];
        rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut b);
        let s: String = b.iter().map(|x| ABC[*x as usize % ABC.len()] as char).collect();
        stored.push(backup_hash(&s));
        shown.push(format!("{}-{}", &s[..4], &s[4..]));
    }
    (shown, stored)
}

/// What is kept of a backup code: its SHA-256 (dashes, spaces, case ignored).
pub fn backup_hash(code: &str) -> String {
    let norm: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect();
    hex::encode(sha2::Sha256::new().chain_update(b"aqua-2fa-backup\0").chain_update(norm.as_bytes()).finalize())
}

/// Does `code` look like a backup code (8 letters/digits) rather than a TOTP?
pub fn looks_like_backup(code: &str) -> bool {
    let n: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    n.len() == 8 && n.chars().any(|c| c.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc6238_sha1_vectors() {
        // RFC 6238 appendix B, SHA-1, last 6 digits.
        let key = b"12345678901234567890";
        assert_eq!(code_at(key, 59 / 30), 287082);
        assert_eq!(code_at(key, 1111111109 / 30), 81804);
        assert_eq!(code_at(key, 1234567890 / 30), 5924);
        assert_eq!(code_at(key, 2000000000 / 30), 279037);
    }

    #[test]
    fn base32_round_trip() {
        assert_eq!(base32(b"foobar"), "MZXW6YTBOI");
        assert_eq!(unbase32("mzxw 6ytb oi").unwrap(), b"foobar");
        let s = new_secret();
        assert_eq!(s.len(), 32);
        assert_eq!(unbase32(&s).unwrap().len(), 20);
    }

    #[test]
    fn a_code_works_once_and_backup_codes_hash_loosely() {
        let s = new_secret();
        let code = format!("{:06}", code_at(&unbase32(&s).unwrap(), now_step()));
        let step = verify(&s, &code, 0).expect("current code");
        assert!(verify(&s, &code, step).is_none(), "the same code twice");
        assert!(verify(&s, "12345", 0).is_none());
        let (shown, stored) = new_backup_codes();
        assert_eq!(shown.len(), BACKUP_CODES);
        assert_eq!(backup_hash(&shown[0].to_uppercase().replace('-', " ")), stored[0]);
        assert!(looks_like_backup(&shown[3]));
        assert!(!looks_like_backup("123 456"));
        assert!(uri(&s, "vlad", "Aquamarine").starts_with("otpauth://totp/Aquamarine:vlad?secret="));
    }
}

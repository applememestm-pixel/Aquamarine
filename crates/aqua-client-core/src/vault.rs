//! Encrypted-at-rest local storage for identity, sessions, and prekeys.
//!
//! Every row is stored as `(nonce, ciphertext)` where `ciphertext =
//! ChaCha20-Poly1305(master_key, nonce, bincode(record))`. The SQLite
//! file on disk therefore reveals only the *shape* of the data
//! (row counts, timestamps) — not the contents, and not any key
//! material.
//!
//! The master key is 32 bytes. In production it should come from the OS
//! keyring (Windows DPAPI, macOS Keychain, Linux Secret Service); the
//! `keyring` crate wires that up in one call. For tests and headless
//! use, callers can pass an explicit key.
//!
//! Schema (see [`Vault::migrate`]):
//!
//! * `identity`         — the local `IdentityKeyPair` (singleton row).
//! * `sessions`         — one row per peer conversation.
//! * `signed_prekeys`   — history of SPKs, most recent one is "active".
//! * `one_time_prekeys` — unused OPKs. Consumed on incoming initial
//!                        message.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use aqua_core::{
    keys::{IdentityKeyPair, OneTimePreKey, PreKeyId, SignedPreKey},
    session::Session,
};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Nonce,
};
use rand_core::{OsRng, RngCore};
use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;
use zeroize::ZeroizeOnDrop;

// ---------- Constants -------------------------------------------------------

/// Length of the master key used to wrap on-disk records.
pub const MASTER_KEY_LEN: usize = 32;

const NONCE_LEN: usize = 12;

/// Domain-separation tag mixed into every AAD. Bumping this invalidates
/// all older on-disk records (useful when the vault format changes).
const AAD_PREFIX: &[u8] = b"Aquamarine/Vault/v1:";

// ---------- Errors ----------------------------------------------------------

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("SQLite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("serialize: {0}")]
    Bincode(#[from] bincode::Error),
    #[error(
        "vault entry could not be decrypted (wrong master key, or ciphertext tampered)"
    )]
    Decrypt,
    #[error("vault entry could not be encrypted")]
    Encrypt,
    #[error("record is malformed (unexpected byte length)")]
    Malformed,
}

// ---------- Master key wrapper ---------------------------------------------

/// A 32-byte master key used to wrap on-disk records. Zeroized on drop.
#[derive(Clone, ZeroizeOnDrop)]
pub struct MasterKey(pub [u8; MASTER_KEY_LEN]);

impl MasterKey {
    /// Generate a fresh random master key. Store it in the OS keyring.
    pub fn generate() -> Self {
        let mut k = [0u8; MASTER_KEY_LEN];
        OsRng.fill_bytes(&mut k);
        MasterKey(k)
    }

    pub fn from_bytes(bytes: [u8; MASTER_KEY_LEN]) -> Self {
        MasterKey(bytes)
    }
}

impl core::fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("MasterKey(<redacted>)")
    }
}

// ---------- Vault -----------------------------------------------------------

/// Handle to a local encrypted store.
///
/// One vault per user account per device. Not `Send` across threads by
/// design of the underlying `rusqlite::Connection` (add a Mutex on the
/// caller side if you need cross-thread access).
pub struct Vault {
    conn: Connection,
    key: MasterKey,
}

impl core::fmt::Debug for Vault {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Vault").finish_non_exhaustive()
    }
}

impl Vault {
    /// Open (or create) the vault at `path`. `master_key` must be the
    /// same one used on prior calls for this file; a different key
    /// makes every existing row un-decryptable.
    pub fn open<P: AsRef<Path>>(path: P, master_key: MasterKey) -> Result<Self, VaultError> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let vault = Self { conn, key: master_key };
        vault.migrate()?;
        Ok(vault)
    }

    /// Open a vault backed by RAM only. Handy for tests.
    pub fn open_in_memory(master_key: MasterKey) -> Result<Self, VaultError> {
        let conn = Connection::open_in_memory()?;
        let vault = Self { conn, key: master_key };
        vault.migrate()?;
        Ok(vault)
    }

    fn migrate(&self) -> Result<(), VaultError> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS identity (
                id         INTEGER PRIMARY KEY CHECK (id = 1),
                nonce      BLOB    NOT NULL,
                ciphertext BLOB    NOT NULL
            );

            CREATE TABLE IF NOT EXISTS sessions (
                peer_id    TEXT    PRIMARY KEY,
                nonce      BLOB    NOT NULL,
                ciphertext BLOB    NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS signed_prekeys (
                id           INTEGER PRIMARY KEY,
                nonce        BLOB    NOT NULL,
                ciphertext   BLOB    NOT NULL,
                generated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS one_time_prekeys (
                id         INTEGER PRIMARY KEY,
                nonce      BLOB    NOT NULL,
                ciphertext BLOB    NOT NULL
            );
            "#,
        )?;
        Ok(())
    }

    // ---------- Identity --------------------------------------------------

    pub fn save_identity(&self, identity: &IdentityKeyPair) -> Result<(), VaultError> {
        let (nonce, ct) = self.seal(b"identity", &bincode::serialize(identity)?)?;
        self.conn.execute(
            "INSERT INTO identity (id, nonce, ciphertext) VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET nonce = excluded.nonce, ciphertext = excluded.ciphertext",
            params![nonce, ct],
        )?;
        Ok(())
    }

    pub fn load_identity(&self) -> Result<Option<IdentityKeyPair>, VaultError> {
        let row = self
            .conn
            .query_row("SELECT nonce, ciphertext FROM identity WHERE id = 1", [], |row| {
                Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .optional()?;
        match row {
            None => Ok(None),
            Some((nonce, ct)) => {
                let pt = self.unseal(b"identity", &nonce, &ct)?;
                Ok(Some(bincode::deserialize(&pt)?))
            }
        }
    }

    // ---------- Sessions --------------------------------------------------

    pub fn save_session(&self, peer_id: &str, session: &Session) -> Result<(), VaultError> {
        let aad = session_aad(peer_id);
        let (nonce, ct) = self.seal(&aad, &bincode::serialize(session)?)?;
        let now = unix_now();
        self.conn.execute(
            "INSERT INTO sessions (peer_id, nonce, ciphertext, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(peer_id) DO UPDATE SET
                 nonce = excluded.nonce,
                 ciphertext = excluded.ciphertext,
                 updated_at = excluded.updated_at",
            params![peer_id, nonce, ct, now],
        )?;
        Ok(())
    }

    pub fn load_session(&self, peer_id: &str) -> Result<Option<Session>, VaultError> {
        let row = self
            .conn
            .query_row(
                "SELECT nonce, ciphertext FROM sessions WHERE peer_id = ?1",
                params![peer_id],
                |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )
            .optional()?;
        match row {
            None => Ok(None),
            Some((nonce, ct)) => {
                let aad = session_aad(peer_id);
                let pt = self.unseal(&aad, &nonce, &ct)?;
                Ok(Some(bincode::deserialize(&pt)?))
            }
        }
    }

    pub fn delete_session(&self, peer_id: &str) -> Result<(), VaultError> {
        self.conn
            .execute("DELETE FROM sessions WHERE peer_id = ?1", params![peer_id])?;
        Ok(())
    }

    pub fn list_session_peer_ids(&self) -> Result<Vec<String>, VaultError> {
        let mut stmt = self
            .conn
            .prepare("SELECT peer_id FROM sessions ORDER BY updated_at DESC")?;
        let rows: Result<Vec<String>, rusqlite::Error> =
            stmt.query_map([], |row| row.get::<_, String>(0))?.collect();
        Ok(rows?)
    }

    // ---------- Signed prekeys -------------------------------------------

    pub fn save_signed_prekey(
        &self,
        spk: &SignedPreKey,
        generated_at: SystemTime,
    ) -> Result<(), VaultError> {
        let aad = spk_aad(spk.id);
        let (nonce, ct) = self.seal(&aad, &bincode::serialize(spk)?)?;
        let ts = generated_at
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.conn.execute(
            "INSERT INTO signed_prekeys (id, nonce, ciphertext, generated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
                 nonce = excluded.nonce,
                 ciphertext = excluded.ciphertext,
                 generated_at = excluded.generated_at",
            params![spk.id, nonce, ct, ts],
        )?;
        Ok(())
    }

    pub fn load_signed_prekey(&self, id: PreKeyId) -> Result<Option<SignedPreKey>, VaultError> {
        let row = self
            .conn
            .query_row(
                "SELECT nonce, ciphertext FROM signed_prekeys WHERE id = ?1",
                params![id],
                |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )
            .optional()?;
        match row {
            None => Ok(None),
            Some((nonce, ct)) => {
                let aad = spk_aad(id);
                let pt = self.unseal(&aad, &nonce, &ct)?;
                Ok(Some(bincode::deserialize(&pt)?))
            }
        }
    }

    /// The most recent SPK plus its generation timestamp, if any.
    pub fn latest_signed_prekey(
        &self,
    ) -> Result<Option<(SignedPreKey, SystemTime)>, VaultError> {
        let row = self
            .conn
            .query_row(
                "SELECT id, nonce, ciphertext, generated_at
                 FROM signed_prekeys
                 ORDER BY generated_at DESC LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, u32>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?;
        match row {
            None => Ok(None),
            Some((id, nonce, ct, ts)) => {
                let aad = spk_aad(id);
                let pt = self.unseal(&aad, &nonce, &ct)?;
                let spk: SignedPreKey = bincode::deserialize(&pt)?;
                let gen = UNIX_EPOCH + std::time::Duration::from_secs(ts as u64);
                Ok(Some((spk, gen)))
            }
        }
    }

    // ---------- One-time prekeys -----------------------------------------

    pub fn save_one_time_prekey(&self, opk: &OneTimePreKey) -> Result<(), VaultError> {
        let aad = opk_aad(opk.id);
        let (nonce, ct) = self.seal(&aad, &bincode::serialize(opk)?)?;
        self.conn.execute(
            "INSERT INTO one_time_prekeys (id, nonce, ciphertext) VALUES (?1, ?2, ?3)",
            params![opk.id, nonce, ct],
        )?;
        Ok(())
    }

    /// Look up and remove a one-time prekey by id. Removal is atomic;
    /// the same id cannot be handed out twice.
    pub fn take_one_time_prekey(
        &self,
        id: PreKeyId,
    ) -> Result<Option<OneTimePreKey>, VaultError> {
        let tx = self.conn.unchecked_transaction()?;
        let row = tx
            .query_row(
                "SELECT nonce, ciphertext FROM one_time_prekeys WHERE id = ?1",
                params![id],
                |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )
            .optional()?;
        let out = match row {
            None => None,
            Some((nonce, ct)) => {
                let aad = opk_aad(id);
                let pt = self.unseal(&aad, &nonce, &ct)?;
                let opk: OneTimePreKey = bincode::deserialize(&pt)?;
                tx.execute("DELETE FROM one_time_prekeys WHERE id = ?1", params![id])?;
                Some(opk)
            }
        };
        tx.commit()?;
        Ok(out)
    }

    pub fn opk_count(&self) -> Result<usize, VaultError> {
        let n: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM one_time_prekeys", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// Enumerate the public halves of all one-time prekeys currently in
    /// the pool, without consuming them. Used at server-connect time to
    /// synchronise the server's OPK stock with what the client has.
    pub fn list_one_time_prekey_publics(
        &self,
    ) -> Result<Vec<aqua_core::keys::OneTimePreKeyPublic>, VaultError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, nonce, ciphertext FROM one_time_prekeys ORDER BY id")?;
        let rows: Vec<Result<(u32, Vec<u8>, Vec<u8>), rusqlite::Error>> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, u32>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            })?
            .collect();
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let (id, nonce, ct) = row?;
            let aad = opk_aad(id);
            let pt = self.unseal(&aad, &nonce, &ct)?;
            let opk: aqua_core::keys::OneTimePreKey = bincode::deserialize(&pt)?;
            out.push(opk.public());
        }
        Ok(out)
    }

    pub fn next_opk_id(&self) -> Result<PreKeyId, VaultError> {
        let n: Option<i64> = self.conn.query_row(
            "SELECT MAX(id) FROM one_time_prekeys",
            [],
            |r| r.get::<_, Option<i64>>(0),
        )?;
        Ok(n.map(|v| v as u32).unwrap_or(0).saturating_add(1))
    }

    // ---------- AEAD internals -------------------------------------------

    fn seal(&self, aad: &[u8], plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>), VaultError> {
        let mut nonce = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce);
        let cipher = ChaCha20Poly1305::new((&self.key.0).into());
        let mut full_aad = Vec::with_capacity(AAD_PREFIX.len() + aad.len());
        full_aad.extend_from_slice(AAD_PREFIX);
        full_aad.extend_from_slice(aad);
        let ct = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload { msg: plaintext, aad: &full_aad },
            )
            .map_err(|_| VaultError::Encrypt)?;
        Ok((nonce.to_vec(), ct))
    }

    fn unseal(&self, aad: &[u8], nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, VaultError> {
        if nonce.len() != NONCE_LEN {
            return Err(VaultError::Malformed);
        }
        let cipher = ChaCha20Poly1305::new((&self.key.0).into());
        let mut full_aad = Vec::with_capacity(AAD_PREFIX.len() + aad.len());
        full_aad.extend_from_slice(AAD_PREFIX);
        full_aad.extend_from_slice(aad);
        cipher
            .decrypt(
                Nonce::from_slice(nonce),
                Payload { msg: ciphertext, aad: &full_aad },
            )
            .map_err(|_| VaultError::Decrypt)
    }
}

// ---------- Helpers ---------------------------------------------------------

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn session_aad(peer_id: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(9 + peer_id.len());
    out.extend_from_slice(b"session:");
    out.extend_from_slice(peer_id.as_bytes());
    out
}

fn spk_aad(id: PreKeyId) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 4);
    out.extend_from_slice(b"spk:");
    out.extend_from_slice(&id.to_be_bytes());
    out
}

fn opk_aad(id: PreKeyId) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 4);
    out.extend_from_slice(b"opk:");
    out.extend_from_slice(&id.to_be_bytes());
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use aqua_core::{
        keys::{IdentityKeyPair, OneTimePreKey, PreKeyBundle, SignedPreKey},
        session::Session,
    };

    fn fresh_key() -> MasterKey {
        MasterKey::generate()
    }

    #[test]
    fn identity_roundtrip() {
        let vault = Vault::open_in_memory(fresh_key()).unwrap();
        let id = IdentityKeyPair::generate();
        let id_pub = id.public();
        vault.save_identity(&id).unwrap();

        let loaded = vault.load_identity().unwrap().expect("identity round-trips");
        assert_eq!(loaded.public().sign_pub, id_pub.sign_pub);
        assert_eq!(loaded.public().dh_pub, id_pub.dh_pub);
    }

    #[test]
    fn wrong_master_key_cannot_decrypt() {
        let key1 = MasterKey::generate();
        let key2 = MasterKey::generate();

        // We need the same on-disk file for this test; use a temp dir.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.sqlite");

        {
            let v = Vault::open(&path, key1).unwrap();
            v.save_identity(&IdentityKeyPair::generate()).unwrap();
        }
        let v2 = Vault::open(&path, key2).unwrap();
        let err = v2.load_identity().unwrap_err();
        assert!(matches!(err, VaultError::Decrypt), "got {err:?}");
    }

    #[test]
    fn signed_prekey_and_latest() {
        let vault = Vault::open_in_memory(fresh_key()).unwrap();
        let id = IdentityKeyPair::generate();
        let spk_old = SignedPreKey::generate(1, &id);
        let spk_new = SignedPreKey::generate(2, &id);
        let t_old = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000);
        let t_new = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(9_000);
        vault.save_signed_prekey(&spk_old, t_old).unwrap();
        vault.save_signed_prekey(&spk_new, t_new).unwrap();

        let (latest, ts) = vault.latest_signed_prekey().unwrap().unwrap();
        assert_eq!(latest.id, 2);
        assert_eq!(ts, t_new);

        // Also verify the signature still validates after roundtrip.
        latest.public().verify(&id.public()).unwrap();
    }

    #[test]
    fn one_time_prekey_take_is_atomic() {
        let vault = Vault::open_in_memory(fresh_key()).unwrap();
        let opk = OneTimePreKey::generate(7);
        vault.save_one_time_prekey(&opk).unwrap();
        assert_eq!(vault.opk_count().unwrap(), 1);

        let taken = vault.take_one_time_prekey(7).unwrap().unwrap();
        assert_eq!(taken.id, 7);
        assert_eq!(vault.opk_count().unwrap(), 0);
        // Second take returns None.
        assert!(vault.take_one_time_prekey(7).unwrap().is_none());
    }

    #[test]
    fn session_survives_restart_and_keeps_decrypting() {
        // Boot two identities and get to the point where Bob has a
        // live Session — then close the vault, reopen, and confirm the
        // reloaded Session decrypts fresh messages Alice sends.
        let alice = IdentityKeyPair::generate();
        let bob = IdentityKeyPair::generate();
        let bob_spk = SignedPreKey::generate(1, &bob);
        let bob_opk = OneTimePreKey::generate(1);

        let bundle = PreKeyBundle {
            identity: bob.public(),
            signed_prekey: bob_spk.public(),
            one_time_prekey: Some(bob_opk.public()),
        };

        let (mut a_sess, initial) =
            Session::start_as_initiator(&alice, &bundle, b"hi").unwrap();
        let (b_sess, first) =
            Session::accept_initial(&bob, bob_spk, Some(bob_opk), &initial).unwrap();
        assert_eq!(first, b"hi");

        // Persist Bob's session, then simulate restart.
        let key = MasterKey::generate();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bob.vault");
        {
            let v = Vault::open(&path, key.clone()).unwrap();
            v.save_session("alice", &b_sess).unwrap();
        }
        // (`b_sess` is dropped here — like a real restart.)

        // Alice keeps sending on her side. Bob reopens vault and picks up.
        let v = Vault::open(&path, key).unwrap();
        let mut b_sess = v.load_session("alice").unwrap().expect("session present");

        for i in 0..5 {
            let msg = format!("post-restart #{i}");
            let env = a_sess.encrypt(msg.as_bytes()).unwrap();
            assert_eq!(b_sess.decrypt(&env).unwrap(), msg.as_bytes());
        }

        // Now Bob replies and Alice decrypts — proves the DH ratchet
        // state survived serialization intact.
        let env = b_sess.encrypt(b"hi from restarted bob").unwrap();
        assert_eq!(a_sess.decrypt(&env).unwrap(), b"hi from restarted bob");

        // And save the newly-advanced state back.
        v.save_session("alice", &b_sess).unwrap();
        assert_eq!(v.list_session_peer_ids().unwrap(), vec!["alice"]);
    }

    #[test]
    fn tampered_ciphertext_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v.sqlite");
        let key = MasterKey::generate();
        {
            let v = Vault::open(&path, key.clone()).unwrap();
            v.save_identity(&IdentityKeyPair::generate()).unwrap();
        }

        // Flip a byte in the identity ciphertext.
        let conn = Connection::open(&path).unwrap();
        conn.execute(
            "UPDATE identity SET ciphertext = ?1 WHERE id = 1",
            params![vec![0xFFu8; 96]],
        )
        .unwrap();
        drop(conn);

        let v = Vault::open(&path, key).unwrap();
        assert!(matches!(v.load_identity().unwrap_err(), VaultError::Decrypt));
    }
}

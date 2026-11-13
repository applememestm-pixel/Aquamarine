//! Writing small state files so a crash can't leave half of one (1.20.1-6).
//!
//! `std::fs::write` truncates the file first: when the app is killed in the
//! middle (Android does that freely, a power cut, a crash), the file is
//! empty or cut — and these files read back as "nothing": `account.json`
//! as logged out, `history-sync.json` without its tombstones (deleted
//! messages came back), the device trust list empty, the account pointer
//! back to the first account. Here the new content goes to a temporary file
//! next to it, is flushed to disk and only then renamed over the old one.

use std::io::Write;
use std::path::Path;

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let res = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

#[cfg(test)]
mod tests {
    #[test]
    fn replaces_whole() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.json");
        super::write_atomic(&p, b"{\"x\":1}").unwrap();
        super::write_atomic(&p, b"{}").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"{}");
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1, "no temporary file left");
    }
}

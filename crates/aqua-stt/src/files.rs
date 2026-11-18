//! What the app downloads, pinned: a file whose SHA-256 differs from the
//! one here is never saved, so a swapped file (the release, the network, a
//! proxy) is never loaded. Made by tools/stt_assets.py (its "=====RUST"
//! block) — a new model or runtime means a new "stt-N" tag and new lines here.

/// The release the files are in: github.com/<repo>/releases/download/<tag>/<name>.
pub const RELEASE_TAG: &str = "stt-1";

pub struct FileSpec {
    pub name: &'static str,
    /// SHA-256, lowercase hex.
    pub sha256: &'static str,
    pub size: u64,
}

const fn f(name: &'static str, sha256: &'static str, size: u64) -> FileSpec {
    FileSpec { name, sha256, size }
}

pub const FILES: &[FileSpec] = &[
    // filled from the stt assets run
];

/// This platform's ONNX Runtime library among [`FILES`].
pub(crate) const RUNTIME: Option<&str> = if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
    Some("ort-windows-x64.dll")
} else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
    Some("ort-macos-arm64.dylib")
} else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
    Some("ort-macos-x64.dylib")
} else if cfg!(all(target_os = "android", target_arch = "aarch64")) {
    Some("ort-android-arm64.so")
} else if cfg!(all(target_os = "android", target_arch = "arm")) {
    Some("ort-android-armv7.so")
} else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    Some("ort-linux-x64.so")
} else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
    Some("ort-linux-arm64.so")
} else {
    None
};

pub fn runtime_file() -> Option<&'static str> {
    RUNTIME
}

#[allow(dead_code)]
const _: fn(&'static str, &'static str, u64) -> FileSpec = f;

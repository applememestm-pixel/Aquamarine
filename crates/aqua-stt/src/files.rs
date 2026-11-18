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
    f("ort-android-arm64.so", "e40f09d07dc53726b8bfbf48a7907673b8f86718a057655a62790a39874a7302", 19347616),
    f("ort-android-armv7.so", "57048b8d54896d16355ee367bfc129c5925468ae503b681b8d0cd49ceefa468e", 13992328),
    f("ort-linux-arm64.so", "648ffa64fbe027ae27139109410900cf776a030dec2dbbac51053318cc44c286", 18693384),
    f("ort-linux-x64.so", "13ab8084954fa4a47c777880180b90810d6020f021441395712b48a75b74c68b", 22326072),
    f("ort-macos-arm64.dylib", "d306d2bc768540766c7ed8a1e0ff05d2870c77a934ebeee4a7bafa1b732ef299", 35138784),
    f("ort-macos-x64.dylib", "8c9c78de65ea3786f987c0d980e9c1b13a3a5fbc6b3e2965ba05b450e6e4c054", 39742608),
    f("ort-windows-x64.dll", "dec964ab1ee36cc9b0ae247d13b376627992fc57dec0454354017ab8fd84f1ea", 14186016),
    f("stt-example.pcm", "84449ca372cd7169d9b94778c547e6934bce6cb4c1ef29ad06692b29c0611569", 158074),
    f("stt-example.txt", "bae4712b1b3a3fae9525ed2a58dde4b018596b454715689ecca663058767fa49", 34),
    f("stt-long_example.pcm", "3a26e4eea84f4e3ec584479c0c2e588ecb09164321bba822617118e935041f17", 649368),
    f("stt-model.onnx", "2e3fcb7a7b66030336fd10c2fcfb033bd1dc7e1bf238fe5cfd83b1d0cfc9d28e", 224893347),
    f("stt-pre.onnx", "919c3bc8a434b6e733e5fe5dcd247974d0a7b1f2bc5b6995b6ad55f7225e014d", 44430),
    f("stt-vocab.txt", "142de7570b3de5b3035ce111a89c228e80e6085273731d944093ddf24fa539cd", 2007),
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

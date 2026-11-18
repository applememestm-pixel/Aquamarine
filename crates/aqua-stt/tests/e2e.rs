//! The whole pipeline against the reference made by onnx-asr (tools/stt_assets.py):
//! needs the stt release's files in `AQUA_STT_DIR` (CI: checks.yml "stt e2e");
//! without it the test does nothing.

#[cfg(not(target_os = "ios"))]
#[test]
fn transcribes_like_onnx_asr() {
    use sha2::{Digest, Sha256};
    let Some(dir) = std::env::var_os("AQUA_STT_DIR").map(std::path::PathBuf::from) else {
        eprintln!("AQUA_STT_DIR not set — skipped");
        return;
    };
    // Every file is the pinned one (what the app would accept).
    for f in aqua_stt::FILES.iter().filter(|f| dir.join(f.name).is_file()) {
        let got = hex(&Sha256::digest(std::fs::read(dir.join(f.name)).unwrap()));
        assert_eq!(got, f.sha256, "{} differs from files.rs", f.name);
    }
    let pcm = |name: &str| -> Vec<f32> {
        std::fs::read(dir.join(name)).unwrap().chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect()
    };
    let mut r = aqua_stt::Recognizer::load(&dir, 2).expect("load");
    let want = std::fs::read_to_string(dir.join("stt-example.txt")).unwrap();
    let got = r.transcribe(&pcm("stt-example.pcm")).unwrap();
    eprintln!("short: {got:?} (onnx-asr: {want:?})");
    assert_eq!(got.trim(), want.trim());
    // Over 20 s: cut in a pause, recognized in pieces.
    let long = r.transcribe(&pcm("stt-long_example.pcm")).unwrap();
    eprintln!("long: {long:?}");
    assert!(long.split_whitespace().count() >= 4, "too little text from the long recording: {long:?}");
}

#[cfg(not(target_os = "ios"))]
fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

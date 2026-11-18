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
    // Same pipeline, but not the same ONNX Runtime build as onnx-asr's (and
    // int8 math on a synthetic voice is borderline): a letter may differ.
    let d = edit_distance(got.trim(), want.trim());
    assert!(d * 10 <= want.trim().chars().count().max(10), "too far from onnx-asr ({d} edits): {got:?} vs {want:?}");
    // Over 20 s: cut in a pause, recognized in pieces without an error. (The
    // synthetic voice is hard for the model: the words themselves aren't checked.)
    let long = r.transcribe(&pcm("stt-long_example.pcm")).unwrap();
    eprintln!("long: {long:?}");
    assert!(aqua_stt::chunks(&pcm("stt-long_example.pcm")).len() >= 2);
    assert!(long.split_whitespace().count() >= 2, "too little text from the long recording: {long:?}");
}

#[cfg(not(target_os = "ios"))]
fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            cur.push((prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

#[cfg(not(target_os = "ios"))]
fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

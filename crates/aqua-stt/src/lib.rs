//! Voice-to-text on the device.
//!
//! GigaAM v3 e2e CTC (Sber, MIT: Russian, with punctuation) run by ONNX
//! Runtime (Microsoft, MIT). Neither is bundled into the app: the first time
//! someone asks for a transcript, the app downloads them once from the
//! project's "stt-N" release (`files.rs` — names, SHA-256, sizes, checked
//! before anything is loaded) into `<data dir>/stt/`. So the app does not
//! grow, transcripts work offline (Bluetooth / Wi-Fi chats too), and no
//! server ever hears a voice message.
//!
//! The pipeline mirrors onnx-asr (tools/stt_assets.py makes the reference
//! transcripts the tests compare with): 16 kHz mono float PCM →
//! preprocessor ONNX (log-mel, 64 bins) → model ONNX (CTC log-probs over 257
//! tokens) → greedy CTC decoding. Audio longer than ~20 s is cut at its
//! quietest moments and recognized piece by piece (GigaAM is trained on
//! short utterances).
//!
//! Decoding the audio (opus / aac in webm, ogg, mp4) is the WebView's job
//! (`decodeAudioData`); this crate takes the samples.

mod files;
pub use files::{runtime_file, FileSpec, FILES, RELEASE_TAG};

/// Sample rate the model takes.
pub const SAMPLE_RATE: usize = 16_000;
/// Longest piece recognized at once.
const MAX_CHUNK: usize = 20 * SAMPLE_RATE;
/// Shortest piece worth cutting off (look for a pause after this).
const MIN_CHUNK: usize = 12 * SAMPLE_RATE;

/// Files this platform needs: the model's and its ONNX Runtime library.
pub fn needed() -> Vec<&'static FileSpec> {
    let rt = runtime_file();
    FILES.iter().filter(|f| !f.name.starts_with("ort-") || Some(f.name) == rt).filter(|f| !f.name.contains("example")).collect()
}

/// Whether voice-to-text can run here at all (iOS can't load a downloaded library).
pub const fn supported() -> bool {
    cfg!(not(target_os = "ios")) && files::RUNTIME.is_some()
}

/// Splits `pcm` into pieces of at most [`MAX_CHUNK`] samples, cutting in the
/// quietest 100 ms between [`MIN_CHUNK`] and [`MAX_CHUNK`] — between words,
/// as a rule, so none is cut in half.
pub fn chunks(pcm: &[f32]) -> Vec<&[f32]> {
    const WIN: usize = SAMPLE_RATE / 10;
    let mut out = Vec::new();
    let mut rest = pcm;
    while rest.len() > MAX_CHUNK {
        let mut best = (f32::MAX, MAX_CHUNK);
        let mut at = MIN_CHUNK;
        while at + WIN <= MAX_CHUNK {
            let e: f32 = rest[at..at + WIN].iter().map(|s| s * s).sum();
            if e < best.0 {
                best = (e, at + WIN / 2);
            }
            at += WIN / 2;
        }
        let (a, b) = rest.split_at(best.1);
        out.push(a);
        rest = b;
    }
    // A tail under 0.3 s is a click, not a word.
    if rest.len() >= SAMPLE_RATE * 3 / 10 || out.is_empty() {
        out.push(rest);
    }
    out
}

/// Greedy CTC: the best token per frame, repeats merged, blanks dropped.
/// `log_probs` is frames × `vocab` (row-major).
pub fn ctc_greedy(log_probs: &[f32], frames: usize, vocab: usize, blank: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut prev = blank;
    for t in 0..frames {
        let row = &log_probs[t * vocab..(t + 1) * vocab];
        let best = row.iter().enumerate().fold((0, f32::MIN), |b, (i, &v)| if v > b.1 { (i, v) } else { b }).0;
        if best != blank && best != prev {
            out.push(best);
        }
        prev = best;
    }
    out
}

/// Tokens to text the way onnx-asr does it (`\A\s|\s\B|(\s)\b`): a word
/// boundary ("▁", already a space here) stays only before a word character —
/// so no space before "," or "." and none at the start.
pub fn join_tokens(tokens: &[&str]) -> String {
    let s: String = tokens.concat();
    let chars: Vec<char> = s.chars().collect();
    let word = |c: Option<&char>| c.is_some_and(|c| c.is_alphanumeric() || *c == '_');
    let mut out = String::with_capacity(s.len());
    for (i, c) in chars.iter().enumerate() {
        if c.is_whitespace() {
            if i == 0 || !word(chars.get(i + 1)) {
                continue;
            }
            out.push(' ');
        } else {
            out.push(*c);
        }
    }
    out.trim().to_string()
}

/// Parses the vocabulary ("<token> <id>" per line; "▁" is a space).
/// Returns the tokens by id and the blank's id.
pub fn parse_vocab(text: &str) -> Result<(Vec<String>, usize), String> {
    let mut v: Vec<(usize, String)> = Vec::new();
    for line in text.lines().filter(|l| !l.is_empty()) {
        let (tok, id) = line.rsplit_once(' ').ok_or("bad vocabulary line")?;
        v.push((id.trim().parse().map_err(|_| "bad vocabulary id")?, tok.replace('\u{2581}', " ")));
    }
    v.sort_by_key(|e| e.0);
    if v.iter().enumerate().any(|(i, e)| e.0 != i) {
        return Err("vocabulary ids have gaps".into());
    }
    let toks: Vec<String> = v.into_iter().map(|e| e.1).collect();
    let blank = toks.iter().position(|t| t == "<blk>").unwrap_or(toks.len().saturating_sub(1));
    Ok((toks, blank))
}

#[cfg(not(target_os = "ios"))]
mod engine;
#[cfg(not(target_os = "ios"))]
pub use engine::Recognizer;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_like_onnx_asr() {
        assert_eq!(join_tokens(&[" при", "вет", ",", " как", " дела", "?"]), "привет, как дела?");
        assert_eq!(join_tokens(&[" ", "а", " ", ".", " б"]), "а. б");
        assert_eq!(join_tokens(&[" «", "да", "»"]), "«да»");
    }

    #[test]
    fn greedy_merges_and_drops_blanks() {
        // vocab 3, blank 2: a a _ a b b _ → a a b
        let f = |i: usize| { let mut r = [0.0f32; 3]; r[i] = 1.0; r };
        let lp: Vec<f32> = [0, 0, 2, 0, 1, 1, 2].iter().flat_map(|&i| f(i)).collect();
        assert_eq!(ctc_greedy(&lp, 7, 3, 2), vec![0, 0, 1]);
    }

    #[test]
    fn vocab() {
        let (t, b) = parse_vocab("<unk> 0\n\u{2581} 1\n. 2\n<blk> 3\n").unwrap();
        assert_eq!(t, vec!["<unk>", " ", ".", "<blk>"]);
        assert_eq!(b, 3);
    }

    #[test]
    fn chunks_cut_in_pauses() {
        // 30 s of "speech" with a silent gap at 15 s → cut there.
        let mut pcm = vec![0.5f32; 30 * SAMPLE_RATE];
        for s in &mut pcm[15 * SAMPLE_RATE..15 * SAMPLE_RATE + SAMPLE_RATE / 5] { *s = 0.0; }
        let c = chunks(&pcm);
        assert_eq!(c.len(), 2);
        assert!((c[0].len() as i64 - (15 * SAMPLE_RATE + SAMPLE_RATE / 10) as i64).abs() < (SAMPLE_RATE / 10) as i64);
        assert_eq!(c.iter().map(|c| c.len()).sum::<usize>(), pcm.len());
        assert_eq!(chunks(&pcm[..5 * SAMPLE_RATE]).len(), 1);
    }
}

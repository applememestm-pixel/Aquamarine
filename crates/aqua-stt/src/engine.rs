//! ONNX Runtime side: loads the downloaded library and the two models.

use std::path::Path;
use std::sync::OnceLock;

use ort::environment::Environment;
use ort::session::{Session, SessionInputValue};
use ort::value::Tensor;

use crate::{chunks, ctc_greedy, join_tokens, parse_vocab, runtime_file, SAMPLE_RATE};

/// The model downsamples features 4×.
const SUBSAMPLING: i64 = 4;

/// ONNX Runtime is loaded once per process, from the first path given.
fn environment(lib: &Path) -> Result<&'static Environment, String> {
    static ENV: OnceLock<Result<Environment, String>> = OnceLock::new();
    ENV.get_or_init(|| {
        ort::init_from(lib)
            .map_err(|e| format!("ONNX Runtime: {e}"))?
            .with_name("aqua-stt")
            .with_telemetry(false)
            .build()
            .map_err(|e| format!("ONNX Runtime: {e}"))
    })
    .as_ref()
    .map_err(Clone::clone)
}

fn err<E: std::fmt::Display>(what: &'static str) -> impl Fn(E) -> String {
    move |e| format!("{what}: {e}")
}

pub struct Recognizer {
    pre: Session,
    model: Session,
    vocab: Vec<String>,
    blank: usize,
}

impl Recognizer {
    /// Loads everything from `dir` (where the app saved [`crate::needed`],
    /// already checked against their SHA-256). `threads`: CPU threads to use.
    pub fn load(dir: &Path, threads: usize) -> Result<Self, String> {
        let lib = runtime_file().ok_or("voice-to-text isn't available on this platform")?;
        let env = environment(&dir.join(lib))?;
        let open = |name: &str| -> Result<Session, String> {
            Session::builder(env)
                .map_err(err("session"))?
                .with_intra_threads(threads.max(1))
                .map_err(err("session"))?
                .commit_from_file(dir.join(name))
                .map_err(err("model"))
        };
        let (vocab, blank) = parse_vocab(&std::fs::read_to_string(dir.join("stt-vocab.txt")).map_err(err("vocabulary"))?)?;
        Ok(Self { pre: open("stt-pre.onnx")?, model: open("stt-model.onnx")?, vocab, blank })
    }

    /// Text of 16 kHz mono samples (−1…1).
    pub fn transcribe(&mut self, pcm: &[f32]) -> Result<String, String> {
        let mut parts = Vec::new();
        for c in chunks(pcm) {
            let t = self.piece(c)?;
            if !t.is_empty() {
                parts.push(t);
            }
        }
        Ok(parts.join(" "))
    }

    fn piece(&mut self, pcm: &[f32]) -> Result<String, String> {
        if pcm.len() < SAMPLE_RATE / 10 {
            return Ok(String::new());
        }
        // Preprocessor: waveform → log-mel features [1, 64, T].
        let n = pcm.len() as i64;
        let wave = Tensor::from_array((vec![1i64, n], pcm.to_vec())).map_err(err("input"))?;
        let lens = Tensor::from_array((vec![1i64], vec![n])).map_err(err("input"))?;
        let (feat_shape, feats, feat_len) = {
            let out = self
                .pre
                .run(vec![("waveforms", SessionInputValue::from(wave)), ("waveforms_lens", SessionInputValue::from(lens))])
                .map_err(err("preprocessor"))?;
            let (shape, data) = out["features"].try_extract_tensor::<f32>().map_err(err("features"))?;
            let (_, l) = out["features_lens"].try_extract_tensor::<i64>().map_err(err("features"))?;
            (shape.iter().copied().collect::<Vec<i64>>(), data.to_vec(), *l.first().ok_or("no feature length")?)
        };
        // Model: features → CTC log-probs [1, T', vocab].
        let feats = Tensor::from_array((feat_shape, feats)).map_err(err("features"))?;
        let flens = Tensor::from_array((vec![1i64], vec![feat_len])).map_err(err("features"))?;
        let out = self
            .model
            .run(vec![("features", SessionInputValue::from(feats)), ("feature_lengths", SessionInputValue::from(flens))])
            .map_err(err("model"))?;
        let (shape, lp) = out["log_probs"].try_extract_tensor::<f32>().map_err(err("model output"))?;
        let (frames, vocab) = match &shape[..] {
            [1, t, v] => (*t as usize, *v as usize),
            s => return Err(format!("unexpected model output shape {s:?}")),
        };
        if vocab > self.vocab.len() {
            return Err("model and vocabulary don't match".into());
        }
        let valid = (((feat_len - 1) / SUBSAMPLING + 1).max(0) as usize).min(frames);
        let ids = ctc_greedy(lp, valid, vocab, self.blank);
        let toks: Vec<&str> = ids.iter().map(|&i| self.vocab[i].as_str()).filter(|t| !t.starts_with('<')).collect();
        Ok(join_tokens(&toks))
    }
}

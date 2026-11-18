#!/usr/bin/env python3
"""Speech-to-text files for the "stt-N" release (CI only: .github/workflows/stt-assets.yml).

The app does not ship the recognizer: the first time someone taps "to text",
it downloads these files once (crates/aqua-stt). This script gathers them in
`out/`, prints each file's SHA-256 (they go into crates/aqua-stt/src/files.rs)
and a reference transcript of a fixed Russian recording made by onnx-asr, the
Python implementation the Rust code mirrors (crates/aqua-stt tests compare).

  model    GigaAM v3 e2e CTC (Sber, MIT; punctuation + normalization), int8
  pre      log-mel preprocessor for GigaAM v3, from onnx-asr (MIT)
  vocab    CTC vocabulary
  ort-*    ONNX Runtime (Microsoft, MIT) shared library per platform
"""
import hashlib, io, json, shutil, subprocess, urllib.request, zipfile
from pathlib import Path

ORT = "1.23.2"  # NuGet Microsoft.ML.OnnxRuntime (the last with macOS Intel) + Maven AAR
MODEL = "gigaam-v3-e2e-ctc"
HF_REPO = "istupakov/gigaam-v3-onnx"

out = Path("out"); out.mkdir(exist_ok=True)
work = Path("work"); work.mkdir(exist_ok=True)


def get(url: str) -> bytes:
    print("GET", url, flush=True)
    with urllib.request.urlopen(url, timeout=300) as r:
        return r.read()


def put(name: str, data: bytes):
    (out / name).write_bytes(data)


# ---------------------------------------------------------------- model
from huggingface_hub import list_repo_files, hf_hub_download  # noqa: E402

files = list_repo_files(HF_REPO)
print("HF files:", files)
cands = [f for f in files if f.startswith("v3_e2e_ctc") and f.endswith(".onnx")]
model_file = next((f for f in cands if "int8" in f), None) or cands[0]
vocab_file = "v3_e2e_ctc_vocab.txt"
put("stt-model.onnx", Path(hf_hub_download(HF_REPO, model_file)).read_bytes())
put("stt-vocab.txt", Path(hf_hub_download(HF_REPO, vocab_file)).read_bytes())
if "config.json" in files:
    print("config.json:", Path(hf_hub_download(HF_REPO, "config.json")).read_text())

import onnx_asr  # noqa: E402
pre_dir = Path(onnx_asr.__file__).parent / "preprocessors" / "data"
shutil.copy(pre_dir / "gigaam_v3.onnx", out / "stt-pre.onnx")

import onnxruntime as rt  # noqa: E402
for f in ("stt-pre.onnx", "stt-model.onnx"):
    s = rt.InferenceSession(str(out / f))
    print(f, "inputs:", [(i.name, i.type, i.shape) for i in s.get_inputs()],
          "outputs:", [(o.name, o.type, o.shape) for o in s.get_outputs()])
vocab = (out / "stt-vocab.txt").read_text(encoding="utf-8").splitlines()
print("vocab:", len(vocab), "head:", vocab[:5], "tail:", vocab[-5:])

# ---------------------------------------------------------------- golden
asr = onnx_asr.load_model(MODEL, quantization="int8" if "int8" in model_file else None)
import numpy as np  # noqa: E402
# Test speech, synthesized here (espeak-ng) so nothing is fetched: the Rust
# tests check they get the same text as onnx-asr — the pipeline, not the
# voice. The long one (> 20 s) goes through the chunking.
SAY = {
    "example": "Привет! Это проверка распознавания голосовых сообщений в Аквамарине.",
    "long_example": " ".join([
        "Сегодня мы проверяем, как приложение превращает длинное голосовое сообщение в текст.",
        "Запись длиннее двадцати секунд режется на части в паузах между словами.",
        "Каждая часть распознаётся отдельно, а потом текст склеивается обратно.",
        "Если всё работает правильно, здесь не будет ни обрывков, ни повторов.",
    ]),
}
for ex, text_in in SAY.items():
    try:
        src = work / f"{ex}.wav"
        subprocess.run(["espeak-ng", "-v", "ru", "-s", "140", "-w", str(src), text_in], check=True)
        # 16 kHz mono s16 — what the app hands the recognizer after decoding.
        subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-i", str(src),
                        "-ac", "1", "-ar", "16000", "-f", "s16le", str(out / f"stt-{ex}.pcm")], check=True)
        pcm = np.frombuffer((out / f"stt-{ex}.pcm").read_bytes(), dtype="<i2").astype(np.float32) / 32768
        print(ex, "seconds:", len(pcm) / 16000)
        if len(pcm) <= 20 * 16000:
            text = asr.recognize(pcm, sample_rate=16000)
            print("golden", ex, ":", text)
            put(f"stt-{ex}.txt", text.encode("utf-8"))
    except Exception as e:  # noqa: BLE001
        print("::warning::fixture", ex, "skipped:", e)

# ---------------------------------------------------------------- runtime
def from_zip(data: bytes, member: str) -> bytes:
    with zipfile.ZipFile(io.BytesIO(data)) as z:
        return z.read(member)


nupkg = get(f"https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime/{ORT}/microsoft.ml.onnxruntime.{ORT}.nupkg")
for out_name, member in [
    ("ort-linux-x64.so", "runtimes/linux-x64/native/libonnxruntime.so"),
    ("ort-linux-arm64.so", "runtimes/linux-arm64/native/libonnxruntime.so"),
    ("ort-macos-arm64.dylib", "runtimes/osx-arm64/native/libonnxruntime.dylib"),
    ("ort-macos-x64.dylib", "runtimes/osx-x64/native/libonnxruntime.dylib"),
    ("ort-windows-x64.dll", "runtimes/win-x64/native/onnxruntime.dll"),
]:
    put(out_name, from_zip(nupkg, member))
aar = get(f"https://repo1.maven.org/maven2/com/microsoft/onnxruntime/onnxruntime-android/{ORT}/onnxruntime-android-{ORT}.aar")
put("ort-android-arm64.so", from_zip(aar, "jni/arm64-v8a/libonnxruntime.so"))
put("ort-android-armv7.so", from_zip(aar, "jni/armeabi-v7a/libonnxruntime.so"))

# ---------------------------------------------------------------- hashes
sums = {}
for p in sorted(out.iterdir()):
    sums[p.name] = (hashlib.sha256(p.read_bytes()).hexdigest(), p.stat().st_size)
    print(f"{p.name:24} {p.stat().st_size:>11} {sums[p.name][0]}")
(work / "sums.json").write_text(json.dumps(sums, indent=1))
print("=====RUST")
for n, (h, s) in sums.items():
    print(f'    ("{n}", "{h}", {s}),')
print("=====END")

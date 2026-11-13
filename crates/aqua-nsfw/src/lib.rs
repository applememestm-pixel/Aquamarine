//! Explicit-image detector: NSFWJS's MobileNetV2 (MIT, see `model/NOTICE.md`)
//! on a small engine written here — no TensorFlow, ONNX or BLAS, nothing to
//! install, the same code on the relay (Linux / Windows / macOS / Android)
//! and in the app (desktop and phones).
//!
//! * The relay looks at sticker packs (they aren't encrypted there) and marks
//!   18+ ones (aqua-server `packs.rs`).
//! * The app looks at photos, GIFs and stickers *on the device*, after it
//!   has decrypted them — nothing leaves it (aqua-client-ui `nsfw.rs`).
//!
//! Five classes: drawing, hentai, neutral, porn, sexy (they add up to 1).
//! One image is ~300 M multiply-adds: 30–80 ms on a computer, 100–250 ms
//! on a phone, one core. Kept fast by: the model's own uint8 weights turned
//! into f32 once (BatchNorm folded in), NHWC loops whose inner loop runs
//! over contiguous channels (the compiler vectorises it), skipping the zeros
//! ReLU leaves (about half of every activation), two buffers reused through
//! the whole network, and big pictures shrunk by area averaging straight
//! from the decoded pixels.

// The kernels take the layer's shape as plain arguments on purpose (hot loops).
#![allow(clippy::too_many_arguments)]

use std::sync::OnceLock;

/// The network input: 224 × 224 RGB in 0..1.
pub const SIZE: usize = 224;

static MODEL: &[u8] = include_bytes!("../model/aqnsfw.bin");

/// What the network sees in a picture (each 0..1, together 1).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Scores {
    pub drawing: f32,
    pub hentai: f32,
    pub neutral: f32,
    pub porn: f32,
    pub sexy: f32,
}

impl Scores {
    /// How explicit, 0..1: porn and hentai fully, "sexy" (underwear,
    /// swimwear) by half.
    pub fn level(&self) -> f32 {
        (self.porn + self.hentai + 0.5 * self.sexy).min(1.0)
    }
    /// Blur it? Tuned against false alarms (beach photos, anime art):
    /// clearly porn / hentai, or overwhelmingly porn + hentai + sexy.
    pub fn explicit(&self) -> bool {
        self.porn + self.hentai >= 0.6 || self.porn + self.hentai + self.sexy >= 0.85
    }
}

// ---------------------------------------------------------------- model

enum Op {
    Conv { k: usize, s: usize, pad: [usize; 4], cin: usize, cout: usize, relu6: bool, w: Vec<f32>, b: Vec<f32> },
    Dw { k: usize, s: usize, pad: [usize; 4], c: usize, relu6: bool, w: Vec<f32>, b: Vec<f32> },
    Save,
    Add,
    Gap,
    Dense { cin: usize, cout: usize, act: u8, w: Vec<f32>, b: Vec<f32> },
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> &'a [u8] {
        let s = &self.b[self.at..self.at + n];
        self.at += n;
        s
    }
    fn u8(&mut self) -> u8 {
        self.take(1)[0]
    }
    fn u16(&mut self) -> usize {
        let s = self.take(2);
        u16::from_le_bytes([s[0], s[1]]) as usize
    }
    fn u32(&mut self) -> usize {
        let s = self.take(4);
        u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize
    }
    fn f32(&mut self) -> f32 {
        let s = self.take(4);
        f32::from_le_bytes([s[0], s[1], s[2], s[3]])
    }
    fn f32s(&mut self, n: usize) -> Vec<f32> {
        (0..n).map(|_| self.f32()).collect()
    }
    fn pad(&mut self) -> [usize; 4] {
        [self.u8() as usize, self.u8() as usize, self.u8() as usize, self.u8() as usize]
    }
    /// uint8 weights → f32: lo + q·scale.
    fn weights(&mut self, n: usize) -> Vec<f32> {
        let (lo, scale) = (self.f32(), self.f32());
        self.take(n).iter().map(|&q| lo + q as f32 * scale).collect()
    }
    /// Convolution weights: lo, scale, BatchNorm factor per output channel
    /// (the last axis), the uint8 values → (lo + q·scale)·bn[channel].
    fn conv_weights(&mut self, n: usize, ch: usize) -> Vec<f32> {
        let (lo, scale) = (self.f32(), self.f32());
        let bn = self.f32s(ch);
        self.take(n).iter().enumerate().map(|(i, &q)| (lo + q as f32 * scale) * bn[i % ch]).collect()
    }
}

fn parse(bytes: &[u8]) -> Vec<Op> {
    let mut r = Reader { b: bytes, at: 0 };
    assert_eq!(r.take(4), b"AQN1", "aqnsfw.bin: bad magic");
    let n = r.u32();
    let mut ops = Vec::with_capacity(n);
    for _ in 0..n {
        ops.push(match r.u8() {
            1 => {
                let (k, s, pad) = (r.u8() as usize, r.u8() as usize, r.pad());
                let (cin, cout, relu6) = (r.u16(), r.u16(), r.u8() != 0);
                let w = r.conv_weights(k * k * cin * cout, cout);
                let b = r.f32s(cout);
                Op::Conv { k, s, pad, cin, cout, relu6, w, b }
            }
            2 => {
                let (k, s, pad) = (r.u8() as usize, r.u8() as usize, r.pad());
                let (c, relu6) = (r.u16(), r.u8() != 0);
                let w = r.conv_weights(k * k * c, c);
                let b = r.f32s(c);
                Op::Dw { k, s, pad, c, relu6, w, b }
            }
            3 => {
                r.u8();
                Op::Save
            }
            4 => {
                r.u8();
                Op::Add
            }
            5 => Op::Gap,
            6 => {
                let (cin, cout, act) = (r.u16(), r.u16(), r.u8());
                let w = r.weights(cin * cout);
                let b = r.f32s(cout);
                Op::Dense { cin, cout, act, w, b }
            }
            x => panic!("aqnsfw.bin: unknown op {x}"),
        });
    }
    assert_eq!(r.at, bytes.len(), "aqnsfw.bin: trailing bytes");
    ops
}

fn model() -> &'static [Op] {
    static M: OnceLock<Vec<Op>> = OnceLock::new();
    M.get_or_init(|| parse(MODEL))
}

/// Load the model now (~10 ms, ~10 MB of f32 weights) instead of on the
/// first picture.
pub fn warm_up() {
    let _ = model();
}

// ---------------------------------------------------------------- engine

/// An activation map, height × width × channels (channels contiguous).
struct Map {
    h: usize,
    w: usize,
    c: usize,
    v: Vec<f32>,
}

fn out_size(n: usize, k: usize, s: usize, a: usize, b: usize) -> usize {
    (n + a + b - k) / s + 1
}

/// 1×1 convolution, stride 1, no padding — most of MobileNetV2's work.
/// Four pixels at a time: each row of weights is loaded once for four
/// outputs (the inner loop is a vectorised multiply-add over channels).
#[inline(always)]
fn pointwise_body(x: &[f32], cin: usize, out: &mut [f32], cout: usize, relu6: bool, w: &[f32], b: &[f32]) {
    let npx = x.len() / cin;
    let mut p = 0;
    for blk in out.chunks_mut(4 * cout) {
        let n = blk.len() / cout;
        for o in blk.chunks_exact_mut(cout) {
            o.copy_from_slice(b);
        }
        if n == 4 {
            let (o01, o23) = blk.split_at_mut(2 * cout);
            let (o0, o1) = o01.split_at_mut(cout);
            let (o2, o3) = o23.split_at_mut(cout);
            let xs = [&x[p * cin..][..cin], &x[(p + 1) * cin..][..cin], &x[(p + 2) * cin..][..cin], &x[(p + 3) * cin..][..cin]];
            for (ci, row) in w.chunks_exact(cout).enumerate() {
                let (v0, v1, v2, v3) = (xs[0][ci], xs[1][ci], xs[2][ci], xs[3][ci]);
                if v0 == 0.0 && v1 == 0.0 && v2 == 0.0 && v3 == 0.0 {
                    continue;
                }
                for ((((a0, a1), a2), a3), &wv) in o0.iter_mut().zip(o1.iter_mut()).zip(o2.iter_mut()).zip(o3.iter_mut()).zip(row) {
                    *a0 += v0 * wv;
                    *a1 += v1 * wv;
                    *a2 += v2 * wv;
                    *a3 += v3 * wv;
                }
            }
        } else {
            for (i, o) in blk.chunks_exact_mut(cout).enumerate() {
                let xi = &x[(p + i) * cin..][..cin];
                for (&v, row) in xi.iter().zip(w.chunks_exact(cout)) {
                    if v == 0.0 {
                        continue;
                    }
                    for (a, &wv) in o.iter_mut().zip(row) {
                        *a += v * wv;
                    }
                }
            }
        }
        if relu6 {
            for a in blk.iter_mut() {
                *a = a.clamp(0.0, 6.0);
            }
        }
        p += n;
    }
    debug_assert_eq!(p, npx);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn pointwise_avx2(x: &[f32], cin: usize, out: &mut [f32], cout: usize, relu6: bool, w: &[f32], b: &[f32]) {
    pointwise_body(x, cin, out, cout, relu6, w, b)
}

fn pointwise(x: &Map, out: &mut Map, cout: usize, relu6: bool, w: &[f32], b: &[f32]) {
    out.h = x.h;
    out.w = x.w;
    out.c = cout;
    out.v.clear();
    out.v.resize(x.h * x.w * cout, 0.0);
    #[cfg(target_arch = "x86_64")]
    if avx2() {
        // SAFETY: the CPU has AVX2 and FMA (checked at run time).
        unsafe { pointwise_avx2(&x.v, x.c, &mut out.v, cout, relu6, w, b) };
        return;
    }
    pointwise_body(&x.v, x.c, &mut out.v, cout, relu6, w, b)
}

#[cfg(target_arch = "x86_64")]
fn avx2() -> bool {
    static A: OnceLock<bool> = OnceLock::new();
    *A.get_or_init(|| std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma"))
}

/// Ordinary convolution (k×k, any stride), BatchNorm already folded in.
fn conv(x: &Map, out: &mut Map, k: usize, s: usize, pad: [usize; 4], cout: usize, relu6: bool, w: &[f32], b: &[f32]) {
    let [pt, pl, pb, pr] = pad;
    let (oh, ow, cin) = (out_size(x.h, k, s, pt, pb), out_size(x.w, k, s, pl, pr), x.c);
    out.h = oh;
    out.w = ow;
    out.c = cout;
    out.v.clear();
    out.v.resize(oh * ow * cout, 0.0);
    for oy in 0..oh {
        for ox in 0..ow {
            let o = &mut out.v[(oy * ow + ox) * cout..][..cout];
            o.copy_from_slice(b);
            for ky in 0..k {
                let iy = (oy * s + ky) as isize - pt as isize;
                if iy < 0 || iy >= x.h as isize {
                    continue;
                }
                for kx in 0..k {
                    let ix = (ox * s + kx) as isize - pl as isize;
                    if ix < 0 || ix >= x.w as isize {
                        continue;
                    }
                    let xi = &x.v[(iy as usize * x.w + ix as usize) * cin..][..cin];
                    let wk = &w[(ky * k + kx) * cin * cout..][..cin * cout];
                    for (&v, row) in xi.iter().zip(wk.chunks_exact(cout)) {
                        // ReLU leaves about half of every map at zero.
                        if v == 0.0 {
                            continue;
                        }
                        for (a, &wv) in o.iter_mut().zip(row) {
                            *a += v * wv;
                        }
                    }
                }
            }
            if relu6 {
                for a in o.iter_mut() {
                    *a = a.clamp(0.0, 6.0);
                }
            }
        }
    }
}

/// Depthwise convolution: each channel on its own.
fn depthwise(x: &Map, out: &mut Map, k: usize, s: usize, pad: [usize; 4], relu6: bool, w: &[f32], b: &[f32]) {
    let [pt, pl, pb, pr] = pad;
    let (oh, ow, c) = (out_size(x.h, k, s, pt, pb), out_size(x.w, k, s, pl, pr), x.c);
    out.h = oh;
    out.w = ow;
    out.c = c;
    out.v.clear();
    out.v.resize(oh * ow * c, 0.0);
    #[cfg(target_arch = "x86_64")]
    if avx2() {
        // SAFETY: the CPU has AVX2 and FMA (checked at run time).
        unsafe { depthwise_avx2(x, out, k, s, pad, relu6, w, b) };
        return;
    }
    depthwise_body(x, out, k, s, pad, relu6, w, b)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn depthwise_avx2(x: &Map, out: &mut Map, k: usize, s: usize, pad: [usize; 4], relu6: bool, w: &[f32], b: &[f32]) {
    depthwise_body(x, out, k, s, pad, relu6, w, b)
}

#[inline(always)]
fn depthwise_body(x: &Map, out: &mut Map, k: usize, s: usize, pad: [usize; 4], relu6: bool, w: &[f32], b: &[f32]) {
    let [pt, pl, _, _] = pad;
    let (oh, ow, c) = (out.h, out.w, out.c);
    for oy in 0..oh {
        for ox in 0..ow {
            let o = &mut out.v[(oy * ow + ox) * c..][..c];
            o.copy_from_slice(b);
            for ky in 0..k {
                let iy = (oy * s + ky) as isize - pt as isize;
                if iy < 0 || iy >= x.h as isize {
                    continue;
                }
                for kx in 0..k {
                    let ix = (ox * s + kx) as isize - pl as isize;
                    if ix < 0 || ix >= x.w as isize {
                        continue;
                    }
                    let xi = &x.v[(iy as usize * x.w + ix as usize) * c..][..c];
                    let wk = &w[(ky * k + kx) * c..][..c];
                    for ((a, &v), &wv) in o.iter_mut().zip(xi).zip(wk) {
                        *a += v * wv;
                    }
                }
            }
            if relu6 {
                for a in o.iter_mut() {
                    *a = a.clamp(0.0, 6.0);
                }
            }
        }
    }
}

fn dense(x: &[f32], cout: usize, act: u8, w: &[f32], b: &[f32]) -> Vec<f32> {
    let mut o = b.to_vec();
    for (&v, row) in x.iter().zip(w.chunks_exact(cout)) {
        if v == 0.0 {
            continue;
        }
        for (a, &wv) in o.iter_mut().zip(row) {
            *a += v * wv;
        }
    }
    match act {
        1 => o.iter_mut().for_each(|a| *a = a.max(0.0)),
        2 => {
            let m = o.iter().cloned().fold(f32::MIN, f32::max);
            let mut sum = 0.0;
            for a in o.iter_mut() {
                *a = (*a - m).exp();
                sum += *a;
            }
            o.iter_mut().for_each(|a| *a /= sum);
        }
        _ => {}
    }
    o
}

/// Run the network on a 224 × 224 RGB picture, values 0..1 (HWC).
pub fn classify_input(input: &[f32]) -> Scores {
    assert_eq!(input.len(), SIZE * SIZE * 3);
    let mut cur = Map { h: SIZE, w: SIZE, c: 3, v: input.to_vec() };
    let mut next = Map { h: 0, w: 0, c: 0, v: Vec::with_capacity(SIZE * SIZE * 32) };
    let mut saved: Vec<f32> = Vec::new();
    let mut flat: Vec<f32> = Vec::new();
    for op in model() {
        match op {
            Op::Conv { k, s, pad, cin, cout, relu6, w, b } => {
                debug_assert_eq!(*cin, cur.c);
                if *k == 1 && *s == 1 && *pad == [0; 4] {
                    pointwise(&cur, &mut next, *cout, *relu6, w, b);
                } else {
                    conv(&cur, &mut next, *k, *s, *pad, *cout, *relu6, w, b);
                }
                std::mem::swap(&mut cur, &mut next);
            }
            Op::Dw { k, s, pad, c, relu6, w, b } => {
                debug_assert_eq!(*c, cur.c);
                depthwise(&cur, &mut next, *k, *s, *pad, *relu6, w, b);
                std::mem::swap(&mut cur, &mut next);
            }
            Op::Save => {
                saved.clear();
                saved.extend_from_slice(&cur.v);
            }
            Op::Add => {
                for (a, &b) in cur.v.iter_mut().zip(&saved) {
                    *a += b;
                }
            }
            Op::Gap => {
                let n = (cur.h * cur.w) as f32;
                let mut g = vec![0.0f32; cur.c];
                for px in cur.v.chunks_exact(cur.c) {
                    for (a, &v) in g.iter_mut().zip(px) {
                        *a += v;
                    }
                }
                g.iter_mut().for_each(|a| *a /= n);
                flat = g;
            }
            Op::Dense { cin, cout, act, w, b } => {
                debug_assert_eq!(*cin, flat.len());
                flat = dense(&flat, *cout, *act, w, b);
            }
        }
    }
    Scores { drawing: flat[0], hentai: flat[1], neutral: flat[2], porn: flat[3], sexy: flat[4] }
}

// ---------------------------------------------------------------- pictures

/// RGBA pixels (row-major) → the 224 × 224 network input. Transparent
/// parts (stickers) go onto white. Bigger pictures are shrunk by averaging
/// every source pixel into its cell (no aliasing, one pass); smaller ones
/// are stretched bilinearly, corners aligned (as NSFWJS does).
pub fn input_from_rgba(w: usize, h: usize, rgba: &[u8]) -> Vec<f32> {
    assert!(w > 0 && h > 0 && rgba.len() >= w * h * 4);
    let px = |x: usize, y: usize| -> [f32; 3] {
        let p = &rgba[(y * w + x) * 4..][..4];
        let a = p[3] as f32 / 255.0;
        [0, 1, 2].map(|i| (p[i] as f32 * a + 255.0 * (1.0 - a)) / 255.0)
    };
    let mut out = vec![0.0f32; SIZE * SIZE * 3];
    if w >= SIZE && h >= SIZE {
        // Area average.
        let mut acc = vec![0.0f32; SIZE * SIZE * 3];
        let mut cnt = vec![0u32; SIZE * SIZE];
        for y in 0..h {
            let oy = y * SIZE / h;
            for x in 0..w {
                let ox = x * SIZE / w;
                let c = px(x, y);
                let i = oy * SIZE + ox;
                acc[i * 3] += c[0];
                acc[i * 3 + 1] += c[1];
                acc[i * 3 + 2] += c[2];
                cnt[i] += 1;
            }
        }
        for i in 0..SIZE * SIZE {
            let n = cnt[i].max(1) as f32;
            for ch in 0..3 {
                out[i * 3 + ch] = acc[i * 3 + ch] / n;
            }
        }
    } else {
        let sx = if SIZE > 1 { (w - 1) as f32 / (SIZE - 1) as f32 } else { 0.0 };
        let sy = if SIZE > 1 { (h - 1) as f32 / (SIZE - 1) as f32 } else { 0.0 };
        for oy in 0..SIZE {
            let fy = oy as f32 * sy;
            let (y0, ty) = (fy.floor() as usize, fy.fract());
            let y1 = (y0 + 1).min(h - 1);
            for ox in 0..SIZE {
                let fx = ox as f32 * sx;
                let (x0, tx) = (fx.floor() as usize, fx.fract());
                let x1 = (x0 + 1).min(w - 1);
                let (a, b, c, d) = (px(x0, y0), px(x1, y0), px(x0, y1), px(x1, y1));
                for ch in 0..3 {
                    let top = a[ch] + (b[ch] - a[ch]) * tx;
                    let bot = c[ch] + (d[ch] - c[ch]) * tx;
                    out[(oy * SIZE + ox) * 3 + ch] = top + (bot - top) * ty;
                }
            }
        }
    }
    out
}

/// Pictures smaller than this aren't worth a look (emoji, icons).
pub const MIN_SIDE: u32 = 48;

/// A PNG / JPEG / WebP / GIF (first frame) → scores. `None`: not a picture
/// it can read (video, Lottie .tgs, broken), or too small to judge.
pub fn classify_bytes(bytes: &[u8]) -> Option<Scores> {
    let mut r = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut lim = image::Limits::default();
    lim.max_image_width = Some(16_384);
    lim.max_image_height = Some(16_384);
    lim.max_alloc = Some(400 * 1024 * 1024);
    r.limits(lim);
    let img = r.decode().ok()?;
    if img.width() < MIN_SIDE || img.height() < MIN_SIDE {
        return None;
    }
    let rgba = img.into_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    Some(classify_input(&input_from_rgba(w, h, rgba.as_raw())))
}

/// [`classify_bytes`] of a file.
pub fn classify_file(path: &std::path::Path) -> Option<Scores> {
    classify_bytes(&std::fs::read(path).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pattern `tools/nsfw_convert.py --pattern` feeds its numpy
    /// reference (same converted weights): the engine must agree.
    fn pattern() -> Vec<f32> {
        let mut v = vec![0.0; SIZE * SIZE * 3];
        for y in 0..SIZE {
            for x in 0..SIZE {
                for c in 0..3 {
                    v[(y * SIZE + x) * 3 + c] = ((x * 7 + y * 13 + c * 61) % 256) as f32 / 255.0;
                }
            }
        }
        v
    }

    #[test]
    fn matches_the_reference() {
        let s = classify_input(&pattern());
        let sum = s.drawing + s.hentai + s.neutral + s.porn + s.sexy;
        assert!((sum - 1.0).abs() < 1e-4, "{s:?}");
        let want = REFERENCE;
        let got = [s.drawing, s.hentai, s.neutral, s.porn, s.sexy];
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() < 2e-3, "got {got:?}, want {want:?}");
        }
    }

    #[test]
    fn plain_pictures_are_not_explicit() {
        for rgb in [[255u8, 255, 255], [20, 20, 20], [40, 120, 220]] {
            let rgba: Vec<u8> = (0..300 * 300).flat_map(|_| [rgb[0], rgb[1], rgb[2], 255]).collect();
            let s = classify_input(&input_from_rgba(300, 300, &rgba));
            assert!(!s.explicit(), "{rgb:?}: {s:?}");
        }
    }

    #[test]
    fn tiny_or_unreadable_is_skipped() {
        assert!(classify_bytes(b"not a picture").is_none());
    }

    // `python3 tools/nsfw_convert.py … --pattern`
    const REFERENCE: [f32; 5] = [0.005_862_96, 0.000_933_796, 0.992_514, 0.000_645_563, 0.000_043_631_4];
}

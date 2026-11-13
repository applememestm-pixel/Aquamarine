//! `cargo run -p aqua-nsfw --release --example classify -- <pictures…>`:
//! the scores and the time of each (for checking the detector by hand).
fn main() {
    let t = std::time::Instant::now();
    aqua_nsfw::warm_up();
    println!("model loaded in {} ms", t.elapsed().as_millis());
    // The network alone, on a fixed 224 × 224 input.
    let input = vec![0.5f32; aqua_nsfw::SIZE * aqua_nsfw::SIZE * 3];
    let t = std::time::Instant::now();
    for _ in 0..10 {
        std::hint::black_box(aqua_nsfw::classify_input(&input));
    }
    println!("network alone: {:.1} ms per picture", t.elapsed().as_secs_f64() * 100.0);
    for p in std::env::args().skip(1) {
        let t = std::time::Instant::now();
        match aqua_nsfw::classify_file(std::path::Path::new(&p)) {
            Some(s) => println!(
                "{p}: drawing {:.3} hentai {:.3} neutral {:.3} porn {:.3} sexy {:.3} → {} ({} ms)",
                s.drawing, s.hentai, s.neutral, s.porn, s.sexy,
                if s.explicit() { "EXPLICIT" } else { "ok" },
                t.elapsed().as_millis()
            ),
            None => println!("{p}: not a picture it reads"),
        }
    }
}

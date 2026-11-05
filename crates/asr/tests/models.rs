use asr::Whisper;
use engine::platform::Transcriber;
use std::path::PathBuf;
use std::time::Instant;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn models() -> PathBuf {
    root().join("models")
}
fn load(name: &str) -> Vec<f32> {
    let mut r = hound::WavReader::open(root().join("crates/engine/tests/fixtures").join(name)).unwrap();
    r.samples::<i16>().map(|v| v.unwrap() as f32 / 32768.0).collect()
}
fn w_small() -> Whisper {
    Whisper::new(&models().join("ggml-small.bin")).unwrap()
}
fn w_turbo() -> Whisper {
    Whisper::new(&models().join("ggml-large-v3-turbo-q5_0.bin")).unwrap()
}

#[test]
#[ignore]
fn small_hears_wake() {
    let w = w_small();
    let a = load("wake.wav");
    let t0 = Instant::now();
    let t = w.transcribe(&a).unwrap();
    eprintln!("small wake.wav {:?}: {t}", t0.elapsed());
    assert!(engine::wake::verify_text(&t), "{t}");
}

#[test]
#[ignore]
fn turbo_hears_question() {
    let w = w_turbo();
    let t0 = Instant::now();
    let _ = w.transcribe(&load("wake.wav")).unwrap();
    eprintln!("turbo wake.wav {:?}", t0.elapsed());
    let t = w.transcribe(&load("question.wav")).unwrap();
    assert!(t.contains("公尺") || t.contains("公分") || t.contains("厘米"), "{t}");
}

#[test]
#[ignore]
fn silence_is_short() {
    let t = w_small().transcribe(&vec![0.0; 16000 * 3]).unwrap();
    assert!(t.chars().count() <= 4, "{t}");
}

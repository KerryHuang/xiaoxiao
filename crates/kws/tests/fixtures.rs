use engine::audio::BLOCK;
use engine::platform::Spotter as _;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn hits(name: &str) -> usize {
    let mut s = kws::Spotter::new(&root().join("models/kws")).expect("載入 KWS 模型");
    let mut r = hound::WavReader::open(root().join("crates/engine/tests/fixtures").join(name)).unwrap();
    let mut x: Vec<f32> = r.samples::<i16>().map(|v| v.unwrap() as f32 / 32768.0).collect();
    x.extend(std::iter::repeat_n(0.0, 16000)); // 尾端補 1 秒靜音，讓串流吐出結果
    x.chunks(BLOCK).filter(|b| s.accept(b)).count()
}

#[test]
#[ignore]
fn spots_wake_not_others() {
    assert!(hits("wake.wav") >= 1, "wake.wav 應命中");
    assert_eq!(hits("no-wake-1.wav"), 0, "no-wake-1 不應命中");
    assert_eq!(hits("no-wake-2.wav"), 0, "no-wake-2 不應命中");
}

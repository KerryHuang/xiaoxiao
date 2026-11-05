//! 測試用：開麥克風 N 秒（預設 4）後結束。聲音不落檔。
#[cfg(target_os = "macos")]
fn main() {
    use engine::platform::AudioSource;
    let secs: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(4);
    let mut a = platform_mac::MacAudio::new();
    a.open(None).expect("開麥克風");
    for _ in 0..secs * 10 {
        a.read_block().expect("讀聲音");
    }
    a.close();
}

#[cfg(not(target_os = "macos"))]
fn main() {}

//! 說話與提示音。
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use engine::platform::{Speaker, Stopper};

#[derive(Default)]
pub struct MacSpeaker {
    stop: Arc<AtomicBool>,
}

impl Speaker for MacSpeaker {
    fn say(&self, text: &str) -> Result<(), String> {
        self.stop.store(false, Ordering::SeqCst);
        let mut c = Command::new("say").args(["-v", "Meijia", "--", text]).spawn().map_err(|e| format!("無法執行 say：{e}"))?;
        loop {
            if let Some(s) = c.try_wait().map_err(|e| format!("等待 say 失敗：{e}"))? {
                return if s.success() { Ok(()) } else { Err(format!("say 失敗：{s}")) };
            }
            if self.stop.load(Ordering::SeqCst) {
                let _ = c.kill();
                let _ = c.wait();
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn stopper(&self) -> Stopper {
        let stop = self.stop.clone();
        Arc::new(move || stop.store(true, Ordering::SeqCst))
    }

    fn chime(&self) {
        // 不阻塞：背景執行緒負責 wait，避免留下殭屍程序
        if let Ok(mut c) = Command::new("afplay")
            .arg("/System/Library/Sounds/Tink.aiff")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            std::thread::spawn(move || c.wait());
        }
    }

    fn wake_sound(&self) {
        let _ = Command::new("afplay")
            .arg("/System/Library/Sounds/Tink.aiff")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

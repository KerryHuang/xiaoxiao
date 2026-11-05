//! 說話與提示音。
use std::process::{Command, Stdio};

use engine::platform::Speaker;

pub struct MacSpeaker;

impl Speaker for MacSpeaker {
    fn say(&self, text: &str) -> Result<(), String> {
        let s = Command::new("say").args(["-v", "Meijia", "--", text]).status().map_err(|e| format!("無法執行 say：{e}"))?;
        if s.success() { Ok(()) } else { Err(format!("say 失敗：{s}")) }
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
}

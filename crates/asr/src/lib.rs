//! whisper.cpp 聽寫。
use std::path::Path;
use std::sync::Mutex;

use engine::audio::{BLOCK, rms};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState};

/// 不得放「小小」：會誘發幻覺成一串「小小小」。
const INITIAL_PROMPT: &str = "以下是繁體中文的句子。";

/// 每個區塊 RMS 都低於此值就視為靜音、不送 whisper：
/// whisper 對靜音會幻覺出「(字幕製作:…)」，且帶 initial prompt 時 no_speech 機率不可靠。
const SILENCE_RMS: f32 = 0.005;

/// 運算長度：涵蓋語音再多留 1 秒，至少 10 秒（500）。whisper 預設固定算 30 秒（1500）。
/// 少於 10 秒實測會把短句聽成亂字；turbo 縮短會鬼打牆，所以只給 small 用。
fn fit_audio_ctx(samples: usize) -> i32 {
    let frames = (samples as u64 / 16_000 + 1) * 50;
    frames.clamp(500, 1500) as i32
}

pub struct Whisper {
    state: Mutex<WhisperState>,
}

impl Whisper {
    pub fn new(model: &Path) -> Result<Whisper, String> {
        whisper_rs::install_logging_hooks(); // 關掉 whisper.cpp 的 stderr 洗版
        let path = model.to_str().ok_or("模型路徑不是 UTF-8")?;
        if !model.is_file() {
            return Err(format!("缺少模型檔 {}", model.display()));
        }
        let ctx = WhisperContext::new_with_params(path, WhisperContextParameters::default())
            .map_err(|e| format!("載入 whisper 模型失敗：{e}"))?;
        let state = ctx.create_state().map_err(|e| format!("建立 whisper state 失敗：{e}"))?;
        Ok(Whisper { state: Mutex::new(state) })
    }
}

impl engine::platform::Transcriber for Whisper {
    fn transcribe(&self, audio_16k: &[f32]) -> Result<String, String> {
        if audio_16k.chunks(BLOCK).all(|b| rms(b) < SILENCE_RMS) {
            return Ok(String::new());
        }
        let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        p.set_language(Some("zh"));
        p.set_initial_prompt(INITIAL_PROMPT);
        p.set_no_context(true);
        p.set_single_segment(true);
        p.set_no_timestamps(true);
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_special(false);
        p.set_print_timestamps(false);
        if cfg!(windows) {
            p.set_audio_ctx(fit_audio_ctx(audio_16k.len()));
        }
        let mut st = self.state.lock().map_err(|_| "whisper state 已損毀")?;
        st.full(p, audio_16k).map_err(|e| format!("whisper 聽寫失敗：{e}"))?;
        let mut out = String::new();
        for seg in st.as_iter() {
            out.push_str(&seg.to_str_lossy().map_err(|e| e.to_string())?);
        }
        Ok(out.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEC: usize = 16_000;

    #[test]
    fn short_speech_keeps_ten_seconds_of_context() {
        // 少於 10 秒的長度在實測會把短句聽成亂字
        assert_eq!(fit_audio_ctx(SEC), 500);
        assert_eq!(fit_audio_ctx(9 * SEC), 500);
    }

    #[test]
    fn long_speech_gets_its_length_plus_a_second() {
        assert_eq!(fit_audio_ctx(20 * SEC), 1050);
    }

    #[test]
    fn context_never_exceeds_the_model_limit() {
        assert_eq!(fit_audio_ctx(60 * SEC), 1500);
    }
}

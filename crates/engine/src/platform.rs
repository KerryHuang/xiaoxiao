//! 平台抽象：各平台實作由獨立 crate 提供。

/// 第一關喚醒偵測器：每次餵一個區塊（16 kHz、BLOCK 個樣本），命中回 true。
pub trait Spotter {
    fn accept(&mut self, block: &[f32]) -> bool;
    fn reset(&mut self);
}

/// 語音轉文字：輸入 16 kHz 單聲道，回傳去頭尾空白的文字。
pub trait Transcriber {
    fn transcribe(&self, audio_16k: &[f32]) -> Result<String, String>;
}

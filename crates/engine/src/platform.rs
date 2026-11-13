//! 平台抽象：各平台實作由獨立 crate 提供。

/// 收音錯誤。`Gone`：裝置消失或串流失效，呼叫端應重開。
#[derive(Debug, Clone, PartialEq)]
pub enum AudioError {
    Gone,
    NoUsableInput,
    Other(String),
}

/// 收音：`read_block` 回 16 kHz、`audio::BLOCK` 長度的區塊。
pub trait AudioSource: Send {
    fn open(&mut self, device: Option<&str>) -> Result<(), AudioError>;
    fn read_block(&mut self) -> Result<Vec<f32>, AudioError>;
    /// 取走已經收進來、還沒讀的整塊聲音（不阻塞）。喚醒複核期間使用者可能已經開口，
    /// 呼叫端要先取走，再 `flush` 丟掉播放回音。
    fn take_buffered(&mut self) -> Vec<Vec<f32>> {
        vec![]
    }
    /// 丟掉已經收進來、還沒讀的聲音（小小講完話後呼叫，避免聽到自己）。
    fn flush(&mut self) {}
    /// 開的是系統預設輸入，而系統預設已經換成別的裝置（呼叫端應重開）。
    fn default_changed(&self) -> bool {
        false
    }
    fn close(&mut self);
}

/// 有沒有別的程序在用麥克風（通話、會議錄音）。
pub trait MicUsage {
    fn others_using_mic(&self) -> bool;
}

pub trait ScreenState {
    fn locked(&self) -> bool;
}

/// 可跨執行緒呼叫的停止動作。
pub type Stopper = std::sync::Arc<dyn Fn() + Send + Sync>;

pub trait Speaker: Send {
    /// 阻塞到說完。
    fn say(&self, text: &str) -> Result<(), String>;
    fn chime(&self);
    /// 從休息被叫醒的起床音；阻塞到播完，之後才開始收音。
    fn wake_sound(&self);
    /// 可跨執行緒呼叫：讓正在進行的 `say` 盡快返回（回 Ok）。沒在說話時呼叫無效。
    fn stopper(&self) -> Stopper {
        std::sync::Arc::new(|| {})
    }
}

pub trait Notifier {
    fn notify(&self, title: &str, body: &str);
}

/// 麥克風授權狀態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicPermission {
    NotDetermined,
    Restricted,
    Denied,
    Authorized,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct InputDevice {
    pub name: String,
    pub bluetooth: bool,
}

pub trait Devices {
    fn inputs(&self) -> Vec<InputDevice>;
    fn default_input(&self) -> Option<InputDevice>;
}

/// 第一關喚醒偵測器：每次餵一個區塊（16 kHz、BLOCK 個樣本），命中回 true。
pub trait Spotter {
    fn accept(&mut self, block: &[f32]) -> bool;
    fn reset(&mut self);
}

/// 語音轉文字：輸入 16 kHz 單聲道，回傳去頭尾空白的文字。
pub trait Transcriber {
    fn transcribe(&self, audio_16k: &[f32]) -> Result<String, String>;
}

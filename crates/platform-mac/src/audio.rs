//! 收音：共用 audio-cpal，這裡只放 macOS 選裝置的規則。
use audio_cpal::CpalAudio;
use engine::platform::{AudioError, Devices};

use crate::devices;

pub type MacAudio = CpalAudio;

pub fn new_audio() -> MacAudio {
    CpalAudio::new(choose)
}

/// 決定要開哪個裝置：指定 > 系統預設（非藍牙）> 替代裝置。
fn choose(device: Option<&str>) -> Result<String, AudioError> {
    if let Some(d) = device {
        return Ok(d.to_string());
    }
    match devices::MacDevices.default_input() {
        Some(d) if !d.bluetooth => Ok(d.name),
        _ => devices::fallback_input().ok_or(AudioError::NoUsableInput),
    }
}

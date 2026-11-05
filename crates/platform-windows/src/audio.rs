//! 收音：共用 audio-cpal，這裡只放 Windows 選裝置的規則。
use audio_cpal::CpalAudio;
use engine::platform::{AudioError, Devices};

use crate::devices;

pub type WinAudio = CpalAudio;

pub fn new_audio() -> WinAudio {
    CpalAudio::new(choose)
}

/// 決定要開哪個裝置：指定 > 系統預設（非藍牙）> 替代裝置。
fn choose(device: Option<&str>) -> Result<String, AudioError> {
    if let Some(d) = device {
        return Ok(d.to_string());
    }
    match devices::WinDevices.default_input() {
        Some(d) if !d.bluetooth => Ok(d.name),
        _ => devices::fallback_input().ok_or(AudioError::NoUsableInput),
    }
}

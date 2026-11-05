//! macOS 平台實作。
#![cfg(target_os = "macos")]
mod audio;
mod devices;
mod ffi;
mod micusage;
mod permission;
mod screen;
mod speaker;

pub use audio::{MacAudio, new_audio};
pub use devices::MacDevices;
pub use micusage::MacMicUsage;
pub use permission::{MicPermission, mic_permission};
pub use screen::MacScreen;
pub use speaker::MacSpeaker;

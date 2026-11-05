//! Windows 平台實作。判斷邏輯放在 logic（純函式，各平台都能測）；呼叫系統 API 的部分只在 Windows 編譯。
pub mod logic;

#[cfg(windows)]
mod audio;
#[cfg(windows)]
mod com;
#[cfg(windows)]
mod devices;

#[cfg(windows)]
pub use audio::{WinAudio, new_audio};
#[cfg(windows)]
pub use devices::WinDevices;

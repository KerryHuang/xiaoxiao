//! 各平台實作對應到相同名稱，其他程式碼不用知道現在是哪個平台。
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
pub use platform_mac::{
    MacAudio as OsAudio, MacDevices as OsDevices, MacMicUsage as OsMicUsage, MacScreen as OsScreen, MacSpeaker as OsSpeaker, MicPermission,
    mic_permission, new_audio,
};
#[cfg(windows)]
pub use platform_windows::{
    MicPermission, WinAudio as OsAudio, WinDevices as OsDevices, WinMicUsage as OsMicUsage, WinScreen as OsScreen, WinSpeaker as OsSpeaker,
    mic_permission, new_audio, voice_ok,
};

/// 系統有沒有小小要用的語音（美佳）。
#[cfg(target_os = "macos")]
pub fn voice_ok() -> bool {
    std::process::Command::new("say")
        .args(["-v", "?"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("Meijia"))
        .unwrap_or(false)
}

/// 交給 whisper.cpp 與 sherpa-onnx 的模型路徑。
#[cfg(target_os = "macos")]
pub fn model_path(p: &Path) -> Result<PathBuf, String> {
    Ok(p.to_path_buf())
}

/// whisper.cpp 在 Windows 用窄字元路徑開檔，帳號資料夾含中文時要換成 8.3 短路徑。
#[cfg(windows)]
pub fn model_path(p: &Path) -> Result<PathBuf, String> {
    let s = platform_windows::short_path(p);
    if s.to_string_lossy().is_ascii() {
        Ok(s)
    } else {
        Err("模型路徑含非英文字元，這台電腦無法載入。請把 Windows 帳號資料夾換成英文，或開啟 8.3 短檔名。".into())
    }
}

/// 開終端機執行使用者自己的 claude，讓使用者在官方 CLI 裡登入；小小不碰帳號。
#[cfg(target_os = "macos")]
pub fn open_terminal(bin: &Path) {
    let cmd = format!("'{}'", bin.to_string_lossy().replace('\'', r"'\''"));
    let script = format!(
        "tell application \"Terminal\"\nactivate\ndo script \"{}\"\nend tell",
        cmd.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let _ = std::process::Command::new("osascript").args(["-e", &script]).spawn().map(|mut c| {
        std::thread::spawn(move || c.wait());
    });
}

#[cfg(windows)]
pub fn open_terminal(bin: &Path) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    let _ = std::process::Command::new(bin).creation_flags(CREATE_NEW_CONSOLE).spawn().map(|mut c| {
        std::thread::spawn(move || c.wait());
    });
}

/// 從 Finder、登入項目或開機啟動時 PATH 可能不完整，補上 claude 常見的安裝位置。
pub fn claude_dirs(home: &Path) -> Vec<PathBuf> {
    if cfg!(windows) {
        vec![home.join(".local").join("bin")]
    } else {
        vec![home.join(".local/bin"), home.join(".claude/local"), "/opt/homebrew/bin".into(), "/usr/local/bin".into()]
    }
}

/// Claude Desktop 的資料夾（內建的 Claude Code 在裡面）。Windows 的位置未經實機驗證，兩處都找。
pub fn desktop_roots(home: &Path) -> Vec<PathBuf> {
    if cfg!(windows) {
        ["APPDATA", "LOCALAPPDATA"].iter().filter_map(std::env::var_os).map(|d| PathBuf::from(d).join("Claude")).collect()
    } else {
        vec![home.join("Library/Application Support/Claude")]
    }
}

//! 使用者設定（settings.json）。
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EngineKind {
    ClaudeCli,
    ApiKey,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub input_device: Option<String>,
    pub read_dirs: Vec<PathBuf>,
    pub notes_path: PathBuf,
    pub hotkey: String,
    pub diagnostic: bool,
    pub engine: EngineKind,
    /// 走完第一次啟動引導（麥克風權限與 Claude 都過關）才開始待命。
    pub onboarded: bool,
}

/// 預設快捷鍵。Windows 鍵盤沒有 Option，寫成 Alt（兩者解析結果相同）。
pub const DEFAULT_HOTKEY: &str = if cfg!(windows) { "Control+Alt+Space" } else { "Control+Option+Space" };

pub fn home() -> PathBuf {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default()
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            input_device: None,
            read_dirs: vec![],
            notes_path: home().join("Documents").join("小小筆記.md"),
            hotkey: DEFAULT_HOTKEY.into(),
            diagnostic: false,
            engine: EngineKind::ClaudeCli,
            onboarded: false,
        }
    }
}

impl Settings {
    /// 檔案不存在 → 全部預設；缺欄位 → 該欄預設。
    pub fn load(path: &Path) -> Result<Settings, String> {
        match std::fs::read_to_string(path) {
            Ok(s) => serde_json::from_str(&s).map_err(|e| format!("設定檔格式錯誤：{e}")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(format!("讀不到設定檔：{e}")),
        }
    }

    /// 先寫暫存檔再改名，避免寫到一半壞掉。
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        let s = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, s).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    pub fn validate(&self) -> Result<(), String> {
        match self.engine {
            EngineKind::ClaudeCli => Ok(()),
            EngineKind::ApiKey => Err("尚未支援".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_defaults_and_roundtrip() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("settings.json");
        std::fs::write(&p, "{}").unwrap();
        let s = Settings::load(&p).unwrap();
        assert_eq!(s.hotkey, DEFAULT_HOTKEY);
        assert!(s.read_dirs.is_empty());
        assert_eq!(s.engine, EngineKind::ClaudeCli);
        assert!(!s.onboarded, "第一次啟動要走引導");
        assert!(s.notes_path.ends_with("Documents/小小筆記.md"));
        let mut s2 = s.clone();
        s2.read_dirs.push("/tmp/docs".into());
        s2.save(&t.path().join("sub/settings.json")).unwrap();
        assert_eq!(Settings::load(&t.path().join("sub/settings.json")).unwrap(), s2);
    }

    #[test]
    fn missing_file_gives_defaults() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(Settings::load(&t.path().join("none.json")).unwrap().hotkey, DEFAULT_HOTKEY);
    }

    #[test]
    fn api_key_engine_not_supported_yet() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("settings.json");
        std::fs::write(&p, r#"{"engine":"ApiKey"}"#).unwrap();
        let s = Settings::load(&p).unwrap();
        assert_eq!(s.validate(), Err("尚未支援".to_string()));
    }
}

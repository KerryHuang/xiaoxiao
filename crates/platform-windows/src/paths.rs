//! whisper.cpp 用窄字元路徑開檔，Windows 使用者名稱含中文時會開不了。
use std::path::{Path, PathBuf};

use windows::Win32::Storage::FileSystem::GetShortPathNameW;
use windows::core::HSTRING;

/// 全 ASCII 就原樣回傳；否則換成 8.3 短路徑。路徑要已存在。
/// 機器關閉 8.3 時回傳原值，由呼叫端判斷（結果仍含非 ASCII 字元就回報錯誤）。
pub fn short_path(p: &Path) -> PathBuf {
    if p.to_string_lossy().is_ascii() {
        return p.to_path_buf();
    }
    let wide = HSTRING::from(p);
    let mut buf = vec![0u16; 4096];
    let n = unsafe { GetShortPathNameW(&wide, Some(&mut buf)) } as usize;
    if n == 0 || n >= buf.len() {
        return p.to_path_buf();
    }
    PathBuf::from(String::from_utf16_lossy(&buf[..n]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_path_of_cjk_dir_is_ascii() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("語音模型");
        std::fs::create_dir(&d).unwrap();
        let s = short_path(&d);
        assert!(s.is_dir());
        assert!(s.to_str().unwrap().is_ascii(), "{s:?}");
    }

    #[test]
    fn short_path_keeps_ascii_path_unchanged() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(short_path(t.path()), t.path());
    }
}

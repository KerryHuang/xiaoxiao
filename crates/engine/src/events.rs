//! 事件紀錄（JSONL）：只記事件，不記內容。
use std::io::Write;
use std::path::PathBuf;

/// 允許寫進紀錄的欄位；其餘一律丟掉，避免內容外洩到檔案。
const ALLOWED: &[&str] = &["latency_ms", "ok", "route", "error", "gain", "state"];

pub struct EventLog {
    path: PathBuf,
    diagnostic: bool,
}

impl EventLog {
    pub fn new(path: PathBuf, diagnostic: bool) -> EventLog {
        EventLog { path, diagnostic }
    }

    /// 寫一行 JSONL。寫入失敗只印到 stderr，不影響主流程。
    pub fn log(&self, ev: &str, fields: serde_json::Value) {
        let mut out = serde_json::Map::new();
        out.insert("t".into(), chrono::Local::now().to_rfc3339().into());
        out.insert("ev".into(), ev.into());
        if let serde_json::Value::Object(m) = fields {
            for (k, v) in m {
                if ALLOWED.contains(&k.as_str()) || (self.diagnostic && k == "text") {
                    out.insert(k, v);
                }
            }
        }
        let line = format!("{}\n", serde_json::Value::Object(out));
        let r = (|| {
            if let Some(d) = self.path.parent() {
                std::fs::create_dir_all(d)?;
            }
            std::fs::OpenOptions::new().create(true).append(true).open(&self.path)?.write_all(line.as_bytes())
        })();
        if let Err(e) = r {
            eprintln!("事件紀錄寫入失敗：{e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn eventlog_drops_text_unless_diagnostic() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("logs/events.jsonl");
        EventLog::new(p.clone(), false).log("wake", json!({"ok": true, "text": "秘密", "question": "也是秘密"}));
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(!s.contains("秘密"), "{s}");
        let v: serde_json::Value = serde_json::from_str(s.lines().next().unwrap()).unwrap();
        assert_eq!(v["ev"], "wake");
        assert_eq!(v["ok"], true);
        assert!(v["t"].is_string());

        EventLog::new(p.clone(), true).log("wake", json!({"text": "診斷"}));
        let s = std::fs::read_to_string(&p).unwrap();
        assert_eq!(s.lines().count(), 2);
        assert!(s.contains("診斷"));
        assert!(!s.contains("也是秘密"));
    }
}

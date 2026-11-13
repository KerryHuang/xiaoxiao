//! 事件紀錄（JSONL）：只記事件，不記內容。
use std::io::Write;
use std::path::PathBuf;

/// 允許寫進紀錄的欄位；其餘一律丟掉，避免內容外洩到檔案。
const ALLOWED: &[&str] = &["latency_ms", "ok", "route", "error", "gain", "state", "peak", "floor", "speech_blocks", "reason", "saved", "turns", "skip", "said", "left", "why"];

/// 紀錄檔上限。抓不到麥克風時每 2 秒一筆，一天約 3.7 MB，不設限會一直長大。
const MAX_BYTES: u64 = 5 * 1024 * 1024;

pub struct EventLog {
    path: PathBuf,
    diagnostic: bool,
    max_bytes: u64,
}

impl EventLog {
    pub fn new(path: PathBuf, diagnostic: bool) -> EventLog {
        EventLog { path, diagnostic, max_bytes: MAX_BYTES }
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
            // 滿了就換檔，只留一份舊檔（覆蓋掉更舊的）
            if std::fs::metadata(&self.path).is_ok_and(|m| m.len() + line.len() as u64 > self.max_bytes) {
                std::fs::rename(&self.path, self.path.with_extension("jsonl.1"))?;
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

    #[test]
    fn eventlog_keeps_one_old_file_when_full() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("events.jsonl");
        let log = EventLog { path: p.clone(), diagnostic: false, max_bytes: 300 };
        for i in 0..100 {
            log.log("audio", json!({"latency_ms": i}));
        }
        let now = std::fs::read_to_string(&p).unwrap();
        let old = std::fs::read_to_string(t.path().join("events.jsonl.1")).unwrap();
        assert!(now.len() <= 300, "目前的檔案超過上限：{}", now.len());
        assert!(old.len() <= 300, "舊檔超過上限：{}", old.len());
        assert!(now.lines().last().unwrap().contains("\"latency_ms\":99"), "最新一筆要留著");
        assert_eq!(std::fs::read_dir(t.path()).unwrap().count(), 2, "只留一份舊檔");
    }
}

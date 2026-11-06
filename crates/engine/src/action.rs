//! 做事動作：解析確認對話的回覆、驗證欄位、提醒儲存、筆記寫入。

use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Reminder { at: DateTime<FixedOffset>, text: String, repeat: Option<String> },
    Timer { secs: u32, name: Option<String> },
    Note { text: String, title: Option<String> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmOutcome {
    Do(Action),
    Cancel,
    /// 這句話跟要做的事無關，或根本沒有要做的事
    Other,
    Say(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionError {
    BadJson,
    MissingField(&'static str),
    InPast,
}

impl fmt::Display for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActionError::BadJson => write!(f, "我沒聽懂要做什麼，可以再說一次嗎"),
            ActionError::MissingField(field) => {
                let name = match *field {
                    "at" => "時間",
                    "text" => "內容",
                    "secs" => "多久",
                    "type" => "要做什麼",
                    other => other,
                };
                write!(f, "還缺{name}，可以告訴我嗎")
            }
            ActionError::InPast => write!(f, "這個時間已經過了，要改成什麼時候"),
        }
    }
}

fn str_field(v: &Value, key: &'static str) -> Result<String, ActionError> {
    match v.get(key).and_then(Value::as_str).map(str::trim) {
        Some(s) if !s.is_empty() => Ok(s.to_string()),
        _ => Err(ActionError::MissingField(key)),
    }
}

fn opt_str(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

/// 解析確認對話的一次回覆：`<<DO>>{json}`、`<<CANCEL>>`、`<<OTHER>>`，其餘當成要唸出來的話。
pub fn parse_confirm_reply(reply: &str, now: DateTime<FixedOffset>) -> Result<ConfirmOutcome, ActionError> {
    let t = reply.trim();
    if t.starts_with("<<CANCEL>>") {
        return Ok(ConfirmOutcome::Cancel);
    }
    if t.starts_with("<<OTHER>>") {
        return Ok(ConfirmOutcome::Other);
    }
    let Some(rest) = t.strip_prefix("<<DO>>") else {
        return Ok(ConfirmOutcome::Say(t.to_string()));
    };
    let v: Value = serde_json::from_str(rest.trim()).map_err(|_| ActionError::BadJson)?;
    let kind = v.get("type").and_then(Value::as_str).ok_or(ActionError::MissingField("type"))?;
    let action = match kind {
        "reminder" => {
            let at_s = str_field(&v, "at")?;
            let at = DateTime::parse_from_rfc3339(&at_s).map_err(|_| ActionError::BadJson)?;
            let text = str_field(&v, "text")?;
            if at <= now {
                return Err(ActionError::InPast);
            }
            Action::Reminder { at, text, repeat: opt_str(&v, "repeat") }
        }
        "timer" => {
            let secs = v.get("secs").and_then(Value::as_u64).filter(|n| *n > 0).ok_or(ActionError::MissingField("secs"))?;
            Action::Timer { secs: secs.min(u32::MAX as u64) as u32, name: opt_str(&v, "name") }
        }
        "note" => Action::Note { text: str_field(&v, "text")?, title: opt_str(&v, "title") },
        _ => return Err(ActionError::BadJson),
    };
    Ok(ConfirmOutcome::Do(action))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reminder {
    pub id: String,
    pub at: DateTime<FixedOffset>,
    pub text: String,
}

/// 提醒清單，存成 JSON；寫檔先寫暫存檔再 rename，避免寫到一半壞掉。
pub struct ReminderStore {
    path: PathBuf,
}

impl ReminderStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn load_all(&self) -> io::Result<Vec<Reminder>> {
        match fs::read_to_string(&self.path) {
            Ok(s) => serde_json::from_str(&s).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    fn save_all(&self, all: &[Reminder]) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(all).map_err(io::Error::other)?)?;
        fs::rename(&tmp, &self.path)
    }

    pub fn add(&self, r: &Reminder) -> io::Result<()> {
        let mut all = self.load_all()?;
        all.retain(|x| x.id != r.id);
        all.push(r.clone());
        self.save_all(&all)
    }

    pub fn remove(&self, id: &str) -> io::Result<()> {
        let mut all = self.load_all()?;
        all.retain(|x| x.id != id);
        self.save_all(&all)
    }

    /// 回傳（已過期, 未到期），都依時間排序；不修改檔案。
    pub fn reload(&self, now: DateTime<FixedOffset>) -> io::Result<(Vec<Reminder>, Vec<Reminder>)> {
        let mut all = self.load_all()?;
        all.sort_by_key(|r| r.at);
        Ok(all.into_iter().partition(|r| r.at <= now))
    }
}

/// 把筆記附加到 Markdown 檔：`## {標題或時間}\n\n{內容}\n\n`。
/// 啟動時發現已過期的提醒。
pub fn missed_message(r: &Reminder) -> String {
    format!("你錯過了一個提醒：{}", r.text)
}

pub fn append_note(path: &Path, note: &Action, now: DateTime<FixedOffset>) -> io::Result<()> {
    let Action::Note { text, title } = note else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "不是筆記"));
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let head = title.clone().unwrap_or_else(|| now.format("%Y-%m-%d %H:%M").to_string());
    let mut f = fs::OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(format!("## {head}\n\n{text}\n\n").as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;

    #[test]
    fn missed_message_names_the_reminder() {
        let r = Reminder { id: "1".into(), at: DateTime::parse_from_rfc3339("2026-11-05T09:00:00+08:00").unwrap(), text: "開會".into() };
        assert_eq!(missed_message(&r), "你錯過了一個提醒：開會");
    }

    fn now() -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339("2026-11-05T09:30:00+08:00").unwrap()
    }
    fn at(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).unwrap()
    }

    #[test]
    fn parses_do_reminder() {
        let r = parse_confirm_reply(r#"<<DO>>{"type":"reminder","at":"2026-11-05T15:00:00+08:00","text":"開會"}"#, now()).unwrap();
        assert!(matches!(r, ConfirmOutcome::Do(Action::Reminder { ref text, .. }) if text == "開會"));
    }

    #[test]
    fn parses_do_timer_and_note() {
        let r = parse_confirm_reply(r#" <<DO>> {"type":"timer","secs":600}"#, now()).unwrap();
        assert!(matches!(r, ConfirmOutcome::Do(Action::Timer { secs: 600, name: None })));
        let r = parse_confirm_reply(r#"<<DO>>{"type":"note","text":"回客戶信","title":"待辦"}"#, now()).unwrap();
        assert!(matches!(r, ConfirmOutcome::Do(Action::Note { .. })));
    }

    #[test]
    fn rejects_past_and_missing() {
        assert!(matches!(
            parse_confirm_reply(r#"<<DO>>{"type":"reminder","at":"2026-11-05T08:00:00+08:00","text":"x"}"#, now()),
            Err(ActionError::InPast)
        ));
        assert!(matches!(parse_confirm_reply(r#"<<DO>>{"type":"note"}"#, now()), Err(ActionError::MissingField("text"))));
        assert!(matches!(parse_confirm_reply(r#"<<DO>>{"type":"reminder","text":"x"}"#, now()), Err(ActionError::MissingField("at"))));
        assert!(matches!(parse_confirm_reply("<<DO>>{oops", now()), Err(ActionError::BadJson)));
        assert!(!ActionError::InPast.to_string().is_empty());
    }

    #[test]
    fn cancel_and_say() {
        assert!(matches!(parse_confirm_reply("<<CANCEL>>", now()).unwrap(), ConfirmOutcome::Cancel));
        assert!(matches!(parse_confirm_reply("<<OTHER>>", now()).unwrap(), ConfirmOutcome::Other));
        assert!(matches!(parse_confirm_reply("今天下午三點提醒你開會，對嗎？", now()).unwrap(), ConfirmOutcome::Say(_)));
    }

    #[test]
    fn store_reload_splits_due_and_pending() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReminderStore::new(dir.path().join("reminders.json"));
        store.add(&Reminder { id: "a".into(), at: at("2026-11-05T08:00:00+08:00"), text: "早會".into() }).unwrap();
        store.add(&Reminder { id: "b".into(), at: at("2026-11-05T15:00:00+08:00"), text: "開會".into() }).unwrap();
        for _ in 0..2 {
            let (due, pending) = store.reload(now()).unwrap();
            assert_eq!(due.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["a"]);
            assert_eq!(pending.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["b"]);
        }
        store.remove("a").unwrap();
        let (due, pending) = store.reload(now()).unwrap();
        assert!(due.is_empty());
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn note_appends_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("筆記.md");
        append_note(&p, &Action::Note { text: "回客戶信".into(), title: Some("待辦".into()) }, now()).unwrap();
        append_note(&p, &Action::Note { text: "買牛奶".into(), title: None }, now()).unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.contains("## 待辦\n\n回客戶信\n\n"));
        assert!(s.contains("## 2026-11-05 09:30\n\n買牛奶\n\n"));
    }
}

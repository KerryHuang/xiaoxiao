//! 串流文字切句：第一句一出來就能開始說。

#[derive(Debug, Default)]
pub struct SentenceSplitter {
    buf: String,
}

const ENDS: &[char] = &['。', '！', '？', '!', '?', '\n'];

impl SentenceSplitter {
    /// 加入一段增量文字，回傳已完整的句子（去頭尾空白、丟掉空句）。
    pub fn push(&mut self, delta: &str) -> Vec<String> {
        self.buf.push_str(delta);
        let mut out = Vec::new();
        while let Some(i) = self.buf.find(ENDS) {
            let end = i + self.buf[i..].chars().next().unwrap().len_utf8();
            let sent: String = self.buf.drain(..end).collect();
            let sent = sent.trim();
            if !sent.is_empty() && sent.chars().any(|c| !ENDS.contains(&c)) {
                out.push(sent.to_string());
            }
        }
        out
    }

    /// 串流結束，回傳剩下沒有句尾標點的部分。
    pub fn finish(&mut self) -> Option<String> {
        let rest = std::mem::take(&mut self.buf);
        let rest = rest.trim();
        (!rest.is_empty()).then(|| rest.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_streaming_deltas() {
        let mut s = SentenceSplitter::default();
        assert!(s.push("1 公斤等於").is_empty());
        assert_eq!(s.push("1000 公克。要不要"), vec!["1 公斤等於1000 公克。"]);
        assert_eq!(s.finish(), Some("要不要".into()));
    }
}

/// whisper 對雜音常見的幻覺：影片字幕、片尾、括號裡的音效。命中就當沒聽到，不送去問 Claude。
const JUNK: &[&str] = &["字幕製作", "字幕由", "字幕志願者", "字幕志愿者", "請不吝", "點贊", "點讚", "訂閱我的頻道", "謝謝觀看", "感謝觀看", "感謝收看", "謝謝收看", "Amara"];

pub fn is_junk_transcript(t: &str) -> bool {
    let t = t.trim();
    if !t.chars().any(char::is_alphanumeric) {
        return true;
    }
    let bracketed = (t.starts_with('(') || t.starts_with('（') || t.starts_with('[')) && (t.ends_with(')') || t.ends_with('）') || t.ends_with(']'));
    bracketed || JUNK.iter().any(|j| t.contains(j))
}

#[cfg(test)]
mod junk_tests {
    use super::is_junk_transcript;

    #[test]
    fn junk_transcripts_detected() {
        for t in ["(字幕製作:貝爾)", "文字幕志愿者 李宗盛", "請不吝點贊 訂閱 轉發", "謝謝觀看", "（音樂）", "...", " "] {
            assert!(is_junk_transcript(t), "{t}");
        }
        for t in ["現在幾點", "幫我記一下明天要買牛奶", "字幕要怎麼加"] {
            assert!(!is_junk_transcript(t), "{t}");
        }
    }
}

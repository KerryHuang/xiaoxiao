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

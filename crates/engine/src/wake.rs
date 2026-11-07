//! 喚醒第二關：Whisper 轉出的文字是否含「嘿，小小」。

/// Whisper 常把「小小」寫成同音字或簡體；含其中之一就算通過。
/// 注意：餵給 Whisper 的 initial prompt 不可含「小小」，否則會被誘導出來。
pub fn verify_text(text: &str) -> bool {
    ["小小", "曉曉", "晓晓"].iter().any(|w| text.contains(w))
}

/// 追問要先叫名字：開頭是（嘿）小小才回傳後面的話，只叫名字回傳空字串。
/// 名字前容許多兩個字，因為語音辨識常把「嘿」聽成「配」「可以」；後面接「的」是在聊別的事。
pub fn strip_call_name(text: &str) -> Option<&str> {
    let skip = |c: char| !c.is_alphanumeric();
    let t = text.trim_start_matches(skip);
    let t = ["嘿", "黑", "嗨", "Hey", "hey", "HEY"].iter().find_map(|p| t.strip_prefix(p)).unwrap_or(t);
    let (at, name) = ["小小", "曉曉", "晓晓"].iter().filter_map(|n| t.find(n).map(|at| (at, n.len()))).min()?;
    if t[..at].chars().filter(|c| c.is_alphanumeric()).count() > 2 {
        return None;
    }
    let rest = &t[at + name..];
    if rest.starts_with('的') {
        return None;
    }
    let rest = rest.trim_start_matches(skip).trim_end();
    // 只叫名字常被聽成「小小小」
    if rest.chars().all(|c| c == '小' || skip(c)) {
        return Some("");
    }
    Some(rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_call_name_needs_the_name_first() {
        for (t, want) in [
            ("小小，那兩公斤呢", "那兩公斤呢"),
            ("小小那兩公斤呢", "那兩公斤呢"),
            ("曉曉, 明天呢", "明天呢"),
            ("晓晓 明天呢", "明天呢"),
            ("嘿，小小，明天呢", "明天呢"),
            ("Hey 小小 明天呢", "明天呢"),
            ("小小。", ""),
            ("小小小。", ""),
            // 語音辨識常把開頭的「嘿」聽錯（實機紀錄）
            ("配小小,一公斤等於幾克", "一公斤等於幾克"),
            ("可以小小。", ""),
            ("好，小小，明天呢", "明天呢"),
        ] {
            assert_eq!(strip_call_name(t), Some(want), "{t}");
        }
        for t in [
            "那兩公斤呢",
            "這是小事",
            "嘿你好",
            "determine这 volks2 won",
            "",
            "一個小小的問題",
            "小小的東西",
            "我們家的小小怎麼了",
        ] {
            assert_eq!(strip_call_name(t), None, "{t}");
        }
    }

    #[test]
    fn verify_text_tolerant() {
        for t in ["嘿,小小", "黑小小", "我測一下嘿小小", "可以小小", "Hey 小小"] {
            assert!(verify_text(t), "{t}");
        }
        for t in ["那我們可以想可以", "這是小事", "嘿你好"] {
            assert!(!verify_text(t), "{t}");
        }
    }
}

//! 喚醒第二關：Whisper 轉出的文字是否含「嘿，小小」。

/// Whisper 常把「小小」寫成同音字或簡體；含其中之一就算通過。
/// 注意：餵給 Whisper 的 initial prompt 不可含「小小」，否則會被誘導出來。
pub fn verify_text(text: &str) -> bool {
    ["小小", "曉曉", "晓晓"].iter().any(|w| text.contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;

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

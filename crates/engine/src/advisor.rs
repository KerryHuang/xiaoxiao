//! 顧問模式的純邏輯：說法判斷、回覆處理、討論文字。

use crate::local_cmd::strip;
use regex::Regex;
use std::sync::LazyLock;

/// 進入顧問模式的說法，長的排前面以免短的先吃掉。
const ENTER: &[&str] = &[
    "我想跟你討論", "我想跟你讨论", "我們來討論", "我们来讨论", "我們討論一下", "我们讨论一下",
    "我們來想一下", "來討論一下", "来讨论一下", "想跟你討論", "想跟你讨论", "一起想一下",
    "陪我討論", "陪我想一下", "陪我想想", "跟你討論", "跟你讨论", "和你討論",
];

static END: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(小小)?(好|那|嗯)?(先這樣|先这样|就這樣|就这样|好了|今天就?到(這|这)(裡|里|邊|边)?|先到(這|这)(裡|里|邊|边)?|結束討論|结束讨论|討論結束|讨论结束|不討論了|不讨论了)(吧|了|啦|喔)?$").unwrap()
});
static YES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(小小)?(對|对|好|嗯|是|可以|要|沒錯|没错)?(啊|呀|的|喔|哦|嗯)?(幫我|帮我)?(存|存起來|存起来|存一下|存吧)?(吧|喔|啊)?$").unwrap()
});
static NO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(小小)?(不用|不要|不必|不需要|免了|算了|不存)(存)?(了|啦|吧)?$").unwrap());

fn is_punct(c: char) -> bool {
    c.is_whitespace() || "，,。.！!？?、~～".contains(c)
}

/// 句首是進入顧問模式的說法就回傳後半句（可能是空字串），否則 `None`。
pub fn enter_advising(text: &str) -> Option<&str> {
    let mut s = text.trim_start_matches(is_punct);
    for lead in ["好", "那", "嗯"] {
        if let Some(r) = s.strip_prefix(lead) {
            s = r.trim_start_matches(is_punct);
            break;
        }
    }
    let rest = ENTER.iter().find_map(|p| s.strip_prefix(p))?;
    if rest.starts_with('的') {
        return None;
    }
    let rest = rest.strip_prefix("一下").unwrap_or(rest);
    let rest = ["好不好", "好嗎", "吧"].iter().find_map(|p| rest.strip_prefix(p)).unwrap_or(rest);
    Some(rest.trim_start_matches(is_punct))
}

/// 整句只有結束說法才是 `true`。
pub fn is_end_advising(text: &str) -> bool {
    END.is_match(&strip(text))
}

/// 同意 `Some(true)`、拒絕 `Some(false)`、其他 `None`。
pub fn parse_save_reply(text: &str) -> Option<bool> {
    let t = strip(text);
    if NO.is_match(&t) {
        return Some(false);
    }
    let caps = YES.captures(&t)?;
    (caps.get(2).is_some() || caps.get(5).is_some()).then_some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entering_phrases_are_spotted() {
        for (t, rest) in [
            ("我們來討論", ""),
            ("我們來討論一下新功能怎麼排", "新功能怎麼排"),
            ("陪我想一下", ""),
            ("跟你討論一下，這週先做哪個", "這週先做哪個"),
            ("好，我們來討論吧", ""),
            ("我们来讨论一下", ""),
            ("陪我想想要怎麼跟老闆報告", "要怎麼跟老闆報告"),
        ] {
            assert_eq!(enter_advising(t), Some(rest), "{t}");
        }
    }

    #[test]
    fn entering_phrases_need_to_lead_the_sentence() {
        for t in ["討論區在哪裡", "我們來討論的那份文件在哪", "一公斤幾公克", "幫我想一下晚餐吃什麼", "上次跟你討論的事", ""] {
            assert_eq!(enter_advising(t), None, "{t}");
        }
    }

    #[test]
    fn ending_needs_the_whole_sentence() {
        for t in ["先這樣", "先這樣吧", "好，先這樣", "好了", "今天到這", "今天就到這裡", "結束討論", "小小，先這樣"] {
            assert!(is_end_advising(t), "{t}");
        }
        for t in ["好了，那下一個問題是什麼", "先這樣做好不好", "今天到這裡要多久", "我覺得先這樣比較好"] {
            assert!(!is_end_advising(t), "{t}");
        }
    }

    #[test]
    fn save_replies_are_yes_no_or_other() {
        for t in ["對", "對啊", "好", "好的", "存", "好，存起來", "可以", "要存", "嗯"] {
            assert_eq!(parse_save_reply(t), Some(true), "{t}");
        }
        for t in ["不用", "不用了", "不要", "不要存", "算了", "不存"] {
            assert_eq!(parse_save_reply(t), Some(false), "{t}");
        }
        for t in ["等等，再補一點", "好了", "先這樣", "那匯出呢"] {
            assert_eq!(parse_save_reply(t), None, "{t}");
        }
    }
}

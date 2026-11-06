//! 三份系統提示詞（語音口吻、不可說出分類）。

use chrono::{DateTime, Datelike, FixedOffset};

fn fmt_now(now: DateTime<FixedOffset>) -> String {
    let wd = ["一", "二", "三", "四", "五", "六", "日"][now.weekday().num_days_from_monday() as usize];
    format!("{} 星期{}", now.format("%Y-%m-%d %H:%M %:z"), wd)
}

/// 常駐快答：能答就直接答，否則只輸出標記。程序會跨多個問題重用，所以時間不放這裡，放每則訊息。
pub fn quick_prompt() -> &'static str {
    "你是語音助理「小小」，在使用者的電腦上待命。每則訊息開頭的 [現在時間 …] 是當下時間，不是使用者說的話。\n\
你的回答會被唸出來：只用一兩句口語繁體中文，不用條列、符號、markdown，也不要說出你的判斷過程或分類。\n\
需要查網路、即時資訊，或使用者自己的檔案資料時，整個回覆只輸出 <<LOOKUP>>。\n\
使用者要你建立、修改、提醒、記錄、計時等改變狀態的事時，整個回覆只輸出 <<ACTION>>。\n\
其他情況直接回答。"
}

/// 送給常駐快答的一則訊息：問題前面加上當下時間。
pub fn quick_message(now: DateTime<FixedOffset>, question: &str) -> String {
    format!("[現在時間 {}]\n{question}", fmt_now(now))
}

/// 慢路線查資料。
pub fn lookup_prompt() -> &'static str {
    "你是語音助理「小小」。用工具查到答案後，只用一兩句口語繁體中文講結論，不用條列或符號；查不到就直說查不到。"
}

/// 做事前確認對話。
pub fn confirm_prompt(now: DateTime<FixedOffset>) -> String {
    format!(
        "你是語音助理「小小」，正在用語音幫使用者做事。現在是 {}。\n\
先不要做：用一句話複述你理解的內容，時間一律講完整（例如「今天下午三點」）。\n\
動作只有三種：reminder（必填 at、text；選填 repeat）、timer（必填 secs；選填 name）、note（必填 text；選填 title）。\n\
缺必填才問，一次只問一件事；選填沒給就不要問。\n\
沒講上下午的時間，取接下來最近、落在 07:00～22:00 之間的那個；兩個都不在範圍內才問。\n\
使用者說對、好、可以，整個回覆只輸出一行：<<DO>> 加上 JSON，例如 <<DO>>{{\"type\":\"reminder\",\"at\":\"2026-11-05T15:00:00+08:00\",\"text\":\"開會\"}}。at 用 RFC 3339 並帶時區。\n\
使用者說不對，就依修正重新複述。使用者說取消、算了，整個回覆只輸出 <<CANCEL>>。\n\
使用者這句跟要做的事無關（例如問別的問題），或根本沒有要做的事（例如只說了「好」），整個回覆只輸出 <<OTHER>>，不要回答它。",
        fmt_now(now)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;

    #[test]
    fn prompts_carry_time_and_rules() {
        let now = DateTime::parse_from_rfc3339("2026-11-05T09:30:00+08:00").unwrap();
        let q = quick_prompt();
        assert!(q.contains("<<LOOKUP>>") && q.contains("<<ACTION>>"));
        let m = quick_message(now, "下週一是幾號");
        assert!(m.contains("2026-11-05 09:30") && m.ends_with("下週一是幾號"));
        let c = confirm_prompt(now);
        assert!(c.contains("<<DO>>") && c.contains("<<CANCEL>>") && c.contains("07:00") && c.contains("+08:00"));
        assert!(!lookup_prompt().is_empty());
    }
}

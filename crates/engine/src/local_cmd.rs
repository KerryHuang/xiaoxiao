//! 本機固定指令：不經 Claude、0 token。

use crate::cn_num::parse_cn_number;
use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalCmd {
    Time,
    Date,
    StartTimer { secs: u32 },
    CancelTimer,
    Rest,
}

const NUM: &str = r"([0-9零一二兩两三四五六七八九十]+|半)";

static TIME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(現在|现在)?(幾|几)(點|点)(了|鐘|钟)?$").unwrap());
static DATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(今天)?((幾|几)(號|号)|星期(幾|几)|禮拜(幾|几)|礼拜(幾|几))$").unwrap());
static CANCEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(取消|停止)(計|计)(時|时)$").unwrap());
static REST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(小小)?(你)?(先|可以|可以去|去)?休息(一下)?(了|吧)?$").unwrap());
static TIMER_A: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^(計|计)(時|时){NUM}(個|个)?(秒(鐘|钟)?|分(鐘|钟)?|小(時|时))$")).unwrap()
});
static TIMER_B: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^(幫我)?(設|设)(一個|一个)?{NUM}(個|个)?(秒(鐘|钟)?|分(鐘|钟)?|小(時|时))(的)?(計|计)(時|时)(器)?$")).unwrap()
});

pub(crate) fn strip(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace() && !"，,。.！!？?、~～".contains(*c))
        .collect()
}

fn secs(num: &str, unit: &str) -> Option<u32> {
    let per = if unit.starts_with('秒') { 1 } else if unit.starts_with('分') { 60 } else { 3600 };
    if num == "半" {
        return (per >= 60).then_some(per / 2);
    }
    Some(parse_cn_number(num)? * per)
}

/// 整句比對本機固定指令；句子裡有別的需求就回 `None`，交給 Claude。
pub fn parse_local(text: &str) -> Option<LocalCmd> {
    let t = strip(text);
    if TIME.is_match(&t) {
        return Some(LocalCmd::Time);
    }
    if DATE.is_match(&t) {
        return Some(LocalCmd::Date);
    }
    if CANCEL.is_match(&t) {
        return Some(LocalCmd::CancelTimer);
    }
    if REST.is_match(&t) {
        return Some(LocalCmd::Rest);
    }
    for re in [&*TIMER_A, &*TIMER_B] {
        if let Some(c) = re.captures(&t) {
            let num = c.iter().skip(1).flatten().map(|m| m.as_str()).find(|s| {
                *s == "半" || s.chars().all(|ch| ch.is_ascii_digit() || "零一二兩两三四五六七八九十".contains(ch))
            })?;
            let unit = c.iter().skip(1).flatten().map(|m| m.as_str())
                .find(|s| s.starts_with('秒') || s.starts_with('分') || s.starts_with('小'))?;
            return secs(num, unit).map(|secs| LocalCmd::StartTimer { secs });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_commands() {
        use LocalCmd::*;
        assert_eq!(parse_local("現在幾點"), Some(Time));
        assert_eq!(parse_local("几点了"), Some(Time));
        assert_eq!(parse_local("今天幾號"), Some(Date));
        assert_eq!(parse_local("今天星期幾"), Some(Date));
        assert_eq!(parse_local("計時十分鐘"), Some(StartTimer { secs: 600 }));
        assert_eq!(parse_local("设一个5分钟计时"), Some(StartTimer { secs: 300 }));
        assert_eq!(parse_local("計時半小時"), Some(StartTimer { secs: 1800 }));
        assert_eq!(parse_local("計時三十秒"), Some(StartTimer { secs: 30 }));
        assert_eq!(parse_local("取消計時"), Some(CancelTimer));
        assert_eq!(parse_local("小小先休息"), Some(Rest));
        assert_eq!(parse_local("提醒我三點開會"), None);
        assert_eq!(parse_local("台北現在幾點的天氣怎樣"), None);
    }

    #[test]
    fn common_ways_to_say_rest() {
        for s in ["小小你可以休息了", "小小，去休息吧", "你可以去休息了", "休息吧", "小小休息一下", "小小，先休息"] {
            assert_eq!(parse_local(s), Some(LocalCmd::Rest), "{s}");
        }
        for s in ["我想休息一下", "休息時間幾點", "小小不要休息"] {
            assert_eq!(parse_local(s), None, "{s}");
        }
    }
}

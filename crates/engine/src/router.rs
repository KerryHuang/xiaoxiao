//! 快答輸出分流：直接回答、轉查資料、轉做事。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Answer,
    Lookup,
    Action,
}

const LOOKUP: &str = "<<LOOKUP>>";
const ACTION: &str = "<<ACTION>>";

/// 依串流到目前為止的文字判斷路線；還無法判斷回 `None`（最多等前 12 個字元）。
pub fn route_prefix(buf: &str) -> Option<Route> {
    let t = buf.trim_start();
    if t.is_empty() {
        return None;
    }
    if t.starts_with(LOOKUP) {
        return Some(Route::Lookup);
    }
    if t.starts_with(ACTION) {
        return Some(Route::Action);
    }
    let undecided = LOOKUP.starts_with(t) || ACTION.starts_with(t);
    if undecided && t.chars().count() < 12 {
        None
    } else {
        Some(Route::Answer)
    }
}

/// 句子明顯是要小小「做事」（提醒、鬧鐘、記錄）。快答偶爾不回 <<ACTION>>、直接說「設好了」，
/// 實際什麼都沒寫；這類句子不看快答怎麼判，一律走確認對話。
pub fn is_action_request(text: &str) -> bool {
    const KEYS: [&str; 11] = [
        "提醒我", "幫我提醒", "設提醒", "設個提醒", "設定提醒", "設鬧鐘", "設個鬧鐘", "設定鬧鐘", "記一下", "幫我記", "記下來",
    ];
    KEYS.iter().any(|k| text.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_by_prefix() {
        assert_eq!(route_prefix("<<LOO"), None);
        assert_eq!(route_prefix("<<LOOKUP>>"), Some(Route::Lookup));
        assert_eq!(route_prefix(" <<ACTION>>"), Some(Route::Action));
        assert_eq!(route_prefix("一公斤"), Some(Route::Answer));
    }

    #[test]
    fn action_requests_are_spotted() {
        for t in ["明天早上九點提醒我開會", "幫我設個提醒，下午三點打給媽媽", "設鬧鐘七點", "幫我記一下買牛奶"] {
            assert!(is_action_request(t), "{t}");
        }
        for t in ["我明天有什麼提醒", "一公斤幾公克", "鬧鐘是誰發明的", "今天天氣如何"] {
            assert!(!is_action_request(t), "{t}");
        }
    }
}

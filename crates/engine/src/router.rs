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
}

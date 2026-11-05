//! 桌面小角色的純邏輯：狀態怎麼對應造型、什麼時候要顯示、位置跑出畫面怎麼拉回來。
use crate::pause::PauseReason;
use crate::pipeline::State;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CharacterKind {
    /// 小麥
    Mic,
    /// 小圓
    Blob,
    /// 小鴞
    Owl,
    /// 小訊
    #[default]
    Pixel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Look {
    Idle,
    Listen,
    Think,
    Speak,
    Pause,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

pub fn look_for(s: State) -> Look {
    match s {
        State::Idle | State::NoMic | State::Degraded => Look::Idle,
        State::Acking | State::Listening | State::FollowUp | State::Confirming => Look::Listen,
        State::Thinking => Look::Think,
        State::Speaking => Look::Speak,
        State::Paused => Look::Pause,
    }
}

pub fn is_warning(s: State) -> bool {
    matches!(s, State::NoMic | State::Degraded)
}

/// 手動隱藏優先：通話結束也不會自己跑回來。休息和鎖定時留在畫面上，只有通話要收起來。
pub fn character_visible(onboarded: bool, onboarding_open: bool, user_hidden: bool, pause: Option<PauseReason>) -> bool {
    onboarded && !onboarding_open && !user_hidden && pause != Some(PauseReason::Call)
}

/// 完整落在某個螢幕內就不動；否則拉回重疊最多的螢幕，都沒重疊就用第一個。
pub fn clamp_to_monitors(x: i32, y: i32, w: u32, h: u32, monitors: &[Rect]) -> (i32, i32) {
    let overlap = |m: &Rect| {
        let dx = (x + w as i32).min(m.x + m.w as i32) - x.max(m.x);
        let dy = (y + h as i32).min(m.y + m.h as i32) - y.max(m.y);
        i64::from(dx.max(0)) * i64::from(dy.max(0))
    };
    let mut best: Option<(&Rect, i64)> = None;
    for m in monitors {
        let o = overlap(m);
        if best.is_none_or(|(_, b)| o > b) {
            best = Some((m, o));
        }
    }
    let Some((m, _)) = best else { return (x, y) };
    let max_x = (m.x + m.w as i32 - w as i32).max(m.x);
    let max_y = (m.y + m.h as i32 - h as i32).max(m.y);
    (x.clamp(m.x, max_x), y.clamp(m.y, max_y))
}

/// 讀角色位置；沒有檔案或檔案壞掉就回 None，用預設位置。
pub fn load_pos(path: &Path) -> Option<(i32, i32)> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

pub fn save_pos(path: &Path, pos: (i32, i32)) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(path, serde_json::to_string(&pos)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn look_matches_spec_table() {
        assert_eq!(look_for(State::Idle), Look::Idle);
        for s in [State::Acking, State::Listening, State::FollowUp, State::Confirming] {
            assert_eq!(look_for(s), Look::Listen);
        }
        assert_eq!(look_for(State::Thinking), Look::Think);
        assert_eq!(look_for(State::Speaking), Look::Speak);
        assert_eq!(look_for(State::Paused), Look::Pause);
        for s in [State::NoMic, State::Degraded] {
            assert_eq!(look_for(s), Look::Idle);
            assert!(is_warning(s));
        }
        assert!(!is_warning(State::Idle));
    }

    #[test]
    fn hidden_during_call_and_onboarding() {
        assert!(character_visible(true, false, false, None));
        assert!(character_visible(true, false, false, Some(PauseReason::Rest)));
        assert!(character_visible(true, false, false, Some(PauseReason::Lock)));
        assert!(!character_visible(true, false, false, Some(PauseReason::Call)));
        assert!(!character_visible(false, false, false, None));
        assert!(!character_visible(true, true, false, None));
    }

    #[test]
    fn hidden_stays_hidden_after_call() {
        assert!(!character_visible(true, false, true, None));
    }

    #[test]
    fn clamp_keeps_position_inside_a_monitor() {
        let m = [Rect { x: 0, y: 0, w: 1000, h: 800 }];
        assert_eq!(clamp_to_monitors(100, 100, 96, 96, &m), (100, 100));
    }

    #[test]
    fn clamp_moves_back_when_monitor_gone() {
        let m = [Rect { x: 0, y: 0, w: 1000, h: 800 }];
        let (x, y) = clamp_to_monitors(2500, 300, 96, 96, &m);
        assert!((0..=904).contains(&x) && (0..=704).contains(&y), "{x},{y}");
    }

    #[test]
    fn clamp_uses_monitor_with_most_overlap() {
        let m = [Rect { x: 0, y: 0, w: 1000, h: 800 }, Rect { x: 1000, y: 0, w: 1000, h: 800 }];
        // 一半跨在第二個螢幕，超出它的右邊界
        assert_eq!(clamp_to_monitors(1950, 100, 96, 96, &m), (1904, 100));
    }

    #[test]
    fn pos_file_roundtrip() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("sub/pos.json");
        assert_eq!(load_pos(&p), None);
        save_pos(&p, (-30, 400)).unwrap();
        assert_eq!(load_pos(&p), Some((-30, 400)));
    }

    #[test]
    fn pos_file_broken_returns_none() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("pos.json");
        std::fs::write(&p, "壞掉了").unwrap();
        assert_eq!(load_pos(&p), None);
    }

    #[test]
    fn clamp_without_monitors_keeps_position() {
        assert_eq!(clamp_to_monitors(-5, 7, 96, 96, &[]), (-5, 7));
    }
}

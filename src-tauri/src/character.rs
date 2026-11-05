//! 桌面小角色視窗：什麼時候出現、顯示什麼。對應規則在 engine::character。
use std::sync::Mutex;

use engine::character::{CharacterKind, Look, character_visible, is_warning, look_for};
use engine::pause::PauseReason;
use engine::pipeline::State;
use tauri::{AppHandle, Emitter, Manager};

use crate::engine_host::Host;
use crate::tray::status_text;

const LABEL: &str = "character";

/// 引擎起來之前也要有東西可以顯示。
#[derive(Debug, Clone, PartialEq)]
enum Status {
    Loading(String),
    Problem(String),
    Engine(State),
}

#[derive(Default)]
struct Inner {
    status: Option<Status>,
    pause: Option<PauseReason>,
    user_hidden: bool,
    onboarding_open: bool,
}

/// 都只在記憶體：重開小小一律回到預設。
#[derive(Default)]
pub struct CharacterState(Mutex<Inner>);

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CharacterView {
    kind: CharacterKind,
    look: Look,
    warning: bool,
    /// 滑過角色時顯示的文字。
    hint: String,
}

fn view(kind: CharacterKind, status: &Status, pause: Option<PauseReason>) -> CharacterView {
    let (look, warning, hint) = match status {
        Status::Loading(m) => (Look::Think, false, m.clone()),
        Status::Problem(m) => (Look::Idle, true, m.clone()),
        Status::Engine(State::Paused) if pause == Some(PauseReason::Rest) => {
            (Look::Pause, false, "休息中：點一下叫醒".to_string())
        }
        Status::Engine(s) => (look_for(*s), is_warning(*s), status_text(*s).to_string()),
    };
    CharacterView { kind, look, warning, hint }
}

fn current_view(app: &AppHandle) -> CharacterView {
    let kind = app.state::<Host>().settings().character;
    let st = app.state::<CharacterState>();
    let g = st.0.lock().unwrap();
    let status = g.status.clone().unwrap_or(Status::Loading("載入中…".to_string()));
    view(kind, &status, g.pause)
}

pub fn emit_view(app: &AppHandle) {
    let _ = app.emit_to(LABEL, "character-view", current_view(app));
}

fn set_status(app: &AppHandle, s: Status) {
    app.state::<CharacterState>().0.lock().unwrap().status = Some(s);
    emit_view(app);
}

pub fn on_loading(app: &AppHandle, msg: &str) {
    set_status(app, Status::Loading(msg.to_string()));
}

pub fn on_problem(app: &AppHandle, msg: &str) {
    set_status(app, Status::Problem(msg.to_string()));
}

pub fn on_state(app: &AppHandle, s: State) {
    set_status(app, Status::Engine(s));
}

pub fn on_pause(app: &AppHandle, p: Option<PauseReason>) {
    app.state::<CharacterState>().0.lock().unwrap().pause = p;
    emit_view(app);
    sync(app);
}

/// 依目前條件顯示或收起角色。不要拿著 CharacterState 的鎖呼叫視窗 API。
pub fn sync(app: &AppHandle) {
    let onboarded = app.state::<Host>().settings().onboarded;
    let visible = {
        let st = app.state::<CharacterState>();
        let g = st.0.lock().unwrap();
        character_visible(onboarded, g.onboarding_open, g.user_hidden, g.pause)
    };
    let Some(w) = app.get_webview_window(LABEL) else { return };
    let _ = if visible { w.show() } else { w.hide() };
}

#[tauri::command]
pub fn character_state(app: AppHandle) -> CharacterView {
    current_view(&app)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: Status, p: Option<PauseReason>) -> CharacterView {
        view(CharacterKind::Pixel, &s, p)
    }

    #[test]
    fn loading_looks_busy_without_warning() {
        let got = v(Status::Loading("載入中…".into()), None);
        assert_eq!((got.look, got.warning, got.hint.as_str()), (Look::Think, false, "載入中…"));
    }

    #[test]
    fn problem_shows_warning_with_reason() {
        let got = v(Status::Problem("模型下載失敗".into()), None);
        assert_eq!((got.look, got.warning, got.hint.as_str()), (Look::Idle, true, "模型下載失敗"));
    }

    #[test]
    fn no_mic_uses_same_text_as_tray() {
        let got = v(Status::Engine(State::NoMic), None);
        assert_eq!((got.look, got.warning), (Look::Idle, true));
        assert_eq!(got.hint, status_text(State::NoMic));
    }

    #[test]
    fn rest_says_how_to_wake() {
        let got = v(Status::Engine(State::Paused), Some(PauseReason::Rest));
        assert_eq!(got.look, Look::Pause);
        assert!(got.hint.contains("叫醒"), "{}", got.hint);
    }

    #[test]
    fn locked_pause_is_not_called_rest() {
        let got = v(Status::Engine(State::Paused), Some(PauseReason::Lock));
        assert_eq!(got.look, Look::Pause);
        assert!(!got.hint.contains("叫醒"), "{}", got.hint);
    }
}

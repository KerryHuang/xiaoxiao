//! 桌面小角色視窗：什麼時候出現、顯示什麼。對應規則在 engine::character。
use std::sync::Mutex;
use std::time::{Duration, Instant};

use engine::character::{
    CharacterKind, Look, Rect, character_visible, clamp_to_monitors, default_pos, is_warning, load_pos, look_for, save_pos,
};
use engine::pause::PauseReason;
use engine::pipeline::{State, Trigger};
use tauri::menu::{Menu, MenuItem};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow};

use crate::engine_host::Host;
use crate::tray::status_text;

const LABEL: &str = "character";
/// 拖曳時視窗一直在動，停下來這麼久才寫檔。
const SAVE_AFTER: Duration = Duration::from_millis(500);

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
pub struct CharacterState {
    inner: Mutex<Inner>,
    /// 等著寫檔的位置與最後移動時間；有值代表寫檔的執行緒在跑。
    unsaved_pos: Mutex<Option<((i32, i32), Instant)>>,
}

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
    let g = st.inner.lock().unwrap();
    let status = g.status.clone().unwrap_or(Status::Loading("載入中…".to_string()));
    view(kind, &status, g.pause)
}

pub fn emit_view(app: &AppHandle) {
    let _ = app.emit_to(LABEL, "character-view", current_view(app));
}

fn set_status(app: &AppHandle, s: Status) {
    app.state::<CharacterState>().inner.lock().unwrap().status = Some(s);
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
    app.state::<CharacterState>().inner.lock().unwrap().pause = p;
    emit_view(app);
    sync(app);
}

/// 依目前條件顯示或收起角色。不要拿著 CharacterState 的鎖呼叫視窗 API。
pub fn sync(app: &AppHandle) {
    let onboarded = app.state::<Host>().settings().onboarded;
    let visible = {
        let st = app.state::<CharacterState>();
        let g = st.inner.lock().unwrap();
        character_visible(onboarded, g.onboarding_open, g.user_hidden, g.pause)
    };
    let Some(w) = app.get_webview_window(LABEL) else { return };
    if !visible {
        let _ = w.hide();
    } else if !w.is_visible().unwrap_or(false) {
        place(app, &w);
        let _ = w.show();
    }
}

/// 每次出現前都檢查一次，螢幕可能已經換過。
fn place(app: &AppHandle, w: &WebviewWindow) {
    let Ok(size) = w.outer_size() else { return };
    let areas = |m: &tauri::Monitor| {
        let a = m.work_area();
        Rect { x: a.position.x, y: a.position.y, w: a.size.width, h: a.size.height }
    };
    let monitors: Vec<Rect> = w.available_monitors().unwrap_or_default().iter().map(areas).collect();
    let (x, y) = match load_pos(&app.state::<Host>().character_pos_path()) {
        Some((x, y)) => clamp_to_monitors(x, y, size.width, size.height, &monitors),
        None => {
            let Ok(Some(m)) = w.primary_monitor() else { return };
            let margin = (24.0 * m.scale_factor()) as u32;
            default_pos(&areas(&m), size.width, size.height, margin)
        }
    };
    let _ = w.set_position(PhysicalPosition::new(x, y));
}

/// 視窗移動後等它停下來才存位置，只存使用者看得到的移動。
pub fn on_moved(app: &AppHandle, x: i32, y: i32) {
    let visible = app.get_webview_window(LABEL).is_some_and(|w| w.is_visible().unwrap_or(false));
    if !visible {
        return;
    }
    let st = app.state::<CharacterState>();
    let mut pending = st.unsaved_pos.lock().unwrap();
    let saver_running = pending.is_some();
    *pending = Some(((x, y), Instant::now()));
    if saver_running {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let path = app.state::<Host>().character_pos_path();
        let st = app.state::<CharacterState>();
        loop {
            std::thread::sleep(Duration::from_millis(100));
            let mut pending = st.unsaved_pos.lock().unwrap();
            if let Some((pos, at)) = *pending
                && at.elapsed() >= SAVE_AFTER
            {
                *pending = None;
                drop(pending);
                if let Err(e) = save_pos(&path, pos) {
                    eprintln!("角色位置存不了：{e}");
                }
                return;
            }
        }
    });
}

/// 手動隱藏或叫回；通話結束不會蓋掉使用者的選擇。
pub fn set_hidden(app: &AppHandle, hidden: bool) {
    app.state::<CharacterState>().inner.lock().unwrap().user_hidden = hidden;
    crate::tray::show_character_hidden(app, hidden);
    sync(app);
}

pub fn is_hidden(app: &AppHandle) -> bool {
    app.state::<CharacterState>().inner.lock().unwrap().user_hidden
}

pub fn set_onboarding(app: &AppHandle, open: bool) {
    app.state::<CharacterState>().inner.lock().unwrap().onboarding_open = open;
    sync(app);
}

/// 再打開一次 app：完成引導就叫回角色，否則開引導視窗。
pub fn reopen(app: &AppHandle) {
    let Some(host) = app.try_state::<Host>() else { return };
    if host.settings().onboarded {
        set_hidden(app, false);
    } else {
        crate::engine_host::show_window(app);
    }
}

#[tauri::command]
pub fn character_state(app: AppHandle) -> CharacterView {
    current_view(&app)
}

/// 左鍵點一下等於按快捷鍵；休息中就是叫醒。
#[tauri::command]
pub fn character_click(host: tauri::State<Host>) {
    host.trigger(Trigger::TrayClick);
}

/// 選單事件由 tray 的 on_menu 處理，tauri 會把所有選單的事件都送過去。
#[tauri::command]
pub fn character_menu(app: AppHandle, window: tauri::Window, host: tauri::State<Host>) -> Result<(), String> {
    let e = |e: tauri::Error| e.to_string();
    let rest = MenuItem::with_id(&app, "rest", crate::tray::rest_text(host.resting()), true, None::<&str>).map_err(e)?;
    let settings = MenuItem::with_id(&app, "settings", "設定…", true, None::<&str>).map_err(e)?;
    let hide = MenuItem::with_id(&app, "hide_character", "隱藏", true, None::<&str>).map_err(e)?;
    let quit = MenuItem::with_id(&app, "quit", "結束小小", true, None::<&str>).map_err(e)?;
    let menu = Menu::with_items(&app, &[&rest, &settings, &hide, &quit]).map_err(e)?;
    window.popup_menu(&menu).map_err(e)
}

#[tauri::command]
pub fn set_onboarding_open(app: AppHandle, open: bool) {
    set_onboarding(&app, open);
}

/// 引導走完：關主視窗，換角色出場。
#[tauri::command]
pub fn finish_onboarding(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
    set_onboarding(&app, false);
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

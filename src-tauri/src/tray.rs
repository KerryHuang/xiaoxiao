//! 選單列（Windows 是系統匣）圖示與選單。
use std::sync::Mutex;

use engine::pipeline::{State, Trigger};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_notification::NotificationExt;

use crate::engine_host::Host;

const TRAY_ID: &str = "xiaoxiao";

struct Items {
    menu: Menu<Wry>,
    status: MenuItem<Wry>,
    rest: MenuItem<Wry>,
    login: MenuItem<Wry>,
    login_shown: Mutex<bool>,
}

fn icon(name: &str) -> Image<'static> {
    // Windows 工作列可能深色也可能淺色，黑色 template 圖示會看不見，改用彩色的
    #[cfg(windows)]
    let bytes: &'static [u8] = match name {
        "listening" => include_bytes!("../icons/tray-win-listening.png"),
        "paused" => include_bytes!("../icons/tray-win-paused.png"),
        "warning" => include_bytes!("../icons/tray-win-warning.png"),
        _ => include_bytes!("../icons/tray-win-idle.png"),
    };
    #[cfg(not(windows))]
    let bytes: &'static [u8] = match name {
        "listening" => include_bytes!("../icons/tray-listening.png"),
        "paused" => include_bytes!("../icons/tray-paused.png"),
        "warning" => include_bytes!("../icons/tray-warning.png"),
        _ => include_bytes!("../icons/tray-idle.png"),
    };
    Image::from_bytes(bytes).expect("內建圖示")
}

pub fn status_text(s: State) -> &'static str {
    match s {
        State::Idle => "待命中：說「嘿，小小」",
        State::Acking | State::Listening => "在聽…",
        State::Thinking => "想一下…",
        State::Speaking => "說話中",
        State::FollowUp => "可以直接追問",
        State::Confirming => "等你確認",
        State::Paused => "暫停中",
        State::NoMic => "找不到可用的麥克風",
        State::Degraded => "連不上 Claude：只能報時和計時",
    }
}

fn icon_for(s: State) -> &'static str {
    match s {
        State::Idle | State::FollowUp => "idle",
        State::Acking | State::Listening | State::Thinking | State::Speaking | State::Confirming => "listening",
        State::Paused => "paused",
        State::NoMic | State::Degraded => "warning",
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, "status", "啟動中…", false, None::<&str>)?;
    let rest = MenuItem::with_id(app, "rest", "休息", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "設定…", true, None::<&str>)?;
    let login = MenuItem::with_id(app, "login", "去登入 Claude", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "結束小小", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&status, &sep, &rest, &settings, &quit])?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon("idle"))
        .icon_as_template(true)
        .tooltip("小小")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| on_menu(app, ev.id().as_ref()))
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                tray.app_handle().state::<Host>().trigger(Trigger::TrayClick);
            }
        })
        .build(app)?;
    app.manage(Items { menu, status, rest, login, login_shown: Mutex::new(false) });
    Ok(())
}

fn on_menu(app: &AppHandle, id: &str) {
    let host = app.state::<Host>();
    match id {
        "rest" => {
            let on = !host.resting();
            if on {
                host.set_resting(true);
            } else {
                host.trigger(Trigger::TrayClick);
            }
            show_rest(app, on);
        }
        "settings" => crate::engine_host::show_window(app),
        "login" => open_login(app, &host),
        "quit" => {
            host.shutdown();
            app.exit(0);
        }
        _ => {}
    }
}

/// 開終端機執行使用者自己的 claude，讓使用者在官方 CLI 裡登入；小小不碰帳號。
fn open_login(app: &AppHandle, host: &Host) {
    crate::os::open_terminal(&host.claude_bin());
    let _ = app
        .notification()
        .builder()
        .title("登入 Claude")
        .body("在打開的終端機裡輸入 /login 並照指示登入，完成後關掉終端機即可。")
        .show();
}

fn set(app: &AppHandle, text: &str, icon_name: &str, show_login: bool) {
    let Some(items) = app.try_state::<Items>() else { return };
    let _ = items.status.set_text(text);
    if let Some(t) = app.tray_by_id(TRAY_ID) {
        let _ = t.set_icon(Some(icon(icon_name)));
        let _ = t.set_icon_as_template(true);
    }
    let mut shown = items.login_shown.lock().unwrap();
    if show_login != *shown {
        let r = if show_login { items.menu.insert(&items.login, 2) } else { items.menu.remove(&items.login) };
        if r.is_ok() {
            *shown = show_login;
        }
    }
}

pub fn show_state(app: &AppHandle, s: State) {
    set(app, status_text(s), icon_for(s), s == State::Degraded);
}

pub fn show_rest(app: &AppHandle, resting: bool) {
    let _ = app.state::<Items>().rest.set_text(if resting { "叫醒" } else { "休息" });
}

pub fn show_problem(app: &AppHandle, msg: &str) {
    set(app, msg, "warning", false);
}

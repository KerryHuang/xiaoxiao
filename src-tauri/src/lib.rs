mod character;
mod engine_host;
mod models;
mod os;
mod tray;

use tauri::{Manager, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 兩個小小會互相當成「別的程式在用麥克風」而一直暫停；要最先註冊
        .plugin(tauri_plugin_single_instance::init(|app, _, _| engine_host::show_window(app)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            // 只出現在選單列，不出現在 Dock
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            // 不開機自動啟動，使用者打開才啟動；舊版註冊過的登入項目要清掉
            if !cfg!(debug_assertions) {
                let al = app.autolaunch();
                if al.is_enabled().unwrap_or(false) {
                    let _ = al.disable();
                }
            }
            let host = engine_host::Host::new(app.handle())?;
            app.manage(host.clone());
            app.manage(character::CharacterState::default());
            tray::build(app.handle())?;
            host.restart();
            Ok(())
        })
        .on_window_event(|w, e| {
            // 關設定視窗只是藏起來，小小繼續待命
            if let WindowEvent::CloseRequested { api, .. } = e {
                api.prevent_close();
                let _ = w.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            engine_host::get_settings,
            engine_host::ensure_models,
            engine_host::save_settings,
            engine_host::list_inputs,
            engine_host::input_level,
            engine_host::stop_input_level,
            engine_host::request_mic_permission,
            engine_host::check_claude,
            engine_host::check_voice,
            engine_host::test_wake,
            character::character_state,
        ])
        .run(tauri::generate_context!())
        .expect("小小啟動失敗");
}

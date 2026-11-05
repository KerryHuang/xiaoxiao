mod engine_host;
mod models;
mod tray;

use tauri::{Manager, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            // 只出現在選單列，不出現在 Dock
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            // 開機自動啟動預設開啟；開發版不註冊，免得把 debug 執行檔設成登入項目
            if !cfg!(debug_assertions) {
                let al = app.autolaunch();
                if !al.is_enabled().unwrap_or(false) {
                    let _ = al.enable();
                }
            }
            let host = engine_host::Host::new(app.handle())?;
            app.manage(host.clone());
            tray::build(app.handle())?;
            host.restart();
            Ok(())
        })
        .on_window_event(|w, e| {
            // 關設定視窗只是藏起來，小小繼續在選單列待命
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
            engine_host::check_claude,
            engine_host::check_voice,
            engine_host::test_wake,
        ])
        .run(tauri::generate_context!())
        .expect("小小啟動失敗");
}

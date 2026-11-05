//! 把引擎跑在背景執行緒，並提供給 UI 的 commands。
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use chrono::{DateTime, FixedOffset};
use engine::audio::{Agc, RingBuffer, rms};
use engine::claude::{ClaudeCfg, find_claude};
use engine::config::{Settings, home};
use engine::pipeline::{Engine, EngineHandle, Platform, Trigger};
use engine::platform::*;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_notification::NotificationExt;

use crate::os::{self, OsAudio, OsDevices, OsMicUsage, OsScreen, OsSpeaker};
use crate::tray;

const KWS_DIR: &str = "kws";
const SMALL: &str = "ggml-small.bin";
const TURBO: &str = "ggml-large-v3-turbo-q5_0.bin";

// ---------- 平台 ----------

struct TauriNotifier(AppHandle);

impl Notifier for TauriNotifier {
    fn notify(&self, title: &str, body: &str) {
        let _ = self.0.notification().builder().title(title).body(body).show();
    }
}

pub struct OsPlatform {
    audio: OsAudio,
    spotter: kws::Spotter,
    small: asr::Whisper,
    turbo: asr::Whisper,
    mic: OsMicUsage,
    speaker: OsSpeaker,
    notifier: TauriNotifier,
    claude: ClaudeCfg,
    data: PathBuf,
    logs: PathBuf,
}

impl OsPlatform {
    fn load(app: &AppHandle, paths: &Paths, settings: &Settings) -> Result<OsPlatform, String> {
        let models = os::model_path(&paths.models())?;
        Ok(OsPlatform {
            audio: os::new_audio(),
            spotter: kws::Spotter::new(&models.join(KWS_DIR))?,
            small: asr::Whisper::new(&models.join(SMALL))?,
            turbo: asr::Whisper::new(&models.join(TURBO))?,
            mic: OsMicUsage::new(),
            speaker: OsSpeaker,
            notifier: TauriNotifier(app.clone()),
            claude: claude_cfg(paths, settings),
            data: paths.data.clone(),
            logs: paths.logs.clone(),
        })
    }
}

impl Platform for OsPlatform {
    fn audio(&mut self) -> &mut dyn AudioSource {
        &mut self.audio
    }
    fn spotter(&mut self) -> &mut dyn Spotter {
        &mut self.spotter
    }
    fn verifier(&self) -> &dyn Transcriber {
        &self.small
    }
    fn transcriber(&self) -> &dyn Transcriber {
        &self.turbo
    }
    fn mic_usage(&self) -> &dyn MicUsage {
        &self.mic
    }
    fn screen(&self) -> &dyn ScreenState {
        &OsScreen
    }
    fn speaker(&self) -> &dyn Speaker {
        &self.speaker
    }
    fn notifier(&self) -> &dyn Notifier {
        &self.notifier
    }
    fn claude(&self) -> ClaudeCfg {
        self.claude.clone()
    }
    fn reminders_path(&self) -> PathBuf {
        self.data.join("reminders.json")
    }
    fn events_path(&self) -> PathBuf {
        self.logs.join("events.jsonl")
    }
    fn now(&self) -> DateTime<FixedOffset> {
        chrono::Local::now().fixed_offset()
    }
}

fn search_path() -> String {
    let h = home();
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
    for d in os::claude_dirs(&h) {
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    std::env::join_paths(dirs).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
}

fn claude_cfg(paths: &Paths, settings: &Settings) -> ClaudeCfg {
    let path = search_path();
    let bin = os::desktop_roots(&home()).iter().find_map(|r| find_claude(&path, r)).unwrap_or_else(|| "claude".into());
    let workdir = paths.data.join("workdir");
    let _ = std::fs::create_dir_all(&workdir);
    ClaudeCfg { bin, workdir, read_dirs: settings.read_dirs.clone() }
}

// ---------- 宿主 ----------

#[derive(Clone)]
pub struct Paths {
    pub settings: PathBuf,
    pub data: PathBuf,
    pub logs: PathBuf,
}

impl Paths {
    fn new(app: &AppHandle) -> Result<Paths, String> {
        let p = app.path();
        Ok(Paths {
            settings: p.app_config_dir().map_err(|e| e.to_string())?.join("settings.json"),
            data: p.app_data_dir().map_err(|e| e.to_string())?,
            logs: p.app_log_dir().map_err(|e| e.to_string())?,
        })
    }

    /// 正式：<app_data_dir>/models；開發版找不到時退回 repo 的 models/。
    pub fn models(&self) -> PathBuf {
        let installed = self.installed_models();
        if cfg!(debug_assertions) && !ready(&installed) && ready(&dev_models()) {
            return dev_models();
        }
        installed
    }

    fn installed_models(&self) -> PathBuf {
        self.data.join("models")
    }
}

fn dev_models() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../models")
}

/// 三個模型都在（只看檔案存在；雜湊由 models::ensure 負責）。
fn ready(dir: &Path) -> bool {
    dir.join(KWS_DIR).join("tokens.txt").is_file() && dir.join(SMALL).is_file() && dir.join(TURBO).is_file()
}

impl Paths {
    /// 缺模型就下載到 <app_data_dir>/models。
    fn ensure_models(&self, progress: impl Fn(u64, u64)) -> Result<(), String> {
        if ready(&self.models()) && self.models() != self.installed_models() {
            return Ok(()); // 開發版用 repo 的 models/
        }
        crate::models::ensure(&self.installed_models(), &crate::models::builtin(), progress)
    }
}

struct Running {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

struct Inner {
    app: AppHandle,
    paths: Paths,
    settings: Mutex<Settings>,
    running: Mutex<Option<Running>>,
    handle: Mutex<Option<EngineHandle>>,
    muted: AtomicBool,
    meter: Mutex<Option<(String, OsAudio)>>,
}

#[derive(Clone)]
pub struct Host(Arc<Inner>);

impl Host {
    pub fn new(app: &AppHandle) -> Result<Host, String> {
        let paths = Paths::new(app)?;
        let mut settings = Settings::load(&paths.settings).unwrap_or_else(|e| {
            eprintln!("{e}，改用預設設定");
            Settings::default()
        });
        if let Ok(docs) = app.path().document_dir() {
            settings.use_documents_dir(&docs);
        }
        Ok(Host(Arc::new(Inner {
            app: app.clone(),
            paths,
            settings: Mutex::new(settings),
            running: Mutex::new(None),
            handle: Mutex::new(None),
            muted: AtomicBool::new(false),
            meter: Mutex::new(None),
        })))
    }

    pub fn trigger(&self, t: Trigger) {
        if let Some(h) = self.0.handle.lock().unwrap().as_ref() {
            h.trigger(t);
        }
    }

    pub fn muted(&self) -> bool {
        self.0.muted.load(Ordering::SeqCst)
    }

    pub fn set_muted(&self, on: bool) {
        self.0.muted.store(on, Ordering::SeqCst);
        if let Some(h) = self.0.handle.lock().unwrap().as_ref() {
            h.set_muted(on);
        }
    }

    pub fn claude_bin(&self) -> PathBuf {
        claude_cfg(&self.0.paths, &self.0.settings.lock().unwrap()).bin
    }

    /// 停掉舊引擎（如果有），在背景載入模型並啟動新引擎。
    pub fn restart(&self) {
        let me = self.clone();
        std::thread::spawn(move || {
            // 先取出再放鎖：join 期間 shutdown() 還要拿這把鎖，持鎖 join 會讓主執行緒卡住
            let old = me.0.running.lock().unwrap().take();
            if let Some(old) = old {
                old.stop.store(true, Ordering::SeqCst);
                let _ = old.thread.join();
            }
            *me.0.handle.lock().unwrap() = None;
            me.register_hotkey();
            let stop = Arc::new(AtomicBool::new(false));
            let s2 = stop.clone();
            let me2 = me.clone();
            let thread = std::thread::spawn(move || me2.run_engine(&s2));
            *me.0.running.lock().unwrap() = Some(Running { stop, thread });
        });
    }

    pub fn shutdown(&self) {
        if let Some(r) = self.0.running.lock().unwrap().as_ref() {
            r.stop.store(true, Ordering::SeqCst);
        }
    }

    fn run_engine(&self, stop: &AtomicBool) {
        let settings = self.0.settings.lock().unwrap().clone();
        if !settings.onboarded {
            tray::show_problem(&self.0.app, "還沒完成設定：點「設定…」");
            show_window(&self.0.app);
            return;
        }
        if let Err(e) = settings.validate() {
            tray::show_problem(&self.0.app, &format!("回答引擎{e}"));
            return;
        }
        let app = self.0.app.clone();
        if let Err(e) = self.0.paths.ensure_models(move |d, t| {
            if t > 0 && d % (t / 100).max(1) < 65_536 {
                tray::show_problem(&app, &format!("下載模型中…{}%", d * 100 / t));
            }
        }) {
            tray::show_problem(&self.0.app, &format!("模型下載失敗：{e}"));
            return;
        }
        tray::show_problem(&self.0.app, "載入中…");
        let p = match OsPlatform::load(&self.0.app, &self.0.paths, &settings) {
            Ok(p) => p,
            Err(e) => {
                tray::show_problem(&self.0.app, &format!("無法啟動：{e}"));
                return;
            }
        };
        let mut engine = Engine::new(p, settings);
        let h = engine.handle();
        h.set_muted(self.muted());
        let app = self.0.app.clone();
        h.set_on_state(Box::new(move |s| tray::show_state(&app, s)));
        tray::show_state(&self.0.app, h.state());
        *self.0.handle.lock().unwrap() = Some(h);
        engine.run(stop);
    }

    fn register_hotkey(&self) {
        let gs = self.0.app.global_shortcut();
        let _ = gs.unregister_all();
        let key = self.0.settings.lock().unwrap().hotkey.clone();
        let me = self.clone();
        let r = gs.on_shortcut(key.as_str(), move |_app, _s, ev| {
            if ev.state == ShortcutState::Pressed {
                me.trigger(Trigger::Hotkey);
            }
        });
        if let Err(e) = r {
            eprintln!("快捷鍵 {key} 註冊失敗：{e}");
        }
    }
}

pub fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

// ---------- commands ----------

#[tauri::command]
pub fn get_settings(host: tauri::State<Host>) -> Settings {
    host.0.settings.lock().unwrap().clone()
}

#[tauri::command]
pub fn save_settings(host: tauri::State<Host>, s: Settings) -> Result<(), String> {
    s.save(&host.0.paths.settings)?;
    *host.0.settings.lock().unwrap() = s;
    host.restart();
    Ok(())
}

#[derive(Clone, serde::Serialize)]
struct Progress {
    done: u64,
    total: u64,
}

/// 下載缺少的模型，進度用 `models-progress` 事件送給 UI。
#[tauri::command]
pub async fn ensure_models(app: AppHandle, host: tauri::State<'_, Host>) -> Result<(), String> {
    let paths = host.0.paths.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let last = std::cell::Cell::new(0u64);
        paths.ensure_models(|done, total| {
            // 每 4 MB 送一次，避免事件洪水
            if done - last.get() >= 4 << 20 || done == total {
                last.set(done);
                let _ = app.emit("models-progress", Progress { done, total });
            }
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn list_inputs() -> Vec<InputDevice> {
    OsDevices.inputs()
}

/// 指定裝置最近 100 ms 的 RMS。裝置保持開著，換裝置或 stop_input_level 才關。
#[tauri::command]
pub async fn input_level(host: tauri::State<'_, Host>, device: String) -> Result<f32, String> {
    let h = host.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut m = h.0.meter.lock().unwrap();
        if m.as_ref().is_none_or(|(d, _)| *d != device) {
            let mut a = os::new_audio();
            a.open(Some(&device)).map_err(|e| format!("開不了這支麥克風：{e:?}"))?;
            *m = Some((device, a));
        }
        let a = &mut m.as_mut().unwrap().1;
        a.flush();
        let b = a.read_block().map_err(|e| format!("{e:?}"))?;
        Ok(rms(&b))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 麥克風授權狀態：authorized／denied／restricted／undetermined。
#[tauri::command]
pub fn mic_permission() -> &'static str {
    match os::mic_permission() {
        os::MicPermission::Authorized => "authorized",
        os::MicPermission::Denied => "denied",
        os::MicPermission::Restricted => "restricted",
        os::MicPermission::NotDetermined => "undetermined",
    }
}

#[tauri::command]
pub fn stop_input_level(host: tauri::State<Host>) {
    *host.0.meter.lock().unwrap() = None;
}

/// 回 Claude CLI 版本；沒找到或沒登入回中文說明。
#[tauri::command]
pub async fn check_claude(host: tauri::State<'_, Host>) -> Result<String, String> {
    let cfg = claude_cfg(&host.0.paths, &host.0.settings.lock().unwrap());
    engine::claude::check(&cfg).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn check_voice() -> bool {
    os::voice_ok()
}

/// 試喊：10 秒內有一次通過兩關喚醒就回 true。測試期間引擎靜音，避免它也回應。
#[tauri::command]
pub async fn test_wake(host: tauri::State<'_, Host>) -> Result<bool, String> {
    let h = host.inner().clone();
    let was_muted = h.muted();
    h.set_muted(true);
    let r = tauri::async_runtime::spawn_blocking(move || {
        let settings = h.0.settings.lock().unwrap().clone();
        let models = os::model_path(&h.0.paths.models())?;
        let mut spotter = kws::Spotter::new(&models.join(KWS_DIR))?;
        let small = asr::Whisper::new(&models.join(SMALL))?;
        let mut a = os::new_audio();
        a.open(settings.input_device.as_deref()).map_err(|e| format!("{e:?}"))?;
        let (mut agc, mut ring) = (Agc::default(), RingBuffer::new(3.9));
        let mut read = |agc: &mut Agc, ring: &mut RingBuffer| -> Result<Vec<f32>, String> {
            let g = agc.process(&a.read_block().map_err(|e| format!("{e:?}"))?);
            ring.push(&g);
            Ok(g)
        };
        for _ in 0..100 {
            let g = read(&mut agc, &mut ring)?;
            if spotter.accept(&g) {
                for _ in 0..4 {
                    read(&mut agc, &mut ring)?;
                }
                if engine::wake::verify_text(&small.transcribe(&ring.snapshot())?) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    })
    .await
    .map_err(|e| e.to_string())?;
    host.set_muted(was_muted);
    r
}

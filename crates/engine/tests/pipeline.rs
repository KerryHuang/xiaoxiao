//! 主狀態機：全部用假實作。
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, FixedOffset};
use engine::audio::BLOCK;
use engine::claude::ClaudeCfg;
use engine::config::Settings;
use engine::pipeline::*;
use engine::platform::*;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Silence,
    Speech,
    Wake,
    Echo,
    /// 辦公室環境音：振幅 0.004 的白噪音
    Noise,
}

enum Item {
    Block(Kind),
    Gone,
}

#[derive(Default)]
struct World {
    script: VecDeque<Item>,
    last: Option<Kind>,
    echo_returned: usize,
    echo_to_spotter: usize,
    open_results: VecDeque<Result<(), AudioError>>,
    opens: usize,
    closes: usize,
    small: VecDeque<String>,
    turbo: VecDeque<String>,
    said: Vec<String>,
    chimes: usize,
    notes: Vec<(String, String)>,
    mic_in_use: bool,
    default_changed: bool,
    seed: u32,
}

type W = Arc<Mutex<World>>;

struct FakeAudio(W);
impl AudioSource for FakeAudio {
    fn open(&mut self, _d: Option<&str>) -> Result<(), AudioError> {
        let mut w = self.0.lock().unwrap();
        w.opens += 1;
        w.default_changed = false;
        w.open_results.pop_front().unwrap_or(Ok(()))
    }
    fn read_block(&mut self) -> Result<Vec<f32>, AudioError> {
        std::thread::sleep(Duration::from_millis(1));
        let mut w = self.0.lock().unwrap();
        let k = match w.script.pop_front() {
            Some(Item::Gone) => return Err(AudioError::Gone),
            Some(Item::Block(k)) => k,
            None => Kind::Silence,
        };
        w.last = Some(k);
        if k == Kind::Echo {
            w.echo_returned += 1;
        }
        if k == Kind::Noise {
            let mut out = Vec::with_capacity(BLOCK);
            for _ in 0..BLOCK {
                w.seed = w.seed.wrapping_mul(1664525).wrapping_add(1013904223);
                out.push((w.seed as f32 / u32::MAX as f32 * 2.0 - 1.0) * 0.004);
            }
            return Ok(out);
        }
        let v = if k == Kind::Silence { 0.0 } else { 0.3 };
        Ok(vec![v; BLOCK])
    }
    fn default_changed(&self) -> bool {
        self.0.lock().unwrap().default_changed
    }
    fn flush(&mut self) {
        // 播放期間「收到」的回音要被丟掉
        self.0.lock().unwrap().script.retain(|i| !matches!(i, Item::Block(Kind::Echo)));
    }
    fn close(&mut self) {
        self.0.lock().unwrap().closes += 1;
    }
}

struct FakeSpotter(W);
impl Spotter for FakeSpotter {
    fn accept(&mut self, _b: &[f32]) -> bool {
        let mut w = self.0.lock().unwrap();
        if w.last == Some(Kind::Echo) {
            w.echo_to_spotter += 1;
        }
        matches!(w.last, Some(Kind::Wake) | Some(Kind::Echo))
    }
    fn reset(&mut self) {}
}

struct FakeAsr(W, bool);
impl Transcriber for FakeAsr {
    fn transcribe(&self, _a: &[f32]) -> Result<String, String> {
        let mut w = self.0.lock().unwrap();
        let q = if self.1 { &mut w.small } else { &mut w.turbo };
        Ok(q.pop_front().unwrap_or_default())
    }
}

struct FakeSpeaker(W);
impl Speaker for FakeSpeaker {
    fn say(&self, text: &str) -> Result<(), String> {
        let mut w = self.0.lock().unwrap();
        w.said.push(text.to_string());
        // 喇叭模式：自己的聲音被麥克風收到，排在最前面
        for _ in 0..3 {
            w.script.push_front(Item::Block(Kind::Echo));
        }
        Ok(())
    }
    fn chime(&self) {
        self.0.lock().unwrap().chimes += 1;
    }
}

struct FakeMisc(W);
impl MicUsage for FakeMisc {
    fn others_using_mic(&self) -> bool {
        self.0.lock().unwrap().mic_in_use
    }
}
impl ScreenState for FakeMisc {
    fn locked(&self) -> bool {
        false
    }
}
impl Notifier for FakeMisc {
    fn notify(&self, t: &str, b: &str) {
        self.0.lock().unwrap().notes.push((t.into(), b.into()));
    }
}

struct FakePlatform {
    audio: FakeAudio,
    spotter: FakeSpotter,
    small: FakeAsr,
    turbo: FakeAsr,
    speaker: FakeSpeaker,
    misc: FakeMisc,
    claude: ClaudeCfg,
    dir: PathBuf,
}

impl Platform for FakePlatform {
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
        &self.misc
    }
    fn screen(&self) -> &dyn ScreenState {
        &self.misc
    }
    fn speaker(&self) -> &dyn Speaker {
        &self.speaker
    }
    fn notifier(&self) -> &dyn Notifier {
        &self.misc
    }
    fn claude(&self) -> ClaudeCfg {
        self.claude.clone()
    }
    fn reminders_path(&self) -> PathBuf {
        self.dir.join("reminders.json")
    }
    fn events_path(&self) -> PathBuf {
        self.dir.join("events.jsonl")
    }
    fn now(&self) -> DateTime<FixedOffset> {
        chrono::Local::now().fixed_offset()
    }
}

fn fake_claude(dir: &Path, mode: &str) -> ClaudeCfg {
    let w = dir.join(format!("claude{}", std::env::consts::EXE_SUFFIX));
    fs::copy(env!("CARGO_BIN_EXE_fake-claude"), &w).unwrap();
    fs::write(dir.join("fake-mode.txt"), format!("{mode}\n{}\n", dir.join("claude-args.txt").display())).unwrap();
    ClaudeCfg { bin: w, workdir: dir.to_path_buf(), read_dirs: vec![] }
}

struct Rig {
    w: W,
    h: EngineHandle,
    stop: Arc<AtomicBool>,
    th: Option<std::thread::JoinHandle<()>>,
    dir: tempfile::TempDir,
}

fn timing() -> Timing {
    Timing {
        pause_check: Duration::from_millis(30),
        followup_blocks: 30,
        claude_retry: Duration::from_secs(300),
        lookup_timeout: Duration::from_secs(5),
        quick_first_token: Duration::from_secs(5),
        quick_total: Duration::from_secs(10),
        poll: Duration::from_millis(5),
    }
}

fn rig(mode: &str, setup: impl FnOnce(&mut World)) -> Rig {
    rig_in(tempfile::tempdir().unwrap(), mode, setup)
}

fn rig_in(dir: tempfile::TempDir, mode: &str, setup: impl FnOnce(&mut World)) -> Rig {
    rig_full(dir, mode, timing(), |_| {}, setup)
}

fn rig_full(
    dir: tempfile::TempDir,
    mode: &str,
    timing: Timing,
    tweak: impl FnOnce(&mut Settings),
    setup: impl FnOnce(&mut World),
) -> Rig {
    let w: W = Arc::new(Mutex::new(World::default()));
    setup(&mut w.lock().unwrap());
    let p = FakePlatform {
        audio: FakeAudio(w.clone()),
        spotter: FakeSpotter(w.clone()),
        small: FakeAsr(w.clone(), true),
        turbo: FakeAsr(w.clone(), false),
        speaker: FakeSpeaker(w.clone()),
        misc: FakeMisc(w.clone()),
        claude: fake_claude(dir.path(), mode),
        dir: dir.path().to_path_buf(),
    };
    let mut settings = Settings { notes_path: dir.path().join("notes.md"), ..Settings::default() };
    tweak(&mut settings);
    let mut e = Engine::with_timing(p, settings, timing);
    let h = e.handle();
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = stop.clone();
    let th = std::thread::spawn(move || e.run(&s2));
    Rig { w, h, stop, th: Some(th), dir }
}

impl Rig {
    fn wait(&self, what: &str, f: impl Fn(&World, State) -> bool) {
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(8) {
            if f(&self.w.lock().unwrap(), self.h.state()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let w = self.w.lock().unwrap();
        panic!("等不到：{what}；state={:?} said={:?}", self.h.state(), w.said);
    }
    fn said(&self) -> Vec<String> {
        self.w.lock().unwrap().said.clone()
    }
    fn push(&self, items: Vec<Item>) {
        self.w.lock().unwrap().script.extend(items);
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.th.take() {
            let _ = t.join();
        }
    }
}

fn n(k: Kind, count: usize) -> Vec<Item> {
    (0..count).map(|_| Item::Block(k)).collect()
}
/// 喚醒＋停頓＋講一句話＋停頓。
fn wake_and_ask() -> Vec<Item> {
    let mut v = n(Kind::Silence, 3);
    v.extend(n(Kind::Wake, 1));
    v.extend(n(Kind::Silence, 4));
    v.extend(utterance());
    v
}
fn utterance() -> Vec<Item> {
    let mut v = n(Kind::Speech, 5);
    v.extend(n(Kind::Silence, 10));
    v
}

#[test]
fn wake_then_quick_answer() {
    let r = rig("ok", |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("一公斤幾公克".into());
    });
    r.wait("回答完回待命", |w, s| w.said.len() == 2 && s == State::Idle);
    assert_eq!(r.said(), ["我在", "一公斤是1000 公克。"]);
    assert_eq!(r.w.lock().unwrap().chimes, 1);
}

#[test]
fn rejected_verify_stays_idle() {
    let r = rig("ok", |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("可以再思考".into());
    });
    r.wait("腳本跑完", |w, _| w.script.is_empty());
    std::thread::sleep(Duration::from_millis(50));
    assert!(r.said().is_empty());
    assert_eq!(r.h.state(), State::Idle);
}

#[test]
fn local_timer_no_claude() {
    let r = rig("args", |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("計時十分鐘".into());
    });
    r.wait("回覆計時", |w, _| w.said.len() == 2);
    assert_eq!(r.said()[1], "好，十分鐘");
    assert!(!r.dir.path().join("claude-args.txt").exists(), "不該呼叫 claude");
}

#[test]
fn action_confirm_do_reminder() {
    let r = rig("action", |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("明天早上九點提醒我開會".into());
        w.turbo.push_back("對".into());
    });
    r.wait("複述", |w, s| w.said.len() == 2 && s == State::Confirming);
    assert!(r.said()[1].contains("對嗎"));
    r.push(utterance());
    r.wait("設好提醒", |w, _| w.said.len() == 3);
    assert!(r.said()[2].contains("提醒設好了"), "{:?}", r.said());
    let s = fs::read_to_string(r.dir.path().join("reminders.json")).unwrap();
    assert!(s.contains("開會"));
}

#[test]
fn followup_without_wake_word() {
    let r = rig("ok", |w| {
        w.script.extend(wake_and_ask());
        w.script.extend(utterance()); // 回答完立刻追問，沒有喚醒詞
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("一公斤幾公克".into());
        w.turbo.push_back("那兩公斤呢".into());
    });
    r.wait("追問也回答", |w, _| w.said.len() == 3);
    assert_eq!(r.said(), ["我在", "一公斤是1000 公克。", "一公斤是1000 公克。"]);
}

#[test]
fn drops_audio_while_speaking() {
    let r = rig("ok", |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("一公斤幾公克".into());
    });
    r.wait("回答完回待命", |w, s| w.said.len() == 2 && s == State::Idle);
    let w = r.w.lock().unwrap();
    assert_eq!(w.echo_returned, 0, "說話期間的聲音不該被讀進來");
    assert_eq!(w.echo_to_spotter, 0);
}

#[test]
fn pauses_when_other_app_uses_mic() {
    let r = rig("ok", |_| {});
    r.wait("待命", |_, s| s == State::Idle);
    r.w.lock().unwrap().mic_in_use = true;
    r.wait("暫停並關麥克風", |w, s| s == State::Paused && w.closes >= 1);
    r.w.lock().unwrap().mic_in_use = false;
    r.wait("恢復並重開", |w, s| s == State::Idle && w.opens >= 2);
}

#[test]
fn recovers_when_audio_source_errors() {
    let r = rig("ok", |w| {
        w.script.push_back(Item::Gone);
        w.open_results.extend([Ok(()), Err(AudioError::Gone)]);
    });
    r.wait("沒有麥克風", |_, s| s == State::NoMic);
    r.wait("重開成功回待命", |w, s| s == State::Idle && w.opens >= 3);
}

#[test]
fn reopens_source_after_stream_dead() {
    let r = rig("ok", |w| {
        w.script.extend([Item::Gone, Item::Gone, Item::Gone]);
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("一公斤幾公克".into());
    });
    r.wait("重開後照常喚醒", |w, _| w.said.len() == 2);
    assert_eq!(r.said()[0], "我在");
    assert!(r.w.lock().unwrap().opens >= 4);
}

#[test]
fn degraded_when_not_logged_in() {
    let r = rig("nologin", |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("一公斤幾公克".into());
    });
    r.wait("降級", |w, s| w.said.len() == 2 && s == State::Degraded);
    assert!(r.said()[1].contains("登入"), "{:?}", r.said());
    {
        let mut w = r.w.lock().unwrap();
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("一公斤幾公克".into());
    }
    r.wait("降級文案", |w, _| w.said.len() == 4);
    assert_eq!(r.said()[3], "現在連不上 Claude，我只能報時和計時");
    {
        let mut w = r.w.lock().unwrap();
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("現在幾點".into());
    }
    r.wait("報時", |w, _| w.said.len() == 6);
    assert!(r.said()[5].starts_with("現在是"), "{:?}", r.said());
}

#[test]
fn hotkey_ignored_while_listening() {
    let r = rig("ok", |w| {
        w.script.extend(n(Kind::Silence, 2));
        w.script.extend(n(Kind::Wake, 1));
        w.script.extend(n(Kind::Silence, 4));
        w.small.push_back("嘿小小".into());
    });
    r.wait("聆聽中", |_, s| s == State::Listening);
    r.h.trigger(Trigger::Hotkey);
    r.wait("沒講話回待命", |_, s| s == State::Idle);
    assert_eq!(r.said(), ["我在"]);
}

#[test]
fn hotkey_starts_listening_from_idle() {
    let r = rig("ok", |_| {});
    r.wait("待命", |_, s| s == State::Idle);
    r.h.trigger(Trigger::TrayClick);
    r.wait("說我在", |w, _| w.said == ["我在"]);
}

#[test]
fn rest_disables_wake_until_hotkey() {
    let r = rig("ok", |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("小小先休息".into());
    });
    r.wait("休息", |w, s| w.said.len() == 2 && s == State::Paused);
    assert!(r.said()[1].contains("休息"));
    r.push(wake_and_ask());
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(r.said().len(), 2, "休息時喚醒詞無效");
    r.h.trigger(Trigger::Hotkey);
    r.wait("快捷鍵叫醒", |w, _| w.said.len() == 3);
    assert_eq!(r.said()[2], "我在");
}

#[test]
fn bluetooth_default_falls_back_or_nomic() {
    let r = rig("ok", |w| {
        w.open_results.extend([Err(AudioError::NoUsableInput), Err(AudioError::NoUsableInput), Err(AudioError::NoUsableInput)]);
    });
    r.wait("沒有可用麥克風", |_, s| s == State::NoMic);
}

#[test]
fn mute_from_tray_pauses() {
    let r = rig("ok", |_| {});
    r.wait("待命", |_, s| s == State::Idle);
    r.h.set_muted(true);
    r.wait("靜音暫停", |_, s| s == State::Paused);
    r.h.trigger(Trigger::Hotkey);
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(r.h.state(), State::Paused, "靜音只能從選單解除");
    r.h.set_muted(false);
    r.wait("解除靜音", |_, s| s == State::Idle);
}

#[test]
fn missed_reminder_notified_on_start() {
    let dir = tempfile::tempdir().unwrap();
    let store = engine::action::ReminderStore::new(dir.path().join("reminders.json"));
    let past = chrono::Local::now().fixed_offset() - chrono::Duration::hours(1);
    let future = chrono::Local::now().fixed_offset() + chrono::Duration::hours(1);
    store.add(&engine::action::Reminder { id: "a".into(), at: past, text: "繳電費".into() }).unwrap();
    store.add(&engine::action::Reminder { id: "b".into(), at: future, text: "開會".into() }).unwrap();
    let r = rig_in(dir, "ok", |_| {});
    r.wait("通知錯過的提醒", |w, _| !w.notes.is_empty());
    std::thread::sleep(Duration::from_millis(50));
    let w = r.w.lock().unwrap();
    assert_eq!(w.notes.len(), 1, "未到期的不能提前通知：{:?}", w.notes);
    assert_eq!(w.notes[0].1, "你錯過了一個提醒：繳電費");
    let s = fs::read_to_string(r.dir.path().join("reminders.json")).unwrap();
    assert!(!s.contains("繳電費") && s.contains("開會"), "過期的移除、未到期的保留：{s}");
}

#[test]
fn office_noise_does_not_loop_followup() {
    // 回答完進追問窗口（正式值 8 秒），問題的語音峰值擠出 AGC 窗口後增益拉滿；
    // 雜音不能被當成追問、更不能把 whisper 的幻覺字串送去問 Claude。
    let t = Timing { followup_blocks: 80, ..timing() };
    let r = rig_full(tempfile::tempdir().unwrap(), "ok", t, |_| {}, |w| {
        w.script.extend(n(Kind::Noise, 60));
        w.script.extend(n(Kind::Wake, 1));
        w.script.extend(n(Kind::Noise, 4));
        w.script.extend(n(Kind::Speech, 5));
        w.script.extend(n(Kind::Noise, 300));
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("一公斤幾公克".into());
        w.turbo.push_back("(字幕製作:貝爾)".into());
        w.turbo.push_back("謝謝觀看".into());
    });
    r.wait("腳本跑完", |w, _| w.script.is_empty());
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(r.said(), ["我在", "一公斤是1000 公克。"]);
    assert_eq!(r.h.state(), State::Idle);
}

#[test]
fn followup_capped_without_wake() {
    let r = rig("ok", |w| {
        w.script.extend(wake_and_ask());
        for _ in 0..4 {
            w.script.extend(utterance());
        }
        w.small.push_back("嘿小小".into());
        for q in ["一", "二", "三", "四", "五"] {
            w.turbo.push_back(q.into());
        }
    });
    r.wait("腳本跑完", |w, _| w.script.is_empty());
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(r.said().len(), 5, "連續追問最多 3 輪，之後要重新喊喚醒詞：{:?}", r.said());
}

#[test]
fn diagnostic_logs_text_and_gain() {
    let r = rig_full(tempfile::tempdir().unwrap(), "ok", timing(), |s| s.diagnostic = true, |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("一公斤幾公克".into());
    });
    r.wait("回答完", |w, s| w.said.len() == 2 && s == State::Idle);
    let log = fs::read_to_string(r.dir.path().join("events.jsonl")).unwrap();
    assert!(log.contains("\"text\":\"一公斤幾公克\""), "{log}");
    assert!(log.contains("\"text\":\"嘿小小\""), "{log}");
    assert!(log.contains("\"gain\""), "{log}");
}

#[test]
fn no_text_logged_without_diagnostic() {
    let r = rig("ok", |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("一公斤幾公克".into());
    });
    r.wait("回答完", |w, s| w.said.len() == 2 && s == State::Idle);
    let log = fs::read_to_string(r.dir.path().join("events.jsonl")).unwrap();
    assert!(!log.contains("一公斤"), "{log}");
}

#[test]
fn repeat_reminder_says_unsupported() {
    let r = rig("action-repeat", |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("每天早上九點提醒我開會".into());
        w.turbo.push_back("對".into());
    });
    r.wait("複述", |w, s| w.said.len() == 2 && s == State::Confirming);
    r.push(utterance());
    r.wait("設好提醒", |w, _| w.said.len() == 3);
    assert!(r.said()[2].contains("重複提醒還不支援"), "{:?}", r.said());
    let s = fs::read_to_string(r.dir.path().join("reminders.json")).unwrap();
    assert!(s.contains("開會"));
}

#[test]
fn reopens_when_default_input_changes() {
    let r = rig("ok", |_| {});
    r.wait("待命", |w, s| s == State::Idle && w.opens == 1);
    r.w.lock().unwrap().default_changed = true;
    r.wait("改接新的預設輸入", |w, s| s == State::Idle && w.opens == 2);
}

#[test]
fn stream_dead_reopens_without_waiting_for_check() {
    let t = Timing { pause_check: Duration::from_secs(60), ..timing() };
    let r = rig_full(tempfile::tempdir().unwrap(), "ok", t, |_| {}, |w| {
        w.script.push_back(Item::Gone);
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("一公斤幾公克".into());
    });
    r.wait("立刻重開照常喚醒", |w, _| w.said.len() == 2);
}

//! 引擎主迴圈：喚醒 → 聽寫 → 本機指令或 Claude → 說出來 → 追問／確認窗口。
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Datelike, FixedOffset, Timelike};
use serde_json::json;

use crate::action::{Action, ConfirmOutcome, Reminder, ReminderStore, append_note, parse_confirm_reply};
use crate::audio::{Agc, EndpointEvent, Endpointer, RingBuffer, SPEECH_RMS, rms};
use crate::claude::{ClaudeCfg, ClaudeError, Session, lookup};
use crate::config::Settings;
use crate::events::EventLog;
use crate::local_cmd::{LocalCmd, parse_local};
use crate::pause::should_pause;
use crate::platform::*;
use crate::prompts::{confirm_prompt, quick_prompt};
use crate::router::{Route, route_prefix};
use crate::sentence::SentenceSplitter;
use crate::wake::verify_text;

pub const DEGRADED_MSG: &str = "現在連不上 Claude，我只能報時和計時";
/// 命中後再收幾塊（0.4 秒）才複核。
const TAIL_BLOCKS: usize = 4;
/// 複核用的錄音長度：命中前 3.5 秒＋命中後 0.4 秒。
const VERIFY_SECS: f32 = 3.9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Acking,
    Listening,
    Thinking,
    Speaking,
    FollowUp,
    Confirming,
    Paused,
    NoMic,
    Degraded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Hotkey,
    TrayClick,
}

/// 時間參數；測試注入短值。
#[derive(Debug, Clone)]
pub struct Timing {
    pub pause_check: Duration,
    pub followup_blocks: usize,
    pub claude_retry: Duration,
    pub lookup_timeout: Duration,
    pub poll: Duration,
}

impl Default for Timing {
    fn default() -> Timing {
        Timing {
            pause_check: Duration::from_secs(2),
            followup_blocks: 80, // 8 秒
            claude_retry: Duration::from_secs(5 * 60),
            lookup_timeout: crate::claude::LOOKUP_TIMEOUT,
            poll: Duration::from_millis(100),
        }
    }
}

/// 把各平台元件綁在一起。
pub trait Platform: Send {
    fn audio(&mut self) -> &mut dyn AudioSource;
    fn spotter(&mut self) -> &mut dyn Spotter;
    /// 喚醒複核（whisper small）。
    fn verifier(&self) -> &dyn Transcriber;
    /// 問題聽寫（whisper turbo）。
    fn transcriber(&self) -> &dyn Transcriber;
    fn mic_usage(&self) -> &dyn MicUsage;
    fn screen(&self) -> &dyn ScreenState;
    fn speaker(&self) -> &dyn Speaker;
    fn notifier(&self) -> &dyn Notifier;
    fn claude(&self) -> ClaudeCfg;
    fn reminders_path(&self) -> PathBuf;
    fn events_path(&self) -> PathBuf;
    fn now(&self) -> DateTime<FixedOffset>;
}

type StateCb = Box<dyn Fn(State) + Send>;

struct Shared {
    state: Mutex<State>,
    trigger: AtomicBool,
    muted: AtomicBool,
    resting: AtomicBool,
    on_state: Mutex<Option<StateCb>>,
}

/// 給外殼（選單列、快捷鍵）用的遙控器；可跨執行緒。
#[derive(Clone)]
pub struct EngineHandle(Arc<Shared>);

impl EngineHandle {
    /// 只有待命、降級、追問窗口時有效；休息中也可以叫醒。
    pub fn trigger(&self, _t: Trigger) {
        let s = self.state();
        let resting_pause = s == State::Paused && self.0.resting.load(Ordering::SeqCst);
        if matches!(s, State::Idle | State::Degraded | State::FollowUp) || resting_pause {
            self.0.trigger.store(true, Ordering::SeqCst);
        }
    }
    pub fn set_muted(&self, on: bool) {
        self.0.muted.store(on, Ordering::SeqCst);
    }
    pub fn state(&self) -> State {
        *self.0.state.lock().unwrap()
    }
    pub fn set_on_state(&self, cb: StateCb) {
        *self.0.on_state.lock().unwrap() = Some(cb);
    }
}

struct Timer {
    at: DateTime<FixedOffset>,
}

pub struct Engine<P: Platform> {
    p: P,
    settings: Settings,
    timing: Timing,
    shared: Arc<Shared>,
    rt: tokio::runtime::Runtime,
    log: EventLog,
    store: ReminderStore,
    agc: Agc,
    ring: RingBuffer,
    ep: Option<Endpointer>,
    window_left: usize,
    quick: Option<Session>,
    confirm: Option<Session>,
    degraded_at: Option<Instant>,
    crashes: u32,
    timers: Vec<Timer>,
    reminders: Vec<Reminder>,
}

impl<P: Platform> Engine<P> {
    pub fn new(p: P, settings: Settings) -> Self {
        Self::with_timing(p, settings, Timing::default())
    }

    pub fn with_timing(p: P, settings: Settings, timing: Timing) -> Self {
        let log = EventLog::new(p.events_path(), settings.diagnostic);
        let store = ReminderStore::new(p.reminders_path());
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("建立 tokio runtime");
        Engine {
            p,
            settings,
            timing,
            shared: Arc::new(Shared {
                state: Mutex::new(State::Idle),
                trigger: AtomicBool::new(false),
                muted: AtomicBool::new(false),
                resting: AtomicBool::new(false),
                on_state: Mutex::new(None),
            }),
            rt,
            log,
            store,
            agc: Agc::default(),
            ring: RingBuffer::new(VERIFY_SECS),
            ep: None,
            window_left: 0,
            quick: None,
            confirm: None,
            degraded_at: None,
            crashes: 0,
            timers: vec![],
            reminders: vec![],
        }
    }

    pub fn handle(&self) -> EngineHandle {
        EngineHandle(self.shared.clone())
    }
    pub fn trigger(&self, t: Trigger) {
        self.handle().trigger(t)
    }
    pub fn set_muted(&self, on: bool) {
        self.handle().set_muted(on)
    }
    pub fn state(&self) -> State {
        self.handle().state()
    }

    // ---------- 狀態 ----------

    fn set_state(&self, s: State) {
        set_state_raw(&self.shared, &self.log, s);
    }

    fn resting(&self) -> bool {
        self.shared.resting.load(Ordering::SeqCst)
    }

    /// 待命：降級中顯示 Degraded，否則 Idle。
    fn to_idle(&mut self) {
        self.ep = None;
        self.confirm = None;
        self.set_state(if self.degraded_at.is_some() { State::Degraded } else { State::Idle });
    }

    fn want_pause(&self) -> bool {
        should_pause(
            self.p.mic_usage().others_using_mic(),
            self.p.screen().locked(),
            self.shared.muted.load(Ordering::SeqCst),
            self.resting(),
        )
    }

    // ---------- 主迴圈 ----------

    pub fn run(&mut self, stop: &AtomicBool) {
        self.startup_reminders();
        self.open_audio();
        let mut last_check = Instant::now();
        while !stop.load(Ordering::SeqCst) {
            self.fire_due();
            if last_check.elapsed() >= self.timing.pause_check {
                last_check = Instant::now();
                self.check_pause();
            }
            if self.shared.trigger.swap(false, Ordering::SeqCst) {
                self.on_trigger();
                continue;
            }
            if matches!(self.state(), State::Paused | State::NoMic) {
                std::thread::sleep(self.timing.poll);
                continue;
            }
            let Some(g) = self.read() else { continue };
            self.step(g);
        }
        self.p.audio().close();
    }

    fn open_audio(&mut self) {
        let dev = self.settings.input_device.clone();
        match self.p.audio().open(dev.as_deref()) {
            Ok(()) => {
                self.agc = Agc::default();
                self.ring.clear();
                self.p.spotter().reset();
                self.to_idle();
            }
            Err(e) => {
                self.log.log("audio", json!({"ok": false, "error": format!("{e:?}")}));
                self.set_state(State::NoMic);
            }
        }
    }

    /// 讀一塊並做 AGC；串流失效就關掉、進 NoMic，由 check_pause 每 2 秒重試。
    fn read(&mut self) -> Option<Vec<f32>> {
        match self.p.audio().read_block() {
            Ok(b) => {
                let g = self.agc.process(&b);
                self.ring.push(&g);
                Some(g)
            }
            Err(e) => {
                self.log.log("audio", json!({"ok": false, "error": format!("{e:?}")}));
                self.p.audio().close();
                self.ep = None;
                self.confirm = None;
                self.set_state(State::NoMic);
                None
            }
        }
    }

    fn check_pause(&mut self) {
        let want = self.want_pause();
        let s = self.state();
        if want && s != State::Paused {
            if s != State::NoMic {
                self.p.audio().close();
            }
            self.ep = None;
            self.confirm = None;
            self.set_state(State::Paused);
        } else if !want && matches!(s, State::Paused | State::NoMic) {
            self.open_audio();
        }
    }

    fn on_trigger(&mut self) {
        if self.resting() {
            self.shared.resting.store(false, Ordering::SeqCst);
            if self.state() == State::Paused {
                if self.want_pause() {
                    return; // 還有其他暫停理由（通話中、靜音、鎖定）
                }
                self.open_audio();
                if self.state() == State::NoMic {
                    return;
                }
            }
        }
        if matches!(self.state(), State::Idle | State::Degraded | State::FollowUp) {
            self.ack();
        }
    }

    fn step(&mut self, g: Vec<f32>) {
        match self.state() {
            State::Idle | State::Degraded => {
                if !self.resting() && self.p.spotter().accept(&g) {
                    self.verify_wake();
                }
            }
            State::Listening => self.listen(&g),
            State::FollowUp | State::Confirming => {
                if self.ep.is_some() {
                    self.listen(&g);
                } else if rms(&g) > SPEECH_RMS {
                    self.ep = Some(Endpointer::new()); // 窗口內開口：不用喚醒詞
                    self.listen(&g);
                } else {
                    self.window_left = self.window_left.saturating_sub(1);
                    if self.window_left == 0 {
                        self.to_idle();
                    }
                }
            }
            _ => {}
        }
    }

    fn verify_wake(&mut self) {
        for _ in 0..TAIL_BLOCKS {
            if self.read().is_none() {
                return;
            }
        }
        let t0 = Instant::now();
        let ok = self.p.verifier().transcribe(&self.ring.snapshot()).map(|t| verify_text(&t)).unwrap_or(false);
        self.log.log("wake", json!({"ok": ok, "latency_ms": t0.elapsed().as_millis() as u64}));
        if ok {
            self.ack();
        } else {
            self.p.spotter().reset();
        }
    }

    /// 「叮」＋「我在」→ 開始聽問題。
    fn ack(&mut self) {
        self.set_state(State::Acking);
        self.p.speaker().chime();
        let _ = self.p.speaker().say("我在");
        self.after_speaking();
        self.ep = Some(Endpointer::new());
        self.set_state(State::Listening);
    }

    /// 說完話：丟掉期間收到的聲音（包含自己的聲音），重設喚醒器。
    fn after_speaking(&mut self) {
        self.p.audio().flush();
        self.ring.clear();
        self.p.spotter().reset();
    }

    fn speak(&mut self, text: &str) {
        self.set_state(State::Speaking);
        let _ = self.p.speaker().say(text);
        self.after_speaking();
    }

    fn open_window(&mut self, s: State) {
        self.ep = None;
        self.window_left = self.timing.followup_blocks;
        self.set_state(s);
    }

    fn listen(&mut self, g: &[f32]) {
        let Some(ep) = self.ep.as_mut() else { return };
        let audio = match ep.push(g) {
            EndpointEvent::Continue => return,
            EndpointEvent::NoSpeech => {
                self.to_idle();
                return;
            }
            EndpointEvent::Done(a) | EndpointEvent::TooLong(a) => a,
        };
        self.ep = None;
        let confirming = self.state() == State::Confirming;
        self.set_state(State::Thinking);
        let text = self.p.transcriber().transcribe(&audio).unwrap_or_default();
        if text.is_empty() {
            self.to_idle();
        } else if confirming {
            self.confirm_turn(&text);
        } else {
            self.question(&text);
        }
    }

    // ---------- 問題處理 ----------

    fn question(&mut self, text: &str) {
        if let Some(cmd) = parse_local(text) {
            self.log.log("route", json!({"route": "local"}));
            self.local(cmd);
            return;
        }
        if let Some(at) = self.degraded_at {
            if at.elapsed() < self.timing.claude_retry {
                self.speak(DEGRADED_MSG);
                self.to_idle();
                return;
            }
        }
        self.quick_answer(text);
    }

    fn local(&mut self, cmd: LocalCmd) {
        let now = self.p.now();
        match cmd {
            LocalCmd::Time => self.speak(&say_time(now)),
            LocalCmd::Date => self.speak(&say_date(now)),
            LocalCmd::StartTimer { secs } => {
                self.timers.push(Timer { at: now + chrono::Duration::seconds(secs as i64) });
                self.speak(&format!("好，{}", say_duration(secs)));
            }
            LocalCmd::CancelTimer => {
                let had = !self.timers.is_empty();
                self.timers.clear();
                self.speak(if had { "好，計時取消了" } else { "現在沒有在計時" });
            }
            LocalCmd::Rest => {
                self.speak("好，我先休息，要叫我請按快捷鍵");
                self.shared.resting.store(true, Ordering::SeqCst);
                self.check_pause();
                return;
            }
        }
        self.open_window(State::FollowUp);
    }

    fn quick_answer(&mut self, text: &str) {
        let cfg = self.p.claude();
        let now = self.p.now();
        let mut sess = self.quick.take().unwrap_or_else(|| Session::new(cfg.clone(), "haiku", &quick_prompt(now)));
        let mut buf = String::new();
        let mut route = None;
        let mut split = SentenceSplitter::default();
        let mut spoke = false;
        let t0 = Instant::now();
        let r = {
            let speaker = self.p.speaker();
            let shared = &self.shared;
            let log = &self.log;
            let say = |s: &str| {
                set_state_raw(shared, log, State::Speaking);
                let _ = speaker.say(s);
            };
            self.rt.block_on(sess.ask(text, |d| {
                if route.is_none() {
                    buf.push_str(d);
                    route = route_prefix(&buf);
                    if route == Some(Route::Answer) {
                        for s in split.push(&buf) {
                            say(&s);
                            spoke = true;
                        }
                    }
                } else if route == Some(Route::Answer) {
                    for s in split.push(d) {
                        say(&s);
                        spoke = true;
                    }
                }
            }))
        };
        self.quick = Some(sess);
        let full = match r {
            Ok(full) => full,
            Err(e) => {
                if spoke {
                    self.after_speaking();
                }
                return self.claude_failed(e);
            }
        };
        self.crashes = 0;
        self.degraded_at = None;
        let streamed = route == Some(Route::Answer);
        let route = route.or_else(|| route_prefix(&full)).unwrap_or(Route::Answer);
        self.log.log("route", json!({"route": format!("{route:?}"), "latency_ms": t0.elapsed().as_millis() as u64}));
        match route {
            Route::Answer => {
                // 串流時沒判斷出路線（例如完全沒有增量），改用完整結果
                let mut rest = if streamed { vec![] } else { split.push(&full) };
                rest.extend(split.finish());
                for s in rest {
                    self.speak(&s);
                }
                if spoke {
                    self.after_speaking();
                }
                self.open_window(State::FollowUp);
            }
            Route::Lookup => {
                self.speak("我查一下");
                self.set_state(State::Thinking);
                match self.rt.block_on(lookup(&cfg, text, self.timing.lookup_timeout)) {
                    Ok(a) => {
                        self.speak(&a);
                        self.open_window(State::FollowUp);
                    }
                    Err(e) => self.claude_failed(e),
                }
            }
            Route::Action => {
                self.confirm = Some(Session::new(cfg, "haiku", &confirm_prompt(now)));
                self.confirm_turn(text);
            }
        }
    }

    fn confirm_turn(&mut self, text: &str) {
        let Some(mut sess) = self.confirm.take() else { return self.to_idle() };
        self.set_state(State::Thinking);
        let r = self.rt.block_on(sess.ask(text, |_| {}));
        let reply = match r {
            Ok(x) => x,
            Err(e) => return self.claude_failed(e),
        };
        let now = self.p.now();
        match parse_confirm_reply(&reply, now) {
            Ok(ConfirmOutcome::Say(t)) => {
                self.confirm = Some(sess);
                self.speak(&t);
                self.open_window(State::Confirming);
            }
            Ok(ConfirmOutcome::Cancel) => {
                self.speak("好，取消了");
                self.to_idle();
            }
            Ok(ConfirmOutcome::Do(a)) => {
                let msg = self.execute(a, now);
                self.log.log("action", json!({"ok": msg.is_ok()}));
                self.speak(&msg.unwrap_or_else(|e| e));
                self.to_idle();
            }
            Err(e) => {
                self.confirm = Some(sess);
                self.speak(&e.to_string());
                self.open_window(State::Confirming);
            }
        }
    }

    fn execute(&mut self, a: Action, now: DateTime<FixedOffset>) -> Result<String, String> {
        match a {
            Action::Reminder { at, text, .. } => {
                let r = Reminder { id: format!("{}", now.timestamp_nanos_opt().unwrap_or_default()), at, text };
                self.store.add(&r).map_err(|e| format!("提醒沒有存起來：{e}"))?;
                self.reminders.push(r);
                Ok("提醒設好了".into())
            }
            Action::Timer { secs, .. } => {
                self.timers.push(Timer { at: now + chrono::Duration::seconds(secs as i64) });
                Ok(format!("好，{}", say_duration(secs)))
            }
            note @ Action::Note { .. } => {
                append_note(&self.settings.notes_path, &note, now).map_err(|e| format!("筆記沒有寫進去：{e}"))?;
                Ok("記好了".into())
            }
        }
    }

    fn claude_failed(&mut self, e: ClaudeError) {
        self.log.log("claude", json!({"ok": false, "error": format!("{e:?}")}));
        self.confirm = None;
        match e {
            ClaudeError::NotLoggedIn | ClaudeError::NotFound => self.degraded_at = Some(Instant::now()),
            ClaudeError::Crashed => {
                self.crashes += 1;
                if self.crashes >= 2 {
                    self.degraded_at = Some(Instant::now());
                }
            }
            ClaudeError::Timeout | ClaudeError::Garbage => {}
        }
        self.speak(&e.to_string());
        self.to_idle();
    }

    // ---------- 計時與提醒 ----------

    fn startup_reminders(&mut self) {
        let now = self.p.now();
        let Ok((due, pending)) = self.store.reload(now) else { return };
        for r in due {
            self.p.notifier().notify("小小", &format!("你錯過了一個提醒：{}", r.text));
            let _ = self.store.remove(&r.id);
        }
        self.reminders = pending;
    }

    fn fire_due(&mut self) {
        let now = self.p.now();
        let mut msgs = vec![];
        let before = self.timers.len();
        self.timers.retain(|t| t.at > now);
        for _ in self.timers.len()..before {
            msgs.push("時間到了".to_string());
        }
        let (due, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.reminders).into_iter().partition(|r| r.at <= now);
        self.reminders = keep;
        for r in due {
            let _ = self.store.remove(&r.id);
            msgs.push(format!("提醒你：{}", r.text));
        }
        for m in msgs {
            self.p.notifier().notify("小小", &m);
            // 對話中或暫停（例如通話中）只發通知，不出聲
            if matches!(self.state(), State::Idle | State::Degraded) {
                self.p.speaker().chime();
                self.speak(&m);
                self.to_idle();
            }
        }
    }
}

fn set_state_raw(shared: &Shared, log: &EventLog, s: State) {
    {
        let mut cur = shared.state.lock().unwrap();
        if *cur == s {
            return;
        }
        *cur = s;
    }
    log.log("state", json!({"state": format!("{s:?}")}));
    if let Some(cb) = shared.on_state.lock().unwrap().as_ref() {
        cb(s);
    }
}

// ---------- 唸出來的中文 ----------

/// 0～99 轉中文數字；更大的用阿拉伯數字。
fn cn(n: u32) -> String {
    const D: [&str; 10] = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
    match n {
        0..=9 => D[n as usize].into(),
        10..=19 => format!("十{}", if n == 10 { "" } else { D[(n - 10) as usize] }),
        20..=99 => format!("{}十{}", D[(n / 10) as usize], if n % 10 == 0 { "" } else { D[(n % 10) as usize] }),
        _ => n.to_string(),
    }
}

/// 量詞前的 2 唸「兩」。
fn cn_count(n: u32) -> String {
    if n == 2 { "兩".into() } else { cn(n) }
}

pub fn say_duration(secs: u32) -> String {
    let (m, s) = (secs / 60, secs % 60);
    match (m, s) {
        (0, s) => format!("{}秒", cn_count(s)),
        (m, 0) if m >= 60 && m % 60 == 0 => format!("{}小時", cn_count(m / 60)),
        (m, 0) => format!("{}分鐘", cn_count(m)),
        (m, s) => format!("{}分{}秒", cn_count(m), cn(s)),
    }
}

pub fn say_time(now: DateTime<FixedOffset>) -> String {
    let (h, m) = (now.hour(), now.minute());
    let period = match h {
        0..=5 => "凌晨",
        6..=11 => "上午",
        12 => "中午",
        13..=17 => "下午",
        _ => "晚上",
    };
    let h12 = if h % 12 == 0 { 12 } else { h % 12 };
    let mm = match m {
        0 => "整".to_string(),
        1..=9 => format!("零{}分", cn(m)),
        _ => format!("{}分", cn(m)),
    };
    format!("現在是{period}{}點{mm}", cn_count(h12))
}

pub fn say_date(now: DateTime<FixedOffset>) -> String {
    let wd = ["一", "二", "三", "四", "五", "六", "日"][now.weekday().num_days_from_monday() as usize];
    format!("今天是{}月{}日，星期{wd}", cn(now.month()), cn(now.day()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spoken_chinese() {
        assert_eq!(say_duration(600), "十分鐘");
        assert_eq!(say_duration(120), "兩分鐘");
        assert_eq!(say_duration(30), "三十秒");
        assert_eq!(say_duration(90), "一分三十秒");
        assert_eq!(say_duration(7200), "兩小時");
        let t = DateTime::parse_from_rfc3339("2026-11-05T15:10:00+08:00").unwrap();
        assert_eq!(say_time(t), "現在是下午三點十分");
        let t = DateTime::parse_from_rfc3339("2026-11-05T14:05:00+08:00").unwrap();
        assert_eq!(say_time(t), "現在是下午兩點零五分");
        let t = DateTime::parse_from_rfc3339("2026-11-05T09:00:00+08:00").unwrap();
        assert_eq!(say_time(t), "現在是上午九點整");
        assert_eq!(say_date(t), "今天是十一月五日，星期四");
    }
}

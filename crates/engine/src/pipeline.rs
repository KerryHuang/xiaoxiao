//! 引擎主迴圈：喚醒 → 聽寫 → 本機指令或 Claude → 說出來 → 追問／確認窗口。
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Datelike, FixedOffset, Timelike};
use serde_json::json;

use crate::action::{Action, ConfirmOutcome, Reminder, ReminderStore, append_note, missed_message, parse_confirm_reply};
use crate::audio::{rms, FLOOR_WARMUP, Agc, EndpointEvent, Endpointer, RingBuffer, SpeechDetector};
use crate::claude::{ClaudeCfg, ClaudeError, Session, lookup};
use crate::config::Settings;
use crate::events::EventLog;
use crate::local_cmd::{LocalCmd, parse_local};
use crate::pause::{PauseReason, pause_reason};
use crate::platform::*;
use crate::prompts::{confirm_prompt, quick_message, quick_prompt};
use crate::router::{Route, route_prefix};
use crate::sentence::{SentenceSplitter, is_junk_transcript};
use crate::wake::{strip_call_name, verify_text};

pub const DEGRADED_MSG: &str = "現在連不上 Claude，我只能報時和計時";
/// 命中後再收幾塊（0.4 秒）才複核。
const TAIL_BLOCKS: usize = 4;
/// 複核用的錄音長度：命中前 3.5 秒＋命中後 0.4 秒。
const VERIFY_SECS: f32 = 3.9;
/// 追問窗口內要連續這麼多塊（0.3 秒）有人講話才算開口。
const ONSET_BLOCKS: usize = 3;
/// 不喊喚醒詞最多連開幾次追問窗口；之後要重新喊「嘿，小小」。
const MAX_FOLLOWUP_WINDOWS: u32 = 3;

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
    /// 快答：等第一段文字最多多久、整個回答最多多久。
    pub quick_first_token: Duration,
    pub quick_total: Duration,
    pub poll: Duration,
    /// 待命這麼久沒互動就自動休息（關麥克風）。
    pub auto_rest: Duration,
}

impl Default for Timing {
    fn default() -> Timing {
        Timing {
            pause_check: Duration::from_secs(2),
            followup_blocks: 80, // 8 秒
            claude_retry: Duration::from_secs(5 * 60),
            lookup_timeout: crate::claude::LOOKUP_TIMEOUT,
            quick_first_token: crate::claude::FIRST_TOKEN_TIMEOUT,
            quick_total: crate::claude::ANSWER_TIMEOUT,
            poll: Duration::from_millis(100),
            auto_rest: Duration::from_secs(120),
        }
    }
}

/// 把各平台元件綁在一起。
pub trait Platform: Send {
    fn audio(&mut self) -> &mut dyn AudioSource;
    fn spotter(&mut self) -> &mut dyn Spotter;
    /// 喚醒複核（whisper small）。
    fn verifier(&self) -> &dyn Transcriber;
    /// 問題聽寫（Mac 用 whisper turbo，Windows 用 small）。
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
type PauseCb = Box<dyn Fn(Option<PauseReason>) + Send>;

struct Shared {
    state: Mutex<State>,
    trigger: AtomicBool,
    resting: AtomicBool,
    /// 休息狀態剛改過，主迴圈不用等下一次定期檢查。
    recheck: AtomicBool,
    on_state: Mutex<Option<StateCb>>,
    on_pause: Mutex<Option<PauseCb>>,
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
    pub fn set_resting(&self, on: bool) {
        self.0.resting.store(on, Ordering::SeqCst);
        self.0.recheck.store(true, Ordering::SeqCst);
    }
    pub fn resting(&self) -> bool {
        self.0.resting.load(Ordering::SeqCst)
    }
    /// 暫停原因改變時通知外殼；角色靠這個知道要不要收起來。
    pub fn set_on_pause(&self, cb: PauseCb) {
        *self.0.on_pause.lock().unwrap() = Some(cb);
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
    vad: SpeechDetector,
    /// 最近讀到的那一塊有沒有人講話。
    speech: bool,
    onset: Vec<Vec<f32>>,
    followups: u32,
    ring: RingBuffer,
    ep: Option<Endpointer>,
    window_left: usize,
    window_peak: f32,
    window_speech: u32,
    quick: Option<Session>,
    confirm: Option<Session>,
    degraded_at: Option<Instant>,
    crashes: u32,
    timers: Vec<Timer>,
    reminders: Vec<Reminder>,
    last_pause: Option<PauseReason>,
    /// 通話或鎖定中被叫醒，要等麥克風打開才回應。
    awake_pending: bool,
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
                resting: AtomicBool::new(false),
                recheck: AtomicBool::new(false),
                on_state: Mutex::new(None),
                on_pause: Mutex::new(None),
            }),
            rt,
            log,
            store,
            agc: Agc::default(),
            vad: SpeechDetector::default(),
            speech: false,
            onset: vec![],
            followups: 0,
            ring: RingBuffer::new(VERIFY_SECS),
            ep: None,
            window_left: 0,
            window_peak: 0.0,
            window_speech: 0,
            quick: None,
            confirm: None,
            degraded_at: None,
            crashes: 0,
            timers: vec![],
            reminders: vec![],
            last_pause: None,
            awake_pending: false,
        }
    }

    pub fn handle(&self) -> EngineHandle {
        EngineHandle(self.shared.clone())
    }
    pub fn trigger(&self, t: Trigger) {
        self.handle().trigger(t)
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
        self.onset.clear();
        self.confirm = None;
        self.set_state(if self.degraded_at.is_some() { State::Degraded } else { State::Idle });
    }

    /// 目前的暫停原因；改變時通知外殼。
    fn pause_now(&mut self) -> Option<PauseReason> {
        let now = pause_reason(self.p.mic_usage().others_using_mic(), self.p.screen().locked(), self.resting());
        if now != self.last_pause {
            self.last_pause = now;
            if let Some(cb) = self.shared.on_pause.lock().unwrap().as_ref() {
                cb(now);
            }
        }
        now
    }

    // ---------- 主迴圈 ----------

    pub fn run(&mut self, stop: &AtomicBool) {
        self.startup_reminders();
        if self.pause_now().is_some() {
            self.set_state(State::Paused);
        } else {
            self.open_audio();
        }
        let mut last_check = Instant::now();
        let mut idle_since = Instant::now();
        while !stop.load(Ordering::SeqCst) {
            self.fire_due();
            if !matches!(self.state(), State::Idle | State::Degraded) {
                idle_since = Instant::now();
            } else if idle_since.elapsed() >= self.timing.auto_rest {
                // 不出聲：角色閉上眼睛就是提示，突然講話反而嚇人
                self.log.log("auto_rest", json!({}));
                self.shared.resting.store(true, Ordering::SeqCst);
                self.check_pause();
                continue;
            }
            if self.shared.recheck.swap(false, Ordering::SeqCst) || last_check.elapsed() >= self.timing.pause_check {
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
                self.vad = SpeechDetector::default();
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

    /// 讀一塊並做 AGC 與說話判斷；串流失效就關掉並立刻重開一次，
    /// 重開失敗才進 NoMic，由 check_pause 每 2 秒重試。
    fn read(&mut self) -> Option<Vec<f32>> {
        match self.p.audio().read_block() {
            Ok(b) => {
                let g = self.agc.process(&b);
                self.speech = self.vad.push(&b, self.agc.gain());
                self.ring.push(&g);
                Some(g)
            }
            Err(e) => {
                self.log.log("audio", json!({"ok": false, "error": format!("{e:?}")}));
                self.p.audio().close();
                self.ep = None;
                self.confirm = None;
                self.open_audio();
                None
            }
        }
    }

    fn check_pause(&mut self) {
        let want = self.pause_now().is_some();
        let s = self.state();
        if want && s != State::Paused {
            if s != State::NoMic {
                self.p.audio().close();
            }
            self.ep = None;
            self.confirm = None;
            self.quick = None; // 暫停期間不留著 claude 佔記憶體，下次叫醒會重開
            self.set_state(State::Paused);
        } else if !want && matches!(s, State::Paused | State::NoMic) {
            self.open_audio();
            if self.awake_pending {
                self.say_awake();
            }
        } else if matches!(s, State::Idle | State::Degraded)
            && self.settings.input_device.is_none()
            && self.p.audio().default_changed()
        {
            // 使用者改了系統預設輸入：待命時才換，不打斷對話
            self.p.audio().close();
            self.open_audio();
        }
    }

    /// 從休息叫醒：說一聲就回到待命，等使用者喊喚醒詞。
    /// 通話或鎖定時點過，暫停結束才補上起床音；這時使用者不一定在等，所以只回到待命、不聽問題。
    fn say_awake(&mut self) {
        self.awake_pending = false;
        if matches!(self.state(), State::Idle | State::Degraded) {
            self.play_wake_sound();
            self.to_idle();
        }
    }

    fn play_wake_sound(&mut self) {
        self.set_state(State::Acking);
        self.p.speaker().wake_sound();
        self.after_speaking();
    }

    /// 從休息被點醒：起床音 → 直接聽問題。沒講話就由 listen 回到待命。
    fn wake_and_listen(&mut self) {
        self.awake_pending = false;
        // 麥克風剛開要先估雜訊底。不先估的話，起床音一停就開口，聲音會被當成雜訊而聽不到
        for _ in 0..FLOOR_WARMUP {
            if self.read().is_none() {
                return;
            }
        }
        if matches!(self.state(), State::Idle | State::Degraded) {
            self.followups = 0;
            self.warm_quick();
            self.play_wake_sound();
            self.ep = Some(Endpointer::new());
            self.set_state(State::Listening);
        }
    }

    fn on_trigger(&mut self) {
        if self.resting() {
            self.shared.resting.store(false, Ordering::SeqCst);
            // 還有其他暫停理由（通話中、鎖定）就先不開麥克風，等理由結束才說
            if self.pause_now().is_some() {
                self.awake_pending = true;
                return;
            }
            if self.state() == State::Paused {
                self.open_audio();
            }
            self.wake_and_listen();
            return;
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
                } else if self.speech {
                    self.window_speech += 1;
                    // 窗口內開口：不用喚醒詞，但要連續講 0.3 秒才算，避免一聲雜音就觸發
                    self.onset.push(g);
                    if self.onset.len() >= ONSET_BLOCKS {
                        self.log.log("followup", json!({"gain": self.agc.gain()}));
                        self.ep = Some(Endpointer::new());
                        for b in std::mem::take(&mut self.onset) {
                            self.listen(&b);
                        }
                    }
                } else {
                    self.window_peak = self.window_peak.max(rms(&g) / self.agc.gain());
                    self.onset.clear();
                    self.window_left = self.window_left.saturating_sub(1);
                    if self.window_left == 0 {
                        // 只記音量，用來分辨「沒人說話」和「有說話但沒偵測到」
                        self.log.log(
                            "window_timeout",
                            json!({"peak": self.window_peak, "floor": self.vad.floor(), "speech_blocks": self.window_speech}),
                        );
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
        let text = self.p.verifier().transcribe(&self.ring.snapshot()).unwrap_or_default();
        let ok = verify_text(&text);
        self.log.log(
            "wake",
            json!({"ok": ok, "latency_ms": t0.elapsed().as_millis() as u64, "gain": self.agc.gain(), "text": text}),
        );
        if ok {
            self.ack();
        } else {
            self.p.spotter().reset();
        }
    }

    /// 「叮」＋「我在」→ 開始聽問題。
    fn ack(&mut self) {
        self.followups = 0;
        self.warm_quick();
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
        if s == State::FollowUp {
            if self.followups >= MAX_FOLLOWUP_WINDOWS {
                return self.to_idle();
            }
            self.followups += 1;
        }
        self.ep = None;
        self.onset.clear();
        self.window_left = self.timing.followup_blocks;
        self.window_peak = 0.0;
        self.window_speech = 0;
        self.set_state(s);
    }

    fn listen(&mut self, g: &[f32]) {
        let Some(ep) = self.ep.as_mut() else { return };
        let audio = match ep.push(g, self.speech) {
            EndpointEvent::Continue => return,
            EndpointEvent::NoSpeech => {
                self.to_idle();
                return;
            }
            EndpointEvent::Done(a) | EndpointEvent::TooLong(a) => a,
        };
        self.ep = None;
        let confirming = self.state() == State::Confirming;
        let followup = self.state() == State::FollowUp;
        self.set_state(State::Thinking);
        let t0 = Instant::now();
        let text = self.p.transcriber().transcribe(&audio).unwrap_or_default();
        self.log.log(
            "asr",
            json!({"latency_ms": t0.elapsed().as_millis() as u64, "gain": self.agc.gain(), "text": text}),
        );
        if text.is_empty() || is_junk_transcript(&text) {
            self.to_idle();
        } else if confirming {
            if parse_local(&text).is_some() {
                self.confirm = None;
                return self.question(&text);
            }
            self.confirm_turn(&text, false);
        } else if followup {
            self.followup_turn(&text);
        } else {
            self.question(&text);
        }
    }

    /// 追問要先叫名字，不然周圍的聲音都會被當成問題送給 Claude。沒叫名字就不回應，繼續等。
    fn followup_turn(&mut self, text: &str) {
        match strip_call_name(text) {
            None => {
                self.log.log("followup_ignored", json!({}));
                self.open_window(State::FollowUp);
            }
            Some("") => self.ack(),
            Some(q) => self.question(q),
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

    fn take_quick(&mut self) -> Session {
        self.quick.take().unwrap_or_else(|| {
            let (first, total) = (self.timing.quick_first_token, self.timing.quick_total);
            Session::new(self.p.claude(), "haiku", quick_prompt()).with_timeouts(first, total)
        })
    }

    /// 一被叫醒就先開好 claude，使用者問完不用再等它啟動。開不起來也不管，真的問時會回報。
    fn warm_quick(&mut self) {
        let mut sess = self.take_quick();
        {
            let _rt = self.rt.enter();
            let _ = sess.warm();
        }
        self.quick = Some(sess);
    }

    fn quick_answer(&mut self, text: &str) {
        let cfg = self.p.claude();
        let now = self.p.now();
        let (first, total) = (self.timing.quick_first_token, self.timing.quick_total);
        let mut sess = self.take_quick();
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
            self.rt.block_on(sess.ask(&quick_message(now, text), |d| {
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
                self.confirm = Some(Session::new(cfg, "haiku", &confirm_prompt(now)).with_timeouts(first, total));
                self.confirm_turn(text, true);
            }
        }
    }

    /// `first`：這句剛被快答判定成要做事。此時確認對話若說沒事可做，不能再送回快答，否則會來回打轉。
    fn confirm_turn(&mut self, text: &str, first: bool) {
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
            Ok(ConfirmOutcome::Other) => {
                self.log.log("confirm_other", json!({"first": first}));
                if first {
                    self.to_idle();
                } else {
                    self.question(text);
                }
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
            Action::Reminder { at, text, repeat } => {
                let r = Reminder { id: format!("{}", now.timestamp_nanos_opt().unwrap_or_default()), at, text };
                self.store.add(&r).map_err(|e| format!("提醒沒有存起來：{e}"))?;
                self.reminders.push(r);
                // 重複提醒還沒做：明說只設了一次，不靜默少做
                Ok(if repeat.is_some() { "提醒設好了。重複提醒還不支援，先幫你設這一次".into() } else { "提醒設好了".into() })
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
            self.p.notifier().notify("小小", &missed_message(&r));
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
            // 休息時照響；對話中、通話中、鎖定時只發通知，不出聲
            if matches!(self.state(), State::Idle | State::Degraded) {
                self.p.speaker().chime();
                self.speak(&m);
                self.to_idle();
            } else if self.state() == State::Paused && self.last_pause == Some(PauseReason::Rest) {
                self.p.speaker().chime();
                self.speak(&m);
                self.set_state(State::Paused);
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

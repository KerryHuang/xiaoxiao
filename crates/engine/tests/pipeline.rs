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
use engine::pause::PauseReason;
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
    /// 設了就用這個時鐘，每讀一次往前一小時。
    clock: Option<DateTime<FixedOffset>>,
    chimes: usize,
    wake_sounds: usize,
    notes: Vec<(String, String)>,
    mic_in_use: bool,
    default_changed: bool,
    seed: u32,
    start_resting: bool,
    http: VecDeque<Result<String, String>>,
    urls: Vec<String>,
    /// 喚醒複核（小模型聽寫）進行期間，script 最前面有幾塊已經進了系統緩衝。
    verify_backlog: usize,
    /// 目前躺在系統緩衝、還沒被讀走的塊數（script 最前面）。
    buffered: usize,
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
    fn take_buffered(&mut self) -> Vec<Vec<f32>> {
        let mut w = self.0.lock().unwrap();
        let k = std::mem::take(&mut w.buffered).min(w.script.len());
        let mut out = vec![];
        for _ in 0..k {
            if let Some(Item::Block(b)) = w.script.pop_front() {
                out.push(match b {
                    Kind::Silence => vec![0.0; BLOCK],
                    _ => vec![0.3; BLOCK],
                });
            }
        }
        out
    }
    fn flush(&mut self) {
        // 播放期間「收到」的回音要被丟掉；緩衝裡還沒讀走的也一併丟掉
        let mut w = self.0.lock().unwrap();
        for _ in 0..std::mem::take(&mut w.buffered).min(w.script.len()) {
            w.script.pop_front();
        }
        w.script.retain(|i| !matches!(i, Item::Block(Kind::Echo)));
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
        if self.1 {
            w.buffered = w.verify_backlog;
        }
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
    fn wake_sound(&self) {
        let mut w = self.0.lock().unwrap();
        w.wake_sounds += 1;
        // 喇叭模式：起床音被麥克風收到，不能當成問題
        for _ in 0..3 {
            w.script.push_front(Item::Block(Kind::Echo));
        }
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
    world: W,
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
    fn http_get(&self, url: &str) -> Result<String, String> {
        let mut w = self.world.lock().unwrap();
        w.urls.push(url.to_string());
        w.http.pop_front().unwrap_or_else(|| Err("沒網路".into()))
    }
    fn now(&self) -> DateTime<FixedOffset> {
        let mut w = self.world.lock().unwrap();
        match w.clock {
            Some(t) => {
                w.clock = Some(t + chrono::Duration::hours(1));
                t
            }
            None => chrono::Local::now().fixed_offset(),
        }
    }
}

fn fake_claude(dir: &Path, mode: &str) -> ClaudeCfg {
    let w = dir.join(format!("claude{}", std::env::consts::EXE_SUFFIX));
    // 硬連結：每次複製新檔案，macOS 可能卡在驗證新執行檔，程序停在 dyld 不動。跨磁碟才複製。
    fs::hard_link(env!("CARGO_BIN_EXE_fake-claude"), &w)
        .or_else(|_| fs::copy(env!("CARGO_BIN_EXE_fake-claude"), &w).map(|_| ()))
        .unwrap();
    fs::write(dir.join("fake-mode.txt"), format!("{mode}\n{}\n", dir.join("claude-args.txt").display())).unwrap();
    ClaudeCfg { bin: w, workdir: dir.to_path_buf(), read_dirs: vec![], location: String::new() }
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
        advise_blocks: 40,
        advise_first_token: Duration::from_secs(5),
        advise_total: Duration::from_secs(10),
        poll: Duration::from_millis(5),
        auto_rest: Duration::from_secs(3600),
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
        world: w.clone(),
    };
    let mut settings = Settings { notes_path: dir.path().join("notes.md"), ..Settings::default() };
    tweak(&mut settings);
    let mut e = Engine::with_timing(p, settings, timing);
    let h = e.handle();
    h.set_resting(w.lock().unwrap().start_resting);
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

/// fake 顧問回覆去掉 `**` 之後的樣子。
const REPLY: &str = "這個想法不錯，你覺得先做哪個？";

/// 顧問模式 1.5 秒才算講完：講 5 塊＋停 20 塊。
fn advising_utterance() -> Vec<Item> {
    let mut v = n(Kind::Speech, 5);
    v.extend(n(Kind::Silence, 20));
    v
}

/// 喚醒後依序講 texts：第一句走一般問答，之後每句一段顧問用的停頓。
fn discuss(mode: &str, t: Timing, texts: &[&str]) -> Rig {
    let texts: Vec<String> = texts.iter().map(|s| s.to_string()).collect();
    rig_full(tempfile::tempdir().unwrap(), mode, t, |_| {}, move |w| {
        w.script.extend(wake_and_ask());
        for _ in 1..texts.len() {
            w.script.extend(advising_utterance());
        }
        w.small.push_back("嘿小小".into());
        w.turbo.extend(texts);
    })
}

/// 顧問程序收到的每一行訊息。
fn advisor_msgs(r: &Rig) -> Vec<String> {
    fs::read_to_string(r.dir.path().join("claude-args.txt")).map(|s| s.lines().map(String::from).collect()).unwrap_or_default()
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

/// 實機：「嘿，小小，幾點了」連著講。複核聽寫期間進來的字，不能被「叮」之後的 flush 一起丟掉。
#[test]
fn speech_during_wake_verify_is_not_lost() {
    let r = rig("ok", |w| {
        w.script.extend(n(Kind::Silence, 3));
        w.script.extend(n(Kind::Wake, 1));
        w.script.extend(n(Kind::Silence, 4)); // 命中後的尾巴
        w.script.extend(utterance()); // 這 5 塊字＋後面的停頓在複核時已進緩衝
        w.verify_backlog = 15;
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("一公斤幾公克".into());
    });
    r.wait("回答完回待命", |w, s| w.said.len() == 2 && s == State::Idle);
    assert_eq!(r.said(), ["我在", "一公斤是1000 公克。"]);
}

#[test]
fn wake_starts_claude_before_the_question() {
    let r = rig("args", |w| {
        w.script.extend(n(Kind::Silence, 3));
        w.script.extend(n(Kind::Wake, 1));
        w.small.push_back("嘿小小".into());
    });
    let args = r.dir.path().join("claude-args.txt");
    // 先開 claude 再說「我在」，所以兩件事都發生了才檢查，不然慢的機器會在中間撞到空檔
    r.wait("喚醒後就先開好 claude", |w, _| args.exists() && !w.said.is_empty());
    assert_eq!(r.said(), ["我在"]);
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
fn claude_question_reaches_claude() {
    let r = rig("asked", |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("一公斤幾公克".into());
    });
    let asked = r.dir.path().join("claude-args.txt");
    r.wait("問題送到 claude", |_, _| asked.exists());
}

#[test]
fn lookup_answer_is_spoken_without_sources() {
    let r = rig("lookup-sources", |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("今天天氣如何".into());
    });
    r.wait("唸完答案", |w, _| w.said.iter().any(|s| s.contains("今天有雨")));
    let said = r.said().join("");
    assert!(!said.contains("http") && !said.contains("Sources"), "{said}");
}

#[test]
fn resting_closes_the_warm_claude() {
    let r = rig("spawns", |w| {
        w.script.extend(n(Kind::Silence, 3));
        w.script.extend(n(Kind::Wake, 1));
        w.small.push_back("嘿小小".into());
    });
    let spawns = r.dir.path().join("claude-args.txt");
    let count = || fs::read_to_string(&spawns).map(|s| s.lines().count()).unwrap_or(0);
    r.wait("第一次叫醒開好 claude", |_, _| count() == 1);
    let first_pid = fs::read_to_string(&spawns).unwrap().split_whitespace().nth(1).unwrap().to_string();
    r.h.set_resting(true);
    r.wait("進入休息", |_, s| s == State::Paused);
    // 不只要少開，舊程序一定要真的結束（記憶體才會還）。被殺但還沒被收屍的殭屍（Z）不佔記憶體，算結束
    #[cfg(unix)]
    {
        let alive = || {
            let o = std::process::Command::new("ps").args(["-o", "stat=", "-p", &first_pid]).output().unwrap();
            let st = String::from_utf8_lossy(&o.stdout).trim().to_string();
            !st.is_empty() && !st.starts_with('Z')
        };
        let t0 = Instant::now();
        while alive() && t0.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!alive(), "休息後常駐的 claude 程序還活著（pid {first_pid}）");
    }
    r.h.set_resting(false);
    r.wait("離開休息", |_, s| s != State::Paused);
    {
        let mut w = r.w.lock().unwrap();
        w.small.push_back("嘿小小".into());
    }
    let mut again = n(Kind::Silence, 3);
    again.extend(n(Kind::Wake, 1));
    r.push(again);
    r.wait("休息時舊程序已關，再叫醒要開新的", |_, _| count() == 2);
}

#[test]
fn every_question_carries_the_current_time() {
    let r = rig("record", |w| {
        w.clock = Some(DateTime::parse_from_rfc3339("2026-11-05T23:30:00+08:00").unwrap());
        w.script.extend(wake_and_ask());
        w.script.extend(utterance());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("下週一是幾號".into());
        w.turbo.push_back("小小，距離月底還有幾天".into());
    });
    let msgs = r.dir.path().join("claude-args.txt");
    r.wait("兩個問題都送到", |_, _| fs::read_to_string(&msgs).is_ok_and(|s| s.lines().count() == 2));
    let text = fs::read_to_string(&msgs).unwrap();
    let times: Vec<&str> = text
        .lines()
        .map(|l| l.split("現在時間 ").nth(1).expect("訊息要帶現在時間").split(']').next().unwrap())
        .collect();
    assert_ne!(times[0], times[1], "同一個 claude 程序的第二問也要是當下時間：{times:?}");
    assert!(text.contains("下週一是幾號") && text.contains("距離月底還有幾天"));
}

#[test]
fn local_timer_no_claude() {
    let r = rig("asked", |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("計時十分鐘".into());
    });
    r.wait("回覆計時", |w, _| w.said.len() == 2);
    assert_eq!(r.said()[1], "好，十分鐘");
    assert!(!r.dir.path().join("claude-args.txt").exists(), "不該把問題送給 claude");
}

#[test]
fn offtopic_while_confirming_is_answered_as_a_new_question() {
    let r = rig("action-other", |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("明天九點提醒我開會".into());
        w.turbo.push_back("一公斤幾公克".into());
    });
    r.wait("複述", |w, s| w.said.len() == 2 && s == State::Confirming);
    r.push(utterance());
    r.wait("偏題改當新問題回答", |w, _| w.said.len() == 3);
    assert_eq!(r.said()[2], "一公斤是1000 公克。");
    assert!(!r.dir.path().join("reminders.json").exists(), "偏題不能把提醒設下去");
}

const WEATHER: &str = r#"{"current":{"temperature_2m":22.4},"daily":{"weather_code":[3,51,3],
    "temperature_2m_max":[26.3,25.1,20.0],"temperature_2m_min":[21.9,22.0,18.0],"precipitation_probability_max":[4,29,10]}}"#;

fn ask_weather(mode: &str, location: &str, question: &str, http: Option<&str>) -> Rig {
    let (location, question, http) = (location.to_string(), question.to_string(), http.map(String::from));
    rig_full(tempfile::tempdir().unwrap(), mode, timing(), move |s| s.location = location, move |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back(question);
        if let Some(h) = http {
            w.http.push_back(Ok(h));
        }
    })
}

#[test]
fn weather_in_taiwan_never_reaches_claude() {
    let r = ask_weather("record", "台北", "明天天氣如何", Some(WEATHER));
    r.wait("唸出天氣", |w, _| w.said.len() == 2);
    assert_eq!(r.said()[1], "台北明天毛毛雨，最高 25、最低 22 度，降雨機率百分之二十九。");
    assert!(r.w.lock().unwrap().urls[0].contains("latitude=25."));
    assert!(!r.dir.path().join("claude-args.txt").exists(), "不該花 token");
}

#[test]
fn weather_place_in_question_beats_setting() {
    let r = ask_weather("record", "台北", "高雄明天天氣如何", Some(WEATHER));
    r.wait("唸出天氣", |w, _| w.said.len() == 2);
    assert!(r.said()[1].starts_with("高雄明天"), "{:?}", r.said());
    assert!(r.w.lock().unwrap().urls[0].contains("latitude=22."));
}

/// 這幾種都要交給 Claude，不能編答案，也不能不回應。
fn falls_back_to_claude(location: &str, question: &str, http: Option<&str>, calls_api: bool) {
    let r = ask_weather("lookup-sources", location, question, http);
    r.wait("Claude 查完回答", |w, _| w.said.len() >= 3);
    assert_eq!(r.said()[1], "我查一下", "{:?}", r.said());
    assert_eq!(!r.w.lock().unwrap().urls.is_empty(), calls_api);
}

#[test]
fn weather_without_location_goes_to_claude() {
    falls_back_to_claude("", "今天天氣如何", None, false);
}

#[test]
fn weather_abroad_goes_to_claude() {
    falls_back_to_claude("台北", "東京天氣如何", None, false);
}

#[test]
fn weather_api_down_goes_to_claude() {
    falls_back_to_claude("台北", "今天天氣如何", None, true);
}

#[test]
fn weather_api_garbage_goes_to_claude() {
    falls_back_to_claude("台北", "今天天氣如何", Some("{}"), true);
}

#[test]
fn followup_with_a_gap_between_syllables_still_counts() {
    let r = rig("ok", |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("一公斤幾公克".into());
        w.turbo.push_back("小小，那兩公斤呢".into());
    });
    r.wait("第一題答完", |w, s| w.said.len() == 2 && s == State::FollowUp);
    let mut v = n(Kind::Speech, 2);
    v.extend(n(Kind::Silence, 1));
    v.extend(n(Kind::Speech, 2));
    v.extend(n(Kind::Silence, 10));
    r.push(v);
    r.wait("第二題也答了", |w, _| w.said.len() >= 3);
}

#[test]
fn followup_without_name_gets_one_hint() {
    let r = rig("record", |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("現在幾點".into());
        w.turbo.push_back("一公斤幾公克".into());
        w.turbo.push_back("一公斤幾公克".into());
    });
    r.wait("第一題答完", |w, s| w.said.len() == 2 && s == State::FollowUp);
    r.push(utterance());
    r.wait("提醒要叫名字", |w, _| w.said.len() == 3);
    assert_eq!(r.said()[2], "要先叫我小小");
    r.wait("回到等追問", |_, s| s == State::FollowUp);
    r.push(utterance());
    r.wait("第二次不再提醒", |w, s| w.turbo.is_empty() && s == State::FollowUp);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(r.said().len(), 3, "{:?}", r.said());
    assert!(!r.dir.path().join("claude-args.txt").exists(), "不該送給 Claude");
}

#[test]
fn name_said_after_the_hotkey_is_not_a_question() {
    let r = rig("record", |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("嘿小小".into());
        w.turbo.push_back("現在幾點".into());
    });
    r.wait("再回一次我在", |w, s| w.said == ["我在", "我在"] && s == State::Listening);
    assert!(!r.dir.path().join("claude-args.txt").exists(), "只叫名字不該花 token");
    r.push(utterance());
    r.wait("接著回答", |w, _| w.said.len() == 3);
}

#[test]
fn name_before_a_question_is_removed() {
    let r = rig("record", |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("小小，現在幾點".into());
    });
    r.wait("當成本機指令回答", |w, _| w.said.len() == 2);
    assert!(!r.dir.path().join("claude-args.txt").exists(), "不該送給 Claude");
}

#[test]
fn window_timeout_logs_how_loud_it_was() {
    let r = rig("action", |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("明天九點提醒我開會".into());
    });
    r.wait("複述", |w, s| w.said.len() == 2 && s == State::Confirming);
    r.wait("窗口逾時", |_, s| s == State::Idle);
    let log = fs::read_to_string(r.dir.path().join("events.jsonl")).unwrap();
    let line = log.lines().find(|l| l.contains("window_timeout")).expect("要有逾時紀錄");
    assert!(line.contains("peak") && line.contains("floor") && line.contains("speech_blocks"), "{line}");
}

#[test]
fn local_command_while_confirming_really_runs() {
    let r = rig("action", |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("明天九點提醒我開會".into());
        w.turbo.push_back("先休息吧".into());
    });
    r.wait("複述", |w, s| w.said.len() == 2 && s == State::Confirming);
    r.push(utterance());
    r.wait("真的進入休息", |_, s| s == State::Paused);
    assert!(r.h.resting(), "嘴上答應不算，要真的休息");
}

#[test]
fn nothing_to_confirm_does_not_loop() {
    let r = rig("action-loop", |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("好".into());
    });
    let confirmed = r.dir.path().join("claude-args.txt");
    r.wait("進過確認", |_, _| confirmed.exists());
    r.wait("回到待命", |_, s| s == State::Idle);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(r.h.state(), State::Idle);
    assert_eq!(r.said(), ["我在"], "沒事可做就安靜回待命");
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

/// 實機：快答偶爾不回 <<ACTION>>、直接說「已經幫你設好」，但什麼都沒寫。
/// 要做事的句子不能交給快答決定，一律走確認對話。
#[test]
fn reminder_request_never_trusts_the_quick_answer() {
    let r = rig("action-lies", |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("明天早上九點提醒我開會".into());
    });
    r.wait("進入確認", |w, s| w.said.len() >= 2 && s == State::Confirming);
    let said = r.said();
    assert_eq!(said[0], "我在");
    assert!(said[1].contains("對嗎"), "{said:?}");
    assert!(!said.iter().any(|t| t.contains("設好")), "{said:?}");
}

#[test]
fn followup_with_name_is_answered_without_the_name() {
    let r = rig("ok", |w| {
        w.script.extend(wake_and_ask());
        w.script.extend(utterance()); // 回答完立刻追問，只叫名字
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("一公斤幾公克".into());
        w.turbo.push_back("小小，那兩公斤呢".into());
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
    r.wait("待命", |w, s| s == State::Idle && w.opens >= 1);
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
    r.wait("待命", |w, s| s == State::Idle && w.opens >= 1);
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
    r.w.lock().unwrap().script.clear();
    r.h.trigger(Trigger::Hotkey);
    r.wait("快捷鍵叫醒後沒講話就回待命", |w, s| s == State::Idle && w.wake_sounds == 1);
    assert_eq!(r.said().len(), 2, "起床只播音效，不說話");
    r.w.lock().unwrap().small.push_back("嘿小小".into());
    r.w.lock().unwrap().turbo.push_back("".into());
    r.push(wake_and_ask());
    r.wait("叫醒後喊喚醒詞有反應", |w, _| w.said.len() >= 3);
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
fn nomic_retries_log_only_first_failure() {
    let r = rig("ok", |w| {
        w.open_results.extend(std::iter::repeat_with(|| Err(AudioError::NoUsableInput)).take(5));
    });
    r.wait("重試幾次後接回麥克風", |w, s| s == State::Idle && w.opens >= 6);
    let log = fs::read_to_string(r.dir.path().join("events.jsonl")).unwrap();
    let failures = log.lines().filter(|l| l.contains(r#""ev":"audio""#) && l.contains(r#""ok":false"#)).count();
    assert_eq!(failures, 1, "{log}");
}

#[test]
fn rest_closes_mic_and_trigger_wakes() {
    let r = rig("ok", |_| {});
    r.wait("待命", |w, s| s == State::Idle && w.opens >= 1);
    r.h.set_resting(true);
    r.wait("休息", |w, s| s == State::Paused && w.closes >= 1);
    assert!(r.h.resting());
    let opens = r.w.lock().unwrap().opens;
    r.h.trigger(Trigger::Hotkey);
    r.wait("叫醒", |w, _| w.opens == opens + 1);
    assert!(!r.h.resting());
}

#[test]
fn wake_during_call_waits_for_call_end() {
    let r = rig("ok", |_| {});
    r.wait("待命", |w, s| s == State::Idle && w.opens >= 1);
    r.h.set_resting(true);
    r.wait("休息", |_, s| s == State::Paused);
    let opens = {
        let mut w = r.w.lock().unwrap();
        w.mic_in_use = true;
        w.opens
    };
    r.h.trigger(Trigger::Hotkey);
    let t0 = Instant::now();
    while r.h.resting() && t0.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!r.h.resting(), "叫醒要清掉休息");
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(r.h.state(), State::Paused, "通話中不能開麥克風");
    assert_eq!(r.w.lock().unwrap().opens, opens);
    r.w.lock().unwrap().mic_in_use = false;
    assert!(r.said().is_empty(), "通話中叫醒不能出聲");
    r.wait("通話結束才開", |w, _| w.opens == opens + 1);
    r.wait("開了才播起床音", |w, s| s == State::Idle && w.wake_sounds == 1);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(r.h.state(), State::Idle, "暫停結束不能自動聽問題，那時使用者可能不在等它");
    assert!(r.said().is_empty());
}

fn rig_auto_rest(ms: u64, setup: impl FnOnce(&mut World)) -> Rig {
    let t = Timing { auto_rest: Duration::from_millis(ms), ..timing() };
    rig_full(tempfile::tempdir().unwrap(), "ok", t, |_| {}, setup)
}

#[test]
fn idle_too_long_rests_silently_and_closes_mic() {
    let r = rig_auto_rest(150, |_| {});
    r.wait("開麥克風", |w, _| w.opens == 1);
    r.wait("自動休息", |w, s| s == State::Paused && w.closes >= 1);
    assert!(r.h.resting(), "要真的進入休息，不只是暫停");
    assert!(r.said().is_empty(), "自動休息不出聲");
    assert_eq!(r.w.lock().unwrap().chimes, 0);
}

#[test]
fn wake_after_auto_rest_counts_from_zero_again() {
    let r = rig_auto_rest(150, |_| {});
    r.wait("自動休息", |w, s| s == State::Paused && w.closes >= 1);
    r.h.trigger(Trigger::Hotkey);
    r.wait("叫醒", |w, _| w.opens == 2);
    assert!(!r.h.resting());
    r.wait("沒講話就回到待命", |w, s| s == State::Idle && w.wake_sounds == 1);
    assert!(r.said().is_empty(), "起床只播音效，不說話");
    r.wait("再次自動休息", |w, s| s == State::Paused && w.closes >= 2);
}

#[test]
fn call_time_does_not_count_as_idle() {
    let r = rig_auto_rest(300, |w| w.mic_in_use = true);
    r.wait("通話暫停", |_, s| s == State::Paused);
    std::thread::sleep(Duration::from_millis(450));
    assert!(!r.h.resting(), "通話中不算閒置");
    r.w.lock().unwrap().mic_in_use = false;
    r.wait("通話結束", |w, _| w.opens == 1);
    std::thread::sleep(Duration::from_millis(60));
    assert!(!r.h.resting(), "通話一結束不能馬上休息");
    assert_eq!(r.h.state(), State::Idle);
}

#[test]
fn click_from_rest_plays_wake_sound_then_listens_for_the_question() {
    let r = rig("ok", |w| {
        w.start_resting = true;
        w.turbo.push_back("一公斤幾公克".into());
    });
    r.wait("休息", |_, s| s == State::Paused);
    // 先留時間估雜訊底，起床音一停馬上開口也要聽得到
    let mut then_speak = n(Kind::Silence, engine::audio::FLOOR_WARMUP);
    then_speak.extend(utterance());
    r.push(then_speak);
    r.h.trigger(Trigger::Hotkey);
    r.wait("不用喚醒詞就回答", |w, _| w.said.iter().any(|s| s.contains("1000")));
    assert_eq!(r.w.lock().unwrap().wake_sounds, 1);
    assert_eq!(r.said()[0], "一公斤是1000 公克。", "起床音之後不再說「我在」或「嗯哼」");
}

#[test]
fn resting_before_run_opens_no_mic() {
    let r = rig("ok", |w| w.start_resting = true);
    r.wait("休息", |_, s| s == State::Paused);
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(r.w.lock().unwrap().opens, 0, "休息中重啟不能打開麥克風");
}

#[test]
fn timer_fires_while_resting() {
    let r = rig("ok", |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("計時一秒".into());
    });
    r.wait("回覆計時", |w, _| w.said.len() == 2);
    r.h.set_resting(true);
    r.wait("休息", |_, s| s == State::Paused);
    r.wait("時間到要出聲", |w, _| w.said.iter().any(|m| m == "時間到了") && w.chimes >= 1);
    r.wait("響完還在休息", |_, s| s == State::Paused);
    let closes = r.w.lock().unwrap().opens;
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(r.w.lock().unwrap().opens, closes, "響完不能打開麥克風");
    assert!(r.h.resting());
}

#[test]
fn timer_stays_silent_during_call() {
    let r = rig("ok", |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("計時一秒".into());
    });
    r.wait("回覆計時", |w, _| w.said.len() == 2);
    r.w.lock().unwrap().mic_in_use = true;
    r.wait("通話暫停", |_, s| s == State::Paused);
    r.wait("發通知", |w, _| !w.notes.is_empty());
    assert_eq!(r.said().len(), 2, "通話中不能出聲");
}

#[test]
fn pause_callback_reports_changes() {
    let r = rig("ok", |_| {});
    r.wait("待命", |w, s| s == State::Idle && w.opens >= 1);
    let got = Arc::new(Mutex::new(Vec::new()));
    let g2 = got.clone();
    r.h.set_on_pause(Box::new(move |p| g2.lock().unwrap().push(p)));
    r.h.set_resting(true);
    r.wait("休息", |_, s| s == State::Paused);
    r.h.trigger(Trigger::Hotkey);
    r.wait("叫醒", |_, s| s != State::Paused);
    r.w.lock().unwrap().mic_in_use = true;
    r.wait("通話", |_, s| s == State::Paused);
    r.w.lock().unwrap().mic_in_use = false;
    r.wait("通話結束", |_, s| s != State::Paused);
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(*got.lock().unwrap(), vec![Some(PauseReason::Rest), None, Some(PauseReason::Call), None]);
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
fn followup_without_name_never_reaches_claude() {
    let r = rig("record", |w| {
        w.script.extend(wake_and_ask());
        w.script.extend(utterance());
        w.script.extend(utterance());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("一公斤幾公克".into());
        w.turbo.push_back("determine这 volks2 won".into());
        w.turbo.push_back("小小，那兩公斤呢".into());
    });
    let msgs = r.dir.path().join("claude-args.txt");
    r.wait("雜音之後叫名字還能問", |_, _| fs::read_to_string(&msgs).is_ok_and(|s| s.lines().count() == 2));
    r.wait("唸完第二個回答", |w, _| w.said.len() >= 4);
    let text = fs::read_to_string(&msgs).unwrap();
    assert!(!text.contains("volks2"), "沒叫名字不能送 claude：{text}");
    assert!(text.contains("那兩公斤呢") && !text.contains("小小"), "送出前要去掉名字：{text}");
    assert_eq!(r.said()[2], "要先叫我小小", "沒叫名字只提醒：{:?}", r.said());
    assert_eq!(r.said().len(), 4, "{:?}", r.said());
}

#[test]
fn followup_with_only_the_name_asks_what_next() {
    let r = rig("ok", |w| {
        w.script.extend(wake_and_ask());
        w.script.extend(utterance());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("一公斤幾公克".into());
        w.turbo.push_back("小小。".into());
    });
    r.wait("只叫名字就回應", |w, s| w.said.len() == 3 && s != State::Speaking);
    assert_eq!(r.said()[2], "我在");
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
        for q in ["一", "小小二", "小小三", "小小四", "小小五"] {
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

// ---------- 顧問模式 ----------

#[test]
fn entering_phrase_starts_advising() {
    let r = discuss("advise", timing(), &["我們來討論"]);
    let spawns = r.dir.path().join("claude-args.spawns");
    // 程序啟動要幾十毫秒，fake 才寫得出紀錄；一起等，免得搶在它前面讀
    r.wait("進入顧問模式", |w, s| w.said == ["我在", "好，你說"] && s == State::Advising && spawns.exists());
    assert_eq!(fs::read_to_string(&spawns).unwrap(), "spawn sonnet\n", "進入時就開好 sonnet");
}

#[test]
fn entering_phrase_with_content_is_the_first_turn() {
    let r = discuss("advise", timing(), &["我們來討論一下新功能怎麼排"]);
    r.wait("直接回第一輪", |w, s| w.said.len() == 2 && s == State::Advising);
    assert_eq!(r.said(), ["我在", REPLY], "有後半句就不說「好，你說」，回覆不能有 **");
    let m = advisor_msgs(&r);
    assert_eq!(m.len(), 1);
    assert!(m[0].contains("新功能怎麼排") && m[0].contains("現在時間") && !m[0].contains("我們來討論"), "{m:?}");
}

#[test]
fn advising_needs_no_name_and_keeps_it() {
    let r = discuss("advise", timing(), &["我們來討論", "那匯出呢", "小小，登入呢"]);
    r.wait("兩輪都回答", |w, _| w.said.len() == 4);
    let m = advisor_msgs(&r);
    assert!(m[0].contains("那匯出呢") && m[1].contains("小小，登入呢"), "整句送出、不去掉名字：{m:?}");
    assert!(!r.said().iter().any(|s| s == "要先叫我小小"));
}

#[test]
fn advising_has_no_turn_limit() {
    let r = discuss("advise", timing(), &["我們來討論", "一", "二", "三", "四", "五"]);
    r.wait("五輪都回答", |w, _| w.said.len() == 7);
    assert_eq!(advisor_msgs(&r).len(), 5, "不受追問 3 輪限制");
}

#[test]
fn advising_window_uses_its_own_length() {
    let t = Timing { followup_blocks: 30, advise_blocks: 80, ..timing() };
    let r = rig_full(tempfile::tempdir().unwrap(), "advise", t, |_| {}, |w| {
        w.script.extend(wake_and_ask());
        w.script.extend(n(Kind::Silence, 50)); // 超過一般窗口、還在顧問窗口內
        w.script.extend(advising_utterance());
        w.small.push_back("嘿小小".into());
        w.turbo.extend(["我們來討論".to_string(), "那匯出呢".to_string()]);
    });
    r.wait("停 5 秒後開口還會回答", |w, _| w.said.len() == 3);
    assert_eq!(r.said()[2], REPLY);
}

#[test]
fn advising_waits_longer_before_cutting() {
    let r = rig("advise", |w| {
        w.script.extend(wake_and_ask());
        w.script.extend(n(Kind::Speech, 5));
        w.script.extend(n(Kind::Silence, 10)); // 停 1 秒：一般問答會切，顧問模式不切
        w.script.extend(n(Kind::Speech, 5));
        w.script.extend(n(Kind::Silence, 20));
        // 12 秒的一句話。固定音量連講超過約 4 秒會被 SpeechDetector 當成雜訊底，所以每 3 塊夾 1 塊靜音（像字間空隙）
        for _ in 0..30 {
            w.script.extend(n(Kind::Speech, 3));
            w.script.extend(n(Kind::Silence, 1));
        }
        w.script.extend(n(Kind::Silence, 20));
        w.small.push_back("嘿小小".into());
        w.turbo.extend(["我們來討論", "第一句", "第二句", "不該出現"].map(String::from));
    });
    r.wait("兩句都回答", |w, _| w.said.len() == 4);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(advisor_msgs(&r).len(), 2, "停頓 1 秒或講 12 秒都不能被切成兩句");
    assert_eq!(r.w.lock().unwrap().turbo.len(), 1);
}

#[test]
fn junk_in_advising_reopens_the_window() {
    let r = discuss("advise", timing(), &["我們來討論", "謝謝觀看", "那匯出呢"]);
    r.wait("幻覺字串後還能接著討論", |w, s| w.said.len() == 3 && s == State::Advising);
    let m = advisor_msgs(&r);
    assert_eq!(m.len(), 1, "{m:?}");
    assert!(m[0].contains("那匯出呢"));
}

#[test]
fn advising_sends_commands_and_reminders_to_sonnet() {
    let r = discuss("advise", timing(), &["我們來討論", "提醒我三點開會", "現在幾點"]);
    r.wait("兩句都交給 sonnet", |w, _| w.said.len() == 4);
    assert_eq!(r.said(), ["我在", "好，你說", REPLY, REPLY]);
    assert_eq!(advisor_msgs(&r).len(), 2);
    assert!(!r.dir.path().join("reminders.json").exists(), "顧問模式不做事");
}

#[test]
fn advisor_model_comes_from_settings() {
    let r = rig_full(tempfile::tempdir().unwrap(), "advise", Timing { advise_blocks: 400, ..timing() }, |s| s.advisor_model = "opus".into(), |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("我們來討論".into());
    });
    let spawns = r.dir.path().join("claude-args.spawns");
    r.wait("開了顧問程序", |_, _| spawns.exists());
    assert_eq!(fs::read_to_string(&spawns).unwrap(), "spawn opus\n");
}

#[test]
fn entering_phrase_beats_weather() {
    let r = rig_full(tempfile::tempdir().unwrap(), "advise", timing(), |s| s.location = "台北".into(), |w| {
        w.script.extend(wake_and_ask());
        w.small.push_back("嘿小小".into());
        w.turbo.push_back("我們來討論一下明天天氣".into());
        w.http.push_back(Ok(WEATHER.into()));
    });
    r.wait("進入顧問模式並回答", |w, s| w.said.len() == 2 && s == State::Advising);
    assert!(r.w.lock().unwrap().urls.is_empty(), "不該打天氣 API");
    assert_eq!(fs::read_to_string(r.dir.path().join("claude-args.spawns")).unwrap(), "spawn sonnet\n");
    assert!(advisor_msgs(&r)[0].contains("明天天氣"));
}

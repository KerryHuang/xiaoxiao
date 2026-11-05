# 小小 Mac App 實作計畫

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 做出 Mac 版小小 app：開機待命，用「嘿，小小」、快捷鍵或選單列圖示叫醒，在本機聽寫，經 Claude Code CLI 回答或確認後做事，再用美佳語音說出來。

**Architecture:** 一個 Cargo workspace。`crates/engine` 放跨平台的純邏輯與平台 trait，用假實作做 TDD；`crates/kws`、`crates/asr` 包 sherpa-onnx C API 與 whisper.cpp；`crates/platform-mac` 實作 Mac 的收音、麥克風使用偵測、說話、鎖定偵測；`src-tauri` 是選單列外殼，把引擎跑在背景執行緒。Windows 之後另寫計畫，只需要新增 `crates/platform-windows`，不動 `engine`。

**Tech Stack:** Rust（stable）、Tauri 2（tray、global-shortcut、autostart、notification plugin）、cpal、rubato、whisper-rs（metal feature）、sherpa-onnx C API（手寫 FFI）、tokio、serde_json、regex、chrono；UI 用 Vite＋原生 TypeScript；套件管理用 bun。

**Spec:** `docs/specs/2026-11-05-xiaoxiao-daily-assistant-app-design.md`

## Global Constraints

- 介面、語音、提示詞、錯誤訊息一律繁體中文；程式碼識別字用英文。
- 公開 repo：不得出現公司名稱、客戶名稱、內網位址、個人路徑（`/Users/<名字>`）；提交前 `git grep -nE '/Users/[A-Za-z0-9]|C:\\\\Users\\\\[A-Za-z0-9]' -- ':!docs/plans'` 必須是空的。
- 喚醒前的聲音只放在記憶體，最近 3.5 秒，不斷覆蓋；**任何音訊都不落檔、不外送**。送到雲端的只有問題的文字。
- 紀錄檔只記事件，不記轉錄內容；診斷模式要手動開。
- app 不得自建 Claude 登入，也不得讀取或保存 Claude token。
- 喚醒第一關：sherpa-onnx KWS 模型 `sherpa-onnx-kws-zipformer-zh-en-3M-2025-12-20`，檔案 `encoder-epoch-13-avg-2-chunk-16-left-64.int8.onnx`、`decoder-epoch-13-avg-2-chunk-16-left-64.onnx`、`joiner-epoch-13-avg-2-chunk-16-left-64.int8.onnx`、`tokens.txt`；關鍵詞行 `h ēi x iǎo x iǎo @嘿小小`；`keywords_threshold=0.25`、`keywords_score=2.0`、`num_threads=1`。
- 喚醒第二關：命中後再收 0.4 秒，取命中點往前 3.5 秒＋0.4 秒的錄音，交 whisper `ggml-small.bin` 複核；比對規則：`(?:嘿|黑|hey|嗨|欸|誒|可以)?[\s，,、。.!！?？]*小小`（不分大小寫），或文字含「小小」「曉曉」。
- 問題聽寫：whisper `ggml-large-v3-turbo-q5_0.bin`。whisper 的 initial prompt 只能放「以下是繁體中文的句子。」，**不得放「小小」**（會幻覺成一串「小小小」）。
- 錄問題：停頓 0.8 秒算講完，最長 10 秒，6 秒內沒開口就放棄；音量門檻用自動增益後的訊號判斷。
- 自動增益：`gain = clamp(0.3 / max(最近 5 秒每 100 ms 區塊峰值), 1.0, 30.0)`；有人講話的門檻是增益後 RMS > 0.04，**且**增益前 RMS 高於雜訊底（最近 5 秒區塊 RMS 第 20 百分位）的 3 倍（終審修正：只看增益後門檻，環境音會被拉過門檻）。
- 追問與確認窗口 8 秒；常駐對話閒置 5 分鐘開新對話；查資料逾時 30 秒；麥克風使用偵測每 2 秒一次；Claude 不可用時每 5 分鐘重試。
- Claude CLI 一律帶 `--setting-sources user --strict-mcp-config --permission-mode dontAsk`，工作目錄用 app 資料夾裡的空目錄 `workdir/`，不用家目錄。快答用 `haiku`，查資料用 `sonnet`。
- 查資料讀本機檔案時，一律加上排除：`.env*`、`.ssh/`、`*.pem`、`*.key`、`id_*`、`Library/Keychains/`、瀏覽器設定檔目錄。沒設定資料夾就只給 WebSearch。
- 不用藍牙裝置當收音麥克風。
- 預設快捷鍵 `Control+Option+Space`。
- 設定檔 `<app_config_dir>/settings.json`；提醒 `<app_data_dir>/reminders.json`；事件紀錄 `<app_log_dir>/events.jsonl`；模型 `<app_data_dir>/models/`；筆記預設 `~/Documents/小小筆記.md`。

## Review Focus

1. **收音裝置中途消失**（藍牙斷線、拔掉 USB 麥克風、改了系統預設輸入）：引擎要在 2 秒內改接新的預設輸入，或進入「沒有麥克風」狀態並在圖示顯示，不得卡死或靜默停止收音。→ Task 10 `recovers_when_audio_source_errors`。
2. **睡眠喚醒後音訊串流失效**：醒來後的第一聲「嘿，小小」要能被聽到。→ 和第 1 項同一套重開機制，Task 10 `reopens_source_after_stream_dead`。
3. **Claude CLI 中途死掉或吐出非 JSON 的行**：小小要說「回答中斷了」並回到待命，不得掛住。→ Task 7 `session_survives_garbage_line`、`session_reports_crash_mid_answer`。
4. **小小講話時聽到自己**（喇叭模式）：播放期間收到的聲音不得送進喚醒器或當成追問。→ Task 10 `drops_audio_while_speaking`。
5. **app 重開時有過期的提醒**：已經過時的提醒要在啟動時立刻通知「你錯過了一個提醒：…」，未到期的照常排程，不得遺失或重複。→ Task 3 `store_reload_splits_due_and_pending`。

---

## 檔案結構

```
Cargo.toml                      workspace
crates/engine/src/
  lib.rs                        匯出模組
  cn_num.rs                     中文數字 → u32
  local_cmd.rs                  本機固定指令解析
  sentence.rs                   串流文字切句
  router.rs                     快答輸出分流（Answer／Lookup／Action）
  action.rs                     <<DO>> JSON 解析與驗證、提醒儲存
  audio.rs                      RingBuffer、Agc、Endpointer
  wake.rs                       複核文字比對、WakeGate（兩關串接）
  claude.rs                     CLI 尋找、參數、常駐對話、單次查詢
  prompts.rs                    三份系統提示詞
  pause.rs                      暫停判斷
  platform.rs                   平台 trait
  pipeline.rs                   狀態機
  events.rs                     事件紀錄
  config.rs                     設定
crates/engine/tests/fixtures/   fake-claude.sh、合成語音 wav
crates/kws/                     sherpa-onnx FFI
crates/asr/                     whisper-rs 包裝
crates/platform-mac/            CoreAudio、say、鎖定偵測
src-tauri/                      Tauri 外殼
ui/                             設定與引導畫面（Vite＋TS）
scripts/fetch-models.sh         下載模型到 models/ 並驗 SHA256
scripts/make-fixtures.sh        用 say 產生測試語音
models/manifest.json            模型 URL 與 SHA256
```

---

### Task 0: 工具鏈與專案骨架

**Files:**
- Create: `Cargo.toml`、`crates/engine/Cargo.toml`、`crates/engine/src/lib.rs`、`rust-toolchain.toml`、`.gitignore`（新增 `target/`、`models/*` 但保留 `models/manifest.json`、`node_modules/`、`src-tauri/gen/`）
- Create: `src-tauri/`、`ui/`（由 `bun create tauri-app` 產生，模板 vanilla-ts）

**Interfaces:**
- Produces: `cargo test --workspace` 可執行；`bun tauri dev` 能開出空白視窗。

- [ ] **Step 1：安裝工具鏈**

Run: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y && brew install cmake`
Expected: `cargo --version`、`cmake --version` 都有輸出。

- [ ] **Step 2：產生 Tauri 骨架並改成 workspace**

用 `bun create tauri-app`（vanilla-ts、bun）產生到 repo 根目錄；app 的 Rust package 名稱設為 `xiaoxiao`、bundle identifier `io.github.kerryhuang.xiaoxiao`；根目錄 `Cargo.toml` 改成 workspace，members 為 `crates/*` 和 `src-tauri`。`rust-toolchain.toml` 釘 `stable`。

- [ ] **Step 3：驗證**

Run: `cargo test --workspace && bun install && bun tauri build --debug --no-bundle`
Expected: 測試 0 個且通過；build 成功。

- [ ] **Step 4：Commit**

`git add` 逐檔加入產生的檔案，commit：`chore: Tauri 2 workspace 骨架`

---

### Task 1: 中文數字與本機固定指令

**Files:**
- Create: `crates/engine/src/cn_num.rs`、`crates/engine/src/local_cmd.rs`
- Test: 同檔 `#[cfg(test)]`

**Interfaces:**
- Produces:
  - `pub fn parse_cn_number(s: &str) -> Option<u32>`
  - `pub enum LocalCmd { Time, Date, StartTimer { secs: u32 }, CancelTimer, Rest }`
  - `pub fn parse_local(text: &str) -> Option<LocalCmd>`：簡體、繁體都要接受（Whisper 可能吐出簡體）。

- [ ] **Step 1：寫失敗的測試**

```rust
#[test] fn cn_numbers() {
    for (s, n) in [("十",10),("十五",15),("二十",20),("二十三",23),("三",3),("兩",2),("九十九",99),("12",12)] {
        assert_eq!(parse_cn_number(s), Some(n), "{s}");
    }
    assert_eq!(parse_cn_number("一百零一"), None);
}
#[test] fn local_commands() {
    use LocalCmd::*;
    assert_eq!(parse_local("現在幾點"), Some(Time));
    assert_eq!(parse_local("几点了"), Some(Time));
    assert_eq!(parse_local("今天幾號"), Some(Date));
    assert_eq!(parse_local("今天星期幾"), Some(Date));
    assert_eq!(parse_local("計時十分鐘"), Some(StartTimer { secs: 600 }));
    assert_eq!(parse_local("设一个5分钟计时"), Some(StartTimer { secs: 300 }));
    assert_eq!(parse_local("計時半小時"), Some(StartTimer { secs: 1800 }));
    assert_eq!(parse_local("計時三十秒"), Some(StartTimer { secs: 30 }));
    assert_eq!(parse_local("取消計時"), Some(CancelTimer));
    assert_eq!(parse_local("小小先休息"), Some(Rest));
    assert_eq!(parse_local("提醒我三點開會"), None); // 寫入走確認流程
    assert_eq!(parse_local("台北現在幾點的天氣怎樣"), None); // 句子裡有別的需求就不攔
}
```

- [ ] **Step 2：Run** `cargo test -p engine cn_num local_` → Expected: FAIL（函式不存在）

- [ ] **Step 3：實作**

`parse_cn_number` 支援 0～99 的阿拉伯數字與中文（零一二兩三…九十）。`parse_local` 先去掉頭尾標點，以整句比對（`^…$` 的 regex），不在句中搜尋，所以「台北現在幾點的天氣」不會被攔下。時長單位支援秒／分鐘／小時，「半小時」＝1800。

- [ ] **Step 4：Run** 同上 → PASS

- [ ] **Step 5：Commit** `feat(engine): 中文數字與本機固定指令`

---

### Task 2: 切句、分流與提示詞

**Files:**
- Create: `crates/engine/src/sentence.rs`、`crates/engine/src/router.rs`、`crates/engine/src/prompts.rs`

**Interfaces:**
- Produces:
  - `pub struct SentenceSplitter`；`fn push(&mut self, delta: &str) -> Vec<String>`、`fn finish(&mut self) -> Option<String>`；以 `。！？!?\n` 斷句，丟掉空句。
  - `pub enum Route { Answer, Lookup, Action }`；`pub fn route_prefix(buf: &str) -> Option<Route>`：前 12 個字元還不能判斷就回 `None`；以 `<<LOOKUP>>` 開頭回 `Lookup`，`<<ACTION>>` 回 `Action`，以其他字元開頭回 `Answer`。
  - `pub fn quick_prompt(now: DateTime<Local>) -> String`、`pub fn lookup_prompt() -> &'static str`、`pub fn confirm_prompt(now: DateTime<Local>) -> String`

- [ ] **Step 1：寫失敗的測試**

```rust
#[test] fn splits_streaming_deltas() {
    let mut s = SentenceSplitter::default();
    assert!(s.push("1 公斤等於").is_empty());
    assert_eq!(s.push("1000 公克。要不要"), vec!["1 公斤等於1000 公克。"]);
    assert_eq!(s.finish(), Some("要不要".into()));
}
#[test] fn routes_by_prefix() {
    assert_eq!(route_prefix("<<LOO"), None);
    assert_eq!(route_prefix("<<LOOKUP>>"), Some(Route::Lookup));
    assert_eq!(route_prefix(" <<ACTION>>"), Some(Route::Action));
    assert_eq!(route_prefix("一公斤"), Some(Route::Answer));
}
#[test] fn prompts_carry_time_and_rules() {
    let now = Local.with_ymd_and_hms(2026, 11, 5, 9, 30, 0).unwrap();
    let q = quick_prompt(now);
    assert!(q.contains("<<LOOKUP>>") && q.contains("<<ACTION>>") && q.contains("2026-11-05"));
    let c = confirm_prompt(now);
    assert!(c.contains("<<DO>>") && c.contains("<<CANCEL>>") && c.contains("07:00") && c.contains("+08:00"));
}
```

- [ ] **Step 2：Run** `cargo test -p engine sentence router prompts` → FAIL

- [ ] **Step 3：實作，提示詞文字固定如下**（`{now}` 帶 `%Y-%m-%d %H:%M %:z 星期X`）

quick：
```
你是語音助理「小小」，在使用者的電腦上待命。現在是 {now}。
你的回答會被唸出來：只用一兩句口語繁體中文，不用條列、符號、markdown，也不要說出你的判斷過程或分類。
需要查網路、即時資訊，或使用者自己的檔案資料時，整個回覆只輸出 <<LOOKUP>>。
使用者要你建立、修改、提醒、記錄、計時等改變狀態的事時，整個回覆只輸出 <<ACTION>>。
其他情況直接回答。
```
lookup：
```
你是語音助理「小小」。用工具查到答案後，只用一兩句口語繁體中文講結論，不用條列或符號；查不到就直說查不到。
```
confirm：
```
你是語音助理「小小」，正在用語音幫使用者做事。現在是 {now}。
先不要做：用一句話複述你理解的內容，時間一律講完整（例如「今天下午三點」）。
動作只有三種：reminder（必填 at、text；選填 repeat）、timer（必填 secs；選填 name）、note（必填 text；選填 title）。
缺必填才問，一次只問一件事；選填沒給就不要問。
沒講上下午的時間，取接下來最近、落在 07:00～22:00 之間的那個；兩個都不在範圍內才問。
使用者說對、好、可以，整個回覆只輸出一行：<<DO>> 加上 JSON，例如 <<DO>>{"type":"reminder","at":"2026-11-05T15:00:00+08:00","text":"開會"}。at 用 RFC 3339 並帶時區。
使用者說不對，就依修正重新複述。使用者說取消、算了，整個回覆只輸出 <<CANCEL>>。
```

- [ ] **Step 4：Run** → PASS

- [ ] **Step 5：Commit** `feat(engine): 切句、分流、提示詞`

---

### Task 3: 做事動作的解析、驗證與提醒儲存

**Files:**
- Create: `crates/engine/src/action.rs`

**Interfaces:**
- Produces:
  - `pub enum Action { Reminder { at: DateTime<FixedOffset>, text: String, repeat: Option<String> }, Timer { secs: u32, name: Option<String> }, Note { text: String, title: Option<String> } }`
  - `pub enum ConfirmOutcome { Do(Action), Cancel, Say(String) }`；`pub fn parse_confirm_reply(reply: &str, now: DateTime<FixedOffset>) -> Result<ConfirmOutcome, ActionError>`
  - `pub enum ActionError { BadJson, MissingField(&'static str), InPast }`，`Display` 為中文，會被唸出來。
  - `pub struct ReminderStore { path: PathBuf }`；`fn add(&self, r: &Reminder) -> io::Result<()>`、`fn remove(&self, id: &str) -> io::Result<()>`、`fn reload(&self, now) -> io::Result<(Vec<Reminder>, Vec<Reminder>)>` 回傳（已過期, 未到期）；`Reminder { id: String, at, text }`；寫檔用暫存檔加 rename，UTF-8。
  - `pub fn append_note(path: &Path, note: &Action, now) -> io::Result<()>`：附加 `## {title 或 時間}\n\n{text}\n\n`。

- [ ] **Step 1：寫失敗的測試**

```rust
fn now() -> DateTime<FixedOffset> { DateTime::parse_from_rfc3339("2026-11-05T09:30:00+08:00").unwrap() }
#[test] fn parses_do_reminder() {
    let r = parse_confirm_reply(r#"<<DO>>{"type":"reminder","at":"2026-11-05T15:00:00+08:00","text":"開會"}"#, now()).unwrap();
    assert!(matches!(r, ConfirmOutcome::Do(Action::Reminder { ref text, .. }) if text == "開會"));
}
#[test] fn rejects_past_and_missing() {
    assert!(matches!(parse_confirm_reply(r#"<<DO>>{"type":"reminder","at":"2026-11-05T08:00:00+08:00","text":"x"}"#, now()), Err(ActionError::InPast)));
    assert!(matches!(parse_confirm_reply(r#"<<DO>>{"type":"note"}"#, now()), Err(ActionError::MissingField("text"))));
    assert!(matches!(parse_confirm_reply("<<DO>>{oops", now()), Err(ActionError::BadJson)));
}
#[test] fn cancel_and_say() {
    assert!(matches!(parse_confirm_reply("<<CANCEL>>", now()).unwrap(), ConfirmOutcome::Cancel));
    assert!(matches!(parse_confirm_reply("今天下午三點提醒你開會，對嗎？", now()).unwrap(), ConfirmOutcome::Say(_)));
}
#[test] fn store_reload_splits_due_and_pending() {
    // 寫入一筆 08:00、一筆 15:00，以 09:30 reload：過期 1 筆、未到期 1 筆；再 reload 一次結果相同（不重複、不遺失）
}
#[test] fn note_appends_utf8() { /* 兩次 append 後檔案含兩段標題 */ }
```

- [ ] **Step 2：Run** `cargo test -p engine action` → FAIL

- [ ] **Step 3：實作**（`<<DO>>` 前後允許空白；JSON 用 serde 的 tagged enum `#[serde(tag="type", rename_all="lowercase")]`）

- [ ] **Step 4：Run** → PASS

- [ ] **Step 5：Commit** `feat(engine): 做事動作驗證與提醒儲存`

---

### Task 4: 音訊核心：環形緩衝、自動增益、斷句

**Files:**
- Create: `crates/engine/src/audio.rs`

**Interfaces:**
- Produces（取樣率一律 16 kHz 單聲道 `f32`，區塊 100 ms＝1600 樣本）：
  - `pub const SR: u32 = 16_000; pub const BLOCK: usize = 1600;`
  - `pub struct RingBuffer`；`fn new(secs: f32)`、`fn push(&mut self, s: &[f32])`、`fn snapshot(&self) -> Vec<f32>`、`fn clear(&mut self)`
  - `pub struct Agc`；`fn process(&mut self, block: &[f32]) -> Vec<f32>`、`fn gain(&self) -> f32`（公式見 Global Constraints，窗口 50 個區塊）
  - `pub enum EndpointEvent { Continue, Done(Vec<f32>), NoSpeech, TooLong(Vec<f32>) }`；`pub struct Endpointer`；`fn new()`、`fn push(&mut self, gained: &[f32]) -> EndpointEvent`（0.8 秒停頓＝8 區塊、10 秒上限＝100 區塊、6 秒沒開口＝60 區塊）
  - `pub fn resample_to_16k(input: &[f32], from_hz: u32) -> Vec<f32>`（rubato）

- [ ] **Step 1：寫失敗的測試**（訊號用正弦波與零值合成）

```rust
#[test] fn ring_keeps_last_secs() { let mut r = RingBuffer::new(3.5); r.push(&vec![1.0; 16000*5]); assert_eq!(r.snapshot().len(), 56000); }
#[test] fn agc_boosts_quiet_and_caps() {
    let mut a = Agc::default();
    a.process(&sine(0.01, BLOCK)); assert!((a.gain() - 30.0).abs() < 1e-3);
    for _ in 0..50 { a.process(&sine(0.6, BLOCK)); } assert!((a.gain() - 1.0).abs() < 1e-3);
}
#[test] fn endpointer_done_after_pause() { /* 10 個講話區塊 + 8 個靜音區塊 → 第 18 個回 Done，長度 18*BLOCK */ }
#[test] fn endpointer_gives_up_without_speech() { /* 60 個靜音區塊 → NoSpeech */ }
#[test] fn endpointer_caps_at_10s() { /* 100 個講話區塊 → TooLong */ }
#[test] fn resample_48k_len() { assert_eq!(resample_to_16k(&vec![0.0; 48000], 48000).len(), 16000); }
```

- [ ] **Step 2：Run** `cargo test -p engine audio` → FAIL
- [ ] **Step 3：實作**
- [ ] **Step 4：Run** → PASS
- [ ] **Step 5：Commit** `feat(engine): 環形緩衝、自動增益、斷句`

---

### Task 5: 喚醒：sherpa-onnx FFI、複核比對、模型與測試語音

**Files:**
- Create: `crates/kws/{Cargo.toml,build.rs,src/lib.rs}`、`crates/engine/src/wake.rs`、`crates/engine/src/platform.rs`（先只放 `Spotter`）、`scripts/fetch-models.sh`、`scripts/make-fixtures.sh`、`models/manifest.json`、`crates/engine/tests/fixtures/*.wav`、`THIRD_PARTY_LICENSES.md`

**Interfaces:**
- Produces:
  - `kws::Spotter::new(model_dir: &Path) -> Result<Spotter>`；`fn accept(&mut self, block: &[f32]) -> bool`（命中回 true，並自動 reset stream）；`fn reset(&mut self)`。參數照 Global Constraints。
  - `engine::wake::verify_text(text: &str) -> bool`
  - `engine::platform::Spotter` trait：`fn accept(&mut self, block: &[f32]) -> bool; fn reset(&mut self);`，由 `kws::Spotter` 實作。
- Consumes: `audio::BLOCK`、`audio::SR`

- [ ] **Step 1：模型與測試語音腳本**

`scripts/fetch-models.sh` 從官方來源下載到 `models/`，並對照 `models/manifest.json` 的 SHA256：KWS 模型 tar（`https://github.com/k2-fsa/sherpa-onnx/releases/download/kws-models/sherpa-onnx-kws-zipformer-zh-en-3M-2025-12-20.tar.bz2`）、`ggml-small.bin`、`ggml-large-v3-turbo-q5_0.bin`（`https://huggingface.co/ggerganov/whisper.cpp/resolve/main/`）。第一次執行時算出 SHA256 寫進 manifest 並 commit。另外下載 sherpa-onnx 預編的 macOS shared library（取執行當下最新的 release，版本號寫進 manifest），放在 `models/sherpa-onnx-lib/`。
`scripts/make-fixtures.sh` 用 `say -v Meijia` 產生 16 kHz 單聲道 wav：`wake.wav`「我們先討論這個。嘿，小小，今天天氣怎麼樣？」、`no-wake-1.wav`「這個小小的問題先放著。」、`no-wake-2.wav`「可以再思考一下。」、`question.wav`「一公尺等於幾公分？」。這些是合成語音，可以提交。

- [ ] **Step 2：寫失敗的測試**

```rust
// crates/engine/src/wake.rs
#[test] fn verify_text_tolerant() {
    for t in ["嘿,小小", "黑小小", "我測一下嘿小小", "可以小小", "Hey 小小"] { assert!(verify_text(t), "{t}"); }
    for t in ["那我們可以想可以", "這是小事", "嘿你好"] { assert!(!verify_text(t), "{t}"); }
}
// crates/kws/tests/fixtures.rs（需要模型；#[ignore]，用 `cargo test -p kws -- --ignored` 執行）
#[test] #[ignore] fn spots_wake_not_others() {
    // 以 100 ms 區塊餵入：wake.wav 至少命中 1 次；no-wake-1、no-wake-2 都是 0 次
}
```

- [ ] **Step 3：Run** `cargo test -p engine wake && cargo test -p kws -- --ignored` → FAIL

- [ ] **Step 4：實作**：手寫 `extern "C"` 綁定 sherpa-onnx `c-api.h` 的 `SherpaOnnxCreateKeywordSpotter`、`SherpaOnnxCreateKeywordStream`、`SherpaOnnxOnlineStreamAcceptWaveform`、`SherpaOnnxIsKeywordStreamReady`、`SherpaOnnxDecodeKeywordStream`、`SherpaOnnxGetKeywordResult`、`SherpaOnnxResetKeywordStream` 與對應的 Destroy；`build.rs` 從 `models/sherpa-onnx-lib/` 連結，並設 rpath 給 app bundle。關鍵詞檔在 `new()` 時寫到暫存位置。

- [ ] **Step 5：Run** 同 Step 3 → PASS
- [ ] **Step 6：Commit** `feat: sherpa-onnx 喚醒與複核比對`（`THIRD_PARTY_LICENSES.md` 列 sherpa-onnx／模型 Apache-2.0、whisper.cpp 與模型 MIT）

---

### Task 6: 聽寫：whisper-rs 包裝

**Files:**
- Create: `crates/asr/{Cargo.toml,src/lib.rs}`
- Modify: `crates/engine/src/platform.rs`（加 `Transcriber`）

**Interfaces:**
- Produces: `asr::Whisper::new(model: &Path) -> Result<Whisper>`；`fn transcribe(&self, audio_16k: &[f32]) -> Result<String>`：語言 `zh`、initial prompt 照 Global Constraints、不沿用上一段上下文、單一 segment 串接後 trim。`engine::platform::Transcriber` trait 同簽名，由它實作。
- Consumes: Task 5 的模型與 fixtures

- [ ] **Step 1：寫失敗的測試**（`#[ignore]`，需要模型）

```rust
#[test] #[ignore] fn small_hears_wake() { let w = Whisper::new(&models().join("ggml-small.bin")).unwrap(); assert!(engine::wake::verify_text(&w.transcribe(&load("wake.wav")).unwrap())); }
#[test] #[ignore] fn turbo_hears_question() { let t = w_turbo().transcribe(&load("question.wav")).unwrap(); assert!(t.contains("公尺") || t.contains("公分") || t.contains("厘米")); }
#[test] #[ignore] fn silence_is_short() { assert!(w_small().transcribe(&vec![0.0; 16000*3]).unwrap().chars().count() <= 4); }
```

- [ ] **Step 2：Run** `cargo test -p asr -- --ignored` → FAIL
- [ ] **Step 3：實作**（`whisper-rs` 開 `metal` feature）
- [ ] **Step 4：Run** → PASS，並記下 small 與 turbo 處理 `wake.wav` 的時間（寫進 commit message）
- [ ] **Step 5：Commit** `feat(asr): whisper.cpp 聽寫`

---

### Task 7: Claude CLI：尋找、常駐快答、單次查詢、確認對話

**Files:**
- Create: `crates/engine/src/claude.rs`、`crates/engine/tests/fixtures/fake-claude.sh`、`crates/engine/tests/claude.rs`

**Interfaces:**
- Produces:
  - `pub fn find_claude(path_env: &str, desktop_root: &Path) -> Option<PathBuf>`：先找 PATH 上的 `claude`；找不到再找 `desktop_root/claude-code/<版本>/<hash>/claude.app/Contents/MacOS/claude`，取版本號最大的那份（版本號依 semver 比較，不比字串）。正式呼叫時 `desktop_root`＝`~/Library/Application Support/Claude`。
  - `pub struct ClaudeCfg { bin: PathBuf, workdir: PathBuf, read_dirs: Vec<PathBuf> }`
  - `pub fn session_args(model: &str, system_prompt: &str) -> Vec<String>`：`-p --model <m> --input-format stream-json --output-format stream-json --verbose --include-partial-messages --tools "" --setting-sources user --permission-mode dontAsk --strict-mcp-config --system-prompt <p>`
  - `pub fn lookup_args(cfg: &ClaudeCfg, question: &str) -> Vec<String>`：`-p <q> --model sonnet --output-format json --append-system-prompt <lookup_prompt>` 加上鎖權限三旗標；`read_dirs` 空的話 `--tools WebSearch --allowedTools WebSearch`；不空的話工具為 `Read,Grep,Glob,WebSearch`，每個資料夾帶 `--add-dir`，並加 `--disallowedTools`，對每個排除樣式產生 `Read(**/<樣式>)`、`Grep(**/<樣式>)`、`Glob(**/<樣式>)`。
  - `pub struct Session`（tokio 子程序）；`async fn ask(&mut self, text: &str, on_delta: impl FnMut(&str)) -> Result<String, ClaudeError>`；`fn idle_for(&self) -> Duration`；閒置超過 5 分鐘時，下一次 `ask` 前自動重開。
  - `pub async fn lookup(cfg: &ClaudeCfg, q: &str) -> Result<String, ClaudeError>`（30 秒逾時）
  - `pub enum ClaudeError { NotFound, NotLoggedIn, Crashed, Timeout, Garbage }`，中文 `Display` 會被唸出來。判斷 `NotLoggedIn`：result 事件 `is_error=true` 且 result 文字含 `login` 或 `/login`。
  - 串流事件：`{"type":"stream_event","event":{"delta":{"type":"text_delta","text":…}}}`、`{"type":"result","result":…,"is_error":bool}`；輸入一行 `{"type":"user","message":{"role":"user","content":…}}`。

- [ ] **Step 1：假的 claude**

`fake-claude.sh` 依環境變數 `FAKE_CLAUDE` 決定行為：`ok`（每讀一行輸入，就送兩個 text_delta「一公斤是」「1000 公克。」再送 result）、`garbage`（先送一行 `not json` 再正常）、`crash`（送一個 delta 後 `exit 1`）、`nologin`（直接送 `{"type":"result","is_error":true,"result":"Please run /login"}`）、`slow`（sleep 40）、`args`（把收到的參數逐行寫到 `$FAKE_ARGS_OUT`）。

- [ ] **Step 2：寫失敗的測試**

```rust
#[test] fn finds_newest_desktop_cli() { /* temp 目錄建 2.1.309/a 與 2.1.317/b，PATH 空 → 回傳 2.1.317 那份；再建 2.1.40 確認不是字串比較 */ }
#[test] fn lookup_args_no_dirs_web_only() { let a = lookup_args(&cfg(vec![]), "q"); assert!(a.windows(2).any(|w| w == ["--tools", "WebSearch"])); assert!(!a.iter().any(|x| x == "--add-dir")); }
#[test] fn lookup_args_with_dir_excludes_secrets() { let a = lookup_args(&cfg(vec!["/tmp/docs".into()]), "q"); assert!(a.iter().any(|x| x.contains("Read(**/.env*)"))); assert!(a.iter().any(|x| x.contains("Grep(**/.ssh/"))); }
#[tokio::test] async fn session_streams_answer() { /* FAKE_CLAUDE=ok：ask 回「一公斤是1000 公克。」，on_delta 被呼叫 2 次 */ }
#[tokio::test] async fn session_survives_garbage_line() { /* FAKE_CLAUDE=garbage：仍回正常答案 */ }
#[tokio::test] async fn session_reports_crash_mid_answer() { /* FAKE_CLAUDE=crash：Err(Crashed)，下一次 ask 會自動重開 */ }
#[tokio::test] async fn session_detects_not_logged_in() { /* FAKE_CLAUDE=nologin → Err(NotLoggedIn) */ }
#[tokio::test] async fn lookup_times_out() { /* FAKE_CLAUDE=slow → 30 秒內回 Err(Timeout)；測試用可注入的逾時值 1 秒 */ }
```

- [ ] **Step 3：Run** `cargo test -p engine --test claude` → FAIL
- [ ] **Step 4：實作**（`lookup` 的逾時用參數 `timeout: Duration`，正式值 30 秒）
- [ ] **Step 5：Run** → PASS
- [ ] **Step 6：真機冒煙（手動，不進 CI）**：`cargo run -p engine --example ask -- "一公斤幾公克"` 用真的 `claude` 回答；記下首字延遲。
- [ ] **Step 7：Commit** `feat(engine): Claude CLI 常駐對話與查詢`

---

### Task 8: 平台 trait、暫停判斷、事件紀錄、設定

**Files:**
- Modify: `crates/engine/src/platform.rs`（補齊其餘 trait）
- Create: `crates/engine/src/pause.rs`、`events.rs`、`config.rs`

**Interfaces:**
- Produces:
  - `platform.rs`：
    - `pub trait AudioSource: Send { fn open(&mut self, device: Option<&str>) -> Result<()>; fn read_block(&mut self) -> Result<Vec<f32>, AudioError>; fn close(&mut self); }`（回 16 kHz、`BLOCK` 長度；`AudioError::Gone` 代表裝置消失或串流失效）
    - `pub trait MicUsage { fn others_using_mic(&self) -> bool; }`
    - `pub trait ScreenState { fn locked(&self) -> bool; }`
    - `pub trait Speaker: Send { fn say(&self, text: &str) -> Result<()>; fn chime(&self); }`（`say` 阻塞到說完）
    - `pub trait Notifier { fn notify(&self, title: &str, body: &str); }`
    - `pub struct InputDevice { name: String, bluetooth: bool }`；`pub trait Devices { fn inputs(&self) -> Vec<InputDevice>; fn default_input(&self) -> Option<InputDevice>; }`
    - Task 5／6 已建立的 `Spotter`、`Transcriber` 保持不變
  - `pause.rs`：`pub fn should_pause(others_using_mic: bool, locked: bool, user_muted: bool, resting: bool) -> bool`（任何一項為真就暫停）
  - `events.rs`：`pub struct EventLog`；`fn log(&self, ev: &str, fields: serde_json::Value)`：寫一行 JSONL，自動加 `t`。**欄位白名單**：`latency_ms`、`ok`、`route`、`error`、`gain`、`state`；其他欄位一律丟掉。診斷模式開啟時才允許 `text`。
  - `config.rs`：`pub struct Settings { input_device: Option<String>, read_dirs: Vec<PathBuf>, notes_path: PathBuf, hotkey: String, diagnostic: bool, engine: EngineKind }`；`pub enum EngineKind { ClaudeCli, ApiKey }`（`ApiKey` 先預留，選了就回 `Err("尚未支援")`）；`load(path)`／`save(path)`，缺欄位用預設值。

- [ ] **Step 1：寫失敗的測試**

```rust
#[test] fn pause_any() { assert!(!should_pause(false,false,false,false)); assert!(should_pause(true,false,false,false)); assert!(should_pause(false,true,false,false)); }
#[test] fn eventlog_drops_text_unless_diagnostic() { /* log("wake", {"ok":true,"text":"秘密"}) → 檔案不含「秘密」；diagnostic=true 時才含 */ }
#[test] fn settings_defaults_and_roundtrip() { /* 空 JSON → hotkey "Control+Option+Space"、read_dirs 空；save 再 load 相等 */ }
#[test] fn api_key_engine_not_supported_yet() {}
```

- [ ] **Step 2：Run** → FAIL　**Step 3：實作**　**Step 4：Run** → PASS
- [ ] **Step 5：Commit** `feat(engine): 平台介面、暫停判斷、事件紀錄、設定`

---

### Task 9: Mac 平台實作

**Files:**
- Create: `crates/platform-mac/{Cargo.toml,src/lib.rs,src/audio.rs,src/micusage.rs,src/devices.rs,src/screen.rs,src/speaker.rs}`

**Interfaces:**
- Consumes: Task 8 的 trait
- Produces: `MacAudio`（cpal 預設或指定輸入，轉單聲道，用 `audio::resample_to_16k` 轉成 16 kHz，以 channel 交給 `read_block`；cpal error callback → 下一次 `read_block` 回 `AudioError::Gone`；`open(None)` 時如果系統預設輸入是藍牙，改用第一個非藍牙的輸入，都沒有就回 `Err(AudioError::NoUsableInput)`，由狀態機進入 `NoMic`）、`MacMicUsage`（CoreAudio `kAudioHardwarePropertyProcessObjectList`＝`prs#`、`kAudioProcessPropertyIsRunningInput`＝`piri`、`kAudioProcessPropertyPID`＝`ppid`，scope `glob`、element 0，**排除自己的 pid**）、`MacDevices`（transport type `tran`，值為 `blue` 或 `blea` 就算藍牙）、`MacScreen`（`CGSessionCopyCurrentDictionary` 的 `CGSSessionScreenIsLocked`）、`MacSpeaker`（`say -v Meijia <text>`；`chime` 播 `/System/Library/Sounds/Tink.aiff`，用 `afplay`，不阻塞）。

- [ ] **Step 1：寫失敗的測試**（真機測試，`#[ignore]`）

```rust
#[test] #[ignore] fn micusage_sees_other_process() {
    // spawn 子程序：`cargo run -p platform-mac --example hold_mic`（用 cpal 開輸入 4 秒）；1.5 秒後 others_using_mic() == true；結束後 == false
}
#[test] #[ignore] fn micusage_ignores_self() { /* 自己用 MacAudio 開麥克風，others_using_mic() == false */ }
#[test] #[ignore] fn audio_reads_16k_blocks() { /* open(None)，讀 5 塊，每塊長度 1600 */ }
#[test] #[ignore] fn devices_list_has_default() {}
#[test] #[ignore] fn speaker_says_meijia() { /* say("測試") 回 Ok */ }
```

- [ ] **Step 2：Run** `cargo test -p platform-mac -- --ignored` → FAIL
- [ ] **Step 3：實作**（FFI 用 `coreaudio-sys` 或手寫 `extern`；`hold_mic` example 一起建）
- [ ] **Step 4：Run** → PASS
- [ ] **Step 5：Commit** `feat(platform-mac): 收音、麥克風使用偵測、藍牙判斷、鎖定、說話`

---

### Task 10: 狀態機（引擎主迴圈）

**Files:**
- Create: `crates/engine/src/pipeline.rs`、`crates/engine/tests/pipeline.rs`（全部用假實作）

**Interfaces:**
- Consumes: Task 1～8 全部
- Produces:
  - `pub enum State { Idle, Acking, Listening, Thinking, Speaking, FollowUp, Confirming, Paused, NoMic, Degraded }`
  - `pub enum Trigger { Hotkey, TrayClick }`；`pub struct Engine<P: Platform>`；`fn new(p: P, settings: Settings) -> Self`；`fn trigger(&self, t: Trigger)`（非 `Idle`／`FollowUp` 時忽略）；`fn set_muted(&self, on: bool)`；`fn run(&mut self, stop: &AtomicBool)`；`fn state(&self) -> State`；狀態改變時呼叫 `on_state: Box<dyn Fn(State) + Send>`（給外殼更新圖示）。
  - `Platform` 是把 Task 8 各 trait 綁在一起的 trait，附 `fn now(&self) -> DateTime<FixedOffset>` 方便測試注入時間。

主迴圈規則：
1. 每塊音訊：`Agc` → 環形緩衝 → `Spotter`。`Speaking`、`Acking` 狀態時**丟掉音訊**，不送進喚醒器，也不放進環形緩衝；說完清空緩衝並 `Spotter.reset()`。
2. 命中 → 再收 4 塊 → small 複核 → 通過才 `chime`＋`say("我在")` → `Listening`。快捷鍵和點圖示直接進 `Acking`。
3. `Listening` 用 `Endpointer`；`NoSpeech` 回 `Idle`。
4. 問題文字先試 `parse_local`；命中就本機處理。其他交給常駐快答，邊串流邊用 `route_prefix` 判斷：`Answer` 逐句 `say`；`Lookup` 先說「我查一下」再 `lookup`；`Action` 開一個新的 confirm 對話，把原句送進去，進 `Confirming`。
5. `Confirming`／`FollowUp`：說完後 8 秒內偵測到講話就直接聽寫，當成回覆或追問，不需要喚醒詞；8 秒沒聲音回 `Idle`。`Do` 就執行並說結果；`Cancel` 說「好，取消了」；`Err(ActionError)` 唸出錯誤並留在 `Confirming`。
6. 每 2 秒檢查 `should_pause`：要暫停就 `close()` 音訊進 `Paused`；恢復就重新 `open()`。
7. `read_block` 回 `Gone` → `close()`，每 2 秒重試 `open()`；重試期間狀態為 `NoMic`。
8. Claude 錯誤：`NotLoggedIn`／`NotFound` 或連續兩次 `Crashed` → `Degraded`。這時只做本機指令，問到其他問題就說「現在連不上 Claude，我只能報時和計時」，每 5 分鐘重試一次。
9. 每個狀態轉換都寫事件紀錄（只寫白名單欄位）。
10. 本機指令 `Rest`（「小小先休息」）：說「好，我先休息，要叫我請按快捷鍵」，關閉喚醒詞（`resting=true`）；快捷鍵或點圖示會解除休息並直接進 `Acking`。tray 的「靜音」是另一個開關（`user_muted`），只能從 tray 解除。

- [ ] **Step 1：寫失敗的測試**（假平台：音訊來源吐預錄的區塊序列；假 Spotter 在指定區塊回 true；假 Transcriber 依呼叫順序回固定字串；假 Speaker 把句子記到 Vec；假 Claude 用 Task 7 的 fake-claude.sh）

```rust
#[test] fn wake_then_quick_answer() { /* Spotter 命中、small 回「嘿小小」、turbo 回「一公斤幾公克」→ 說了 ["我在", "一公斤是1000 公克。"] */ }
#[test] fn rejected_verify_stays_idle() { /* small 回「可以再思考」→ 沒有任何 say */ }
#[test] fn local_timer_no_claude() { /* 問題「計時十分鐘」→ fake-claude 沒被呼叫；說「好，十分鐘」 */ }
#[test] fn action_confirm_do_reminder() { /* 快答回 <<ACTION>>；confirm 先回複述、使用者說「對」後回 <<DO>>{reminder…} → ReminderStore 多一筆、說「提醒設好了」 */ }
#[test] fn followup_without_wake_word() { /* 回答後 3 秒內講話 → 直接被聽寫成追問 */ }
#[test] fn drops_audio_while_speaking() { /* Speaking 期間餵入的區塊 Spotter.accept 呼叫次數為 0 */ }
#[test] fn pauses_when_other_app_uses_mic() { /* MicUsage 回 true → 2 秒內 State::Paused 且 AudioSource.close 被呼叫；回 false 後重新 open */ }
#[test] fn recovers_when_audio_source_errors() { /* read_block 回 Gone → NoMic → 下一次 open 成功回 Idle */ }
#[test] fn reopens_source_after_stream_dead() { /* 連續回 Gone 3 次後 open 成功，之後的喚醒照常 */ }
#[test] fn degraded_when_not_logged_in() { /* FAKE_CLAUDE=nologin → Degraded；問「一公斤幾公克」→ 說降級文案；問「現在幾點」仍正常 */ }
#[test] fn hotkey_ignored_while_listening() {}
#[test] fn rest_disables_wake_until_hotkey() { /* 「小小先休息」後 Spotter 命中不觸發；trigger(Hotkey) 後恢復 */ }
#[test] fn bluetooth_default_falls_back_or_nomic() { /* 假 AudioSource.open 回 NoUsableInput → State::NoMic */ }
```

- [ ] **Step 2：Run** `cargo test -p engine --test pipeline` → FAIL
- [ ] **Step 3：實作**（時間相關的 2 秒、8 秒、5 分鐘做成 `Timing` 結構，測試注入短值）
- [ ] **Step 4：Run** → PASS；再跑 `cargo test --workspace` 全綠
- [ ] **Step 5：Commit** `feat(engine): 主狀態機`

---

### Task 11: Tauri 外殼

**Files:**
- Modify: `src-tauri/Cargo.toml`、`src-tauri/src/main.rs`、`src-tauri/tauri.conf.json`、`src-tauri/capabilities/default.json`
- Create: `src-tauri/src/{tray.rs,reminders.rs,engine_host.rs}`、`src-tauri/icons/tray-{idle,listening,paused,warning}.png`

**Interfaces:**
- Consumes: `Engine`、`Settings`、`ReminderStore`、`MacPlatform`
- Produces: Tauri commands（給 UI 用）：`get_settings() -> Settings`、`save_settings(s: Settings)`、`list_inputs() -> Vec<InputDevice>`、`input_level(device: String) -> f32`（最近 100 ms RMS）、`check_claude() -> Result<String, String>`（回版本，或回「沒找到／沒登入」）、`check_voice() -> bool`（`say -v '?'` 含 `Meijia`）、`test_wake() -> bool`（10 秒內有一次通過複核就回 true）。

- [ ] **Step 1：寫失敗的測試**：`reminders.rs` 的 `fn schedule(store, now) -> Vec<(Duration, Reminder)>` 與 `fn missed_message(r) -> String`（「你錯過了一個提醒：開會」）

- [ ] **Step 2：Run** `cargo test -p xiaoxiao reminders` → FAIL

- [ ] **Step 3：實作**
  - app 設成 `LSUIElement`（只出現在選單列，不出現在 Dock）。
  - tray 選單：狀態文字（依 `State`）、靜音／取消靜音、設定…、Claude 未登入時出現「去登入 Claude」（開「終端機」執行 `claude`，並顯示說明）、結束。圖示依狀態切換。
  - 點 tray 圖示 → `engine.trigger(TrayClick)`；global-shortcut 註冊 `settings.hotkey` → `Hotkey`。
  - autostart plugin 預設開啟。
  - 引擎跑在 `std::thread`，`on_state` 透過 Tauri event 更新 tray。
  - 啟動時 `ReminderStore::reload`：過期的立刻通知，未到期的用 tokio sleep 排程；到期時 `Notifier` 通知，並 `say` 一次。計時器到期一樣處理。

- [ ] **Step 4：Run** 測試 PASS；`bun tauri dev` 手動確認：選單列出現圖示、按快捷鍵會叮一聲並說「我在」。

- [ ] **Step 5：Commit** `feat(app): 選單列外殼、快捷鍵、提醒排程`

---

### Task 12: 設定與第一次啟動引導

**Files:**
- Create: `ui/src/{main.ts,onboarding.ts,settings.ts,style.css}`、`ui/index.html`

**Interfaces:**
- Consumes: Task 11 的 commands

- [ ] **Step 1：引導五步**（每步都能「稍後再說」，但沒過第 1、4 步不能開始待命）
  1. 麥克風權限：說明橘點會一直亮；按鈕觸發系統授權。
  2. 選收音麥克風：列出 `list_inputs()`，每個都有即時音量條（`input_level` 每 200 ms 更新）。**藍牙裝置顯示為灰色**，附說明「用藍牙耳機收音會讓耳機變成通話音質」；10 秒內音量都低於 -80 dB 的裝置顯示「這支麥克風沒有聲音」。
  3. 語音：`check_voice()`；沒有美佳就顯示「系統設定 → 輔助使用 → 閱讀與朗讀 → 系統聲音旁的 ⓘ → 聲音，搜尋並下載『美佳』」。
  4. Claude：`check_claude()`；沒找到或沒登入就說明「請在 Claude Code 或 Claude Desktop 自己登入，小小不會碰你的帳號」，附重新檢查按鈕。
  5. 試喊：`test_wake()`，成功就打勾；失敗就提示可以先用快捷鍵。

- [ ] **Step 2：設定頁**：收音麥克風、快捷鍵、可讀資料夾（加入或移除；上方固定說明「這些資料夾的內容可能會送到 Claude；金鑰、`.env`、`.ssh` 一律不讀」）、筆記檔位置、診斷模式（開啟時顯示紅字「會記錄你說的每一句話，測完請關閉並刪除紀錄」）、回答引擎（Claude Code；「API key（之後支援）」為灰色）。

- [ ] **Step 3：手動驗收**：`bun tauri dev` 走完引導五步；接上藍牙耳機時它顯示為灰色；改設定、重開 app 後設定還在。

- [ ] **Step 4：Commit** `feat(ui): 第一次啟動引導與設定頁`

---

### Task 13: 模型下載、打包、發布說明

**Files:**
- Create: `src-tauri/src/models.rs`、`docs/install.md`、`docs/voice-acceptance.md`
- Modify: `README.md`、`src-tauri/tauri.conf.json`（bundle：dmg、附上 sherpa-onnx dylib 和授權檔）

**Interfaces:**
- Produces: `models::ensure(dir: &Path, manifest: &Manifest, progress: impl Fn(u64, u64)) -> Result<()>`：缺檔或 SHA256 不符就重新下載，下載到 `.part` 再 rename。

- [ ] **Step 1：寫失敗的測試**：`ensure` 對已存在且雜湊正確的檔案不重新下載；雜湊錯誤的會被刪除並回 `Err`（用本機 HTTP server 或 `file://`）。
- [ ] **Step 2：Run** → FAIL　**Step 3：實作**（引導第 1 步前先下載模型，畫面顯示進度）　**Step 4：Run** → PASS
- [ ] **Step 5：打包**：`bun tauri build` 產生 `.dmg`（不簽章）。在乾淨的使用者帳號上安裝：拖進「應用程式」，右鍵→打開略過 Gatekeeper，走完引導，快捷鍵問「現在幾點」能回答。
- [ ] **Step 6：文件**：`docs/install.md`（下載、略過未簽章警告、引導步驟、怎麼移除）；`docs/voice-acceptance.md` 抄錄設計 §11 第 3 點的驗收項目，附紀錄表格；README 加上安裝連結與「尚未在 Windows 上提供」。
- [ ] **Step 7：驗證**：`cargo test --workspace`、`cargo test --workspace -- --ignored`、`git grep -nE '/Users/[A-Za-z0-9]|C:\\\\Users\\\\[A-Za-z0-9]' -- ':!docs/plans'` 為空。
- [ ] **Step 8：Commit** `feat: 模型下載與 Mac 打包`

---

## 交付後（不在本計畫內）

- 語音實測：照 `docs/voice-acceptance.md` 進行（使用者決定放到最後一起做，含 Windows）。
- Windows 計畫：`crates/platform-windows`（WASAPI 收音、麥克風使用偵測、台灣國語語音、開機啟動），另外寫。
- 公開發布：API key 引擎、簽章、自動更新。

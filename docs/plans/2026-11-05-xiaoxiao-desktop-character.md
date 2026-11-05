# 桌面小角色與休息模式 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **開工條件：** v0.2.0 發佈之後才開始。開工時先從當時的 `main` 開分支，並重新核對下列行號與介面（v0.2.0 收尾可能再改到）。

**Goal:** 設定完成後，打開小小只出現可拖曳的原創小角色，用造型顯示狀態、點一下說話、右鍵出選單，並把靜音與先休息合併成一個真的關麥克風的「休息」。

**Architecture:** 引擎（`crates/engine`）拿掉 `muted`、只留 `resting`，並新增暫停原因通知。對應與判斷都寫成 `crates/engine/src/character.rs` 的純函式以便測試。外殼新增第二個 Tauri 視窗 `character`（透明、無邊框、最上層），新檔 `src-tauri/src/character.rs` 負責顯示、位置與事件；前端新增 `character.html`、`src/character.ts`（頁面）與 `src/characters.ts`（四個造型的 SVG）。

**Tech Stack:** Rust、Tauri 2、TypeScript、Vite（改多頁面）、SVG＋CSS 動畫。

**Spec:** `docs/specs/2026-11-05-xiaoxiao-desktop-character-design.md`

## Global Constraints

- 造型必須原創，不使用 ChatGPT 的角色或素材；預設造型「小訊」。
- 休息時麥克風必須真的關閉；角色顯示的狀態必須與實際一致。
- 模型留在記憶體，計時在休息時照響；休息不跨啟動保留。
- 只做語音：不做打字輸入、文字回答、對話紀錄。不做開機自動啟動。
- 不改語音流程與 Claude 權限設定；聲音不存檔、不上傳；紀錄不含內容。
- 待命時整機 CPU 比沒有角色的版本增加不超過 1 個百分點；超過就把待命動畫改成靜態。
- 公開 repo：不放公司或客戶名稱、個人路徑。
- Mac 與 Windows 行為一致，兩邊都要實測。
- 所有介面文字用繁體中文。

## Review Focus

1. 設定頁開著時拖曳角色，再按儲存：位置不能被舊設定蓋掉（位置存獨立檔，不放 `Settings`；Task 2、4）。
2. 休息中遇到通話時按叫醒：休息清掉，但麥克風要等通話結束才開（`wake_during_call_waits_for_call_end`，Task 1）。
3. 使用者手動隱藏後通話結束：角色不能自己回來（`hidden_stays_hidden_after_call`，Task 2）。
4. 外接螢幕拔掉後：角色要回到可見範圍（`clamp_moves_back_when_monitor_gone`，Task 2）。
5. 休息中在設定頁存檔（引擎會重啟）：要維持休息，麥克風不能打開（`resting_before_run_opens_no_mic`，Task 1）。

---

### Task 1: 引擎合併靜音與休息，新增暫停原因

**Files:**
- Modify: `crates/engine/src/pause.rs`（`should_pause` 拿掉 `user_muted`；新增 `PauseReason`、`pause_reason`）
- Modify: `crates/engine/src/pipeline.rs`（`Shared.muted`、`set_muted`（`:121`、`:207`）、`want_pause`（`:232`）、`check_pause`（`:305`））
- Modify: `crates/engine/tests/pipeline.rs`（`:499` 的靜音測試改寫）
- Modify: `src-tauri/src/engine_host.rs`（`:186`、`:220-229`、`:291`、`:422-452`）、`src-tauri/src/tray.rs`（`:18`、`:67`、`:94-96`）

**Interfaces:**
- Produces: `pub enum PauseReason { Rest, Call, Lock }`（`Debug, Clone, Copy, PartialEq, Eq`）
- Produces: `pub fn pause_reason(others_using_mic: bool, locked: bool, resting: bool) -> Option<PauseReason>`，優先序 Call > Lock > Rest。
- Produces: `EngineHandle::set_resting(&self, on: bool)`、`resting(&self) -> bool`、`set_on_pause(&self, cb: Box<dyn Fn(Option<PauseReason>) + Send>)`。理由改變才呼叫 callback，由 `check_pause` 比對上次的值。

- [ ] **Step 1: 寫失敗的測試**

`pause.rs`：
```rust
#[test]
fn pause_reason_priority() {
    assert_eq!(pause_reason(false, false, false), None);
    assert_eq!(pause_reason(false, false, true), Some(PauseReason::Rest));
    assert_eq!(pause_reason(false, true, true), Some(PauseReason::Lock));
    assert_eq!(pause_reason(true, true, true), Some(PauseReason::Call));
}
```

`tests/pipeline.rs`（照既有 `rig` 寫法；世界的「其他程式用麥克風」旗標見 `:134`）：
- `rest_closes_mic_and_trigger_wakes`：`set_resting(true)` → 狀態 `Paused`、麥克風已關；`trigger` → `resting()` 為 false，且回到可聽。
- `wake_during_call_waits_for_call_end`：休息＋通話中 `trigger` → 仍 `Paused`、麥克風沒開、`resting()` 為 false；通話結束 → 麥克風打開。
- `resting_before_run_opens_no_mic`：`run` 之前 `set_resting(true)` → 收音從未被開啟。
- `timer_fires_while_resting`：休息中計時到點仍會說話。
- `pause_callback_reports_changes`：依序 休息→叫醒、通話開始→結束，callback 收到 `Some(Rest)`、`None`、`Some(Call)`、`None`，沒有重複。

- [ ] **Step 2: 跑測試確認失敗**

Run: `cargo test -p engine`
Expected: 編譯失敗，`PauseReason`、`set_resting` 未定義。

- [ ] **Step 3: 實作引擎改動**

拿掉 `Shared.muted` 與兩個 `set_muted`；`want_pause` 改用 `pause_reason(...).is_some()` 判斷，刪掉只剩無意義的 `should_pause` 及其測試。

- [ ] **Step 4: 外殼跟著改**

`Host.muted`、`muted()`、`set_muted()` 換成 `resting()`、`set_resting()`，轉呼叫 handle。引擎重啟（`restart`）前先讀舊 handle 的 `resting()`，啟動時傳給新 handle，取代 `:291`。`test_wake`（`:422-452`）改用同樣的方法暫時讓引擎休息，結束後還原。`tray.rs` 的 `mute` 改名 `rest`，文字「休息」／「叫醒」：休息中選「叫醒」呼叫 `trigger`，否則 `set_resting(true)`；文字依 `set_on_pause` 更新，語音指令進入休息時才會同步。

- [ ] **Step 5: 跑測試確認通過**

Run: `cargo test --workspace`
Expected: 全部通過。

- [ ] **Step 6: Commit**

```bash
git add crates/engine/src/pause.rs crates/engine/src/pipeline.rs crates/engine/tests/pipeline.rs src-tauri/src/engine_host.rs src-tauri/src/tray.rs
git commit -m "feat: 靜音與先休息合併成休息，並提供暫停原因"
```

### Task 2: 角色的純邏輯與設定

**Files:**
- Create: `crates/engine/src/character.rs`
- Modify: `crates/engine/src/lib.rs`（`pub mod character;`）
- Modify: `crates/engine/src/config.rs`（`Settings` 加 `character`）
- Modify: `src/api.ts`（`Settings` 型別加 `character`）

**Interfaces:**
- Consumes: `PauseReason`（Task 1）、`State`。
- Produces:
```rust
#[serde(rename_all = "lowercase")]
pub enum CharacterKind { Mic, Blob, Owl, #[default] Pixel }   // 小麥、小圓、小鴞、小訊
pub enum Look { Idle, Listen, Think, Speak, Pause }
pub fn look_for(s: State) -> Look;
pub fn is_warning(s: State) -> bool;
pub fn character_visible(onboarded: bool, onboarding_open: bool, user_hidden: bool, pause: Option<PauseReason>) -> bool;
pub struct Rect { pub x: i32, pub y: i32, pub w: u32, pub h: u32 }
pub fn clamp_to_monitors(x: i32, y: i32, w: u32, h: u32, monitors: &[Rect]) -> (i32, i32);
```
- Produces: `Settings.character: CharacterKind`。**位置不放進 `Settings`**（見 Review Focus 1）。

- [ ] **Step 1: 寫失敗的測試**

```rust
#[test]
fn look_matches_spec_table() {
    assert_eq!(look_for(State::Idle), Look::Idle);
    for s in [State::Acking, State::Listening, State::FollowUp, State::Confirming] { assert_eq!(look_for(s), Look::Listen); }
    assert_eq!(look_for(State::Thinking), Look::Think);
    assert_eq!(look_for(State::Speaking), Look::Speak);
    assert_eq!(look_for(State::Paused), Look::Pause);
    for s in [State::NoMic, State::Degraded] { assert_eq!(look_for(s), Look::Idle); assert!(is_warning(s)); }
}
#[test]
fn hidden_during_call_and_onboarding() {
    assert!(character_visible(true, false, false, None));
    assert!(character_visible(true, false, false, Some(PauseReason::Rest)));
    assert!(!character_visible(true, false, false, Some(PauseReason::Call)));
    assert!(!character_visible(false, false, false, None));
    assert!(!character_visible(true, true, false, None));
}
#[test]
fn hidden_stays_hidden_after_call() {
    assert!(!character_visible(true, false, true, None));
}
#[test]
fn clamp_keeps_position_inside_a_monitor() {
    let m = [Rect { x: 0, y: 0, w: 1000, h: 800 }];
    assert_eq!(clamp_to_monitors(100, 100, 96, 96, &m), (100, 100));
}
#[test]
fn clamp_moves_back_when_monitor_gone() {
    let m = [Rect { x: 0, y: 0, w: 1000, h: 800 }];
    let (x, y) = clamp_to_monitors(2500, 300, 96, 96, &m);
    assert!(x + 96 <= 1000 && y + 96 <= 800);
}
```
`config.rs`：`settings_without_character_load_as_pixel`：舊設定 JSON（沒有 `character`）解析後為 `CharacterKind::Pixel`。

- [ ] **Step 2: 跑測試確認失敗**

Run: `cargo test -p engine character`
Expected: 編譯失敗，模組不存在。

- [ ] **Step 3: 實作**

`clamp_to_monitors`：位置完整落在某個螢幕內就不動；否則移到面積重疊最大的螢幕內，都不重疊就用第一個螢幕。`monitors` 為空時原樣回傳。

- [ ] **Step 4: 跑測試確認通過**

Run: `cargo test --workspace`
Expected: 全部通過。

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/character.rs crates/engine/src/lib.rs crates/engine/src/config.rs src/api.ts
git commit -m "feat: 角色狀態對應、顯示條件與位置修正"
```

### Task 3: 角色視窗與四個造型

**Files:**
- Create: `character.html`、`src/character.ts`、`src/characters.ts`、`src-tauri/src/character.rs`
- Modify: `vite.config.ts`（`build.rollupOptions.input` 加 `index.html`、`character.html`）
- Modify: `src-tauri/tauri.conf.json`、`src-tauri/capabilities/default.json`、`src-tauri/src/lib.rs`、`src-tauri/src/engine_host.rs`（`:293`、`:315`、`run_engine`）

**Interfaces:**
- Consumes: Task 1、2 的介面。
- Produces（`src-tauri/src/character.rs`）：`pub fn sync(app: &AppHandle)` 依 `character_visible` 顯示或收起；`pub fn on_state(app, State)` 發出 `character-state` 事件；`pub fn on_pause(app, Option<PauseReason>)` 記下原因後 `sync`；Tauri 指令 `character_state() -> CharacterView`。
- Produces：`CharacterView { kind: CharacterKind, look: Look, warning: Option<String> }`（序列化給前端）。
- Produces（`src/characters.ts`）：`export function render(kind: CharacterKind, look: Look): string` 回傳 SVG 字串。
- 外殼狀態放在 `app.manage` 的 `CharacterState`：`user_hidden`、`onboarding_open`、`pause`（都只在記憶體）。

- [ ] **Step 1: 設定視窗**

`tauri.conf.json` 的 `windows` 加 `label: "character"`、`url: "character.html"`、`width/height: 96`、`transparent`、`decorations: false`、`alwaysOnTop`、`skipTaskbar`、`shadow: false`、`resizable: false`、`visible: false`、`visibleOnAllWorkspaces`；`app` 加 `"macOSPrivateApi": true`（Mac 透明視窗需要）。`capabilities` 的 `windows` 加 `character`，permissions 加 `core:window:allow-start-dragging`、`core:event:default`。`on_window_event` 的關閉攔截只處理 `main`。

- [ ] **Step 2: 移植造型**

把 `docs/specs/assets/2026-11-05-character-sketches.html` 的四個造型五種狀態移植到 `src/characters.ts`，並精修。使用者設定 `prefers-reduced-motion` 時關掉動畫；NoMic／Degraded 加警告標記，`title` 顯示原因（與 `tray::status_text` 相同文字）。

- [ ] **Step 3: 接上狀態**

`character.ts`：載入時呼叫 `character_state`，再監聽 `character-state`、`character-kind`，更新畫面。`engine_host.rs:293` 的 callback 同時通知 tray 與 `character::on_state`；`set_on_pause` 接 `character::on_pause`。引擎啟動後呼叫 `character::sync`；`!onboarded` 時照舊 `show_window`、不顯示角色。

- [ ] **Step 4: 確認建置與自動測試**

Run: `bun tauri build && cargo test --workspace`
Expected: 都成功。

- [ ] **Step 5: Mac 實機確認**

安裝後走完引導，確認角色背景透明、最上層、不在 Dock；五種狀態與實際相符。不符就修到相符。

- [ ] **Step 6: Commit**

```bash
git add character.html src/character.ts src/characters.ts src-tauri/src/character.rs vite.config.ts src-tauri/tauri.conf.json src-tauri/capabilities/default.json src-tauri/src/lib.rs src-tauri/src/engine_host.rs
git commit -m "feat: 桌面小角色視窗與四個造型"
```

### Task 4: 點擊、拖曳、右鍵、隱藏、開啟流程

**Files:**
- Modify: `src/character.ts`、`src-tauri/src/character.rs`、`src-tauri/src/tray.rs`、`src-tauri/src/lib.rs:13`、`src/main.ts`、`src/onboarding.ts`、`src/api.ts`

**Interfaces:**
- Consumes: `character::sync`、`Host::trigger`、`Host::set_resting`。
- Produces 指令：`character_click()`（`trigger(Trigger::TrayClick)`）、`character_menu()`（`window.popup_menu`）、`set_character_hidden(hidden: bool)`、`set_onboarding_open(open: bool)`、`finish_onboarding()`（隱藏主視窗並 `sync`）。
- Produces 檔案：位置存在資料夾下的 `character-pos.json`（`{"x":..,"y":..}`）。

- [ ] **Step 1: 寫失敗的測試**

`src-tauri/src/character.rs`（或抽到 engine 的純函式）：`pos_file_roundtrip`、`pos_file_broken_returns_none`：壞檔回傳 `None`，不是錯誤。

- [ ] **Step 2: 跑測試確認失敗**

Run: `cargo test -p xiaoxiao character`
Expected: 失敗，函式不存在。

- [ ] **Step 3: 實作點擊與拖曳**

`pointerdown` 後移動超過 4 px 就 `getCurrentWindow().startDragging()`；沒有移動的 `pointerup`（左鍵）呼叫 `character_click`；`contextmenu` 呼叫 `character_menu` 並 `preventDefault`。

- [ ] **Step 4: 實作位置記憶**

視窗 `Moved` 事件後 500 ms 沒有再移動就寫 `character-pos.json`；顯示前讀檔，經 `clamp_to_monitors` 後設定位置；沒有檔案就放在主螢幕右下角。

- [ ] **Step 5: 實作選單**

右鍵選單：休息／叫醒、設定…、隱藏、結束小小；ID 與 tray 共用處理函式。tray 加「顯示小角色」／「隱藏小角色」。

- [ ] **Step 6: 實作開啟流程**

`single_instance` callback：已完成引導就清掉 `user_hidden` 並 `sync`，否則 `show_window`。`main.ts`：引導完成呼叫 `finish_onboarding`；進入「重新走一次引導」呼叫 `set_onboarding_open(true)`，結束時改回 false。

- [ ] **Step 7: 確認**

Run: `cargo test --workspace && bun tauri build`
Expected: 都成功。

- [ ] **Step 8: Commit**

```bash
git add src/character.ts src-tauri/src/character.rs src-tauri/src/tray.rs src-tauri/src/lib.rs src/main.ts src/onboarding.ts src/api.ts
git commit -m "feat: 角色點擊、拖曳、右鍵選單與開啟流程"
```

### Task 5: 設定頁換造型、文件與驗收

**Files:**
- Modify: `src/settings.ts`、`src-tauri/src/engine_host.rs`
- Modify: `docs/manual.md`（`:15`、`:71-78`、`:90`、`:130`）、`docs/voice-acceptance.md`、`docs/install.md`、`README.md`、`docs/specs/2026-11-05-xiaoxiao-daily-assistant-app-design.md`
- Modify: `src-tauri/tauri.conf.json`（`version` → `0.3.0`）、`src-tauri/Cargo.toml`、`package.json`

**Interfaces:**
- Consumes: `render`（Task 3）、`character-kind` 事件。
- Produces 指令：`set_character(kind: CharacterKind)`：寫入設定但**不重啟引擎**，發出 `character-kind`。

- [ ] **Step 1: 設定頁**

加「小角色造型」四個選項與預覽；改變立即呼叫 `set_character`，也更新頁面的 `s.character`，之後按儲存才不會蓋回舊值。

- [ ] **Step 2: 文件**

手冊把「靜音」全改成「休息」，補上角色操作、換造型、隱藏與叫回；驗收清單加入 spec「怎麼算完成」八項。

- [ ] **Step 3: Mac 實機驗收**

逐項跑 spec「怎麼算完成」1～8，結果記入 `docs/voice-acceptance.md`。休息時確認選單列的麥克風燈號熄滅。CPU 用 `top -pid` 與 v0.2.0 比較。

- [ ] **Step 4: Windows 實機驗收**

**動 Windows 機器前先告訴使用者。** 打包安裝到測試機，同樣跑 1～8。CPU 超過上限就把待命動畫改成靜態再量一次。

- [ ] **Step 5: 全部測試**

Run: `cargo test --workspace && bunx tsc --noEmit`
Expected: 都通過。

- [ ] **Step 6: Commit**

```bash
git add src/settings.ts src-tauri/src/engine_host.rs docs/manual.md docs/voice-acceptance.md docs/install.md README.md docs/specs/2026-11-05-xiaoxiao-daily-assistant-app-design.md src-tauri/tauri.conf.json src-tauri/Cargo.toml package.json
git commit -m "feat: 設定頁換造型，更新文件並升版 0.3.0"
```

發佈（push、CI、合併、tag）依使用者當時的指示，不在此計畫內自行進行。

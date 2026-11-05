> **狀態：** 程式已實作，手動驗收與發佈未完成（清單見 `docs/voice-acceptance.md`）。內容中的開機啟動已被後續決定取代。

# 小小 Windows 版 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 讓 GitHub Release 同時提供 Windows x64 安裝程式，一台沒裝過小小的 Windows 11 能裝起來、喊「嘿，小小」並聽到回答。

**Architecture:** 新增 `crates/platform-windows` 實作既有的 `engine::platform` trait；Mac 與 Windows 共用的 cpal 收音抽成 `crates/audio-cpal`。`src-tauri` 透過 `os.rs` 依平台選實作。開發機是 Mac，所以 Windows 的編譯與測試都在 CI 跑；真機測試程式由 CI 編好，下載到測試機執行。

**Tech Stack:** Rust（`windows` 0.62、cpal 0.18）、Tauri 2（NSIS）、sherpa-onnx 2.0.1（win-x64 MD Release no-tts）、whisper-rs 0.16（CPU）、SAPI 語音、GitHub Actions `windows-latest`。

**Spec:** `docs/specs/2026-11-05-xiaoxiao-daily-assistant-app-design.md`（Windows 相關：§4、§7、§8、§11.2、§13、§14）

## Global Constraints

- app 不得自建 Claude 登入，也不得讀取或保存 token（§6）。
- 喚醒前的聲音只在記憶體，不落檔、不外送；事件紀錄不含內容，診斷模式除外（§7）。
- 其他程序在用麥克風就暫停並放開麥克風，每 2 秒檢查（§7）；偵測不到時走備案：通話中只留快捷鍵、喚醒詞關閉，設定頁標明「此平台無法自動偵測通話」。
- 預設輸入是藍牙耳機時不拿它收音，改用內建或 USB（§7）。
- 查資料一律排除 `.env*`、`.ssh/`、`*.pem`、`*.key`、`id_*`、憑證庫與瀏覽器設定檔（§5.3）。
- Windows 驗證失敗的功能可以降級上線，但要在設定頁標明，不可靜默缺功能（§13）。
- 不做自動更新；自用與熟人階段不簽章，教使用者略過系統警告（§9、§13）。
- 不得破壞 Mac 版：每個 task 都要在 Mac 跑過 `cargo test --workspace`。
- 公開 repo 不得出現公司名稱、客戶名稱、內網位址、個人路徑；測試資料裡的裝置序號、藍牙位址要換成假值。

## 已查證的事實（2026-11-05）

| 項目 | 結果 | 對計畫的影響 |
|---|---|---|
| sherpa-onnx `v2.0.1` | 有 `sherpa-onnx-v2.0.1-win-x64-shared-MD-Release-no-tts-lib.tar.bz2`，SHA256 `3aea6fc2360ac67b349f61b5cbe237f34b2bb53dc805aa4b3df7ab23ba7ab35c`；`lib/` 內有 `sherpa-onnx-c-api.{dll,lib}`、`onnxruntime.{dll,lib}`、`onnxruntime_providers_shared.dll` | 直接用 |
| 測試機（Windows 11 x64，Intel） | 有 cargo、bun、git、gh、`~/.local/bin/claude.exe`、bash；**沒有 MSVC、cmake**；有 VC runtime | 不能本機編譯；也不算全新機器，VC runtime 缺漏在這台測不出來 |
| 語音 | SAPI 有 `Microsoft Hanhan Desktop zh-TW` | 用 SAPI，依 `Language=404` 找 |
| 藍牙判斷 | 這台 Intel Smart Sound 上，藍牙與 USB 麥克風的 `PKEY_Device_EnumeratorName` 全是 `INTELAUDIO`；真正來源在 `{b3f8fa53-0004-438e-9003-51a46e139bfc},39`（如 `{1}.BTHENUM\…`、`{1}.USB\…`），沒有 offload 時在同組的 `,2` | 不能用 enumerator 名稱，要看 `,2` 與 `,39` |
| CPU 指令集 | whisper.cpp 預設 `GGML_NATIVE=ON`，MSVC 會偵測建置機的指令集；CI 機器有 AVX-512 時，編出來的程式到沒有 AVX-512 的電腦會當掉 | CI 明確設 `GGML_NATIVE=OFF`、`GGML_AVX=ON`、`GGML_AVX2=ON`、`GGML_BMI2=ON`（whisper-rs-sys 會轉傳 `GGML_*` 環境變數） |
| Claude Code 權限規則 | 官方文件：Windows 路徑先轉成 POSIX，`C:\Users\alice` 比對 `/c/Users/alice`，絕對路徑規則寫 `//c/Users/alice/…` | 現有規則在 Windows 上比對不到，會讓秘密檔排除失效 |
| Windows 內建 `onnxruntime.dll` | System32 可能有另一版，搜尋順序在 PATH 之前 | DLL 一定要放在 exe 同資料夾，不能靠 PATH |

**未驗證，靠本計畫的真機測試回答：** 麥克風工作階段偵測能否抓到 Teams／瀏覽器通話；沒裝 Git for Windows 時 `claude -p` 能否運作；Claude Desktop 內建 CLI 在 Windows 的路徑（測試機沒裝）；CPU 版 Whisper 的延遲是否達到快答 3 秒內。

## Review Focus

1. **Windows 使用者名稱含中文**（`C:\Users\王小明`）：whisper.cpp 用窄字元路徑開檔，應該仍能載入模型；由 Task 7 的 `short_path_of_cjk_dir_is_ascii` 與 Task 10 的真機項目驗證。
2. **CI 機器的 CPU 比使用者新**：編出來的程式不得依賴 AVX-512；由 Task 9 的編譯環境變數與 Task 10 在測試機實際聽寫驗證。
3. **乾淨電腦沒有 VC++ runtime**：安裝後應能直接啟動；由 Task 9 把 runtime 放進安裝目錄、CI 冒煙測試檢查檔案存在。
4. **藍牙麥克風經 Intel Smart Sound 轉接**：仍要判定為藍牙，不拿來收音；由 Task 5 的 `transport_uses_offload_property` 驗證。
5. **`read_dirs` 是 Windows 路徑**（含反斜線、磁碟代號、結尾斜線）：秘密檔規則要真的生效；由 Task 2 的 `secret_rules_use_posix_form_for_windows_paths` 與 Task 10 用真的 Claude CLI 驗證。

---

### Task 1: 測試不再依賴 Unix

**Files:**
- Create: `crates/engine/tests/fixtures/fake_claude.rs`
- Delete: `crates/engine/tests/fixtures/fake-claude.sh`
- Modify: `crates/engine/Cargo.toml`、`crates/engine/tests/claude.rs:3,11-18`、`crates/engine/tests/pipeline.rs:4,200-212`

**Interfaces:**
- Produces: `fake-claude` 可執行檔（`env!("CARGO_BIN_EXE_fake-claude")`）；模式與參數輸出路徑從執行檔旁的 `fake-mode.txt`（兩行：模式、輸出路徑）讀取。

- [ ] **Step 1:** 在 `crates/engine/Cargo.toml` 加 `[[bin]] name = "fake-claude"`、`path = "tests/fixtures/fake_claude.rs"`、`test = false`、`doc = false`（它是相依套件的 bin，不會被打包進 app）。
- [ ] **Step 2:** 把 `fake-claude.sh` 全部模式照原行為移植成 Rust：`--version`、`auth`、`ok`、`garbage`、`crash`、`nologin`、`action`、`action-repeat`、`slow`、`args`。輸出的 JSON 字串逐字相同。
- [ ] **Step 3:** 兩個測試檔的 `fake()`／`fake_claude()` 改成：把 fake 複製到 `dir.join(format!("claude{}", std::env::consts::EXE_SUFFIX))`，在旁邊寫 `fake-mode.txt`。拿掉 `PermissionsExt`。
- [ ] **Step 4:** 跑 `cargo test -p engine`。Expected：與移植前相同的 68 個測試全過（28＋17＋23）。
- [ ] **Step 5:** 刪掉 `fake-claude.sh`，commit `test: 假 claude 改用 Rust，測試不再依賴 Unix`。

### Task 2: 引擎的 Windows 行為

**Files:**
- Modify: `crates/engine/src/claude.rs:21-33,71-88,118-131,188,263,284`
- Test: `crates/engine/tests/claude.rs`

**Interfaces:**
- Produces: `pub fn posix_abs(dir: &str) -> String`、`pub fn quiet(cmd: &mut tokio::process::Command)`；`find_claude` 簽章不變。

- [ ] **Step 1: 寫失敗的測試**

```rust
#[test]
fn secret_rules_use_posix_form_for_windows_paths() {
    let a = lookup_args(&cfg(vec![r"C:\Users\me\docs\".into()]), "q");
    assert!(a.iter().any(|x| x == "Read(//c/Users/me/docs/**/.env*)"), "{a:?}");
    assert!(a.iter().any(|x| x.starts_with("Glob(//c/Users/me/docs/**/AppData/Local/Google/Chrome/")));
    assert!(a.windows(2).any(|w| w == ["--add-dir", r"C:\Users\me\docs\"]));
}

#[test]
fn posix_abs_keeps_unix_paths() {
    assert_eq!(posix_abs("/tmp/docs/"), "/tmp/docs");
    assert_eq!(posix_abs(r"D:\a b\c"), "/d/a b/c");
}

#[test]
fn finds_exe_on_path() { /* 暫存資料夾放 claude{EXE_SUFFIX}，find_claude 回該路徑 */ }

#[test]
fn finds_newest_desktop_cli_flat_layout() { /* claude-code/<ver>/<hash>/claude{EXE_SUFFIX} */ }
```

- [ ] **Step 2:** 跑 `cargo test -p engine --test claude`。Expected：FAIL（`posix_abs` 不存在）。
- [ ] **Step 3:** 實作：
  - `posix_abs` 是純字串函式，所有平台都能測：符合 `^[A-Za-z]:[\\/]` 才轉成 `/x/…`（磁碟代號小寫、反斜線換斜線），一律去掉結尾斜線。規則改成 `format!("{t}(/{}/**/{p})", posix_abs(&d))`。
  - `SECRET_PATTERNS` 加上 `AppData/Local/Google/Chrome/**`、`AppData/Local/Microsoft/Edge/**`、`AppData/Local/BraveSoftware/**`、`AppData/Roaming/Mozilla/Firefox/**`、`AppData/Local/Microsoft/Credentials/**`、`AppData/Roaming/Microsoft/Credentials/**`、`AppData/Roaming/Microsoft/Protect/**`。
  - `find_claude` 用 `format!("claude{}", EXE_SUFFIX)`；不接受 `claude.cmd`（Rust 拒絕把多行參數傳給批次檔，系統提示是多行）。Desktop 版除了 Mac 的 `claude.app/Contents/MacOS/claude`，也找 `<hash>/claude{EXE_SUFFIX}`。
  - `quiet`：Windows 上 `creation_flags(0x0800_0000)`（CREATE_NO_WINDOW），其他平台不做事。三處 `Command::new(&cfg.bin)` 都呼叫它，否則每次回答都會跳出黑視窗。
- [ ] **Step 4:** 跑 `cargo test -p engine`。Expected：全過，既有的 `//tmp/docs` 斷言不變。
- [ ] **Step 5:** commit `feat: 引擎支援 Windows 路徑、claude.exe 與隱藏主控台視窗`。

### Task 3: 建置鏈與 Windows 測試 CI

第一次拿到 Windows 的真實回饋，所以排在平台層之前。

**Files:**
- Create: `.gitattributes`、`.github/workflows/windows-ci.yml`
- Modify: `crates/kws/build.rs`、`crates/asr/Cargo.toml`、`crates/platform-mac/Cargo.toml`、`crates/platform-mac/src/lib.rs`、`crates/platform-mac/tests/real.rs`、`crates/platform-mac/examples/hold_mic.rs`、`models/manifest.json`、`scripts/fetch-models.sh`、`src-tauri/Cargo.toml`、`src-tauri/src/models.rs:57`

**Interfaces:**
- Produces: `models/sherpa-onnx-lib/lib/` 在 Windows 有 DLL；分支 `windows` 的每次 push 都會跑 Windows 測試；`models::tar_bin() -> PathBuf`。

- [ ] **Step 1:** `.gitattributes` 寫 `*.sh text eol=lf`（runner 的 autocrlf 會讓腳本壞掉）。
- [ ] **Step 2:** `manifest.json` 新增 `sherpa-onnx-lib-windows-x64`（上表 URL 與 SHA256、`extract_to: sherpa-onnx-lib`、`dev_only: true`），兩個 lib 項目各加 `"platform": "macos"`／`"windows"`。`fetch-models.sh` 沒帶 `--only` 時跳過不符合目前平台的項目（否則兩份解到同一個資料夾），並改成 `PYTHONUTF8=1 python3 -`（runner 預設編碼印不出中文）。
- [ ] **Step 3:** `kws/build.rs` 依 `CARGO_CFG_TARGET_OS` 分支：Mac 維持 rpath；Windows 不加 rpath，並把 `lib/*.dll` 複製到 `OUT_DIR` 往上三層的 `target/<profile>/` 與其 `deps/`、`examples/`。
- [ ] **Step 4:** `asr/Cargo.toml` 的 `metal` feature 限 `cfg(target_os = "macos")`，其他平台用不帶 feature 的 `whisper-rs`。
- [ ] **Step 5:** `platform-mac` 的 `lib.rs`、`tests/real.rs` 加 `#![cfg(target_os = "macos")]`，`hold_mic` 非 Mac 時是空 `main`；`src-tauri/Cargo.toml` 的 `platform-mac` 移到 `[target.'cfg(target_os = "macos")'.dependencies]`。讓 `cargo test --workspace` 在兩邊都能用同一條指令。
- [ ] **Step 6:** `models.rs` 的 `tar` 改呼叫 `tar_bin()`：Windows 回 `%SystemRoot%\System32\tar.exe`（PATH 上的 Git tar 會把 `C:` 當成遠端主機），並設 CREATE_NO_WINDOW；測試也改用它。
- [ ] **Step 7:** 新增 `windows-ci.yml`，`on: push: branches: [windows]` 與 `workflow_dispatch`，`windows-latest` 上依序：rust-cache、setup-bun、`bash scripts/fetch-models.sh --only sherpa-onnx-lib-windows-x64`、`cargo test --workspace --exclude xiaoxiao`。這個 task 先排除還編不過的 app。
- [ ] **Step 8:** Mac 跑 `cargo test --workspace`、`scripts/fetch-models.sh`（確認不動既有檔案）。Expected：全過。
- [ ] **Step 9:** commit `build: 建置鏈支援 Windows，新增 Windows 測試 CI`，**先問使用者**再 push 到 `windows` 分支，看 CI。Expected：engine、kws、asr 在 Windows 全過。失敗就照 systematic-debugging 修，這裡可能出現 Mac 沒遇過的編譯問題。

### Task 4: 抽出共用收音

**Files:**
- Create: `crates/audio-cpal/Cargo.toml`、`crates/audio-cpal/src/lib.rs`
- Modify: `crates/platform-mac/src/audio.rs`、`crates/platform-mac/src/lib.rs`、`crates/platform-mac/Cargo.toml`

**Interfaces:**
- Produces: `audio_cpal::CpalAudio::new(choose: fn(Option<&str>) -> Result<String, AudioError>)`，實作 `AudioSource`，並有 `device_name()`。

- [ ] **Step 1:** 把 `platform-mac/src/audio.rs` 除了 `choose` 以外的內容搬進 `audio-cpal`，行為不動。`default_changed` 改呼叫注入的 `choose(None)`。
- [ ] **Step 2:** `platform-mac` 的 `MacAudio` 變成薄包裝：`pub fn new() -> CpalAudio { CpalAudio::new(choose) }`，對外名稱維持可用。
- [ ] **Step 3:** 跑 `cargo test --workspace`，再跑 `cargo test -p platform-mac -- --ignored --test-threads=1`。Expected：全過（`--ignored` 這條要開 Mac 麥克風，不在辦公室吵到人的時間跑）。
- [ ] **Step 4:** commit `refactor: 收音抽成 audio-cpal，Mac 與 Windows 共用`。

### Task 5: Windows 裝置與收音

**Files:**
- Create: `crates/platform-windows/{Cargo.toml,src/lib.rs,src/com.rs,src/devices.rs,src/audio.rs,tests/real.rs}`
- Modify: `.github/workflows/windows-ci.yml`

**Interfaces:**
- Consumes: `audio_cpal::CpalAudio`
- Produces: `WinDevices`（`Devices`）、`WinAudio::new() -> CpalAudio`、`com::Com::init() -> Com`（drop 時對應 `CoUninitialize`）。

- [ ] **Step 1: 寫失敗的測試**（`devices.rs` 內，字串形狀取自實機、序號換假值）

```rust
#[test]
fn transport_uses_offload_property() {
    // Intel Smart Sound：,2 是 INTELAUDIO，真正來源在 ,39
    assert_eq!(transport(Some(r"{1}.INTELAUDIO\CTLR_X\0"), Some(r"{1}.BTHENUM\{0000111F}_X\0")), Transport::Bluetooth);
    assert_eq!(transport(Some(r"{1}.INTELAUDIO\CTLR_X\0"), Some(r"{1}.USB\VID_0000&PID_0000\0")), Transport::Usb);
    assert_eq!(transport(Some(r"{1}.INTELAUDIO\CTLR_X\0"), None), Transport::Builtin);
}

#[test]
fn transport_without_offload() {
    assert_eq!(transport(Some(r"{1}.BTHHFENUM\X\0"), None), Transport::Bluetooth);
    assert_eq!(transport(Some(r"{1}.USB\VID_0000\0"), None), Transport::Usb);
    assert_eq!(transport(Some(r"{1}.HDAUDIO\X\0"), None), Transport::Builtin);
    assert_eq!(transport(None, None), Transport::Other);
}

#[test]
fn fallback_prefers_builtin_then_usb_never_bluetooth() { /* rank 的純函式測試 */ }
```

- [ ] **Step 2:** 跑 CI。Expected：FAIL（`transport` 不存在）。
- [ ] **Step 3:** 實作。`transport` 看 `,39` 優先、其次 `,2`，去掉 `{1}.` 後比前綴：`BTH` → Bluetooth；`USB` → Usb；`INTELAUDIO`、`HDAUDIO` → Builtin；其他 → Other。`WinDevices` 用 `IMMDeviceEnumerator::EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)`，名稱取 `PKEY_Device_FriendlyName`（cpal 用同一個值）；預設裝置用 `GetDefaultAudioEndpoint(eCapture, eConsole)`。`fallback_input` 順序：Builtin > Usb > Other，排除 Bluetooth。`Com::init` 容許 `S_FALSE` 與 `RPC_E_CHANGED_MODE`（Tauri 主執行緒已是 STA），後者不呼叫 uninit。全 crate 加 `#![cfg(windows)]`，`windows` 依賴限 `cfg(windows)`，版本用 Cargo.lock 既有的 0.62。
- [ ] **Step 4:** `tests/real.rs`（都 `#[ignore]`）移植 Mac 的 `audio_reads_16k_blocks`、`audio_never_picks_bluetooth_by_default`、`devices_list_has_default`、`default_unchanged_right_after_open`。
- [ ] **Step 5:** `windows-ci.yml` 加步驟：`cargo test -p platform-windows --no-run`，把測試 exe 與 `hold_mic.exe` 上傳成 artifact `real-tests`。
- [ ] **Step 6:** CI 全過後，在測試機互動式工作階段（SSH 拿不到麥克風，要用桌面上的終端機）用 `gh run download` 取 artifact，跑 `<real.exe> --ignored --test-threads=1`。Expected：全過；有接藍牙耳機時也不會選到它。
- [ ] **Step 7:** commit `feat: Windows 裝置列舉、藍牙判斷與收音`。

### Task 6: 暫停偵測與麥克風權限

**Files:**
- Create: `crates/platform-windows/{src/micusage.rs,src/screen.rs,src/permission.rs,examples/hold_mic.rs}`
- Modify: `crates/engine/src/platform.rs`、`crates/platform-mac/src/permission.rs`、`crates/platform-windows/tests/real.rs`

**Interfaces:**
- Produces: `engine::platform::MicPermission`（從 Mac crate 搬上來；`platform_mac` 仍 re-export）、`WinMicUsage::new()`、`WinScreen`、`platform_windows::mic_permission() -> MicPermission`。

- [ ] **Step 1: 寫失敗的測試**

```rust
#[test]
fn permission_from_registry() {
    assert_eq!(decide(None, None, None), MicPermission::Authorized); // 沒設定就是允許
    assert_eq!(decide(Some("Deny"), Some("Allow"), Some("Allow")), MicPermission::Restricted); // 全機關閉
    assert_eq!(decide(None, Some("Deny"), Some("Allow")), MicPermission::Denied);
    assert_eq!(decide(None, Some("Allow"), Some("Deny")), MicPermission::Denied); // 只關傳統桌面 app
}

#[test]
fn locked_from_session_flags() {
    assert!(locked_flag(0));  // WTS_SESSIONSTATE_LOCK
    assert!(!locked_flag(1)); // WTS_SESSIONSTATE_UNLOCK
    assert!(!locked_flag(0xFFFF_FFFF)); // 未知：不要無故暫停
}

#[test]
fn only_active_sessions_of_other_processes_count() {
    assert!(counts(ACTIVE, 42, false, 7));
    assert!(!counts(ACTIVE, 7, false, 7));   // 自己
    assert!(!counts(INACTIVE, 42, false, 7));
    assert!(!counts(ACTIVE, 42, true, 7));   // 系統音效工作階段
}
```

- [ ] **Step 2:** 跑 CI。Expected：FAIL。
- [ ] **Step 3:** 實作：
  - `WinMicUsage`：對每個 active capture 端點 `Activate::<IAudioSessionManager2>` → `GetSessionEnumerator`，逐一 `QueryInterface::<IAudioSessionControl2>` 取 `GetState`、`GetProcessId`、`IsSystemSoundsSession`，再交給 `counts`。**不用** `ConsentStore\LastUsedTimeStop`：社群回報不穩，程式當掉還會留下「使用中」，小小就永遠暫停；工作階段 API 有文件、有 PID，也沒有殘留問題。
  - `WinScreen`：`WTSQuerySessionInformationW(WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION, WTSSessionInfoEx)`，取 `SessionFlags` 交給 `locked_flag`。
  - `mic_permission`：讀 HKLM 的 `…\CapabilityAccessManager\ConsentStore\microphone` `Value`、HKCU 同路徑、HKCU `…\microphone\NonPackaged` `Value`，交給 `decide`。Windows 傳統桌面 app 不會跳詢問視窗，所以永遠不回 `NotDetermined`。
- [ ] **Step 4:** 移植 `micusage_sees_other_process`、`micusage_ignores_self`、`screen_not_locked_during_test`。`hold_mic` 路徑先找測試 exe 同資料夾（artifact 情境），再找 `../examples/`。
- [ ] **Step 5:** CI 過後在測試機跑真機測試，再手動打開一個瀏覽器的麥克風測試頁或 Teams 通話，確認 2 秒內暫停。
  - Expected：`others_using_mic()` 回 true。
  - **如果偵測不到通話類軟體**：不要改用不可靠的方法。依 §7 備案，`os::MIC_USAGE_RELIABLE = false`，Task 8 的設定頁顯示「此平台無法自動偵測通話」，並把結果寫進 spec §3。
- [ ] **Step 6:** commit `feat: Windows 麥克風使用偵測、鎖定偵測與權限狀態`。

### Task 7: 說話與路徑

**Files:**
- Create: `crates/platform-windows/{src/speaker.rs,src/paths.rs}`
- Modify: `crates/platform-windows/src/lib.rs`、`crates/platform-windows/tests/real.rs`

**Interfaces:**
- Produces: `WinSpeaker`（`Speaker`）、`platform_windows::voice_ok() -> bool`、`platform_windows::short_path(&Path) -> PathBuf`。

- [ ] **Step 1: 寫失敗的測試**（不需音效裝置，CI 就能跑）

```rust
#[test]
fn short_path_of_cjk_dir_is_ascii() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path().join("語音模型");
    std::fs::create_dir(&d).unwrap();
    let s = short_path(&d);
    assert!(s.is_dir());
    assert!(s.to_str().unwrap().is_ascii(), "{s:?}");
}

#[test]
fn short_path_keeps_ascii_path_unchanged() { /* 全 ASCII 路徑原樣回傳 */ }
```

- [ ] **Step 2:** 跑 CI。Expected：FAIL。
- [ ] **Step 3:** 實作：
  - `short_path`：路徑全 ASCII 就原樣回傳；否則 `GetShortPathNameW`，失敗或結果仍非 ASCII 時回傳原值（8.3 被關閉的機器，由 app 轉成明確錯誤訊息，見 Task 8）。
  - `WinSpeaker::say`：每次呼叫都 `Com::init()`、`CoCreateInstance::<ISpVoice>(SpVoice)`，用 `ISpObjectTokenCategory::EnumTokens(SPCAT_VOICES, "Language=404")` 取第一個 token 設給 voice，再 `Speak(text, SPF_IS_NOT_XML)` 同步播完。沒有台灣語音就回 Err。每次建立是因為 COM 物件不是 `Send`，而 `Speaker` 要 `Send`；真機測試若發現明顯延遲才改常駐執行緒。
  - `chime`：`PlaySoundW("%SystemRoot%\Media\Windows Notify Calendar.wav", SND_FILENAME | SND_ASYNC | SND_NODEFAULT)`，不阻塞。
  - `voice_ok`：`EnumTokens` 有結果。
- [ ] **Step 4:** 移植 `speaker_says_taiwanese`（小音量），在測試機聽一次。Expected：聽得到台灣語音，說完才回傳。
- [ ] **Step 5:** commit `feat: Windows 語音、提示音與中文路徑處理`。

### Task 8: 接上 app

**Files:**
- Create: `src-tauri/src/os.rs`、`src-tauri/icons/tray-win-{idle,listening,paused,warning}.png`
- Modify: `src-tauri/Cargo.toml`、`src-tauri/src/engine_host.rs:13,34-63,104-125,191,364,374,390,412,430`、`src-tauri/src/tray.rs:23-31,100-116`、`src-tauri/src/lib.rs`、`src/api.ts`、`src/onboarding.ts`、`src/settings.ts`、`scripts/make-tray-icons.py`、`.github/workflows/windows-ci.yml`

**Interfaces:**
- Consumes: Task 5～7 的所有型別。
- Produces: `crate::os::{OsAudio, OsDevices, OsMicUsage, OsScreen, OsSpeaker, new_audio, mic_permission, voice_ok, model_path, open_terminal, MIC_USAGE_RELIABLE}`；前端 `IS_WINDOWS`。

- [ ] **Step 1:** `os.rs` 用 `#[cfg]` 把兩個平台對應到相同名稱。`model_path`：Mac 原樣；Windows 走 `short_path`，結果仍非 ASCII 時回 Err「模型路徑含非英文字元，這台電腦無法載入，請把 Windows 帳號資料夾換成英文或開啟 8.3 短檔名」。`open_terminal`：Mac 搬 `tray.rs:101-109`；Windows 用 `cmd /C start "" <bin>`。
- [ ] **Step 2:** `engine_host.rs`：`MacPlatform` 改名 `OsPlatform`，全部換成 `os::*`；`home()` 改用 `HOME` 或 `USERPROFILE`；`search_path` 的 `/opt/homebrew` 等只在 Mac 加，Windows 加 `~/.local/bin`；`claude_cfg` 的 Desktop 根目錄，Windows 依序找 `%APPDATA%\Claude`、`%LOCALAPPDATA%\Claude`；`check_voice` 改呼叫 `os::voice_ok()`；載入模型前先過 `os::model_path`。
- [ ] **Step 3:** `tray.rs`：Windows 的工作列可能是深色或淺色，黑色 template 圖示會看不見，`icon()` 在 Windows 用彩色的 `tray-win-*`（`make-tray-icons.py` 產生，深淺底都看得見）。`open_login` 改呼叫 `os::open_terminal`。
- [ ] **Step 4:** 前端：`api.ts` 加 `IS_WINDOWS = navigator.userAgent.includes("Windows")`。文字依平台切換：
  - 第 1 步：不提橘點；失敗訊息改「設定 → 隱私權與安全性 → 麥克風，打開『讓桌面應用程式存取麥克風』」。
  - 第 3 步：「已安裝台灣國語語音。」；未安裝時「到『設定 → 時間與語言 → 語言與地區』新增『中文（繁體，台灣）』並勾選語音，再按重新檢查」。
  - 設定頁快捷鍵範例 `Control+Alt+Space`，收音欄位與 `MIC_USAGE_RELIABLE=false` 時的「此平台無法自動偵測通話」提示。
- [ ] **Step 5:** `windows-ci.yml` 拿掉 `--exclude xiaoxiao`。如果 `src-tauri` 的單元測試以 `0xc0000139` 結束（Tauri 測試 exe 缺 comctl32 v6 manifest 的已知問題），在 `src-tauri/build.rs` 對 Windows 測試加 `cargo:rustc-link-arg-tests=/MANIFEST:EMBED` 與對應的 `/MANIFESTDEPENDENCY`，不要跳過測試。
- [ ] **Step 6:** 跑 Mac 的 `cargo test --workspace`、`bun run build`、`bun tauri build`。Expected：全過，產出的 Mac app 照常啟動。CI：Windows `cargo test --workspace` 全過。
- [ ] **Step 7:** commit `feat: app 接上 Windows 平台層與系統匣`。

### Task 9: 安裝程式與 Release

**Files:**
- Create: `src-tauri/tauri.windows.conf.json`
- Modify: `.github/workflows/release.yml`、`.github/workflows/windows-ci.yml`、`.gitignore`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`、`THIRD_PARTY_LICENSES.md`

**Interfaces:**
- Produces: Release 資產 `xiaoxiao_<版本>_x64-setup.exe`；安裝位置 `%LOCALAPPDATA%\xiaoxiao\`。

- [ ] **Step 1:** `tauri.windows.conf.json`（Tauri 會合併到主設定）：`bundle.targets: ["nsis"]`；`windows.nsis.installMode: "currentUser"`（不需管理員）、`languages: ["TradChinese"]`；`resources` 把 `sherpa-onnx-c-api.dll`、`onnxruntime.dll`、`onnxruntime_providers_shared.dll`、`win-runtime/{vcruntime140,vcruntime140_1,msvcp140}.dll` 對應到安裝根目錄（要在 exe 旁邊，見事實表）。
- [ ] **Step 2:** `release.yml` 改成三個 job：`macos`、`windows`，都把安裝檔傳成 artifact；`release` 用 `needs: [macos, windows]`，下載後執行一次 `gh release create`。**任一平台失敗就不發布**，也避免兩個 job 搶建同一個 Release。Release notes 改成兩個平台各自的略過警告說明。
- [ ] **Step 3:** `windows` job（`windows-latest`），job 層 `env: GGML_NATIVE: "OFF"`、`GGML_AVX: "ON"`、`GGML_AVX2: "ON"`、`GGML_BMI2: "ON"`。步驟：版本檢查 → rust-cache → setup-bun → `bash scripts/fetch-models.sh --only sherpa-onnx-lib-windows-x64` → 從 `%SystemRoot%\System32` 複製三個 VC runtime DLL 到 `src-tauri/win-runtime/`（gitignore；tauri-build 檢查資源，所以要在 test 之前）→ `cargo test --workspace` → `bun install --frozen-lockfile` → `bun tauri build` → 冒煙測試。
- [ ] **Step 4:** 冒煙測試（PowerShell）：`Start-Process <setup.exe> -ArgumentList '/S' -Wait`；確認 `%LOCALAPPDATA%\xiaoxiao\` 有 `xiaoxiao.exe`、所有上述 DLL；啟動後等 15 秒 `Get-Process xiaoxiao` 還活著；再 `Stop-Process`。Expected：通過。這一關能抓到 DLL 放錯位置與啟動就崩潰，但抓不到 VC runtime 缺漏，因為 runner 本身就裝了。
- [ ] **Step 5:** `windows-ci.yml` 也跑打包與冒煙，把 setup.exe 上傳成 artifact，讓測試機不用等發版就能裝。`release.yml` 的 `macos` job 只改最後一步（上傳 artifact 取代建立 Release），其餘不動。
- [ ] **Step 6:** `THIRD_PARTY_LICENSES.md` 補 ONNX Runtime 與 Microsoft VC++ runtime 的授權說明；`tauri.conf.json`、`src-tauri/Cargo.toml` 版本改 `0.2.0`。
- [ ] **Step 7:** CI 全過。Expected：`windows-ci` 的 artifact 內有 `xiaoxiao_0.2.0_x64-setup.exe`。commit `ci: 打包 Windows 安裝程式，Release 同時發布兩個平台`。

### Task 10: 文件與測試機驗收

**Files:**
- Modify: `docs/install.md`、`README.md`、`docs/voice-acceptance.md`、`docs/specs/2026-11-05-xiaoxiao-daily-assistant-app-design.md`

- [ ] **Step 1:** `install.md` 加 Windows 段：系統需求（Windows 10／11 x64、CPU 支援 AVX2、Claude Code 已登入）、SmartScreen「其他資訊 → 仍要執行」、台灣語音安裝方式。「讓 AI 協助安裝」提示詞加 Windows 分支：檢查 `PROCESSOR_ARCHITECTURE`、`claude --version`、`claude auth status`，從 `releases/latest` 找 `*_x64-setup.exe`，靜默安裝到使用者目錄，啟動後交代使用者要親手做的事。同樣不輸入密碼、不提權、不關閉安全功能。Mac 段「目前只有 Mac 版」的說明拿掉。
- [ ] **Step 2:** `README.md` 狀態改成兩個平台。`voice-acceptance.md` 加 Windows 真機清單（Step 3）。spec §3「未驗證」與 §7 依真機結果更新，記下 Windows 用的方法。
- [ ] **Step 3: 通知測試機安裝。** 從 `windows-ci` artifact 取 setup.exe，在測試機桌面工作階段逐項驗收並回報：
  - [ ] 安裝程式能裝、SmartScreen 能略過，開始功能表有小小，系統匣看得到圖示。
  - [ ] 引導五步都能走完；模型 1.1 GB 下載完成。
  - [ ] 喊「嘿，小小，一公斤幾公克」聽得到回答，記下問題講完到開口的秒數（目標 3 秒內，§11）。**CPU 版 Whisper 超標就回報數字**，要不要加 GPU 由使用者決定，不在本計畫內。
  - [ ] 全程沒有跳出黑色主控台視窗。
  - [ ] 打開通話或瀏覽器麥克風測試時小小暫停，結束後恢復；鎖定螢幕時暫停。
  - [ ] 接藍牙耳機（這台已配對過）當預設輸入時，小小不選它收音。
  - [ ] 快捷鍵 `Control+Alt+Space`、點系統匣圖示都能叫出小小；重開機後自動待命。
  - [ ] 秘密檔：把測試資料夾加入可讀，問它 `.env` 內容，應該讀不到；一般檔案讀得到。
  - [ ] 把模型複製到中文資料夾跑 `short_path` 真機測試，確認 whisper 與 sherpa-onnx 都載入得了。
  - [ ] 解除安裝乾淨，模型留在 `%APPDATA%` 的行為要寫進 `install.md`。
- [ ] **Step 4:** 把真機發現的問題逐項修掉（紅綠測試），文件與驗收結果一起 commit `docs: Windows 安裝說明與驗收清單`。
- [ ] **Step 5:** 分支合回 main 後，由使用者推 tag `v0.2.0`。CI 綠燈後確認 Release 有兩個安裝檔、`releases/latest` API 查得到兩者、Mac 的 dmg 仍能掛載且簽章通過。

**未涵蓋（要另外決定）：** 全新機器的 VC runtime 實測（測試機已有，需要 Windows Sandbox 或另一台）；Windows 程式碼簽章；§11 的語音門檻（20 次命中 18 次、30 分鐘誤觸）留到兩平台一起實測。

# 小小

日常語音助理：桌面上有個小角色，說「嘿，小小」就回應，幫你查資料、回答問題、記提醒和筆記。喚醒、聽寫、說話都在你的電腦上完成，只有問題的文字會送到 Claude。

> **狀態：試用中。** Mac 版已發布。v0.2.0 準備中，會加入 Windows 版、桌面小角色和休息模式；Windows 測試機實測回報通過，正式打包版還在驗證。

## 可以做什麼

說「嘿，小小」，聽到「我在」後再說：

| 你說 | 小小會 |
|---|---|
| 一公斤幾公克 | 直接回答 |
| 台北今天會下雨嗎 | 上網查，再唸結論 |
| 提醒我三點開會 | 複述「今天下午三點」，你說「對」才建立 |
| 計時十分鐘 | 在本機開始計時，不用連網 |
| 記一下要回客戶的信 | 確認後寫進筆記檔 |

也可以按快捷鍵、點桌面的小角色，或點選單列、系統匣的圖示叫它。回答後 8 秒內可以追問，開頭叫一聲「小小」就好。

### 小角色與休息

設定完成後，打開小小只會出現一個小角色，不開視窗，開機也不會自動啟動。角色的表情會顯示它在待命、聽、想、說話或休息；可以拖曳，按右鍵有選單，還有八個造型可換。

**待命 2 分鐘沒有互動，小小會自動休息並關掉麥克風，這時喊「嘿，小小」不會有反應。** 點一下角色或按快捷鍵，它會播起床音並直接開始聽，不用喊喚醒詞。

## 需要什麼

- Mac（Apple 晶片、macOS 13 以上），或 Windows 10／11 x64（CPU 支援 AVX2）。
- 已安裝並登入 [Claude Code](https://claude.com/claude-code)。小小只呼叫你自己登入的 `claude`，不會自己登入，也不會讀取憑證；問 Claude 會用到你的額度。
- 硬碟空間放語音模型（Mac 約 1.1 GB，Windows 約 0.5 GB），第一次開啟時下載。

安裝檔沒有付費簽章，第一次開啟時系統會警告，[安裝說明](docs/install.md)有略過的步驟。

## 開始使用

1. 到 [Releases](https://github.com/KerryHuang/xiaoxiao/releases) 下載安裝檔，照[安裝說明](docs/install.md)裝好。也可以把說明裡的提示詞貼給 Claude Code，請它協助安裝。
2. 開啟後照引導走完：下載模型、麥克風、聲音、連上 Claude、試喊一次。
3. 用法看[操作手冊](docs/manual.md)。

## 隱私

- 喚醒前的聲音只留在記憶體，不存檔也不外送；錄音不會送出去。
- 紀錄檔不記內容，除非你手動開啟診斷模式。
- 小小要讀你的檔案，得先在設定裡逐一加入資料夾。金鑰、`.env`、`.ssh` 一律不讀。
- 寫入提醒和筆記前一定先跟你確認。
- 通話、開會時（其他程式在用麥克風）和螢幕鎖定時，小小會自動暫停，角色在通話時也會收起來。
- 休息時麥克風真的關閉；閒置 2 分鐘會自動休息。

## 文件

- [安裝說明](docs/install.md)
- [操作手冊](docs/manual.md)
- [語音實測紀錄](docs/voice-acceptance.md)
- [設計文件](docs/specs/2026-11-05-xiaoxiao-daily-assistant-app-design.md)
- [桌面小角色與休息模式設計](docs/specs/2026-11-05-xiaoxiao-desktop-character-design.md)
- [語音助理做好了：小小誕生的一天](https://kerry-notes.kh-lab.workers.dev/posts/xiaoxiao-voice-assistant/)：這個專案的製作過程

## 開發

用 Tauri、Rust 和 TypeScript 寫成，一份程式碼出兩個平台的版本。

| 位置 | 內容 |
|---|---|
| `crates/engine` | 流程、分流、Claude 呼叫、提醒與筆記、休息與角色的判斷規則 |
| `crates/kws`、`crates/asr` | 喚醒詞（sherpa-onnx）、聽寫（whisper.cpp） |
| `crates/audio-cpal` | 兩個平台共用的收音 |
| `crates/platform-mac`、`crates/platform-windows` | 各平台的裝置、暫停偵測、語音 |
| `src-tauri`、`src` | 系統匣與視窗、桌面小角色、設定和引導畫面 |

需要 Rust（版本見 `rust-toolchain.toml`）、[Bun](https://bun.sh)、CMake 和 C++ 編譯器。

```bash
scripts/fetch-models.sh   # 下載模型與 sherpa-onnx，並驗證 SHA256
cargo test --workspace
bun install
bun tauri build
```

Windows 打包還要先把 VC++ runtime 複製到 `src-tauri/win-runtime/`，完整步驟見 [`windows-build.yml`](.github/workflows/windows-build.yml)。

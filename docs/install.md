# 安裝小小（Mac）

> 目前只有 Mac 版（Apple 晶片、macOS 13 以上）。尚未在 Windows 上提供。

## 事前準備

- 已安裝 [Claude Code](https://claude.com/claude-code)，並在終端機用自己的帳號登入過（`claude` → `/login`）。
  小小只會呼叫你電腦上的 `claude` 指令，不會自己登入、也不會讀取你的憑證。
- 約 1.1 GB 的硬碟空間（語音模型，第一次開啟時下載）。

## 下載與安裝

1. 到 [GitHub Releases](https://github.com/KerryHuang/xiaoxiao/releases) 下載最新的 `xiaoxiao_<版本>_aarch64.dmg`。
2. 打開 `.dmg`，把「xiaoxiao」拖進「應用程式」。
3. 第一次打開：這個版本沒有 Apple 簽章，系統會擋下並說「無法驗證開發者」。
   - 先在「應用程式」裡對 xiaoxiao 按右鍵 →「打開」。
   - 如果還是被擋（macOS 15 以後常見）：到「系統設定 → 隱私權與安全性」，往下捲到「已阻擋 xiaoxiao」，按「強制打開」，再輸入密碼確認。
   - 備案（會用終端機的話）：執行下面這行拿掉「從網路下載」的標記，之後就能直接打開：
     ```bash
     xattr -dr com.apple.quarantine /Applications/xiaoxiao.app
     ```

小小只出現在螢幕右上角的選單列，不會出現在 Dock。

## 讓 AI 協助安裝

已經裝好 Claude Code 的話，可以把下面整段貼給它，讓它幫你下載、安裝、檢查環境。需要你親手做的（登入、按「允許」、下載語音）它會停下來告訴你。

```text
請幫我在這台 Mac 安裝「小小」語音助理（https://github.com/KerryHuang/xiaoxiao）。
照順序做，每一步都把指令的結果讀完再往下；失敗就停下來告訴我原因，不要換別的方法硬裝。

1. 檢查環境：`uname -m` 要是 arm64，`sw_vers -productVersion` 要 13 以上；不符合就停止，告訴我這台不支援。
2. 檢查 Claude Code：執行 `claude --version` 和 `claude auth status`。
   沒裝或沒登入時，請告訴我自己執行 `claude` 再輸入 /login 登入；你不要幫我登入，也不要讀取任何憑證或 token。
3. 下載最新版安裝檔：用 GitHub API
   `curl -fsSL https://api.github.com/repos/KerryHuang/xiaoxiao/releases/latest`
   找出 .dmg 的 browser_download_url，下載到一個新建的暫存資料夾。告訴我版本號和檔案大小。
4. 安裝：`hdiutil attach` 掛載 .dmg，把 xiaoxiao.app 複製到 /Applications（已有舊版就先問我要不要覆蓋），
   然後 `hdiutil detach`，刪掉暫存資料夾。
5. 這版沒有 Apple 公證：執行 `xattr -dr com.apple.quarantine /Applications/xiaoxiao.app`，
   再用 `codesign -v --deep --strict /Applications/xiaoxiao.app` 確認簽章完整。
   不要關閉 Gatekeeper，也不要改任何系統安全設定。
6. 檢查語音：`say -v '?' | grep Meijia`。沒有的話，告訴我到「系統設定 → 輔助使用 → 閱讀與朗讀 →
   系統聲音旁的 ⓘ → 聲音」搜尋並下載「美佳」（這一步你沒辦法代勞）。
7. 執行 `open -a xiaoxiao` 打開小小，然後告訴我接下來要自己做的事：
   - 設定視窗會先下載約 1.1 GB 的語音模型，等它跑完；
   - 系統問要不要讓小小使用麥克風時，由我自己按「允許」；
   - 照視窗走完五個步驟，最後按「開始待命」。
8. 最後回報：哪些步驟做完了、哪些要我自己動手、有沒有任何失敗。

全程不要輸入我的密碼、不要用 sudo；需要管理員權限時停下來告訴我。
```

## 第一次設定（引導）

開啟後會自動跳出設定視窗：

0. **下載語音模型**：約 1.1 GB，只需要一次；中斷了重開會從頭檢查、補齊。
1. **允許使用麥克風**：系統跳出詢問時按「允許」。之後選單列的橘色麥克風燈號會一直亮著，這是正常的——小小要一直聽才聽得到「嘿，小小」。聲音只在這台電腦上處理，不存檔、不上傳。
2. **選收音的麥克風**：建議用內建或 USB 麥克風。藍牙耳機會顯示成灰色，因為拿來收音會讓耳機變成通話音質。
3. **小小的聲音**：需要系統的「美佳」語音。沒有的話照畫面指示到系統設定下載。
4. **連上 Claude**：檢查 Claude Code 已安裝並登入。
5. **試喊一次**：用平常的音量說「嘿，小小」。

第 1 步與第 4 步通過後才能按「開始待命」。

## 怎麼叫小小

- 說「嘿，小小」，聽到「我在」後說出問題。
- 或按快捷鍵（預設 Control+Option+Space），或點選單列的小小圖示。
- 開會、通話、別的 app 在用麥克風、螢幕鎖定時，小小會自動暫停。

## 移除

1. 在選單列點小小 →「結束小小」。
2. 把「應用程式」裡的 xiaoxiao 丟到垃圾桶。
3. 刪除設定、模型與紀錄（要保留設定就跳過）：
   ```bash
   rm -rf ~/Library/Application\ Support/io.github.kerryhuang.xiaoxiao
   rm -rf ~/Library/Logs/io.github.kerryhuang.xiaoxiao
   ```
4. 開機自動啟動：到「系統設定 → 一般 → 登入項目」把 xiaoxiao 移除（若還在）。
5. 麥克風權限：「系統設定 → 隱私權與安全性 → 麥克風」裡的 xiaoxiao 可以一併移除。

你的筆記檔（預設 `~/Documents/小小筆記.md`）不會被刪除。

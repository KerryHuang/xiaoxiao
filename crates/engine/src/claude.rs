//! Claude CLI：尋找、常駐快答、單次查詢。
//! 只呼叫使用者自己安裝並登入的官方 CLI；不碰登入流程與 token。
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::prompts::lookup_prompt;

pub const IDLE_RESET: Duration = Duration::from_secs(5 * 60);
pub const LOOKUP_TIMEOUT: Duration = Duration::from_secs(45);
/// 快答等第一段文字的上限、整個回答的上限；超過就砍掉程序回 Timeout，引擎不會卡住。
pub const FIRST_TOKEN_TIMEOUT: Duration = Duration::from_secs(8);
pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(60);

/// 查資料讀本機檔案時一律排除。
const SECRET_PATTERNS: &[&str] = &[
    ".env*",
    ".ssh/**",
    "*.pem",
    "*.key",
    "id_*",
    "Library/Keychains/**",
    "Library/Application Support/Google/Chrome/**",
    "Library/Application Support/Firefox/**",
    "Library/Application Support/Microsoft Edge/**",
    "Library/Application Support/BraveSoftware/**",
    "Library/Safari/**",
    "AppData/Local/Google/Chrome/**",
    "AppData/Local/Microsoft/Edge/**",
    "AppData/Local/BraveSoftware/**",
    "AppData/Roaming/Mozilla/Firefox/**",
    "AppData/Local/Microsoft/Credentials/**",
    "AppData/Roaming/Microsoft/Credentials/**",
    "AppData/Roaming/Microsoft/Protect/**",
];

const LOCKDOWN: [&str; 4] = ["--setting-sources", "user", "--permission-mode", "dontAsk"];

#[derive(Debug, Clone)]
pub struct ClaudeCfg {
    pub bin: PathBuf,
    pub workdir: PathBuf,
    pub read_dirs: Vec<PathBuf>,
    /// 使用者所在地，查天氣這類跟地點有關的事要用；空字串代表沒設定。
    pub location: String,
}

#[derive(Debug)]
pub enum ClaudeError {
    NotFound,
    NotLoggedIn,
    Crashed,
    Timeout,
    Garbage,
}

impl fmt::Display for ClaudeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ClaudeError::NotFound => "找不到 Claude，請先安裝 Claude Code 或 Claude Desktop。",
            ClaudeError::NotLoggedIn => "Claude 還沒登入，請在終端機執行 claude 並登入。",
            ClaudeError::Crashed => "回答中斷了。",
            ClaudeError::Timeout => "我連不上或查太久了，請檢查網路後再問一次。",
            ClaudeError::Garbage => "Claude 的回應看不懂。",
        })
    }
}

impl std::error::Error for ClaudeError {}

fn parse_version(s: &str) -> Option<Vec<u64>> {
    s.split('.').map(|p| p.parse().ok()).collect()
}

/// 權限規則要用的 POSIX 形式：`C:\Users\me\` → `/c/Users/me`，並去掉結尾斜線。
/// Claude Code 在 Windows 上先把路徑轉成這個形式才比對，寫原始路徑會比對不到。
pub fn posix_abs(dir: &str) -> String {
    let b = dir.as_bytes();
    let drive = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/');
    let s = if drive {
        format!("/{}{}", (b[0] as char).to_ascii_lowercase(), dir[2..].replace('\\', "/"))
    } else {
        dir.to_string()
    };
    s.trim_end_matches('/').to_string()
}

/// 網路路徑（`\\server\share`）。沒驗過 Claude Code 怎麼比對它們，秘密檔規則可能失效，所以不給讀取權。
pub fn is_network_path(dir: &str) -> bool {
    dir.starts_with(r"\\") || dir.starts_with("//")
}

/// Windows 上不要替 claude 開主控台視窗（否則每次回答都會跳出黑視窗）。
/// CREATE_NO_WINDOW｜BELOW_NORMAL_PRIORITY_CLASS。
/// claude 在 Windows 啟動要約 5 秒、很吃 CPU，剛好碰上使用者開口後的聽寫，聽寫會慢一倍以上。
pub const WIN_SPAWN_FLAGS: u32 = 0x0800_0000 | 0x0000_4000;

pub fn quiet(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    cmd.creation_flags(WIN_SPAWN_FLAGS);
    cmd
}

/// PATH 上的 `claude` 優先；否則找 Claude Desktop 內建、版本最大的那份。
/// 不接受 `claude.cmd`：Rust 不能把多行參數傳給批次檔，而系統提示是多行。
pub fn find_claude(path_env: &str, desktop_root: &Path) -> Option<PathBuf> {
    let exe = format!("claude{}", std::env::consts::EXE_SUFFIX);
    if let Some(p) = std::env::split_paths(path_env).map(|d| d.join(&exe)).find(|p| p.is_file()) {
        return Some(p);
    }
    let mut best: Option<(Vec<u64>, PathBuf)> = None;
    for ver in std::fs::read_dir(desktop_root.join("claude-code")).ok()?.flatten() {
        let Some(v) = ver.file_name().to_str().and_then(parse_version) else { continue };
        let Ok(hashes) = std::fs::read_dir(ver.path()) else { continue };
        for h in hashes.flatten() {
            for bin in [h.path().join("claude.app/Contents/MacOS/claude"), h.path().join(&exe)] {
                if bin.is_file() && best.as_ref().is_none_or(|(bv, _)| v > *bv) {
                    best = Some((v.clone(), bin));
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

pub fn session_args(model: &str, system_prompt: &str) -> Vec<String> {
    let mut a: Vec<String> = [
        "-p", "--model", model, "--input-format", "stream-json", "--output-format", "stream-json",
        "--verbose", "--include-partial-messages", "--tools", "",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    a.extend(LOCKDOWN.iter().map(|s| s.to_string()));
    a.push("--strict-mcp-config".into());
    a.push("--system-prompt".into());
    a.push(system_prompt.into());
    a
}

pub fn lookup_args(cfg: &ClaudeCfg, question: &str) -> Vec<String> {
    let mut a: Vec<String> = ["-p", question, "--model", "sonnet", "--output-format", "json", "--append-system-prompt", &lookup_prompt(&cfg.location)]
        .iter()
        .map(|s| s.to_string())
        .collect();
    a.extend(LOCKDOWN.iter().map(|s| s.to_string()));
    a.push("--strict-mcp-config".into());
    let dirs: Vec<String> = cfg.read_dirs.iter().map(|d| d.to_string_lossy().into_owned()).filter(|d| !is_network_path(d)).collect();
    if dirs.is_empty() {
        a.extend(["--tools", "WebSearch", "--allowedTools", "WebSearch"].map(String::from));
    } else {
        let tools = "Read,Grep,Glob,WebSearch";
        a.extend(["--tools", tools, "--allowedTools", tools].map(String::from));
        for d in &dirs {
            a.push("--add-dir".into());
            a.push(d.clone());
        }
        a.push("--disallowedTools".into());
        for p in SECRET_PATTERNS {
            for t in ["Read", "Grep", "Glob"] {
                // 絕對路徑規則（// 開頭）綁在每個加入的資料夾上；**/ 開頭只相對 cwd，比對不到 --add-dir
                for d in &dirs {
                    a.push(format!("{t}(/{}/**/{p})", posix_abs(d)));
                }
            }
        }
    }
    a
}

/// 這一行是不是「來源」標題（WebSearch 會要求在答案後面列來源）。
fn is_sources_heading(line: &str) -> bool {
    let t = line.trim().trim_matches(|c: char| "#*_-: ：".contains(c)).to_lowercase();
    matches!(t.as_str(), "sources" | "source" | "來源" | "參考資料" | "參考來源")
}

/// 去掉 `[文字](網址)` 的網址部分，以及裸網址。
fn strip_links(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('[')
            && let Some((label, tail)) = after.split_once("](")
            && let Some((_, tail)) = tail.split_once(')')
        {
            out.push_str(label);
            rest = tail;
        } else if rest.starts_with("http://") || rest.starts_with("https://") {
            rest = rest.trim_start_matches(|c: char| c.is_ascii_graphic());
        } else {
            let c = rest.chars().next().unwrap();
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// 查資料的結果轉成要唸的文字：拿掉來源段落和網址，語音念網址沒有意義。
pub fn spoken_answer(text: &str) -> String {
    let body: Vec<String> = text.lines().take_while(|l| !is_sources_heading(l)).map(strip_links).collect();
    body.join("\n").trim().to_string()
}

/// result 事件 → 答案或錯誤。
fn result_of(v: &Value) -> Result<String, ClaudeError> {
    let text = v.get("result").and_then(Value::as_str).unwrap_or("");
    if v.get("is_error").and_then(Value::as_bool).unwrap_or(false) {
        let low = text.to_lowercase();
        return Err(if low.contains("login") { ClaudeError::NotLoggedIn } else { ClaudeError::Crashed });
    }
    Ok(text.to_string())
}

fn spawn_err(e: std::io::Error) -> ClaudeError {
    if e.kind() == std::io::ErrorKind::NotFound { ClaudeError::NotFound } else { ClaudeError::Crashed }
}

struct Proc {
    child: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
}

/// 常駐快答程序（stream-json）。閒置超過 5 分鐘或程序死掉時，下一次 ask 重開。
pub struct Session {
    cfg: ClaudeCfg,
    args: Vec<String>,
    proc: Option<Proc>,
    last: Instant,
    first_token: Duration,
    total: Duration,
}

impl Session {
    pub fn new(cfg: ClaudeCfg, model: &str, system_prompt: &str) -> Session {
        Session {
            cfg,
            args: session_args(model, system_prompt),
            proc: None,
            last: Instant::now(),
            first_token: FIRST_TOKEN_TIMEOUT,
            total: ANSWER_TIMEOUT,
        }
    }

    pub fn with_timeouts(mut self, first_token: Duration, total: Duration) -> Session {
        self.first_token = first_token;
        self.total = total;
        self
    }

    pub fn idle_for(&self) -> Duration {
        self.last.elapsed()
    }

    /// 先把程序開好，不送任何訊息；之後的 ask 就不用等它啟動。
    pub fn warm(&mut self) -> Result<(), ClaudeError> {
        self.drop_stale();
        if self.proc.is_none() {
            self.proc = Some(self.spawn()?);
            self.last = Instant::now();
        }
        Ok(())
    }

    /// 閒置太久的換掉開新對話，避免上下文越積越多；等待時死掉的也不留。
    fn drop_stale(&mut self) {
        let dead = self.proc.as_mut().is_some_and(|p| !matches!(p.child.try_wait(), Ok(None)));
        if dead || self.idle_for() > IDLE_RESET {
            self.proc = None;
        }
    }

    fn spawn(&self) -> Result<Proc, ClaudeError> {
        let mut child = quiet(&mut Command::new(&self.cfg.bin))
            .args(&self.args)
            .current_dir(&self.cfg.workdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(spawn_err)?;
        let stdin = child.stdin.take().ok_or(ClaudeError::Crashed)?;
        let lines = BufReader::new(child.stdout.take().ok_or(ClaudeError::Crashed)?).lines();
        Ok(Proc { child, stdin, lines })
    }

    pub async fn ask(&mut self, text: &str, mut on_delta: impl FnMut(&str)) -> Result<String, ClaudeError> {
        self.drop_stale();
        self.last = Instant::now();
        if self.proc.is_none() {
            self.proc = Some(self.spawn()?);
        }
        let (first, total) = (self.first_token, self.total);
        let r = Self::exchange(self.proc.as_mut().unwrap(), text, &mut on_delta, first, total).await;
        if r.is_err() {
            self.proc = None;
        }
        self.last = Instant::now();
        r
    }

    async fn exchange(
        p: &mut Proc,
        text: &str,
        on_delta: &mut impl FnMut(&str),
        first_token: Duration,
        total: Duration,
    ) -> Result<String, ClaudeError> {
        let start = tokio::time::Instant::now();
        let msg = serde_json::json!({"type": "user", "message": {"role": "user", "content": text}});
        let line = format!("{msg}\n");
        p.stdin.write_all(line.as_bytes()).await.map_err(|_| ClaudeError::Crashed)?;
        p.stdin.flush().await.map_err(|_| ClaudeError::Crashed)?;
        let mut acc = String::new();
        loop {
            // 還沒拿到第一段文字時用較短的期限；逾時由呼叫端丟掉程序（kill_on_drop）
            let limit = if acc.is_empty() { first_token.min(total) } else { total };
            let next = tokio::time::timeout_at(start + limit, p.lines.next_line()).await.map_err(|_| ClaudeError::Timeout)?;
            let Some(l) = next.map_err(|_| ClaudeError::Crashed)? else { break };
            let Ok(v) = serde_json::from_str::<Value>(&l) else { continue }; // 非 JSON 行略過
            match v.get("type").and_then(Value::as_str) {
                Some("stream_event") => {
                    let d = &v["event"]["delta"];
                    if d["type"] == "text_delta" {
                        if let Some(t) = d["text"].as_str() {
                            acc.push_str(t);
                            on_delta(t);
                        }
                    }
                }
                Some("result") => {
                    let r = result_of(&v)?;
                    return Ok(if r.is_empty() { acc } else { r });
                }
                _ => {}
            }
        }
        Err(ClaudeError::Crashed) // 還沒給 result 就結束
    }
}

/// 健康檢查：回 CLI 版本號；沒登入回 NotLoggedIn。不花 token、不碰憑證——
/// `auth status --json` 只讀 loggedIn，其他欄位（含 email）一律丟掉。
pub async fn check(cfg: &ClaudeCfg) -> Result<String, ClaudeError> {
    let run = |args: &'static [&'static str]| {
        let mut c = Command::new(&cfg.bin);
        quiet(&mut c).args(args).current_dir(&cfg.workdir).stdin(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true);
        async move {
            let out = tokio::time::timeout(Duration::from_secs(10), c.output())
                .await
                .map_err(|_| ClaudeError::Timeout)?
                .map_err(spawn_err)?;
            Ok::<_, ClaudeError>(String::from_utf8_lossy(&out.stdout).into_owned())
        }
    };
    let ver = run(&["--version"]).await?;
    let ver = ver.split_whitespace().next().ok_or(ClaudeError::Garbage)?.to_string();
    let status: Value = serde_json::from_str(&run(&["auth", "status", "--json"]).await?).map_err(|_| ClaudeError::Garbage)?;
    if status.get("loggedIn").and_then(Value::as_bool) != Some(true) {
        return Err(ClaudeError::NotLoggedIn);
    }
    Ok(ver)
}

/// 單次查資料（sonnet、可上網），逾時就砍掉子程序。正式值用 LOOKUP_TIMEOUT。
pub async fn lookup(cfg: &ClaudeCfg, q: &str, timeout: Duration) -> Result<String, ClaudeError> {
    let child = quiet(&mut Command::new(&cfg.bin))
        .args(lookup_args(cfg, q))
        .current_dir(&cfg.workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(spawn_err)?;
    let out = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| ClaudeError::Timeout)?
        .map_err(|_| ClaudeError::Crashed)?;
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|_| {
        if out.status.success() { ClaudeError::Garbage } else { ClaudeError::Crashed }
    })?;
    let answer = spoken_answer(&result_of(&v)?);
    if answer.is_empty() { Err(ClaudeError::Garbage) } else { Ok(answer) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 斷網時 claude 只會默默重試，唯一的徵兆是等不到第一段文字；
    /// 實測 Mac 冷啟動到第一段文字約 1.5 秒，Windows 啟動約 5 秒（喚醒時已先開好），所以 8 秒就該放棄。
    #[test]
    fn offline_gives_up_within_eight_seconds() {
        assert!(FIRST_TOKEN_TIMEOUT <= Duration::from_secs(8), "{FIRST_TOKEN_TIMEOUT:?}");
        assert!(FIRST_TOKEN_TIMEOUT >= Duration::from_secs(6), "太短會把 Windows 冷啟動誤判成斷網");
    }
}

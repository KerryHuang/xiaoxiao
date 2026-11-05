use engine::claude::*;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn cfg(read_dirs: Vec<PathBuf>) -> ClaudeCfg {
    ClaudeCfg { bin: PathBuf::from("claude"), workdir: std::env::temp_dir(), read_dirs }
}

/// 每個測試各自包一層 wrapper 設好 FAKE_CLAUDE，避免平行測試互改環境變數。
fn fake(dir: &Path, mode: &str) -> ClaudeCfg {
    let real = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let w = dir.join("claude");
    fs::write(&w, format!("#!/bin/sh\nFAKE_CLAUDE={mode} exec '{}' \"$@\"\n", real.display())).unwrap();
    fs::set_permissions(&w, fs::Permissions::from_mode(0o755)).unwrap();
    ClaudeCfg { bin: w, workdir: dir.to_path_buf(), read_dirs: vec![] }
}

fn mk_desktop(root: &Path, ver: &str, hash: &str) -> PathBuf {
    let d = root.join("claude-code").join(ver).join(hash).join("claude.app/Contents/MacOS");
    fs::create_dir_all(&d).unwrap();
    let b = d.join("claude");
    fs::write(&b, "").unwrap();
    b
}

#[test]
fn finds_newest_desktop_cli() {
    let t = tempfile::tempdir().unwrap();
    mk_desktop(t.path(), "2.1.309", "a");
    let newest = mk_desktop(t.path(), "2.1.317", "b");
    assert_eq!(find_claude("", t.path()), Some(newest.clone()));
    mk_desktop(t.path(), "2.1.40", "c");
    assert_eq!(find_claude("", t.path()), Some(newest));
}

#[test]
fn path_claude_wins_over_desktop() {
    let t = tempfile::tempdir().unwrap();
    mk_desktop(t.path(), "2.1.317", "b");
    let bin = t.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::write(bin.join("claude"), "").unwrap();
    assert_eq!(find_claude(bin.to_str().unwrap(), t.path()), Some(bin.join("claude")));
}

#[test]
fn session_args_lock_down() {
    let a = session_args("haiku", "SP");
    for w in [["--model", "haiku"], ["--tools", ""], ["--setting-sources", "user"], ["--permission-mode", "dontAsk"], ["--system-prompt", "SP"]] {
        assert!(a.windows(2).any(|x| x == w), "{w:?}");
    }
    assert!(a.iter().any(|x| x == "--strict-mcp-config"));
}

#[test]
fn lookup_args_no_dirs_web_only() {
    let a = lookup_args(&cfg(vec![]), "q");
    assert!(a.windows(2).any(|w| w == ["--tools", "WebSearch"]));
    assert!(a.windows(2).any(|w| w == ["--model", "sonnet"]));
    assert!(!a.iter().any(|x| x == "--add-dir"));
}

#[test]
fn lookup_args_with_dir_excludes_secrets() {
    let a = lookup_args(&cfg(vec!["/tmp/docs".into()]), "q");
    assert!(a.windows(2).any(|w| w == ["--add-dir", "/tmp/docs"]));
    assert!(a.windows(2).any(|w| w == ["--tools", "Read,Grep,Glob,WebSearch"]));
    assert!(a.iter().any(|x| x.contains("Read(**/.env*)")));
    assert!(a.iter().any(|x| x.contains("Grep(**/.ssh/")));
    assert!(a.iter().any(|x| x.contains("Glob(**/Library/Keychains/")));
}

#[tokio::test]
async fn session_streams_answer() {
    let t = tempfile::tempdir().unwrap();
    let mut s = Session::new(fake(t.path(), "ok"), "haiku", "SP");
    let mut n = 0;
    let r = s.ask("一公斤幾公克", |_| n += 1).await.unwrap();
    assert_eq!(r, "一公斤是1000 公克。");
    assert_eq!(n, 2);
    // 同一個常駐程序可以接著問
    assert_eq!(s.ask("再一次", |_| {}).await.unwrap(), "一公斤是1000 公克。");
}

#[tokio::test]
async fn session_survives_garbage_line() {
    let t = tempfile::tempdir().unwrap();
    let mut s = Session::new(fake(t.path(), "garbage"), "haiku", "SP");
    assert_eq!(s.ask("q", |_| {}).await.unwrap(), "一公斤是1000 公克。");
}

#[tokio::test]
async fn session_reports_crash_mid_answer() {
    let t = tempfile::tempdir().unwrap();
    let mut s = Session::new(fake(t.path(), "crash"), "haiku", "SP");
    assert!(matches!(s.ask("q", |_| {}).await, Err(ClaudeError::Crashed)));
    // 下一次 ask 自動重開，同樣是 crash 模式，所以會再拿到一個 delta 後 Crashed
    let mut n = 0;
    assert!(matches!(s.ask("q", |_| n += 1).await, Err(ClaudeError::Crashed)));
    assert_eq!(n, 1, "第二次有重開程序並收到 delta");
}

#[tokio::test]
async fn session_detects_not_logged_in() {
    let t = tempfile::tempdir().unwrap();
    let mut s = Session::new(fake(t.path(), "nologin"), "haiku", "SP");
    assert!(matches!(s.ask("q", |_| {}).await, Err(ClaudeError::NotLoggedIn)));
}

#[tokio::test]
async fn session_missing_binary_is_not_found() {
    let t = tempfile::tempdir().unwrap();
    let c = ClaudeCfg { bin: t.path().join("nope"), workdir: t.path().into(), read_dirs: vec![] };
    let mut s = Session::new(c, "haiku", "SP");
    assert!(matches!(s.ask("q", |_| {}).await, Err(ClaudeError::NotFound)));
}

#[tokio::test]
async fn lookup_times_out() {
    let t = tempfile::tempdir().unwrap();
    let t0 = Instant::now();
    let r = lookup(&fake(t.path(), "slow"), "q", Duration::from_secs(1)).await;
    assert!(matches!(r, Err(ClaudeError::Timeout)));
    assert!(t0.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn lookup_returns_result() {
    let t = tempfile::tempdir().unwrap();
    let mut c = fake(t.path(), "args");
    let out = t.path().join("args.txt");
    let w = fs::read_to_string(&c.bin).unwrap().replace("exec", &format!("FAKE_ARGS_OUT='{}' exec", out.display()));
    fs::write(&c.bin, w).unwrap();
    c.read_dirs = vec![];
    assert_eq!(lookup(&c, "今天新聞", LOOKUP_TIMEOUT).await.unwrap(), "ok");
    let args = fs::read_to_string(out).unwrap();
    assert!(args.lines().any(|l| l == "今天新聞"));
}

#[test]
fn errors_speak_chinese() {
    assert!(ClaudeError::NotLoggedIn.to_string().contains("登入"));
    assert!(ClaudeError::Crashed.to_string().contains("中斷"));
}

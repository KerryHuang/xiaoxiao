use engine::claude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn cfg(read_dirs: Vec<PathBuf>) -> ClaudeCfg {
    ClaudeCfg { bin: PathBuf::from("claude"), workdir: std::env::temp_dir(), read_dirs, location: String::new() }
}

/// 每個測試各自一份假 claude，模式寫在執行檔旁邊，避免平行測試互改環境變數。
fn fake_with_out(dir: &Path, mode: &str, out: &Path) -> ClaudeCfg {
    let w = dir.join(format!("claude{}", std::env::consts::EXE_SUFFIX));
    // 硬連結：每次複製新檔案，macOS 可能卡在驗證新執行檔。跨磁碟才複製。
    fs::hard_link(env!("CARGO_BIN_EXE_fake-claude"), &w)
        .or_else(|_| fs::copy(env!("CARGO_BIN_EXE_fake-claude"), &w).map(|_| ()))
        .unwrap();
    fs::write(dir.join("fake-mode.txt"), format!("{mode}\n{}\n", out.display())).unwrap();
    ClaudeCfg { bin: w, workdir: dir.to_path_buf(), read_dirs: vec![], location: String::new() }
}

fn fake(dir: &Path, mode: &str) -> ClaudeCfg {
    fake_with_out(dir, mode, &dir.join("claude-args.txt"))
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
    let exe = bin.join(format!("claude{}", std::env::consts::EXE_SUFFIX));
    fs::write(&exe, "").unwrap();
    assert_eq!(find_claude(bin.to_str().unwrap(), t.path()), Some(exe));
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

fn system_prompt(a: &[String]) -> String {
    let i = a.iter().position(|x| x == "--append-system-prompt").unwrap();
    a[i + 1].clone()
}

#[test]
fn lookup_knows_where_the_user_lives() {
    let c = ClaudeCfg { location: "高雄".into(), ..cfg(vec![]) };
    let p = system_prompt(&lookup_args(&c, "今天天氣如何"));
    assert!(p.contains("使用者住在高雄"), "{p}");
}

#[test]
fn lookup_without_location_mentions_no_place() {
    let c = ClaudeCfg { location: "  ".into(), ..cfg(vec![]) };
    assert!(!system_prompt(&lookup_args(&c, "q")).contains("使用者住在"));
}

#[test]
fn lookup_args_with_dir_excludes_secrets() {
    let a = lookup_args(&cfg(vec!["/tmp/docs".into()]), "q");
    assert!(a.windows(2).any(|w| w == ["--add-dir", "/tmp/docs"]));
    assert!(a.windows(2).any(|w| w == ["--tools", "Read,Grep,Glob,WebSearch"]));
    // 規則要用絕對路徑（// 開頭）綁在加入的資料夾上；**/ 開頭只相對 cwd，比對不到 --add-dir
    assert!(a.iter().any(|x| x == "Read(//tmp/docs/**/.env*)"), "{a:?}");
    assert!(a.iter().any(|x| x.starts_with("Grep(//tmp/docs/**/.ssh/")));
    assert!(a.iter().any(|x| x.starts_with("Glob(//tmp/docs/**/Library/Keychains/")));
    assert!(!a.iter().any(|x| x.contains("(**/")));
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

fn wait_for_file(p: &Path) {
    let t0 = Instant::now();
    while !p.exists() {
        assert!(t0.elapsed() < Duration::from_secs(5), "等不到 {}", p.display());
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[tokio::test]
async fn warm_starts_the_process_before_any_question() {
    let t = tempfile::tempdir().unwrap();
    let mut s = Session::new(fake(t.path(), "args"), "haiku", "SP");
    s.warm().unwrap();
    wait_for_file(&t.path().join("claude-args.txt"));
}

#[tokio::test]
async fn warm_process_that_died_is_replaced_on_ask() {
    let t = tempfile::tempdir().unwrap();
    let marker = t.path().join("died");
    let mut s = Session::new(fake_with_out(t.path(), "die-once", &marker), "haiku", "SP");
    s.warm().unwrap();
    wait_for_file(&marker);
    std::thread::sleep(Duration::from_millis(200)); // 等它真的結束
    assert_eq!(s.ask("q", |_| {}).await.unwrap(), "一公斤是1000 公克。");
}

#[tokio::test]
async fn warm_with_missing_binary_is_not_found() {
    let t = tempfile::tempdir().unwrap();
    let c = ClaudeCfg { bin: t.path().join("nope"), workdir: t.path().into(), read_dirs: vec![], location: String::new() };
    assert!(matches!(Session::new(c, "haiku", "SP").warm(), Err(ClaudeError::NotFound)));
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
    let c = ClaudeCfg { bin: t.path().join("nope"), workdir: t.path().into(), read_dirs: vec![], location: String::new() };
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

#[test]
fn secret_rules_use_posix_form_for_windows_paths() {
    let a = lookup_args(&cfg(vec![r"C:\Users\me\docs\".into()]), "q");
    assert!(a.iter().any(|x| x == "Read(//c/Users/me/docs/**/.env*)"), "{a:?}");
    assert!(a.iter().any(|x| x.starts_with("Glob(//c/Users/me/docs/**/AppData/Local/Google/Chrome/")));
    assert!(a.windows(2).any(|w| w == ["--add-dir", r"C:\Users\me\docs\"]));
}

#[test]
fn network_folders_get_no_read_access() {
    // 規則比對不到網路路徑：給了讀取權，.env 就擋不住，所以寧可不給
    for unc in [r"\\server\share\docs", "//server/share/docs"] {
        let a = lookup_args(&cfg(vec![unc.into()]), "q");
        assert!(!a.iter().any(|x| x == "--add-dir"), "{a:?}");
        assert!(!a.iter().any(|x| x.starts_with("Read")), "{a:?}");
        assert!(a.windows(2).any(|w| w == ["--tools", "WebSearch"]), "{a:?}");
    }
    // 本機資料夾不受影響
    let a = lookup_args(&cfg(vec![r"\\server\share".into(), r"C:\Users\me\docs".into()]), "q");
    assert!(a.windows(2).any(|w| w == ["--add-dir", r"C:\Users\me\docs"]), "{a:?}");
    assert_eq!(a.iter().filter(|x| *x == "--add-dir").count(), 1, "{a:?}");
    assert!(!a.iter().any(|x| x.contains("server")), "{a:?}");
}

#[test]
fn posix_abs_keeps_unix_paths() {
    assert_eq!(posix_abs("/tmp/docs/"), "/tmp/docs");
    assert_eq!(posix_abs(r"D:\a b\c"), "/d/a b/c");
    assert_eq!(posix_abs(r"D:\"), "/d");
}

#[test]
fn finds_exe_on_path() {
    let t = tempfile::tempdir().unwrap();
    let exe = t.path().join(format!("claude{}", std::env::consts::EXE_SUFFIX));
    fs::write(&exe, "").unwrap();
    assert_eq!(find_claude(t.path().to_str().unwrap(), &t.path().join("none")), Some(exe));
}

#[test]
fn finds_newest_desktop_cli_flat_layout() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path().join("claude-code/2.1.317/h");
    fs::create_dir_all(&d).unwrap();
    let exe = d.join(format!("claude{}", std::env::consts::EXE_SUFFIX));
    fs::write(&exe, "").unwrap();
    assert_eq!(find_claude("", t.path()), Some(exe));
}

#[tokio::test]
async fn lookup_returns_result() {
    let t = tempfile::tempdir().unwrap();
    let out = t.path().join("args.txt");
    let c = fake_with_out(t.path(), "args", &out);
    assert_eq!(lookup(&c, "今天新聞", LOOKUP_TIMEOUT).await.unwrap(), "ok");
    let args = fs::read_to_string(out).unwrap();
    assert!(args.lines().any(|l| l == "今天新聞"));
}

#[test]
fn errors_speak_chinese() {
    assert!(ClaudeError::NotLoggedIn.to_string().contains("登入"));
    assert!(ClaudeError::Crashed.to_string().contains("中斷"));
}

#[tokio::test]
async fn check_reports_version_when_logged_in() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(check(&fake(t.path(), "ok")).await.unwrap(), "2.1.999");
}

#[tokio::test]
async fn check_detects_not_logged_in() {
    let t = tempfile::tempdir().unwrap();
    assert!(matches!(check(&fake(t.path(), "nologin")).await, Err(ClaudeError::NotLoggedIn)));
}

#[tokio::test]
async fn check_missing_binary() {
    let t = tempfile::tempdir().unwrap();
    let c = ClaudeCfg { bin: t.path().join("nope"), workdir: t.path().into(), read_dirs: vec![], location: String::new() };
    assert!(matches!(check(&c).await, Err(ClaudeError::NotFound)));
}

#[tokio::test]
async fn session_times_out_waiting_first_token() {
    let t = tempfile::tempdir().unwrap();
    let mut s = Session::new(fake(t.path(), "slow"), "haiku", "SP").with_timeouts(Duration::from_millis(500), Duration::from_secs(5));
    let t0 = Instant::now();
    assert!(matches!(s.ask("q", |_| {}).await, Err(ClaudeError::Timeout)));
    assert!(t0.elapsed() < Duration::from_secs(3));
}

#[test]
fn spoken_answer_drops_sources_section() {
    let raw = "今天北部轉涼有雨，出門記得帶傘。\n\nSources:\n- [11/6天氣預報 (NOWnews)](https://www.nownews.com/news/6889701)\n- [東北季風 (NewTalk)](https://newtalk.tw/news/1)";
    assert_eq!(spoken_answer(raw), "今天北部轉涼有雨，出門記得帶傘。");
}

#[test]
fn spoken_answer_handles_other_source_headings() {
    for heading in ["**Sources:**", "來源：", "## 參考資料", "sources"] {
        let raw = format!("答案。\n{heading}\n- https://example.com/a");
        assert_eq!(spoken_answer(&raw), "答案。", "{heading}");
    }
}

#[test]
fn spoken_answer_removes_inline_links_but_keeps_words() {
    let raw = "根據[氣象署](https://cwa.gov.tw/x)，今天有雨，詳見 https://example.com/a?b=1。明天放晴。";
    assert_eq!(spoken_answer(raw), "根據氣象署，今天有雨，詳見 。明天放晴。");
}

#[test]
fn spoken_answer_keeps_ordinary_text_about_sources() {
    let raw = "來源可靠的說法是今天有雨。\n明天會放晴。";
    assert_eq!(spoken_answer(raw), raw);
}

#[test]
fn spoken_answer_of_only_sources_is_empty() {
    assert_eq!(spoken_answer("Sources:\n- https://example.com/a"), "");
}

#[test]
fn windows_claude_runs_below_normal_priority() {
    // 啟動時很吃 CPU，會跟聽寫搶核心
    assert_eq!(WIN_SPAWN_FLAGS & 0x0000_4000, 0x0000_4000, "BELOW_NORMAL_PRIORITY_CLASS");
    assert_eq!(WIN_SPAWN_FLAGS & 0x0800_0000, 0x0800_0000, "CREATE_NO_WINDOW");
}

#[tokio::test]
async fn needs_spawn_tracks_the_process() {
    let t = tempfile::tempdir().unwrap();
    let mut s = Session::new(fake(t.path(), "crash"), "sonnet", "SP");
    assert!(s.needs_spawn());
    s.warm().unwrap();
    assert!(!s.needs_spawn());
    assert!(s.ask("q", |_| {}).await.is_err());
    assert!(s.needs_spawn(), "失敗會丟掉程序，下一問要補前情");
}

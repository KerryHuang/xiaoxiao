//! 測試用假 claude：行為由執行檔旁的 fake-mode.txt 決定（第一行模式、第二行參數輸出路徑）。
use std::io::BufRead;
use std::path::PathBuf;
use std::time::Duration;

fn delta(s: &str) {
    println!(r#"{{"type":"stream_event","event":{{"delta":{{"type":"text_delta","text":"{s}"}}}}}}"#);
}

fn result(s: &str) {
    println!(r#"{{"type":"result","is_error":false,"result":"{s}"}}"#);
}

/// 每讀到一行輸入就回一次；stdin 關閉時結束。
fn each_line(mut f: impl FnMut(usize)) {
    for (i, line) in std::io::stdin().lock().lines().enumerate() {
        if line.is_err() {
            return;
        }
        f(i + 1);
    }
}

fn read_one() {
    let mut s = String::new();
    let _ = std::io::stdin().lock().read_line(&mut s);
}

fn mode() -> (String, Option<PathBuf>) {
    let exe = std::env::current_exe().expect("找不到自己的路徑");
    let txt = std::fs::read_to_string(exe.with_file_name("fake-mode.txt")).expect("缺 fake-mode.txt");
    let mut lines = txt.lines();
    (lines.next().unwrap_or("").to_string(), lines.next().map(PathBuf::from))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mode, out) = mode();
    // 健康檢查：--version 與 auth status 不看模式（nologin 除外）
    match args.first().map(String::as_str) {
        Some("--version") => return println!("2.1.999 (Claude Code)"),
        Some("auth") if mode == "nologin" => return println!(r#"{{"loggedIn":false}}"#),
        Some("auth") => return println!(r#"{{"loggedIn":true,"email":"x@example.com"}}"#),
        _ => {}
    }
    match mode.as_str() {
        "ok" => each_line(|_| {
            delta("一公斤是");
            delta("1000 公克。");
            result("一公斤是1000 公克。");
        }),
        "garbage" => each_line(|_| {
            println!("not json");
            delta("一公斤是");
            delta("1000 公克。");
            result("一公斤是1000 公克。");
        }),
        "crash" => {
            read_one();
            delta("一公斤");
            std::process::exit(1);
        }
        "nologin" => {
            read_one();
            println!(r#"{{"type":"result","is_error":true,"result":"Please run /login"}}"#);
        }
        "action" | "action-repeat" => {
            // 快答回 <<ACTION>>；確認對話（系統提示含「幫使用者做事」）第一輪複述、之後回 <<DO>>
            if args.iter().any(|a| a.contains("幫使用者做事")) {
                let repeat = if mode == "action-repeat" { r#",\"repeat\":\"每天\""# } else { "" };
                each_line(|n| {
                    if n == 1 {
                        result("好，明天早上九點提醒你開會，對嗎？");
                    } else {
                        result(&format!(
                            r#"<<DO>>{{\"type\":\"reminder\",\"at\":\"2099-01-01T09:00:00+08:00\",\"text\":\"開會\"{repeat}}}"#
                        ));
                    }
                });
            } else {
                each_line(|_| {
                    delta("<<ACTION>>");
                    result("<<ACTION>>");
                });
            }
        }
        "slow" => std::thread::sleep(Duration::from_secs(40)),
        "args" => {
            std::fs::write(out.expect("args 模式要有輸出路徑"), args.join("\n") + "\n").unwrap();
            result("ok");
        }
        m => panic!("不認識的模式 {m}"),
    }
}

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
        "action" | "action-repeat" | "action-lies" => {
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
            } else if mode == "action-lies" {
                // 快答沒照規矩回 <<ACTION>>，直接謊稱做好了
                each_line(|_| {
                    delta("好，已經幫你設好提醒了。");
                    result("好，已經幫你設好提醒了。");
                });
            } else {
                each_line(|_| {
                    delta("<<ACTION>>");
                    result("<<ACTION>>");
                });
            }
        }
        // 真的收到問題才留紀錄；只有啟動不算
        "asked" => {
            let marker = out.expect("asked 模式要有輸出路徑");
            each_line(|_| {
                std::fs::write(&marker, "").unwrap();
                result("一公斤是1000 公克。");
            });
        }
        // 確認第一輪複述，第二輪判定偏題；快答一律直接答
        "action-other" => {
            if args.iter().any(|a| a.contains("幫使用者做事")) {
                each_line(|n| if n == 1 { result("好，明天九點提醒你開會，對嗎？") } else { result("<<OTHER>>") });
            } else {
                // 「提醒我…」由規則直接送確認對話，快答只會收到偏題後重問的那句
                each_line(|_| {
                    delta("一公斤是1000 公克。");
                    result("一公斤是1000 公克。");
                });
            }
        }
        // 快答永遠說要做事、確認永遠說沒事可做：不能來回打轉
        "action-loop" => {
            if args.iter().any(|a| a.contains("幫使用者做事")) {
                std::fs::write(out.expect("action-loop 模式要有輸出路徑"), "").unwrap();
                each_line(|_| result("<<OTHER>>"));
            } else {
                each_line(|_| {
                    delta("<<ACTION>>");
                    result("<<ACTION>>");
                });
            }
        }
        // 快答轉查資料，查資料的結果像真的 WebSearch 一樣附來源
        "lookup-sources" => {
            if args.iter().any(|a| a == "json") {
                result(r"今天有雨，記得帶傘。\n\nSources:\n- [天氣](https://example.com/a)");
            } else {
                each_line(|_| {
                    delta("<<LOOKUP>>");
                    result("<<LOOKUP>>");
                });
            }
        }
        // 每次啟動記一行，用來數開了幾個程序
        "spawns" => {
            let path = out.expect("spawns 模式要有輸出路徑");
            let old = std::fs::read_to_string(&path).unwrap_or_default();
            std::fs::write(&path, format!("{old}spawn {}\n", std::process::id())).unwrap();
            each_line(|_| result("好。"));
        }
        // 把收到的每則訊息逐行記下來
        "record" => {
            let path = out.expect("record 模式要有輸出路徑");
            for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                let old = std::fs::read_to_string(&path).unwrap_or_default();
                std::fs::write(&path, format!("{old}{line}\n")).unwrap();
                result("好。");
            }
        }
        // 顧問程序（系統提示含「陪使用者討論工作」）：記下啟動時的模型與每則訊息，依內容回覆；快答 warm 照 ok
        "advise" => {
            if args.iter().any(|a| a.contains("陪使用者討論工作")) {
                let path = out.expect("advise 模式要有輸出路徑");
                let model = args.iter().position(|a| a == "--model").and_then(|i| args.get(i + 1)).cloned().unwrap_or_default();
                let spawns = path.with_extension("spawns");
                let old = std::fs::read_to_string(&spawns).unwrap_or_default();
                std::fs::write(&spawns, format!("{old}spawn {model}\n")).unwrap();
                for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                    let old = std::fs::read_to_string(&path).unwrap_or_default();
                    std::fs::write(&path, format!("{old}{line}\n")).unwrap();
                    if line.contains("[整理重點]") {
                        result("<<SAY>>我們談了新功能排程。\\n<<TOPIC>>新功能排程\\n<<NOTES>>\\n- 先做登入\\n- 再做匯出");
                    } else if line.contains("電視") {
                        delta("<<SK");
                        delta("IP>>");
                        result("<<SKIP>>");
                    } else {
                        delta("**這個想法不錯**，");
                        delta("你覺得先做哪個？");
                        result("**這個想法不錯**，你覺得先做哪個？");
                    }
                }
            } else {
                each_line(|_| {
                    delta("一公斤是");
                    delta("1000 公克。");
                    result("一公斤是1000 公克。");
                });
            }
        }
        // 第一次一啟動就死掉，之後正常回答：模擬預先開好的程序在等待時死掉
        "die-once" => {
            let marker = out.expect("die-once 模式要有輸出路徑");
            if !marker.exists() {
                std::fs::write(marker, "").unwrap();
                std::process::exit(1);
            }
            each_line(|_| result("一公斤是1000 公克。"));
        }
        "slow" => std::thread::sleep(Duration::from_secs(40)),
        "args" => {
            std::fs::write(out.expect("args 模式要有輸出路徑"), args.join("\n") + "\n").unwrap();
            result("ok");
        }
        m => panic!("不認識的模式 {m}"),
    }
}

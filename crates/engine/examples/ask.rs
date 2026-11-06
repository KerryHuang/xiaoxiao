//! 手動冒煙：用本機真的 claude 問一題，印出首字延遲。
//! cargo run -p engine --example ask -- "一公斤幾公克"
use engine::claude::{ClaudeCfg, Session, find_claude};
use std::path::PathBuf;
use std::time::Instant;

#[tokio::main]
async fn main() {
    let q = std::env::args().nth(1).unwrap_or_else(|| "一公斤幾公克".into());
    let home = PathBuf::from(std::env::var("HOME").expect("HOME"));
    let bin = find_claude(&std::env::var("PATH").unwrap_or_default(), &home.join("Library/Application Support/Claude"))
        .expect("找不到 claude");
    let workdir = std::env::temp_dir().join("xiaoxiao-workdir");
    std::fs::create_dir_all(&workdir).unwrap();
    let mut s = Session::new(ClaudeCfg { bin, workdir, read_dirs: vec![], location: String::new() }, "haiku", engine::prompts::quick_prompt());
    let t0 = Instant::now();
    let mut first = None;
    let r = s.ask(&q, |_| { first.get_or_insert(t0.elapsed()); }).await;
    println!("首字 {:?}，全部 {:?}", first, t0.elapsed());
    println!("{r:?}");
}

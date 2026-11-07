//! 模型下載：缺檔或 SHA256 不符就重新下載；先下載到 .part，驗過再改名。
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc::{RecvTimeoutError, sync_channel};
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub files: Vec<ModelFile>,
}

#[derive(Debug, Deserialize)]
pub struct ModelFile {
    pub name: String,
    pub url: String,
    pub sha256: String,
    pub save_as: Option<String>,
    pub extract_to: Option<String>,
    #[serde(default)]
    pub dev_only: bool,
    /// 只有這個平台要下載（`std::env::consts::OS` 的值）；沒寫就是都要。
    pub platform: Option<String>,
}

/// 內建的模型清單（與 scripts/fetch-models.sh 共用）。
pub fn builtin() -> Manifest {
    serde_json::from_str(include_str!("../../models/manifest.json")).expect("models/manifest.json 格式錯誤")
}

/// 缺檔或雜湊不符就下載；`progress(已下載, 總大小)` 逐檔回報（總大小未知時為 0）。
pub fn ensure(dir: &Path, manifest: &Manifest, progress: impl Fn(u64, u64)) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("建立模型資料夾失敗：{e}"))?;
    for f in manifest.files.iter().filter(|f| !f.dev_only && f.platform.as_deref().is_none_or(|p| p == std::env::consts::OS)) {
        // 驗過的檔案留一個標記（雜湊＋大小），之後啟動不用再算 1.9 GB 的雜湊
        let marker = dir.join(format!(".{}.verified", f.name));
        match (&f.save_as, &f.extract_to) {
            (Some(name), _) => {
                let dest = dir.join(name);
                if verified(&marker, f, &dest) {
                    continue;
                }
                let part = dir.join(format!("{name}.part"));
                fetch_verified(f, &part, &progress)?;
                fs::rename(&part, &dest).map_err(|e| e.to_string())?;
                mark(&marker, f, &dest)?;
            }
            (None, Some(sub)) => {
                let target = dir.join(sub);
                if target.is_dir() && fs::read_to_string(&marker).is_ok_and(|m| m.trim() == f.sha256) {
                    continue;
                }
                let part = dir.join(format!("{}.archive.part", f.name));
                fetch_verified(f, &part, &progress)?;
                let tmp = dir.join(format!("{sub}.extracting"));
                let _ = fs::remove_dir_all(&tmp);
                fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
                let ok = tar_command()
                    .arg("-xjf")
                    .arg(&part)
                    .args(["--strip-components", "1", "-C"])
                    .arg(&tmp)
                    .status()
                    .map_err(|e| format!("無法執行 tar：{e}"))?
                    .success();
                let _ = fs::remove_file(&part);
                if !ok {
                    let _ = fs::remove_dir_all(&tmp);
                    return Err(format!("{} 解壓縮失敗", f.name));
                }
                let _ = fs::remove_dir_all(&target);
                fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
                fs::write(&marker, &f.sha256).map_err(|e| e.to_string())?;
            }
            _ => return Err(format!("{} 沒有指定 save_as 或 extract_to", f.name)),
        }
    }
    Ok(())
}

/// 系統內建的 tar。Windows 不能靠 PATH：Git 附的 tar 會把 `C:` 當成遠端主機。
fn tar_bin() -> std::path::PathBuf {
    match std::env::var_os("SystemRoot") {
        Some(root) if cfg!(windows) => Path::new(&root).join("System32").join("tar.exe"),
        _ => "tar".into(),
    }
}

fn tar_command() -> std::process::Command {
    #[allow(unused_mut)]
    let mut c = std::process::Command::new(tar_bin());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW：別跳黑視窗
    }
    c
}

fn hex(d: &[u8]) -> String {
    d.iter().map(|x| format!("{x:02x}")).collect()
}

fn sha_file(p: &Path) -> Option<String> {
    let mut h = Sha256::new();
    let mut r = fs::File::open(p).ok()?;
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = r.read(&mut buf).ok()?;
        if n == 0 {
            return Some(hex(&h.finalize()));
        }
        h.update(&buf[..n]);
    }
}

fn marker_text(f: &ModelFile, dest: &Path) -> Option<String> {
    Some(format!("{} {}", f.sha256, fs::metadata(dest).ok()?.len()))
}

fn verified(marker: &Path, f: &ModelFile, dest: &Path) -> bool {
    if !dest.is_file() {
        return false;
    }
    if fs::read_to_string(marker).ok().as_deref().map(str::trim) == marker_text(f, dest).as_deref() {
        return true;
    }
    // 沒有標記（例如手動放進來的檔案）就算一次雜湊
    sha_file(dest).as_deref() == Some(f.sha256.as_str()) && mark(marker, f, dest).is_ok()
}

fn mark(marker: &Path, f: &ModelFile, dest: &Path) -> Result<(), String> {
    fs::write(marker, marker_text(f, dest).unwrap_or_default()).map_err(|e| e.to_string())
}

/// 下載到 part 並驗 SHA256；不符就刪掉 part 回錯。
fn fetch_verified(f: &ModelFile, part: &Path, progress: &impl Fn(u64, u64)) -> Result<(), String> {
    let got = download(&f.url, part, progress).inspect_err(|_| {
        let _ = fs::remove_file(part);
    })?;
    if got != f.sha256 {
        let _ = fs::remove_file(part);
        return Err(format!("{} 的 SHA256 不符，下載的檔案已刪除", f.name));
    }
    Ok(())
}

/// 下載並邊寫邊算雜湊；支援 https:// 與 file://。
/// 下載時連續這麼久沒收到資料就放棄。網路斷了不一定會報錯，不設限會永遠卡住。
const STALL: Duration = Duration::from_secs(60);

/// 讀到結束，每塊交給 `sink`；超過 `stall` 沒有新資料就回錯誤。
/// 卡住的讀取無法中斷，只能丟下那條執行緒，它會在程式結束時消失。
fn pump(mut reader: Box<dyn Read + Send>, stall: Duration, mut sink: impl FnMut(&[u8]) -> Result<(), String>) -> Result<(), String> {
    let (tx, rx) = sync_channel(4);
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let chunk = match reader.read(&mut buf) {
                Ok(0) => return,
                Ok(n) => Ok(buf[..n].to_vec()),
                Err(e) => Err(e),
            };
            let failed = chunk.is_err();
            if tx.send(chunk).is_err() || failed {
                return;
            }
        }
    });
    loop {
        match rx.recv_timeout(stall) {
            Ok(Ok(b)) => sink(&b)?,
            Ok(Err(e)) => return Err(format!("下載中斷：{e}")),
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
            Err(RecvTimeoutError::Timeout) => return Err(format!("下載卡住了，{} 秒沒有收到資料，請檢查網路後再試一次", stall.as_secs())),
        }
    }
}

fn download(url: &str, part: &Path, progress: &impl Fn(u64, u64)) -> Result<String, String> {
    let (reader, total): (Box<dyn Read + Send>, u64) = if let Some(p) = url.strip_prefix("file://") {
        let f = fs::File::open(p).map_err(|e| format!("讀不到 {p}：{e}"))?;
        let n = f.metadata().map(|m| m.len()).unwrap_or(0);
        (Box::new(f), n)
    } else {
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_connect(Some(STALL)).timeout_recv_response(Some(STALL)).build().into();
        let resp = agent.get(url).call().map_err(|e| format!("下載失敗：{e}"))?;
        let n = resp.body().content_length().unwrap_or(0);
        (Box::new(resp.into_body().into_reader()), n)
    };
    let mut out = fs::File::create(part).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut done = 0u64;
    pump(reader, STALL, |b| {
        out.write_all(b).map_err(|e| e.to_string())?;
        h.update(b);
        done += b.len() as u64;
        progress(done, total);
        Ok(())
    })?;
    out.sync_all().map_err(|e| e.to_string())?;
    Ok(hex(&h.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 給完資料就卡住，跟網路斷了卻沒有錯誤一樣。
    struct Stuck(Option<Vec<u8>>);
    impl Read for Stuck {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if let Some(d) = self.0.take() {
                buf[..d.len()].copy_from_slice(&d);
                return Ok(d.len());
            }
            std::thread::sleep(Duration::from_secs(3600));
            Ok(0)
        }
    }

    #[test]
    fn stalled_download_fails_instead_of_hanging() {
        let mut got = vec![];
        let t0 = std::time::Instant::now();
        let r = pump(Box::new(Stuck(Some(vec![1, 2, 3]))), Duration::from_millis(50), |b| {
            got.extend_from_slice(b);
            Ok(())
        });
        assert!(r.unwrap_err().contains("卡住"));
        assert_eq!(got, [1, 2, 3]);
        assert!(t0.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn pump_reads_to_the_end() {
        let mut got = vec![];
        pump(Box::new(&b"abc"[..]), Duration::from_secs(5), |b| {
            got.extend_from_slice(b);
            Ok(())
        })
        .unwrap();
        assert_eq!(got, b"abc");
    }
    use sha2::{Digest, Sha256};
    use std::cell::Cell;
    use std::fs;
    use std::path::PathBuf;

    fn sha(b: &[u8]) -> String {
        Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
    }

    fn src(dir: &Path, name: &str, body: &[u8]) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, body).unwrap();
        p
    }

    fn file(name: &str, url: &Path, sha256: String, save_as: Option<&str>, extract_to: Option<&str>) -> ModelFile {
        ModelFile {
            name: name.into(),
            url: format!("file://{}", url.display()),
            sha256,
            save_as: save_as.map(Into::into),
            extract_to: extract_to.map(Into::into),
            dev_only: false,
            platform: None,
        }
    }

    #[test]
    fn downloads_then_skips_when_hash_matches() {
        let t = tempfile::tempdir().unwrap();
        let s = src(t.path(), "m.bin", b"model-bytes");
        let out = t.path().join("models");
        let m = Manifest { files: vec![file("m", &s, sha(b"model-bytes"), Some("m.bin"), None)] };
        let calls = Cell::new(0);
        ensure(&out, &m, |_, _| calls.set(calls.get() + 1)).unwrap();
        assert_eq!(fs::read(out.join("m.bin")).unwrap(), b"model-bytes");
        assert!(calls.get() > 0, "下載時要回報進度");
        // 來源刪掉也不影響：已存在且雜湊正確就不重新下載
        fs::remove_file(&s).unwrap();
        ensure(&out, &m, |_, _| {}).unwrap();
        assert!(!out.join("m.bin.part").exists());
    }

    #[test]
    fn wrong_hash_is_deleted_and_errors() {
        let t = tempfile::tempdir().unwrap();
        let s = src(t.path(), "m.bin", b"tampered");
        let out = t.path().join("models");
        let m = Manifest { files: vec![file("m", &s, sha(b"model-bytes"), Some("m.bin"), None)] };
        let e = ensure(&out, &m, |_, _| {}).unwrap_err();
        assert!(e.contains("SHA256"), "{e}");
        assert!(!out.join("m.bin").exists() && !out.join("m.bin.part").exists());
    }

    #[test]
    fn redownloads_when_existing_file_is_corrupt() {
        let t = tempfile::tempdir().unwrap();
        let s = src(t.path(), "m.bin", b"model-bytes");
        let out = t.path().join("models");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("m.bin"), b"half").unwrap();
        let m = Manifest { files: vec![file("m", &s, sha(b"model-bytes"), Some("m.bin"), None)] };
        ensure(&out, &m, |_, _| {}).unwrap();
        assert_eq!(fs::read(out.join("m.bin")).unwrap(), b"model-bytes");
    }

    #[test]
    fn extracts_archive_stripping_top_dir() {
        let t = tempfile::tempdir().unwrap();
        let stage = t.path().join("stage/top");
        fs::create_dir_all(&stage).unwrap();
        fs::write(stage.join("tokens.txt"), "x").unwrap();
        let tar = t.path().join("a.tar.bz2");
        let ok = tar_command()
            .args(["-cjf", tar.to_str().unwrap(), "-C", t.path().join("stage").to_str().unwrap(), "top"])
            .status()
            .unwrap();
        assert!(ok.success());
        let out = t.path().join("models");
        let m = Manifest { files: vec![file("kws", &tar, sha(&fs::read(&tar).unwrap()), None, Some("kws"))] };
        ensure(&out, &m, |_, _| {}).unwrap();
        assert!(out.join("kws/tokens.txt").is_file());
        fs::remove_file(&tar).unwrap();
        ensure(&out, &m, |_, _| {}).unwrap(); // 已解開過就不再下載
    }

    #[test]
    fn dev_only_entries_are_skipped() {
        let t = tempfile::tempdir().unwrap();
        let mut f = file("lib", &t.path().join("missing"), "00".into(), Some("x"), None);
        f.dev_only = true;
        ensure(&t.path().join("models"), &Manifest { files: vec![f] }, |_, _| {}).unwrap();
    }

    #[test]
    fn other_platform_entries_are_skipped() {
        let t = tempfile::tempdir().unwrap();
        let mut f = file("m", &t.path().join("missing"), "00".into(), Some("x"), None);
        f.platform = Some("no-such-os".into());
        ensure(&t.path().join("models"), &Manifest { files: vec![f] }, |_, _| {}).unwrap();
    }

    #[test]
    fn this_platform_entries_are_downloaded() {
        let t = tempfile::tempdir().unwrap();
        let s = src(t.path(), "m.bin", b"model-bytes");
        let out = t.path().join("models");
        let mut f = file("m", &s, sha(b"model-bytes"), Some("m.bin"), None);
        f.platform = Some(std::env::consts::OS.into());
        ensure(&out, &Manifest { files: vec![f] }, |_, _| {}).unwrap();
        assert!(out.join("m.bin").is_file());
    }
}

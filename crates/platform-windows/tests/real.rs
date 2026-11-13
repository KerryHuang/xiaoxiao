//! 真機測試：會開麥克風（只在記憶體）與喇叭。要在桌面的終端機跑，SSH 工作階段拿不到麥克風：
//! `real.exe --ignored --test-threads=1`
#![cfg(windows)]
use engine::audio::BLOCK;
use engine::platform::{AudioSource, Devices, MicUsage, ScreenState, Speaker};
use platform_windows::*;
use std::process::Command;
use std::time::Duration;

/// 測試程式同資料夾（CI 下載的 artifact），或 cargo 的 examples 資料夾。
fn hold_mic() -> std::path::PathBuf {
    let exe = std::env::current_exe().unwrap();
    let here = exe.with_file_name("hold_mic.exe");
    if here.is_file() { here } else { exe.parent().unwrap().parent().unwrap().join("examples/hold_mic.exe") }
}

#[test]
#[ignore]
fn micusage_sees_other_process() {
    let m = WinMicUsage::new();
    assert!(!m.others_using_mic(), "測試前已有程序在用麥克風");
    let mut c = Command::new(hold_mic()).arg("4").spawn().unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    assert!(m.others_using_mic());
    c.wait().unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert!(!m.others_using_mic());
}

#[test]
#[ignore]
fn micusage_ignores_self() {
    let mut a = new_audio();
    a.open(None).unwrap();
    a.read_block().unwrap();
    assert!(!WinMicUsage::new().others_using_mic());
    a.close();
}

#[test]
#[ignore]
fn screen_not_locked_during_test() {
    assert!(!WinScreen.locked());
}

#[test]
#[ignore]
fn permission_is_authorized() {
    assert_eq!(mic_permission(), MicPermission::Authorized);
}

#[test]
#[ignore]
fn audio_reads_16k_blocks() {
    let mut a = new_audio();
    a.open(None).unwrap();
    for _ in 0..5 {
        assert_eq!(a.read_block().unwrap().len(), BLOCK);
    }
    a.close();
}

#[test]
#[ignore]
fn audio_never_picks_bluetooth_by_default() {
    let mut a = new_audio();
    a.open(None).unwrap();
    let name = a.device_name().unwrap().to_string();
    let d = WinDevices.inputs().into_iter().find(|d| d.name == name).unwrap();
    assert!(!d.bluetooth, "{name}");
    a.close();
}

#[test]
#[ignore]
fn devices_list_has_default() {
    let d = WinDevices;
    let def = d.default_input().expect("有預設輸入");
    assert!(d.inputs().iter().any(|i| i.name == def.name));
}

#[test]
#[ignore]
fn default_unchanged_right_after_open() {
    let mut a = new_audio();
    a.open(None).unwrap();
    assert!(!a.default_changed());
    a.close();
}

#[test]
#[ignore]
fn speaker_says_taiwanese() {
    assert!(voice_ok(), "沒有台灣語音");
    let t = std::time::Instant::now();
    WinSpeaker::default().say("測試").unwrap();
    assert!(t.elapsed() > Duration::from_millis(200), "應該說完才回傳");
    WinSpeaker::default().chime();
    std::thread::sleep(Duration::from_millis(800));
}

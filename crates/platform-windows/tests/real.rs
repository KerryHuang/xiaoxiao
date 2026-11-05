//! 真機測試：會開麥克風（只在記憶體）與喇叭。要在桌面的終端機跑，SSH 工作階段拿不到麥克風：
//! `real.exe --ignored --test-threads=1`
#![cfg(windows)]
use engine::audio::BLOCK;
use engine::platform::{AudioSource, Devices};
use platform_windows::*;

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

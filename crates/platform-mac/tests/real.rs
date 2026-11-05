//! 真機測試：會開麥克風（只在記憶體）與喇叭。`cargo test -p platform-mac -- --ignored --test-threads=1`
use engine::audio::BLOCK;
use engine::platform::{AudioSource, Devices, MicUsage, ScreenState, Speaker};
use platform_mac::*;
use std::process::Command;
use std::time::Duration;

#[test]
#[ignore]
fn micusage_sees_other_process() {
    let m = MacMicUsage::new();
    assert!(!m.others_using_mic(), "測試前已有程序在用麥克風");
    // cargo test 會一併編譯 examples：target/<profile>/deps/<test> → target/<profile>/examples/hold_mic
    let exe = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("examples/hold_mic");
    let mut c = Command::new(exe).arg("4").spawn().unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    assert!(m.others_using_mic());
    c.wait().unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert!(!m.others_using_mic());
}

#[test]
#[ignore]
fn micusage_ignores_self() {
    let mut a = MacAudio::new();
    a.open(None).unwrap();
    a.read_block().unwrap();
    assert!(!MacMicUsage::new().others_using_mic());
    a.close();
}

#[test]
#[ignore]
fn audio_reads_16k_blocks() {
    let mut a = MacAudio::new();
    a.open(None).unwrap();
    for _ in 0..5 {
        assert_eq!(a.read_block().unwrap().len(), BLOCK);
    }
    a.close();
}

#[test]
#[ignore]
fn audio_never_picks_bluetooth_by_default() {
    let mut a = MacAudio::new();
    a.open(None).unwrap();
    let name = a.device_name().unwrap().to_string();
    let d = MacDevices.inputs().into_iter().find(|d| d.name == name).unwrap();
    assert!(!d.bluetooth, "{name}");
    a.close();
}

#[test]
#[ignore]
fn devices_list_has_default() {
    let d = MacDevices;
    let def = d.default_input().expect("有預設輸入");
    assert!(d.inputs().iter().any(|i| i.name == def.name));
}

#[test]
#[ignore]
fn screen_not_locked_during_test() {
    assert!(!MacScreen.locked());
}

#[test]
#[ignore]
fn speaker_says_meijia() {
    // 小音量，避免在辦公室吵到人
    MacSpeaker.say("[[volm 0.05]]測試").unwrap();
}

#[test]
#[ignore]
fn default_unchanged_right_after_open() {
    let mut a = MacAudio::new();
    a.open(None).unwrap();
    assert!(!a.default_changed());
    a.close();
}

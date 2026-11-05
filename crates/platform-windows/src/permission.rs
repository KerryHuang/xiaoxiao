//! 麥克風權限狀態：讀「設定 → 隱私權與安全性 → 麥克風」的開關。
use engine::platform::MicPermission;
use windows::Win32::System::Registry::{HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
use windows::core::{HSTRING, w};

use crate::logic::decide;

const CONSENT: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";

fn value(root: HKEY, sub: &str) -> Option<String> {
    let mut buf = [0u16; 32];
    let mut bytes = size_of_val(&buf) as u32;
    let r = unsafe { RegGetValueW(root, &HSTRING::from(sub), w!("Value"), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut bytes)) };
    if r.is_err() {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

pub fn mic_permission() -> MicPermission {
    decide(
        value(HKEY_LOCAL_MACHINE, CONSENT).as_deref(),
        value(HKEY_CURRENT_USER, CONSENT).as_deref(),
        value(HKEY_CURRENT_USER, &format!(r"{CONSENT}\NonPackaged")).as_deref(),
    )
}

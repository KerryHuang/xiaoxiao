//! 偵測別的程序是否正在用麥克風（通話、錄音）。
//! 不用 ConsentStore 的 LastUsedTimeStop：程式當掉會留下「使用中」，小小就永遠暫停。
//! 音訊工作階段 API 有文件、有 PID，程序一結束就消失。
use engine::platform::MicUsage;
use windows::Win32::Foundation::S_OK;
use windows::Win32::Media::Audio::{AudioSessionStateActive, DEVICE_STATE_ACTIVE, IAudioSessionControl2, IAudioSessionManager2, IMMDevice, eCapture};
use windows::Win32::System::Com::CLSCTX_ALL;
use windows::core::{Interface, Result};

use crate::com::Com;
use crate::devices::enumerator;
use crate::logic::counts;

pub struct WinMicUsage {
    me: u32,
}

impl WinMicUsage {
    pub fn new() -> WinMicUsage {
        WinMicUsage { me: std::process::id() }
    }
}

impl Default for WinMicUsage {
    fn default() -> Self {
        Self::new()
    }
}

fn device_in_use(d: &IMMDevice, me: u32) -> Result<bool> {
    unsafe {
        let mgr: IAudioSessionManager2 = d.Activate(CLSCTX_ALL, None)?;
        let sessions = mgr.GetSessionEnumerator()?;
        for i in 0..sessions.GetCount()? {
            let s: IAudioSessionControl2 = sessions.GetSession(i)?.cast()?;
            let active = s.GetState()? == AudioSessionStateActive;
            if counts(active, s.GetProcessId().unwrap_or(0), s.IsSystemSoundsSession() == S_OK, me) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

impl MicUsage for WinMicUsage {
    fn others_using_mic(&self) -> bool {
        let _com = Com::init();
        let Ok(en) = enumerator() else { return false };
        let Ok(list) = (unsafe { en.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) }) else { return false };
        let n = unsafe { list.GetCount() }.unwrap_or(0);
        (0..n).any(|i| unsafe { list.Item(i) }.and_then(|d| device_in_use(&d, self.me)).unwrap_or(false))
    }
}

//! 螢幕是否鎖定。
use engine::platform::ScreenState;
use windows::Win32::System::RemoteDesktop::{
    WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION, WTSFreeMemory, WTSINFOEXW, WTSQuerySessionInformationW, WTSSessionInfoEx,
};
use windows::core::PWSTR;

use crate::logic::locked_flag;

pub struct WinScreen;

impl ScreenState for WinScreen {
    fn locked(&self) -> bool {
        unsafe {
            let mut buf = PWSTR::null();
            let mut n = 0u32;
            if WTSQuerySessionInformationW(Some(WTS_CURRENT_SERVER_HANDLE), WTS_CURRENT_SESSION, WTSSessionInfoEx, &mut buf, &mut n).is_err() {
                return false;
            }
            let info = &*(buf.0 as *const WTSINFOEXW);
            let locked = info.Level == 1 && locked_flag(info.Data.WTSInfoExLevel1.SessionFlags);
            WTSFreeMemory(buf.0 as _);
            locked
        }
    }
}

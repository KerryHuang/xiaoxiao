//! 每個執行緒用 COM 前要先初始化。
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

/// 離開範圍時收尾。Tauri 主執行緒已是 STA，這時 CoInitializeEx 回 RPC_E_CHANGED_MODE：
/// COM 照樣能用，只是不是我們初始化的，不可以 uninit。
pub struct Com(bool);

impl Com {
    pub fn init() -> Com {
        Com(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

//! 麥克風權限狀態（AVCaptureDevice）。權限被拒時 CoreAudio 不報錯、只送全零，
//! 所以引導第 1 步要直接問系統授權狀態，不能拿「開得起來」當作允許。

pub use engine::platform::MicPermission;

pub fn from_code(c: isize) -> MicPermission {
    match c {
        1 => MicPermission::Restricted,
        2 => MicPermission::Denied,
        3 => MicPermission::Authorized,
        _ => MicPermission::NotDetermined,
    }
}

mod objc {
    use std::ffi::{c_char, c_void};

    #[link(name = "AVFoundation", kind = "framework")]
    unsafe extern "C" {
        pub static AVMediaTypeAudio: *const c_void;
    }

    #[link(name = "objc")]
    unsafe extern "C" {
        pub fn objc_getClass(name: *const c_char) -> *const c_void;
        pub fn sel_registerName(name: *const c_char) -> *const c_void;
        pub fn objc_msgSend();
    }
}

/// `[AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio]`。
pub fn mic_permission() -> MicPermission {
    type Send = unsafe extern "C" fn(*const std::ffi::c_void, *const std::ffi::c_void, *const std::ffi::c_void) -> isize;
    unsafe {
        let cls = objc::objc_getClass(c"AVCaptureDevice".as_ptr());
        if cls.is_null() {
            return MicPermission::NotDetermined;
        }
        let sel = objc::sel_registerName(c"authorizationStatusForMediaType:".as_ptr());
        // arm64 上 objc_msgSend 要照實際簽名轉型後呼叫
        let send: Send = std::mem::transmute(objc::objc_msgSend as *const ());
        from_code(send(cls, sel, objc::AVMediaTypeAudio))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_code_mapping() {
        assert_eq!(from_code(0), MicPermission::NotDetermined);
        assert_eq!(from_code(1), MicPermission::Restricted);
        assert_eq!(from_code(2), MicPermission::Denied);
        assert_eq!(from_code(3), MicPermission::Authorized);
        assert_eq!(from_code(99), MicPermission::NotDetermined);
    }

    #[test]
    fn mic_permission_does_not_crash() {
        // 真的呼叫系統；結果依這台機器的授權而定，只確認 FFI 能正常回傳
        let _ = mic_permission();
    }
}

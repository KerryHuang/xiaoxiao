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

/// 跳出系統詢問並等使用者回應（已回應過就立刻回傳）。不開麥克風，所以不會動到 iPhone 等裝置。
/// 用這支而不是「開麥克風＋輪詢狀態」：開麥克風觸發的詢問，同一個行程內狀態可能一直停在未決定。
pub fn request_mic_permission() -> MicPermission {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicU8, Ordering};

    const PENDING: u8 = 0;
    const GRANTED: u8 = 1;
    const REFUSED: u8 = 2;
    static ANSWER: AtomicU8 = AtomicU8::new(PENDING);

    // Objective-C 全域 block 的記憶體配置；沒有捕捉變數，結果放在 ANSWER
    #[repr(C)]
    struct Descriptor {
        reserved: usize,
        size: usize,
    }
    #[repr(C)]
    struct Block {
        isa: *const c_void,
        flags: i32,
        reserved: i32,
        invoke: unsafe extern "C" fn(*const Block, bool),
        descriptor: *const Descriptor,
    }
    unsafe impl Sync for Block {}
    unsafe extern "C" fn answered(_: *const Block, granted: bool) {
        ANSWER.store(if granted { GRANTED } else { REFUSED }, Ordering::SeqCst);
    }
    static DESCRIPTOR: Descriptor = Descriptor { reserved: 0, size: size_of::<Block>() };
    unsafe extern "C" {
        static _NSConcreteGlobalBlock: c_void;
    }
    const BLOCK_IS_GLOBAL: i32 = 1 << 28;
    static BLOCK: Block = Block {
        isa: &raw const _NSConcreteGlobalBlock,
        flags: BLOCK_IS_GLOBAL,
        reserved: 0,
        invoke: answered,
        descriptor: &DESCRIPTOR,
    };

    ANSWER.store(PENDING, Ordering::SeqCst);
    type Send = unsafe extern "C" fn(*const c_void, *const c_void, *const c_void, *const Block);
    unsafe {
        let cls = objc::objc_getClass(c"AVCaptureDevice".as_ptr());
        if cls.is_null() {
            return MicPermission::NotDetermined;
        }
        let sel = objc::sel_registerName(c"requestAccessForMediaType:completionHandler:".as_ptr());
        let send: Send = std::mem::transmute(objc::objc_msgSend as *const ());
        send(cls, sel, objc::AVMediaTypeAudio, &BLOCK);
    }
    // 最多等 2 分鐘，沒回應就照當下狀態回報，畫面會請使用者再按一次
    for _ in 0..1200 {
        match ANSWER.load(Ordering::SeqCst) {
            GRANTED => return MicPermission::Authorized,
            REFUSED => return mic_permission(),
            _ => std::thread::sleep(std::time::Duration::from_millis(100)),
        }
    }
    mic_permission()
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

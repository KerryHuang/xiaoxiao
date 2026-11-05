//! 螢幕是否鎖定。
use std::ffi::{CString, c_void};

use engine::platform::ScreenState;

use crate::ffi::{CFRelease, CFTypeRef};

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGSessionCopyCurrentDictionary() -> CFTypeRef;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFDictionaryGetValue(d: CFTypeRef, k: *const c_void) -> CFTypeRef;
    fn CFStringCreateWithCString(alloc: CFTypeRef, s: *const i8, enc: u32) -> CFTypeRef;
    fn CFBooleanGetValue(b: CFTypeRef) -> u8;
}

pub struct MacScreen;

impl ScreenState for MacScreen {
    fn locked(&self) -> bool {
        unsafe {
            let d = CGSessionCopyCurrentDictionary();
            if d.is_null() {
                return false; // 沒有登入工作階段（例如在 ssh 裡）
            }
            let k = CString::new("CGSSessionScreenIsLocked").unwrap();
            let key = CFStringCreateWithCString(std::ptr::null(), k.as_ptr(), 0x0800_0100);
            let v = CFDictionaryGetValue(d, key);
            let locked = !v.is_null() && CFBooleanGetValue(v) != 0;
            CFRelease(key);
            CFRelease(d);
            locked
        }
    }
}

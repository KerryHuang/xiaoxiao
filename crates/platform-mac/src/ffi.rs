//! CoreAudio／CoreFoundation 手寫綁定（只放用到的部分）。
#![allow(non_upper_case_globals)]
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr;

pub type ObjectId = u32;
pub type CFTypeRef = *const c_void;

#[repr(C)]
pub struct Address {
    pub selector: u32,
    pub scope: u32,
    pub element: u32,
}

const fn fourcc(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

pub const SYSTEM_OBJECT: ObjectId = 1;
pub const SCOPE_GLOBAL: u32 = fourcc(b"glob");
pub const SCOPE_INPUT: u32 = fourcc(b"inpt");
pub const PROCESS_LIST: u32 = fourcc(b"prs#");
pub const PROCESS_RUNNING_INPUT: u32 = fourcc(b"piri");
pub const PROCESS_PID: u32 = fourcc(b"ppid");
pub const DEVICES: u32 = fourcc(b"dev#");
pub const DEFAULT_INPUT: u32 = fourcc(b"dIn ");
pub const NAME: u32 = fourcc(b"lnam");
pub const STREAMS: u32 = fourcc(b"stm#");
pub const TRANSPORT: u32 = fourcc(b"tran");
pub const T_BLUETOOTH: u32 = fourcc(b"blue");
pub const T_BLUETOOTH_LE: u32 = fourcc(b"blea");
pub const T_BUILTIN: u32 = fourcc(b"bltn");
pub const T_USB: u32 = fourcc(b"usb ");
pub const T_VIRTUAL: u32 = fourcc(b"virt");
pub const T_AGGREGATE: u32 = fourcc(b"grup");
pub const T_CONTINUITY_WIRED: u32 = fourcc(b"ccwd");
pub const T_CONTINUITY_WIRELESS: u32 = fourcc(b"ccwl");

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectGetPropertyDataSize(o: ObjectId, a: *const Address, qs: u32, q: *const c_void, size: *mut u32) -> i32;
    fn AudioObjectGetPropertyData(o: ObjectId, a: *const Address, qs: u32, q: *const c_void, size: *mut u32, data: *mut c_void) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringGetCString(s: CFTypeRef, buf: *mut u8, len: isize, enc: u32) -> u8;
    pub fn CFRelease(o: CFTypeRef);
}
const UTF8: u32 = 0x0800_0100;

fn addr(selector: u32, scope: u32) -> Address {
    Address { selector, scope, element: 0 }
}

pub fn data_size(o: ObjectId, selector: u32, scope: u32) -> Option<u32> {
    let mut size = 0u32;
    let r = unsafe { AudioObjectGetPropertyDataSize(o, &addr(selector, scope), 0, ptr::null(), &mut size) };
    (r == 0).then_some(size)
}

pub fn get_u32(o: ObjectId, selector: u32, scope: u32) -> Option<u32> {
    let mut v = 0u32;
    let mut size = size_of::<u32>() as u32;
    let r = unsafe {
        AudioObjectGetPropertyData(o, &addr(selector, scope), 0, ptr::null(), &mut size, &mut v as *mut u32 as *mut c_void)
    };
    (r == 0).then_some(v)
}

pub fn get_i32(o: ObjectId, selector: u32, scope: u32) -> Option<i32> {
    get_u32(o, selector, scope).map(|v| v as i32)
}

pub fn get_ids(o: ObjectId, selector: u32, scope: u32) -> Vec<ObjectId> {
    let Some(mut size) = data_size(o, selector, scope) else { return vec![] };
    let mut v = vec![0u32; size as usize / size_of::<u32>()];
    let r = unsafe {
        AudioObjectGetPropertyData(o, &addr(selector, scope), 0, ptr::null(), &mut size, v.as_mut_ptr() as *mut c_void)
    };
    if r != 0 {
        return vec![];
    }
    v.truncate(size as usize / size_of::<u32>());
    v
}

pub fn cfstring_to_string(s: CFTypeRef) -> Option<String> {
    if s.is_null() {
        return None;
    }
    let mut buf = vec![0u8; 1024];
    let ok = unsafe { CFStringGetCString(s, buf.as_mut_ptr(), buf.len() as isize, UTF8) };
    if ok == 0 {
        return None;
    }
    let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8(buf[..n].to_vec()).ok()
}

pub fn get_name(o: ObjectId) -> Option<String> {
    let mut s: CFTypeRef = ptr::null();
    let mut size = size_of::<CFTypeRef>() as u32;
    let r = unsafe {
        AudioObjectGetPropertyData(o, &addr(NAME, SCOPE_GLOBAL), 0, ptr::null(), &mut size, &mut s as *mut CFTypeRef as *mut c_void)
    };
    if r != 0 {
        return None;
    }
    let out = cfstring_to_string(s);
    unsafe { CFRelease(s) };
    out
}

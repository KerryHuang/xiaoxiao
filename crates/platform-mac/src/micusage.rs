//! 偵測別的程序是否正在用麥克風（通話、錄音）。
use engine::platform::MicUsage;

use crate::ffi::*;

pub struct MacMicUsage {
    me: i32,
}

impl MacMicUsage {
    pub fn new() -> MacMicUsage {
        MacMicUsage { me: std::process::id() as i32 }
    }
}

impl Default for MacMicUsage {
    fn default() -> Self {
        Self::new()
    }
}

impl MicUsage for MacMicUsage {
    fn others_using_mic(&self) -> bool {
        get_ids(SYSTEM_OBJECT, PROCESS_LIST, SCOPE_GLOBAL).into_iter().any(|p| {
            get_u32(p, PROCESS_RUNNING_INPUT, SCOPE_GLOBAL) == Some(1)
                && get_i32(p, PROCESS_PID, SCOPE_GLOBAL).is_some_and(|pid| pid != self.me)
        })
    }
}

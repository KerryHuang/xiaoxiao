//! 何時暫停收音。

/// 任何一項為真就暫停：別的程序在用麥克風（通話中）、螢幕鎖定、使用者靜音、休息中。
pub fn should_pause(others_using_mic: bool, locked: bool, user_muted: bool, resting: bool) -> bool {
    others_using_mic || locked || user_muted || resting
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_any() {
        assert!(!should_pause(false, false, false, false));
        assert!(should_pause(true, false, false, false));
        assert!(should_pause(false, true, false, false));
        assert!(should_pause(false, false, true, false));
        assert!(should_pause(false, false, false, true));
    }
}

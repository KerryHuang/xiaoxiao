//! 何時暫停收音，以及為什麼。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseReason {
    Rest,
    Call,
    Lock,
}

/// 目前該暫停的原因；同時有多個時回報最優先的，通話最優先，因為角色要跟著收起來。
pub fn pause_reason(others_using_mic: bool, locked: bool, resting: bool) -> Option<PauseReason> {
    if others_using_mic {
        Some(PauseReason::Call)
    } else if locked {
        Some(PauseReason::Lock)
    } else if resting {
        Some(PauseReason::Rest)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_reason_priority() {
        assert_eq!(pause_reason(false, false, false), None);
        assert_eq!(pause_reason(false, false, true), Some(PauseReason::Rest));
        assert_eq!(pause_reason(false, true, true), Some(PauseReason::Lock));
        assert_eq!(pause_reason(true, true, true), Some(PauseReason::Call));
    }
}

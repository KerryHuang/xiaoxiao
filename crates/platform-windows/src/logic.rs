//! 不碰系統 API 的判斷邏輯。

use engine::platform::MicPermission;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Builtin,
    Usb,
    Bluetooth,
    Other,
}

/// 由端點屬性 `{b3f8fa53-0004-438e-9003-51a46e139bfc}` 的 `,2` 與 `,39` 判斷麥克風接在哪裡。
/// 經 Intel Smart Sound 轉接時，`,2` 與 `PKEY_Device_EnumeratorName` 都是 INTELAUDIO，真正來源在 `,39`。
pub fn transport(parent: Option<&str>, offload: Option<&str>) -> Transport {
    let Some(id) = offload.or(parent) else { return Transport::Other };
    let bus = id.strip_prefix("{1}.").unwrap_or(id);
    if bus.starts_with("BTH") {
        Transport::Bluetooth
    } else if bus.starts_with("USB") {
        Transport::Usb
    } else if bus.starts_with("INTELAUDIO") || bus.starts_with("HDAUDIO") {
        Transport::Builtin
    } else {
        Transport::Other
    }
}

/// 預設輸入是藍牙時改用的裝置：內建 > USB > 其他，排除藍牙。
pub fn pick_fallback(devices: Vec<(Transport, String)>) -> Option<String> {
    let rank = |t: Transport| match t {
        Transport::Builtin => Some(0),
        Transport::Usb => Some(1),
        Transport::Other => Some(2),
        Transport::Bluetooth => None,
    };
    let mut v: Vec<(u8, String)> = devices.into_iter().filter_map(|(t, n)| Some((rank(t)?, n))).collect();
    v.sort_by_key(|(r, _)| *r);
    v.into_iter().next().map(|(_, n)| n)
}

/// 由隱私設定判斷麥克風權限；參數依序是全機、目前使用者、傳統桌面 app 的 `Value`。
/// 傳統桌面 app 不會跳詢問視窗，所以不會回 `NotDetermined`；沒有設定值就是允許。
pub fn decide(machine: Option<&str>, user: Option<&str>, desktop_apps: Option<&str>) -> MicPermission {
    if machine == Some("Deny") {
        MicPermission::Restricted
    } else if user == Some("Deny") || desktop_apps == Some("Deny") {
        MicPermission::Denied
    } else {
        MicPermission::Authorized
    }
}

/// WTSINFOEX 的 SessionFlags：0 鎖定、1 未鎖定、其他未知。未知時不要無故暫停。
pub fn locked_flag(flags: i32) -> bool {
    flags == 0
}

/// 這個收音工作階段算不算「別的程序在用麥克風」。
pub fn counts(active: bool, pid: u32, system_sounds: bool, me: u32) -> bool {
    active && !system_sounds && pid != me
}

/// 逐一看工作階段，每筆是（使用中、PID、系統音效）。讀取失敗的跳過：
/// 工作階段隨時會消失，不能因為一筆壞掉就漏掉同一端點上正在進行的通話。
pub fn any_session_counts<E>(sessions: impl IntoIterator<Item = Result<(bool, u32, bool), E>>, me: u32) -> bool {
    sessions.into_iter().flatten().any(|(active, pid, system_sounds)| counts(active, pid, system_sounds, me))
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::platform::MicPermission;

    #[test]
    fn permission_from_registry() {
        assert_eq!(decide(None, None, None), MicPermission::Authorized); // 沒設定就是允許
        assert_eq!(decide(Some("Deny"), Some("Allow"), Some("Allow")), MicPermission::Restricted); // 全機關閉
        assert_eq!(decide(None, Some("Deny"), Some("Allow")), MicPermission::Denied);
        assert_eq!(decide(None, Some("Allow"), Some("Deny")), MicPermission::Denied); // 只關傳統桌面 app
    }

    #[test]
    fn locked_from_session_flags() {
        assert!(locked_flag(0)); // WTS_SESSIONSTATE_LOCK
        assert!(!locked_flag(1)); // WTS_SESSIONSTATE_UNLOCK
        assert!(!locked_flag(-1)); // 未知：不要無故暫停
    }

    #[test]
    fn only_active_sessions_of_other_processes_count() {
        assert!(counts(true, 42, false, 7));
        assert!(!counts(true, 7, false, 7)); // 自己
        assert!(!counts(false, 42, false, 7));
        assert!(!counts(true, 42, true, 7)); // 系統音效工作階段
    }

    #[test]
    fn one_broken_session_does_not_hide_a_call() {
        // 剛掛斷的工作階段讀取失敗，同一端點上還有另一個通話
        assert!(any_session_counts([Err("gone"), Ok((true, 42, false))], 7));
        assert!(!any_session_counts([Err("gone"), Ok((true, 7, false))], 7));
        assert!(!any_session_counts::<&str>([], 7));
    }

    #[test]
    fn transport_uses_offload_property() {
        // Intel Smart Sound：,2 是 INTELAUDIO，真正來源在 ,39
        assert_eq!(transport(Some(r"{1}.INTELAUDIO\CTLR_X\0"), Some(r"{1}.BTHENUM\{0000111F}_X\0")), Transport::Bluetooth);
        assert_eq!(transport(Some(r"{1}.INTELAUDIO\CTLR_X\0"), Some(r"{1}.USB\VID_0000&PID_0000\0")), Transport::Usb);
        assert_eq!(transport(Some(r"{1}.INTELAUDIO\CTLR_X\0"), None), Transport::Builtin);
    }

    #[test]
    fn transport_without_offload() {
        assert_eq!(transport(Some(r"{1}.BTHHFENUM\X\0"), None), Transport::Bluetooth);
        assert_eq!(transport(Some(r"{1}.USB\VID_0000\0"), None), Transport::Usb);
        assert_eq!(transport(Some(r"{1}.HDAUDIO\X\0"), None), Transport::Builtin);
        assert_eq!(transport(None, None), Transport::Other);
    }

    #[test]
    fn fallback_prefers_builtin_then_usb_never_bluetooth() {
        let d = |t, n: &str| (t, n.to_string());
        let v = vec![d(Transport::Bluetooth, "藍牙"), d(Transport::Other, "虛擬"), d(Transport::Usb, "USB"), d(Transport::Builtin, "內建")];
        assert_eq!(pick_fallback(v).as_deref(), Some("內建"));
        assert_eq!(pick_fallback(vec![d(Transport::Other, "虛擬"), d(Transport::Usb, "USB")]).as_deref(), Some("USB"));
        assert_eq!(pick_fallback(vec![d(Transport::Bluetooth, "藍牙")]), None);
    }
}

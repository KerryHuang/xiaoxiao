//! 不碰系統 API 的判斷邏輯。

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

#[cfg(test)]
mod tests {
    use super::*;

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

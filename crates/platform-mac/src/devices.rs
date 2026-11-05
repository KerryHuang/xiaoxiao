//! 輸入裝置列舉與藍牙判斷。
use engine::platform::{Devices, InputDevice};

use crate::ffi::*;

pub struct MacDevices;

/// 內部用：帶 transport type 的輸入裝置。
pub(crate) struct RawInput {
    pub name: String,
    pub transport: u32,
}

fn is_bluetooth(t: u32) -> bool {
    t == T_BLUETOOTH || t == T_BLUETOOTH_LE
}

pub(crate) fn raw_inputs() -> Vec<RawInput> {
    get_ids(SYSTEM_OBJECT, DEVICES, SCOPE_GLOBAL)
        .into_iter()
        .filter(|&d| data_size(d, STREAMS, SCOPE_INPUT).unwrap_or(0) > 0)
        .filter_map(|d| Some(RawInput { name: get_name(d)?, transport: get_u32(d, TRANSPORT, SCOPE_GLOBAL).unwrap_or(0) }))
        .collect()
}

fn to_input(r: &RawInput) -> InputDevice {
    InputDevice { name: r.name.clone(), bluetooth: is_bluetooth(r.transport) }
}

/// 預設輸入是藍牙時改用的裝置：內建 > USB > 其他；排除藍牙、虛擬、聚合、iPhone 接續互通。
pub(crate) fn fallback_input() -> Option<String> {
    let rank = |t: u32| match t {
        T_BUILTIN => Some(0),
        T_USB => Some(1),
        T_VIRTUAL | T_AGGREGATE | T_CONTINUITY_WIRED | T_CONTINUITY_WIRELESS => None,
        t if is_bluetooth(t) => None,
        _ => Some(2),
    };
    let mut v: Vec<(u8, String)> = raw_inputs().into_iter().filter_map(|r| Some((rank(r.transport)?, r.name))).collect();
    v.sort_by_key(|(r, _)| *r);
    v.into_iter().next().map(|(_, n)| n)
}

impl Devices for MacDevices {
    fn inputs(&self) -> Vec<InputDevice> {
        raw_inputs().iter().map(to_input).collect()
    }

    fn default_input(&self) -> Option<InputDevice> {
        let id = get_u32(SYSTEM_OBJECT, DEFAULT_INPUT, SCOPE_GLOBAL).filter(|&d| d != 0)?;
        Some(InputDevice {
            name: get_name(id)?,
            bluetooth: is_bluetooth(get_u32(id, TRANSPORT, SCOPE_GLOBAL).unwrap_or(0)),
        })
    }
}

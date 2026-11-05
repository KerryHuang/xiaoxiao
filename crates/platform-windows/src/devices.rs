//! 輸入裝置列舉與藍牙判斷。
use engine::platform::{Devices, InputDevice};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::{DEVICE_STATE_ACTIVE, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eConsole};
use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PropVariantClear};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, STGM_READ};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::core::GUID;

use crate::com::Com;
use crate::logic::{Transport, pick_fallback, transport};

const ENDPOINT_PROPS: GUID = GUID::from_u128(0xb3f8fa53_0004_438e_9003_51a46e139bfc);
const PARENT: PROPERTYKEY = PROPERTYKEY { fmtid: ENDPOINT_PROPS, pid: 2 };
const OFFLOAD: PROPERTYKEY = PROPERTYKEY { fmtid: ENDPOINT_PROPS, pid: 39 };

pub struct WinDevices;

pub(crate) fn enumerator() -> windows::core::Result<IMMDeviceEnumerator> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
}

fn string_prop(store: &IPropertyStore, key: &PROPERTYKEY) -> Option<String> {
    unsafe {
        let mut v: PROPVARIANT = store.GetValue(key).ok()?;
        let s = if v.Anonymous.Anonymous.vt == VT_LPWSTR { v.Anonymous.Anonymous.Anonymous.pwszVal.to_string().ok() } else { None };
        let _ = PropVariantClear(&mut v);
        s
    }
}

fn describe(d: &IMMDevice) -> Option<(Transport, String)> {
    let store = unsafe { d.OpenPropertyStore(STGM_READ) }.ok()?;
    let name = string_prop(&store, &PKEY_Device_FriendlyName)?;
    let t = transport(string_prop(&store, &PARENT).as_deref(), string_prop(&store, &OFFLOAD).as_deref());
    Some((t, name))
}

fn raw_inputs() -> Vec<(Transport, String)> {
    let _com = Com::init();
    let Ok(en) = enumerator() else { return vec![] };
    let Ok(list) = (unsafe { en.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) }) else { return vec![] };
    let n = unsafe { list.GetCount() }.unwrap_or(0);
    (0..n).filter_map(|i| describe(&unsafe { list.Item(i) }.ok()?)).collect()
}

fn to_input((t, name): (Transport, String)) -> InputDevice {
    InputDevice { name, bluetooth: t == Transport::Bluetooth }
}

pub(crate) fn fallback_input() -> Option<String> {
    pick_fallback(raw_inputs())
}

impl Devices for WinDevices {
    fn inputs(&self) -> Vec<InputDevice> {
        raw_inputs().into_iter().map(to_input).collect()
    }

    fn default_input(&self) -> Option<InputDevice> {
        let _com = Com::init();
        let d = unsafe { enumerator().ok()?.GetDefaultAudioEndpoint(eCapture, eConsole) }.ok()?;
        describe(&d).map(to_input)
    }
}

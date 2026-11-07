//! 說話與提示音：SAPI 的台灣語音（LCID 0x404）。
//! 每次呼叫都建立物件：COM 物件不是 Send，而 Speaker 要 Send。
use engine::platform::Speaker;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT, SND_SYNC};
use windows::Win32::Media::Speech::{ISpObjectToken, ISpObjectTokenCategory, ISpVoice, SPCAT_VOICES, SPF_IS_NOT_XML, SpObjectTokenCategory, SpVoice};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::core::{HSTRING, PCWSTR, w};

use crate::com::Com;

pub struct WinSpeaker;

fn taiwan_voice() -> windows::core::Result<Option<ISpObjectToken>> {
    unsafe {
        let cat: ISpObjectTokenCategory = CoCreateInstance(&SpObjectTokenCategory, None, CLSCTX_ALL)?;
        cat.SetId(SPCAT_VOICES, false)?;
        let list = cat.EnumTokens(w!("Language=404"), PCWSTR::null())?;
        let mut tok = None;
        list.Next(1, &mut tok, None)?;
        Ok(tok)
    }
}

/// 有沒有台灣語音可以用。
pub fn voice_ok() -> bool {
    let _com = Com::init();
    matches!(taiwan_voice(), Ok(Some(_)))
}

impl Speaker for WinSpeaker {
    fn say(&self, text: &str) -> Result<(), String> {
        let _com = Com::init();
        let tok = taiwan_voice().map_err(|e| format!("找語音失敗：{e}"))?.ok_or("沒有安裝台灣語音")?;
        unsafe {
            let v: ISpVoice = CoCreateInstance(&SpVoice, None, CLSCTX_ALL).map_err(|e| format!("無法啟動語音：{e}"))?;
            v.SetVoice(&tok).map_err(|e| format!("無法設定語音：{e}"))?;
            v.Speak(&HSTRING::from(text), SPF_IS_NOT_XML.0 as u32, None).map_err(|e| format!("說話失敗：{e}"))
        }
    }

    fn chime(&self) {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let file = HSTRING::from(format!(r"{root}\Media\Windows Notify Calendar.wav"));
        let _ = unsafe { PlaySoundW(&file, None, SND_FILENAME | SND_ASYNC | SND_NODEFAULT) };
    }

    fn wake_sound(&self) {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let file = HSTRING::from(format!(r"{root}\Media\ding.wav"));
        let _ = unsafe { PlaySoundW(&file, None, SND_FILENAME | SND_SYNC | SND_NODEFAULT) };
    }
}

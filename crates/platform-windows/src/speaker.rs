//! 說話與提示音：SAPI 的台灣語音（LCID 0x404）。
//! 每次呼叫都建立物件：COM 物件不是 Send，而 Speaker 要 Send。
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use engine::platform::{Speaker, Stopper};
use windows::Win32::Foundation::WAIT_TIMEOUT;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT, SND_SYNC, WAVE_FORMAT_PCM, WAVEFORMATEX};
use windows::Win32::Media::Speech::{
    ISpObjectToken, ISpObjectTokenCategory, ISpStream, ISpVoice, SPCAT_VOICES, SPF_ASYNC, SPF_IS_NOT_XML,
    SPF_PURGEBEFORESPEAK, SPFM_CREATE_ALWAYS, SPFM_OPEN_READONLY, SpObjectTokenCategory, SpStream, SpVoice,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::System::Threading::WaitForSingleObject;
use windows::core::{GUID, HSTRING, PCWSTR, w};

use crate::com::Com;

#[derive(Default)]
pub struct WinSpeaker {
    stop: Arc<AtomicBool>,
}

/// 等非同步的 Speak／SpeakStream 完成；每 50 ms 看一次旗標，被設就清掉佇列後返回。
/// 只有說話的執行緒碰 COM 物件，旗標是唯一跨執行緒的東西。
fn wait_done(v: &ISpVoice, stop: &AtomicBool) {
    unsafe {
        let h = v.SpeakCompleteEvent();
        // 只有逾時才繼續等；完成（WAIT_OBJECT_0）或失敗（WAIT_FAILED）都返回，免得失敗時空轉
        while WaitForSingleObject(h, 50) == WAIT_TIMEOUT {
            if stop.load(Ordering::SeqCst) {
                let _ = v.Speak(PCWSTR::null(), SPF_PURGEBEFORESPEAK.0 as u32, None);
                return;
            }
        }
    }
}

/// 先錄好、剪掉空白的短句。即時合成的「我在」句尾有 0.7 秒空白，
/// 而且語音引擎冷啟動時第一句要 4 秒以上，喊完要等很久才開始聽。
const CANNED: &[&str] = &["我在"];
/// sapi.h 的 SPDFID_WaveFormatEx；windows crate 沒有收錄。
const SPDFID_WAVE_FORMAT_EX: GUID = GUID::from_u128(0xc31adbae_527f_4ff5_a230_f62bb61ff70c);
static CANNED_WAV: OnceLock<Vec<(&'static str, PathBuf)>> = OnceLock::new();

/// 啟動時在背景呼叫一次；還沒錄好之前照常即時合成。
pub fn prepare_canned(dir: PathBuf) {
    let _com = Com::init();
    let list = CANNED
        .iter()
        .enumerate()
        .filter_map(|(i, &text)| {
            let raw = dir.join(format!("canned-{i}-raw.wav"));
            let out = dir.join(format!("canned-{i}.wav"));
            render(text, &raw).ok()?;
            let bytes = std::fs::read(&raw).ok();
            let _ = std::fs::remove_file(&raw);
            std::fs::write(&out, engine::audio::trim_wav_silence(&bytes?)?).ok()?;
            Some((text, out))
        })
        .collect();
    let _ = CANNED_WAV.set(list);
}

fn canned(text: &str) -> Option<&'static Path> {
    CANNED_WAV.get()?.iter().find(|(t, _)| *t == text).map(|(_, p)| p.as_path())
}

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

fn voice() -> Result<ISpVoice, String> {
    let tok = taiwan_voice().map_err(|e| format!("找語音失敗：{e}"))?.ok_or("沒有安裝台灣語音")?;
    unsafe {
        let v: ISpVoice = CoCreateInstance(&SpVoice, None, CLSCTX_ALL).map_err(|e| format!("無法啟動語音：{e}"))?;
        v.SetVoice(&tok).map_err(|e| format!("無法設定語音：{e}"))?;
        Ok(v)
    }
}

/// 22.05 kHz 16-bit 單聲道，trim_wav_silence 讀得懂的格式。
fn render(text: &str, path: &Path) -> Result<(), String> {
    let v = voice()?;
    let fmt = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM as u16,
        nChannels: 1,
        nSamplesPerSec: 22050,
        nAvgBytesPerSec: 44100,
        nBlockAlign: 2,
        wBitsPerSample: 16,
        cbSize: 0,
    };
    unsafe {
        let s: ISpStream = CoCreateInstance(&SpStream, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        s.BindToFile(&HSTRING::from(path.as_os_str()), SPFM_CREATE_ALWAYS, Some(&SPDFID_WAVE_FORMAT_EX), Some(&fmt), 0)
            .map_err(|e| e.to_string())?;
        v.SetOutput(&s, true).map_err(|e| e.to_string())?;
        let r = v.Speak(&HSTRING::from(text), SPF_IS_NOT_XML.0 as u32, None).map_err(|e| e.to_string());
        let _ = s.Close();
        r
    }
}

/// 走 SAPI 播放，不用 PlaySound：PlaySound 一次只能放一個聲音，會切掉還在響的提示音。
fn play_wav(path: &Path, stop: &AtomicBool) -> Result<(), String> {
    unsafe {
        let v: ISpVoice = CoCreateInstance(&SpVoice, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        let s: ISpStream = CoCreateInstance(&SpStream, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        s.BindToFile(&HSTRING::from(path.as_os_str()), SPFM_OPEN_READONLY, None, None, 0).map_err(|e| e.to_string())?;
        let r = v.SpeakStream(&s, SPF_ASYNC.0 as u32, None).map_err(|e| e.to_string());
        if r.is_ok() {
            wait_done(&v, stop);
        }
        let _ = s.Close();
        r
    }
}

/// 有沒有台灣語音可以用。
pub fn voice_ok() -> bool {
    let _com = Com::init();
    matches!(taiwan_voice(), Ok(Some(_)))
}

impl Speaker for WinSpeaker {
    fn say(&self, text: &str) -> Result<(), String> {
        self.stop.store(false, Ordering::SeqCst);
        let _com = Com::init();
        if let Some(p) = canned(text)
            && play_wav(p, &self.stop).is_ok()
        {
            return Ok(());
        }
        let v = voice()?;
        unsafe {
            v.Speak(&HSTRING::from(text), (SPF_IS_NOT_XML.0 | SPF_ASYNC.0) as u32, None).map_err(|e| format!("說話失敗：{e}"))?;
        }
        wait_done(&v, &self.stop);
        Ok(())
    }

    fn stopper(&self) -> Stopper {
        let stop = self.stop.clone();
        Arc::new(move || stop.store(true, Ordering::SeqCst))
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

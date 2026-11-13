//! 一段多句的說話：逐句唸、記下唸了哪些、被快捷鍵切掉時把沒唸的收起來。
use std::sync::atomic::{AtomicBool, Ordering};

use crate::audio::{Agc, SpeechDetector};
use crate::pipeline::Platform;

/// 這段話怎麼被切斷的。
pub(crate) enum Cut {
    /// 快捷鍵按停。
    Stopped,
    /// 使用者開口插話；帶著已經收到的聲音區塊。
    #[allow(dead_code)]
    Barge(Vec<Vec<f32>>),
}

pub(crate) struct Recital<'a, P: Platform> {
    p: &'a mut P,
    #[allow(dead_code)]
    agc: &'a mut Agc,
    #[allow(dead_code)]
    vad: &'a mut SpeechDetector,
    stop: &'a AtomicBool,
    on_speak: &'a dyn Fn(),
    #[allow(dead_code)]
    gaps: bool,
    pub spoken: Vec<String>,
    pub unsaid: Vec<String>,
    pub cut: Option<Cut>,
}

impl<'a, P: Platform> Recital<'a, P> {
    /// 建立時清掉舊的停止旗標。
    pub fn new(
        p: &'a mut P,
        agc: &'a mut Agc,
        vad: &'a mut SpeechDetector,
        stop: &'a AtomicBool,
        on_speak: &'a dyn Fn(),
        gaps: bool,
    ) -> Self {
        stop.store(false, Ordering::SeqCst);
        Recital { p, agc, vad, stop, on_speak, gaps, spoken: vec![], unsaid: vec![], cut: None }
    }

    /// 唸一句（呼叫端先做好 speakable）；空字串略過；已被切就只收進 unsaid。
    pub fn sentence(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        if self.cut.is_some() {
            self.unsaid.push(s.to_string());
            return;
        }
        // 平台的 say 開頭會清它自己的旗標，更早到的 stop 會被吃掉，所以先看引擎旗標
        if self.stop.load(Ordering::SeqCst) {
            self.cut = Some(Cut::Stopped);
            self.unsaid.push(s.to_string());
            return;
        }
        (self.on_speak)();
        let _ = self.p.speaker().say(s);
        self.spoken.push(s.to_string());
        self.p.audio().flush();
        if self.stop.swap(false, Ordering::SeqCst) {
            self.cut = Some(Cut::Stopped);
        }
    }
}

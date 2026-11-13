//! 音訊核心：16 kHz 單聲道 f32；環形緩衝、自動增益、斷句、重取樣。

use std::collections::VecDeque;

pub const SR: u32 = 16_000;
/// 100 ms 一塊。
pub const BLOCK: usize = 1600;

const AGC_WINDOW: usize = 50;
const AGC_TARGET: f32 = 0.3;
const AGC_MAX: f32 = 30.0;
/// 增益拉滿（AGC_MAX）後 RMS 超過這個值算有人講話。
pub const SPEECH_RMS: f32 = 0.02;
const PAUSE_BLOCKS: usize = 8;
const MAX_BLOCKS: usize = 100;
/// 顧問模式：停頓 1.5 秒算講完、最長 30 秒。
pub const ADVISE_PAUSE_BLOCKS: usize = 15;
pub const ADVISE_MAX_BLOCKS: usize = 300;
const NO_SPEECH_BLOCKS: usize = 60;

/// 只保留最近 N 秒的環形緩衝（只在記憶體）。
pub struct RingBuffer {
    cap: usize,
    buf: VecDeque<f32>,
}

impl RingBuffer {
    pub fn new(secs: f32) -> Self {
        let cap = (secs * SR as f32) as usize;
        Self { cap, buf: VecDeque::with_capacity(cap) }
    }
    pub fn push(&mut self, s: &[f32]) {
        self.buf.extend(s.iter().copied());
        let extra = self.buf.len().saturating_sub(self.cap);
        self.buf.drain(..extra);
    }
    pub fn snapshot(&self) -> Vec<f32> {
        self.buf.iter().copied().collect()
    }
    pub fn clear(&mut self) {
        self.buf.clear();
    }
}

/// 自動增益：gain = clamp(0.3 / 最近 5 秒區塊峰值的最大值, 1, 30)。
pub struct Agc {
    peaks: VecDeque<f32>,
    gain: f32,
}

impl Default for Agc {
    fn default() -> Self {
        Self { peaks: VecDeque::with_capacity(AGC_WINDOW), gain: 1.0 }
    }
}

impl Agc {
    pub fn process(&mut self, block: &[f32]) -> Vec<f32> {
        let peak = block.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        if self.peaks.len() == AGC_WINDOW {
            self.peaks.pop_front();
        }
        self.peaks.push_back(peak);
        let max = self.peaks.iter().fold(0.0f32, |m, x| m.max(*x)).max(1e-4);
        self.gain = (AGC_TARGET / max).clamp(1.0, AGC_MAX);
        block.iter().map(|x| (x * self.gain).clamp(-1.0, 1.0)).collect()
    }
    pub fn gain(&self) -> f32 {
        self.gain
    }
}

/// 判斷一塊是不是有人在講話：原始 RMS 夠大（相當於增益拉滿時超過 SPEECH_RMS），而且比雜訊底高約 10 dB。
/// 絕對門檻不隨目前增益走：增益會被剛才的大聲（例如喚醒詞）壓低，小聲說話就永遠過不了門檻。
/// 雜訊底取增益前最近 5 秒區塊 RMS 的第 20 百分位——增益跳動時不會跟著變，
/// 所以 AGC 把環境音拉大也不會被當成說話。
/// 說話的塊不算進雜訊底，不然連講 4 秒就把底撐高、後面的字被切掉；
/// 但連續超過 `ADAPT_AFTER` 塊都是「說話」就當成環境變吵，照常更新。
#[derive(Default)]
pub struct SpeechDetector {
    win: VecDeque<f32>,
    run: usize,
}

const FLOOR_FACTOR: f32 = 3.0;
/// 連續這麼多塊（10 秒）都判成說話，就讓雜訊底跟上。
const ADAPT_AFTER: usize = 100;
/// 剛開麥克風時，先累積這麼多塊（0.5 秒）估雜訊底，期間一律當沒講話。
pub const FLOOR_WARMUP: usize = 5;

impl SpeechDetector {
    pub fn push(&mut self, raw: &[f32]) -> bool {
        let r = rms(raw);
        let speech = self.win.len() >= FLOOR_WARMUP && r * AGC_MAX > SPEECH_RMS && r > self.floor() * FLOOR_FACTOR;
        self.run = if speech { self.run + 1 } else { 0 };
        if !speech || self.run > ADAPT_AFTER {
            if self.win.len() == AGC_WINDOW {
                self.win.pop_front();
            }
            self.win.push_back(r);
        }
        speech
    }

    pub fn floor(&self) -> f32 {
        if self.win.is_empty() {
            return 0.0;
        }
        let mut v: Vec<f32> = self.win.iter().copied().collect();
        v.sort_by(f32::total_cmp);
        v[v.len() / 5]
    }
}

pub fn rms(block: &[f32]) -> f32 {
    if block.is_empty() {
        return 0.0;
    }
    (block.iter().map(|x| x * x).sum::<f32>() / block.len() as f32).sqrt()
}

/// 剪掉 16-bit 單聲道 PCM wav 前後的靜音，前後各留 30 毫秒。
/// Windows 語音唸短句時句尾會拖 0.7 秒空白，讓「我在」之後要多等才開始聽。
/// 讀不懂的格式或整段靜音回傳 None。
pub fn trim_wav_silence(wav: &[u8]) -> Option<Vec<u8>> {
    let u16_at = |i: usize| wav.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |i: usize| wav.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    if wav.get(0..4)? != b"RIFF" || wav.get(8..12)? != b"WAVE" {
        return None;
    }
    let (mut pos, mut rate, mut data) = (12, None, None);
    while let (Some(id), Some(len)) = (wav.get(pos..pos + 4), u32_at(pos + 4)) {
        let body = pos + 8;
        match id {
            b"fmt " if u16_at(body)? == 1 && u16_at(body + 2)? == 1 && u16_at(body + 14)? == 16 => rate = u32_at(body + 4),
            b"fmt " => return None,
            b"data" => data = Some(wav.get(body..(body + len as usize).min(wav.len()))?),
            _ => {}
        }
        pos = body + len as usize + (len as usize & 1);
    }
    let (rate, data) = (rate?, data?);
    let s: Vec<i16> = data.as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b)).collect();
    let peak = s.iter().map(|v| v.unsigned_abs()).max()?;
    let th = peak / 20;
    let first = s.iter().position(|v| v.unsigned_abs() > th)?;
    let last = s.iter().rposition(|v| v.unsigned_abs() > th)?;
    let m = rate as usize * 3 / 100;
    let keep = &s[first.saturating_sub(m)..(last + m + 1).min(s.len())];
    let n = keep.len() as u32 * 2;
    let mut out = Vec::with_capacity(44 + n as usize);
    out.extend(b"RIFF");
    out.extend((36 + n).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend([1u8, 0, 1, 0]);
    out.extend(rate.to_le_bytes());
    out.extend((rate * 2).to_le_bytes());
    out.extend([2u8, 0, 16, 0]);
    out.extend(b"data");
    out.extend(n.to_le_bytes());
    keep.iter().for_each(|v| out.extend(v.to_le_bytes()));
    Some(out)
}

#[derive(Debug)]
pub enum EndpointEvent {
    Continue,
    Done(Vec<f32>),
    NoSpeech,
    TooLong(Vec<f32>),
}

/// 錄問題：停頓 0.8 秒算講完、最長 10 秒、6 秒沒開口就放棄。輸入是增益後的訊號。顧問模式用 `ADVISE_*`（見 `with_limits`）。
pub struct Endpointer {
    audio: Vec<f32>,
    blocks: usize,
    spoke: bool,
    silent: usize,
    pause: usize,
    max: usize,
}

impl Endpointer {
    pub fn new() -> Self {
        Self::with_limits(PAUSE_BLOCKS, MAX_BLOCKS)
    }
    pub fn with_limits(pause_blocks: usize, max_blocks: usize) -> Self {
        Self { audio: Vec::new(), blocks: 0, spoke: false, silent: 0, pause: pause_blocks, max: max_blocks }
    }
    /// `speech`：這塊有沒有人講話（由 SpeechDetector 判斷）。
    pub fn push(&mut self, gained: &[f32], speech: bool) -> EndpointEvent {
        self.audio.extend_from_slice(gained);
        self.blocks += 1;
        if speech {
            self.spoke = true;
            self.silent = 0;
        } else {
            self.silent += 1;
        }
        if self.spoke && self.silent >= self.pause {
            return EndpointEvent::Done(std::mem::take(&mut self.audio));
        }
        if self.blocks >= self.max {
            return if self.spoke { EndpointEvent::TooLong(std::mem::take(&mut self.audio)) } else { EndpointEvent::NoSpeech };
        }
        if !self.spoke && self.blocks >= NO_SPEECH_BLOCKS {
            return EndpointEvent::NoSpeech;
        }
        EndpointEvent::Continue
    }
}

impl Default for Endpointer {
    fn default() -> Self {
        Self::new()
    }
}

/// 轉成 16 kHz：降頻時先取區間平均（簡單抗混疊），再線性內插。輸出長度 = floor(len * 16000 / from_hz)。
pub fn resample_to_16k(input: &[f32], from_hz: u32) -> Vec<f32> {
    if from_hz == SR {
        return input.to_vec();
    }
    let out_len = (input.len() as u64 * SR as u64 / from_hz as u64) as usize;
    let ratio = from_hz as f64 / SR as f64;
    let half = (ratio / 2.0).max(0.5);
    (0..out_len)
        .map(|i| {
            let center = i as f64 * ratio;
            if ratio > 1.0 {
                let lo = (center - half).max(0.0) as usize;
                let hi = ((center + half).ceil() as usize).min(input.len()).max(lo + 1);
                input[lo..hi].iter().sum::<f32>() / (hi - lo) as f32
            } else {
                let j = center.floor() as usize;
                let frac = (center - j as f64) as f32;
                let a = input[j.min(input.len() - 1)];
                let b = input[(j + 1).min(input.len() - 1)];
                a + (b - a) * frac
            }
        })
        .collect()
}

/// 窗口內開口要連續幾塊（0.1 秒一塊）才算，避免一聲雜音就觸發。
pub const ONSET_BLOCKS: usize = 3;

pub enum OnsetStep {
    /// 開口成立，附上累積的全部塊。
    Started(Vec<Vec<f32>>),
    /// 已有開口塊但還沒定案。
    Pending,
    /// 沒有開口跡象。
    Quiet,
}

/// 開口判定：連續 ONSET_BLOCKS 塊有人聲，字間容許一塊空隙。
#[derive(Debug, Default)]
pub struct Onset {
    blocks: Vec<Vec<f32>>,
    gap: bool,
}

impl Onset {
    pub fn push(&mut self, g: Vec<f32>, speech: bool) -> OnsetStep {
        if speech {
            self.blocks.push(g);
            if self.blocks.len() >= ONSET_BLOCKS + usize::from(self.gap) {
                self.gap = false;
                return OnsetStep::Started(std::mem::take(&mut self.blocks));
            }
            OnsetStep::Pending
        } else if !self.blocks.is_empty() && !self.gap {
            // 字與字之間的小空隙不算講完
            self.gap = true;
            self.blocks.push(g);
            OnsetStep::Pending
        } else {
            self.clear();
            OnsetStep::Quiet
        }
    }

    pub fn pending(&self) -> bool {
        !self.blocks.is_empty()
    }

    pub fn clear(&mut self) {
        self.blocks.clear();
        self.gap = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onset_needs_three_blocks_and_allows_one_gap() {
        let b = || vec![0.3; BLOCK];
        let mut o = Onset::default();
        assert!(matches!(o.push(b(), true), OnsetStep::Pending));
        assert!(matches!(o.push(b(), false), OnsetStep::Pending)); // 字間空隙
        assert!(matches!(o.push(b(), true), OnsetStep::Pending));
        match o.push(b(), true) { OnsetStep::Started(v) => assert_eq!(v.len(), 4), _ => panic!("要開口") }
        assert!(!o.pending());
    }

    #[test]
    fn onset_single_click_is_quiet() {
        let b = || vec![0.3; BLOCK];
        let mut o = Onset::default();
        assert!(matches!(o.push(b(), true), OnsetStep::Pending));
        assert!(matches!(o.push(b(), false), OnsetStep::Pending));
        assert!(matches!(o.push(b(), false), OnsetStep::Quiet));
        assert!(!o.pending());
    }

    fn sine(amp: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| amp * (i as f32 * 2.0 * std::f32::consts::PI * 440.0 / SR as f32).sin()).collect()
    }

    /// 實機：重音把增益壓到 1 左右後，後面的字只剩 0.03，比雜訊底高很多卻被當成沒講話。
    #[test]
    fn speech_after_gain_drop_still_counts() {
        let mut d = SpeechDetector::default();
        for _ in 0..FLOOR_WARMUP {
            d.push(&sine(0.0015, BLOCK));
        }
        assert!(d.push(&sine(0.04, BLOCK)), "RMS 0.028，是雜訊底的 25 倍");
    }

    /// 喊完喚醒詞（正常音量，把增益壓低）後小聲問問題：原本要求「增益後」RMS 超過 0.02，
    /// 增益被剛才的喚醒詞壓住時，比雜訊底高很多的小聲說話也被當成沒講話，6 秒後默默回待命。
    #[test]
    fn quiet_question_after_a_normal_volume_wake_word_counts() {
        let (mut a, mut d) = (Agc::default(), SpeechDetector::default());
        let mut seed = 1;
        let mut feed = |b: Vec<f32>| {
            a.process(&b);
            d.push(&b)
        };
        for _ in 0..10 {
            feed(noise(0.001, BLOCK, &mut seed));
        }
        for _ in 0..8 {
            feed(sine(0.3, BLOCK)); // 平常音量的「嘿，小小」
        }
        for _ in 0..8 {
            feed(noise(0.001, BLOCK, &mut seed));
        }
        // 小聲問題：RMS 約 0.004，是雜訊底（0.0006）的 6 倍
        assert!(feed(sine(0.0057, BLOCK)), "增益被喚醒詞壓到 1 倍時小聲說話沒被偵測到");
    }

    #[test]
    fn ring_keeps_last_secs() {
        let mut r = RingBuffer::new(3.5);
        r.push(&vec![1.0; 16000 * 5]);
        assert_eq!(r.snapshot().len(), 56000);
        r.clear();
        assert!(r.snapshot().is_empty());
    }

    #[test]
    fn agc_boosts_quiet_and_caps() {
        let mut a = Agc::default();
        a.process(&sine(0.01, BLOCK));
        assert!((a.gain() - 30.0).abs() < 1e-3, "{}", a.gain());
        for _ in 0..50 {
            a.process(&sine(0.6, BLOCK));
        }
        assert!((a.gain() - 1.0).abs() < 1e-3, "{}", a.gain());
    }

    #[test]
    fn endpointer_done_after_pause() {
        let mut e = Endpointer::new();
        for _ in 0..10 {
            assert!(matches!(e.push(&sine(0.5, BLOCK), true), EndpointEvent::Continue));
        }
        for i in 0..8 {
            let ev = e.push(&vec![0.0; BLOCK], false);
            if i < 7 {
                assert!(matches!(ev, EndpointEvent::Continue));
            } else {
                assert!(matches!(ev, EndpointEvent::Done(ref a) if a.len() == 18 * BLOCK));
            }
        }
    }

    #[test]
    fn endpointer_gives_up_without_speech() {
        let mut e = Endpointer::new();
        for _ in 0..59 {
            assert!(matches!(e.push(&vec![0.0; BLOCK], false), EndpointEvent::Continue));
        }
        assert!(matches!(e.push(&vec![0.0; BLOCK], false), EndpointEvent::NoSpeech));
    }

    #[test]
    fn advising_endpointer_waits_1_5s_pause() {
        let mut e = Endpointer::with_limits(ADVISE_PAUSE_BLOCKS, ADVISE_MAX_BLOCKS);
        for _ in 0..10 { assert!(matches!(e.push(&sine(0.5, BLOCK), true), EndpointEvent::Continue)); }
        for i in 0..15 {
            let ev = e.push(&vec![0.0; BLOCK], false);
            if i < 14 { assert!(matches!(ev, EndpointEvent::Continue), "第 {i} 塊靜音就切了"); }
            else { assert!(matches!(ev, EndpointEvent::Done(ref a) if a.len() == 25 * BLOCK)); }
        }
    }

    #[test]
    fn advising_endpointer_caps_at_30s() {
        let mut e = Endpointer::with_limits(ADVISE_PAUSE_BLOCKS, ADVISE_MAX_BLOCKS);
        for _ in 0..299 { assert!(matches!(e.push(&sine(0.5, BLOCK), true), EndpointEvent::Continue)); }
        assert!(matches!(e.push(&sine(0.5, BLOCK), true), EndpointEvent::TooLong(ref a) if a.len() == 300 * BLOCK));
    }

    #[test]
    fn endpointer_caps_at_10s() {
        let mut e = Endpointer::new();
        for _ in 0..99 {
            assert!(matches!(e.push(&sine(0.5, BLOCK), true), EndpointEvent::Continue));
        }
        assert!(matches!(e.push(&sine(0.5, BLOCK), true), EndpointEvent::TooLong(ref a) if a.len() == 100 * BLOCK));
    }

    fn noise(amp: f32, n: usize, seed: &mut u32) -> Vec<f32> {
        (0..n)
            .map(|_| {
                *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (*seed as f32 / u32::MAX as f32 * 2.0 - 1.0) * amp
            })
            .collect()
    }

    #[test]
    fn noise_at_max_gain_is_not_speech() {
        let (mut a, mut d) = (Agc::default(), SpeechDetector::default());
        let mut seed = 1;
        let mut hits = 0;
        for _ in 0..100 {
            let b = noise(0.004, BLOCK, &mut seed);
            a.process(&b);
            if d.push(&b) {
                hits += 1;
            }
        }
        assert!((a.gain() - 30.0).abs() < 1e-3);
        assert_eq!(hits, 0, "增益拉滿後的雜音不能算說話");
    }

    #[test]
    fn speech_over_noise_is_speech() {
        let (mut a, mut d) = (Agc::default(), SpeechDetector::default());
        let mut seed = 1;
        for _ in 0..60 {
            let b = noise(0.004, BLOCK, &mut seed);
            a.process(&b);
            d.push(&b);
        }
        let b = sine(0.05, BLOCK);
        a.process(&b);
        assert!(d.push(&b));
    }

    /// 實機 11-13：連講約 4 秒後，雜訊底被說話聲撐高，後面的字都判成沒講話，長句在 5 秒處被切斷。
    #[test]
    fn long_speech_does_not_raise_the_floor() {
        let mut d = SpeechDetector::default();
        let mut seed = 1;
        for _ in 0..60 {
            d.push(&noise(0.002, BLOCK, &mut seed));
        }
        let quiet = (0..90).filter(|_| !d.push(&noise(0.05, BLOCK, &mut seed))).count();
        assert_eq!(quiet, 0, "連講 9 秒都要算說話");
    }

    /// 環境真的變吵（開風扇）時不能永遠當成有人講話：連續超過 10 秒後雜訊底要跟上。
    #[test]
    fn steady_new_noise_stops_counting_as_speech() {
        let mut d = SpeechDetector::default();
        let mut seed = 1;
        for _ in 0..60 {
            d.push(&noise(0.002, BLOCK, &mut seed));
        }
        for _ in 0..200 {
            d.push(&noise(0.05, BLOCK, &mut seed));
        }
        assert!(!d.push(&noise(0.05, BLOCK, &mut seed)), "20 秒後風扇聲應已是雜訊底");
    }

    #[test]
    fn resample_48k_len() {
        assert_eq!(resample_to_16k(&vec![0.0; 48000], 48000).len(), 16000);
        assert_eq!(resample_to_16k(&vec![0.0; 44100], 44100).len(), 16000);
        assert_eq!(resample_to_16k(&vec![0.5; 1600], 16000), vec![0.5; 1600]);
    }

    fn wav16(rate: u32, samples: &[i16]) -> Vec<u8> {
        let data = samples.len() as u32 * 2;
        let mut w = Vec::new();
        w.extend(b"RIFF");
        w.extend((36 + data).to_le_bytes());
        w.extend(b"WAVEfmt ");
        w.extend(16u32.to_le_bytes());
        w.extend([1u8, 0, 1, 0]);
        w.extend(rate.to_le_bytes());
        w.extend((rate * 2).to_le_bytes());
        w.extend([2u8, 0, 16, 0]);
        w.extend(b"data");
        w.extend(data.to_le_bytes());
        for s in samples {
            w.extend(s.to_le_bytes());
        }
        w
    }

    fn data_len(w: &[u8]) -> u32 {
        u32::from_le_bytes(w[40..44].try_into().unwrap())
    }

    #[test]
    fn trims_silence_around_speech_keeping_a_short_margin() {
        let r = 22050;
        let mut s = vec![0i16; r / 10];
        s.extend((0..r * 4 / 10).map(|i| if i % 2 == 0 { 8000 } else { -8000 }));
        s.extend(vec![0i16; r * 7 / 10]);
        let out = trim_wav_silence(&wav16(r as u32, &s)).unwrap();
        let secs = data_len(&out) as f32 / 2.0 / r as f32;
        assert!((0.45..0.48).contains(&secs), "{secs}");
        assert_eq!(out.len() as u32, 44 + data_len(&out));
        assert_eq!(u32::from_le_bytes(out[4..8].try_into().unwrap()), out.len() as u32 - 8);
    }

    #[test]
    fn refuses_wav_it_cannot_read() {
        assert!(trim_wav_silence(b"RIFF....WAVE").is_none());
        assert!(trim_wav_silence(&wav16(22050, &[0; 100])).is_none(), "全是靜音");
    }
}

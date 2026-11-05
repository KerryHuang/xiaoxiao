//! 音訊核心：16 kHz 單聲道 f32；環形緩衝、自動增益、斷句、重取樣。

use std::collections::VecDeque;

pub const SR: u32 = 16_000;
/// 100 ms 一塊。
pub const BLOCK: usize = 1600;

const AGC_WINDOW: usize = 50;
const AGC_TARGET: f32 = 0.3;
const AGC_MAX: f32 = 30.0;
/// 增益後 RMS 超過這個值算有人講話。
pub const SPEECH_RMS: f32 = 0.04;
const PAUSE_BLOCKS: usize = 8;
const MAX_BLOCKS: usize = 100;
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

pub fn rms(block: &[f32]) -> f32 {
    if block.is_empty() {
        return 0.0;
    }
    (block.iter().map(|x| x * x).sum::<f32>() / block.len() as f32).sqrt()
}

#[derive(Debug)]
pub enum EndpointEvent {
    Continue,
    Done(Vec<f32>),
    NoSpeech,
    TooLong(Vec<f32>),
}

/// 錄問題：停頓 0.8 秒算講完、最長 10 秒、6 秒沒開口就放棄。輸入是增益後的訊號。
pub struct Endpointer {
    audio: Vec<f32>,
    blocks: usize,
    spoke: bool,
    silent: usize,
}

impl Endpointer {
    pub fn new() -> Self {
        Self { audio: Vec::new(), blocks: 0, spoke: false, silent: 0 }
    }
    pub fn push(&mut self, gained: &[f32]) -> EndpointEvent {
        self.audio.extend_from_slice(gained);
        self.blocks += 1;
        if rms(gained) > SPEECH_RMS {
            self.spoke = true;
            self.silent = 0;
        } else {
            self.silent += 1;
        }
        if self.spoke && self.silent >= PAUSE_BLOCKS {
            return EndpointEvent::Done(std::mem::take(&mut self.audio));
        }
        if self.blocks >= MAX_BLOCKS {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(amp: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| amp * (i as f32 * 2.0 * std::f32::consts::PI * 440.0 / SR as f32).sin()).collect()
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
            assert!(matches!(e.push(&sine(0.5, BLOCK)), EndpointEvent::Continue));
        }
        for i in 0..8 {
            let ev = e.push(&vec![0.0; BLOCK]);
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
            assert!(matches!(e.push(&vec![0.0; BLOCK]), EndpointEvent::Continue));
        }
        assert!(matches!(e.push(&vec![0.0; BLOCK]), EndpointEvent::NoSpeech));
    }

    #[test]
    fn endpointer_caps_at_10s() {
        let mut e = Endpointer::new();
        for _ in 0..99 {
            assert!(matches!(e.push(&sine(0.5, BLOCK)), EndpointEvent::Continue));
        }
        assert!(matches!(e.push(&sine(0.5, BLOCK)), EndpointEvent::TooLong(ref a) if a.len() == 100 * BLOCK));
    }

    #[test]
    fn resample_48k_len() {
        assert_eq!(resample_to_16k(&vec![0.0; 48000], 48000).len(), 16000);
        assert_eq!(resample_to_16k(&vec![0.0; 44100], 44100).len(), 16000);
        assert_eq!(resample_to_16k(&vec![0.5; 1600], 16000), vec![0.5; 1600]);
    }
}

//! 收音：cpal 輸入 → 單聲道 → 16 kHz、每塊 BLOCK 個樣本。
//! 聲音只在記憶體裡流動，不落檔。
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use engine::audio::{BLOCK, resample_to_16k};
use engine::platform::{AudioError, AudioSource};

use crate::devices;

/// 這麼久沒收到任何聲音資料就當串流失效（例如睡眠醒來後）。
const STALL: Duration = Duration::from_secs(2);

enum Msg {
    Data(Vec<f32>),
    Dead,
}

struct Running {
    rx: Receiver<Msg>,
    stop: Sender<()>,
    thread: Option<JoinHandle<()>>,
    rate: u32,
    pending: Vec<f32>,
    name: String,
    /// 開的時候沒指定裝置（跟著系統預設走）。
    follows_default: bool,
}

pub struct MacAudio {
    run: Option<Running>,
}

impl MacAudio {
    pub fn new() -> MacAudio {
        MacAudio { run: None }
    }

    /// 目前開著的裝置名稱。
    pub fn device_name(&self) -> Option<&str> {
        self.run.as_ref().map(|r| r.name.as_str())
    }
}

impl Default for MacAudio {
    fn default() -> Self {
        Self::new()
    }
}

/// 決定要開哪個裝置：指定 > 系統預設（非藍牙）> 替代裝置。
fn choose(device: Option<&str>) -> Result<String, AudioError> {
    if let Some(d) = device {
        return Ok(d.to_string());
    }
    use engine::platform::Devices;
    match devices::MacDevices.default_input() {
        Some(d) if !d.bluetooth => Ok(d.name),
        _ => devices::fallback_input().ok_or(AudioError::NoUsableInput),
    }
}

fn spawn_stream(name: String, tx: Sender<Msg>, stop: Receiver<()>, ready: Sender<Result<u32, AudioError>>) {
    let host = cpal::default_host();
    let dev = host.input_devices().ok().and_then(|mut it| it.find(|d| d.to_string() == name));
    let Some(dev) = dev else {
        let _ = ready.send(Err(AudioError::Gone));
        return;
    };
    let cfg = match dev.default_input_config() {
        Ok(c) => c,
        Err(e) => {
            let _ = ready.send(Err(AudioError::Other(e.to_string())));
            return;
        }
    };
    let ch = cfg.channels() as usize;
    let rate = cfg.sample_rate();
    let mono = move |x: &[f32]| -> Vec<f32> { x.chunks(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect() };
    let tx_err = tx.clone();
    let on_err = move |_e: cpal::Error| {
        let _ = tx_err.send(Msg::Dead);
    };
    let stream = match cfg.sample_format() {
        cpal::SampleFormat::F32 => {
            let tx = tx.clone();
            dev.build_input_stream::<f32, _, _>(cfg.config(), move |x, _| { let _ = tx.send(Msg::Data(mono(x))); }, on_err, None)
        }
        cpal::SampleFormat::I16 => {
            let tx = tx.clone();
            dev.build_input_stream::<i16, _, _>(
                cfg.config(),
                move |x, _| {
                    let f: Vec<f32> = x.iter().map(|&v| v as f32 / 32768.0).collect();
                    let _ = tx.send(Msg::Data(mono(&f)));
                },
                on_err,
                None,
            )
        }
        f => {
            let _ = ready.send(Err(AudioError::Other(format!("不支援的取樣格式 {f:?}"))));
            return;
        }
    };
    let stream = match stream.and_then(|s| s.play().map(|_| s)) {
        Ok(s) => s,
        Err(e) => {
            let _ = ready.send(Err(AudioError::Other(e.to_string())));
            return;
        }
    };
    let _ = ready.send(Ok(rate));
    let _ = stop.recv(); // 等 close()；stream 在這個執行緒結束時釋放
    drop(stream);
}

impl AudioSource for MacAudio {
    fn open(&mut self, device: Option<&str>) -> Result<(), AudioError> {
        self.close();
        let name = choose(device)?;
        let (tx, rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let n = name.clone();
        let thread = std::thread::spawn(move || spawn_stream(n, tx, stop_rx, ready_tx));
        let rate = ready_rx.recv().map_err(|_| AudioError::Other("收音執行緒結束".into()))??;
        self.run = Some(Running { rx, stop: stop_tx, thread: Some(thread), rate, pending: vec![], name, follows_default: device.is_none() });
        Ok(())
    }

    fn read_block(&mut self) -> Result<Vec<f32>, AudioError> {
        let r = self.run.as_mut().ok_or(AudioError::Gone)?;
        let need = (r.rate as usize).div_ceil(10); // 100 ms
        while r.pending.len() < need {
            match r.rx.recv_timeout(STALL) {
                Ok(Msg::Data(d)) => r.pending.extend(d),
                Ok(Msg::Dead) | Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => return Err(AudioError::Gone),
            }
        }
        let chunk: Vec<f32> = r.pending.drain(..need).collect();
        let mut out = if r.rate == engine::audio::SR { chunk } else { resample_to_16k(&chunk, r.rate) };
        out.resize(BLOCK, 0.0);
        Ok(out)
    }

    fn flush(&mut self) {
        if let Some(r) = self.run.as_mut() {
            while r.rx.try_recv().is_ok() {}
            r.pending.clear();
        }
    }

    fn default_changed(&self) -> bool {
        match &self.run {
            Some(r) if r.follows_default => choose(None).is_ok_and(|n| n != r.name),
            _ => false,
        }
    }

    fn close(&mut self) {
        if let Some(mut r) = self.run.take() {
            let _ = r.stop.send(());
            if let Some(t) = r.thread.take() {
                let _ = t.join();
            }
        }
    }
}

impl Drop for MacAudio {
    fn drop(&mut self) {
        self.close();
    }
}

//! sherpa-onnx 關鍵詞偵測（第一關喚醒）。手寫綁定 c-api.h v2.0.1 用到的部分。
use std::ffi::{CString, c_char, c_float, c_void};
use std::path::Path;
use std::ptr;

use engine::audio::SR;

const KEYWORDS: &str = "h ēi x iǎo x iǎo @嘿小小\n";
const ENCODER: &str = "encoder-epoch-13-avg-2-chunk-16-left-64.int8.onnx";
const DECODER: &str = "decoder-epoch-13-avg-2-chunk-16-left-64.onnx";
const JOINER: &str = "joiner-epoch-13-avg-2-chunk-16-left-64.int8.onnx";

#[repr(C)]
struct FeatureConfig { sample_rate: i32, feature_dim: i32 }
#[repr(C)]
struct Transducer { encoder: *const c_char, decoder: *const c_char, joiner: *const c_char }
#[repr(C)]
struct Paraformer { encoder: *const c_char, decoder: *const c_char }
#[repr(C)]
struct OneModel { model: *const c_char }
#[repr(C)]
struct OnlineModelConfig {
    transducer: Transducer,
    paraformer: Paraformer,
    zipformer2_ctc: OneModel,
    tokens: *const c_char,
    num_threads: i32,
    provider: *const c_char,
    debug: i32,
    model_type: *const c_char,
    modeling_unit: *const c_char,
    bpe_vocab: *const c_char,
    tokens_buf: *const c_char,
    tokens_buf_size: i32,
    nemo_ctc: OneModel,
    t_one_ctc: OneModel,
}
#[repr(C)]
struct KwsConfig {
    feat_config: FeatureConfig,
    model_config: OnlineModelConfig,
    max_active_paths: i32,
    num_trailing_blanks: i32,
    keywords_score: c_float,
    keywords_threshold: c_float,
    keywords_file: *const c_char,
    keywords_buf: *const c_char,
    keywords_buf_size: i32,
}
#[repr(C)]
struct KeywordResult {
    keyword: *const c_char,
    tokens: *const c_char,
    tokens_arr: *const *const c_char,
    count: i32,
    timestamps: *mut c_float,
    start_time: c_float,
    json: *const c_char,
}

unsafe extern "C" {
    fn SherpaOnnxCreateKeywordSpotter(c: *const KwsConfig) -> *const c_void;
    fn SherpaOnnxDestroyKeywordSpotter(s: *const c_void);
    fn SherpaOnnxCreateKeywordStream(s: *const c_void) -> *const c_void;
    fn SherpaOnnxDestroyOnlineStream(st: *const c_void);
    fn SherpaOnnxOnlineStreamAcceptWaveform(st: *const c_void, sr: i32, x: *const c_float, n: i32);
    fn SherpaOnnxIsKeywordStreamReady(s: *const c_void, st: *const c_void) -> i32;
    fn SherpaOnnxDecodeKeywordStream(s: *const c_void, st: *const c_void);
    fn SherpaOnnxResetKeywordStream(s: *const c_void, st: *const c_void);
    fn SherpaOnnxGetKeywordResult(s: *const c_void, st: *const c_void) -> *const KeywordResult;
    fn SherpaOnnxDestroyKeywordResult(r: *const KeywordResult);
}

pub struct Spotter {
    spotter: *const c_void,
    stream: *const c_void,
}

// 只在擁有者執行緒上使用；可移交執行緒。
unsafe impl Send for Spotter {}

fn cpath(dir: &Path, f: &str) -> Result<CString, String> {
    let p = dir.join(f);
    if !p.is_file() {
        return Err(format!("缺少模型檔 {}", p.display()));
    }
    CString::new(p.to_string_lossy().as_bytes()).map_err(|e| e.to_string())
}

impl Spotter {
    pub fn new(model_dir: &Path) -> Result<Spotter, String> {
        let enc = cpath(model_dir, ENCODER)?;
        let dec = cpath(model_dir, DECODER)?;
        let joi = cpath(model_dir, JOINER)?;
        let tok = cpath(model_dir, "tokens.txt")?;
        let provider = CString::new("cpu").unwrap();
        let kw = CString::new(KEYWORDS).unwrap();
        let null = ptr::null();
        let cfg = KwsConfig {
            feat_config: FeatureConfig { sample_rate: SR as i32, feature_dim: 80 },
            model_config: OnlineModelConfig {
                transducer: Transducer { encoder: enc.as_ptr(), decoder: dec.as_ptr(), joiner: joi.as_ptr() },
                paraformer: Paraformer { encoder: null, decoder: null },
                zipformer2_ctc: OneModel { model: null },
                tokens: tok.as_ptr(),
                num_threads: 1,
                provider: provider.as_ptr(),
                debug: 0,
                model_type: null,
                modeling_unit: null,
                bpe_vocab: null,
                tokens_buf: null,
                tokens_buf_size: 0,
                nemo_ctc: OneModel { model: null },
                t_one_ctc: OneModel { model: null },
            },
            max_active_paths: 4,
            num_trailing_blanks: 1,
            keywords_score: 2.0,
            keywords_threshold: 0.25,
            keywords_file: null,
            keywords_buf: kw.as_ptr(),
            keywords_buf_size: KEYWORDS.len() as i32,
        };
        let spotter = unsafe { SherpaOnnxCreateKeywordSpotter(&cfg) };
        if spotter.is_null() {
            return Err("sherpa-onnx 建立 KWS 失敗".into());
        }
        let stream = unsafe { SherpaOnnxCreateKeywordStream(spotter) };
        if stream.is_null() {
            unsafe { SherpaOnnxDestroyKeywordSpotter(spotter) };
            return Err("sherpa-onnx 建立 KWS stream 失敗".into());
        }
        Ok(Spotter { spotter, stream })
    }
}

impl engine::platform::Spotter for Spotter {
    fn accept(&mut self, block: &[f32]) -> bool {
        unsafe {
            SherpaOnnxOnlineStreamAcceptWaveform(self.stream, SR as i32, block.as_ptr(), block.len() as i32);
            let mut hit = false;
            while SherpaOnnxIsKeywordStreamReady(self.spotter, self.stream) != 0 {
                SherpaOnnxDecodeKeywordStream(self.spotter, self.stream);
                let r = SherpaOnnxGetKeywordResult(self.spotter, self.stream);
                if !r.is_null() {
                    let kw = (*r).keyword;
                    if !kw.is_null() && *kw != 0 {
                        hit = true;
                    }
                    SherpaOnnxDestroyKeywordResult(r);
                }
                if hit {
                    SherpaOnnxResetKeywordStream(self.spotter, self.stream);
                    break;
                }
            }
            hit
        }
    }

    fn reset(&mut self) {
        unsafe { SherpaOnnxResetKeywordStream(self.spotter, self.stream) };
    }
}

impl Drop for Spotter {
    fn drop(&mut self) {
        unsafe {
            SherpaOnnxDestroyOnlineStream(self.stream);
            SherpaOnnxDestroyKeywordSpotter(self.spotter);
        }
    }
}

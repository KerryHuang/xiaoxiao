# 第三方授權

| 元件 | 用途 | 授權 | 來源 |
|---|---|---|---|
| sherpa-onnx（含 onnxruntime 預編 library） | 喚醒詞偵測 | Apache-2.0（onnxruntime：MIT） | https://github.com/k2-fsa/sherpa-onnx |
| sherpa-onnx-kws-zipformer-zh-en-3M-2025-12-20 | 喚醒詞模型 | Apache-2.0 | https://github.com/k2-fsa/sherpa-onnx/releases/tag/kws-models |
| whisper.cpp | 語音轉文字 | MIT | https://github.com/ggerganov/whisper.cpp |
| Whisper ggml 模型（small、large-v3-turbo q5_0） | 語音轉文字模型 | MIT | https://huggingface.co/ggerganov/whisper.cpp |
| Microsoft Visual C++ runtime（vcruntime140、vcruntime140_1、msvcp140，隨 Windows 版安裝） | C++ 執行環境 | Microsoft Visual C++ Redistributable 授權，可隨應用程式散布 | https://visualstudio.microsoft.com/license-terms/ |

模型與 library 不放進 repo，由 `scripts/fetch-models.sh` 下載並以 SHA256 驗證。

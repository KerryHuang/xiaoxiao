#!/usr/bin/env bash
# 用 macOS `say` 產生測試用合成語音（16 kHz 單聲道 wav）。
set -euo pipefail
cd "$(dirname "$0")/.."
out=crates/engine/tests/fixtures
mkdir -p "$out"
gen() { say -v Meijia -o "$out/$1" --file-format=WAVE --data-format=LEI16@16000 --channels=1 "$2"; }
gen wake.wav "我們先討論這個。嘿，小小。今天天氣怎麼樣？"
gen no-wake-1.wav "這個小小的問題先放著。"
gen no-wake-2.wav "可以再思考一下。"
gen question.wav "一公尺等於幾公分？"
echo "fixtures 已產生：$out"

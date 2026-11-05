#!/bin/sh
# 測試用假 claude：行為由 FAKE_CLAUDE 決定。
delta() { printf '{"type":"stream_event","event":{"delta":{"type":"text_delta","text":"%s"}}}\n' "$1"; }
result() { printf '{"type":"result","is_error":false,"result":"%s"}\n' "$1"; }
case "$FAKE_CLAUDE" in
  ok)
    while read -r _line; do delta "一公斤是"; delta "1000 公克。"; result "一公斤是1000 公克。"; done ;;
  garbage)
    while read -r _line; do echo "not json"; delta "一公斤是"; delta "1000 公克。"; result "一公斤是1000 公克。"; done ;;
  crash)
    read -r _line; delta "一公斤"; exit 1 ;;
  nologin)
    read -r _line; echo '{"type":"result","is_error":true,"result":"Please run /login"}' ;;
  slow)
    sleep 40 ;;
  args)
    for a in "$@"; do printf '%s\n' "$a"; done > "$FAKE_ARGS_OUT"
    echo '{"type":"result","is_error":false,"result":"ok"}' ;;
esac

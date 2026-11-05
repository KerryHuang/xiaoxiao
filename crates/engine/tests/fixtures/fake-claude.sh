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
  action)
    # 快答回 <<ACTION>>；確認對話（系統提示含「幫使用者做事」）第一輪複述、之後回 <<DO>>
    case "$*" in
      *幫使用者做事*)
        n=0
        while read -r _line; do
          n=$((n+1))
          if [ "$n" = 1 ]; then result "好，明天早上九點提醒你開會，對嗎？"
          else echo '{"type":"result","is_error":false,"result":"<<DO>>{\"type\":\"reminder\",\"at\":\"2099-01-01T09:00:00+08:00\",\"text\":\"開會\"}"}'
          fi
        done ;;
      *) while read -r _line; do delta "<<ACTION>>"; result "<<ACTION>>"; done ;;
    esac ;;
  slow)
    sleep 40 ;;
  args)
    for a in "$@"; do printf '%s\n' "$a"; done > "$FAKE_ARGS_OUT"
    echo '{"type":"result","is_error":false,"result":"ok"}' ;;
esac

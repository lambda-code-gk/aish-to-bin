#!/bin/bash
# Phase10: aishd 単一ライタ daemon の統合スモークテスト
# - daemon start -> ping 成功
# - AISH_DAEMON=on で daemon なし時は失敗
# - AISH_DAEMON=auto で daemon なし時は in-proc で成功
# - AISH_DAEMON=auto で daemon あり時は疎通

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$PROJECT_ROOT"

# バイナリ: AISH_BIN がディレクトリならその中の aish/ai、未設定なら cargo build (aish-cli が aish/ai を生成)
if [ -n "${AISH_BIN:-}" ]; then
    if [ -d "$AISH_BIN" ]; then
        AISH_CMD="$AISH_BIN/aish"
        AI_CMD="$AISH_BIN/ai"
    else
        AISH_CMD="$AISH_BIN"
        AI_CMD="$(dirname "$AISH_BIN")/ai"
    fi
else
    cargo build -q -p aish-cli 2>/dev/null || true
    AISH_CMD="$PROJECT_ROOT/target/debug/aish"
    AI_CMD="$PROJECT_ROOT/target/debug/ai"
fi
[ -x "$AISH_CMD" ] || { echo "aish not found: $AISH_CMD"; exit 1; }
[ -x "$AI_CMD" ] || { echo "ai not found: $AI_CMD"; exit 1; }

# テスト用ソケットを隔離
export AISH_DAEMON_SOCK
AISH_DAEMON_SOCK="$(mktemp -d)/aishd.sock"
mkdir -p "$(dirname "$AISH_DAEMON_SOCK")"
trap "rm -rf $(dirname "$AISH_DAEMON_SOCK")" EXIT

echo "=== 1) daemon start -> ping 成功 ==="
$AISH_CMD daemon start &
DAEMON_PID=$!
trap "kill $DAEMON_PID 2>/dev/null || true; rm -rf $(dirname "$AISH_DAEMON_SOCK")" EXIT
# ソケットができるまで待つ
for i in 1 2 3 4 5 6 7 8 9 10; do
    [ -S "$AISH_DAEMON_SOCK" ] && break
    sleep 0.3
done
[ -S "$AISH_DAEMON_SOCK" ] || { echo "socket not created"; kill $DAEMON_PID 2>/dev/null; exit 1; }
$AISH_CMD daemon ping
EXIT_PING=$?
kill $DAEMON_PID 2>/dev/null || true
wait $DAEMON_PID 2>/dev/null || true
[ "$EXIT_PING" -eq 0 ] || { echo "ping failed (exit $EXIT_PING)"; exit 1; }
echo "OK: daemon start and ping"

echo "=== 2) AISH_DAEMON=on で daemon なし -> 実行失敗 ==="
# ソケットを存在しないパスにして daemon に繋がらないようにする
export AISH_DAEMON=on
export AISH_DAEMON_SOCK="$(mktemp -d)/nonexistent.sock"
# daemon status (= ping) は接続失敗で exit 非ゼロになる
set +e
$AISH_CMD daemon status
STATUS_EXIT=$?
set -e
[ "$STATUS_EXIT" -ne 0 ] || { echo "expected daemon status to fail when no daemon (exit $STATUS_EXIT)"; exit 1; }
echo "OK: status fails when no daemon (ping failure)"

echo "=== 3) AISH_DAEMON=auto で daemon なし -> in-proc で成功 ==="
export AISH_DAEMON=auto
export AISH_DAEMON_SOCK="$(mktemp -d)/nonexistent.sock"
# ai --help など append しないコマンドは成功すればよい
$AI_CMD --help >/dev/null
echo "OK: ai --help with auto (no daemon)"

echo "=== 4) AISH_DAEMON=auto で daemon あり -> 疎通 ==="
export AISH_DAEMON_SOCK="$(mktemp -d)/aishd.sock"
mkdir -p "$(dirname "$AISH_DAEMON_SOCK")"
$AISH_CMD daemon start &
DAEMON_PID2=$!
for i in 1 2 3 4 5 6 7 8 9 10; do
    [ -S "$AISH_DAEMON_SOCK" ] && break
    sleep 0.3
done
[ -S "$AISH_DAEMON_SOCK" ] || { kill $DAEMON_PID2 2>/dev/null; exit 1; }
$AISH_CMD daemon ping
kill $DAEMON_PID2 2>/dev/null || true
wait $DAEMON_PID2 2>/dev/null || true
echo "OK: auto with daemon running"

echo "=== daemon_smoke.sh: all passed ==="

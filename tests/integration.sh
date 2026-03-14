#!/bin/bash
# プロジェクト全体の結合テストを実行するスクリプト

set -euo pipefail

# 色付き出力のための変数
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# プロジェクトルートの取得
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# テスト用のディレクトリ
TEST_DIR=$(mktemp -d)
trap "rm -rf $TEST_DIR" EXIT

# ビルドモード（デフォルトはrelease）
BUILD_MODE="${BUILD_MODE:-release}"
TARGET_DIR="$BUILD_MODE"

# バイナリ配置ディレクトリ（AISH_BIN 対応 + 新CLI前提, P8-7）
# AISH_BIN 未設定時は dist/bin を使い、なければ xtask dist で用意する
resolve_bin_dir() {
    local bin_dir
    if [ -n "${AISH_BIN:-}" ]; then
        if [ -d "$AISH_BIN" ]; then
            bin_dir="$AISH_BIN"
        else
            bin_dir="$(dirname "$AISH_BIN")"
        fi
    else
        bin_dir="$PROJECT_ROOT/dist/bin"
        # dist/bin が既にあっても、ソース変更後は古い可能性があるため常に再生成する
        log_info "Running xtask dist..." >&2
        (cd "$PROJECT_ROOT" && cargo run -p xtask -- dist $([ "$BUILD_MODE" = "debug" ] && echo "--debug")) >&2 || return 1
    fi
    if [ ! -f "$bin_dir/aish" ]; then
        log_error "aish binary not found in $bin_dir (set AISH_BIN to use another path)"
        return 1
    fi
    if [ ! -f "$bin_dir/ai" ]; then
        log_error "ai binary not found in $bin_dir (set AISH_BIN to use another path)"
        return 1
    fi
    echo "$bin_dir"
}

# テスト結果のカウント
TESTS_PASSED=0
TESTS_FAILED=0
FAILED_TESTS=()

log_info() {
    echo -e "${GREEN}[INFO]${NC} $*"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $*"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $*"
}

test_case() {
    local name="$1"
    echo ""
    echo "========================================="
    echo "Test: $name"
    echo "========================================="
}

assert_exit_code() {
    local expected="$1"
    local actual="$2"
    if [ "$actual" -eq "$expected" ]; then
        return 0
    else
        log_error "Exit code mismatch: expected $expected, got $actual"
        return 1
    fi
}

# バイナリをビルドする関数
build_binary() {
    local project_name="$1"
    local project_path="$2"
    local binary_name="$3"
    
    log_info "Building $project_name..." >&2
    
    if [ ! -d "$project_path" ]; then
        log_error "Project directory not found: $project_path" >&2
        return 1
    fi
    
    if [ ! -f "$project_path/Cargo.toml" ]; then
        log_error "Cargo.toml not found: $project_path/Cargo.toml" >&2
        return 1
    fi
    
    cd "$project_path"
    if [ "$BUILD_MODE" == "debug" ]; then
        cargo build >&2
    else
        cargo build --release >&2
    fi
    
    cd "$PROJECT_ROOT"
    
    local binary_path="$project_path/target/$TARGET_DIR/$binary_name"
    if [ ! -f "$binary_path" ]; then
        log_error "Binary not found after build: $binary_path" >&2
        return 1
    fi
    
    echo "$binary_path"
}

# aiコマンドの結合テスト（AISH_BIN/dist の ai を使用）
test_ai_binary() {
    test_case "ai command integration test"
    
    local binary_path="${AI_BIN_PATH:?}"
    
    log_info "Binary path: $binary_path"
    
    # テスト1: 引数なしで実行した場合、適切なエラーメッセージが表示されること
    log_info "Test 1: Error handling (no query)"
    if env -u AISH_SESSION -u AISH_HOME "$binary_path" > "$TEST_DIR/ai_test1.stdout" 2> "$TEST_DIR/ai_test1.stderr"; then
        log_error "✗ Expected error for no query, but command succeeded"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("ai (should fail without query)")
        return 1
    else
        local exit_code=$?
        if [ $exit_code -eq 64 ]; then
            log_info "✓ Correctly failed with no query (exit code: $exit_code)"
        else
            log_error "✗ Expected exit code 64, got $exit_code"
            cat "$TEST_DIR/ai_test1.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai (wrong exit code)")
            return 1
        fi
    fi
    
    # テスト2: エラーハンドリング（存在しないオプション）
    log_info "Test 2: Error handling (invalid option)"
    if env -u AISH_SESSION -u AISH_HOME "$binary_path" --unknown-option > "$TEST_DIR/ai_test2.stdout" 2> "$TEST_DIR/ai_test2.stderr"; then
        log_error "✗ Expected error for unknown option, but command succeeded"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("ai (should fail with unknown option)")
        return 1
    else
        local exit_code=$?
        if [ $exit_code -eq 64 ]; then
            log_info "✓ Correctly failed with unknown option (exit code: $exit_code)"
        else
            log_error "✗ Expected exit code 64, got $exit_code"
            cat "$TEST_DIR/ai_test2.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai (wrong exit code for unknown option)")
            return 1
        fi
    fi
    
    log_info "ai integration test PASSED"
    TESTS_PASSED=$((TESTS_PASSED + 1))
    return 0
}

# aishコマンドの結合テスト（AISH_BIN/dist の aish を使用）
test_aish_binary() {
    test_case "aish command integration test"
    
    local binary_path="${AISH_BIN_PATH:?}"
    
    log_info "Binary path: $binary_path"
    
    # テスト用のホームディレクトリを作成
    local test_home_dir="$TEST_DIR/aish_home"
    mkdir -p "$test_home_dir"
    
    # テスト1: バイナリが実行できること（パイプで入力を与えてシェルが動作し、正常終了することを確認）
    log_info "Test 1: Binary execution with pipe input"
    local test_output
    # シェルが終了するように、最後にexitを送る
    # 既存のAISH関連環境変数やPROMPT_COMMANDをクリアしてクリーンな環境でテスト
    if test_output=$(printf 'echo test\nexit\n' | env -u AISH_SESSION -u AISH_HOME -u AISH_PID -u PROMPT_COMMAND "$binary_path" -d "$test_home_dir" 2> "$TEST_DIR/aish_test1.stderr"); then
        local exit_code=$?
        if echo "$test_output" | grep -q "test"; then
            log_info "✓ Binary executed successfully and shell output is correct (exit code: $exit_code)"
        else
            log_error "✗ Shell output is incorrect. Expected 'test', got: $test_output"
            cat "$TEST_DIR/aish_test1.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("aish (shell output incorrect)")
            return 1
        fi
    else
        local exit_code=$?
        log_error "✗ Binary execution failed (exit code: $exit_code)"
        cat "$TEST_DIR/aish_test1.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish (execution failed)")
        return 1
    fi
    
    # テスト2: エラーハンドリング（存在しないオプション）
    # 64 = EX_USAGE (legacy), 2 = clap default (新CLI)
    log_info "Test 2: Error handling (invalid option)"
    if env -u AISH_SESSION -u AISH_HOME "$binary_path" -d "$test_home_dir" --invalid-option 2> "$TEST_DIR/aish_test2.stderr"; then
        log_error "✗ Should have failed with invalid option"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish (error handling)")
        return 1
    else
        local exit_code=$?
        if [ $exit_code -eq 64 ] || [ $exit_code -eq 2 ]; then
            log_info "✓ Correctly failed with invalid option (exit code: $exit_code)"
        else
            log_error "✗ Wrong exit code for invalid option: expected 64 or 2, got $exit_code"
            cat "$TEST_DIR/aish_test2.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("aish (error handling)")
            return 1
        fi
    fi

    # テスト3: デフォルトで2回起動すると別セッションになる（同居しない）
    log_info "Test 3: Two launches use different session dirs (no mixing)"
    local session1 session2
    # AISH_SESSION と AISH_HOME をクリアして、-d オプションのみで動作確認
    # state/session/ を含むパスを抽出（プロンプトや改行の有無に依存しない）
    session1=$(printf 'echo "$AISH_SESSION"\nexit\n' | env -u AISH_SESSION -u AISH_HOME "$binary_path" -d "$test_home_dir" 2> "$TEST_DIR/aish_test3a.stderr" | tr -d '\r' | grep -oE '/[^[:space:]]*state/session/[^[:space:]]+' | head -1 || true)
    session2=$(printf 'echo "$AISH_SESSION"\nexit\n' | env -u AISH_SESSION -u AISH_HOME "$binary_path" -d "$test_home_dir" 2> "$TEST_DIR/aish_test3b.stderr" | tr -d '\r' | grep -oE '/[^[:space:]]*state/session/[^[:space:]]+' | head -1 || true)
    if [ -z "$session1" ] || [ -z "$session2" ]; then
        log_error "✗ Could not get AISH_SESSION from runs"
        [ -f "$TEST_DIR/aish_test3a.stderr" ] && log_error "Run 1 stderr:" && cat "$TEST_DIR/aish_test3a.stderr"
        [ -f "$TEST_DIR/aish_test3b.stderr" ] && log_error "Run 2 stderr:" && cat "$TEST_DIR/aish_test3b.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish (session dir not printed)")
        return 1
    fi
    if [ "$session1" = "$session2" ]; then
        log_error "✗ Two launches shared the same session dir (expected unique): $session1"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish (sessions should not mix by default)")
        return 1
    fi
    log_info "✓ Two launches used different session dirs"

    # テスト4: -s 指定で同一セッションに入れる（再開）
    log_info "Test 4: -s specifies same session dir (resume)"
    local resume_dir="$test_home_dir/state/session/resume_test"
    mkdir -p "$resume_dir"
    printf '2\n' > "$resume_dir/session_schema_version"
    local out4
    # AISH_SESSION と AISH_HOME をクリアして、-s オプションで動作確認
    out4=$(printf 'echo "$AISH_SESSION"\nexit\n' | env -u AISH_SESSION -u AISH_HOME "$binary_path" -d "$test_home_dir" -s "$resume_dir" 2> "$TEST_DIR/aish_test4.stderr")
    local got_session
    got_session=$(echo "$out4" | tr -d '\r' | grep -oE '/[^[:space:]]*state/session/[^[:space:]]+' | head -1)
    if [ -z "$got_session" ]; then
        log_error "✗ Could not get AISH_SESSION with -s"
        cat "$TEST_DIR/aish_test4.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish (-s session dir)")
        return 1
    fi
    # 正規化して比較（got_session はセッションディレクトリのフルパス）
    local want_canon got_canon
    want_canon=$(cd "$resume_dir" && pwd)
    got_canon=$(cd "$got_session" 2>/dev/null && pwd)
    if [ -z "$got_canon" ] || [ "$got_canon" != "$want_canon" ]; then
        log_error "✗ -s session dir mismatch: got $got_canon, want $want_canon"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish (-s should use specified dir)")
        return 1
    fi
    if [ ! -f "$resume_dir/shell_attachment.json" ]; then
        log_error "✗ shell attachment metadata was not created"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish shell attachment metadata missing")
        return 1
    fi
    if ! grep -q '"status": "detached"' "$resume_dir/shell_attachment.json"; then
        log_error "✗ shell attachment metadata did not transition to detached"
        cat "$resume_dir/shell_attachment.json"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish shell attachment metadata detached")
        return 1
    fi
    log_info "✓ -s uses specified session dir for resume"

    log_info "Test 4.1: shell status separates shell attachment from console log"
    local shell_status_output
    if shell_status_output=$(env -u AISH_SESSION -u AISH_HOME "$binary_path" -d "$test_home_dir" -s "$resume_dir" shell status 2>"$TEST_DIR/aish_shell_status.stderr"); then
        grep -q '^\[session\]$' <<<"$shell_status_output" || {
            log_error "✗ shell status missing session section"
            echo "$shell_status_output"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("aish shell status session section")
            return 1
        }
        grep -q '^\[shell attachment\]$' <<<"$shell_status_output" || {
            log_error "✗ shell status missing shell attachment section"
            echo "$shell_status_output"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("aish shell status attachment section")
            return 1
        }
        grep -q '^\[console log\]$' <<<"$shell_status_output" || {
            log_error "✗ shell status missing console log section"
            echo "$shell_status_output"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("aish shell status console section")
            return 1
        }
        grep -q $'^status\tdetached$' <<<"$shell_status_output" || {
            log_error "✗ shell status missing detached state"
            echo "$shell_status_output"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("aish shell status detached")
            return 1
        }
        grep -Eq '^part_file_count[[:space:]][0-9]+$' <<<"$shell_status_output" || {
            log_error "✗ shell status missing part file count"
            echo "$shell_status_output"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("aish shell status part file count")
            return 1
        }
        log_info "✓ shell status separates attachment and console log state"
    else
        local exit_code=$?
        log_error "✗ shell status failed (exit code: $exit_code)"
        cat "$TEST_DIR/aish_shell_status.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish shell status")
        return 1
    fi

    # テスト5: plugins/tools list（deny-by-default + enabled で tools に出る）
    test_case "aish plugins/tools list (Phase9 MCP bridge)"
    local socket_path="$TEST_DIR/aish_read.sock"
    env AISH_DAEMON_SOCK="$socket_path" "$binary_path" daemon start \
        > "$TEST_DIR/aish_read_daemon.stdout" \
        2> "$TEST_DIR/aish_read_daemon.stderr" &
    local daemon_pid=$!
    trap 'kill "$daemon_pid" 2>/dev/null || true' RETURN
    local ready=0
    for _ in $(seq 1 20); do
        if env AISH_DAEMON_SOCK="$socket_path" "$binary_path" daemon ping >/dev/null 2>&1; then
            ready=1
            break
        fi
        sleep 0.2
    done
    if [ "$ready" -ne 1 ]; then
        log_error "✗ aish read backend daemon did not become ready"
        [ -f "$TEST_DIR/aish_read_daemon.stderr" ] && cat "$TEST_DIR/aish_read_daemon.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish read backend daemon startup")
        return 1
    fi
    local proj_dir="$TEST_DIR/plugin_project"
    mkdir -p "$proj_dir/.aish/plugins/dummy"
    local plugin_py="$proj_dir/.aish/plugins/dummy/dummy_plugin.py"
    cat > "$plugin_py" <<'PY'
import json, sys, time
def reply(req_id, result=None, error=None):
    if error is not None:
        out = {"jsonrpc":"2.0","id":req_id,"error":error}
    else:
        out = {"jsonrpc":"2.0","id":req_id,"result":result}
    sys.stdout.write(json.dumps(out) + "\n")
    sys.stdout.flush()

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    req = json.loads(line)
    mid = req.get("method","")
    rid = req.get("id",0)
    if mid == "initialize":
        reply(rid, {"ok": True})
    elif mid == "list_tools":
        reply(rid, [
            {"name":"echo","description":"Dummy echo tool","input_schema":{"type":"object","properties":{"message":{"type":"string"}}}}
        ])
    elif mid == "call_tool":
        params = req.get("params") or {}
        name = params.get("name","")
        args = params.get("arguments")
        if name == "sleep":
            time.sleep(2.0)
        reply(rid, {"content":{"tool":name,"arguments":args}})
    else:
        reply(rid, None, {"code":-32601,"message":"method not found"})
PY
    local plugin_toml="$proj_dir/.aish/plugins/dummy/plugin.toml"
    cat > "$plugin_toml" <<EOF
id = "dummy"
namespace = "dummy"
display_name = "Dummy Plugin"
command = "python3"
args = ["$plugin_py"]
enabled = false
timeout_ms = 500
EOF

    log_info "Test 5.1: plugins list shows disabled plugin"
    local out_plugins
    out_plugins=$(cd "$proj_dir" && env -u AISH_SESSION -u AISH_HOME AISH_DAEMON_SOCK="$socket_path" "$binary_path" -d "$test_home_dir" plugins list)
    echo "$out_plugins" | grep -q "disabled[[:space:]]\+dummy" || {
        log_error "✗ plugins list did not include disabled dummy plugin"
        echo "$out_plugins"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish plugins list")
        return 1
    }
    log_info "✓ plugins list includes dummy (disabled)"

    log_info "Test 5.2: tools list does not include tools when disabled"
    local out_tools_disabled
    out_tools_disabled=$(cd "$proj_dir" && env -u AISH_SESSION -u AISH_HOME AISH_DAEMON_SOCK="$socket_path" "$binary_path" -d "$test_home_dir" tools list)
    if echo "$out_tools_disabled" | grep -q "dummy\\.echo"; then
        log_error "✗ tools list included dummy.echo while plugin disabled"
        echo "$out_tools_disabled"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish tools list (disabled)")
        return 1
    fi
    log_info "✓ tools list excludes dummy tools when disabled"

    log_info "Test 5.3: tools list includes tools when enabled=true"
    cat > "$plugin_toml" <<EOF
id = "dummy"
namespace = "dummy"
display_name = "Dummy Plugin"
command = "python3"
args = ["$plugin_py"]
enabled = true
timeout_ms = 500
EOF
    local out_tools_enabled
    out_tools_enabled=$(cd "$proj_dir" && env -u AISH_SESSION -u AISH_HOME AISH_DAEMON_SOCK="$socket_path" "$binary_path" -d "$test_home_dir" tools list)
    echo "$out_tools_enabled" | grep -q "dummy\\.echo" || {
        log_error "✗ tools list did not include dummy.echo when enabled"
        echo "$out_tools_enabled"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish tools list (enabled)")
        return 1
    }
    log_info "✓ tools list includes dummy.echo when enabled"

    mkdir -p "$proj_dir/.aish/plugins/broken"
    cat > "$proj_dir/.aish/plugins/broken/plugin.toml" <<EOF
id = "broken"
namespace = "broken"
display_name = "Broken Plugin"
command = "python3"
args = ["$TEST_DIR/does-not-exist.py"]
enabled = true
timeout_ms = 500
EOF

    log_info "Test 5.4: tools list degrades gracefully when one plugin is broken"
    local out_tools_with_broken
    out_tools_with_broken=$(cd "$proj_dir" && env -u AISH_SESSION -u AISH_HOME AISH_DAEMON_SOCK="$socket_path" "$binary_path" -d "$test_home_dir" tools list 2>"$TEST_DIR/aish_tools_list_broken.stderr")
    echo "$out_tools_with_broken" | grep -q "dummy\\.echo" || {
        log_error "✗ tools list lost healthy tools when one plugin was broken"
        echo "$out_tools_with_broken"
        cat "$TEST_DIR/aish_tools_list_broken.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish tools list (broken plugin)")
        return 1
    }
    grep -q "warning: failed to list tools for plugin 'broken'" "$TEST_DIR/aish_tools_list_broken.stderr" || {
        log_error "✗ tools list did not warn about broken plugin"
        cat "$TEST_DIR/aish_tools_list_broken.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish tools list (broken plugin warning)")
        return 1
    }
    log_info "✓ tools list keeps healthy tools when one plugin is broken"
    env AISH_DAEMON_SOCK="$socket_path" "$binary_path" daemon stop >/dev/null 2>&1 || true

    log_info "aish integration test PASSED"
    TESTS_PASSED=$((TESTS_PASSED + 1))
    return 0
}

test_ai_backend_binary() {
    test_case "ai backend integration test"

    local ai_binary_path="${AI_BIN_PATH:?}"
    local aish_binary_path="${AISH_BIN_PATH:?}"
    local test_home_dir="$TEST_DIR/ai_backend_home"
    local socket_path="$TEST_DIR/aishd.sock"
    mkdir -p "$test_home_dir"

    log_info "Starting backend daemon"
    env AISH_DAEMON_SOCK="$socket_path" "$aish_binary_path" daemon start \
        > "$TEST_DIR/ai_backend_daemon.stdout" \
        2> "$TEST_DIR/ai_backend_daemon.stderr" &
    local daemon_pid=$!
    trap 'kill "$daemon_pid" 2>/dev/null || true' RETURN

    local ready=0
    for _ in $(seq 1 20); do
        if env AISH_DAEMON_SOCK="$socket_path" "$aish_binary_path" daemon ping >/dev/null 2>&1; then
            ready=1
            break
        fi
        sleep 0.2
    done
    if [ "$ready" -ne 1 ]; then
        log_error "✗ backend daemon did not become ready"
        [ -f "$TEST_DIR/ai_backend_daemon.stderr" ] && cat "$TEST_DIR/ai_backend_daemon.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("ai backend daemon startup")
        return 1
    fi

    log_info "Test 1: ai query uses backend daemon"
    local query_output
    if query_output=$(env -u AISH_SESSION -u AISH_JOB_DEPTH -u AISH_FRONTEND_TTY AISH_FORCE_BACKEND=1 AISH_DAEMON_SOCK="$socket_path" AISH_HOME="$test_home_dir" "$ai_binary_path" -p echo "backend hello" 2>"$TEST_DIR/ai_backend_query.stderr"); then
        grep -q "\[Echo Provider\] Query: user message: backend hello" <<<"$query_output" || {
            log_error "✗ ai backend query output did not contain echo query details"
            echo "$query_output"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai backend query output")
            return 1
        }
        log_info "✓ ai query succeeded via backend"
    else
        local exit_code=$?
        log_error "✗ ai backend query failed (exit code: $exit_code)"
        cat "$TEST_DIR/ai_backend_query.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("ai backend query")
        return 1
    fi

    mkdir -p "$test_home_dir/config/task.d"
    cat > "$test_home_dir/config/task.d/hello-task.sh" <<'SH'
#!/bin/sh
echo "task stdout: $1"
echo "task stderr: $1" >&2
SH
    chmod +x "$test_home_dir/config/task.d/hello-task.sh"

    cat > "$test_home_dir/config/task.d/nested-task.sh" <<SH
#!/bin/sh
"$ai_binary_path" -p echo "nested hello from task"
SH
    chmod +x "$test_home_dir/config/task.d/nested-task.sh"

    cat > "$test_home_dir/config/task.d/read-stdin-task.sh" <<'SH'
#!/bin/sh
IFS= read -r line
echo "stdin from task: $line"
SH
    chmod +x "$test_home_dir/config/task.d/read-stdin-task.sh"

    cat > "$test_home_dir/config/task.d/slow-task.sh" <<'SH'
#!/bin/sh
sleep 30
echo "slow task finished"
SH
    chmod +x "$test_home_dir/config/task.d/slow-task.sh"

    cat > "$test_home_dir/config/task.d/nested-slow-task.sh" <<SH
#!/bin/sh
"$ai_binary_path" slow-task
SH
    chmod +x "$test_home_dir/config/task.d/nested-slow-task.sh"

    log_info "Test 1.1: ai task output is forwarded from backend to frontend"
    local task_stdout
    if task_stdout=$(env -u AISH_SESSION -u AISH_JOB_DEPTH -u AISH_FRONTEND_TTY AISH_FORCE_BACKEND=1 AISH_DAEMON_SOCK="$socket_path" AISH_HOME="$test_home_dir" "$ai_binary_path" hello-task forwarded 2>"$TEST_DIR/ai_backend_task.stderr"); then
        grep -q "task stdout: forwarded" <<<"$task_stdout" || {
            log_error "✗ ai backend task stdout was not forwarded to frontend"
            echo "$task_stdout"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai backend task stdout")
            return 1
        }
        grep -q "task stderr: forwarded" "$TEST_DIR/ai_backend_task.stderr" || {
            log_error "✗ ai backend task stderr was not forwarded to frontend stderr"
            cat "$TEST_DIR/ai_backend_task.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai backend task stderr")
            return 1
        }
        log_info "✓ ai task output was forwarded via backend"
    else
        local exit_code=$?
        log_error "✗ ai backend task failed (exit code: $exit_code)"
        cat "$TEST_DIR/ai_backend_task.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("ai backend task execution")
        return 1
    fi

    log_info "Test 1.2: ai task can nest ai via backend without deadlock"
    local nested_stdout
    if nested_stdout=$(env -u AISH_SESSION -u AISH_JOB_DEPTH -u AISH_FRONTEND_TTY AISH_FORCE_BACKEND=1 AISH_DAEMON_SOCK="$socket_path" AISH_HOME="$test_home_dir" "$ai_binary_path" nested-task 2>"$TEST_DIR/ai_backend_nested_task.stderr"); then
        grep -q "\[Echo Provider\] Query: user message: nested hello from task" <<<"$nested_stdout" || {
            log_error "✗ nested ai task did not forward nested backend output"
            echo "$nested_stdout"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai backend nested task output")
            return 1
        }
        grep -q "started nested job_id=" "$TEST_DIR/ai_backend_nested_task.stderr" || {
            log_error "✗ nested ai task did not label nested backend start"
            cat "$TEST_DIR/ai_backend_nested_task.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai backend nested task start label")
            return 1
        }
        grep -q "finished nested job_id=" "$TEST_DIR/ai_backend_nested_task.stderr" || {
            log_error "✗ nested ai task did not label nested backend completion"
            cat "$TEST_DIR/ai_backend_nested_task.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai backend nested task completion label")
            return 1
        }
        log_info "✓ ai task nested ai via backend"
    else
        local exit_code=$?
        log_error "✗ ai nested backend task failed (exit code: $exit_code)"
        cat "$TEST_DIR/ai_backend_nested_task.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("ai backend nested task execution")
        return 1
    fi

    log_info "Test 1.2.1: ai nested backend task fails when nesting depth limit is exceeded"
    if env -u AISH_SESSION -u AISH_JOB_DEPTH -u AISH_FRONTEND_TTY AISH_FORCE_BACKEND=1 AISH_DAEMON_SOCK="$socket_path" AISH_HOME="$test_home_dir" AISH_MAX_BACKEND_JOB_DEPTH=0 \
        "$ai_binary_path" nested-task >"$TEST_DIR/ai_backend_nested_depth.stdout" 2>"$TEST_DIR/ai_backend_nested_depth.stderr"; then
        log_error "✗ ai nested backend task unexpectedly succeeded despite depth limit"
        cat "$TEST_DIR/ai_backend_nested_depth.stdout"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("ai backend nested depth limit")
        return 1
    else
        local exit_code=$?
        if [ "$exit_code" -eq 0 ]; then
            log_error "✗ ai nested backend task returned zero despite depth limit"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai backend nested depth exit code")
            return 1
        fi
        grep -q "nested ai depth 1 exceeds limit 0" "$TEST_DIR/ai_backend_nested_depth.stderr" || {
            log_error "✗ ai nested backend task did not report depth limit"
            cat "$TEST_DIR/ai_backend_nested_depth.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai backend nested depth stderr")
            return 1
        }
        log_info "✓ ai nested backend task enforced depth limit"
    fi

    log_info "Test 1.3: ai backend task preserves stdin semantics"
    local stdin_task_stdout
    if stdin_task_stdout=$(printf 'pipe value\n' | env AISH_DAEMON_SOCK="$socket_path" AISH_HOME="$test_home_dir" "$ai_binary_path" read-stdin-task 2>"$TEST_DIR/ai_backend_stdin_task.stderr"); then
        grep -q "stdin from task: pipe value" <<<"$stdin_task_stdout" || {
            log_error "✗ ai backend task did not receive piped stdin"
            echo "$stdin_task_stdout"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai backend task stdin")
            return 1
        }
        log_info "✓ ai backend task preserved stdin"
    else
        local exit_code=$?
        log_error "✗ ai backend stdin task failed (exit code: $exit_code)"
        cat "$TEST_DIR/ai_backend_stdin_task.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("ai backend stdin task execution")
        return 1
    fi

    log_info "Test 1.4: aish daemon cancel interrupts a running ai backend job"
    local cancel_job_id="cancel-job-1"
    local cancel_session_dir="$TEST_DIR/ai_backend_cancel_session"
    mkdir -p "$cancel_session_dir"
    printf '2\n' > "$cancel_session_dir/session_schema_version"
    env AISH_DAEMON_SOCK="$socket_path" AISH_HOME="$test_home_dir" AISH_SESSION="$cancel_session_dir" AISH_BACKEND_JOB_ID="$cancel_job_id" AISH_FORCE_BACKEND=1 \
        "$ai_binary_path" slow-task >"$TEST_DIR/ai_backend_cancel.stdout" 2>"$TEST_DIR/ai_backend_cancel.stderr" &
    local cancel_ai_pid=$!
    sleep 1
    local active_jobs_output
    if active_jobs_output=$(env AISH_DAEMON_SOCK="$socket_path" "$aish_binary_path" daemon jobs --active 2>"$TEST_DIR/aish_daemon_jobs_active.stderr"); then
        grep -q '^\[active jobs\]$' <<<"$active_jobs_output" || {
            log_error "✗ aish daemon jobs did not show active section while job was running"
            echo "$active_jobs_output"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("aish daemon jobs active section")
            kill "$cancel_ai_pid" 2>/dev/null || true
            wait "$cancel_ai_pid" 2>/dev/null || true
            return 1
        }
        grep -q "$cancel_job_id" <<<"$active_jobs_output" && grep -q "running" <<<"$active_jobs_output" || {
            log_error "✗ aish daemon jobs did not show running active job"
            echo "$active_jobs_output"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("aish daemon jobs active entry")
            kill "$cancel_ai_pid" 2>/dev/null || true
            wait "$cancel_ai_pid" 2>/dev/null || true
            return 1
        }
    else
        local exit_code=$?
        log_error "✗ aish daemon jobs failed while listing active jobs (exit code: $exit_code)"
        cat "$TEST_DIR/aish_daemon_jobs_active.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish daemon jobs active")
        kill "$cancel_ai_pid" 2>/dev/null || true
        wait "$cancel_ai_pid" 2>/dev/null || true
        return 1
    fi
    local cancel_output
    if cancel_output=$(env AISH_DAEMON_SOCK="$socket_path" "$aish_binary_path" daemon cancel "$cancel_job_id" 2>"$TEST_DIR/aish_daemon_cancel.stderr"); then
        grep -q "cancel requested for $cancel_job_id" <<<"$cancel_output" || {
            log_error "✗ daemon cancel did not report success"
            echo "$cancel_output"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("daemon cancel output")
            kill "$cancel_ai_pid" 2>/dev/null || true
            wait "$cancel_ai_pid" 2>/dev/null || true
            return 1
        }
    else
        local exit_code=$?
        log_error "✗ daemon cancel command failed (exit code: $exit_code)"
        cat "$TEST_DIR/aish_daemon_cancel.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("daemon cancel command")
        kill "$cancel_ai_pid" 2>/dev/null || true
        wait "$cancel_ai_pid" 2>/dev/null || true
        return 1
    fi
    local cancel_exit_code=0
    if wait "$cancel_ai_pid"; then
        cancel_exit_code=0
    else
        cancel_exit_code=$?
    fi
    if [ "$cancel_exit_code" -ne 130 ]; then
        log_error "✗ cancelled ai backend job exited with $cancel_exit_code instead of 130"
        cat "$TEST_DIR/ai_backend_cancel.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("ai backend cancel exit code")
        return 1
    fi
    if grep -q "slow task finished" "$TEST_DIR/ai_backend_cancel.stdout"; then
        log_error "✗ cancelled ai backend job still completed task output"
        cat "$TEST_DIR/ai_backend_cancel.stdout"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("ai backend cancel output")
        return 1
    fi
    local cancel_events_file="$cancel_session_dir/events.jsonl"
    if [ -f "$cancel_events_file" ]; then
        grep -q "\"job_id\":\"$cancel_job_id\"" "$cancel_events_file" || {
            log_error "✗ cancelled ai backend job did not persist cancel lifecycle"
            cat "$cancel_events_file"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai backend cancel lifecycle payload")
            return 1
        }
        grep -q '"state":"cancelled"' "$cancel_events_file" || {
            log_error "✗ cancelled ai backend job did not persist cancelled state"
            cat "$cancel_events_file"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai backend cancel lifecycle state")
            return 1
        }
        if env AISH_HOME="$test_home_dir" "$aish_binary_path" -s "$cancel_session_dir" daemon jobs --persisted >"$TEST_DIR/aish_daemon_jobs_cancelled.stdout" 2>"$TEST_DIR/aish_daemon_jobs_cancelled.stderr"; then
            grep -q "$cancel_job_id" "$TEST_DIR/aish_daemon_jobs_cancelled.stdout" && \
            grep -q "cancelled" "$TEST_DIR/aish_daemon_jobs_cancelled.stdout" && \
            grep -q "exit=130" "$TEST_DIR/aish_daemon_jobs_cancelled.stdout" || {
                log_error "✗ aish daemon jobs did not show cancelled lifecycle state"
                cat "$TEST_DIR/aish_daemon_jobs_cancelled.stdout"
                TESTS_FAILED=$((TESTS_FAILED + 1))
                FAILED_TESTS+=("aish daemon jobs cancelled")
                return 1
            }
        else
            local exit_code=$?
            log_error "✗ aish daemon jobs failed for cancelled session (exit code: $exit_code)"
            cat "$TEST_DIR/aish_daemon_jobs_cancelled.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("aish daemon jobs cancelled command")
            return 1
        fi
    fi
    log_info "✓ daemon cancel interrupted ai backend job"

    log_info "Test 1.4.1: daemon cancel cascades to nested backend child jobs"
    local nested_cancel_parent_job_id="cancel-parent-job-1"
    env AISH_DAEMON_SOCK="$socket_path" AISH_HOME="$test_home_dir" AISH_BACKEND_JOB_ID="$nested_cancel_parent_job_id" AISH_FORCE_BACKEND=1 \
        "$ai_binary_path" nested-slow-task >"$TEST_DIR/ai_backend_nested_cancel.stdout" 2>"$TEST_DIR/ai_backend_nested_cancel.stderr" &
    local nested_cancel_ai_pid=$!
    sleep 2
    local nested_active_jobs_output
    if nested_active_jobs_output=$(env AISH_DAEMON_SOCK="$socket_path" "$aish_binary_path" daemon jobs --active 2>"$TEST_DIR/aish_daemon_jobs_nested_active.stderr"); then
        grep -q "$nested_cancel_parent_job_id" <<<"$nested_active_jobs_output" && grep -q "running" <<<"$nested_active_jobs_output" || {
            log_error "✗ nested cancel setup did not show running parent job"
            echo "$nested_active_jobs_output"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("nested cancel parent active job")
            kill "$nested_cancel_ai_pid" 2>/dev/null || true
            wait "$nested_cancel_ai_pid" 2>/dev/null || true
            return 1
        }
        grep -q "child parent=$nested_cancel_parent_job_id" <<<"$nested_active_jobs_output" || {
            log_error "✗ nested cancel setup did not show running child job"
            echo "$nested_active_jobs_output"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("nested cancel child active job")
            kill "$nested_cancel_ai_pid" 2>/dev/null || true
            wait "$nested_cancel_ai_pid" 2>/dev/null || true
            return 1
        }
    else
        local exit_code=$?
        log_error "✗ aish daemon jobs failed while preparing nested cancel test (exit code: $exit_code)"
        cat "$TEST_DIR/aish_daemon_jobs_nested_active.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("nested cancel active jobs command")
        kill "$nested_cancel_ai_pid" 2>/dev/null || true
        wait "$nested_cancel_ai_pid" 2>/dev/null || true
        return 1
    fi
    if ! env AISH_DAEMON_SOCK="$socket_path" "$aish_binary_path" daemon cancel "$nested_cancel_parent_job_id" >"$TEST_DIR/aish_daemon_nested_cancel.stdout" 2>"$TEST_DIR/aish_daemon_nested_cancel.stderr"; then
        local exit_code=$?
        log_error "✗ daemon cancel failed for nested parent job (exit code: $exit_code)"
        cat "$TEST_DIR/aish_daemon_nested_cancel.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("nested cancel command")
        kill "$nested_cancel_ai_pid" 2>/dev/null || true
        wait "$nested_cancel_ai_pid" 2>/dev/null || true
        return 1
    fi
    local nested_cancel_exit_code=0
    if wait "$nested_cancel_ai_pid"; then
        nested_cancel_exit_code=0
    else
        nested_cancel_exit_code=$?
    fi
    if [ "$nested_cancel_exit_code" -ne 130 ]; then
        log_error "✗ nested cancelled parent job exited with $nested_cancel_exit_code instead of 130"
        cat "$TEST_DIR/ai_backend_nested_cancel.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("nested cancel parent exit code")
        return 1
    fi
    sleep 1
    if nested_active_jobs_output=$(env AISH_DAEMON_SOCK="$socket_path" "$aish_binary_path" daemon jobs --active 2>"$TEST_DIR/aish_daemon_jobs_nested_active_after.stderr"); then
        if grep -q "$nested_cancel_parent_job_id" <<<"$nested_active_jobs_output"; then
            log_error "✗ nested parent/child job remained active after cancelling parent"
            echo "$nested_active_jobs_output"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("nested cancel active subtree remained")
            return 1
        fi
    else
        local exit_code=$?
        log_error "✗ aish daemon jobs failed after nested cancel (exit code: $exit_code)"
        cat "$TEST_DIR/aish_daemon_jobs_nested_active_after.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("nested cancel active jobs after")
        return 1
    fi
    log_info "✓ daemon cancel cascaded to nested backend child jobs"

    log_info "Test 1.4.2: frontend SIGINT cancels backend job subtree"
    local sigint_job_id="sigint-job-1"
    local sigint_session_dir="$TEST_DIR/ai_backend_sigint_session"
    mkdir -p "$sigint_session_dir"
    printf '2\n' > "$sigint_session_dir/session_schema_version"
    env AISH_DAEMON_SOCK="$socket_path" AISH_HOME="$test_home_dir" AISH_SESSION="$sigint_session_dir" AISH_BACKEND_JOB_ID="$sigint_job_id" AISH_FORCE_BACKEND=1 \
        "$ai_binary_path" nested-slow-task >"$TEST_DIR/ai_backend_sigint.stdout" 2>"$TEST_DIR/ai_backend_sigint.stderr" &
    local sigint_ai_pid=$!
    sleep 2
    kill -INT "$sigint_ai_pid" 2>/dev/null || true
    local sigint_exit_code=0
    if wait "$sigint_ai_pid"; then
        sigint_exit_code=0
    else
        sigint_exit_code=$?
    fi
    if [ "$sigint_exit_code" -ne 130 ]; then
        log_error "✗ frontend SIGINT job exited with $sigint_exit_code instead of 130"
        cat "$TEST_DIR/ai_backend_sigint.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("frontend SIGINT exit code")
        return 1
    fi
    if grep -q "slow task finished" "$TEST_DIR/ai_backend_sigint.stdout"; then
        log_error "✗ frontend SIGINT still allowed nested slow task to finish"
        cat "$TEST_DIR/ai_backend_sigint.stdout"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("frontend SIGINT slow task output")
        return 1
    fi
    if [ -f "$sigint_session_dir/events.jsonl" ]; then
        grep -q "\"job_id\":\"$sigint_job_id\"" "$sigint_session_dir/events.jsonl" || {
            log_error "✗ frontend SIGINT did not persist lifecycle for parent job"
            cat "$sigint_session_dir/events.jsonl"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("frontend SIGINT lifecycle payload")
            return 1
        }
        grep -q '"state":"cancelled"' "$sigint_session_dir/events.jsonl" || {
            log_error "✗ frontend SIGINT did not persist cancelled state"
            cat "$sigint_session_dir/events.jsonl"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("frontend SIGINT lifecycle state")
            return 1
        }
    fi
    log_info "✓ frontend SIGINT cancelled backend job subtree"

    log_info "Test 1.5: ai backend job lifecycle is persisted to events.jsonl"
    local lifecycle_session_dir="$TEST_DIR/ai_backend_lifecycle_session"
    local lifecycle_job_id="lifecycle-job-1"
    mkdir -p "$lifecycle_session_dir"
    printf '2\n' > "$lifecycle_session_dir/session_schema_version"
    if env AISH_FORCE_BACKEND=1 AISH_DAEMON_SOCK="$socket_path" AISH_HOME="$test_home_dir" AISH_SESSION="$lifecycle_session_dir" \
        AISH_BACKEND_JOB_ID="$lifecycle_job_id" "$ai_binary_path" -p echo "persist lifecycle" \
        >"$TEST_DIR/ai_backend_lifecycle.stdout" 2>"$TEST_DIR/ai_backend_lifecycle.stderr"; then
        local events_file="$lifecycle_session_dir/events.jsonl"
        [ -f "$events_file" ] || {
            log_error "✗ ai backend lifecycle did not create events.jsonl"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai backend lifecycle events file")
            return 1
        }
        grep -q '"kind":"job.lifecycle"' "$events_file" || {
            log_error "✗ ai backend lifecycle event kind was not persisted"
            cat "$events_file"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai backend lifecycle kind")
            return 1
        }
        grep -q "\"run_id\":\"$lifecycle_job_id\"" "$events_file" || {
            log_error "✗ ai backend lifecycle run_id was not persisted"
            cat "$events_file"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai backend lifecycle run_id")
            return 1
        }
        grep -q "\"job_id\":\"$lifecycle_job_id\"" "$events_file" || {
            log_error "✗ ai backend lifecycle payload job_id was not persisted"
            cat "$events_file"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai backend lifecycle payload job_id")
            return 1
        }
        grep -q '"state":"queued"' "$events_file" || {
            log_error "✗ ai backend lifecycle queued state was not persisted"
            cat "$events_file"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai backend lifecycle queued")
            return 1
        }
        grep -q '"state":"running"' "$events_file" || {
            log_error "✗ ai backend lifecycle running state was not persisted"
            cat "$events_file"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai backend lifecycle running")
            return 1
        }
        grep -q '"state":"completed"' "$events_file" || {
            log_error "✗ ai backend lifecycle completed state was not persisted"
            cat "$events_file"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai backend lifecycle completed")
            return 1
        }
        log_info "✓ ai backend lifecycle was persisted to events.jsonl"
    else
        local exit_code=$?
        log_error "✗ ai backend lifecycle query failed (exit code: $exit_code)"
        cat "$TEST_DIR/ai_backend_lifecycle.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("ai backend lifecycle query")
        return 1
    fi

    log_info "Test 1.6: aish daemon jobs shows persisted lifecycle section"
    local jobs_output
    if jobs_output=$(env AISH_HOME="$test_home_dir" "$aish_binary_path" -s "$lifecycle_session_dir" daemon jobs --persisted 2>"$TEST_DIR/aish_daemon_jobs.stderr"); then
        grep -q '^\[persisted lifecycle\]$' <<<"$jobs_output" || {
            log_error "✗ aish daemon jobs did not show persisted section"
            echo "$jobs_output"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("aish daemon jobs persisted section")
            return 1
        }
        grep -q "$lifecycle_job_id" <<<"$jobs_output" && grep -q "completed" <<<"$jobs_output" && grep -q "exit=0" <<<"$jobs_output" || {
            log_error "✗ aish daemon jobs did not show completed lifecycle state"
            echo "$jobs_output"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("aish daemon jobs output")
            return 1
        }
        log_info "✓ aish daemon jobs showed persisted lifecycle state"
    else
        local exit_code=$?
        log_error "✗ aish daemon jobs failed (exit code: $exit_code)"
        cat "$TEST_DIR/aish_daemon_jobs.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish daemon jobs")
        return 1
    fi

    log_info "Test 2: ai dry-run uses backend daemon"
    local dry_run_output
    if dry_run_output=$(env -u AISH_SESSION -u AISH_JOB_DEPTH -u AISH_FRONTEND_TTY AISH_FORCE_BACKEND=1 AISH_DAEMON_SOCK="$socket_path" AISH_HOME="$test_home_dir" "$ai_binary_path" --dry-run -p echo "dry run hello" 2>"$TEST_DIR/ai_backend_dry_run.stderr"); then
        grep -q "=== ai dry run ===" <<<"$dry_run_output" || {
            log_error "✗ ai backend dry-run missing header"
            echo "$dry_run_output"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai backend dry-run header")
            return 1
        }
        grep -q "dry run hello" <<<"$dry_run_output" || {
            log_error "✗ ai backend dry-run missing query content"
            echo "$dry_run_output"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai backend dry-run output")
            return 1
        }
        log_info "✓ ai dry-run succeeded via backend"
    else
        local exit_code=$?
        log_error "✗ ai backend dry-run failed (exit code: $exit_code)"
        cat "$TEST_DIR/ai_backend_dry_run.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("ai backend dry-run")
        return 1
    fi

    local shell_session_dir="$TEST_DIR/ai_backend_shell_session"
    mkdir -p "$shell_session_dir"
    printf '2\n' > "$shell_session_dir/session_schema_version"
    cat > "$shell_session_dir/console.txt" <<'EOF'
$ cargo test
running tests...
test result: ok
EOF

    log_info "Test 2.1: ai dry-run reads shell console snapshot from session dir"
    local shell_context_output
    if shell_context_output=$(env AISH_FORCE_BACKEND=1 AISH_DAEMON_SOCK="$socket_path" AISH_HOME="$test_home_dir" AISH_SESSION="$shell_session_dir" "$ai_binary_path" --dry-run -p echo "shell context hello" 2>"$TEST_DIR/ai_backend_shell_context.stderr"); then
        grep -q "Context: shell console" <<<"$shell_context_output" || {
            log_error "✗ ai backend dry-run missing shell console addon"
            echo "$shell_context_output"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai backend shell console addon header")
            return 1
        }
        grep -q "running tests" <<<"$shell_context_output" || {
            log_error "✗ ai backend dry-run missing shell console content"
            echo "$shell_context_output"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai backend shell console addon content")
            return 1
        }
        log_info "✓ ai dry-run included shell console snapshot"
    else
        local exit_code=$?
        log_error "✗ ai backend shell console dry-run failed (exit code: $exit_code)"
        cat "$TEST_DIR/ai_backend_shell_context.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("ai backend shell console dry-run")
        return 1
    fi

    log_info "Test 3: ai list-profiles uses backend daemon"
    local list_profiles_output
    if list_profiles_output=$(env AISH_DAEMON_SOCK="$socket_path" AISH_HOME="$test_home_dir" "$ai_binary_path" --list-profiles 2>"$TEST_DIR/ai_backend_list_profiles.stderr"); then
        grep -q "echo" <<<"$list_profiles_output" || {
            log_error "✗ ai backend list-profiles missing expected profile"
            echo "$list_profiles_output"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai backend list-profiles output")
            return 1
        }
        log_info "✓ ai list-profiles succeeded via backend"
    else
        local exit_code=$?
        log_error "✗ ai backend list-profiles failed (exit code: $exit_code)"
        cat "$TEST_DIR/ai_backend_list_profiles.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("ai backend list-profiles")
        return 1
    fi

    log_info "Test 4: ai policy-explain uses backend daemon"
    local policy_output
    if policy_output=$(env AISH_DAEMON_SOCK="$socket_path" AISH_HOME="$test_home_dir" "$ai_binary_path" --policy-explain 2>"$TEST_DIR/ai_backend_policy.stderr"); then
        grep -q '"resolved"' <<<"$policy_output" || {
            log_error "✗ ai backend policy-explain missing resolved payload"
            echo "$policy_output"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai backend policy-explain output")
            return 1
        }
        log_info "✓ ai policy-explain succeeded via backend"
    else
        local exit_code=$?
        log_error "✗ ai backend policy-explain failed (exit code: $exit_code)"
        cat "$TEST_DIR/ai_backend_policy.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("ai backend policy-explain")
        return 1
    fi

    kill "$daemon_pid" 2>/dev/null || true
    wait "$daemon_pid" 2>/dev/null || true
    trap - RETURN

    log_info "ai backend integration test PASSED"
    TESTS_PASSED=$((TESTS_PASSED + 1))
    return 0
}

test_daemon_sigint() {
    test_case "aish daemon SIGINT shutdown"

    local binary_path="${AISH_BIN_PATH:?}"
    local socket_path="$TEST_DIR/aishd-sigint.sock"

    log_info "Test 6: daemon start exits on SIGINT"
    if bash -lc '
        set -e
        binary_path="$1"
        socket_path="$2"
        stdout_path="$3"
        stderr_path="$4"
        env AISH_DAEMON_SOCK="$socket_path" "$binary_path" daemon start >"$stdout_path" 2>"$stderr_path" &
        pid=$!
        sleep 1
        kill -INT "$pid"
        for _ in $(seq 1 20); do
            if ! kill -0 "$pid" 2>/dev/null; then
                wait "$pid" || true
                exit 0
            fi
            sleep 0.2
        done
        kill -TERM "$pid" 2>/dev/null || true
        wait "$pid" || true
        exit 1
    ' bash "$binary_path" "$socket_path" "$TEST_DIR/aish_daemon_sigint.stdout" "$TEST_DIR/aish_daemon_sigint.stderr"; then
        log_info "✓ daemon terminated on SIGINT"
    else
        log_error "✗ daemon did not stop on SIGINT"
        [ -f "$TEST_DIR/aish_daemon_sigint.stderr" ] && cat "$TEST_DIR/aish_daemon_sigint.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish daemon SIGINT")
        return 1
    fi

    if [ -S "$socket_path" ]; then
        log_error "✗ daemon socket remained after SIGINT shutdown"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish daemon SIGINT socket cleanup")
        return 1
    fi

    log_info "aish daemon SIGINT test PASSED"
    TESTS_PASSED=$((TESTS_PASSED + 1))
    return 0
}

test_daemon_stop() {
    test_case "aish daemon stop command"

    local binary_path="${AISH_BIN_PATH:?}"
    local socket_path="$TEST_DIR/aishd-stop.sock"

    log_info "Test 7: daemon stop terminates foreground daemon"
    env AISH_DAEMON_SOCK="$socket_path" "$binary_path" daemon start \
        >"$TEST_DIR/aish_daemon_stop.stdout" \
        2>"$TEST_DIR/aish_daemon_stop.stderr" &
    local daemon_pid=$!
    trap 'kill "$daemon_pid" 2>/dev/null || true' RETURN

    local ready=0
    for _ in $(seq 1 20); do
        if env AISH_DAEMON_SOCK="$socket_path" "$binary_path" daemon ping >/dev/null 2>&1; then
            ready=1
            break
        fi
        sleep 0.2
    done
    if [ "$ready" -ne 1 ]; then
        log_error "✗ daemon did not become ready for stop test"
        [ -f "$TEST_DIR/aish_daemon_stop.stderr" ] && cat "$TEST_DIR/aish_daemon_stop.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish daemon stop startup")
        return 1
    fi

    local stop_output
    if stop_output=$(env AISH_DAEMON_SOCK="$socket_path" "$binary_path" daemon stop 2>"$TEST_DIR/aish_daemon_stop_cmd.stderr"); then
        grep -q "daemon stopped" <<<"$stop_output" || {
            log_error "✗ daemon stop did not print confirmation"
            echo "$stop_output"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("aish daemon stop output")
            return 1
        }
    else
        local exit_code=$?
        log_error "✗ daemon stop failed (exit code: $exit_code)"
        [ -f "$TEST_DIR/aish_daemon_stop_cmd.stderr" ] && cat "$TEST_DIR/aish_daemon_stop_cmd.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish daemon stop")
        return 1
    fi

    wait "$daemon_pid" 2>/dev/null || true
    trap - RETURN

    if [ -S "$socket_path" ]; then
        log_error "✗ daemon socket remained after stop command"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish daemon stop socket cleanup")
        return 1
    fi

    log_info "aish daemon stop test PASSED"
    TESTS_PASSED=$((TESTS_PASSED + 1))
    return 0
}

test_aish_memory_remove_backend() {
    test_case "aish memory remove via backend"

    local binary_path="${AISH_BIN_PATH:?}"
    local socket_path="$TEST_DIR/aishd-memory-remove.sock"
    local test_home_dir="$TEST_DIR/aish_memory_remove_home"
    local proj_dir="$TEST_DIR/aish_memory_remove_project"
    mkdir -p "$proj_dir/.aish/memory/entries"
    mkdir -p "$test_home_dir/data/memory/entries"

    cat > "$proj_dir/.aish/memory/metadata.json" <<'JSON'
{
  "memories": [
    {
      "id": "project-mem",
      "category": "note",
      "keywords": [],
      "subject": "Project memory",
      "timestamp": "2026-03-15T00:00:00Z"
    }
  ],
  "last_updated": 0
}
JSON
    cat > "$proj_dir/.aish/memory/entries/project-mem.json" <<'JSON'
{"id":"project-mem","category":"note","subject":"Project memory","content":"project content","timestamp":"2026-03-15T00:00:00Z","keywords":[]}
JSON

    log_info "Starting backend daemon for memory remove"
    env AISH_DAEMON_SOCK="$socket_path" "$binary_path" daemon start \
        >"$TEST_DIR/aish_memory_remove_daemon.stdout" \
        2>"$TEST_DIR/aish_memory_remove_daemon.stderr" &
    local daemon_pid=$!
    trap 'kill "$daemon_pid" 2>/dev/null || true' RETURN

    local ready=0
    for _ in $(seq 1 20); do
        if env AISH_DAEMON_SOCK="$socket_path" "$binary_path" daemon ping >/dev/null 2>&1; then
            ready=1
            break
        fi
        sleep 0.2
    done
    if [ "$ready" -ne 1 ]; then
        log_error "✗ backend daemon did not become ready for memory remove"
        [ -f "$TEST_DIR/aish_memory_remove_daemon.stderr" ] && cat "$TEST_DIR/aish_memory_remove_daemon.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish memory remove daemon startup")
        return 1
    fi

    log_info "Test 8: memory remove works via backend"
    if (cd "$proj_dir" && env AISH_DAEMON_SOCK="$socket_path" "$binary_path" -d "$test_home_dir" memory remove project-mem >"$TEST_DIR/aish_memory_remove.stdout" 2>"$TEST_DIR/aish_memory_remove.stderr"); then
        if [ -e "$proj_dir/.aish/memory/entries/project-mem.json" ]; then
            log_error "✗ memory remove via backend did not remove entry file"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("aish memory remove backend file")
            return 1
        fi
        grep -q '"id": "project-mem"' "$proj_dir/.aish/memory/metadata.json" && {
            log_error "✗ memory remove via backend did not update metadata"
            cat "$proj_dir/.aish/memory/metadata.json"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("aish memory remove backend metadata")
            return 1
        }
        log_info "✓ aish memory remove succeeded via backend"
    else
        local exit_code=$?
        log_error "✗ aish memory remove via backend failed (exit code: $exit_code)"
        cat "$TEST_DIR/aish_memory_remove.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish memory remove backend")
        return 1
    fi

    env AISH_DAEMON_SOCK="$socket_path" "$binary_path" daemon stop >/dev/null 2>&1 || true
    wait "$daemon_pid" 2>/dev/null || true
    trap - RETURN

    log_info "aish memory remove backend test PASSED"
    TESTS_PASSED=$((TESTS_PASSED + 1))
    return 0
}

test_aish_read_without_daemon() {
    test_case "aish read commands without daemon"

    local binary_path="${AISH_BIN_PATH:?}"
    local test_home_dir="$TEST_DIR/aish_read_no_daemon_home"
    local proj_dir="$TEST_DIR/aish_read_no_daemon_project"
    mkdir -p "$proj_dir/.aish/plugins/localonly"
    cat > "$proj_dir/.aish/plugins/localonly/plugin.toml" <<'EOF'
id = "localonly"
namespace = "localonly"
display_name = "Local Only Plugin"
command = "python3"
args = ["missing-local-only.py"]
enabled = false
timeout_ms = 500
EOF

    log_info "Test 8: plugins list falls back locally without daemon"
    local plugins_output
    if plugins_output=$(cd "$proj_dir" && env -u AISH_DAEMON_SOCK "$binary_path" -d "$test_home_dir" plugins list 2>"$TEST_DIR/aish_plugins_list_no_daemon.stderr"); then
        grep -q "localonly" <<<"$plugins_output" || {
            log_error "✗ aish plugins list without daemon did not show local plugin"
            echo "$plugins_output"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("aish plugins list without daemon output")
            return 1
        }
        log_info "✓ aish plugins list falls back locally without daemon"
    else
        local exit_code=$?
        log_error "✗ aish plugins list without daemon failed (exit code: $exit_code)"
        cat "$TEST_DIR/aish_plugins_list_no_daemon.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish plugins list without daemon")
        return 1
    fi

    mkdir -p "$proj_dir/.aish/memory/entries"
    cat > "$proj_dir/.aish/memory/metadata.json" <<'JSON'
{
  "memories": [
    {
      "id": "local-mem",
      "category": "note",
      "keywords": [],
      "subject": "Local fallback memory",
      "timestamp": "2026-03-15T00:00:00Z"
    }
  ],
  "last_updated": 0
}
JSON
    cat > "$proj_dir/.aish/memory/entries/local-mem.json" <<'JSON'
{"id":"local-mem","category":"note","subject":"Local fallback memory","content":"local content","timestamp":"2026-03-15T00:00:00Z","keywords":[]}
JSON

    log_info "Test 9: memory remove falls back locally without daemon"
    if (cd "$proj_dir" && env -u AISH_DAEMON_SOCK "$binary_path" -d "$test_home_dir" memory remove local-mem >"$TEST_DIR/aish_memory_remove_no_daemon.stdout" 2>"$TEST_DIR/aish_memory_remove_no_daemon.stderr"); then
        if [ -e "$proj_dir/.aish/memory/entries/local-mem.json" ]; then
            log_error "✗ aish memory remove without daemon did not remove entry file"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("aish memory remove without daemon file")
            return 1
        fi
        grep -q '"id": "local-mem"' "$proj_dir/.aish/memory/metadata.json" && {
            log_error "✗ aish memory remove without daemon did not update metadata"
            cat "$proj_dir/.aish/memory/metadata.json"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("aish memory remove without daemon metadata")
            return 1
        }
        log_info "✓ aish memory remove falls back locally without daemon"
    else
        local exit_code=$?
        log_error "✗ aish memory remove without daemon failed (exit code: $exit_code)"
        cat "$TEST_DIR/aish_memory_remove_no_daemon.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish memory remove without daemon")
        return 1
    fi

    log_info "aish read fallback test PASSED"
    TESTS_PASSED=$((TESTS_PASSED + 1))
    return 0
}

# メイン実行
main() {
    echo "========================================="
    echo "Integration Test Suite"
    echo "========================================="
    echo "Project root: $PROJECT_ROOT"
    echo "Build mode: $BUILD_MODE"
    echo "Test directory: $TEST_DIR"
    echo ""
    
    # バイナリ配置を解決（AISH_BIN または dist/bin）
    local bin_dir
    if ! bin_dir=$(resolve_bin_dir); then
        log_error "Could not resolve binary directory"
        exit 1
    fi
    export AI_BIN_PATH="$bin_dir/ai"
    export AISH_BIN_PATH="$bin_dir/aish"
    log_info "Using ai: $AI_BIN_PATH, aish: $AISH_BIN_PATH"
    echo ""
    
    # 結合テストを実行
    log_info "Running integration tests..."
    
    # aiコマンドのテスト
    test_ai_binary || true

    # ai backend 経路のテスト
    test_ai_backend_binary || true
    
    # aishコマンドのテスト
    test_aish_binary || true

    # daemon の SIGINT 終了テスト
    test_daemon_sigint || true

    # daemon の stop コマンド終了テスト
    test_daemon_stop || true

    # memory remove の backend 経路
    test_aish_memory_remove_backend || true

    # daemon 未起動時の read fallback
    test_aish_read_without_daemon || true
    
    # 結果サマリー
    echo ""
    echo "========================================="
    echo "Test Summary"
    echo "========================================="
    echo "Passed: $TESTS_PASSED"
    echo "Failed: $TESTS_FAILED"
    echo "Total:  $((TESTS_PASSED + TESTS_FAILED))"
    
    if [ ${#FAILED_TESTS[@]} -gt 0 ]; then
        echo ""
        log_error "Failed tests:"
        for failed_test in "${FAILED_TESTS[@]}"; do
            echo "  - $failed_test"
        done
    fi
    
    if [ $TESTS_FAILED -eq 0 ]; then
        echo ""
        log_info "All integration tests passed! ✓"
        exit 0
    else
        echo ""
        log_error "Some integration tests failed. ✗"
        exit 1
    fi
}

main "$@"

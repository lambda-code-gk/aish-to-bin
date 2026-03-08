#!/bin/bash

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TASK_SCRIPT="$PROJECT_ROOT/assets/defaults/config/task.d/evolve/execute"

TEST_DIR="$(mktemp -d)"
trap 'rm -rf "$TEST_DIR"' EXIT

fail() {
    echo "[FAIL] $*" >&2
    exit 1
}

assert_contains() {
    local file="$1"
    local needle="$2"
    if ! grep -Fq "$needle" "$file"; then
        echo "--- $file ---" >&2
        cat "$file" >&2
        fail "expected '$needle' in $file"
    fi
}

assert_not_contains() {
    local file="$1"
    local needle="$2"
    if grep -Fq "$needle" "$file"; then
        echo "--- $file ---" >&2
        cat "$file" >&2
        fail "did not expect '$needle' in $file"
    fi
}

make_fake_ai() {
    local body="$1"
    mkdir -p "$TEST_DIR/bin"
    cat > "$TEST_DIR/bin/ai" <<EOF
#!/bin/bash
set -euo pipefail
$body
EOF
    chmod +x "$TEST_DIR/bin/ai"
}

test_propose_surfaces_ai_failure_without_parse_noise() {
    make_fake_ai 'echo "HTTP request failed: dns error" >&2
exit 1'

    local stdout_file="$TEST_DIR/fail.stdout"
    local stderr_file="$TEST_DIR/fail.stderr"
    set +e
    PATH="$TEST_DIR/bin:$PATH" AISH_SESSION="$TEST_DIR/session" bash "$TASK_SCRIPT" "request" >"$stdout_file" 2>"$stderr_file"
    local status=$?
    set -e

    [ "$status" -eq 1 ] || fail "expected exit 1, got $status"
    assert_contains "$stdout_file" "HTTP request failed: dns error"
    assert_contains "$stderr_file" "evolve proposal failed while running ai -M evolve-plan"
    assert_not_contains "$stdout_file" "failed to parse evolve output"
    assert_not_contains "$stderr_file" "failed to parse evolve output"
}

test_propose_saves_extracted_blocks_on_success() {
    make_fake_ai 'cat <<'"'"'EOF'"'"'
<<<EVOLVE_PLAN_JSON_BEGIN>>>
{"summary":"s","rationale":"r","changes":[],"verification_commands":[]}
<<<EVOLVE_PLAN_JSON_END>>>

<<<EVOLVE_MESSAGE_BEGIN>>>
hello
<<<EVOLVE_MESSAGE_END>>>
EOF'

    local stdout_file="$TEST_DIR/success.stdout"
    local stderr_file="$TEST_DIR/success.stderr"
    PATH="$TEST_DIR/bin:$PATH" AISH_SESSION="$TEST_DIR/session" bash "$TASK_SCRIPT" "request" >"$stdout_file" 2>"$stderr_file"

    assert_contains "$stdout_file" "hello"
    assert_contains "$TEST_DIR/session/evolve/proposal.json" '"summary":"s"'
    assert_contains "$TEST_DIR/session/evolve/proposal.txt" "hello"
}

test_propose_surfaces_ai_failure_without_parse_noise
test_propose_saves_extracted_blocks_on_success

echo "[PASS] task_evolve.sh"

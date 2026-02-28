#!/bin/bash
# プロジェクト全体のテストを実行するスクリプト

set -euo pipefail

# 色付き出力のための変数
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# プロジェクトルートの取得
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

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

# Rustプロジェクトのテストを実行する関数（workspace 対応: -p でパッケージ指定）
run_rust_test() {
    local project_name="$1"
    local package_name="$2"
    
    echo ""
    echo "========================================="
    echo "Running: $project_name (cargo test -p $package_name)"
    echo "========================================="
    
    cd "$PROJECT_ROOT"
    if cargo test -p "$package_name"; then
        log_info "✓ $project_name PASSED"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        log_error "✗ $project_name FAILED"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("$project_name")
        return 1
    fi
}

# メイン実行
main() {
    echo "========================================="
    echo "Running All Tests"
    echo "========================================="
    echo "Project root: $PROJECT_ROOT"
    echo ""
    
    # Rustプロジェクトのテストを実行
    log_info "Running Rust project tests..."
    
    # core/ai (workspace package name: ai)
    run_rust_test "core/ai" "ai" || true
    
    # core/aish (workspace package name: aish)
    run_rust_test "core/aish" "aish" || true
    
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
        log_info "All tests passed! ✓"
        exit 0
    else
        echo ""
        log_error "Some tests failed. ✗"
        exit 1
    fi
}

main "$@"


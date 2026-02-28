#!/usr/bin/env bash
set -e

# デバッグビルドかどうかを判定
BUILD_MODE="release"
TARGET_DIR="release"

if [[ "$1" == "--debug" ]] || [[ "$1" == "-d" ]]; then
    BUILD_MODE="debug"
    TARGET_DIR="debug"
    echo "Building in DEBUG mode..."
else
    echo "Building in RELEASE mode..."
fi

# プロジェクトルートを取得
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$SCRIPT_DIR"

# 成果物は dist/bin に配置（repo 汚染防止）
BIN_DIR="$PROJECT_ROOT/dist/bin"
mkdir -p "$BIN_DIR"

# ビルドコマンドを決定
if [[ "$BUILD_MODE" == "debug" ]]; then
    BUILD_CMD="cargo build"
else
    BUILD_CMD="cargo build --release"
fi

# 簡易アーキテクチャチェック
"$SCRIPT_DIR/tests/architecture.sh"
rg "crate::adapter" core/**/src/usecase && (echo "illigal dependency" ; exit 1)
rg "crate::cli" core/**/src/usecase && (echo "illigal dependency" ; exit 1)


# aish-captureをビルド
#echo "Building aish-capture..."
#cd "$PROJECT_ROOT/tools/aish-capture"
#$BUILD_CMD

# aish-renderをビルド
#echo "Building aish-render..."
#cd "$PROJECT_ROOT/tools/aish-render"
#$BUILD_CMD

# aish-scriptをビルド
#echo "Building aish-script..."
#cd "$PROJECT_ROOT/tools/aish-script"
#$BUILD_CMD

# ai / aish を dist/bin に配置（xtask dist がビルド＆コピー）
echo "Building ai and aish (xtask dist)..."
cd "$PROJECT_ROOT"
cargo run -p xtask -- dist $([ "$BUILD_MODE" = "debug" ] && echo "--debug")

# leakscanをビルド
echo "Building leakscan..."
cd "$PROJECT_ROOT/tools/leakscan"
$BUILD_CMD

# md-fmtをビルド
echo "Building md-fmt..."
cd "$PROJECT_ROOT/tools/md-fmt"
$BUILD_CMD

# 補助ツールを dist/bin にコピー（ワークスペースでは成果物はルート target/ に出る）
echo "Deploying extra binaries to $BIN_DIR/..."
rm -f "$BIN_DIR/aish-capture" "$BIN_DIR/aish-render" "$BIN_DIR/aish-script" "$BIN_DIR/leakscan" "$BIN_DIR/md-fmt"

copy_if_exists() {
    local src="$1"
    local name="${2:-$(basename "$src")}"
    if [ -f "$src" ]; then
        cp "$src" "$BIN_DIR/$name"
    else
        echo "  [warn] Skip $name (not built: $src)" >&2
    fi
}

copy_if_exists "$PROJECT_ROOT/target/$TARGET_DIR/leakscan" "leakscan"
copy_if_exists "$PROJECT_ROOT/target/$TARGET_DIR/md-fmt" "md-fmt"
# 以下はビルドコメントアウト中のためスキップ
# copy_if_exists "$PROJECT_ROOT/tools/aish-capture/target/$TARGET_DIR/aish-capture" "aish-capture"
# copy_if_exists "$PROJECT_ROOT/tools/aish-render/target/$TARGET_DIR/aish-render" "aish-render"
# copy_if_exists "$PROJECT_ROOT/tools/aish-script/target/$TARGET_DIR/aish-script" "aish-script"

echo "Build complete! Binaries are in $BIN_DIR/"
ls -lh "$BIN_DIR/" 2>/dev/null || true


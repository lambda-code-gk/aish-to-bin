#!/usr/bin/env bash
# AISH_HOME 用のディレクトリを作成し、assets/defaults の config をコピーする。
# 用法: scripts/mkhome.sh [ -f | --force ] [ -n | --dry-run ] [ AISH_HOME_DIR ]
#   AISH_HOME_DIR 省略時は $HOME/.aish を使用。

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ASSETS_DEFAULTS="$ROOT/assets/defaults"
DEFAULTS_CONFIG="$ASSETS_DEFAULTS/config"

usage() {
  cat <<EOF
Usage:
  $0 [ -f | --force ] [ -n | --dry-run ] [ AISH_HOME_DIR ]

Create AISH_HOME directory layout and copy config from assets/defaults into it.
  - AISH_HOME_DIR: target directory (default: \$HOME/.aish)
  -f, --force:    overwrite existing files under config/
  -n, --dry-run:  only print what would be done

Examples:
  $0                    # create \$HOME/.aish and copy defaults
  $0 /opt/aish          # create /opt/aish and copy defaults
  $0 -f ~/.aish         # overwrite existing config under ~/.aish
EOF
}

FORCE=0
DRY_RUN=0
AISH_HOME_DIR=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    -f|--force)
      FORCE=1; shift ;;
    -n|--dry-run)
      DRY_RUN=1; shift ;;
    -h|--help)
      usage; exit 0 ;;
    *)
      if [[ -z "$AISH_HOME_DIR" ]]; then
        AISH_HOME_DIR="$1"; shift
      else
        echo "Unknown arg: $1" >&2
        usage; exit 1
      fi ;;
  esac
done

if [[ -z "$AISH_HOME_DIR" ]]; then
  AISH_HOME_DIR="${HOME:-}/.aish"
fi

if [[ -z "$HOME" && "$AISH_HOME_DIR" == "/.aish" ]]; then
  echo "ERROR: HOME is not set and AISH_HOME_DIR was not given." >&2
  usage; exit 1
fi

if [[ ! -d "$DEFAULTS_CONFIG" ]]; then
  echo "ERROR: Defaults config not found: $DEFAULTS_CONFIG" >&2
  exit 1
fi

# 再帰的にコピー（既存ファイルは force でなければスキップ）
copy_tree() {
  local src="$1" dest="$2"
  local f name rel_dest
  shopt -s nullglob
  for f in "$src"/*; do
    name="${f##*/}"
    rel_dest="$dest/$name"
    if [[ -d "$f" ]]; then
      if [[ $DRY_RUN -eq 1 ]]; then
        echo "[dry-run] mkdir -p $rel_dest"
      else
        mkdir -p "$rel_dest"
      fi
      copy_tree "$f" "$rel_dest"
    else
      if [[ ! -e "$rel_dest" ]] || [[ $FORCE -eq 1 ]]; then
        if [[ $DRY_RUN -eq 1 ]]; then
          echo "[dry-run] cp $f -> $rel_dest"
        else
          mkdir -p "$(dirname "$rel_dest")"
          cp "$f" "$rel_dest"
        fi
      fi
    fi
  done
  shopt -u nullglob
}

# AISH_HOME 配下: config, data, state, cache（EnvResolver の Dirs に合わせる）
SUBDIRS="config data state cache"

if [[ $DRY_RUN -eq 1 ]]; then
  echo "[dry-run] mkdir -p $AISH_HOME_DIR/{config,data,state,cache}"
  echo "[dry-run] copy $DEFAULTS_CONFIG/* -> $AISH_HOME_DIR/config/"
else
  mkdir -p "$AISH_HOME_DIR"
  for d in $SUBDIRS; do
    mkdir -p "$AISH_HOME_DIR/$d"
  done
fi

copy_tree "$DEFAULTS_CONFIG" "$AISH_HOME_DIR/config"

echo "AISH_HOME layout created: $AISH_HOME_DIR"
echo "  config/ (from assets/defaults/config)"
echo "  data/ state/ cache/ (empty)"
echo "Set with: export AISH_HOME=$AISH_HOME_DIR"

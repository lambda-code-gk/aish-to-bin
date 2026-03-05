#!/usr/bin/env bash
set -euo pipefail

SESSION_DIR="${1:?session dir required}"

src="$SESSION_DIR/manifest.jsonl"
dst="$SESSION_DIR/reviewed_history.jsonl"

mv "$src" "$dst"
echo "[0001] renamed manifest.jsonl -> reviewed_history.jsonl"


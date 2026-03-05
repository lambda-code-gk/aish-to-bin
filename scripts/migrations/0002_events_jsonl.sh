#!/usr/bin/env bash
set -euo pipefail

SESSION_DIR="${1:?session dir required}"

src="$SESSION_DIR/events/events.ndjson"
dst="$SESSION_DIR/events.jsonl"

mv "$src" "$dst"

# remove old dir if empty (ignore failure)
rmdir "$SESSION_DIR/events" 2>/dev/null || true

echo "[0002] moved events/events.ndjson -> events.jsonl"


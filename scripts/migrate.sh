#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MIG_DIR="$ROOT/scripts/migrations"
VERSION_FILE_NAME="session_schema_version"

usage() {
  cat <<EOF
Usage:
  $0 -s <session_dir> [--dry-run] [--to <version>]
  $0 <session_dir>     [--dry-run] [--to <version>]

Notes:
  - If ${VERSION_FILE_NAME} does not exist, current version is treated as 0.
  - Migrations are applied in ascending order by the 4-digit prefix:
      scripts/migrations/0001_*.sh -> version 1
      scripts/migrations/0002_*.sh -> version 2
EOF
}

SESSION_DIR=""
DRY_RUN=0
TO_VERSION=""

# args
while [[ $# -gt 0 ]]; do
  case "$1" in
    -s|--session)
      SESSION_DIR="${2:-}"; shift 2;;
    --dry-run)
      DRY_RUN=1; shift;;
    --to)
      TO_VERSION="${2:-}"; shift 2;;
    -h|--help)
      usage; exit 0;;
    *)
      if [[ -z "$SESSION_DIR" ]]; then
        SESSION_DIR="$1"; shift
      else
        echo "Unknown arg: $1" >&2
        usage; exit 1
      fi;;
  esac
done

if [[ -z "$SESSION_DIR" ]]; then
  if [[ -n "${AISH_SESSION:-}" ]]; then
    SESSION_DIR="$AISH_SESSION"
  else
    echo "ERROR: session dir not specified. Use -s <session_dir> or set AISH_SESSION." >&2
    usage; exit 1
  fi
fi

if [[ ! -d "$SESSION_DIR" ]]; then
  echo "ERROR: session dir not found: $SESSION_DIR" >&2
  exit 1
fi

VERSION_FILE="$SESSION_DIR/$VERSION_FILE_NAME"
current=0
if [[ -f "$VERSION_FILE" ]]; then
  current="$(tr -d ' \t\r\n' < "$VERSION_FILE")"
  if [[ ! "$current" =~ ^[0-9]+$ ]]; then
    echo "ERROR: invalid $VERSION_FILE_NAME: '$current'" >&2
    exit 1
  fi
fi

# collect migrations
shopt -s nullglob
migs=( "$MIG_DIR"/[0-9][0-9][0-9][0-9]_*.sh )
shopt -u nullglob

if [[ ${#migs[@]} -eq 0 ]]; then
  echo "No migrations found in $MIG_DIR" >&2
  exit 1
fi

# compute latest as max prefix
latest=0
for m in "${migs[@]}"; do
  base="$(basename "$m")"
  prefix="${base%%_*}"
  v=$((10#$prefix))
  if (( v > latest )); then latest=$v; fi
done

target="$latest"
if [[ -n "$TO_VERSION" ]]; then
  if [[ ! "$TO_VERSION" =~ ^[0-9]+$ ]]; then
    echo "ERROR: --to must be integer" >&2
    exit 1
  fi
  target="$TO_VERSION"
fi

echo "[migrate] session=$SESSION_DIR current=$current target=$target latest=$latest"

if (( current >= target )); then
  echo "[migrate] nothing to do"
  exit 0
fi

# apply in order
IFS=$'\n' migs_sorted=($(printf '%s\n' "${migs[@]}" | sort))
unset IFS

for m in "${migs_sorted[@]}"; do
  base="$(basename "$m")"
  prefix="${base%%_*}"
  v=$((10#$prefix))

  if (( v <= current )); then
    continue
  fi
  if (( v > target )); then
    break
  fi

  expected=$((current + 1))
  if (( v != expected )); then
    echo "ERROR: missing migration. expected version $expected but found $v ($base)" >&2
    exit 1
  fi

  echo "[migrate] apply v$current -> v$v : $base"
  if (( DRY_RUN == 1 )); then
    current="$v"
    continue
  fi

  bash "$m" "$SESSION_DIR"

  printf '%s\n' "$v" > "$VERSION_FILE"
  current="$v"
done

echo "[migrate] done. now version=$current"


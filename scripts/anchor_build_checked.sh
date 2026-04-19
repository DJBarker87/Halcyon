#!/usr/bin/env bash

set -euo pipefail

if [ "$#" -lt 1 ]; then
  echo "usage: $0 <anchor-program> [<anchor-program> ...]" >&2
  exit 1
fi

tmp_log="$(mktemp -t anchor-build-checked)"
trap 'rm -f "$tmp_log"' EXIT

for program in "$@"; do
  echo "== anchor build -p $program =="
  : >"$tmp_log"
  if ! anchor build -p "$program" 2>&1 | tee "$tmp_log"; then
    exit 1
  fi
  if grep -Eq '^Error:' "$tmp_log"; then
    echo "error: anchor build emitted verifier/runtime errors for $program" >&2
    exit 1
  fi
done

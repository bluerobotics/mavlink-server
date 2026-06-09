#!/usr/bin/env bash
set -euo pipefail

if [[ -z "${SCCACHE_PATH:-}" ]] || [[ ! -x "$SCCACHE_PATH" ]]; then
  exit 0
fi

"$SCCACHE_PATH" --show-stats 2>/dev/null \
  | grep -E '^(Cache hits rate|Compilations) ' \
  || true

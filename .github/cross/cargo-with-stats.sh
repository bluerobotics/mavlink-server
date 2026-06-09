#!/bin/sh
set -e

if [ -x /usr/local/cargo/bin/cargo ]; then
  real_cargo=/usr/local/cargo/bin/cargo
elif [ -x /rust/bin/cargo ]; then
  real_cargo=/rust/bin/cargo
else
  echo "cargo wrapper: real cargo not found" >&2
  exit 1
fi

if [ "$1" = "build" ]; then
  "$real_cargo" "$@"
  /usr/bin/sccache --show-stats 2>/dev/null \
    | grep -E '^(Cache hits rate|Compilations) ' \
    || true
else
  "$real_cargo" "$@"
fi

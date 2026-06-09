#!/usr/bin/env bash
set -euo pipefail

version="v0.15.0"
target="${CROSS_TARGET:-${1:-}}"

case "$target" in
x86_64-unknown-linux-musl)
    triple="x86_64-unknown-linux-musl"
    ;;
aarch64-unknown-linux-musl)
    triple="aarch64-unknown-linux-musl"
    ;;
arm*-unknown-linux-musleabihf)
    triple="armv7-unknown-linux-musleabi"
    ;;
*)
    echo "unsupported cross target for sccache: $target" >&2
    exit 1
    ;;
esac

url="https://github.com/mozilla/sccache/releases/download/${version}/sccache-${version}-${triple}.tar.gz"
curl -fsSL "$url" | tar xz -C /usr/local/bin --strip-components=1
chmod +x /usr/local/bin/sccache

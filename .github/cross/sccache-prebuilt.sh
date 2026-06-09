#!/bin/bash
set -euxo pipefail

triple="${1:?usage: sccache-prebuilt.sh TRIPLE}"
url="https://github.com/mozilla/sccache"
version="${SCCACHE_VERSION:-v0.15.0}"

apt-get update
apt-get install -y --no-install-recommends ca-certificates curl
rm -rf /var/lib/apt/lists/*

td="$(mktemp -d)"
trap 'rm -rf "${td}"' EXIT
cd "${td}"

curl -LSfs "${url}/releases/download/${version}/sccache-${version}-${triple}.tar.gz" -o sccache.tar.gz
tar xzf sccache.tar.gz
install -m 755 "sccache-${version}-${triple}/sccache" /usr/bin/sccache

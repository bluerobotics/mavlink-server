#!/usr/bin/env bash
set -euo pipefail

log_file="${1:-sccache-build.log}"
stats="$(grep '^{"stats"' "$log_file" | tail -1)"
if [[ -z "$stats" ]]; then
  echo "::warning title=sccache stats::No sccache stats found in build log"
  exit 0
fi

hits="$(echo "$stats" | jq '[.stats.cache_hits.counts | to_entries[]?.value // 0] | add // 0')"
misses="$(echo "$stats" | jq '[.stats.cache_misses.counts | to_entries[]?.value // 0] | add // 0')"
executed="$(echo "$stats" | jq '.stats.requests_executed // 0')"
compile_requests="$(echo "$stats" | jq '.stats.compile_requests // 0')"
compilations="$(echo "$stats" | jq '.stats.compilations // 0')"
not_cacheable="$(echo "$stats" | jq '.stats.requests_not_cacheable // 0')"
total=$((hits + misses))
if (( total > 0 )); then
  rate=$(( hits * 100 / total ))
else
  rate=0
fi

echo "::notice title=sccache stats::${rate}% - ${hits} hits, ${misses} misses, ${executed} executed, ${compilations} compilations, ${compile_requests} requests, ${not_cacheable} not-cacheable"

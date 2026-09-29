#!/usr/bin/env bash
# How far do the report's counts move when one assumed constant changes?
# Each variant edits one constant in a throwaway copy, rebuilds, and reruns the report's rows
# (200 seeds, 60 requests, 2 faults). Produces the table in docs/REDTEAM.md (R3).
set -euo pipefail
cd "$(dirname "$0")/.."
root=$(pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

rows() {
  local shoke=$1
  for args in "--profile kev-cold --require-floor" "--require-floor" "--bug retry-storm" \
    "--bug cache-fallback --floor" "--bug no-validation" "--bug no-deadline" \
    "--bug lose-request-on-error"; do
    # shellcheck disable=SC2086
    "$shoke" sweep $args | awk -v a="$args" '$1 ~ /^I[0-9]/ && $3+0 > 0 {printf "%s %s ", $1, $3} END {print " <- " a}' || true
  done
}

variant() {
  local name=$1 file=$2 from=$3 to=$4
  rm -rf "$tmp/src" && mkdir "$tmp/src"
  cp -r "$root/Cargo.toml" "$root/Cargo.lock" "$root/crates" "$tmp/src/"
  if [ -n "$file" ]; then
    grep -qF "$from" "$tmp/src/$file" || { echo "variant '$name': '$from' not found in $file" >&2; exit 1; }
    sed -i "s/$from/$to/" "$tmp/src/$file"
  fi
  (cd "$tmp/src" && CARGO_TARGET_DIR="$tmp/target" cargo build --release --quiet -p shoke-cli)
  echo "== $name"
  rows "$tmp/target/release/shoke"
}

j=crates/shoke-jev/src
variant "baseline" "" "" ""
variant "idle threshold 240 s" $j/provider.rs "IDLE_COLD_MS: u64 = 120_000" "IDLE_COLD_MS: u64 = 240_000"
variant "idle threshold 15 min" $j/provider.rs "IDLE_COLD_MS: u64 = 120_000" "IDLE_COLD_MS: u64 = 900_000"
variant "long gap 1 in 1000" $j/workload.rs "rng.chance(1, 12)" "rng.chance(1, 1000)"
variant "long gap 30-60 s" $j/workload.rs "130_000 + rng.below(70_000)" "30_000 + rng.below(30_000)"
variant "fault windows 0.25-2 s" $j/faults.rs "1000 + rng.below(15_000)" "250 + rng.below(1750)"
variant "jev latency 2-7 s" $j/provider.rs "600 + rng.below(1900)" "2000 + rng.below(5000)"
variant "repeats 1 in 20" $j/workload.rs "rng.chance(1, 4)" "rng.chance(1, 20)"
variant "cold start 20 s" $j/provider.rs "COLD_START_MS: u64 = 35_000" "COLD_START_MS: u64 = 20_000"
variant "retry cap 3" $j/sim.rs "calls < 40" "calls < 3"
variant "books 31" $j/menu.rs "BOOKS: u32 = 15" "BOOKS: u32 = 31"

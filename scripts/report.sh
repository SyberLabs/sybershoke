#!/usr/bin/env bash
# Regenerate REPORT.md, or with --check fail if the committed copy is stale.
# Roadmap rule 1: no number is published until a script in the repo reproduces it.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release --quiet -p shoke-cli
tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT
./target/release/shoke report > "$tmp"

case "${1:-}" in
  --check)
    if diff -u REPORT.md "$tmp"; then
      echo "REPORT.md is up to date"
    else
      echo "REPORT.md is stale: run scripts/report.sh" >&2
      exit 1
    fi
    ;;
  "")
    cp "$tmp" REPORT.md
    echo "wrote REPORT.md"
    ;;
  *)
    echo "usage: scripts/report.sh [--check]" >&2
    exit 2
    ;;
esac

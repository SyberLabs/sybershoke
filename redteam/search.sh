#!/usr/bin/env bash
# Look for a false positive: a correct pipeline (floor on, I7 required) that fails any invariant.
# 3 profiles x 4 request counts x 4 fault counts x 6 mixes x SEEDS seeds. Prints failures only.
set -euo pipefail
cd "$(dirname "$0")/.."
seeds=${1:-2000}
cargo build --release --quiet -p shoke-cli
found=0
for profile in jev kev-warm kev-cold; do for requests in 1 5 60 400; do for faults in 0 2 10 60; do
  for mix in all http slow cold truncate menu; do
    if ! out=$(./target/release/shoke sweep --floor --require-floor --profile "$profile" \
        --requests "$requests" --faults "$faults" --mix "$mix" --seeds "$seeds"); then
      echo "FAIL: $profile requests=$requests faults=$faults mix=$mix"; echo "$out"; found=1
    fi
done; done; done; done
echo "searched $((3 * 4 * 4 * 6)) configurations x $seeds seeds; failures: $found"
exit $found

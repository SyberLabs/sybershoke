#!/usr/bin/env bash
# Phase 2 exit test against a RISE checkout: the keyword misfire and the missing fallback, each
# found with a replayable seed; also the sound rank (P8) and I3 over more turns (Q1-Q4).
# Reproduces every result in docs/ADAPTER-RISE.md.
#   adapters/rise-worker/check.sh /path/to/RISE
set -euo pipefail
cd "$(dirname "$0")/../.."
rise=${1:?usage: adapters/rise-worker/check.sh /path/to/RISE}
cargo build --release --quiet -p shoke-cli
shoke=./target/release/shoke
run() { node adapters/rise-worker/run.mjs --rise "$rise" "$@"; }
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }
# `shoke check` exits 1 on violations, which are expected here; only 2 (bad input) is an error.
chk() { local c=0; "$shoke" check "$@" --all || c=$?; [ "$c" -le 1 ] || fail "check $1 exited $c"; }
# Requests failing invariant $2 in history $1, one per line.
failing() { chk "$1" --require-floor | awk -v inv="$2" \
  '/^I[0-9]/ {cur=$1} cur==inv && $1 ~ /^r[0-9]+$/ {print $1}' | sort -u; }

# P1: schema 3 (what the production client sends). p1 and p2 are r40, r41 and r82, r83.
run --seed 1 --schema 3 --out "$tmp/s3.hist" > "$tmp/s3.txt"
 slow=$(chk "$tmp/s3.hist" | { grep '"slow/sleep/calm"' || true; } | awk '{print $1}' | sort -u | tr '\n' ' ')
# RISE #306 (42d1c86) fixed the misfire. A checkout that contains it must show none.
if git -C "$rise" merge-base --is-ancestor 42d1c86 HEAD 2>/dev/null; then
  [ -z "$slow" ] || fail "fixed Worker: slow violations on [$slow]"
  echo "P1 on a fixed Worker (RISE #306): no misfire"
else
  [ "$slow" = "r40 r41 r82 r83 " ] || fail "P1 misfire: slow violations on [$slow]"
  echo "P1 misfire, schema 3: 'drift off to sleep' raised to 300 wpm on $slow"
fi

# P8, with the sound rank read from RISE's descriptions: on the 39 cases (not the probes r40-r42,
# r82-r84), only "soft bossa" and "quiet mystery" stay flagged, on both turns.
i4=$(failing "$tmp/s3.hist" I4 | grep -vxE 'r(40|41|42|82|83|84)' | sort -V | tr '\n' ' ')
[ "$i4" = "r23 r26 r65 r68 " ] || fail "P8: clean-case I4 flags on [$i4]"
ranked=$(grep '^meta sound_rank=' "$tmp/s3.hist" | cut -d= -f2 | tr ',' '\n' | grep -cvE '^(silent|other):')
sounds=$(grep -cE "^  \('[a-z-]+', '.*', TRUE\)" "$rise/scripts/seed-rise-sounds.sql")
[ "$ranked" -eq 7 ] && [ "$sounds" -eq 24 ] || fail "sound rank: $ranked ranked of $sounds sounds"
echo "P8 sound rank: clean-case I4 flags on $i4; $ranked of $sounds sounds ranked 1 or 3"

# The rank table cites a word from each description; a RISE that drops the word stops the run.
drift="$tmp/drift"
mkdir -p "$drift/scripts"
ln -s "$(cd "$rise" && pwd)/worker" "$drift/worker"
ln -s "$(cd "$rise" && pwd)/src" "$drift/src"
for f in "$rise"/scripts/jev-eval-*.json; do ln -s "$(cd "$(dirname "$f")" && pwd)/$(basename "$f")" "$drift/scripts/"; done
sed 's/Gentle piano melody/Plain piano melody/' "$rise/scripts/seed-rise-sounds.sql" > "$drift/scripts/seed-rise-sounds.sql"
if node adapters/rise-worker/run.mjs --rise "$drift" --out "$tmp/drift.hist" 2> "$tmp/drift.txt"; then
  fail "sound rank: a description without its cited word was accepted"
fi
grep -q 'description of piano no longer says "Gentle"' "$tmp/drift.txt" || fail "sound rank: $(cat "$tmp/drift.txt")"
echo "sound rank: a description that drops its cited word stops the run"

# P2: schema 2 offers no neon, so no misfire.
run --seed 1 --schema 2 --out "$tmp/s2.hist" > /dev/null
if chk "$tmp/s2.hist" | grep -q '"slow/sleep/calm"'; then fail "P2: misfire on schema 2"; fi
echo "P2 schema 2: no misfire"

# P3: every injected fault ends in an error; nothing answers in its place (no floor).
run --seed 7 --fault-rate 30 --out "$tmp/f.hist" > "$tmp/f.txt"
faults=$(grep -o 'faults=[0-9]*' "$tmp/f.txt" | cut -d= -f2)
i7=$(failing "$tmp/f.hist" I7 | wc -l)
[ "$faults" -gt 0 ] && [ "$i7" -eq "$faults" ] || fail "P3: $faults faults, $i7 I7 failures"
for inv in I1 I3 I5; do [ -z "$(failing "$tmp/f.hist" $inv)" ] || fail "P3: $inv failed"; done
echo "P3 missing fallback: $faults faults, $i7 requests with no plan; I1 I3 I5 pass"

# P7: the Worker's 8 s covers the provider call (meta deadline_scope=call). Each timeout's resp
# comes exactly 8000 ms after its call, and no timeout fails I2.
run --seed 3 --fault-rate 20 --mix slow --out "$tmp/t.hist" > "$tmp/t.txt"
timeouts=$(grep -o 'faults=[0-9]*' "$tmp/t.txt" | cut -d= -f2)
grep -qx 'meta deadline_scope=call' "$tmp/t.hist" || fail "P7: history does not declare deadline_scope=call"
at8000=$(awk '$2=="call" { for (i = 3; i <= NF; i++) if ($i ~ /^req=/) c[substr($i, 5)] = $1 }
  $2=="resp" && / status=timeout/ { for (i = 3; i <= NF; i++) if ($i ~ /^req=/) r = substr($i, 5)
    if ($1 - c[r] == 8000) n++ } END { print n + 0 }' "$tmp/t.hist")
i2=$(failing "$tmp/t.hist" I2 | wc -l)
[ "$timeouts" -gt 0 ] && [ "$at8000" -eq "$timeouts" ] || fail "P7: $timeouts timeouts, $at8000 at call+8000"
[ "$i2" -eq 0 ] || fail "P7: $i2 requests fail I2 under the call deadline"
echo "P7 deadline: $at8000 of $timeouts timeouts end at call+8000 ms; none fails I2 under deadline_scope=call"

# Q1-Q4: I3 over 3 and 25 turns, with and without the Worker's key.
# Cache hits whose key differs from the latest model answer's key for the same text, then the first.
stale() { awk '$2=="req" { for (i = 3; i <= NF; i++) if ($i ~ /^id=/) id = substr($i, 4);
    else if ($i ~ /^text=/) text[id] = substr($i, 6) }
  $2=="decision" { for (i = 3; i <= NF; i++) { split($i, kv, "="); f[kv[1]] = substr($i, length(kv[1]) + 2) }
    t = text[f["req"]]; if (f["source"] == "model") last[t] = f["key"]
    else if (last[t] != f["key"]) { n++; if (!first) first = f["req"] } }
  END { print n + 0, first }' "$1"; }
for n in 3 25; do
  run --seed 1 --turns "$n" --out "$tmp/k$n.hist" > /dev/null
  run --seed 1 --turns "$n" --no-key --out "$tmp/n$n.hist" > /dev/null
  for h in k n; do [ -z "$(failing "$tmp/$h$n.hist" I3)" ] || fail "Q: I3 failed, $n turns, $h"; done
done
[ "$(stale "$tmp/k3.hist")" = "0 " ] || fail "Q2: stale-key hits at 3 turns: $(stale "$tmp/k3.hist")"
[ "$(stale "$tmp/k25.hist")" = "70 r925" ] || fail "Q3: stale-key hits at 25 turns: $(stale "$tmp/k25.hist")"
echo "Q1-Q4 I3: passes at 3 and 25 turns, with and without the key; 70 stale-key hits from r925 (turn 23), all equal"

# --turns 2 is the default: the history is byte-identical.
run --seed 7 --fault-rate 30 --turns 2 --out "$tmp/f3.hist" > /dev/null
cmp -s "$tmp/f.hist" "$tmp/f3.hist" || fail "--turns 2 differs from the default"

# P10: the same seed gives the same history.
run --seed 7 --fault-rate 30 --out "$tmp/f2.hist" > /dev/null
cmp -s "$tmp/f.hist" "$tmp/f2.hist" || fail "P10: seed 7 is not reproducible"
echo "P10 replayable: seed 7 gives a byte-identical history"
echo "phase 2 exit test passed against $(git -C "$rise" rev-parse --short HEAD 2>/dev/null || echo "$rise")"

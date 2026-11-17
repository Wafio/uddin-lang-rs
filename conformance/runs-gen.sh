#!/usr/bin/env bash
# Compares `uddin run` with `oracle-go run` on generated programs. Needs cargo and go on Linux.
#
#   conformance/runs-gen.sh SEED COUNT [WORKDIR]
#
# Each chunk of programs runs twice: under the production profile and under a small one (a few hundred
# operations, 4 calls, 6 blocks, 64-byte values, 2 KiB of memory), so that every limit is reached by programs
# that are otherwise ordinary. Go runs each case three times; a result that differs between the runs (map
# order) is compared on outcome, limit and the number of output lines. A case that differs is run through Go again
# with 40 runs, because three can all pick the same map order. Variables: ORACLE (an oracle-go binary), PARITY_CHUNK (default
# 20000), SHOW, GO_RUNS (runs of Go per case, default 3).
set -euo pipefail
cd "$(dirname "$0")/.."
seed=${1:?usage: runs-gen.sh SEED COUNT [WORKDIR]}
count=${2:?usage: runs-gen.sh SEED COUNT [WORKDIR]}
work=${3:-}
chunk=${PARITY_CHUNK:-20000}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
if [ -z "$work" ]; then work=$tmp; else mkdir -p "$work"; fi
oracle=${ORACLE:-}
if [ -z "$oracle" ]; then
  (cd conformance/oracle-go && GOPROXY=off go build -o "$tmp/oracle-go" .)
  oracle=$tmp/oracle-go
fi
cargo build --locked -q --release -p uddin-cli -p uddin-parity
bin=${CARGO_TARGET_DIR:-target}/release
cp "$bin/uddin" "$bin/parity" "$tmp/"
small=$tmp/small.json
sed -e 's/"max_ops": *[0-9]*/"max_ops": 300/' -e 's/"max_call_depth": *[0-9]*/"max_call_depth": 4/' \
  -e 's/"max_nesting": *[0-9]*/"max_nesting": 6/' -e 's/"max_value_len": *[0-9]*/"max_value_len": 64/' \
  -e 's/"max_alloc_bytes": *[0-9]*/"max_alloc_bytes": 2048/' profile/sandbox-profile.json >"$small"
failed=0
for ((start = 0; start < count; start += chunk)); do
  n=$((count - start < chunk ? count - start : chunk))
  "$tmp/parity" gen-run --seed "$seed" --start "$start" --count "$n" >"$work/cases-$start.jsonl"
  for mode in production small; do
    profile=profile/sandbox-profile.json
    flags=()
    if [ "$mode" = small ]; then
      profile=$small
      flags=(-limit max_ops=300 -limit max_call_depth=4 -limit max_nesting=6 -limit max_value_len=64 -limit max_alloc_bytes=2048)
    fi
    "$oracle" run -profile "$profile" -runs "${GO_RUNS:-3}" -deadline-ms 30000 <"$work/cases-$start.jsonl" >"$work/go-$start.jsonl"
    "$tmp/uddin" run -deadline-ms 30000 "${flags[@]}" <"$work/cases-$start.jsonl" >"$work/rust-$start.jsonl"
    printf 'seed %s, programs %s to %s, %s limits: ' "$seed" "$start" "$((start + n - 1))" "$mode"
    "$tmp/parity" compare-run --go "$work/go-$start.jsonl" --rust "$work/rust-$start.jsonl" --cases "$work/cases-$start.jsonl" \
      --auto-known --out "$work/mismatch-$mode-$start.jsonl" --retry "$work/retry-$mode-$start.jsonl" --show "${SHOW:-5}" --shapes || true
    # Go picks a random map order, and three runs can all pick the same one: a case that differs gets 40 more.
    if [ -s "$work/retry-$mode-$start.jsonl" ]; then
      "$oracle" run -profile "$profile" -runs 40 -deadline-ms 30000 <"$work/retry-$mode-$start.jsonl" >"$work/go-retry.jsonl"
      "$tmp/uddin" run -deadline-ms 30000 "${flags[@]}" <"$work/retry-$mode-$start.jsonl" >"$work/rust-retry.jsonl"
      printf '  again with 40 runs of Go: '
      "$tmp/parity" compare-run --go "$work/go-retry.jsonl" --rust "$work/rust-retry.jsonl" --cases "$work/retry-$mode-$start.jsonl" \
        --auto-known --out "$work/mismatch-$mode-$start.jsonl" --show "${SHOW:-5}" || failed=1
    fi
    rm -f "$work/retry-$mode-$start.jsonl" "$work/go-retry.jsonl" "$work/rust-retry.jsonl"
    [ -s "$work/mismatch-$mode-$start.jsonl" ] || rm -f "$work/mismatch-$mode-$start.jsonl"
  done
  rm -f "$work/cases-$start.jsonl" "$work/go-$start.jsonl" "$work/rust-$start.jsonl"
done
exit $failed

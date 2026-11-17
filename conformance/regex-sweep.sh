#!/usr/bin/env bash
# Compares the regex builtins with the Go oracle on generated pattern and text pairs, weighted toward the corners of
# the syntax: classes, anchors, flags, lazy forms, named groups, Unicode classes, empty matches, replacement
# templates. Needs go and cargo on Linux.
#
#   conformance/regex-sweep.sh [PAIRS] [SEED]      (20000 pairs, seed 1)
#
# Each script of the sweep holds 25 pairs, and each pair goes through all six builtins. ORACLE names an oracle-go
# binary; without it one is built. WORKDIR keeps the cases and results (default: a temporary directory). The scripts
# stay far from the regex work budget, which this sweep does not test.
set -euo pipefail
cd "$(dirname "$0")/.."
pairs=${1:-20000}
seed=${2:-1}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
work=${WORKDIR:-$tmp}
mkdir -p "$work"
oracle=${ORACLE:-}
if [ -z "$oracle" ]; then
  (cd conformance/oracle-go && GOPROXY=off go build -o "$tmp/oracle-go" .)
  oracle=$tmp/oracle-go
fi
(cd conformance/oracle-go && GOPROXY=off UDDIN_REGEX_SWEEP="$pairs" UDDIN_REGEX_SEED="$seed" go run ./cmd/extract -only lib-regex-sweep -out "$work/corpus" >/dev/null)
cases=$work/corpus/run/lib-regex-sweep.cases.jsonl
echo "sweep: $pairs pairs in $(wc -l <"$cases") scripts, seed $seed"
"$oracle" run -profile profile/sandbox-profile.json -runs 1 -deadline-ms 30000 <"$cases" >"$work/sweep.go.jsonl"
cargo build --locked -q -p uddin-cli -p uddin-parity
bin=${CARGO_TARGET_DIR:-target}/debug
"$bin/uddin" run -deadline-ms 30000 <"$cases" >"$work/sweep.rust.jsonl"
"$bin/parity" compare-run --go "$work/sweep.go.jsonl" --rust "$work/sweep.rust.jsonl" --cases "$cases" \
  --out "$work/sweep.mismatch.jsonl" --show "${SHOW:-5}" --shapes

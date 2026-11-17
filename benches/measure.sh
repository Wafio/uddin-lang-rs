#!/usr/bin/env bash
# Measures the bench scripts. Needs cargo, valgrind (VALGRIND names the binary) and go.
#
#   benches/measure.sh [SCRIPT...]
#
# Instructions per run come from cachegrind over 1 and 1001 runs: their difference over 1000 leaves out the start
# and the compile, and does not move with the load of the host. Time per run is the median of 3 runs of 20,000 for
# Rust and Go in the same job; on a shared host it is only a rough guide.
set -euo pipefail
cd "$(dirname "$0")/.."
vg=${VALGRIND:-valgrind}
if [ "$#" -gt 0 ]; then names=("$@"); else names=(simple no_match request_vars string_ops closure_calls loop_20 map_access); fi
cargo build --locked -q --release -p uddin-bench
bin=${CARGO_TARGET_DIR:-target}/release/bench

refs() { "$vg" --tool=cachegrind --cache-sim=no --cachegrind-out-file=/dev/null "$bin" "$1" "$2" 2>&1 | awk '/I +refs/ { gsub(",", "", $NF); print $NF }'; }
median() { sort -n | awk '{ a[NR] = $1 } END { print a[int((NR + 1) / 2)] }'; }

printf '%-15s %12s %10s %12s %12s\n' script instr/run ops/run rust_ns/run go_ns/run
go_out=$(cd conformance/oracle-go && GOPROXY=off go test -run '^$' -bench CoreScripts -benchtime 20000x -count 3 ./internal/oracle 2>&1)
for name in "${names[@]}"; do
  a=$(refs "$name" 1)
  b=$(refs "$name" 1001)
  ops=$("$bin" "$name" 1 | sed -n 's/.* ops=\([0-9]*\) .*/\1/p')
  ns=$(for _ in 1 2 3; do "$bin" "$name" 20000 | sed -n 's/.* ns_per_run=\([0-9]*\) .*/\1/p'; done | median)
  go_ns=$(printf '%s\n' "$go_out" | awk -v n="BenchmarkCoreScripts/$name-" 'index($1, n) == 1 { print int($3) }' | median)
  printf '%-15s %12d %10s %12s %12s\n' "$name" $(((b - a) / 1000)) "$ops" "$ns" "$go_ns"
done

#!/usr/bin/env bash
# Runs every group of the run corpus through `uddin run` and compares the results with the ones Go gave
# (corpus/run/<group>.go.jsonl). Needs cargo, not go: the Go results are recorded.
#
#   conformance/runs.sh [GROUP...]    all groups, or the named ones
#
# The recorded results used a deadline of 30 s, a backstop, so this uses the same, except for the timing group,
# whose scripts only the 5 ms deadline stops. WORKDIR keeps the results (default: a temporary directory).
# A case that a fixed defect of the reference changes has its expected result in
# corpus/run/<group>.intended.jsonl, and counts as an intended difference; any other difference fails.
set -euo pipefail
cd "$(dirname "$0")/.."
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
work=${WORKDIR:-$tmp}
mkdir -p "$work"
cargo build --locked -q -p uddin-cli -p uddin-parity
bin=${CARGO_TARGET_DIR:-target}/debug

if [ "$#" -gt 0 ]; then groups=("$@"); else
  groups=(); for f in conformance/corpus/run/*.cases.jsonl; do g=$(basename "$f" .cases.jsonl); groups+=("$g"); done
fi
failed=0
for g in "${groups[@]}"; do
  cases=conformance/corpus/run/$g.cases.jsonl
  deadline=30000
  if [ "$g" = timing ]; then deadline=5; fi
  "$bin/uddin" run -deadline-ms "$deadline" <"$cases" >"$work/$g.rust.jsonl"
  printf '%s: ' "$g"
  intended=()
  if [ -f "conformance/corpus/run/$g.intended.jsonl" ]; then intended=(--intended "conformance/corpus/run/$g.intended.jsonl"); fi
  "$bin/parity" compare-run --go "conformance/corpus/run/$g.go.jsonl" --rust "$work/$g.rust.jsonl" --cases "$cases" \
    ${intended[@]+"${intended[@]}"} --readme README.md --out "$work/$g.mismatch.jsonl" --show "${SHOW:-5}" --shapes || failed=1
  [ -s "$work/$g.mismatch.jsonl" ] || rm -f "$work/$g.mismatch.jsonl"
done
exit $failed

#!/usr/bin/env bash
# Compares the acceptance of this implementation with the Go interpreter on generated sources: `uddin check`
# against `oracle-go check`, on ok and forbidden. Needs cargo and go on Linux. Run it
# from anywhere inside the repo.
#
#   conformance/parity.sh SEED COUNT [WORKDIR]
#   PARITY_ENUM=LEN [PARITY_ALPHABET=full|core|blocks] conformance/parity.sh 0 0 [WORKDIR]
#   PARITY_NEIGHBORS=1 conformance/parity.sh 0 0 [WORKDIR]
#
# The other forms have no seed: the second compares every sequence of LEN tokens over an alphabet of
# conformance/parity/src/enumerate.rs, the third every source one edit away from a source of the corpus. Both
# are as many sources as `parity size` says.
#
# The sources come from conformance/parity (see its gen command) and the corpus in conformance/corpus/check.
# They are made and compared in chunks of PARITY_CHUNK (default 100000). WORKDIR keeps the cases that differ
# (mismatch-<start>.jsonl); without it a temporary directory is used and removed. Other variables:
#   PARITY_PROFILE=dev   build without optimisations (default release)
#   PARITY_FROM, PARITY_TO  only the sources from FROM up to TO (default 0 and all)
#   PARITY_KINDS=1       show the Go result per kind of source in each chunk
#   PARITY_SHOW_STAGE=N  show N sources per class of difference in stage only
#   ORACLE               an oracle-go binary; without it one is built
set -euo pipefail
cd "$(dirname "$0")/.."
seed=${1:?usage: parity.sh SEED COUNT [WORKDIR]}
count=${2:?usage: parity.sh SEED COUNT [WORKDIR]}
work=${3:-}
chunk=${PARITY_CHUNK:-100000}
release=(--release)
if [ "${PARITY_PROFILE:-release}" = dev ]; then release=(); fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
if [ -z "$work" ]; then work=$tmp; else mkdir -p "$work"; fi
oracle=${ORACLE:-}
if [ -z "$oracle" ]; then
  (cd conformance/oracle-go && GOPROXY=off go build -o "$tmp/oracle-go" .)
  oracle=$tmp/oracle-go
fi
cargo build --locked -q "${release[@]}" -p uddin-cli -p uddin-parity
# Copies, so that a rebuild in the middle of a long run cannot change the programs under test.
bin=${CARGO_TARGET_DIR:-target}/release
if [ "${PARITY_PROFILE:-release}" = dev ]; then bin=${CARGO_TARGET_DIR:-target}/debug; fi
cp "$bin/uddin" "$bin/parity" "$tmp/"
parity() { "$tmp/parity" "$@"; }
uddin() { "$tmp/uddin" "$@"; }

if [ -n "${PARITY_NEIGHBORS:-}" ]; then
  gen=(gen --corpus conformance/corpus/check --neighbors)
  count=$(parity size --corpus conformance/corpus/check --neighbors)
  label="one edit from a corpus source"
elif [ -n "${PARITY_ENUM:-}" ]; then
  gen=(gen --enum "$PARITY_ENUM" --alphabet "${PARITY_ALPHABET:-full}")
  label="sequences of $PARITY_ENUM tokens, ${PARITY_ALPHABET:-full} alphabet"
  count=$(parity size --enum "$PARITY_ENUM" --alphabet "${PARITY_ALPHABET:-full}")
else
  gen=(gen --corpus conformance/corpus/check --seed "$seed")
  label="seed $seed"
fi

failed=0
for ((start = ${PARITY_FROM:-0}; start < ${PARITY_TO:-$count}; start += chunk)); do
  end=${PARITY_TO:-$count}
  n=$((end - start < chunk ? end - start : chunk))
  parity "${gen[@]}" --start "$start" --count "$n" >"$work/cases-$start.jsonl"
  "$oracle" check -profile profile/sandbox-profile.json <"$work/cases-$start.jsonl" >"$work/go-$start.jsonl"
  uddin check <"$work/cases-$start.jsonl" >"$work/rust-$start.jsonl"
  printf '%s, sources %s to %s: ' "$label" "$start" "$((start + n - 1))"
  parity compare --go "$work/go-$start.jsonl" --rust "$work/rust-$start.jsonl" --cases "$work/cases-$start.jsonl" \
    --out "$work/mismatch-$start.jsonl" --show-stage "${PARITY_SHOW_STAGE:-0}" ${PARITY_KINDS:+--kinds} || failed=1
  rm -f "$work/cases-$start.jsonl" "$work/go-$start.jsonl" "$work/rust-$start.jsonl"
  [ -s "$work/mismatch-$start.jsonl" ] || rm -f "$work/mismatch-$start.jsonl"
done
exit $failed

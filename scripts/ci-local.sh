#!/usr/bin/env bash
# The gate of every contributor and every CI run, cheapest first. Run it on a Linux host that has cargo. It
# needs no Go: the comparison with the Go reference uses the results recorded in conformance/corpus. The steps
# that run the reference live are in scripts/ci-oracle.sh.
set -euo pipefail
cd "$(dirname "$0")/.."

# The date builtins read the zone of the host, as the reference does, and the corpus was recorded in this zone
# (conformance/corpus/SOURCE). Another zone gives other results for the dates written with an offset.
export TZ=Asia/Jakarta

echo '== agent material'
if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  scripts/check-ai.sh
else
  echo 'skipped: no .git here (run scripts/check-ai.sh in the checkout)'
fi

echo '== fmt'
cargo fmt --all --check

echo '== clippy'
cargo clippy --locked --workspace --all-targets -- -D warnings

echo '== test'
cargo test --locked --workspace

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo '== uddin check against the recorded Go results of the corpus'
for cases in conformance/corpus/check/*.cases.jsonl; do
  cargo run --locked -q -p uddin-cli -- check <"$cases" >"$tmp/rust.jsonl"
  printf '%s: ' "$cases"
  cargo run --locked -q -p uddin-parity -- compare --go "${cases%.cases.jsonl}.go.jsonl" --rust "$tmp/rust.jsonl" --cases "$cases"
done

echo '== uddin run against the recorded Go results of the run corpus'
conformance/runs.sh

echo '== unsafe code in the dependency tree (report)'
if command -v cargo-geiger >/dev/null 2>&1; then
  cargo geiger --locked --manifest-path "$PWD/crates/uddin/Cargo.toml" --output-format Ascii 2>/dev/null || echo 'cargo-geiger exited with a non-zero status (report only; it does that when it has warnings)'
else
  echo 'skipped: cargo-geiger is not installed'
fi

echo '== fuzz the machine'
if cargo +nightly fuzz --help >/dev/null 2>&1; then
  FUZZ_WORK="$tmp/fuzz" scripts/fuzz.sh vm "${FUZZ_SECONDS:-30}"
else
  echo 'skipped: no nightly toolchain or cargo-fuzz'
fi

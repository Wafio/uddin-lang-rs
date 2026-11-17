#!/usr/bin/env bash
# Every gate of this repo, cheapest first. Run it on a Linux host that has cargo.
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

# The Go oracle builds from the copy of the fork in conformance/oracle-go/fork. GOPROXY=off makes any network
# fetch a build failure: the toolchain named by go.mod and the modules pinned by go.sum must already be in the
# host's module cache (run `go mod download` there once if they are not).
echo '== oracle-go build, gofmt, vet, test'
oracle_tmp=$(mktemp -d)
trap 'rm -rf "$oracle_tmp"' EXIT
(
  cd conformance/oracle-go
  export GOPROXY=off
  go version
  go build -o "$oracle_tmp/oracle-go" .
  unformatted=$(gofmt -l main.go internal cmd)
  [ -z "$unformatted" ] || { echo "gofmt: $unformatted" >&2; exit 1; }
  go vet ./...
  go test ./...
)

echo '== oracle-go corpus against the recorded Go results'
ORACLE="$oracle_tmp/oracle-go" conformance/corpus.sh verify

echo '== uddin check against the recorded Go results of the corpus'
for cases in conformance/corpus/check/*.cases.jsonl; do
  cargo run --locked -q -p uddin-cli -- check <"$cases" >"$oracle_tmp/rust.jsonl"
  printf '%s: ' "$cases"
  cargo run --locked -q -p uddin-parity -- compare --go "${cases%.cases.jsonl}.go.jsonl" --rust "$oracle_tmp/rust.jsonl" --cases "$cases"
done

echo '== seeded parity run, 20000 generated sources'
PARITY_PROFILE=dev ORACLE="$oracle_tmp/oracle-go" conformance/parity.sh 1 20000

echo '== uddin run against the recorded Go results of the run corpus'
conformance/runs.sh

echo '== uddin run against the oracle on 10000 generated programs, under two profiles'
ORACLE="$oracle_tmp/oracle-go" PARITY_CHUNK=10000 conformance/runs-gen.sh 1 10000

echo '== unsafe code in the dependency tree (report)'
if command -v cargo-geiger >/dev/null 2>&1; then
  cargo geiger --locked --manifest-path "$PWD/crates/uddin/Cargo.toml" --output-format Ascii 2>/dev/null || echo 'cargo-geiger exited with a non-zero status (report only; it does that when it has warnings)'
else
  echo 'skipped: cargo-geiger is not installed'
fi

echo '== fuzz the machine'
if cargo +nightly fuzz --help >/dev/null 2>&1; then
  FUZZ_WORK="$oracle_tmp/fuzz" scripts/fuzz.sh vm "${FUZZ_SECONDS:-30}"
else
  echo 'skipped: no nightly toolchain or cargo-fuzz'
fi

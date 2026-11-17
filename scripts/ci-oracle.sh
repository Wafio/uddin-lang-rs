#!/usr/bin/env bash
# The steps of the gate that run the Go reference live. Run it on a Linux host that has cargo and Go, when a
# change touches crates/, conformance/, profile/, Cargo.toml, Cargo.lock or rust-toolchain.toml. The everyday
# gate, which needs no Go, is scripts/ci-local.sh; the two scripts together hold every step, and CI runs this
# one in .github/workflows/oracle.yml.
set -euo pipefail
cd "$(dirname "$0")/.."

# The oracle and this implementation read the zone of the host, and the recorded corpus was made in this zone
# (conformance/corpus/SOURCE). Another zone gives other results for the dates written with an offset.
export TZ=Asia/Jakarta

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

echo '== seeded parity run, 20000 generated sources'
PARITY_PROFILE=dev ORACLE="$oracle_tmp/oracle-go" conformance/parity.sh 1 20000

echo '== uddin run against the oracle on 10000 generated programs, under two profiles'
ORACLE="$oracle_tmp/oracle-go" PARITY_CHUNK=10000 conformance/runs-gen.sh 1 10000

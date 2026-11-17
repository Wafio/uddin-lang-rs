#!/usr/bin/env bash
# Compares the date builtins with the Go oracle under several time zones of the host. The reference reads the zone
# from TZ, and a date written with an offset is in that zone when the zone has that offset then, which changes what
# adding a day or an hour does across a change of clock. Needs go and cargo on Linux.
#
#   conformance/dates-tz.sh [ZONE...]
#
# Without arguments it runs a set of zones with and without a change of clock, half-hour and forty-five-minute
# offsets included. ORACLE names an oracle-go binary; without it one is built. WORKDIR keeps the cases and results.
set -euo pipefail
cd "$(dirname "$0")/.."
zones=("$@")
if [ "${#zones[@]}" -eq 0 ]; then
  zones=(UTC Asia/Jakarta America/New_York Europe/London Australia/Lord_Howe Asia/Kolkata Pacific/Chatham Asia/Kathmandu America/St_Johns Pacific/Apia America/Sao_Paulo)
fi
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
work=${WORKDIR:-$tmp}
mkdir -p "$work"
oracle=${ORACLE:-}
if [ -z "$oracle" ]; then
  (cd conformance/oracle-go && GOPROXY=off go build -o "$tmp/oracle-go" .)
  oracle=$tmp/oracle-go
fi
(cd conformance/oracle-go && GOPROXY=off UDDIN_DATES_TZ=1 go run ./cmd/extract -only lib-dates-tz -out "$work/corpus" >/dev/null)
cases=$work/corpus/run/lib-dates-tz.cases.jsonl
cargo build --locked -q -p uddin-cli -p uddin-parity
bin=${CARGO_TARGET_DIR:-target}/debug
failed=0
for zone in "${zones[@]}"; do
  TZ=$zone "$oracle" run -profile profile/sandbox-profile.json -runs 1 -deadline-ms 30000 <"$cases" >"$work/tz.go.jsonl"
  TZ=$zone "$bin/uddin" run -deadline-ms 30000 <"$cases" >"$work/tz.rust.jsonl"
  printf '%s: ' "$zone"
  "$bin/parity" compare-run --go "$work/tz.go.jsonl" --rust "$work/tz.rust.jsonl" --cases "$cases" \
    --out "$work/tz.mismatch.jsonl" --show "${SHOW:-3}" --shapes || failed=1
done
exit $failed

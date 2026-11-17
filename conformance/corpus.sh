#!/usr/bin/env bash
# Records or verifies the results of the Go oracle over the corpus. Needs go on Linux. Run it from
# anywhere inside the repo.
#
#   conformance/corpus.sh record [group...]   runs the oracle and writes corpus/{check,run}/<group>.go.jsonl
#   conformance/corpus.sh verify [group...]   runs it again and compares with those files
#
# With group names, only those groups; without, all.
#
# ORACLE names an oracle-go binary; without it one is built into a temporary directory.
#
# The run corpus uses a deadline of 30 s, a backstop, not the 5 ms of production: a deadline that fires turns
# the result into a timing accident, and the corpus compares what scripts do, not how fast. A result that Go
# itself does not repeat (map order, the clock) is marked in its line and not compared by verify. The timing
# group is the exception: it exists to reach the deadline, so it runs with the production deadline of 5 ms, and
# a line that ended in a timeout is marked and not compared either.
set -euo pipefail
cd "$(dirname "$0")/.."
mode=${1:?usage: corpus.sh record|verify [group...]}
shift
groups=" $* "
profile=$PWD/profile/sandbox-profile.json

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
oracle=${ORACLE:-}
if [ -z "$oracle" ]; then
  (cd conformance/oracle-go && GOPROXY=off go build -o "$tmp/oracle-go" .)
  oracle=$tmp/oracle-go
fi

# Lines of the two files are paired by position; a line marked order_sensitive, clock or timing in either is
# skipped.
compare_run() {
  awk '
    NR == FNR { rec[FNR] = $0; n = FNR; next }
    {
      seen++
      if (rec[FNR] ~ /"(order_sensitive|clock|timing)":true/ || $0 ~ /"(order_sensitive|clock|timing)":true/) { skipped++; next }
      if ($0 != rec[FNR]) { bad++; printf "line %d differs\n  recorded: %.300s\n  fresh:    %.300s\n", FNR, rec[FNR], $0 }
    }
    END {
      if (seen != n) { bad++; printf "recorded %d lines, fresh %d\n", n, seen }
      printf "%d lines, %d not compared, %d differ\n", seen, skipped, bad
      exit bad > 0
    }' "$1" "$2"
}

failed=0
for cases in conformance/corpus/check/*.cases.jsonl conformance/corpus/run/*.cases.jsonl; do
  kind=$(basename "$(dirname "$cases")")
  group=$(basename "$cases" .cases.jsonl)
  if [ "$groups" != "  " ] && [[ "$groups" != *" $group "* ]]; then continue; fi
  recorded=${cases%.cases.jsonl}.go.jsonl
  fresh=$tmp/$kind-$(basename "$recorded")
  if [ "$kind" = check ]; then
    "$oracle" check -profile "$profile" <"$cases" >"$fresh"
  else
    deadline=30000
    if [ "$(basename "$cases")" = timing.cases.jsonl ]; then deadline=5; fi
    "$oracle" run -profile "$profile" -runs 8 -deadline-ms "$deadline" <"$cases" >"$fresh"
  fi
  if [ "$mode" = record ]; then
    cp "$fresh" "$recorded"
    echo "recorded $recorded ($(wc -l <"$fresh") lines)"
    continue
  fi
  printf '%s: ' "$recorded"
  if [ "$kind" = check ]; then
    if cmp -s "$recorded" "$fresh"; then echo "$(wc -l <"$fresh") lines, identical"; else echo differs; diff "$recorded" "$fresh" | head -20 || true; failed=1; fi
  else
    compare_run "$recorded" "$fresh" || failed=1
  fi
done
exit $failed

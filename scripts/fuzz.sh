#!/usr/bin/env bash
# Builds the fuzz targets and runs one of them on one core for SECONDS seconds, with the sources of the corpus
# as seeds. Needs a nightly toolchain and cargo-fuzz (see fuzz/README.md).
#
#   scripts/fuzz.sh TARGET SECONDS      TARGET is lexer, parser, check or vm
#
# FUZZ_WORK is where the corpus, the artifacts and the build go (default: a directory under $TMPDIR); it is not
# removed, so a later run continues from the corpus the earlier ones found. A crash, a hang or a case over the
# time limit leaves its input in $FUZZ_WORK/artifacts and fails the script.
set -euo pipefail
cd "$(dirname "$0")/.."
target=${1:?usage: fuzz.sh TARGET SECONDS}
seconds=${2:?usage: fuzz.sh TARGET SECONDS}
work=${FUZZ_WORK:-${TMPDIR:-/tmp}/uddin-fuzz}
mkdir -p "$work/corpus/$target" "$work/seeds" "$work/artifacts"
cargo run --locked -q -p uddin-parity -- seeds --corpus conformance/corpus/check --out "$work/seeds"
cd fuzz
cargo +nightly fuzz build -s none --target-dir "$work/target" "$target"
cargo +nightly fuzz run -s none --target-dir "$work/target" "$target" "$work/corpus/$target" "$work/seeds" -- \
  -max_len=65536 -timeout=10 -max_total_time="$seconds" -artifact_prefix="$work/artifacts/"

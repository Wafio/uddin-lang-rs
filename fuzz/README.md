# Fuzz targets

Four libFuzzer targets, built with cargo-fuzz on a nightly toolchain:

| target | runs |
|---|---|
| `lexer` | `uddin_syntax::tokenize` on any text |
| `parser` | `uddin_syntax::parse_source` on any bytes |
| `check` | `uddin::check` (size, syntax, allowlist) on any bytes |
| `vm` | compiles any bytes with the `waf_*` builtins and runs them under the profile with 20,000 operations and the production deadline, on a request made of the bytes (body, path, query, headers, detected categories, the JSON body when the bytes are JSON, fields); it also checks that the operation limit holds and that the run did not panic |

A case passes when nothing panics, aborts or hangs and it takes at most 10 ms (`src/lib.rs`: the best of three
timings counts, so a pause of a shared host does not fail a fast case) plus half a millisecond for every KiB of
input, since a 64 KiB source takes about 20 ms in an instrumented build and 4 ms in a release build. The `vm` target allows 200 ms: the
deadline of a run plus the longest step of a builtin that does not read the clock, about twenty times slower under
coverage instrumentation than in a release build. Overflow checks and debug assertions are
on, as cargo-fuzz builds by default.

The targets are built without a sanitizer (`-s none`): the crates under test forbid `unsafe`, so the sanitizer
finds nothing there, and its slowdown (about twenty times on a 30 KB source) pushes cases over the 10 ms bound
that run in well under 1 ms in a normal build. A stack overflow still ends the run as a signal.

    cargo run -p uddin-parity -- seeds --corpus conformance/corpus/check --out /tmp/seeds
    scripts/fuzz.sh TARGET SECONDS

`scripts/fuzz.sh` makes the seeds, builds the target and runs it on one core; `FUZZ_WORK` says where the corpus,
the artifacts and the build go, so a later run continues from the corpus the earlier ones found. By hand, from
`fuzz/`:

    cargo +nightly fuzz build -s none
    cargo +nightly fuzz run -s none check corpus/check /tmp/seeds -- -max_len=65536 -timeout=10 -max_total_time=3600

The corpus sources are the seeds. `fuzz/corpus` and `fuzz/artifacts` are ignored by git. This crate is outside
the workspace on purpose (the libFuzzer entry point is `unsafe`), so the workspace build does not compile it;
`scripts/ci-local.sh` runs the `vm` target for `FUZZ_SECONDS` (default 30) when `cargo-fuzz` and a nightly toolchain
are there.

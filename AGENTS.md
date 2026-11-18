# AGENTS.md

Instructions for coding assistants that work on uddin-lang-rs. The rules for every contribution are in
`CONTRIBUTING.md`; this file is the short form.

## What this is

A Rust interpreter of the uddin script language. It runs untrusted scripts under limits on time, memory and
depth, and the Wafio WAF agent uses it for script rules. Its results must match the recorded results of the Go
reference interpreter.

## Commands

    cargo build --locked --release -p uddin-cli
    TZ=Asia/Jakarta cargo test --locked --workspace   # the corpus was recorded in this time zone
    scripts/ci-local.sh                               # the gate: must pass before a pull request

The gate runs `cargo fmt --check`, `cargo clippy` with `-D warnings`, the tests and the comparison with the
recorded results. `scripts/ci-oracle.sh` runs the Go reference live and needs Go; run it when a change touches
`crates/`, `conformance/` or `profile/`. Fix what they report; do not disable a check.

## Rules

- The interpreter gives the same result as the reference on the recorded corpus. A deliberate difference is
  listed in the README, "Differences from the reference", and in the corpus.
- A script must never run longer or use more memory than its limits allow. A change to the machine comes with
  a test for the limit it touches, and the fuzz targets still pass (`fuzz/README.md`).
- No `unsafe` code: the workspace forbids it.
- A new dependency must be maintained and have a permissive licence.

## Do not edit

- The recorded results under `conformance/corpus/` (`*.go.jsonl`): they come from the Go reference, not from
  this interpreter.
- `conformance/oracle-go/`: the pinned copy of the reference.
- `profile/`: a pinned copy of the sandbox profile of wafio-cp-backend.
- `LICENSE`.

## Writing

Comments, commit messages and documents are plain English. One topic per pull request.

## Security

Do not put a vulnerability in an issue, a pull request or a commit message. `SECURITY.md` says how to report it.

## The person, not the assistant

The person who opens the pull request has read the change, understands it and answers for it, and signs
off the commits. A security
report must be reproduced before it is sent. `CONTRIBUTING.md`, "Work done with a coding assistant", says the same.

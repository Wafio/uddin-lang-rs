# Contributing to uddin-lang-rs

Thank you for looking at the code. This file says what a change needs before it can be merged.

## Before you start

- For anything larger than a small fix, open an issue first and say what you want to change. It saves you work if the change does not fit.
- A security problem does not go into an issue: see `SECURITY.md`.

## Build and test

The requirements are in the README. The gate runs on Linux and needs no Go, because it compares with the results of the reference interpreter that are recorded in the corpus:

```sh
scripts/ci-local.sh
```

If your change touches `crates/`, `conformance/`, `profile/`, `Cargo.toml`, `Cargo.lock` or `rust-toolchain.toml`, also run the checks that run the reference interpreter live. They need Go:

```sh
scripts/ci-oracle.sh
```

CI runs the gate on every pull request, and the second script on pull requests with such changes.

## Rules of this repository

- The interpreter must give the same result as the reference on the recorded corpus. A deliberate difference is listed in the README section "Differences from the reference" and in the corpus.
- A script must never be able to run longer or use more memory than its limits allow. A change to the machine comes with a test for the limit it touches, and the fuzz targets must still pass (`fuzz/README.md`).
- No `unsafe` code: the workspace forbids it (`unsafe_code = "forbid"` in `Cargo.toml`).
- A new dependency must be maintained and have a permissive licence.

## Pull requests

- One topic per pull request, with tests for what it changes.
- Say what the change does and how you checked it.
- Commit messages and comments are plain English.
- Do not commit secrets, local configuration, certificates or keys.
- `AGENTS.md` at the root is the one instruction file for coding assistants. Do not add another one, or planning notes; the gate refuses them.

## Licence of your contribution

The project is under the MIT licence (`LICENSE`), and your contribution is under the same licence. Sign off every commit with `git commit -s`. The `Signed-off-by` line states that you wrote the change or have the right to send it, as described in the Developer Certificate of Origin (https://developercertificate.org).

If part of a change comes from another project, say so in the pull request: the project, the copyright holder and the licence.

## Work done with a coding assistant

You may use a coding assistant. `AGENTS.md` holds the short form of these rules for it; tools that read another
file can be pointed at that one. The person who opens the pull request has read the change, understands it and
answers for it, and is the one who signs off the commits. A security report must be reproduced before it is sent: a finding that
nobody ran is not a report.

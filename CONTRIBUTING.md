# Contributing to uddin-lang-rs

Thank you for looking at the code. This file says what a change needs before it can be merged.

## Before you start

- For anything larger than a small fix, open an issue first and say what you want to change. It saves you work if the change does not fit.
- A security problem does not go into an issue: see `SECURITY.md`.

## Build and test

The requirements are in the README. The gate runs on Linux and needs Go for the reference interpreter it compares with:

```sh
scripts/ci-local.sh
```

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
- Do not add instruction files for coding assistants, or planning notes, to the repository. The gate refuses them.

## Licence of your contribution

The project is under the MIT licence (`LICENSE`), and your contribution is under the same licence. Sign off every commit with `git commit -s`. The `Signed-off-by` line states that you wrote the change or have the right to send it, as described in the Developer Certificate of Origin (https://developercertificate.org).

If part of a change comes from another project, say so in the pull request: the project, the copyright holder and the licence.

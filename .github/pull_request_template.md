## What this changes


## How it was checked

- [ ] `scripts/ci-local.sh` passes on Linux
- [ ] If the change touches `crates/`, `conformance/` or `Cargo.lock`: `scripts/ci-oracle.sh` passes (needs Go)
- [ ] Every commit is signed off (`git commit -s`)

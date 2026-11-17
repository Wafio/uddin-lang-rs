//! Shared by the fuzz targets.

use std::time::{Duration, Instant};

/// The most one case may take.
pub const CASE_LIMIT: Duration = Duration::from_millis(10);

/// The most one case of the `vm` target may take: the deadline of a run (5 ms) plus the longest step a builtin
/// takes without looking at the clock (a few milliseconds over a megabyte of text), and coverage instrumentation
/// makes both about twenty times slower than a release build. A hang is still caught by the timeout of libFuzzer.
pub const VM_CASE_LIMIT: Duration = Duration::from_millis(200);

/// Runs `f` and panics when it needs more than [`CASE_LIMIT`].
pub fn within_limit(f: impl FnMut()) {
    within(CASE_LIMIT, f);
}

/// [`within_limit`] for work on `len` bytes of text: half a millisecond more for every KiB, because coverage
/// instrumentation makes a 64 KiB source take about twenty milliseconds where a release build takes four.
pub fn within_limit_for(len: usize, f: impl FnMut()) {
    within(CASE_LIMIT + Duration::from_micros(500) * (len / 1024) as u32, f);
}

/// Runs `f` and panics when it needs more than `limit`. A slow case is timed up to three times and the best time
/// counts, so a pause of the shared host does not fail a case that is fast.
pub fn within(limit: Duration, mut f: impl FnMut()) {
    let mut best = Duration::MAX;
    for _ in 0..3 {
        let start = Instant::now();
        f();
        best = best.min(start.elapsed());
        if best <= limit {
            return;
        }
    }
    panic!("a case took {best:?}, the limit is {limit:?}");
}

//! Runs one script many times, for a measurement from outside: `bench NAME ITERATIONS` prints the time per
//! run and the operations a run counts. Cachegrind over the two counts 1 and 1001 gives the instructions of one
//! run, which does not move with the load of the host.

#![forbid(unsafe_code)]

use std::time::Instant;

use uddin::{Limits, Profile, RequestVars, Runner, Script};

const SCRIPTS: [(&str, &str); 7] = [
    ("simple", include_str!("../scripts/simple.din")),
    ("no_match", include_str!("../scripts/no_match.din")),
    ("request_vars", include_str!("../scripts/request_vars.din")),
    ("string_ops", include_str!("../scripts/string_ops.din")),
    (
        "closure_calls",
        include_str!("../scripts/closure_calls.din"),
    ),
    ("loop_20", include_str!("../scripts/loop_20.din")),
    ("map_access", include_str!("../scripts/map_access.din")),
];

fn measure(name: &str, src: &str, iterations: u64, limits: &Limits, profile: &Profile) {
    let script = Script::compile(src.as_bytes(), profile).expect("the bench scripts compile");
    let vars = RequestVars {
        method: b"GET",
        path: b"/index.php",
        ua: b"curl/7.68",
        score: 3.5,
        ..RequestVars::default()
    };
    let mut runner = Runner::new();
    let mut ended = runner.run(&script, &vars, limits);
    let ops = runner.ops();
    let start = Instant::now();
    for _ in 0..iterations {
        ended = runner.run(&script, &vars, limits);
    }
    let elapsed = start.elapsed();
    println!(
        "{name} iterations={iterations} ns_per_run={} ops={ops} ended={:?}",
        elapsed.as_nanos() / u128::from(iterations.max(1)),
        ended
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let profile = Profile::embedded();
    let mut limits = profile.limits();
    limits.deadline = None;
    let iterations: u64 = args.get(1).and_then(|n| n.parse().ok()).unwrap_or(20_000);
    for (name, src) in SCRIPTS {
        if args.first().is_none_or(|a| a == "all" || a == name) {
            measure(name, src, iterations, &limits, &profile);
        }
    }
}

//! The recorded results of the Go interpreter for the regular expression cases that reach a limit of the sandbox,
//! replayed against the machine. A run that passes 1,024 instructions in a pattern, or 524,288 units of regex
//! work, ends with the limit `regex_size` or `regex_work`. The first test replays the cases that end for another
//! reason, the second those that end in one of these two.

use std::io::Cursor;

use serde_json::Value;
use uddin::Profile;
use uddin::jsonl::run_jsonl;

const CASES: &str = include_str!("../../../conformance/corpus/run/lib-regex-limits.cases.jsonl");
const RECORDED: &str = include_str!("../../../conformance/corpus/run/lib-regex-limits.go.jsonl");

/// The ids of the cases whose recorded result passes `keep` and whose result here differs from it.
fn differing(keep: impl Fn(&Value) -> bool) -> (usize, Vec<String>) {
    let profile = Profile::embedded();
    let mut limits = profile.limits();
    limits.deadline = None;
    let mut out = Vec::new();
    run_jsonl(Cursor::new(CASES), &mut out, &profile, limits).expect("the cases run");
    let ours = String::from_utf8(out).expect("utf-8");
    let (mut kept, mut bad) = (0, Vec::new());
    for (rec, got) in RECORDED.lines().zip(ours.lines()) {
        let rec: Value = serde_json::from_str(rec).expect("a recorded line");
        let got: Value = serde_json::from_str(got).expect("a result line");
        if !keep(&rec) || got["outcome"] == "unported" {
            continue;
        }
        kept += 1;
        let same = ["outcome", "limit", "error", "print_b64"]
            .iter()
            .all(|k| rec[k] == got[k]);
        if !same {
            bad.push(rec["id"].as_str().unwrap_or("?").to_owned());
        }
    }
    (kept, bad)
}

fn needs_the_regex_limits(rec: &Value) -> bool {
    matches!(rec["limit"].as_str(), Some("regex_size" | "regex_work"))
}

#[test]
fn the_cases_that_end_for_another_reason_equal_the_recorded_ones() {
    let (kept, bad) = differing(|r| !needs_the_regex_limits(r));
    assert!(kept > 50, "{kept} cases compared");
    assert!(bad.is_empty(), "{} of {kept} differ: {bad:?}", bad.len());
}

#[test]
fn the_cases_that_end_in_a_regex_limit_equal_the_recorded_ones() {
    let (kept, bad) = differing(needs_the_regex_limits);
    assert!(kept > 30, "{kept} cases compared");
    assert!(bad.is_empty(), "{} of {kept} differ: {bad:?}", bad.len());
}

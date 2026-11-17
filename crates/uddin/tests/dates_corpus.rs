//! The recorded results of the Go interpreter for the date builtins, replayed against the machine, with the results
//! the fixes of the reference's defects change taken from the intended file of the group, and what the machine
//! cannot do for them: `date_diff` and `date_compare` give the reference an integer of a kind its own arithmetic
//! refuses, which the value model has no kind for.

mod common;

use std::io::Cursor;

use common::{run, table};
use serde_json::Value;
use uddin::jsonl::run_jsonl;
use uddin::{Outcome, Profile};

const CASES: &str = include_str!("../../../conformance/corpus/run/lib-dates.cases.jsonl");
const RECORDED: &str = include_str!("../../../conformance/corpus/run/lib-dates.go.jsonl");
/// What this implementation gives where a fixed defect of the reference changes the result.
const INTENDED: &str = include_str!("../../../conformance/corpus/run/lib-dates.intended.jsonl");

#[test]
fn the_recorded_cases_have_the_recorded_results() {
    let profile = Profile::embedded();
    let mut limits = profile.limits();
    limits.deadline = None;
    let mut out = Vec::new();
    run_jsonl(Cursor::new(CASES), &mut out, &profile, limits).expect("the cases run");
    let ours = String::from_utf8(out).expect("utf-8");
    let intended: std::collections::HashMap<String, Value> = INTENDED
        .lines()
        .map(|l| {
            let v: Value = serde_json::from_str(l).expect("an intended line");
            (v["id"].as_str().expect("an id").to_owned(), v)
        })
        .collect();
    let (mut compared, mut bad) = (0, Vec::new());
    for (rec, got) in RECORDED.lines().zip(ours.lines()) {
        let rec: Value = serde_json::from_str(rec).expect("a recorded line");
        let rec = intended
            .get(rec["id"].as_str().unwrap_or_default())
            .cloned()
            .unwrap_or(rec);
        let got: Value = serde_json::from_str(got).expect("a result line");
        compared += 1;
        let same = ["outcome", "limit", "error", "print_b64"]
            .iter()
            .all(|k| rec[k] == got[k]);
        if !same {
            bad.push(rec["id"].as_str().unwrap_or("?").to_owned());
        }
    }
    assert!(compared > 150, "{compared} cases compared");
    assert!(
        bad.is_empty(),
        "{} of {compared} differ: {bad:?}",
        bad.len()
    );
}

fn printed(src: &str) -> String {
    let ran = run(&table(&[]), src, |_| {});
    assert_eq!(ran.ended, Outcome::Ok, "{src}");
    ran.text()
}

#[test]
fn a_date_difference_is_an_integer() {
    assert_eq!(
        printed(
            "x = date_diff(\"2023-12-25\", \"2023-12-24\", \"days\")\nprint(x + 1, typeof(x))\n"
        ),
        "2 integer\n"
    );
}

#[test]
#[ignore = "the machine has no value for the integer kind the reference gives a date difference"]
fn a_date_difference_is_an_integer_of_the_kind_the_reference_refuses_arithmetic_on() {
    // In the reference `typeof` says unknown, `x + 1` is a type error, `x == 1` is false, and `date_add` takes it
    // as the amount, where an ordinary integer is refused.
    let out = printed(
        "x = date_diff(\"2023-12-25\", \"2023-12-24\", \"days\")\nprint(typeof(x), x == 1)\n\
         try:\n    print(x + 1)\ncatch (e):\n    print(e)\nend\n\
         print(date_add(\"2023-12-25\", \"days\", x))\n",
    );
    assert_eq!(
        out,
        "unknown false\n\
         type error at 3:11: + requires two integers, strings, arrays, or objects\n\
         2023-12-26T00:00:00Z\n"
    );
}

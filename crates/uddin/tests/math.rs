//! The recorded results of the Go interpreter for the scripts of the `lib-math` group, replayed through the
//! public API: outcome, limit, error class and text, and what the script printed.

use std::time::Duration;

use serde_json::Value as Json;
use uddin::{LimitReason, Outcome, Profile, RequestVars, Runner, Script};

const CASES: &str = include_str!("../../../conformance/corpus/run/lib-math.cases.jsonl");
const RECORDED: &str = include_str!("../../../conformance/corpus/run/lib-math.go.jsonl");
/// What this implementation gives where a fixed defect of the reference changes the result.
const INTENDED: &str = include_str!("../../../conformance/corpus/run/lib-math.intended.jsonl");

fn decode_base64(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut bits, mut have) = (0u32, 0);
    for c in text.bytes().take_while(|&c| c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            _ => 63,
        };
        bits = bits << 6 | u32::from(v);
        have += 6;
        if have >= 8 {
            have -= 8;
            out.push((bits >> have) as u8);
            bits &= (1 << have) - 1;
        }
    }
    out
}

/// Outcome, limit, error class, error text and printed bytes.
#[derive(Debug, PartialEq)]
struct Result5(String, String, String, String, Vec<u8>);

fn recorded(line: &str) -> (String, bool, Result5) {
    let v: Json = serde_json::from_str(line).expect("a result line");
    let field = |k: &str| v[k].as_str().expect(k).to_owned();
    let shaky = ["order_sensitive", "clock", "timing"]
        .iter()
        .any(|k| v[k].as_bool() == Some(true));
    (
        field("id"),
        shaky,
        Result5(
            field("outcome"),
            field("limit"),
            field("error_class"),
            field("error"),
            decode_base64(&field("print_b64")),
        ),
    )
}

fn ours(src: &str) -> Result5 {
    let profile = Profile::embedded();
    let script = Script::compile(src.as_bytes(), &profile).expect("the script compiles");
    let mut limits = profile.limits();
    limits.capture_print = true;
    limits.deadline = Some(Duration::from_secs(30));
    let mut runner = Runner::new();
    let ended = runner.run(&script, &RequestVars::default(), &limits);
    let (outcome, limit, class, error) = match ended {
        Outcome::Ok => ("ok", String::new(), "", String::new()),
        Outcome::Error { class, text } => ("error", String::new(), class, text),
        Outcome::Limit { reason, text } => {
            let name = if reason == LimitReason::Canceled {
                "timeout"
            } else {
                reason.as_str()
            };
            ("limit", name.to_owned(), "limit", text)
        }
    };
    Result5(
        outcome.to_owned(),
        limit,
        class.to_owned(),
        error,
        runner.printed().to_vec(),
    )
}

#[test]
fn the_builtins_give_the_results_the_go_interpreter_recorded() {
    let cases: Vec<Json> = CASES
        .lines()
        .map(|l| serde_json::from_str(l).expect("a case line"))
        .collect();
    let results: Vec<(String, bool, Result5)> = RECORDED.lines().map(recorded).collect();
    let intended: std::collections::HashMap<String, Result5> = INTENDED
        .lines()
        .map(|l| {
            let (id, _, result) = recorded(l);
            (id, result)
        })
        .collect();
    assert_eq!(cases.len(), results.len());
    assert!(cases.len() >= 100, "{} cases", cases.len());
    let mut differ = Vec::new();
    for (case, (id, shaky, want)) in cases.iter().zip(&results) {
        assert_eq!(case["id"].as_str(), Some(id.as_str()));
        if *shaky {
            continue;
        }
        let src = case["src"].as_str().expect("a source");
        let got = ours(src);
        let want = intended.get(id).unwrap_or(want);
        if got != *want {
            differ.push(format!("{id}\n  want {want:?}\n  got  {got:?}"));
        }
    }
    assert!(
        differ.is_empty(),
        "{} of {} differ:\n{}",
        differ.len(),
        cases.len(),
        differ
            .iter()
            .take(4)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// The operator `**` and `pow()` are one function in the Go interpreter. The machine has its own copy of it, whose
/// logarithm normalizes a subnormal argument, where the amd64 assembly of Go reads its exponent bits as they
/// are: Go gives 4.116030317769531e-93 for `1e-310 ** 0.3` and `pow()` here agrees, the operator does not.
#[test]
fn the_power_operator_gives_what_pow_gives_for_a_subnormal_base() {
    let profile = Profile::embedded();
    let src = "print(str(1e-310 ** 0.3), str(pow(1e-310, 0.3)))\n";
    let script = Script::compile(src.as_bytes(), &profile).expect("the script compiles");
    let mut limits = profile.limits();
    limits.capture_print = true;
    let mut runner = Runner::new();
    assert_eq!(
        runner.run(&script, &RequestVars::default(), &limits),
        Outcome::Ok
    );
    assert_eq!(
        runner.printed(),
        b"4.116030317769531e-93 4.116030317769531e-93\n"
    );
}

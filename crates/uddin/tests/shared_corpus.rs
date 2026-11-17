//! The recorded results of the Go interpreter for the scripts of the `lib-shared` group, replayed against the
//! machine with the two test builtins those scripts call: `date_add`, which returns errors as values, and
//! `range`, which has a guard, a reservation and fresh elements. Both do only what the scripts need of them.

mod common;

use std::time::Duration;

use common::{run_under, table};
use serde_json::Value as Json;
use uddin::{LimitReason, Outcome};
use uddin_vm::{
    BuiltinDef, Exc, Fresh, LIST_COST, RANGE_ARG_BOUND, RANGE_ELEM_COST, Rt, Value,
    checked_product, sum_estimate,
};

const CASES: &str = include_str!("../../../conformance/corpus/run/lib-shared.cases.jsonl");
const RECORDED: &str = include_str!("../../../conformance/corpus/run/lib-shared.go.jsonl");
/// What this implementation gives where a fixed defect of the reference changes the result.
const INTENDED: &str = include_str!("../../../conformance/corpus/run/lib-shared.intended.jsonl");

/// The first argument is not a date string, or no layout parses it. Only inputs of letters are told apart.
fn date_add(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let Some(&Value::Str(s)) = args.first() else {
        return rt.error_value(b"date_add: first argument must be a date string");
    };
    let input = rt.heap.str_bytes(s).to_vec();
    let mut text = b"date_add: failed to parse date: parsing time \"".to_vec();
    text.extend_from_slice(&input);
    text.extend_from_slice(b"\" as \"2006-01-02 15:04:05\": cannot parse \"");
    text.extend_from_slice(&input);
    text.extend_from_slice(b"\" as \"2006\"");
    rt.error_value(&text)
}

fn range_guard(rt: &mut Rt<'_>, args: &[Value]) -> Result<i64, Exc> {
    let mut ints = Vec::new();
    for a in args {
        let Value::Int(n) = *a else {
            return Ok(0);
        };
        if !(-RANGE_ARG_BOUND..=RANGE_ARG_BOUND).contains(&n) {
            rt.size_check(-1)?;
        }
        ints.push(n);
    }
    let n = match ints[..] {
        [n] => n,
        [a, b] => b - a,
        [a, b, step] if step != 0 => (b - a) / step,
        _ => 0,
    };
    if n > 0 {
        rt.size_check(n)?;
    }
    Ok(0)
}

fn range_len(args: &[Value]) -> i64 {
    if args.is_empty() || args.len() > 3 {
        return 0;
    }
    let mut ints = [0i64; 3];
    for (i, a) in args.iter().enumerate() {
        let Value::Int(n) = *a else {
            return 0;
        };
        if !(-RANGE_ARG_BOUND..=RANGE_ARG_BOUND).contains(&n) {
            return 0;
        }
        ints[i] = n;
    }
    let n = match args.len() {
        1 => ints[0],
        2 => ints[1] - ints[0],
        _ if ints[2] != 0 => {
            let span = ints[1] - ints[0];
            span / ints[2] + i64::from(span % ints[2] != 0)
        }
        _ => 0,
    };
    n.max(0)
}

fn range_reserve(rt: &mut Rt<'_>, args: &[Value]) -> Result<i64, Exc> {
    let n = range_len(args);
    let estimate = if n > 0 {
        sum_estimate(LIST_COST, checked_product(RANGE_ELEM_COST, n))
    } else {
        0
    };
    rt.reserve(estimate)
}

fn counting(rt: &mut Rt<'_>, start: i64, stop: i64) -> Result<Value, Exc> {
    let n = (stop - start).max(0) as usize;
    let id = rt.heap.new_list(n, n).map_err(rt.oom())?;
    for i in 0..n {
        rt.heap
            .list_push(id, Value::Int(start + i as i64))
            .map_err(rt.oom())?;
    }
    Ok(Value::List(id))
}

fn range(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    match args {
        [Value::Int(n)] if *n < 0 => Err(Exc::value_error(
            rt.pos,
            "range() argument must not be negative",
        )),
        [Value::Int(n)] => counting(rt, 0, *n),
        [_] => Err(rt.type_error("range() requires an integer")),
        [Value::Int(a), Value::Int(b)] => counting(rt, *a, *b),
        [_, _] => Err(rt.type_error("range() requires integer arguments")),
        _ => Err(rt.type_error("this test builtin takes one or two arguments")),
    }
}

fn builtins() -> Vec<BuiltinDef> {
    table(&[
        BuiltinDef::new("date_add", date_add, 3),
        BuiltinDef::new("range", range, -1)
            .guard(range_guard)
            .reserve(range_reserve)
            .fresh(Fresh::Elements),
    ])
}

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

/// What a result line says, as the comparison reads it: outcome, limit, error class, error text, printed bytes.
#[derive(Debug, PartialEq)]
struct Result5(String, String, String, String, Vec<u8>);

fn recorded(line: &str) -> (String, Result5) {
    let v: Json = serde_json::from_str(line).expect("a result line");
    let field = |k: &str| v[k].as_str().expect(k).to_owned();
    (
        field("id"),
        Result5(
            field("outcome"),
            field("limit"),
            field("error_class"),
            field("error"),
            decode_base64(&field("print_b64")),
        ),
    )
}

fn ours(id: &str, src: &str) -> Result5 {
    let mut limits = common::limits(|l| l.deadline = Some(Duration::from_secs(30)));
    limits.capture_print = true;
    let ran = run_under(&builtins(), src, &limits);
    let _ = id;
    let (outcome, limit, class, error) = match ran.ended {
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
        ran.printed,
    )
}

#[test]
fn the_machine_gives_the_results_the_go_interpreter_recorded() {
    let cases: Vec<Json> = CASES
        .lines()
        .map(|l| serde_json::from_str(l).expect("a case line"))
        .collect();
    let results: Vec<(String, Result5)> = RECORDED.lines().map(recorded).collect();
    let intended: std::collections::HashMap<String, Result5> =
        INTENDED.lines().map(recorded).collect();
    assert_eq!(cases.len(), results.len());
    assert!(cases.len() >= 60, "{} cases", cases.len());
    let mut differ = Vec::new();
    for (case, (id, want)) in cases.iter().zip(&results) {
        assert_eq!(case["id"].as_str(), Some(id.as_str()));
        let src = case["src"].as_str().expect("a source");
        let got = ours(id, src);
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
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

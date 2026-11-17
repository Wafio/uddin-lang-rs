//! Runs the cases of a batch with the `waf_*` builtins, on the request the case gives.
//!
//! The `ctx` member of a case holds the strings `method`, `path`, `query`, `ip`, `country`, `ua`, `body` and
//! `action`, the number `score`, an object of strings `headers`, an array of strings `detected`, any value
//! `json`, an object of strings `fields` and an object of arrays of strings `field_values`.

use std::collections::HashMap;

use serde_json::Value;
use serde_json::value::RawValue;
use uddin::jsonl::{CaseHost, Done};
use uddin::{CompileFailure, Limits, Outcome, Profile};
use uddin_waf::{End, Json, RequestView, WafRunner, WafScript, parse_json};

#[derive(Default)]
pub struct WafHost {
    runner: WafRunner,
}

/// The data of a case's request, owned, for a view to point at.
#[derive(Default)]
struct Owned {
    text: [Vec<u8>; 8],
    score: f64,
    headers: Vec<(Vec<u8>, Vec<u8>)>,
    detected: Vec<Vec<u8>>,
    json: Option<Json>,
    fields: HashMap<Vec<u8>, Vec<u8>>,
    field_values: HashMap<Vec<u8>, Vec<Vec<u8>>>,
}

const TEXT: [&str; 8] = [
    "method", "path", "query", "ip", "country", "ua", "body", "action",
];

fn strings(v: &Value, what: &str) -> Result<Vec<Vec<u8>>, String> {
    v.as_array()
        .ok_or_else(|| format!("{what} is not an array"))?
        .iter()
        .map(|s| {
            s.as_str()
                .map(|s| s.as_bytes().to_vec())
                .ok_or_else(|| format!("{what} holds a value that is not a string"))
        })
        .collect()
}

/// The text of `ctx.json` in the case line, as it was written.
fn json_text(line: &str) -> Result<Option<Vec<u8>>, String> {
    let bad = |e: serde_json::Error| e.to_string();
    let top: HashMap<&str, &RawValue> = serde_json::from_str(line).map_err(bad)?;
    let Some(ctx) = top.get("ctx") else {
        return Ok(None);
    };
    let ctx: HashMap<&str, &RawValue> = serde_json::from_str(ctx.get()).map_err(bad)?;
    Ok(ctx.get("json").map(|j| j.get().as_bytes().to_vec()))
}

fn parse(case: &Value, line: &str) -> Result<Owned, String> {
    let mut owned = Owned::default();
    let Some(ctx) = case.get("ctx") else {
        return Ok(owned);
    };
    for (i, key) in TEXT.iter().enumerate() {
        match ctx.get(key) {
            None => {}
            Some(Value::String(s)) => owned.text[i] = s.as_bytes().to_vec(),
            Some(_) => return Err(format!("ctx.{key} is not a string")),
        }
    }
    match ctx.get("score") {
        None => {}
        Some(Value::Number(n)) => owned.score = n.as_f64().unwrap_or(0.0),
        Some(_) => return Err("ctx.score is not a number".to_owned()),
    }
    if let Some(h) = ctx.get("headers") {
        let map = h.as_object().ok_or("ctx.headers is not an object")?;
        for (k, v) in map {
            let v = v
                .as_str()
                .ok_or("ctx.headers holds a value that is not a string")?;
            owned
                .headers
                .push((k.as_bytes().to_vec(), v.as_bytes().to_vec()));
        }
    }
    if let Some(d) = ctx.get("detected") {
        owned.detected = strings(d, "ctx.detected")?;
    }
    if let Some(text) = json_text(line)? {
        owned.json = Some(parse_json(&text).ok_or("ctx.json is not a JSON document")?);
    }
    if let Some(f) = ctx.get("fields") {
        let map = f.as_object().ok_or("ctx.fields is not an object")?;
        for (k, v) in map {
            let v = v
                .as_str()
                .ok_or("ctx.fields holds a value that is not a string")?;
            owned
                .fields
                .insert(k.as_bytes().to_vec(), v.as_bytes().to_vec());
        }
    }
    if let Some(f) = ctx.get("field_values") {
        let map = f.as_object().ok_or("ctx.field_values is not an object")?;
        for (k, v) in map {
            owned
                .field_values
                .insert(k.as_bytes().to_vec(), strings(v, "ctx.field_values")?);
        }
    }
    Ok(owned)
}

impl CaseHost for WafHost {
    type Compiled = WafScript;

    fn compile(&mut self, src: &[u8], profile: &Profile) -> Result<WafScript, CompileFailure> {
        WafScript::compile(src, profile)
    }

    fn run(
        &mut self,
        script: &WafScript,
        case: &Value,
        line: &str,
        limits: &Limits,
    ) -> Result<Done, String> {
        let owned = parse(case, line)?;
        let headers: Vec<(&[u8], &[u8])> = owned
            .headers
            .iter()
            .map(|(k, v)| (k.as_slice(), v.as_slice()))
            .collect();
        let detected: Vec<&[u8]> = owned.detected.iter().map(Vec::as_slice).collect();
        let field =
            |name: &[u8]| -> Vec<u8> { owned.fields.get(name).cloned().unwrap_or_default() };
        let field_values = |name: &[u8], each: &mut dyn FnMut(&[u8])| {
            for v in owned.field_values.get(name).into_iter().flatten() {
                each(v);
            }
        };
        let view = RequestView {
            method: &owned.text[0],
            path: &owned.text[1],
            query: &owned.text[2],
            ip: &owned.text[3],
            country: &owned.text[4],
            ua: &owned.text[5],
            body: &owned.text[6],
            action: &owned.text[7],
            score: owned.score,
            headers: &headers,
            detected: &detected,
            json: owned.json.as_ref(),
            field: Some(&field),
            field_values: Some(&field_values),
        };
        let run = self.runner.run(script, &view, limits, None);
        let (ended, panicked) = match run.end {
            End::Done => (Outcome::Ok, false),
            End::Error { class, text } => (Outcome::Error { class, text }, false),
            End::Limit { reason, text } => (Outcome::Limit { reason, text }, false),
            End::Panic => (Outcome::Ok, true),
        };
        Ok(Done {
            ended,
            verdict: run.verdict,
            panicked,
        })
    }

    fn printed(&self) -> &[u8] {
        self.runner.printed()
    }

    fn print_truncated(&self) -> bool {
        self.runner.print_truncated()
    }
}

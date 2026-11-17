//! The batch form of `check`, used to compare this implementation with the Go one.
//!
//! Input is one JSON object per line: `{"id": "...", "src": "..."}`, or `{"id": "...", "src_b64": "..."}` for
//! a source that is not valid UTF-8 (standard base64 with padding). Output is one line per case, in order:
//! `{"id": "...", "ok": bool, "stage": null|"size"|"tokenize"|"parse"|"allowlist", "forbidden": [...]}`.
//! `stage` names the first check that refused the source and `forbidden` lists the refused builtin names,
//! `"import"` included, and is empty unless the stage is `allowlist`. Blank input lines are skipped.

use std::io::{self, BufRead, Write};

use serde_json::Value;

use crate::{
    CheckError, CompileFailure, LimitReason, Limits, Outcome, Profile, RequestVars, Runner, Script,
    Verdict, check,
};

fn invalid(line: usize, what: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("line {line}: {what}"))
}

/// One input line: the id, the source and the whole object.
fn parse_case(number: usize, line: &str) -> io::Result<(String, Vec<u8>, Value)> {
    let case: Value = serde_json::from_str(line).map_err(|e| invalid(number, e))?;
    let id = case
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(number, "\"id\" is not a string"))?
        .to_owned();
    let src = match (case.get("src"), case.get("src_b64")) {
        (Some(Value::String(text)), None) => text.as_bytes().to_vec(),
        (None, Some(Value::String(text))) => decode_base64(text).map_err(|e| invalid(number, e))?,
        _ => {
            return Err(invalid(
                number,
                "need exactly one string field, \"src\" or \"src_b64\"",
            ));
        }
    };
    Ok((id, src, case))
}

pub fn check_jsonl(input: impl BufRead, mut out: impl Write, profile: &Profile) -> io::Result<()> {
    for (index, line) in input.lines().enumerate() {
        let number = index + 1;
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let (id, src, _) = parse_case(number, &line)?;
        let (stage, forbidden) = match check(&src, profile) {
            Ok(()) => (Value::Null, Vec::new()),
            Err(CheckError::Disallowed(names)) => (Value::from("allowlist"), names),
            Err(e) => (Value::from(e.stage()), Vec::new()),
        };
        writeln!(
            out,
            "{{\"id\":{},\"ok\":{},\"stage\":{},\"forbidden\":{}}}",
            Value::from(id),
            stage.is_null(),
            stage,
            Value::from(forbidden)
        )?;
    }
    Ok(())
}

/// The request of a run case: the fields of `ctx` that a script can read without a `waf_*` builtin.
struct Ctx {
    text: [Vec<u8>; 8],
    score: f64,
}

const CTX_TEXT: [&str; 8] = [
    "method", "path", "query", "ip", "country", "ua", "body", "action",
];

fn parse_ctx(case: &Value) -> Result<Ctx, String> {
    let mut ctx = Ctx {
        text: Default::default(),
        score: 0.0,
    };
    let Some(obj) = case.get("ctx") else {
        return Ok(ctx);
    };
    for (i, key) in CTX_TEXT.iter().enumerate() {
        match obj.get(key) {
            None => {}
            Some(Value::String(s)) => ctx.text[i] = s.as_bytes().to_vec(),
            Some(_) => return Err(format!("ctx.{key} is not a string")),
        }
    }
    match obj.get("score") {
        None => {}
        Some(Value::Number(n)) => ctx.score = n.as_f64().unwrap_or(0.0),
        Some(_) => return Err("ctx.score is not a number".to_owned()),
    }
    Ok(ctx)
}

/// How a case run ended, and what the script decided.
pub struct Done {
    pub ended: Outcome,
    pub verdict: Option<Verdict>,
    /// The run panicked inside the interpreter; `ended` is then not meaningful.
    pub panicked: bool,
}

/// The library that compiles and runs the scripts of a batch: the plain interpreter, or one with more builtins.
pub trait CaseHost {
    /// What a source compiles to.
    type Compiled;

    fn compile(&mut self, src: &[u8], profile: &Profile) -> Result<Self::Compiled, CompileFailure>;
    /// Runs `compiled` for the case, whose `ctx` member describes the request; `line` is the text of the case,
    /// for a host that needs the numbers in it as they were written. The error is a case that cannot be run.
    fn run(
        &mut self,
        compiled: &Self::Compiled,
        case: &Value,
        line: &str,
        limits: &Limits,
    ) -> Result<Done, String>;
    fn printed(&self) -> &[u8];
    fn print_truncated(&self) -> bool;
}

/// The host of the plain interpreter: the variables of a request, and no request builtins.
#[derive(Default)]
pub struct PlainHost {
    runner: Runner,
}

impl CaseHost for PlainHost {
    type Compiled = Script;

    fn compile(&mut self, src: &[u8], profile: &Profile) -> Result<Script, CompileFailure> {
        Script::compile(src, profile)
    }

    fn run(
        &mut self,
        script: &Script,
        case: &Value,
        _line: &str,
        limits: &Limits,
    ) -> Result<Done, String> {
        let ctx = parse_ctx(case)?;
        let vars = RequestVars {
            method: &ctx.text[0],
            path: &ctx.text[1],
            query: &ctx.text[2],
            ip: &ctx.text[3],
            country: &ctx.text[4],
            ua: &ctx.text[5],
            body: &ctx.text[6],
            score: ctx.score,
            action: &ctx.text[7],
        };
        let ended = self.runner.run(script, &vars, limits);
        Ok(Done {
            ended,
            verdict: self.runner.verdict(),
            panicked: false,
        })
    }

    fn printed(&self) -> &[u8] {
        self.runner.printed()
    }

    fn print_truncated(&self) -> bool {
        self.runner.print_truncated()
    }
}

/// Runs every case of `input` as a script rule runs: the source is checked, compiled and executed under the
/// `limits`, which a profile gives through [`Profile::limits`]. One line of
/// `{"id", "outcome", "verdict", "limit", "error_class", "error", "print_b64"}` per case, in order; `outcome`
/// is `ok`, `error`, `limit`, `panic` or `rejected` (with `stage` and `forbidden` as `check` gives them, and
/// `compile` for a `break` or `continue` outside a loop), or `unported` when the script names a builtin this build
/// lacks (with the names in `unported`).
pub fn run_jsonl(
    input: impl BufRead,
    out: impl Write,
    profile: &Profile,
    limits: Limits,
) -> io::Result<()> {
    run_jsonl_with(input, out, profile, limits, &mut PlainHost::default())
}

/// [`run_jsonl`] with the library that runs the scripts given.
pub fn run_jsonl_with<H: CaseHost>(
    input: impl BufRead,
    mut out: impl Write,
    profile: &Profile,
    mut limits: Limits,
    host: &mut H,
) -> io::Result<()> {
    limits.capture_print = true;
    for (index, line) in input.lines().enumerate() {
        let number = index + 1;
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let (id, src, case) = parse_case(number, &line)?;
        let line = match host.compile(&src, profile) {
            Err(CompileFailure::Check(e)) => {
                let forbidden = match &e {
                    CheckError::Disallowed(names) => names.clone(),
                    _ => Vec::new(),
                };
                format!(
                    "{{\"id\":{},\"outcome\":\"rejected\",\"stage\":{},\"forbidden\":{},\"verdict\":\"\",\"limit\":\"\",\"error_class\":\"\",\"error\":\"\",\"print_b64\":\"\"}}",
                    Value::from(id),
                    Value::from(e.stage()),
                    Value::from(forbidden)
                )
            }
            Err(CompileFailure::Flow(text)) => format!(
                "{{\"id\":{},\"outcome\":\"rejected\",\"stage\":\"compile\",\"forbidden\":[],\"verdict\":\"\",\"limit\":\"\",\"error_class\":\"\",\"error\":{},\"print_b64\":\"\"}}",
                Value::from(id),
                Value::from(text)
            ),
            Err(CompileFailure::Unported(names)) => format!(
                "{{\"id\":{},\"outcome\":\"unported\",\"unported\":{}}}",
                Value::from(id),
                Value::from(names)
            ),
            Ok(script) => {
                let done = host
                    .run(&script, &case, &line, &limits)
                    .map_err(|e| invalid(number, e))?;
                let (outcome, limit, class, error) = if done.panicked {
                    ("panic", String::new(), "", String::new())
                } else {
                    match done.ended {
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
                    }
                };
                let verdict = done.verdict.map_or("", Verdict::as_str);
                let mut line = format!(
                    "{{\"id\":{},\"outcome\":\"{outcome}\",\"verdict\":\"{verdict}\",\"limit\":{},\"error_class\":\"{class}\",\"error\":{},\"print_b64\":\"{}\"",
                    Value::from(id),
                    Value::from(limit.clone()),
                    Value::from(error),
                    encode_base64(host.printed())
                );
                if host.print_truncated() {
                    line.push_str(",\"print_truncated\":true");
                }
                if limit == "timeout" {
                    line.push_str(",\"timing\":true");
                }
                line.push('}');
                line
            }
        };
        writeln!(out, "{line}")?;
    }
    Ok(())
}

fn encode_base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n =
            chunk.iter().fold(0u32, |acc, &b| acc << 8 | u32::from(b)) << (8 * (3 - chunk.len()));
        for i in 0..=chunk.len() {
            out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
        }
        for _ in chunk.len()..3 {
            out.push('=');
        }
    }
    out
}

/// Standard base64 with padding, like Go's `base64.StdEncoding`.
fn decode_base64(text: &str) -> Result<Vec<u8>, &'static str> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err("src_b64 length is not a multiple of 4");
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for (i, quad) in bytes.chunks(4).enumerate() {
        let last = (i + 1) * 4 == bytes.len();
        let mut bits = 0u32;
        let mut padding = 0;
        for (j, &c) in quad.iter().enumerate() {
            let value = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' if last && j >= 2 => {
                    padding += 1;
                    0
                }
                _ => return Err("src_b64 is not valid base64"),
            };
            if padding > 0 && c != b'=' {
                return Err("src_b64 is not valid base64");
            }
            bits = bits << 6 | u32::from(value);
        }
        let [_, a, b, c] = bits.to_be_bytes();
        out.extend_from_slice(&[a, b, c][..3 - padding]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(input: &str) -> io::Result<String> {
        let mut out = Vec::new();
        check_jsonl(input.as_bytes(), &mut out, &Profile::embedded())?;
        Ok(String::from_utf8(out).expect("output is JSON"))
    }

    #[test]
    fn base64_decoding() {
        for (text, want) in [
            ("", ""),
            ("Zg==", "f"),
            ("Zm8=", "fo"),
            ("Zm9v", "foo"),
            ("Zm9vYg==", "foob"),
            ("Zm9vYmE=", "fooba"),
            ("Zm9vYmFy", "foobar"),
        ] {
            assert_eq!(
                decode_base64(text).as_deref(),
                Ok(want.as_bytes()),
                "{text}"
            );
        }
        assert_eq!(decode_base64("/+8A"), Ok(vec![0xff, 0xef, 0x00]));
        for bad in [
            "Zg=",
            "Zg=a",
            "Z===",
            "Zm9v=",
            "Z!==",
            "Zm 9",
            "Zg==Zg==",
            "====",
            "Zm9vYg=="[1..].to_string().as_str(),
        ] {
            assert!(decode_base64(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn one_line_per_case_in_order() {
        let input = [
            r#"{"id": "ok", "src": "x = 1"}"#,
            r#"{"id": "big", "src": "sleep(1)\nexit(2)\nx = sleep"}"#,
            r#"{"id": "imp", "src": "import \"a.din\""}"#,
            r#"{"id": "syn", "src": "x = "}"#,
            r#"{"id": "tok", "src": "x = @"}"#,
            "",
            r#"{"id": "raw\"id", "src_b64": "eCA9IP8="}"#,
            r#"{"id": "empty", "src": ""}"#,
        ]
        .join("\n");
        let want = [
            r#"{"id":"ok","ok":true,"stage":null,"forbidden":[]}"#,
            r#"{"id":"big","ok":false,"stage":"allowlist","forbidden":["exit","sleep"]}"#,
            r#"{"id":"imp","ok":false,"stage":"allowlist","forbidden":["import"]}"#,
            r#"{"id":"syn","ok":false,"stage":"parse","forbidden":[]}"#,
            r#"{"id":"tok","ok":false,"stage":"tokenize","forbidden":[]}"#,
            r#"{"id":"raw\"id","ok":false,"stage":"tokenize","forbidden":[]}"#,
            r#"{"id":"empty","ok":true,"stage":null,"forbidden":[]}"#,
        ]
        .join("\n");
        assert_eq!(run(&input).expect("valid input").trim_end(), want);
    }

    #[test]
    fn size_stage() {
        let src = "a".repeat(65_537);
        let out = run(&format!("{{\"id\": \"s\", \"src\": \"{src}\"}}")).expect("valid input");
        assert_eq!(
            out,
            "{\"id\":\"s\",\"ok\":false,\"stage\":\"size\",\"forbidden\":[]}\n"
        );
    }

    #[test]
    fn malformed_input_stops_with_the_line_number() {
        for (input, want) in [
            ("{\"id\": \"a\", \"src\": \"x\"}\nnot json", "line 2:"),
            (r#"{"src": "x"}"#, "line 1: \"id\" is not a string"),
            (r#"{"id": "a"}"#, "line 1: need exactly one"),
            (
                r#"{"id": "a", "src": "x", "src_b64": "eA=="}"#,
                "line 1: need exactly one",
            ),
            (r#"{"id": "a", "src_b64": "x"}"#, "line 1: src_b64 length"),
            (r#"{"id": "a", "src": 1}"#, "line 1: need exactly one"),
        ] {
            let e = run(input).expect_err(input);
            assert!(e.to_string().starts_with(want), "{input}: {e}");
        }
    }
}

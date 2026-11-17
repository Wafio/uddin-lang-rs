//! The seventeen builtins that read the request or decide what becomes of it. They find the request through the
//! variable `_waf_ctx`, as the host's other builtins find none: a script that gives that name another value
//! takes the request away from them, and each then answers as it does for a request with nothing in it.

use std::borrow::Cow;

use uddin_vm::{
    BuiltinDef, Exc, Json, LimitReason, ListId, MapId, Rt, Value, Verdict, quote_for_error,
    type_name,
};

use crate::{glob, net};

pub(crate) const TABLE: &[BuiltinDef] = &[
    BuiltinDef::new("waf_header", waf_header, 1),
    BuiltinDef::new("waf_query", waf_query, 1),
    BuiltinDef::new("waf_body_contains", waf_body_contains, 1),
    BuiltinDef::new("waf_field", waf_field, 1),
    BuiltinDef::new("waf_values", waf_values, 1),
    BuiltinDef::new("waf_json", waf_json, 0),
    BuiltinDef::new("waf_cidr_match", waf_cidr_match, 2),
    BuiltinDef::new("waf_path_match", waf_path_match, 1),
    BuiltinDef::new("waf_score", waf_score, 0),
    BuiltinDef::new("waf_action", waf_action, 0),
    BuiltinDef::new("waf_detected", waf_detected, 1),
    BuiltinDef::new("waf_detected_any", waf_detected_any, 0),
    BuiltinDef::new("waf_detected_list", waf_detected_list, 0),
    BuiltinDef::new("waf_return", waf_return, 1),
    BuiltinDef::new("waf_block", waf_block, -1),
    BuiltinDef::new("waf_allow", waf_allow, -1),
    BuiltinDef::new("waf_log", waf_log, -1),
];

/// The error value of a call whose argument is not a string.
fn not_a_string(rt: &mut Rt<'_>, name: &str, what: &str, got: Value) -> Result<Value, Exc> {
    let msg = format!("{name}() requires {what}, not {}", type_name(got));
    rt.error_value(msg.as_bytes())
}

/// A copy of the string argument, or the error value the call answers with. The copy lets the builtin allocate
/// while it holds the text.
fn text(
    rt: &mut Rt<'_>,
    name: &str,
    args: &[Value],
    at: usize,
) -> Result<Vec<u8>, Result<Value, Exc>> {
    match args[at] {
        Value::Str(s) => Ok(rt.heap.str_bytes(s).to_vec()),
        other => Err(not_a_string(rt, name, "a string argument", other)),
    }
}

fn string_value(rt: &mut Rt<'_>, bytes: &[u8]) -> Result<Value, Exc> {
    Ok(Value::Str(rt.heap.new_str(bytes).map_err(rt.oom())?))
}

fn waf_header(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let name = match text(rt, "waf_header", args, 0) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let Some(req) = rt.request() else {
        return Ok(Value::Null);
    };
    let wanted = uddin_std::lower_text(&name);
    for i in 0..req.header_count() {
        let (key, value) = req.header(i);
        if uddin_std::lower_text(key) == wanted {
            return string_value(rt, value);
        }
    }
    Ok(Value::Null)
}

/// `%` and two hex digits for a byte, `+` for a space; `None` for an escape that is not whole.
fn query_unescape(s: &[u8]) -> Option<Vec<u8>> {
    let hex = |b: u8| char::from(b).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'%' => {
                let hi = s.get(i + 1).copied().and_then(hex)?;
                let lo = s.get(i + 2).copied().and_then(hex)?;
                out.push(hi << 4 | lo);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Some(out)
}

fn waf_query(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let name = match text(rt, "waf_query", args, 0) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let Some(req) = rt.request() else {
        return Ok(Value::Null);
    };
    let query = req.query();
    if query.is_empty() {
        return Ok(Value::Null);
    }
    for pair in query.split(|&b| b == b'&') {
        let Some(eq) = pair.iter().position(|&b| b == b'=') else {
            continue;
        };
        let key = query_unescape(&pair[..eq]).unwrap_or_default();
        if key == name {
            let value = query_unescape(&pair[eq + 1..]).unwrap_or_default();
            return string_value(rt, &value);
        }
    }
    Ok(Value::Null)
}

fn waf_body_contains(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let needle = match text(rt, "waf_body_contains", args, 0) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let Some(req) = rt.request() else {
        return Ok(Value::Bool(false));
    };
    Ok(Value::Bool(
        needle.is_empty() || memchr::memmem::find(req.body(), &needle).is_some(),
    ))
}

fn waf_field(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let name = match text(rt, "waf_field", args, 0) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let Some(req) = rt.request() else {
        return string_value(rt, b"");
    };
    let value = req.field(&name);
    string_value(rt, &value)
}

fn waf_values(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let name = match text(rt, "waf_values", args, 0) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let mut values: Vec<Vec<u8>> = Vec::new();
    if let Some(req) = rt.request() {
        req.field_values(&name, &mut |v| values.push(v.to_vec()));
    }
    let list = rt.heap.new_list(0, 0).map_err(rt.oom())?;
    for v in &values {
        let item = string_value(rt, v)?;
        rt.heap.list_append(list, &[item]).map_err(rt.oom())?;
    }
    Ok(Value::List(list))
}

/// What a decoded number is to a script: an integer when it is one, else a float, else its own text.
fn number(rt: &mut Rt<'_>, text: &str) -> Result<Value, Exc> {
    if let Ok(i) = text.parse::<i64>() {
        return Ok(Value::Int(i));
    }
    match text.parse::<f64>() {
        Ok(f) if f.is_finite() => Ok(Value::Float(f)),
        _ => string_value(rt, text.as_bytes()),
    }
}

/// A container being built from a JSON value: where its children are, and the next one to add.
enum Frame<'j> {
    List(ListId, &'j [Json], usize),
    Map(MapId, &'j [(Vec<u8>, Json)], usize, Option<&'j [u8]>),
}

/// The script value for `root`, built without recursion, since a document can nest thousands of levels.
fn to_value(rt: &mut Rt<'_>, root: &Json) -> Result<Value, Exc> {
    let mut stack: Vec<Frame<'_>> = Vec::new();
    let mut entering: Option<&Json> = Some(root);
    let mut finished: Option<Value> = None;
    loop {
        if let Some(node) = entering.take() {
            match node {
                Json::Null => finished = Some(Value::Null),
                Json::Bool(b) => finished = Some(Value::Bool(*b)),
                Json::Number(t) => finished = Some(number(rt, t)?),
                Json::String(s) => finished = Some(string_value(rt, s)?),
                Json::Array(items) => {
                    let id = rt
                        .heap
                        .new_list(items.len(), items.len())
                        .map_err(rt.oom())?;
                    stack
                        .try_reserve(1)
                        .map_err(|_| Exc::limit(LimitReason::Memory, rt.pos))?;
                    stack.push(Frame::List(id, items, 0));
                }
                Json::Object(entries) => {
                    let id = rt.heap.new_map(entries.len()).map_err(rt.oom())?;
                    stack
                        .try_reserve(1)
                        .map_err(|_| Exc::limit(LimitReason::Memory, rt.pos))?;
                    stack.push(Frame::Map(id, entries, 0, None));
                }
            }
        }
        if let Some(v) = finished.take() {
            match stack.last_mut() {
                None => return Ok(v),
                Some(Frame::List(id, ..)) => {
                    rt.heap.list_push(*id, v).map_err(rt.oom())?;
                }
                Some(Frame::Map(id, _, _, key)) => {
                    let key = key.take().unwrap_or_default();
                    let k = rt.heap.new_str(key).map_err(rt.oom())?;
                    rt.heap.map_set(*id, k, v).map_err(rt.oom())?;
                }
            }
        }
        match stack.last_mut() {
            None => return Ok(Value::Null),
            Some(Frame::List(id, items, next)) => {
                if let Some(child) = items.get(*next) {
                    *next += 1;
                    entering = Some(child);
                } else {
                    finished = Some(Value::List(*id));
                    stack.pop();
                }
            }
            Some(Frame::Map(id, entries, next, key)) => {
                if let Some((k, child)) = entries.get(*next) {
                    *next += 1;
                    *key = Some(k.as_slice());
                    entering = Some(child);
                } else {
                    finished = Some(Value::Map(*id));
                    stack.pop();
                }
            }
        }
    }
}

fn waf_json(rt: &mut Rt<'_>, _args: &[Value]) -> Result<Value, Exc> {
    let Some(doc) = rt.request().and_then(|req| req.json()) else {
        return Ok(Value::Null);
    };
    to_value(rt, &doc)
}

fn waf_cidr_match(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let mut parts = [Vec::new(), Vec::new()];
    for (i, part) in parts.iter_mut().enumerate() {
        match args[i] {
            Value::Str(s) => *part = rt.heap.str_bytes(s).to_vec(),
            other => return not_a_string(rt, "waf_cidr_match", "string arguments", other),
        }
    }
    Ok(Value::Bool(net::contains(&parts[1], &parts[0])))
}

fn waf_path_match(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let pattern = match text(rt, "waf_path_match", args, 0) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let Some(req) = rt.request() else {
        return Ok(Value::Bool(false));
    };
    match glob::matches(&pattern, req.path()) {
        Some(hit) => Ok(Value::Bool(hit)),
        None => Err(Exc::limit(LimitReason::Canceled, rt.pos)),
    }
}

fn waf_score(rt: &mut Rt<'_>, _args: &[Value]) -> Result<Value, Exc> {
    Ok(Value::Float(rt.request().map_or(0.0, |req| req.score())))
}

fn waf_action(rt: &mut Rt<'_>, _args: &[Value]) -> Result<Value, Exc> {
    let action: Cow<'_, [u8]> = match rt.request() {
        Some(req) => Cow::Borrowed(req.action()),
        None => Cow::Borrowed(b""),
    };
    string_value(rt, &action)
}

fn waf_detected(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let category = match text(rt, "waf_detected", args, 0) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let Some(req) = rt.request() else {
        return Ok(Value::Bool(false));
    };
    let wanted = uddin_std::lower_text(&category);
    let found = (0..req.detected_count()).any(|i| uddin_std::lower_text(req.detected(i)) == wanted);
    Ok(Value::Bool(found))
}

fn waf_detected_any(rt: &mut Rt<'_>, _args: &[Value]) -> Result<Value, Exc> {
    Ok(Value::Bool(
        rt.request().is_some_and(|req| req.detected_count() > 0),
    ))
}

fn waf_detected_list(rt: &mut Rt<'_>, _args: &[Value]) -> Result<Value, Exc> {
    let n = rt.request().map_or(0, |req| req.detected_count());
    let list = rt.heap.new_list(n, n).map_err(rt.oom())?;
    for i in 0..n {
        let category = rt
            .request()
            .map_or(Vec::new(), |req| req.detected(i).to_vec());
        let item = string_value(rt, &category)?;
        rt.heap.list_push(list, item).map_err(rt.oom())?;
    }
    Ok(Value::List(list))
}

/// Writes the decision and ends the function that called, as `return` does, with the verdict in capitals as its
/// value.
fn decide(rt: &mut Rt<'_>, verdict: Verdict) -> Result<Value, Exc> {
    let word: &[u8] = match verdict {
        Verdict::Allow => b"ALLOW",
        Verdict::Log => b"LOG",
        Verdict::Block => b"BLOCK",
    };
    rt.decide(verdict);
    let value = string_value(rt, word)?;
    Err(Exc::Return(value, rt.pos))
}

fn waf_return(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let action = match text(rt, "waf_return", args, 0) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let verdict = match uddin_std::upper_text(uddin_std::trim_space(&action)).as_slice() {
        b"ALLOW" => Verdict::Allow,
        b"LOG" => Verdict::Log,
        b"BLOCK" => Verdict::Block,
        _ => {
            let mut msg = b"waf_return() invalid action ".to_vec();
            quote_for_error(&action, &mut msg);
            msg.extend_from_slice(b": must be ALLOW, LOG, or BLOCK");
            return rt.error_value(&msg);
        }
    };
    decide(rt, verdict)
}

fn with_message(rt: &mut Rt<'_>, name: &str, args: &[Value], v: Verdict) -> Result<Value, Exc> {
    if args.len() > 1 {
        let msg = format!("{name}() accepts at most 1 optional argument");
        return rt.error_value(msg.as_bytes());
    }
    decide(rt, v)
}

fn waf_block(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    with_message(rt, "waf_block", args, Verdict::Block)
}

fn waf_allow(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    with_message(rt, "waf_allow", args, Verdict::Allow)
}

fn waf_log(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    with_message(rt, "waf_log", args, Verdict::Log)
}

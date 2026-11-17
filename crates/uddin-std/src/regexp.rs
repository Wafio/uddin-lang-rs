//! Regular expressions: is_regex_match regex_match regex_find regex_find_all regex_replace regex_split.
//!
//! The dialect is Go's RE2 with the Perl extensions, searched leftmost-first over characters, where a byte that is
//! not valid UTF-8 is U+FFFD. `syntax` parses the pattern, words the errors and shapes the tree the way the
//! reference does; `lower` turns the tree into a `regex-syntax` expression for `regex-automata`, and `cache` keeps
//! the sixteen patterns a thread used last. The memory the reference charges for parsing and compiling is charged
//! on every call, whether the pattern came from the cache or not.
//!
//! What differs from the reference:
//! - the sets behind `\p{Name}` and the case folding of classes come from `regex-syntax`, so they follow its
//!   Unicode version, not the reference's 15.0;
//! - the search itself is the one of `regex-automata`; the two agree on which match is leftmost-first, and the
//!   sweep in `conformance/regex-sweep.sh` compares them on generated patterns;
//! - the limit on the size of a program and the budget of regex work are not enforced as such: the machine has
//!   no limit reason for them yet (see `limits`);
//! - the matcher of a pattern may take 4 MiB at most, which also bounds the time to build it. A pattern within
//!   the reference's limits whose matcher is bigger, such as a hundred `\pL` in a row, ends the run as memory.

mod cache;
mod expand;
#[cfg(test)]
mod facts;
mod iter;
mod limits;
mod lower;
mod search;
mod shape;
mod syntax;
mod uniclass;

use uddin_vm::{
    BuiltinDef, Exc, Fresh, Heap, LIST_COST, REGEX_BASE_COST, REGEX_BYTES_PER_INST,
    REGEX_INST_COST, REGEX_MATCH_COST, REGEX_PARSE_BYTE_COST, Rt, Str, Value, WORD_COST,
    checked_product, sum_estimate, type_name,
};

use cache::Ready;
use iter::Searcher;
use limits::{MAX_PROGRAM, charge_work, compile_units, out_of_memory, too_large};
use search::Hay;

pub(crate) const TABLE: &[BuiltinDef] = &[
    BuiltinDef::new("is_regex_match", is_regex_match, 2).guard_regex(guard_first),
    BuiltinDef::new("regex_match", regex_match, 2).guard_regex(guard_single),
    BuiltinDef::new("regex_find", regex_find, 2).guard_regex(guard_single),
    BuiltinDef::new("regex_find_all", regex_find_all, 2)
        .guard_regex(guard_multi)
        .reserve(reserve_matches)
        .fresh(Fresh::Elements),
    BuiltinDef::new("regex_replace", regex_replace, 3)
        .guard(guard_replace)
        .guard_regex(guard_multi)
        .reserve(reserve_replace),
    BuiltinDef::new("regex_split", regex_split, 2)
        .guard_regex(guard_multi)
        .reserve(reserve_matches)
        .fresh(Fresh::Elements),
];

fn str_arg(args: &[Value], i: usize) -> Option<Str> {
    match args.get(i) {
        Some(&Value::Str(s)) => Some(s),
        _ => None,
    }
}

/// Bounds the pattern in `args[at]` and charges the memory its parse and compile cost: 256 bytes per byte of
/// pattern, then a fixed part and a kilobyte per instruction, twice for the builtins that search past the first
/// match. Returns what it charged.
fn guard_pattern(rt: &mut Rt<'_>, args: &[Value], at: usize, multi: bool) -> Result<i64, Exc> {
    let Some(p) = str_arg(args, at) else {
        return Ok(0);
    };
    let len = i64::from(p.len);
    let pos = rt.pos;
    let limit = |reason| Exc::limit(reason, pos);
    if len / REGEX_BYTES_PER_INST > MAX_PROGRAM {
        return Err(too_large(&limit));
    }
    let parse = checked_product(REGEX_PARSE_BYTE_COST, len);
    rt.meter.charge(pos, parse)?;
    let estimate = cache::with_entry(rt.heap.str_bytes(p), |e| {
        e.parsed.as_ref().ok().map(|i| i.estimate)
    });
    let Some(insts) = estimate else {
        return Ok(parse);
    };
    if insts > MAX_PROGRAM {
        return Err(too_large(&limit));
    }
    let mut compile = sum_estimate(REGEX_BASE_COST, checked_product(REGEX_INST_COST, insts));
    if multi {
        compile = sum_estimate(compile, compile);
    }
    rt.meter.charge(pos, compile)?;
    Ok(parse + compile)
}

fn guard_first(rt: &mut Rt<'_>, args: &[Value]) -> Result<i64, Exc> {
    guard_pattern(rt, args, 0, false)
}

fn guard_single(rt: &mut Rt<'_>, args: &[Value]) -> Result<i64, Exc> {
    guard_pattern(rt, args, 1, false)
}

fn guard_multi(rt: &mut Rt<'_>, args: &[Value]) -> Result<i64, Exc> {
    guard_pattern(rt, args, 1, true)
}

/// The text a replacement makes is at most the replacement at every position of the text: refuse one that
/// could pass the size of a value.
fn guard_replace(rt: &mut Rt<'_>, args: &[Value]) -> Result<i64, Exc> {
    let len = |i| str_arg(args, i).map_or(0, |s| i64::from(s.len));
    let (text, repl) = (len(0), len(2));
    rt.size_check(text + checked_product(repl, text + 1))?;
    Ok(0)
}

/// Charges the most a search can make of a list: a match at every position.
fn reserve_matches(rt: &mut Rt<'_>, args: &[Value]) -> Result<i64, Exc> {
    let Some(text) = str_arg(args, 0) else {
        return Ok(0);
    };
    let matches = i64::from(text.len) + 1;
    rt.reserve(sum_estimate(
        LIST_COST,
        checked_product(REGEX_MATCH_COST, matches),
    ))
}

/// Charges the most a replacement can make of a string: the replacement at every position of the text.
fn reserve_replace(rt: &mut Rt<'_>, args: &[Value]) -> Result<i64, Exc> {
    let len = |i| str_arg(args, i).map_or(0, |s| i64::from(s.len));
    let (text, repl) = (len(0), len(2));
    rt.reserve(sum_estimate(
        WORD_COST + text,
        checked_product(repl, text + 1),
    ))
}

/// Why a pattern cannot be searched with.
enum Unusable {
    /// It does not parse; the text of the error, as the reference words it.
    Syntax(Vec<u8>),
    TooLarge,
}

/// Runs `f` with the parse and matcher of `pattern`.
fn with_regex<R>(
    pattern: &[u8],
    f: impl FnOnce(&cache::Info, &mut cache::Engine) -> R,
) -> Result<R, Unusable> {
    cache::with_entry(pattern, |e| match e.ready() {
        Ready::Syntax(err) => Err(Unusable::Syntax(err.text())),
        Ready::TooLarge => Err(Unusable::TooLarge),
        Ready::Search(info, engine) => Ok(f(info, engine)),
    })
}

fn type_msg(fun: &str, which: &str, what: &str, v: Value) -> Vec<u8> {
    format!(
        "{fun}() requires {which} argument to be a string{what}, not {}",
        type_name(v)
    )
    .into_bytes()
}

/// The error value for a pattern the call cannot use, or the limit when its compiled form is too large.
fn unusable(rt: &mut Rt<'_>, why: Unusable) -> Result<Value, Exc> {
    match why {
        Unusable::TooLarge => Err(out_of_memory(&|r| Exc::limit(r, rt.pos))),
        Unusable::Syntax(text) => {
            let mut msg = b"invalid regex pattern: ".to_vec();
            msg.extend_from_slice(&text);
            rt.error_value(&msg)
        }
    }
}

/// The text and pattern of the builtins that take the text first, or the error value for what is wrong.
fn text_and_pattern(
    rt: &mut Rt<'_>,
    name: &str,
    args: &[Value],
) -> Result<Result<(Str, Str), Value>, Exc> {
    let [a, b] = args else {
        return Err(rt.arity_error(name, 2, args.len()));
    };
    let Value::Str(text) = *a else {
        let msg = type_msg(name, "first", "", *a);
        return Ok(Err(rt.error_value(&msg)?));
    };
    let Value::Str(pattern) = *b else {
        let msg = type_msg(name, "second", " pattern", *b);
        return Ok(Err(rt.error_value(&msg)?));
    };
    Ok(Ok((text, pattern)))
}

/// The part of `text` from `start` to `end`, sharing its bytes.
fn substring(text: Str, start: usize, end: usize) -> Str {
    Str {
        off: text.off + start as u32,
        len: (end - start) as u32,
    }
}

fn string_list(rt: &mut Rt<'_>, text: Str, spans: &[(u32, u32)]) -> Result<Value, Exc> {
    let id = rt
        .heap
        .new_list(spans.len(), spans.len())
        .map_err(rt.oom())?;
    for &(a, b) in spans {
        rt.heap
            .list_push(id, Value::Str(substring(text, a as usize, b as usize)))
            .map_err(rt.oom())?;
    }
    Ok(Value::List(id))
}

/// The compile work the reference charges for the pattern of a call: the program, and for the builtins that
/// search past the first match the program of the search that resumes in the middle of the text too.
fn compile_work(info: &cache::Info, multi: bool) -> u64 {
    let mut units = compile_units(info.program);
    if multi {
        units += compile_units(info.after_program);
    }
    units
}

fn is_regex_match(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let [a, b] = args else {
        return Err(rt.arity_error("regex_match", 2, args.len()));
    };
    let Value::Str(pattern) = *a else {
        return Err(rt.type_error("regex() requires first argument to be a string"));
    };
    let Value::Str(text) = *b else {
        return Err(rt.type_error("regex() requires second argument to be a string"));
    };
    match is_match(rt, text, pattern)? {
        Ok(found) => Ok(Value::Bool(found)),
        Err(Unusable::Syntax(_)) => Ok(Value::Bool(false)),
        Err(Unusable::TooLarge) => Err(out_of_memory(&|r| Exc::limit(r, rt.pos))),
    }
}

fn is_match(rt: &mut Rt<'_>, text: Str, pattern: Str) -> Result<Result<bool, Unusable>, Exc> {
    let pos = rt.pos;
    let limit = |reason| Exc::limit(reason, pos);
    let meter = &mut *rt.meter;
    let heap: &Heap = &*rt.heap;
    let Some(hay) = Hay::new(heap.str_bytes(text)) else {
        return Err(out_of_memory(&limit));
    };
    let ran = with_regex(heap.str_bytes(pattern), |info, eng| {
        charge_work(meter, &limit, compile_work(info, false))?;
        let end = hay.earliest_end(&eng.re, &mut eng.cache);
        let read = end.map_or(hay.len(), |e| hay.advance(e, 2));
        charge_work(meter, &limit, info.program.saturating_mul(read as u64 + 1))?;
        Ok::<_, Exc>(end.is_some())
    });
    match ran {
        Ok(found) => Ok(Ok(found?)),
        Err(why) => Ok(Err(why)),
    }
}

fn regex_match(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let (text, pattern) = match text_and_pattern(rt, "regex_match", args)? {
        Ok(both) => both,
        Err(error) => return Ok(error),
    };
    match is_match(rt, text, pattern)? {
        Ok(found) => Ok(Value::Bool(found)),
        Err(why) => unusable(rt, why),
    }
}

/// Runs `body` with a searcher over `text` for `pattern`, after charging the compile work of the call.
fn searching<R>(
    rt: &mut Rt<'_>,
    text: Str,
    pattern: Str,
    multi: bool,
    body: impl FnOnce(&mut Searcher<'_>, &cache::Info, &Heap) -> Result<R, Exc>,
) -> Result<Result<R, Unusable>, Exc> {
    let pos = rt.pos;
    let limit = |reason| Exc::limit(reason, pos);
    let meter = &mut *rt.meter;
    let heap: &Heap = &*rt.heap;
    let Some(hay) = Hay::new(heap.str_bytes(text)) else {
        return Err(out_of_memory(&limit));
    };
    let ran = with_regex(heap.str_bytes(pattern), |info, eng| {
        charge_work(meter, &limit, compile_work(info, multi))?;
        let mut searcher = Searcher {
            hay: &hay,
            re: &eng.re,
            cache: &mut eng.cache,
            meter,
            program: info.program,
            after_program: info.after_program,
            limit: &limit,
        };
        body(&mut searcher, info, heap)
    });
    match ran {
        Ok(r) => Ok(Ok(r?)),
        Err(why) => Ok(Err(why)),
    }
}

fn regex_find(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let (text, pattern) = match text_and_pattern(rt, "regex_find", args)? {
        Ok(both) => both,
        Err(error) => return Ok(error),
    };
    match searching(rt, text, pattern, false, |s, _, _| s.next(0))? {
        Ok(Some((a, b))) if b > a => Ok(Value::Str(substring(text, a, b))),
        Ok(_) => Ok(Value::Null),
        Err(why) => unusable(rt, why),
    }
}

fn regex_find_all(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let (text, pattern) = match text_and_pattern(rt, "regex_find_all", args)? {
        Ok(both) => both,
        Err(error) => return Ok(error),
    };
    match searching(rt, text, pattern, true, |s, _, _| s.find_all())? {
        Ok(spans) => string_list(rt, text, &spans),
        Err(why) => unusable(rt, why),
    }
}

fn regex_split(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let (text, pattern) = match text_and_pattern(rt, "regex_split", args)? {
        Ok(both) => both,
        Err(error) => return Ok(error),
    };
    let pattern_empty = pattern.len == 0;
    match searching(rt, text, pattern, true, |s, _, _| s.split(pattern_empty))? {
        Ok(spans) => string_list(rt, text, &spans),
        Err(why) => unusable(rt, why),
    }
}

fn regex_replace(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let [a, b, c] = args else {
        return Err(rt.arity_error("regex_replace", 3, args.len()));
    };
    let Value::Str(text) = *a else {
        let msg = type_msg("regex_replace", "first", "", *a);
        return rt.error_value(&msg);
    };
    let Value::Str(pattern) = *b else {
        let msg = type_msg("regex_replace", "second", " pattern", *b);
        return rt.error_value(&msg);
    };
    let Value::Str(repl) = *c else {
        let msg = type_msg("regex_replace", "third", " (replacement)", *c);
        return rt.error_value(&msg);
    };
    match searching(rt, text, pattern, true, |s, info, heap| {
        s.replace(heap.str_bytes(text), heap.str_bytes(repl), &info.names)
    })? {
        Ok(out) => Ok(Value::Str(rt.heap.new_str(&out).map_err(rt.oom())?)),
        Err(why) => unusable(rt, why),
    }
}

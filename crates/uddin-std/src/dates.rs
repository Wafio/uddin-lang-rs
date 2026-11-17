//! Dates: date_add date_between date_compare date_diff date_format date_format_new date_now date_parse date_subtract time_now.
//!
//! A date is a string. The builtins read it as RFC 3339 first, then as `2006-01-02T15:04:05`, `2006-01-02` and
//! `2006-01-02 15:04:05`, a time with no offset being UTC and one with an offset being in the host's zone when
//! the host's zone has that offset then, and in a zone of its own otherwise. Results are RFC 3339 strings.
//!
//! `date_parse(text, layout)` and `date_format_new(text, layout, new_layout)` read and write the layouts of Go's
//! `time` package. `date_add` and `date_subtract` take a duration text (`"2h45m"`) or a unit and an amount, an
//! integer or a float. The reference's argument table refuses those documented forms (`date_parse` is declared to
//! take one argument, `date_format_new` two, `date_add` and `date_subtract` three), and its `date_add` refuses an
//! integer amount; here each takes the arguments its documentation gives.
//!
//! What else differs from the reference: `date_diff` and `date_compare` give an integer of a kind the reference's
//! arithmetic refuses (its `typeof` is `unknown`, `x + 1` is a type error), which the machine has no value for, so
//! here they are plain integers.

mod civil;
mod duration;
mod layout;

use civil::{Loc, Time};
use layout::{RFC3339, format, format_rfc3339};
use uddin_vm::{BuiltinDef, Count, Exc, Rt, Value, float_to_int};

pub(crate) const TABLE: &[BuiltinDef] = &[
    BuiltinDef::new("date_add", date_add, -1),
    BuiltinDef::new("date_between", date_between, 3),
    BuiltinDef::new("date_compare", date_compare, 2),
    BuiltinDef::new("date_diff", date_diff, -1),
    BuiltinDef::new("date_format", date_format, 2),
    BuiltinDef::new("date_format_new", date_format_new, -1),
    BuiltinDef::new("date_now", date_now, 0).fast_path(),
    BuiltinDef::new("date_parse", date_parse, -1),
    BuiltinDef::new("date_subtract", date_subtract, -1),
    BuiltinDef::new("time_now", time_now, 0).fast_path(),
];

const HOUR: i64 = 3_600_000_000_000;
const MINUTE: i64 = 60_000_000_000;
const SECOND: i64 = 1_000_000_000;

const FORMATS: [&[u8]; 4] = [
    RFC3339,
    b"2006-01-02T15:04:05",
    b"2006-01-02",
    b"2006-01-02 15:04:05",
];

/// The bytes of a string argument.
fn string<'a>(rt: &'a Rt<'_>, v: Value) -> Option<&'a [u8]> {
    match v {
        Value::Str(s) => Some(rt.heap.str_bytes(s)),
        _ => None,
    }
}

/// Reads a date in the first of the four layouts that fits; when none does, the error of the last.
fn parse_date(s: &[u8]) -> Result<Time, Vec<u8>> {
    let mut last = Vec::new();
    for layout in FORMATS {
        match layout::parse(layout, s) {
            Ok(t) => return Ok(t),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// An error value made of `parts`, which are joined as they are.
fn error(rt: &mut Rt<'_>, parts: &[&[u8]]) -> Result<Value, Exc> {
    rt.error_value(&parts.concat())
}

fn text_value(rt: &mut Rt<'_>, bytes: &[u8]) -> Result<Value, Exc> {
    Ok(Value::Str(rt.heap.new_str(bytes).map_err(rt.oom())?))
}

fn date_now(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    if !args.is_empty() {
        return Err(rt.arity_error("date_now", 0, args.len()));
    }
    let (sec, nsec) = civil::now();
    text_value(rt, &format_rfc3339(&Time::from_unix(sec, nsec, Loc::Local)))
}

fn time_now(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    if !args.is_empty() {
        return Err(rt.arity_error("time_now", 0, args.len()));
    }
    let (sec, nsec) = civil::now();
    Ok(Value::Int(
        sec.wrapping_mul(1000)
            .wrapping_add(i64::from(nsec) / 1_000_000),
    ))
}

/// The layout of a user, with the short names of the reference's replacer put in as the layout elements they stand
/// for: `YYYY` for the year, `MM` the month, `DD` the day, `hh` the hour, `mm` the minute, `ss` the second, `ee` and
/// `EE` the weekday, `nn` and `NN` the month by name.
fn expand_layout(layout: &[u8]) -> Vec<u8> {
    const NAMES: [(&[u8], &[u8]); 10] = [
        (b"YYYY", b"2006"),
        (b"MM", b"01"),
        (b"DD", b"02"),
        (b"hh", b"15"),
        (b"mm", b"04"),
        (b"ss", b"05"),
        (b"ee", b"Mon"),
        (b"EE", b"Monday"),
        (b"nn", b"Jan"),
        (b"NN", b"January"),
    ];
    let mut out = Vec::with_capacity(layout.len() + 8);
    let mut i = 0;
    'scan: while i < layout.len() {
        for (old, new) in NAMES {
            if layout[i..].starts_with(old) {
                out.extend_from_slice(new);
                i += old.len();
                continue 'scan;
            }
        }
        out.push(layout[i]);
        i += 1;
    }
    out
}

fn date_format(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let [date, layout] = args else {
        return Err(rt.arity_error("date_format", 2, args.len()));
    };
    let Some(date) = string(rt, *date) else {
        return Err(rt.type_error("date_format() requires first argument to be a string"));
    };
    let Some(layout) = string(rt, *layout) else {
        return Err(rt.type_error("date_format() requires second argument to be a string"));
    };
    let Ok(time) = layout::parse(RFC3339, date) else {
        return Ok(Value::Null);
    };
    let out = format(&time, &expand_layout(layout));
    text_value(rt, &out)
}

/// A string argument, or the error value the builtin gives for it. `ordinal` is the word for its place.
fn text_arg(
    rt: &mut Rt<'_>,
    name: &str,
    args: &[Value],
    at: usize,
    ordinal: &str,
) -> Result<Vec<u8>, Result<Value, Exc>> {
    match string(rt, args[at]) {
        Some(s) => Ok(s.to_vec()),
        None => {
            let msg = format!("{name}() requires {ordinal} argument to be a string");
            Err(rt.error_value(msg.as_bytes()))
        }
    }
}

fn date_parse(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    if args.len() != 2 {
        let msg = format!(
            "date_parse() requires exactly 2 arguments, got {}",
            args.len()
        );
        return rt.error_value(msg.as_bytes());
    }
    let date = match text_arg(rt, "date_parse", args, 0, "first") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let layout = match text_arg(rt, "date_parse", args, 1, "second") {
        Ok(s) => s,
        Err(e) => return e,
    };
    rt.work(Count::Free, (date.len() + layout.len()) as u64)?;
    match layout::parse(&layout, &date) {
        Ok(t) => text_value(rt, &format_rfc3339(&t)),
        Err(e) => error(rt, &[b"date_parse() failed to parse date: ", &e]),
    }
}

fn date_format_new(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    if args.len() != 3 {
        let msg = format!(
            "date_format_new() requires exactly 3 arguments, got {}",
            args.len()
        );
        return rt.error_value(msg.as_bytes());
    }
    let date = match text_arg(rt, "date_format_new", args, 0, "first") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let input = match text_arg(rt, "date_format_new", args, 1, "second") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let output = match text_arg(rt, "date_format_new", args, 2, "third") {
        Ok(s) => s,
        Err(e) => return e,
    };
    rt.work(
        Count::Free,
        (date.len() + input.len() + output.len()) as u64,
    )?;
    match layout::parse(&input, &date) {
        Ok(t) => {
            let out = format(&t, &output);
            text_value(rt, &out)
        }
        Err(e) => error(rt, &[b"date_format_new() failed to parse date: ", &e]),
    }
}

/// The unit in lower case, for comparing with the names of the units: ASCII letters, and the two other letters
/// that lower-case to one, the Kelvin sign and the dotted capital I.
fn lower(unit: &[u8]) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(unit) else {
        return Vec::new();
    };
    text.chars()
        .map(|c| match c {
            '\u{212A}' => 'k',
            '\u{130}' => 'i',
            c => c.to_ascii_lowercase(),
        })
        .collect::<String>()
        .into_bytes()
}

fn date_add(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    shift(rt, args, "date_add", false)
}

fn date_subtract(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    shift(rt, args, "date_subtract", true)
}

/// A date moved by a duration text, or by an amount of a unit; an integer amount is taken as it is and a float
/// is cut to an integer.
fn shift(rt: &mut Rt<'_>, args: &[Value], name: &str, subtract: bool) -> Result<Value, Exc> {
    if args.len() < 2 {
        return error(rt, &[name.as_bytes(), b" requires at least 2 arguments"]);
    }
    let Some(date) = string(rt, args[0]) else {
        return error(
            rt,
            &[name.as_bytes(), b": first argument must be a date string"],
        );
    };
    let date_len = date.len() as u64;
    let time = match parse_date(date) {
        Ok(t) => t,
        Err(e) => return error(rt, &[name.as_bytes(), b": failed to parse date: ", &e]),
    };
    rt.work(Count::Free, date_len)?;
    if args.len() == 2 {
        let Some(text) = string(rt, args[1]).map(<[u8]>::to_vec) else {
            return error(
                rt,
                &[
                    name.as_bytes(),
                    b": second argument must be a duration string",
                ],
            );
        };
        rt.work(Count::Free, text.len() as u64)?;
        let d = match duration::parse(&text) {
            Ok(d) => d,
            Err(e) => {
                return error(rt, &[name.as_bytes(), b": invalid duration format: ", &e]);
            }
        };
        let result = time.add(if subtract { d.wrapping_neg() } else { d });
        return text_value(rt, &format_rfc3339(&result));
    }
    if args.len() != 3 {
        return error(rt, &[name.as_bytes(), b": invalid number of arguments"]);
    }
    let Some(unit) = string(rt, args[1]).map(<[u8]>::to_vec) else {
        return error(
            rt,
            &[name.as_bytes(), b": second argument must be a unit string"],
        );
    };
    let n = match args[2] {
        Value::Int(i) => i,
        Value::Float(f) => float_to_int(f),
        _ => return error(rt, &[name.as_bytes(), b": third argument must be a number"]),
    };
    let n = if subtract { n.wrapping_neg() } else { n };
    let result = match lower(&unit).as_slice() {
        b"years" | b"year" => time.add_date(n, 0, 0),
        b"months" | b"month" => time.add_date(0, n, 0),
        b"days" | b"day" => time.add_date(0, 0, n),
        b"hours" | b"hour" => time.add(n.wrapping_mul(HOUR)),
        b"minutes" | b"minute" => time.add(n.wrapping_mul(MINUTE)),
        b"seconds" | b"second" => time.add(n.wrapping_mul(SECOND)),
        _ => {
            return error(rt, &[name.as_bytes(), b": unsupported unit '", &unit, b"'"]);
        }
    };
    text_value(rt, &format_rfc3339(&result))
}

/// A duration in nanoseconds as seconds, minutes or hours: the whole units and the rest as a fraction.
fn units(d: i64, unit: i64) -> f64 {
    (d / unit) as f64 + (d % unit) as f64 / unit as f64
}

fn date_diff(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    if args.len() < 2 {
        return error(rt, &[b"date_diff requires at least 2 arguments"]);
    }
    let Some(first) = string(rt, args[0]) else {
        return error(rt, &[b"date_diff: first argument must be a date string"]);
    };
    let Some(second) = string(rt, args[1]) else {
        return error(rt, &[b"date_diff: second argument must be a date string"]);
    };
    let a = match parse_date(first) {
        Ok(t) => t,
        Err(e) => return error(rt, &[b"date_diff: failed to parse first date: ", &e]),
    };
    let b = match parse_date(second) {
        Ok(t) => t,
        Err(e) => return error(rt, &[b"date_diff: failed to parse second date: ", &e]),
    };
    let diff = a.sub(&b);
    if args.len() == 2 {
        return Ok(Value::Int(float_to_int(units(diff, SECOND))));
    }
    let Some(unit) = string(rt, args[2]).map(<[u8]>::to_vec) else {
        return error(rt, &[b"date_diff: third argument must be a unit string"]);
    };
    let n = match lower(&unit).as_slice() {
        b"nanoseconds" | b"nanosecond" => diff,
        b"microseconds" | b"microsecond" => diff / 1000,
        b"milliseconds" | b"millisecond" => diff / 1_000_000,
        b"seconds" | b"second" => float_to_int(units(diff, SECOND)),
        b"minutes" | b"minute" => float_to_int(units(diff, MINUTE)),
        b"hours" | b"hour" => float_to_int(units(diff, HOUR)),
        b"days" | b"day" => float_to_int(units(diff, HOUR) / 24.0),
        _ => return error(rt, &[b"date_diff: unsupported unit '", &unit, b"'"]),
    };
    Ok(Value::Int(n))
}

fn date_between(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let [date, start, end] = args else {
        return error(
            rt,
            &[b"date_between requires 3 arguments (date, startDate, endDate)"],
        );
    };
    let which = ["first", "second", "third"];
    let mut texts: Vec<Vec<u8>> = Vec::with_capacity(3);
    for (i, v) in [date, start, end].into_iter().enumerate() {
        let Some(s) = string(rt, *v).map(<[u8]>::to_vec) else {
            let msg = format!("date_between: {} argument must be a date string", which[i]);
            return rt.error_value(msg.as_bytes());
        };
        texts.push(s);
    }
    let labels: [&[u8]; 3] = [
        b"date_between: failed to parse date: ",
        b"date_between: failed to parse start date: ",
        b"date_between: failed to parse end date: ",
    ];
    let mut times = Vec::with_capacity(3);
    for (text, label) in texts.iter().zip(labels) {
        match parse_date(text) {
            Ok(t) => times.push(t),
            Err(e) => return error(rt, &[label, &e]),
        }
    }
    let inside = !times[0].before(&times[1]) && !times[0].after(&times[2]);
    Ok(Value::Bool(inside))
}

fn date_compare(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let [first, second] = args else {
        return error(rt, &[b"date_compare requires 2 arguments (date1, date2)"]);
    };
    let Some(first) = string(rt, *first) else {
        return error(rt, &[b"date_compare: first argument must be a date string"]);
    };
    let Some(second) = string(rt, *second) else {
        return error(
            rt,
            &[b"date_compare: second argument must be a date string"],
        );
    };
    let a = match parse_date(first) {
        Ok(t) => t,
        Err(e) => return error(rt, &[b"date_compare: failed to parse first date: ", &e]),
    };
    let b = match parse_date(second) {
        Ok(t) => t,
        Err(e) => return error(rt, &[b"date_compare: failed to parse second date: ", &e]),
    };
    Ok(Value::Int(if a.before(&b) {
        -1
    } else {
        i64::from(a.after(&b))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_the_short_names() {
        assert_eq!(
            expand_layout(b"YYYY-MM-DD hh:mm:ss"),
            b"2006-01-02 15:04:05"
        );
        assert_eq!(expand_layout(b"EE, DD NN YYYY"), b"Monday, 02 January 2006");
        assert_eq!(expand_layout(b"YYYYY MMM"), b"2006Y 01M");
        assert_eq!(expand_layout(b"ee nn x"), b"Mon Jan x");
    }

    #[test]
    fn lowers_the_way_the_reference_does() {
        assert_eq!(lower(b"DaYs"), b"days");
        assert_eq!(lower("m\u{130}nutes".as_bytes()), b"minutes");
        assert_eq!(lower(b"\xff"), b"");
    }

    #[test]
    fn reads_the_four_layouts() {
        for s in [
            "2023-12-25T10:30:00Z",
            "2023-12-25T10:30:00",
            "2023-12-25",
            "2023-12-25 10:30:00",
        ] {
            assert!(parse_date(s.as_bytes()).is_ok(), "{s}");
        }
        let e = parse_date(b"nope").unwrap_err();
        assert!(e.starts_with(b"parsing time \"nope\" as \"2006-01-02 15:04:05\""));
    }
}

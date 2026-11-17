//! Layouts: a layout is an example of how the reference time, Monday 2 January 2006 at 15:04:05 in the zone seven
//! hours west of UTC, is written. `2006` is a year, `01` a month, `Mon` a weekday, `15` an hour, `-0700` an
//! offset, and so on; the characters between are copied. This module writes a time as a layout says, and reads the
//! few layouts the builtins read.

use super::civil::{Loc, Time, clock, days_in, days_to_date, days_to_year_yday, is_leap, weekday};

const LONG_MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const LONG_DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

pub(super) const RFC3339: &[u8] = b"2006-01-02T15:04:05Z07:00";

/// One element of a layout.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Std {
    LongMonth,
    Month,
    NumMonth,
    ZeroMonth,
    LongWeekDay,
    WeekDay,
    Day,
    UnderDay,
    ZeroDay,
    UnderYearDay,
    ZeroYearDay,
    Hour,
    Hour12,
    ZeroHour12,
    Minute,
    ZeroMinute,
    Second,
    ZeroSecond,
    LongYear,
    Year,
    Pm,
    PmLower,
    Tz,
    Iso8601Tz,
    Iso8601SecondsTz,
    Iso8601ShortTz,
    Iso8601ColonTz,
    Iso8601ColonSecondsTz,
    NumTz,
    NumSecondsTz,
    NumShortTz,
    NumColonTz,
    NumColonSecondsTz,
    /// `.000`: the digits, and whether the separator is a comma.
    Frac0(usize, bool),
    /// `.999`: the digits with trailing zeros left out.
    Frac9(usize, bool),
}

fn starts_with_lower(s: &[u8]) -> bool {
    s.first().is_some_and(u8::is_ascii_lowercase)
}

fn is_digit_at(s: &[u8], i: usize) -> bool {
    s.get(i).is_some_and(u8::is_ascii_digit)
}

/// The text before the next element of the layout, the element, and the text after it.
fn next_chunk(layout: &[u8]) -> (&[u8], Option<Std>, &[u8]) {
    let has = |i: usize, w: &[u8]| layout[i..].starts_with(w);
    for i in 0..layout.len() {
        let hit = |std: Std, len: usize| Some((&layout[..i], Some(std), &layout[i + len..]));
        let found = match layout[i] {
            b'J' if has(i, b"Jan") => {
                if has(i, b"January") {
                    hit(Std::LongMonth, 7)
                } else if !starts_with_lower(&layout[i + 3..]) {
                    hit(Std::Month, 3)
                } else {
                    None
                }
            }
            b'M' => {
                if has(i, b"Monday") {
                    hit(Std::LongWeekDay, 6)
                } else if has(i, b"Mon") && !starts_with_lower(&layout[i + 3..]) {
                    hit(Std::WeekDay, 3)
                } else if has(i, b"MST") {
                    hit(Std::Tz, 3)
                } else {
                    None
                }
            }
            b'0' => {
                if layout.len() >= i + 2 && (b'1'..=b'6').contains(&layout[i + 1]) {
                    let std = [
                        Std::ZeroMonth,
                        Std::ZeroDay,
                        Std::ZeroHour12,
                        Std::ZeroMinute,
                        Std::ZeroSecond,
                        Std::Year,
                    ][usize::from(layout[i + 1] - b'1')];
                    hit(std, 2)
                } else if has(i, b"002") {
                    hit(Std::ZeroYearDay, 3)
                } else {
                    None
                }
            }
            b'1' => {
                if layout.len() >= i + 2 && layout[i + 1] == b'5' {
                    hit(Std::Hour, 2)
                } else {
                    hit(Std::NumMonth, 1)
                }
            }
            b'2' => {
                if has(i, b"2006") {
                    hit(Std::LongYear, 4)
                } else {
                    hit(Std::Day, 1)
                }
            }
            b'_' => {
                if layout.len() >= i + 2 && layout[i + 1] == b'2' {
                    if has(i + 1, b"2006") {
                        Some((&layout[..=i], Some(Std::LongYear), &layout[i + 5..]))
                    } else {
                        hit(Std::UnderDay, 2)
                    }
                } else if has(i, b"__2") {
                    hit(Std::UnderYearDay, 3)
                } else {
                    None
                }
            }
            b'3' => hit(Std::Hour12, 1),
            b'4' => hit(Std::Minute, 1),
            b'5' => hit(Std::Second, 1),
            b'P' if has(i, b"PM") => hit(Std::Pm, 2),
            b'p' if has(i, b"pm") => hit(Std::PmLower, 2),
            b'-' => {
                if has(i, b"-070000") {
                    hit(Std::NumSecondsTz, 7)
                } else if has(i, b"-07:00:00") {
                    hit(Std::NumColonSecondsTz, 9)
                } else if has(i, b"-0700") {
                    hit(Std::NumTz, 5)
                } else if has(i, b"-07:00") {
                    hit(Std::NumColonTz, 6)
                } else if has(i, b"-07") {
                    hit(Std::NumShortTz, 3)
                } else {
                    None
                }
            }
            b'Z' => {
                if has(i, b"Z070000") {
                    hit(Std::Iso8601SecondsTz, 7)
                } else if has(i, b"Z07:00:00") {
                    hit(Std::Iso8601ColonSecondsTz, 9)
                } else if has(i, b"Z0700") {
                    hit(Std::Iso8601Tz, 5)
                } else if has(i, b"Z07:00") {
                    hit(Std::Iso8601ColonTz, 6)
                } else if has(i, b"Z07") {
                    hit(Std::Iso8601ShortTz, 3)
                } else {
                    None
                }
            }
            c @ (b'.' | b',') => {
                if i + 1 < layout.len() && matches!(layout[i + 1], b'0' | b'9') {
                    let digit = layout[i + 1];
                    let mut j = i + 1;
                    while j < layout.len() && layout[j] == digit {
                        j += 1;
                    }
                    if is_digit_at(layout, j) {
                        None
                    } else {
                        let n = (j - (i + 1)) & 0xFFF;
                        let comma = c == b',';
                        let std = if digit == b'0' {
                            Std::Frac0(n, comma)
                        } else {
                            Std::Frac9(n, comma)
                        };
                        Some((&layout[..i], Some(std), &layout[j..]))
                    }
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(found) = found {
            return found;
        }
    }
    (layout, None, &[])
}

/// Appends `x` in decimal, with a sign when it is negative, padded with zeros to `width` digits.
fn append_int(b: &mut Vec<u8>, x: i64, width: usize) {
    let mut u = x as u64;
    if x < 0 {
        b.push(b'-');
        u = u.wrapping_neg();
    }
    let digits = u.to_string();
    for _ in digits.len()..width {
        b.push(b'0');
    }
    b.extend_from_slice(digits.as_bytes());
}

fn append_nano(b: &mut Vec<u8>, nanosec: i32, digits: usize, comma: bool, trim: bool) {
    if trim && (digits == 0 || nanosec == 0) {
        return;
    }
    let sep = if comma { b',' } else { b'.' };
    b.push(sep);
    let mut nine = Vec::new();
    append_int(&mut nine, i64::from(nanosec), 9);
    nine.truncate(digits.min(9));
    b.extend_from_slice(&nine);
    if trim {
        while b.last() == Some(&b'0') {
            b.pop();
        }
        if b.last() == Some(&sep) {
            b.pop();
        }
    }
}

/// The time written as `layout` says.
pub(super) fn format(t: &Time, layout: &[u8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(layout.len() + 10);
    let zone = t.zone();
    let offset = zone.offset;
    let abs = t.abs_sec(offset);
    let days = abs / 86_400;
    let (year, month, day) = days_to_date(days);
    let (hour, min, sec) = clock(abs);
    let mut layout = layout;
    while !layout.is_empty() {
        let (prefix, std, suffix) = next_chunk(layout);
        b.extend_from_slice(prefix);
        let Some(std) = std else {
            break;
        };
        layout = suffix;
        match std {
            Std::Year => append_int(&mut b, year.wrapping_abs() % 100, 2),
            Std::LongYear => append_int(&mut b, year, 4),
            Std::Month => b.extend_from_slice(&LONG_MONTHS[(month - 1) as usize].as_bytes()[..3]),
            Std::LongMonth => b.extend_from_slice(LONG_MONTHS[(month - 1) as usize].as_bytes()),
            Std::NumMonth => append_int(&mut b, month, 0),
            Std::ZeroMonth => append_int(&mut b, month, 2),
            Std::WeekDay => b.extend_from_slice(&LONG_DAYS[weekday(days)].as_bytes()[..3]),
            Std::LongWeekDay => b.extend_from_slice(LONG_DAYS[weekday(days)].as_bytes()),
            Std::Day => append_int(&mut b, day, 0),
            Std::UnderDay => {
                if day < 10 {
                    b.push(b' ');
                }
                append_int(&mut b, day, 0);
            }
            Std::ZeroDay => append_int(&mut b, day, 2),
            Std::UnderYearDay => {
                let (_, yday) = days_to_year_yday(days);
                if yday < 100 {
                    b.push(b' ');
                    if yday < 10 {
                        b.push(b' ');
                    }
                }
                append_int(&mut b, yday, 0);
            }
            Std::ZeroYearDay => append_int(&mut b, days_to_year_yday(days).1, 3),
            Std::Hour => append_int(&mut b, hour, 2),
            Std::Hour12 | Std::ZeroHour12 => {
                let hr = match hour % 12 {
                    0 => 12,
                    h => h,
                };
                append_int(&mut b, hr, if std == Std::Hour12 { 0 } else { 2 });
            }
            Std::Minute => append_int(&mut b, min, 0),
            Std::ZeroMinute => append_int(&mut b, min, 2),
            Std::Second => append_int(&mut b, sec, 0),
            Std::ZeroSecond => append_int(&mut b, sec, 2),
            Std::Pm => b.extend_from_slice(if hour >= 12 { b"PM" } else { b"AM" }),
            Std::PmLower => b.extend_from_slice(if hour >= 12 { b"pm" } else { b"am" }),
            Std::Iso8601Tz
            | Std::Iso8601ColonTz
            | Std::Iso8601SecondsTz
            | Std::Iso8601ShortTz
            | Std::Iso8601ColonSecondsTz
            | Std::NumTz
            | Std::NumColonTz
            | Std::NumSecondsTz
            | Std::NumShortTz
            | Std::NumColonSecondsTz => {
                let iso = matches!(
                    std,
                    Std::Iso8601Tz
                        | Std::Iso8601ColonTz
                        | Std::Iso8601SecondsTz
                        | Std::Iso8601ShortTz
                        | Std::Iso8601ColonSecondsTz
                );
                if offset == 0 && iso {
                    b.push(b'Z');
                    continue;
                }
                let mut zone_min = i64::from(offset) / 60;
                let mut abs_offset = i64::from(offset);
                if zone_min < 0 {
                    b.push(b'-');
                    zone_min = -zone_min;
                    abs_offset = -abs_offset;
                } else {
                    b.push(b'+');
                }
                append_int(&mut b, zone_min / 60, 2);
                let colon = matches!(
                    std,
                    Std::Iso8601ColonTz
                        | Std::NumColonTz
                        | Std::Iso8601ColonSecondsTz
                        | Std::NumColonSecondsTz
                );
                if colon {
                    b.push(b':');
                }
                if !matches!(std, Std::NumShortTz | Std::Iso8601ShortTz) {
                    append_int(&mut b, zone_min % 60, 2);
                }
                if matches!(
                    std,
                    Std::Iso8601SecondsTz
                        | Std::NumSecondsTz
                        | Std::NumColonSecondsTz
                        | Std::Iso8601ColonSecondsTz
                ) {
                    if matches!(std, Std::NumColonSecondsTz | Std::Iso8601ColonSecondsTz) {
                        b.push(b':');
                    }
                    append_int(&mut b, abs_offset % 60, 2);
                }
            }
            Std::Tz => {
                if !zone.name.is_empty() {
                    b.extend_from_slice(zone.name.as_bytes());
                    continue;
                }
                let mut zone_min = i64::from(offset) / 60;
                if zone_min < 0 {
                    b.push(b'-');
                    zone_min = -zone_min;
                } else {
                    b.push(b'+');
                }
                append_int(&mut b, zone_min / 60, 2);
                append_int(&mut b, zone_min % 60, 2);
            }
            Std::Frac0(n, comma) => append_nano(&mut b, t.nsec, n, comma, false),
            Std::Frac9(n, comma) => append_nano(&mut b, t.nsec, n, comma, true),
        }
    }
    b
}

/// The time as RFC 3339 writes it, without fractions.
pub(super) fn format_rfc3339(t: &Time) -> Vec<u8> {
    format(t, RFC3339)
}

/// The text as the messages of this module show it: in double quotes, a quote or backslash escaped, and every byte of
/// a character outside printable ASCII as `\x` and two hex digits.
pub(super) fn quote(s: &[u8]) -> Vec<u8> {
    let mut out = vec![b'"'];
    let mut i = 0;
    while i < s.len() {
        let (c, w) = decode(&s[i..]);
        if !(0x20..0x80).contains(&c) {
            let width = if c == 0xFFFD {
                if i + 2 < s.len() && s[i..i + 3] == [0xEF, 0xBF, 0xBD] {
                    3
                } else {
                    1
                }
            } else {
                w
            };
            for &byte in &s[i..i + width] {
                out.extend_from_slice(b"\\x");
                out.push(b"0123456789abcdef"[usize::from(byte >> 4)]);
                out.push(b"0123456789abcdef"[usize::from(byte & 0xF)]);
            }
            i += w.max(width);
            continue;
        }
        if c == 0x22 || c == 0x5C {
            out.push(b'\\');
        }
        out.push(s[i]);
        i += 1;
    }
    out.push(b'"');
    out
}

/// The character at the start of `s` and its width; an invalid byte is U+FFFD of width 1.
fn decode(s: &[u8]) -> (u32, usize) {
    match std::str::from_utf8(&s[..s.len().min(4)]) {
        Ok(text) => text
            .chars()
            .next()
            .map_or((0xFFFD, 1), |c| (c as u32, c.len_utf8())),
        Err(e) if e.valid_up_to() > 0 => {
            let text = std::str::from_utf8(&s[..e.valid_up_to()]).unwrap_or("");
            text.chars()
                .next()
                .map_or((0xFFFD, 1), |c| (c as u32, c.len_utf8()))
        }
        Err(_) => (0xFFFD, 1),
    }
}

/// A value the layout does not describe: what was being read and where, in the words of the reference.
struct ParseError {
    layout: Vec<u8>,
    value: Vec<u8>,
    layout_elem: Vec<u8>,
    value_elem: Vec<u8>,
    message: Vec<u8>,
}

impl ParseError {
    fn text(&self) -> Vec<u8> {
        let mut out = b"parsing time ".to_vec();
        out.extend(quote(&self.value));
        if self.message.is_empty() {
            out.extend_from_slice(b" as ");
            out.extend(quote(&self.layout));
            out.extend_from_slice(b": cannot parse ");
            out.extend(quote(&self.value_elem));
            out.extend_from_slice(b" as ");
            out.extend(quote(&self.layout_elem));
        } else {
            out.extend_from_slice(&self.message);
        }
        out
    }
}

/// Two digits, or one when `fixed` is false.
fn getnum(s: &[u8], fixed: bool) -> Option<(i64, &[u8])> {
    if !is_digit_at(s, 0) {
        return None;
    }
    if !is_digit_at(s, 1) {
        if fixed {
            return None;
        }
        return Some((i64::from(s[0] - b'0'), &s[1..]));
    }
    Some((
        i64::from(s[0] - b'0') * 10 + i64::from(s[1] - b'0'),
        &s[2..],
    ))
}

/// Takes the literal text `prefix` off the front of `value`; a space in the prefix stands for any run of spaces.
fn skip<'a>(mut value: &'a [u8], mut prefix: &[u8]) -> Result<&'a [u8], &'a [u8]> {
    while let Some(&p) = prefix.first() {
        if p == b' ' {
            if value.first().is_some_and(|&v| v != b' ') {
                return Err(value);
            }
            prefix = &prefix[prefix.iter().take_while(|&&c| c == b' ').count()..];
            value = &value[value.iter().take_while(|&&c| c == b' ').count()..];
            continue;
        }
        if value.first() != Some(&p) {
            return Err(value);
        }
        prefix = &prefix[1..];
        value = &value[1..];
    }
    Ok(value)
}

/// `[0-9]*` at the start of `s` and what follows it; `None` when the number does not fit.
fn leading_int(s: &[u8]) -> Option<(u64, &[u8])> {
    let mut x: u64 = 0;
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        if x > (1u64 << 63) / 10 {
            return None;
        }
        x = x * 10 + u64::from(s[i] - b'0');
        if x > 1u64 << 63 {
            return None;
        }
        i += 1;
    }
    Some((x, &s[i..]))
}

/// A whole number with an optional sign, and nothing else.
fn atoi(s: &[u8]) -> Option<i64> {
    let (neg, digits) = match s.first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let (q, rest) = leading_int(digits)?;
    if !rest.is_empty() {
        return None;
    }
    let x = q as i64;
    Some(if neg { x.wrapping_neg() } else { x })
}

/// One to three digits, or exactly three when `fixed` is set.
fn getnum3(s: &[u8], fixed: bool) -> Option<(i64, &[u8])> {
    let mut n = 0i64;
    let mut i = 0;
    while i < 3 && is_digit_at(s, i) {
        n = n * 10 + i64::from(s[i] - b'0');
        i += 1;
    }
    if i == 0 || (fixed && i != 3) {
        return None;
    }
    Some((n, &s[i..]))
}

/// The first name of `names`, or of its first three letters when `short`, that `value` starts with, ignoring ASCII
/// case, and what follows it.
fn lookup<'a>(names: &[&str], short: bool, value: &'a [u8]) -> Option<(usize, &'a [u8])> {
    for (i, name) in names.iter().enumerate() {
        let name = if short { &name[..3] } else { name };
        let n = name.len();
        if value.len() >= n && value[..n].eq_ignore_ascii_case(name.as_bytes()) {
            return Some((i, &value[n..]));
        }
    }
    None
}

/// The nanoseconds of a fraction of a second that starts with its separator and is `nbytes` long: the number, and
/// the range message when it is negative. `None` when the text is not a number.
fn parse_nanoseconds(value: &[u8], mut nbytes: usize) -> Option<(i64, &'static str)> {
    if !matches!(value[0], b',' | b'.') {
        return None;
    }
    if nbytes > 10 {
        nbytes = 10;
    }
    let mut ns = atoi(&value[1..nbytes])?;
    if ns < 0 {
        return Some((ns, "fractional second"));
    }
    for _ in 0..10 - nbytes {
        ns = ns.wrapping_mul(10);
    }
    Some((ns, ""))
}

/// The length of a signed offset such as `+03` or `-04` at the start of `value`, or 0.
fn parse_signed_offset(value: &[u8]) -> usize {
    if !matches!(value.first(), Some(b'-' | b'+')) {
        return 0;
    }
    match leading_int(&value[1..]) {
        Some((x, rest)) if rest.len() != value.len() - 1 && x <= 23 => value.len() - rest.len(),
        _ => 0,
    }
}

/// The length of a zone name at the start of `value`, if it looks like one: names are made up by people, so this
/// only looks for a run of capital letters, and the few names that are not.
fn parse_time_zone(value: &[u8]) -> Option<usize> {
    if value.len() < 3 {
        return None;
    }
    if value.starts_with(b"ChST") || value.starts_with(b"MeST") {
        return Some(4);
    }
    if value.starts_with(b"GMT") {
        let rest = &value[3..];
        return Some(
            3 + if rest.is_empty() {
                0
            } else {
                parse_signed_offset(rest)
            },
        );
    }
    if matches!(value[0], b'+' | b'-') {
        return Some(parse_signed_offset(value)).filter(|&n| n > 0);
    }
    let upper = value
        .iter()
        .take(6)
        .take_while(|c| c.is_ascii_uppercase())
        .count();
    match upper {
        5 if value[4] == b'T' => Some(5),
        4 if value[3] == b'T' || value.starts_with(b"WITA") => Some(4),
        3 => Some(3),
        _ => None,
    }
}

fn days_before(month: i64) -> i64 {
    const BEFORE: [i64; 13] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334, 365];
    BEFORE[(month - 1) as usize]
}

/// Reads `value` as `layout` says, with every element of a layout. On failure the text of the error.
pub(super) fn parse(layout: &[u8], value: &[u8]) -> Result<Time, Vec<u8>> {
    let (alayout, avalue) = (layout, value);
    let fail = |layout_elem: &[u8], value_elem: &[u8], message: &[u8]| {
        ParseError {
            layout: alayout.to_vec(),
            value: avalue.to_vec(),
            layout_elem: layout_elem.to_vec(),
            value_elem: value_elem.to_vec(),
            message: message.to_vec(),
        }
        .text()
    };
    let (mut layout, mut value) = (layout, value);
    let mut range_err = "";
    let (mut am_set, mut pm_set) = (false, false);
    let (mut year, mut month, mut day, mut yday) = (0i64, -1i64, -1i64, -1i64);
    let (mut hour, mut min, mut sec, mut nsec) = (0i64, 0i64, 0i64, 0i64);
    let mut z_utc = false;
    let mut zone_offset: i64 = -1;
    let mut zone_name: Vec<u8> = Vec::new();
    loop {
        let (prefix, std, suffix) = next_chunk(layout);
        let std_text = &layout[prefix.len()..layout.len() - suffix.len()];
        value = match skip(value, prefix) {
            Ok(v) => v,
            Err(v) => return Err(fail(prefix, v, b"")),
        };
        let Some(std) = std else {
            if !value.is_empty() {
                let mut msg = b": extra text: ".to_vec();
                msg.extend(quote(value));
                return Err(fail(b"", value, &msg));
            }
            break;
        };
        layout = suffix;
        let hold = value;
        let mut bad = false;
        match std {
            Std::Year => {
                if value.len() < 2 {
                    bad = true;
                } else {
                    let (p, rest) = value.split_at(2);
                    value = rest;
                    match atoi(p) {
                        Some(y) => year = y + if y >= 69 { 1900 } else { 2000 },
                        None => bad = true,
                    }
                }
            }
            Std::LongYear => {
                if value.len() < 4 || !is_digit_at(value, 0) {
                    bad = true;
                } else {
                    let (p, rest) = value.split_at(4);
                    value = rest;
                    match atoi(p) {
                        Some(y) => year = y,
                        None => bad = true,
                    }
                }
            }
            Std::Month | Std::LongMonth => match lookup(&LONG_MONTHS, std == Std::Month, value) {
                Some((m, rest)) => {
                    month = m as i64 + 1;
                    value = rest;
                }
                None => bad = true,
            },
            Std::NumMonth | Std::ZeroMonth => match getnum(value, std == Std::ZeroMonth) {
                Some((m, rest)) => {
                    month = m;
                    value = rest;
                    if !(1..=12).contains(&m) {
                        range_err = "month";
                    }
                }
                None => bad = true,
            },
            Std::WeekDay | Std::LongWeekDay => {
                match lookup(&LONG_DAYS, std == Std::WeekDay, value) {
                    Some((_, rest)) => value = rest,
                    None => bad = true,
                }
            }
            Std::Day | Std::UnderDay | Std::ZeroDay => {
                if std == Std::UnderDay && value.first() == Some(&b' ') {
                    value = &value[1..];
                }
                match getnum(value, std == Std::ZeroDay) {
                    Some((d, rest)) => {
                        day = d;
                        value = rest;
                    }
                    None => bad = true,
                }
            }
            Std::UnderYearDay | Std::ZeroYearDay => {
                for _ in 0..2 {
                    if std == Std::UnderYearDay && value.first() == Some(&b' ') {
                        value = &value[1..];
                    }
                }
                match getnum3(value, std == Std::ZeroYearDay) {
                    Some((d, rest)) => {
                        yday = d;
                        value = rest;
                    }
                    None => bad = true,
                }
            }
            Std::Hour => match getnum(value, false) {
                Some((h, rest)) => {
                    hour = h;
                    value = rest;
                    if !(0..24).contains(&h) {
                        range_err = "hour";
                    }
                }
                None => bad = true,
            },
            Std::Hour12 | Std::ZeroHour12 => match getnum(value, std == Std::ZeroHour12) {
                Some((h, rest)) => {
                    hour = h;
                    value = rest;
                    if !(0..=12).contains(&h) {
                        range_err = "hour";
                    }
                }
                None => bad = true,
            },
            Std::Minute | Std::ZeroMinute => match getnum(value, std == Std::ZeroMinute) {
                Some((m, rest)) => {
                    min = m;
                    value = rest;
                    if !(0..60).contains(&m) {
                        range_err = "minute";
                    }
                }
                None => bad = true,
            },
            Std::Second | Std::ZeroSecond => match getnum(value, std == Std::ZeroSecond) {
                Some((s, rest)) => {
                    sec = s;
                    value = rest;
                    if !(0..60).contains(&s) {
                        range_err = "second";
                    } else if value.len() >= 2
                        && matches!(value[0], b'.' | b',')
                        && is_digit_at(value, 1)
                    {
                        let (_, next, _) = next_chunk(layout);
                        if !matches!(next, Some(Std::Frac0(..) | Std::Frac9(..))) {
                            let mut n = 2;
                            while n < value.len() && is_digit_at(value, n) {
                                n += 1;
                            }
                            match parse_nanoseconds(value, n) {
                                Some((ns, range)) => {
                                    nsec = ns;
                                    range_err = range;
                                }
                                None => bad = true,
                            }
                            value = &value[n..];
                        }
                    }
                }
                None => bad = true,
            },
            Std::Pm | Std::PmLower => {
                if value.len() < 2 {
                    bad = true;
                } else {
                    let (p, rest) = value.split_at(2);
                    value = rest;
                    let (pm, am): (&[u8], &[u8]) = if std == Std::Pm {
                        (b"PM", b"AM")
                    } else {
                        (b"pm", b"am")
                    };
                    if p == pm {
                        pm_set = true;
                    } else if p == am {
                        am_set = true;
                    } else {
                        bad = true;
                    }
                }
            }
            Std::Iso8601Tz
            | Std::Iso8601ShortTz
            | Std::Iso8601ColonTz
            | Std::Iso8601SecondsTz
            | Std::Iso8601ColonSecondsTz
            | Std::NumTz
            | Std::NumShortTz
            | Std::NumColonTz
            | Std::NumSecondsTz
            | Std::NumColonSecondsTz => {
                let iso = matches!(
                    std,
                    Std::Iso8601Tz
                        | Std::Iso8601ShortTz
                        | Std::Iso8601ColonTz
                        | Std::Iso8601SecondsTz
                        | Std::Iso8601ColonSecondsTz
                );
                if iso && value.first() == Some(&b'Z') {
                    value = &value[1..];
                    z_utc = true;
                } else {
                    // The sign, hours, minutes and seconds, and how much of the value they take.
                    let (need, colon) = match std {
                        Std::Iso8601ColonTz | Std::NumColonTz => (6, true),
                        Std::NumShortTz | Std::Iso8601ShortTz => (3, false),
                        Std::Iso8601ColonSecondsTz | Std::NumColonSecondsTz => (9, true),
                        Std::Iso8601SecondsTz | Std::NumSecondsTz => (7, false),
                        _ => (5, false),
                    };
                    if value.len() < need
                        || (colon && (value[3] != b':' || (need == 9 && value[6] != b':')))
                    {
                        bad = true;
                    } else {
                        let v = &value[..need];
                        let (hh, mm, ss): (&[u8], &[u8], &[u8]) = match need {
                            6 => (&v[1..3], &v[4..6], b"00"),
                            3 => (&v[1..3], b"00", b"00"),
                            9 => (&v[1..3], &v[4..6], &v[7..9]),
                            7 => (&v[1..3], &v[3..5], &v[5..7]),
                            _ => (&v[1..3], &v[3..5], b"00"),
                        };
                        let sign = v[0];
                        value = &value[need..];
                        let num = |t: &[u8]| getnum(t, true).map(|(n, _)| n);
                        let (hr, mi, se) = (num(hh), num(mm), num(ss));
                        bad = hr.is_none() || mi.is_none() || se.is_none();
                        let (hr, mi, se) = (hr.unwrap_or(0), mi.unwrap_or(0), se.unwrap_or(0));
                        if hr > 24 {
                            range_err = "time zone offset hour";
                        }
                        if mi > 60 {
                            range_err = "time zone offset minute";
                        }
                        if se > 60 {
                            range_err = "time zone offset second";
                        }
                        zone_offset = (hr * 60 + mi) * 60 + se;
                        match sign {
                            b'+' => {}
                            b'-' => zone_offset = -zone_offset,
                            _ => bad = true,
                        }
                    }
                }
            }
            Std::Tz => {
                if value.starts_with(b"UTC") {
                    z_utc = true;
                    value = &value[3..];
                } else if let Some(n) = parse_time_zone(value) {
                    zone_name = value[..n].to_vec();
                    value = &value[n..];
                } else {
                    bad = true;
                }
            }
            Std::Frac0(digits, _) => {
                let n = 1 + digits;
                if value.len() < n {
                    bad = true;
                } else {
                    match parse_nanoseconds(value, n) {
                        Some((ns, range)) => {
                            nsec = ns;
                            range_err = range;
                        }
                        None => bad = true,
                    }
                    value = &value[n..];
                }
            }
            Std::Frac9(..) => {
                if value.len() >= 2 && matches!(value[0], b'.' | b',') && is_digit_at(value, 1) {
                    let mut i = 0;
                    while i + 1 < value.len() && is_digit_at(value, i + 1) {
                        i += 1;
                    }
                    match parse_nanoseconds(value, 1 + i) {
                        Some((ns, range)) => {
                            nsec = ns;
                            range_err = range;
                        }
                        None => bad = true,
                    }
                    value = &value[1 + i..];
                }
            }
        }
        if !range_err.is_empty() {
            let msg = format!(": {range_err} out of range");
            return Err(fail(std_text, value, msg.as_bytes()));
        }
        if bad {
            return Err(fail(std_text, hold, b""));
        }
    }
    if pm_set && hour < 12 {
        hour += 12;
    } else if am_set && hour == 12 {
        hour = 0;
    }

    if yday >= 0 {
        let (mut m, mut d) = (0i64, 0i64);
        if is_leap(year) {
            if yday == 31 + 29 {
                m = 2;
                d = 29;
            } else if yday > 31 + 29 {
                yday -= 1;
            }
        }
        if !(1..=365).contains(&yday) {
            return Err(fail(b"", value, b": day-of-year out of range"));
        }
        if m == 0 {
            m = (yday - 1) / 31 + 1;
            if days_before(m + 1) < yday {
                m += 1;
            }
            d = yday - days_before(m);
        }
        if month >= 0 && month != m {
            return Err(fail(b"", value, b": day-of-year does not match month"));
        }
        month = m;
        if day >= 0 && day != d {
            return Err(fail(b"", value, b": day-of-year does not match day"));
        }
        day = d;
    } else {
        if month < 0 {
            month = 1;
        }
        if day < 0 {
            day = 1;
        }
    }
    if day < 1 || day > days_in(month, year) {
        return Err(fail(b"", value, b": day out of range"));
    }

    let at = |loc: Loc| super::civil::date(year, month, day, hour, min, sec, nsec, loc);
    if z_utc {
        return Ok(at(Loc::Utc));
    }
    if zone_offset != -1 {
        let mut t = at(Loc::Utc);
        t = t.add(-zone_offset.wrapping_mul(1_000_000_000));
        let info = super::civil::host_lookup(t.unix_sec());
        let named = String::from_utf8_lossy(&zone_name).into_owned();
        if i64::from(info.offset) == zone_offset && (zone_name.is_empty() || info.name == named) {
            t.loc = Loc::Local;
        } else if zone_name.is_empty() {
            t.loc = Loc::Fixed(zone_offset as i32);
        } else {
            t.loc = Loc::Named(zone_offset as i32, named.into());
        }
        return Ok(t);
    }
    if !zone_name.is_empty() {
        let mut t = at(Loc::Utc);
        let named = String::from_utf8_lossy(&zone_name).into_owned();
        if let Some(offset) = super::civil::host_lookup_name(&named, t.unix_sec()) {
            t = t.add(-i64::from(offset).wrapping_mul(1_000_000_000));
            t.loc = Loc::Local;
            return Ok(t);
        }
        let hours = if zone_name.len() > 3 && zone_name.starts_with(b"GMT") {
            atoi(&zone_name[3..]).unwrap_or(0) * 3600
        } else {
            0
        };
        t.loc = Loc::Named(hours as i32, named.into());
        return Ok(t);
    }
    Ok(at(Loc::Utc))
}

#[cfg(test)]
mod tests {
    use super::super::civil::date;
    use super::*;

    fn utc(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> Time {
        date(y, mo, d, h, mi, s, 0, Loc::Utc)
    }

    fn fmt(t: &Time, layout: &str) -> String {
        String::from_utf8(format(t, layout.as_bytes())).expect("utf-8")
    }

    #[test]
    fn writes_the_reference_time() {
        let t = utc(2006, 1, 2, 15, 4, 5);
        assert_eq!(
            fmt(&t, "Mon Jan _2 15:04:05 MST 2006"),
            "Mon Jan  2 15:04:05 UTC 2006"
        );
        assert_eq!(
            fmt(&t, "Monday, 02-Jan-06 3:04PM"),
            "Monday, 02-Jan-06 3:04PM"
        );
        assert_eq!(fmt(&t, "2006-01-02T15:04:05Z07:00"), "2006-01-02T15:04:05Z");
        assert_eq!(fmt(&t, "-0700 -07:00 Z0700 Z07"), "+0000 +00:00 Z Z");
        assert_eq!(
            fmt(&t, "January 2, 2006 __2 002"),
            "January 2, 2006   2 002"
        );
    }

    #[test]
    fn writes_fractions_and_other_elements() {
        let t = date(2023, 12, 25, 0, 30, 5, 120_000_000, Loc::Utc);
        assert_eq!(
            fmt(&t, "05.000 05.999 05,000000 .9"),
            "05.120 05.12 05,120000 .1"
        );
        assert_eq!(fmt(&t, "pm 3 12h"), "am 12 1225h");
        assert_eq!(fmt(&t, "Jane Mon Monk Jan"), "Jane Mon Monk Dec");
    }

    #[test]
    fn reads_the_layouts_of_the_builtins() {
        let t = parse(RFC3339, b"2023-12-25T10:30:00Z").expect("parses");
        assert_eq!(fmt(&t, "2006-01-02 15:04:05"), "2023-12-25 10:30:00");
        let t = parse(RFC3339, b"2023-12-25T10:30:00.5+05:30").expect("parses");
        assert_eq!(t.nsec, 500_000_000);
        assert_eq!(fmt(&t, "-07:00"), "+05:30");
        assert!(parse(b"2006-01-02", b"2024-02-29").is_ok());
        assert!(parse(b"2006-01-02", b"2023-02-29").is_err());
    }

    #[test]
    fn words_what_it_cannot_read_as_the_reference_does() {
        let err = |l: &str, v: &str| {
            String::from_utf8_lossy(&parse(l.as_bytes(), v.as_bytes()).unwrap_err()).into_owned()
        };
        assert_eq!(
            err("2006-01-02 15:04:05", "x"),
            "parsing time \"x\" as \"2006-01-02 15:04:05\": cannot parse \"x\" as \"2006\""
        );
        assert_eq!(
            err("2006-01-02", "2023-13-01"),
            "parsing time \"2023-13-01\": month out of range"
        );
        assert_eq!(
            err("2006-01-02", "2023-02-30"),
            "parsing time \"2023-02-30\": day out of range"
        );
        assert_eq!(
            err("2006-01-02", "2023-02-28x"),
            "parsing time \"2023-02-28x\": extra text: \"x\""
        );
        assert_eq!(
            err("2006-01-02T15:04:05Z07:00", "2023-02-28"),
            "parsing time \"2023-02-28\" as \"2006-01-02T15:04:05Z07:00\": cannot parse \"\" as \"T\""
        );
    }
}

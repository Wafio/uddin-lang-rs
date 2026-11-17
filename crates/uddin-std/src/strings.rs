//! Strings: contains find join split lower upper replace reverse_str substr trim starts_with ends_with.
//!
//! Strings are byte strings, and invalid UTF-8 follows one rule. A builtin that hands back characters (`split` with
//! an empty separator, `reverse_str`, `upper`, `lower`, and `for` and the spread of a string in the machine) reads
//! each byte that is not part of a valid sequence as one character, U+FFFD, and hands back U+FFFD. A builtin that
//! finds, cuts or compares bytes (`contains`, `find`, `substr`, `replace`, `trim`, `starts_with`, `ends_with`,
//! `join`, and `split` by a separator or at white space) keeps the bytes as they are, and a result that is a part of
//! an argument shares the argument's bytes.

use uddin_vm::{
    BuiltinDef, Count, Exc, Fresh, LIST_COST, ListId, Rt, SPLIT_PART_COST, Str, Value,
    checked_product, sum_estimate, to_text,
};

pub(crate) const TABLE: &[BuiltinDef] = &[
    BuiltinDef::new("contains", contains, 2).fast_path().deep(),
    BuiltinDef::new("find", find, 2).deep(),
    BuiltinDef::new("join", join, 2)
        .fast_path()
        .deep()
        .guard(join_guard),
    BuiltinDef::new("split", split, 2)
        .fast_path()
        .reserve(split_reserve)
        .fresh(Fresh::Elements),
    BuiltinDef::new("lower", lower, 1).fast_path(),
    BuiltinDef::new("upper", upper, 1).fast_path(),
    BuiltinDef::new("replace", replace, 3).guard(replace_guard),
    BuiltinDef::new("reverse_str", reverse_str, 1).fast_path(),
    BuiltinDef::new("substr", substr, 3),
    BuiltinDef::new("trim", trim, 1).fast_path(),
    BuiltinDef::new("starts_with", starts_with, 2),
    BuiltinDef::new("ends_with", ends_with, 2),
];

const RUNE_ERROR: u32 = 0xFFFD;
const REPLACEMENT: &[u8] = "\u{FFFD}".as_bytes();

/// The first rune of `s` and its width. A bad or cut sequence is `(RUNE_ERROR, 1)`, an empty slice `(RUNE_ERROR, 0)`.
fn decode_rune(s: &[u8]) -> (u32, usize) {
    let Some(&b0) = s.first() else {
        return (RUNE_ERROR, 0);
    };
    if b0 < 0x80 {
        return (u32::from(b0), 1);
    }
    let (lo, hi, width) = match b0 {
        0xC2..=0xDF => (0x80, 0xBF, 2),
        0xE0 => (0xA0, 0xBF, 3),
        0xE1..=0xEC | 0xEE..=0xEF => (0x80, 0xBF, 3),
        0xED => (0x80, 0x9F, 3),
        0xF0 => (0x90, 0xBF, 4),
        0xF1..=0xF3 => (0x80, 0xBF, 4),
        0xF4 => (0x80, 0x8F, 4),
        _ => return (RUNE_ERROR, 1),
    };
    if s.len() < width || s[1] < lo || s[1] > hi {
        return (RUNE_ERROR, 1);
    }
    if s[2..width].iter().any(|b| !(0x80..=0xBF).contains(b)) {
        return (RUNE_ERROR, 1);
    }
    let mut r = u32::from(b0) & (0x7F >> width);
    for &b in &s[1..width] {
        r = r << 6 | u32::from(b & 0x3F);
    }
    (r, width)
}

/// The last rune of `s` and its width, found by stepping back over at most three continuation bytes.
fn decode_last_rune(s: &[u8]) -> (u32, usize) {
    let end = s.len();
    if end == 0 {
        return (RUNE_ERROR, 0);
    }
    if s[end - 1] < 0x80 {
        return (u32::from(s[end - 1]), 1);
    }
    let lim = end.saturating_sub(4);
    let mut start = end - 1;
    while start > lim {
        start -= 1;
        if s[start] & 0xC0 != 0x80 {
            break;
        }
    }
    let (r, width) = decode_rune(&s[start..end]);
    if start + width != end {
        return (RUNE_ERROR, 1);
    }
    (r, width)
}

fn push_rune(out: &mut Vec<u8>, r: u32) {
    let c = char::from_u32(r).unwrap_or('\u{FFFD}');
    let mut buf = [0u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
}

fn rune_count(s: &[u8]) -> usize {
    let (mut i, mut n) = (0, 0);
    while i < s.len() {
        i += if s[i] < 0x80 {
            1
        } else {
            decode_rune(&s[i..]).1
        };
        n += 1;
    }
    n
}

const UPPER: usize = 0;
const LOWER: usize = 1;

/// The simple case mapping of `r`: `UPPER` or `LOWER`.
fn to_case(case: usize, r: u32) -> u32 {
    if r < 0x80 {
        return match (case, r) {
            (UPPER, 0x61..=0x7A) => r - 32,
            (LOWER, 0x41..=0x5A) => r + 32,
            _ => r,
        };
    }
    let (mut lo, mut hi) = (0, CASE_RANGES.len());
    while lo < hi {
        let m = (lo + hi) / 2;
        let (first, last, delta) = CASE_RANGES[m];
        if first <= r && r <= last {
            let d = delta[case];
            if d > 0x10FFFF {
                // A run that alternates upper and lower case, starting with upper case.
                return first + ((r - first) & !1 | case as u32 & 1);
            }
            return (i64::from(r) + i64::from(d)) as u32;
        }
        if r < first {
            hi = m;
        } else {
            lo = m + 1;
        }
    }
    r
}

fn is_space(r: u32) -> bool {
    if r <= 0xFF {
        matches!(r, 0x09..=0x0D | 0x20 | 0x85 | 0xA0)
    } else {
        matches!(
            r,
            0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000
        )
    }
}

fn is_print(r: u32) -> bool {
    let i = IS_PRINT.partition_point(|&(_, hi)| hi < r);
    i < IS_PRINT.len() && IS_PRINT[i].0 <= r
}

/// An upper bound of `s` once quoted: a JSON encoder escapes `<`, `>`, `&`, U+2028 and U+2029, and a quoted
/// literal every rune that does not print.
fn quoted_len(s: &[u8], json: bool) -> usize {
    let mut n = 2;
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c < 0x80 {
            n += match c {
                b'"' | b'\\' => 2,
                _ if c < 0x20 || c == 0x7F => 6,
                b'<' | b'>' | b'&' if json => 6,
                _ => 1,
            };
            i += 1;
            continue;
        }
        let (r, w) = decode_rune(&s[i..]);
        n += if r == RUNE_ERROR && w == 1 || json && (r == 0x2028 || r == 0x2029) {
            6
        } else if !json && !is_print(r) {
            10
        } else {
            w
        };
        i += w;
    }
    n
}

/// A needle of up to this many bytes is found by a scan for its first byte, a longer one with a table.
const SHORT_NEEDLE: usize = 8;

/// Finds the first occurrence of a needle from a given place, as often as asked, in time linear in the haystack.
/// It is made from the needle and then used with it, so a call that searches many times makes the table once.
struct Finder {
    /// For a needle longer than `SHORT_NEEDLE`: the length of the longest proper prefix of each prefix of the
    /// needle that is also its suffix.
    fail: Vec<u32>,
}

impl Finder {
    fn new(needle: &[u8]) -> Finder {
        let mut fail = Vec::new();
        if needle.len() > SHORT_NEEDLE {
            fail.resize(needle.len(), 0u32);
            let mut k = 0usize;
            for i in 1..needle.len() {
                while k > 0 && needle[i] != needle[k] {
                    k = fail[k - 1] as usize;
                }
                if needle[i] == needle[k] {
                    k += 1;
                }
                fail[i] = k as u32;
            }
        }
        Finder { fail }
    }

    /// The place of the first occurrence of `n` in `hay` at or after `from`. An empty needle is found at `from`.
    fn find(&self, n: &[u8], hay: &[u8], from: usize) -> Option<usize> {
        if n.is_empty() {
            return (from <= hay.len()).then_some(from);
        }
        if from >= hay.len() || hay.len() - from < n.len() {
            return None;
        }
        if n.len() <= SHORT_NEEDLE {
            let last = hay.len() - n.len();
            let mut i = from;
            while i <= last {
                i += hay[i..=last].iter().position(|&b| b == n[0])?;
                if &hay[i..i + n.len()] == n {
                    return Some(i);
                }
                i += 1;
            }
            return None;
        }
        let mut k = 0usize;
        for (i, &b) in hay.iter().enumerate().skip(from) {
            while k > 0 && b != n[k] {
                k = self.fail[k - 1] as usize;
            }
            if b == n[k] {
                k += 1;
                if k == n.len() {
                    return Some(i + 1 - k);
                }
            }
        }
        None
    }

    /// How many times `n` occurs in `hay` without overlapping. An empty needle is a place before each rune and
    /// one after the last.
    fn count(&self, n: &[u8], hay: &[u8]) -> usize {
        if n.is_empty() {
            return rune_count(hay) + 1;
        }
        let (mut count, mut at) = (0, 0);
        while let Some(i) = self.find(n, hay, at) {
            count += 1;
            at = i + n.len();
        }
        count
    }
}

fn str_of(v: Option<&Value>) -> Option<Str> {
    match v {
        Some(&Value::Str(s)) => Some(s),
        _ => None,
    }
}

/// A part of `s`: the same bytes, no copy.
fn part(s: Str, from: usize, to: usize) -> Str {
    if from == to {
        return Str::EMPTY;
    }
    Str {
        off: s.off + from as u32,
        len: (to - from) as u32,
    }
}

fn too_big(rt: &Rt<'_>) -> Exc {
    Exc::limit(uddin_vm::LimitReason::Memory, rt.pos)
}

/// Makes room for `n` more bytes without aborting when there is none.
fn room(rt: &Rt<'_>, buf: &mut Vec<u8>, n: usize) -> Result<(), Exc> {
    buf.try_reserve(n).map_err(|_| too_big(rt))
}

/// Builds a string in the scratch buffer. `fill` writes it, may keep reading the arguments, and says whether the
/// buffer is the result.
fn build_if(
    rt: &mut Rt<'_>,
    fill: impl FnOnce(&mut Rt<'_>, &mut Vec<u8>) -> Result<bool, Exc>,
) -> Result<Option<Str>, Exc> {
    let mut buf = std::mem::take(&mut *rt.scratch);
    buf.clear();
    let made = match fill(rt, &mut buf) {
        Ok(true) => rt.heap.new_str(&buf).map(Some).map_err(rt.oom()),
        Ok(false) => Ok(None),
        Err(e) => Err(e),
    };
    *rt.scratch = buf;
    made
}

fn build(
    rt: &mut Rt<'_>,
    fill: impl FnOnce(&mut Rt<'_>, &mut Vec<u8>) -> Result<(), Exc>,
) -> Result<Str, Exc> {
    let made = build_if(rt, |rt, buf| fill(rt, buf).map(|()| true))?;
    Ok(made.unwrap_or(Str::EMPTY))
}

fn list_of(rt: &mut Rt<'_>, items: &[Value]) -> Result<Value, Exc> {
    let id = rt
        .heap
        .new_list(items.len(), items.len())
        .map_err(rt.oom())?;
    for &v in items {
        rt.heap.list_push(id, v).map_err(rt.oom())?;
    }
    Ok(Value::List(id))
}

fn contains(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let [hay, needle] = args else {
        return Err(rt.arity_error("contains", 2, args.len()));
    };
    match *hay {
        Value::Str(h) => {
            let Value::Str(n) = *needle else {
                return Err(
                    rt.type_error("contains() on str requires second argument to be a string")
                );
            };
            rt.work(Count::Free, u64::from(h.len))?;
            let (needle, hay) = (rt.heap.str_bytes(n), rt.heap.str_bytes(h));
            Ok(Value::Bool(
                Finder::new(needle).find(needle, hay, 0).is_some(),
            ))
        }
        Value::List(l) => {
            for i in 0..rt.heap.list(l).items.len() {
                let v = rt.heap.list(l).items[i];
                if rt.equal(*needle, v)? {
                    return Ok(Value::Bool(true));
                }
            }
            Ok(Value::Bool(false))
        }
        _ => Err(rt.type_error("contains() requires first argument to be a string or array")),
    }
}

fn find(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let [hay, needle] = args else {
        return Err(rt.arity_error("find", 2, args.len()));
    };
    match *hay {
        Value::Str(h) => {
            let Value::Str(n) = *needle else {
                return Err(
                    rt.type_error("find() on string requires second argument to be a string")
                );
            };
            rt.work(Count::Free, u64::from(h.len))?;
            let (needle, hay) = (rt.heap.str_bytes(n), rt.heap.str_bytes(h));
            let found = Finder::new(needle).find(needle, hay, 0);
            Ok(Value::Int(found.map_or(-1, |i| i as i64)))
        }
        Value::List(l) => {
            for i in 0..rt.heap.list(l).items.len() {
                let v = rt.heap.list(l).items[i];
                if rt.equal(*needle, v)? {
                    return Ok(Value::Int(i as i64));
                }
            }
            Ok(Value::Int(-1))
        }
        _ => Err(rt.type_error("find() requires first argument to be a string or array")),
    }
}

/// A bound of the text `join` writes for `list`, counted until it passes `budget`: a string element as it is,
/// anything nested in an element as `str` shows it, quoted. The separators come on top.
fn join_size(rt: &Rt<'_>, list: ListId, budget: i64) -> i64 {
    let mut n = 0i64;
    let mut stack: Vec<(Value, bool)> = Vec::new();
    for i in 0..rt.heap.list(list).items.len() {
        match rt.heap.list(list).items[i] {
            Value::Str(s) => n += i64::from(s.len),
            v @ (Value::List(_) | Value::Map(_)) => stack.push((v, true)),
            _ => n += 24,
        }
        if n > budget {
            return n;
        }
    }
    while n <= budget {
        let Some((cur, quoted)) = stack.pop() else {
            break;
        };
        match cur {
            Value::Str(s) => {
                let b = rt.heap.str_bytes(s);
                n += if quoted {
                    quoted_len(b, false) as i64
                } else {
                    b.len() as i64
                };
            }
            Value::List(l) => {
                n += 2;
                for i in 0..rt.heap.list(l).items.len() {
                    n += 2;
                    if n > budget {
                        break;
                    }
                    stack.push((rt.heap.list(l).items[i], true));
                }
            }
            Value::Map(m) => {
                n += 2;
                for i in 0..rt.heap.map_len(m) {
                    let (k, e) = rt.heap.map_entries(m)[i];
                    n += quoted_len(rt.heap.str_bytes(k), false) as i64 + 4;
                    if n > budget {
                        break;
                    }
                    stack.push((e, true));
                }
            }
            _ => n += 24,
        }
    }
    n
}

fn join_guard(rt: &mut Rt<'_>, args: &[Value]) -> Result<i64, Exc> {
    let Some(&Value::List(l)) = args.first() else {
        return Ok(0);
    };
    let budget = rt.meter.lim.max_value_len as i64;
    let sep = str_of(args.get(1)).map_or(0, |s| i64::from(s.len));
    let count = rt.heap.list(l).items.len() as i64;
    let total = sum_estimate(join_size(rt, l, budget), checked_product(sep, count));
    rt.size_check(total)?;
    Ok(0)
}

fn join(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let [list, sep] = args else {
        return Err(rt.arity_error("join", 2, args.len()));
    };
    let Value::List(l) = *list else {
        return Err(rt.type_error("join() requires first argument to be an array"));
    };
    let Value::Str(sep) = *sep else {
        return Err(rt.type_error("join() requires second argument to be a string"));
    };
    let n = rt.heap.list(l).items.len();
    if n == 0 {
        return Ok(Value::Str(Str::EMPTY));
    }
    let made = build(rt, |rt, buf| {
        for i in 0..n {
            if i > 0 {
                let sep = rt.heap.str_bytes(sep);
                room(rt, buf, sep.len())?;
                buf.extend_from_slice(sep);
            }
            let v = rt.heap.list(l).items[i];
            to_text(rt.env, rt.heap, rt.meter, v, false, 0, buf)?;
        }
        Ok(())
    })?;
    Ok(Value::Str(made))
}

/// The most parts a split of the first argument by the second can make; a split at white space of `n` bytes has
/// at most `n / 2 + 1`.
fn split_parts(rt: &Rt<'_>, args: &[Value]) -> Option<usize> {
    let s = str_of(args.first())?;
    let bytes = rt.heap.str_bytes(s);
    Some(match str_of(args.get(1)) {
        Some(sep) if sep.len == 0 => rune_count(bytes),
        Some(sep) => {
            let needle = rt.heap.str_bytes(sep);
            Finder::new(needle).count(needle, bytes) + 1
        }
        None => bytes.len() / 2 + 1,
    })
}

fn split_reserve(rt: &mut Rt<'_>, args: &[Value]) -> Result<i64, Exc> {
    if let Some(Value::Str(s)) = args.first() {
        rt.work(Count::Free, u64::from(s.len))?;
    }
    let Some(parts) = split_parts(rt, args) else {
        return Ok(0);
    };
    rt.size_check(parts as i64)?;
    rt.reserve(sum_estimate(
        LIST_COST,
        checked_product(SPLIT_PART_COST, parts as i64),
    ))
}

fn split(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    if args.len() != 1 && args.len() != 2 {
        return Err(rt.type_error(format!("split() requires 1 or 2 args, got {}", args.len())));
    }
    let Value::Str(s) = args[0] else {
        return Err(rt.type_error("split() requires first argument to be a string"));
    };
    let sep = match args.get(1) {
        None | Some(Value::Null) => None,
        Some(&Value::Str(sep)) => Some(sep),
        Some(_) => return Err(rt.type_error("split() requires separator to be a str or null")),
    };
    rt.work(Count::Free, u64::from(s.len))?;
    let mut parts: Vec<Value> = Vec::new();
    if matches!(sep, Some(sep) if sep.len == 0) {
        explode(rt, s, &mut parts)?;
        return list_of(rt, &parts);
    }
    let bytes = rt.heap.str_bytes(s);
    match sep {
        None => fields(bytes, s, &mut parts),
        Some(sep) => {
            let needle = rt.heap.str_bytes(sep);
            let finder = Finder::new(needle);
            let (mut at, step) = (0, sep.len as usize);
            while let Some(i) = finder.find(needle, bytes, at) {
                parts.push(Value::Str(part(s, at, i)));
                at = i + step;
            }
            parts.push(Value::Str(part(s, at, bytes.len())));
        }
    }
    list_of(rt, &parts)
}

/// The runs of the bytes that are not white space, as parts of `s`.
fn fields(bytes: &[u8], s: Str, out: &mut Vec<Value>) {
    let mut start = None;
    let mut i = 0;
    while i < bytes.len() {
        let (r, w) = if bytes[i] < 0x80 {
            (u32::from(bytes[i]), 1)
        } else {
            decode_rune(&bytes[i..])
        };
        if is_space(r) {
            if let Some(from) = start.take() {
                out.push(Value::Str(part(s, from, i)));
            }
        } else if start.is_none() {
            start = Some(i);
        }
        i += w;
    }
    if let Some(from) = start {
        out.push(Value::Str(part(s, from, bytes.len())));
    }
}

/// One part per character. A byte that is not part of a valid sequence is one character, U+FFFD, as it is in
/// `for`, `reverse_str`, `upper` and `lower`.
fn explode(rt: &mut Rt<'_>, s: Str, out: &mut Vec<Value>) -> Result<(), Exc> {
    let mut i = 0;
    while i < s.len as usize {
        let (r, w) = decode_rune(&rt.heap.str_bytes(s)[i..]);
        let item = if r == RUNE_ERROR && w == 1 {
            rt.heap.new_str(REPLACEMENT).map_err(rt.oom())?
        } else {
            part(s, i, i + w)
        };
        out.push(Value::Str(item));
        i += w;
    }
    Ok(())
}

/// `lower` and `upper`: the string itself when no rune changes.
fn change_case(rt: &mut Rt<'_>, args: &[Value], name: &str, case: usize) -> Result<Value, Exc> {
    let [arg] = args else {
        return Err(rt.arity_error(name, 1, args.len()));
    };
    let Value::Str(s) = *arg else {
        return Err(rt.type_error(format!("{name}() requires a string")));
    };
    rt.work(Count::Free, u64::from(s.len))?;
    let made = build_if(rt, |rt, buf| {
        let bytes = rt.heap.str_bytes(s);
        room(rt, buf, bytes.len())?;
        let mut changed = false;
        let mut i = 0;
        while i < bytes.len() {
            let (r, w) = decode_rune(&bytes[i..]);
            if r == RUNE_ERROR && w == 1 {
                changed = true;
                room(rt, buf, 3)?;
                buf.extend_from_slice(REPLACEMENT);
            } else {
                let m = to_case(case, r);
                changed |= m != r;
                if m < 0x80 {
                    buf.push(m as u8);
                } else {
                    room(rt, buf, 4)?;
                    push_rune(buf, m);
                }
            }
            i += w;
        }
        Ok(changed)
    })?;
    Ok(Value::Str(made.unwrap_or(s)))
}

/// `text` with every character mapped to its lower or upper case, a byte that is not part of a valid sequence
/// becoming U+FFFD, as the reference's `strings.ToLower` and `strings.ToUpper` write it.
fn map_case(text: &[u8], case: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let (r, w) = decode_rune(&text[i..]);
        if r == RUNE_ERROR && w == 1 {
            out.extend_from_slice(REPLACEMENT);
        } else {
            push_rune(&mut out, to_case(case, r));
        }
        i += w;
    }
    out
}

/// `text` in lower case, for a comparison that ignores case.
pub fn lower_text(text: &[u8]) -> Vec<u8> {
    map_case(text, LOWER)
}

/// `text` in upper case.
pub fn upper_text(text: &[u8]) -> Vec<u8> {
    map_case(text, UPPER)
}

/// `text` without the white space at both ends.
pub fn trim_space(text: &[u8]) -> &[u8] {
    let (mut start, mut stop) = (0, text.len());
    while start < stop {
        let (r, w) = decode_rune(&text[start..stop]);
        if !is_space(r) {
            break;
        }
        start += w;
    }
    while stop > start {
        let (r, w) = decode_last_rune(&text[start..stop]);
        if !is_space(r) {
            break;
        }
        stop -= w;
    }
    &text[start..stop]
}

fn lower(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    change_case(rt, args, "lower", LOWER)
}

fn upper(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    change_case(rt, args, "upper", UPPER)
}

fn replace_guard(rt: &mut Rt<'_>, args: &[Value]) -> Result<i64, Exc> {
    // An argument that is not a string counts as an empty one here; the call reports its type.
    let len = |i| str_of(args.get(i)).map_or(0, |s| i64::from(s.len));
    let (s_len, old_len, new_len) = (len(0), len(1), len(2));
    if new_len > old_len {
        let count = if old_len == 0 {
            s_len + 1
        } else {
            rt.work(Count::Free, s_len as u64)?;
            let hay = str_of(args.first()).map_or(&[][..], |s| rt.heap.str_bytes(s));
            let old = str_of(args.get(1)).map_or(&[][..], |s| rt.heap.str_bytes(s));
            Finder::new(old).count(old, hay) as i64
        };
        rt.size_check(s_len + checked_product(new_len - old_len, count))?;
    }
    Ok(0)
}

fn replace(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let [s, old, new] = args else {
        return Err(rt.arity_error("replace", 3, args.len()));
    };
    let Value::Str(s) = *s else {
        return Err(rt.type_error("replace() requires first argument to be a string"));
    };
    let Value::Str(old) = *old else {
        return Err(rt.type_error("replace() requires second argument to be a string"));
    };
    let Value::Str(new) = *new else {
        return Err(rt.type_error("replace() requires third argument to be a string"));
    };
    rt.work(Count::Free, u64::from(s.len))?;
    // Nothing to change when the text to find is the text to put, or does not occur: the result is the string.
    let finder = {
        let (b, o, n) = (
            rt.heap.str_bytes(s),
            rt.heap.str_bytes(old),
            rt.heap.str_bytes(new),
        );
        let finder = Finder::new(o);
        if o == n || finder.count(o, b) == 0 {
            return Ok(Value::Str(s));
        }
        finder
    };
    let made = build(rt, |rt, buf| {
        let bytes = rt.heap.str_bytes(s);
        let (o, n) = (rt.heap.str_bytes(old), rt.heap.str_bytes(new));
        if o.is_empty() {
            room(rt, buf, bytes.len() + n.len() * (rune_count(bytes) + 1))?;
            let mut i = 0;
            while i < bytes.len() {
                buf.extend_from_slice(n);
                let w = decode_rune(&bytes[i..]).1;
                buf.extend_from_slice(&bytes[i..i + w]);
                i += w;
            }
            buf.extend_from_slice(n);
            return Ok(());
        }
        let mut at = 0;
        room(rt, buf, bytes.len())?;
        while let Some(i) = finder.find(o, bytes, at) {
            room(rt, buf, i - at + n.len())?;
            buf.extend_from_slice(&bytes[at..i]);
            buf.extend_from_slice(n);
            at = i + o.len();
        }
        buf.extend_from_slice(&bytes[at..]);
        Ok(())
    })?;
    Ok(Value::Str(made))
}

fn reverse_str(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let [arg] = args else {
        return Err(rt.arity_error("reverse_str", 1, args.len()));
    };
    let Value::Str(s) = *arg else {
        return Err(rt.type_error("reverse_str() requires a string argument"));
    };
    rt.work(Count::Free, u64::from(s.len))?;
    let made = build(rt, |rt, buf| {
        let bytes = rt.heap.str_bytes(s);
        // Each rune is written from the end, so the width of the result is counted first.
        let (mut i, mut total) = (0, 0);
        while i < bytes.len() {
            let (r, w) = decode_rune(&bytes[i..]);
            total += if r == RUNE_ERROR { 3 } else { w };
            i += w;
        }
        room(rt, buf, total)?;
        buf.resize(total, 0);
        let (mut i, mut end) = (0, total);
        while i < bytes.len() {
            let (r, w) = decode_rune(&bytes[i..]);
            let width = if r == RUNE_ERROR { 3 } else { w };
            end -= width;
            if r == RUNE_ERROR {
                buf[end..end + 3].copy_from_slice(REPLACEMENT);
            } else {
                buf[end..end + width].copy_from_slice(&bytes[i..i + w]);
            }
            i += w;
        }
        Ok(())
    })?;
    Ok(Value::Str(made))
}

fn substr(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let [s, start, end] = args else {
        return Err(rt.arity_error("substr", 3, args.len()));
    };
    let Value::Str(s) = *s else {
        return Err(rt.type_error("substr() requires first argument to be a string"));
    };
    let Value::Int(start) = *start else {
        return Err(rt.type_error("substr() requires second argument to be an integer"));
    };
    let Value::Int(end) = *end else {
        return Err(rt.type_error("substr() requires third argument to be an integer"));
    };
    let len = i64::from(s.len);
    if start < 0 || end < start || end > len {
        return Err(Exc::value_error(
            rt.pos,
            format!("substr() range {start}:{end} is outside a string of {len} bytes"),
        ));
    }
    Ok(Value::Str(part(s, start as usize, end as usize)))
}

fn trim(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let [arg] = args else {
        return Err(rt.arity_error("trim", 1, args.len()));
    };
    let Value::Str(s) = *arg else {
        return Err(rt.type_error("trim() requires a string argument"));
    };
    rt.work(Count::Free, u64::from(s.len))?;
    let bytes = rt.heap.str_bytes(s);
    let (mut start, mut stop) = (0, bytes.len());
    while start < stop {
        let (r, w) = if bytes[start] < 0x80 {
            (u32::from(bytes[start]), 1)
        } else {
            decode_rune(&bytes[start..stop])
        };
        if !is_space(r) {
            break;
        }
        start += w;
    }
    while stop > start {
        let (r, w) = decode_last_rune(&bytes[start..stop]);
        if !is_space(r) {
            break;
        }
        stop -= w;
    }
    Ok(Value::Str(if start == 0 && stop == bytes.len() {
        s
    } else {
        part(s, start, stop)
    }))
}

fn affix(
    rt: &mut Rt<'_>,
    args: &[Value],
    name: &str,
    test: fn(&[u8], &[u8]) -> bool,
) -> Result<Value, Exc> {
    let [s, other] = args else {
        return Err(rt.arity_error(name, 2, args.len()));
    };
    let Value::Str(s) = *s else {
        return Err(rt.type_error(format!("{name}() requires first argument to be a string")));
    };
    let Value::Str(other) = *other else {
        return Err(rt.type_error(format!("{name}() requires second argument to be a string")));
    };
    Ok(Value::Bool(test(
        rt.heap.str_bytes(s),
        rt.heap.str_bytes(other),
    )))
}

fn starts_with(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    affix(rt, args, "starts_with", |s, p| s.starts_with(p))
}

fn ends_with(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    affix(rt, args, "ends_with", |s, p| s.ends_with(p))
}

/// The simple case mappings of Unicode 15.0.0: first and last rune of a run, and the distance to the upper,
/// lower and title case rune. A distance above the last rune marks a run that alternates upper and lower case.
const CASE_RANGES: &[(u32, u32, [i32; 3])] = &[
    (0x41, 0x5A, [0, 32, 0]),
    (0x61, 0x7A, [-32, 0, -32]),
    (0xB5, 0xB5, [743, 0, 743]),
    (0xC0, 0xD6, [0, 32, 0]),
    (0xD8, 0xDE, [0, 32, 0]),
    (0xE0, 0xF6, [-32, 0, -32]),
    (0xF8, 0xFE, [-32, 0, -32]),
    (0xFF, 0xFF, [121, 0, 121]),
    (0x100, 0x12F, [1114112, 1114112, 1114112]),
    (0x130, 0x130, [0, -199, 0]),
    (0x131, 0x131, [-232, 0, -232]),
    (0x132, 0x137, [1114112, 1114112, 1114112]),
    (0x139, 0x148, [1114112, 1114112, 1114112]),
    (0x14A, 0x177, [1114112, 1114112, 1114112]),
    (0x178, 0x178, [0, -121, 0]),
    (0x179, 0x17E, [1114112, 1114112, 1114112]),
    (0x17F, 0x17F, [-300, 0, -300]),
    (0x180, 0x180, [195, 0, 195]),
    (0x181, 0x181, [0, 210, 0]),
    (0x182, 0x185, [1114112, 1114112, 1114112]),
    (0x186, 0x186, [0, 206, 0]),
    (0x187, 0x188, [1114112, 1114112, 1114112]),
    (0x189, 0x18A, [0, 205, 0]),
    (0x18B, 0x18C, [1114112, 1114112, 1114112]),
    (0x18E, 0x18E, [0, 79, 0]),
    (0x18F, 0x18F, [0, 202, 0]),
    (0x190, 0x190, [0, 203, 0]),
    (0x191, 0x192, [1114112, 1114112, 1114112]),
    (0x193, 0x193, [0, 205, 0]),
    (0x194, 0x194, [0, 207, 0]),
    (0x195, 0x195, [97, 0, 97]),
    (0x196, 0x196, [0, 211, 0]),
    (0x197, 0x197, [0, 209, 0]),
    (0x198, 0x199, [1114112, 1114112, 1114112]),
    (0x19A, 0x19A, [163, 0, 163]),
    (0x19C, 0x19C, [0, 211, 0]),
    (0x19D, 0x19D, [0, 213, 0]),
    (0x19E, 0x19E, [130, 0, 130]),
    (0x19F, 0x19F, [0, 214, 0]),
    (0x1A0, 0x1A5, [1114112, 1114112, 1114112]),
    (0x1A6, 0x1A6, [0, 218, 0]),
    (0x1A7, 0x1A8, [1114112, 1114112, 1114112]),
    (0x1A9, 0x1A9, [0, 218, 0]),
    (0x1AC, 0x1AD, [1114112, 1114112, 1114112]),
    (0x1AE, 0x1AE, [0, 218, 0]),
    (0x1AF, 0x1B0, [1114112, 1114112, 1114112]),
    (0x1B1, 0x1B2, [0, 217, 0]),
    (0x1B3, 0x1B6, [1114112, 1114112, 1114112]),
    (0x1B7, 0x1B7, [0, 219, 0]),
    (0x1B8, 0x1B9, [1114112, 1114112, 1114112]),
    (0x1BC, 0x1BD, [1114112, 1114112, 1114112]),
    (0x1BF, 0x1BF, [56, 0, 56]),
    (0x1C4, 0x1C4, [0, 2, 1]),
    (0x1C5, 0x1C5, [-1, 1, 0]),
    (0x1C6, 0x1C6, [-2, 0, -1]),
    (0x1C7, 0x1C7, [0, 2, 1]),
    (0x1C8, 0x1C8, [-1, 1, 0]),
    (0x1C9, 0x1C9, [-2, 0, -1]),
    (0x1CA, 0x1CA, [0, 2, 1]),
    (0x1CB, 0x1CB, [-1, 1, 0]),
    (0x1CC, 0x1CC, [-2, 0, -1]),
    (0x1CD, 0x1DC, [1114112, 1114112, 1114112]),
    (0x1DD, 0x1DD, [-79, 0, -79]),
    (0x1DE, 0x1EF, [1114112, 1114112, 1114112]),
    (0x1F1, 0x1F1, [0, 2, 1]),
    (0x1F2, 0x1F2, [-1, 1, 0]),
    (0x1F3, 0x1F3, [-2, 0, -1]),
    (0x1F4, 0x1F5, [1114112, 1114112, 1114112]),
    (0x1F6, 0x1F6, [0, -97, 0]),
    (0x1F7, 0x1F7, [0, -56, 0]),
    (0x1F8, 0x21F, [1114112, 1114112, 1114112]),
    (0x220, 0x220, [0, -130, 0]),
    (0x222, 0x233, [1114112, 1114112, 1114112]),
    (0x23A, 0x23A, [0, 10795, 0]),
    (0x23B, 0x23C, [1114112, 1114112, 1114112]),
    (0x23D, 0x23D, [0, -163, 0]),
    (0x23E, 0x23E, [0, 10792, 0]),
    (0x23F, 0x240, [10815, 0, 10815]),
    (0x241, 0x242, [1114112, 1114112, 1114112]),
    (0x243, 0x243, [0, -195, 0]),
    (0x244, 0x244, [0, 69, 0]),
    (0x245, 0x245, [0, 71, 0]),
    (0x246, 0x24F, [1114112, 1114112, 1114112]),
    (0x250, 0x250, [10783, 0, 10783]),
    (0x251, 0x251, [10780, 0, 10780]),
    (0x252, 0x252, [10782, 0, 10782]),
    (0x253, 0x253, [-210, 0, -210]),
    (0x254, 0x254, [-206, 0, -206]),
    (0x256, 0x257, [-205, 0, -205]),
    (0x259, 0x259, [-202, 0, -202]),
    (0x25B, 0x25B, [-203, 0, -203]),
    (0x25C, 0x25C, [42319, 0, 42319]),
    (0x260, 0x260, [-205, 0, -205]),
    (0x261, 0x261, [42315, 0, 42315]),
    (0x263, 0x263, [-207, 0, -207]),
    (0x265, 0x265, [42280, 0, 42280]),
    (0x266, 0x266, [42308, 0, 42308]),
    (0x268, 0x268, [-209, 0, -209]),
    (0x269, 0x269, [-211, 0, -211]),
    (0x26A, 0x26A, [42308, 0, 42308]),
    (0x26B, 0x26B, [10743, 0, 10743]),
    (0x26C, 0x26C, [42305, 0, 42305]),
    (0x26F, 0x26F, [-211, 0, -211]),
    (0x271, 0x271, [10749, 0, 10749]),
    (0x272, 0x272, [-213, 0, -213]),
    (0x275, 0x275, [-214, 0, -214]),
    (0x27D, 0x27D, [10727, 0, 10727]),
    (0x280, 0x280, [-218, 0, -218]),
    (0x282, 0x282, [42307, 0, 42307]),
    (0x283, 0x283, [-218, 0, -218]),
    (0x287, 0x287, [42282, 0, 42282]),
    (0x288, 0x288, [-218, 0, -218]),
    (0x289, 0x289, [-69, 0, -69]),
    (0x28A, 0x28B, [-217, 0, -217]),
    (0x28C, 0x28C, [-71, 0, -71]),
    (0x292, 0x292, [-219, 0, -219]),
    (0x29D, 0x29D, [42261, 0, 42261]),
    (0x29E, 0x29E, [42258, 0, 42258]),
    (0x345, 0x345, [84, 0, 84]),
    (0x370, 0x373, [1114112, 1114112, 1114112]),
    (0x376, 0x377, [1114112, 1114112, 1114112]),
    (0x37B, 0x37D, [130, 0, 130]),
    (0x37F, 0x37F, [0, 116, 0]),
    (0x386, 0x386, [0, 38, 0]),
    (0x388, 0x38A, [0, 37, 0]),
    (0x38C, 0x38C, [0, 64, 0]),
    (0x38E, 0x38F, [0, 63, 0]),
    (0x391, 0x3A1, [0, 32, 0]),
    (0x3A3, 0x3AB, [0, 32, 0]),
    (0x3AC, 0x3AC, [-38, 0, -38]),
    (0x3AD, 0x3AF, [-37, 0, -37]),
    (0x3B1, 0x3C1, [-32, 0, -32]),
    (0x3C2, 0x3C2, [-31, 0, -31]),
    (0x3C3, 0x3CB, [-32, 0, -32]),
    (0x3CC, 0x3CC, [-64, 0, -64]),
    (0x3CD, 0x3CE, [-63, 0, -63]),
    (0x3CF, 0x3CF, [0, 8, 0]),
    (0x3D0, 0x3D0, [-62, 0, -62]),
    (0x3D1, 0x3D1, [-57, 0, -57]),
    (0x3D5, 0x3D5, [-47, 0, -47]),
    (0x3D6, 0x3D6, [-54, 0, -54]),
    (0x3D7, 0x3D7, [-8, 0, -8]),
    (0x3D8, 0x3EF, [1114112, 1114112, 1114112]),
    (0x3F0, 0x3F0, [-86, 0, -86]),
    (0x3F1, 0x3F1, [-80, 0, -80]),
    (0x3F2, 0x3F2, [7, 0, 7]),
    (0x3F3, 0x3F3, [-116, 0, -116]),
    (0x3F4, 0x3F4, [0, -60, 0]),
    (0x3F5, 0x3F5, [-96, 0, -96]),
    (0x3F7, 0x3F8, [1114112, 1114112, 1114112]),
    (0x3F9, 0x3F9, [0, -7, 0]),
    (0x3FA, 0x3FB, [1114112, 1114112, 1114112]),
    (0x3FD, 0x3FF, [0, -130, 0]),
    (0x400, 0x40F, [0, 80, 0]),
    (0x410, 0x42F, [0, 32, 0]),
    (0x430, 0x44F, [-32, 0, -32]),
    (0x450, 0x45F, [-80, 0, -80]),
    (0x460, 0x481, [1114112, 1114112, 1114112]),
    (0x48A, 0x4BF, [1114112, 1114112, 1114112]),
    (0x4C0, 0x4C0, [0, 15, 0]),
    (0x4C1, 0x4CE, [1114112, 1114112, 1114112]),
    (0x4CF, 0x4CF, [-15, 0, -15]),
    (0x4D0, 0x52F, [1114112, 1114112, 1114112]),
    (0x531, 0x556, [0, 48, 0]),
    (0x561, 0x586, [-48, 0, -48]),
    (0x10A0, 0x10C5, [0, 7264, 0]),
    (0x10C7, 0x10C7, [0, 7264, 0]),
    (0x10CD, 0x10CD, [0, 7264, 0]),
    (0x10D0, 0x10FA, [3008, 0, 0]),
    (0x10FD, 0x10FF, [3008, 0, 0]),
    (0x13A0, 0x13EF, [0, 38864, 0]),
    (0x13F0, 0x13F5, [0, 8, 0]),
    (0x13F8, 0x13FD, [-8, 0, -8]),
    (0x1C80, 0x1C80, [-6254, 0, -6254]),
    (0x1C81, 0x1C81, [-6253, 0, -6253]),
    (0x1C82, 0x1C82, [-6244, 0, -6244]),
    (0x1C83, 0x1C84, [-6242, 0, -6242]),
    (0x1C85, 0x1C85, [-6243, 0, -6243]),
    (0x1C86, 0x1C86, [-6236, 0, -6236]),
    (0x1C87, 0x1C87, [-6181, 0, -6181]),
    (0x1C88, 0x1C88, [35266, 0, 35266]),
    (0x1C90, 0x1CBA, [0, -3008, 0]),
    (0x1CBD, 0x1CBF, [0, -3008, 0]),
    (0x1D79, 0x1D79, [35332, 0, 35332]),
    (0x1D7D, 0x1D7D, [3814, 0, 3814]),
    (0x1D8E, 0x1D8E, [35384, 0, 35384]),
    (0x1E00, 0x1E95, [1114112, 1114112, 1114112]),
    (0x1E9B, 0x1E9B, [-59, 0, -59]),
    (0x1E9E, 0x1E9E, [0, -7615, 0]),
    (0x1EA0, 0x1EFF, [1114112, 1114112, 1114112]),
    (0x1F00, 0x1F07, [8, 0, 8]),
    (0x1F08, 0x1F0F, [0, -8, 0]),
    (0x1F10, 0x1F15, [8, 0, 8]),
    (0x1F18, 0x1F1D, [0, -8, 0]),
    (0x1F20, 0x1F27, [8, 0, 8]),
    (0x1F28, 0x1F2F, [0, -8, 0]),
    (0x1F30, 0x1F37, [8, 0, 8]),
    (0x1F38, 0x1F3F, [0, -8, 0]),
    (0x1F40, 0x1F45, [8, 0, 8]),
    (0x1F48, 0x1F4D, [0, -8, 0]),
    (0x1F51, 0x1F51, [8, 0, 8]),
    (0x1F53, 0x1F53, [8, 0, 8]),
    (0x1F55, 0x1F55, [8, 0, 8]),
    (0x1F57, 0x1F57, [8, 0, 8]),
    (0x1F59, 0x1F59, [0, -8, 0]),
    (0x1F5B, 0x1F5B, [0, -8, 0]),
    (0x1F5D, 0x1F5D, [0, -8, 0]),
    (0x1F5F, 0x1F5F, [0, -8, 0]),
    (0x1F60, 0x1F67, [8, 0, 8]),
    (0x1F68, 0x1F6F, [0, -8, 0]),
    (0x1F70, 0x1F71, [74, 0, 74]),
    (0x1F72, 0x1F75, [86, 0, 86]),
    (0x1F76, 0x1F77, [100, 0, 100]),
    (0x1F78, 0x1F79, [128, 0, 128]),
    (0x1F7A, 0x1F7B, [112, 0, 112]),
    (0x1F7C, 0x1F7D, [126, 0, 126]),
    (0x1F80, 0x1F87, [8, 0, 8]),
    (0x1F88, 0x1F8F, [0, -8, 0]),
    (0x1F90, 0x1F97, [8, 0, 8]),
    (0x1F98, 0x1F9F, [0, -8, 0]),
    (0x1FA0, 0x1FA7, [8, 0, 8]),
    (0x1FA8, 0x1FAF, [0, -8, 0]),
    (0x1FB0, 0x1FB1, [8, 0, 8]),
    (0x1FB3, 0x1FB3, [9, 0, 9]),
    (0x1FB8, 0x1FB9, [0, -8, 0]),
    (0x1FBA, 0x1FBB, [0, -74, 0]),
    (0x1FBC, 0x1FBC, [0, -9, 0]),
    (0x1FBE, 0x1FBE, [-7205, 0, -7205]),
    (0x1FC3, 0x1FC3, [9, 0, 9]),
    (0x1FC8, 0x1FCB, [0, -86, 0]),
    (0x1FCC, 0x1FCC, [0, -9, 0]),
    (0x1FD0, 0x1FD1, [8, 0, 8]),
    (0x1FD8, 0x1FD9, [0, -8, 0]),
    (0x1FDA, 0x1FDB, [0, -100, 0]),
    (0x1FE0, 0x1FE1, [8, 0, 8]),
    (0x1FE5, 0x1FE5, [7, 0, 7]),
    (0x1FE8, 0x1FE9, [0, -8, 0]),
    (0x1FEA, 0x1FEB, [0, -112, 0]),
    (0x1FEC, 0x1FEC, [0, -7, 0]),
    (0x1FF3, 0x1FF3, [9, 0, 9]),
    (0x1FF8, 0x1FF9, [0, -128, 0]),
    (0x1FFA, 0x1FFB, [0, -126, 0]),
    (0x1FFC, 0x1FFC, [0, -9, 0]),
    (0x2126, 0x2126, [0, -7517, 0]),
    (0x212A, 0x212A, [0, -8383, 0]),
    (0x212B, 0x212B, [0, -8262, 0]),
    (0x2132, 0x2132, [0, 28, 0]),
    (0x214E, 0x214E, [-28, 0, -28]),
    (0x2160, 0x216F, [0, 16, 0]),
    (0x2170, 0x217F, [-16, 0, -16]),
    (0x2183, 0x2184, [1114112, 1114112, 1114112]),
    (0x24B6, 0x24CF, [0, 26, 0]),
    (0x24D0, 0x24E9, [-26, 0, -26]),
    (0x2C00, 0x2C2F, [0, 48, 0]),
    (0x2C30, 0x2C5F, [-48, 0, -48]),
    (0x2C60, 0x2C61, [1114112, 1114112, 1114112]),
    (0x2C62, 0x2C62, [0, -10743, 0]),
    (0x2C63, 0x2C63, [0, -3814, 0]),
    (0x2C64, 0x2C64, [0, -10727, 0]),
    (0x2C65, 0x2C65, [-10795, 0, -10795]),
    (0x2C66, 0x2C66, [-10792, 0, -10792]),
    (0x2C67, 0x2C6C, [1114112, 1114112, 1114112]),
    (0x2C6D, 0x2C6D, [0, -10780, 0]),
    (0x2C6E, 0x2C6E, [0, -10749, 0]),
    (0x2C6F, 0x2C6F, [0, -10783, 0]),
    (0x2C70, 0x2C70, [0, -10782, 0]),
    (0x2C72, 0x2C73, [1114112, 1114112, 1114112]),
    (0x2C75, 0x2C76, [1114112, 1114112, 1114112]),
    (0x2C7E, 0x2C7F, [0, -10815, 0]),
    (0x2C80, 0x2CE3, [1114112, 1114112, 1114112]),
    (0x2CEB, 0x2CEE, [1114112, 1114112, 1114112]),
    (0x2CF2, 0x2CF3, [1114112, 1114112, 1114112]),
    (0x2D00, 0x2D25, [-7264, 0, -7264]),
    (0x2D27, 0x2D27, [-7264, 0, -7264]),
    (0x2D2D, 0x2D2D, [-7264, 0, -7264]),
    (0xA640, 0xA66D, [1114112, 1114112, 1114112]),
    (0xA680, 0xA69B, [1114112, 1114112, 1114112]),
    (0xA722, 0xA72F, [1114112, 1114112, 1114112]),
    (0xA732, 0xA76F, [1114112, 1114112, 1114112]),
    (0xA779, 0xA77C, [1114112, 1114112, 1114112]),
    (0xA77D, 0xA77D, [0, -35332, 0]),
    (0xA77E, 0xA787, [1114112, 1114112, 1114112]),
    (0xA78B, 0xA78C, [1114112, 1114112, 1114112]),
    (0xA78D, 0xA78D, [0, -42280, 0]),
    (0xA790, 0xA793, [1114112, 1114112, 1114112]),
    (0xA794, 0xA794, [48, 0, 48]),
    (0xA796, 0xA7A9, [1114112, 1114112, 1114112]),
    (0xA7AA, 0xA7AA, [0, -42308, 0]),
    (0xA7AB, 0xA7AB, [0, -42319, 0]),
    (0xA7AC, 0xA7AC, [0, -42315, 0]),
    (0xA7AD, 0xA7AD, [0, -42305, 0]),
    (0xA7AE, 0xA7AE, [0, -42308, 0]),
    (0xA7B0, 0xA7B0, [0, -42258, 0]),
    (0xA7B1, 0xA7B1, [0, -42282, 0]),
    (0xA7B2, 0xA7B2, [0, -42261, 0]),
    (0xA7B3, 0xA7B3, [0, 928, 0]),
    (0xA7B4, 0xA7C3, [1114112, 1114112, 1114112]),
    (0xA7C4, 0xA7C4, [0, -48, 0]),
    (0xA7C5, 0xA7C5, [0, -42307, 0]),
    (0xA7C6, 0xA7C6, [0, -35384, 0]),
    (0xA7C7, 0xA7CA, [1114112, 1114112, 1114112]),
    (0xA7D0, 0xA7D1, [1114112, 1114112, 1114112]),
    (0xA7D6, 0xA7D9, [1114112, 1114112, 1114112]),
    (0xA7F5, 0xA7F6, [1114112, 1114112, 1114112]),
    (0xAB53, 0xAB53, [-928, 0, -928]),
    (0xAB70, 0xABBF, [-38864, 0, -38864]),
    (0xFF21, 0xFF3A, [0, 32, 0]),
    (0xFF41, 0xFF5A, [-32, 0, -32]),
    (0x10400, 0x10427, [0, 40, 0]),
    (0x10428, 0x1044F, [-40, 0, -40]),
    (0x104B0, 0x104D3, [0, 40, 0]),
    (0x104D8, 0x104FB, [-40, 0, -40]),
    (0x10570, 0x1057A, [0, 39, 0]),
    (0x1057C, 0x1058A, [0, 39, 0]),
    (0x1058C, 0x10592, [0, 39, 0]),
    (0x10594, 0x10595, [0, 39, 0]),
    (0x10597, 0x105A1, [-39, 0, -39]),
    (0x105A3, 0x105B1, [-39, 0, -39]),
    (0x105B3, 0x105B9, [-39, 0, -39]),
    (0x105BB, 0x105BC, [-39, 0, -39]),
    (0x10C80, 0x10CB2, [0, 64, 0]),
    (0x10CC0, 0x10CF2, [-64, 0, -64]),
    (0x118A0, 0x118BF, [0, 32, 0]),
    (0x118C0, 0x118DF, [-32, 0, -32]),
    (0x16E40, 0x16E5F, [0, 32, 0]),
    (0x16E60, 0x16E7F, [-32, 0, -32]),
    (0x1E900, 0x1E921, [0, 34, 0]),
    (0x1E922, 0x1E943, [-34, 0, -34]),
];
/// The runes that print: letters, marks, numbers, punctuation, symbols and the ASCII space.
const IS_PRINT: &[(u32, u32)] = &[
    (0x20, 0x7E),
    (0xA1, 0xAC),
    (0xAE, 0x377),
    (0x37A, 0x37F),
    (0x384, 0x38A),
    (0x38C, 0x38C),
    (0x38E, 0x3A1),
    (0x3A3, 0x52F),
    (0x531, 0x556),
    (0x559, 0x58A),
    (0x58D, 0x58F),
    (0x591, 0x5C7),
    (0x5D0, 0x5EA),
    (0x5EF, 0x5F4),
    (0x606, 0x61B),
    (0x61D, 0x6DC),
    (0x6DE, 0x70D),
    (0x710, 0x74A),
    (0x74D, 0x7B1),
    (0x7C0, 0x7FA),
    (0x7FD, 0x82D),
    (0x830, 0x83E),
    (0x840, 0x85B),
    (0x85E, 0x85E),
    (0x860, 0x86A),
    (0x870, 0x88E),
    (0x898, 0x8E1),
    (0x8E3, 0x983),
    (0x985, 0x98C),
    (0x98F, 0x990),
    (0x993, 0x9A8),
    (0x9AA, 0x9B0),
    (0x9B2, 0x9B2),
    (0x9B6, 0x9B9),
    (0x9BC, 0x9C4),
    (0x9C7, 0x9C8),
    (0x9CB, 0x9CE),
    (0x9D7, 0x9D7),
    (0x9DC, 0x9DD),
    (0x9DF, 0x9E3),
    (0x9E6, 0x9FE),
    (0xA01, 0xA03),
    (0xA05, 0xA0A),
    (0xA0F, 0xA10),
    (0xA13, 0xA28),
    (0xA2A, 0xA30),
    (0xA32, 0xA33),
    (0xA35, 0xA36),
    (0xA38, 0xA39),
    (0xA3C, 0xA3C),
    (0xA3E, 0xA42),
    (0xA47, 0xA48),
    (0xA4B, 0xA4D),
    (0xA51, 0xA51),
    (0xA59, 0xA5C),
    (0xA5E, 0xA5E),
    (0xA66, 0xA76),
    (0xA81, 0xA83),
    (0xA85, 0xA8D),
    (0xA8F, 0xA91),
    (0xA93, 0xAA8),
    (0xAAA, 0xAB0),
    (0xAB2, 0xAB3),
    (0xAB5, 0xAB9),
    (0xABC, 0xAC5),
    (0xAC7, 0xAC9),
    (0xACB, 0xACD),
    (0xAD0, 0xAD0),
    (0xAE0, 0xAE3),
    (0xAE6, 0xAF1),
    (0xAF9, 0xAFF),
    (0xB01, 0xB03),
    (0xB05, 0xB0C),
    (0xB0F, 0xB10),
    (0xB13, 0xB28),
    (0xB2A, 0xB30),
    (0xB32, 0xB33),
    (0xB35, 0xB39),
    (0xB3C, 0xB44),
    (0xB47, 0xB48),
    (0xB4B, 0xB4D),
    (0xB55, 0xB57),
    (0xB5C, 0xB5D),
    (0xB5F, 0xB63),
    (0xB66, 0xB77),
    (0xB82, 0xB83),
    (0xB85, 0xB8A),
    (0xB8E, 0xB90),
    (0xB92, 0xB95),
    (0xB99, 0xB9A),
    (0xB9C, 0xB9C),
    (0xB9E, 0xB9F),
    (0xBA3, 0xBA4),
    (0xBA8, 0xBAA),
    (0xBAE, 0xBB9),
    (0xBBE, 0xBC2),
    (0xBC6, 0xBC8),
    (0xBCA, 0xBCD),
    (0xBD0, 0xBD0),
    (0xBD7, 0xBD7),
    (0xBE6, 0xBFA),
    (0xC00, 0xC0C),
    (0xC0E, 0xC10),
    (0xC12, 0xC28),
    (0xC2A, 0xC39),
    (0xC3C, 0xC44),
    (0xC46, 0xC48),
    (0xC4A, 0xC4D),
    (0xC55, 0xC56),
    (0xC58, 0xC5A),
    (0xC5D, 0xC5D),
    (0xC60, 0xC63),
    (0xC66, 0xC6F),
    (0xC77, 0xC8C),
    (0xC8E, 0xC90),
    (0xC92, 0xCA8),
    (0xCAA, 0xCB3),
    (0xCB5, 0xCB9),
    (0xCBC, 0xCC4),
    (0xCC6, 0xCC8),
    (0xCCA, 0xCCD),
    (0xCD5, 0xCD6),
    (0xCDD, 0xCDE),
    (0xCE0, 0xCE3),
    (0xCE6, 0xCEF),
    (0xCF1, 0xCF3),
    (0xD00, 0xD0C),
    (0xD0E, 0xD10),
    (0xD12, 0xD44),
    (0xD46, 0xD48),
    (0xD4A, 0xD4F),
    (0xD54, 0xD63),
    (0xD66, 0xD7F),
    (0xD81, 0xD83),
    (0xD85, 0xD96),
    (0xD9A, 0xDB1),
    (0xDB3, 0xDBB),
    (0xDBD, 0xDBD),
    (0xDC0, 0xDC6),
    (0xDCA, 0xDCA),
    (0xDCF, 0xDD4),
    (0xDD6, 0xDD6),
    (0xDD8, 0xDDF),
    (0xDE6, 0xDEF),
    (0xDF2, 0xDF4),
    (0xE01, 0xE3A),
    (0xE3F, 0xE5B),
    (0xE81, 0xE82),
    (0xE84, 0xE84),
    (0xE86, 0xE8A),
    (0xE8C, 0xEA3),
    (0xEA5, 0xEA5),
    (0xEA7, 0xEBD),
    (0xEC0, 0xEC4),
    (0xEC6, 0xEC6),
    (0xEC8, 0xECE),
    (0xED0, 0xED9),
    (0xEDC, 0xEDF),
    (0xF00, 0xF47),
    (0xF49, 0xF6C),
    (0xF71, 0xF97),
    (0xF99, 0xFBC),
    (0xFBE, 0xFCC),
    (0xFCE, 0xFDA),
    (0x1000, 0x10C5),
    (0x10C7, 0x10C7),
    (0x10CD, 0x10CD),
    (0x10D0, 0x1248),
    (0x124A, 0x124D),
    (0x1250, 0x1256),
    (0x1258, 0x1258),
    (0x125A, 0x125D),
    (0x1260, 0x1288),
    (0x128A, 0x128D),
    (0x1290, 0x12B0),
    (0x12B2, 0x12B5),
    (0x12B8, 0x12BE),
    (0x12C0, 0x12C0),
    (0x12C2, 0x12C5),
    (0x12C8, 0x12D6),
    (0x12D8, 0x1310),
    (0x1312, 0x1315),
    (0x1318, 0x135A),
    (0x135D, 0x137C),
    (0x1380, 0x1399),
    (0x13A0, 0x13F5),
    (0x13F8, 0x13FD),
    (0x1400, 0x167F),
    (0x1681, 0x169C),
    (0x16A0, 0x16F8),
    (0x1700, 0x1715),
    (0x171F, 0x1736),
    (0x1740, 0x1753),
    (0x1760, 0x176C),
    (0x176E, 0x1770),
    (0x1772, 0x1773),
    (0x1780, 0x17DD),
    (0x17E0, 0x17E9),
    (0x17F0, 0x17F9),
    (0x1800, 0x180D),
    (0x180F, 0x1819),
    (0x1820, 0x1878),
    (0x1880, 0x18AA),
    (0x18B0, 0x18F5),
    (0x1900, 0x191E),
    (0x1920, 0x192B),
    (0x1930, 0x193B),
    (0x1940, 0x1940),
    (0x1944, 0x196D),
    (0x1970, 0x1974),
    (0x1980, 0x19AB),
    (0x19B0, 0x19C9),
    (0x19D0, 0x19DA),
    (0x19DE, 0x1A1B),
    (0x1A1E, 0x1A5E),
    (0x1A60, 0x1A7C),
    (0x1A7F, 0x1A89),
    (0x1A90, 0x1A99),
    (0x1AA0, 0x1AAD),
    (0x1AB0, 0x1ACE),
    (0x1B00, 0x1B4C),
    (0x1B50, 0x1B7E),
    (0x1B80, 0x1BF3),
    (0x1BFC, 0x1C37),
    (0x1C3B, 0x1C49),
    (0x1C4D, 0x1C88),
    (0x1C90, 0x1CBA),
    (0x1CBD, 0x1CC7),
    (0x1CD0, 0x1CFA),
    (0x1D00, 0x1F15),
    (0x1F18, 0x1F1D),
    (0x1F20, 0x1F45),
    (0x1F48, 0x1F4D),
    (0x1F50, 0x1F57),
    (0x1F59, 0x1F59),
    (0x1F5B, 0x1F5B),
    (0x1F5D, 0x1F5D),
    (0x1F5F, 0x1F7D),
    (0x1F80, 0x1FB4),
    (0x1FB6, 0x1FC4),
    (0x1FC6, 0x1FD3),
    (0x1FD6, 0x1FDB),
    (0x1FDD, 0x1FEF),
    (0x1FF2, 0x1FF4),
    (0x1FF6, 0x1FFE),
    (0x2010, 0x2027),
    (0x2030, 0x205E),
    (0x2070, 0x2071),
    (0x2074, 0x208E),
    (0x2090, 0x209C),
    (0x20A0, 0x20C0),
    (0x20D0, 0x20F0),
    (0x2100, 0x218B),
    (0x2190, 0x2426),
    (0x2440, 0x244A),
    (0x2460, 0x2B73),
    (0x2B76, 0x2B95),
    (0x2B97, 0x2CF3),
    (0x2CF9, 0x2D25),
    (0x2D27, 0x2D27),
    (0x2D2D, 0x2D2D),
    (0x2D30, 0x2D67),
    (0x2D6F, 0x2D70),
    (0x2D7F, 0x2D96),
    (0x2DA0, 0x2DA6),
    (0x2DA8, 0x2DAE),
    (0x2DB0, 0x2DB6),
    (0x2DB8, 0x2DBE),
    (0x2DC0, 0x2DC6),
    (0x2DC8, 0x2DCE),
    (0x2DD0, 0x2DD6),
    (0x2DD8, 0x2DDE),
    (0x2DE0, 0x2E5D),
    (0x2E80, 0x2E99),
    (0x2E9B, 0x2EF3),
    (0x2F00, 0x2FD5),
    (0x2FF0, 0x2FFB),
    (0x3001, 0x303F),
    (0x3041, 0x3096),
    (0x3099, 0x30FF),
    (0x3105, 0x312F),
    (0x3131, 0x318E),
    (0x3190, 0x31E3),
    (0x31F0, 0x321E),
    (0x3220, 0xA48C),
    (0xA490, 0xA4C6),
    (0xA4D0, 0xA62B),
    (0xA640, 0xA6F7),
    (0xA700, 0xA7CA),
    (0xA7D0, 0xA7D1),
    (0xA7D3, 0xA7D3),
    (0xA7D5, 0xA7D9),
    (0xA7F2, 0xA82C),
    (0xA830, 0xA839),
    (0xA840, 0xA877),
    (0xA880, 0xA8C5),
    (0xA8CE, 0xA8D9),
    (0xA8E0, 0xA953),
    (0xA95F, 0xA97C),
    (0xA980, 0xA9CD),
    (0xA9CF, 0xA9D9),
    (0xA9DE, 0xA9FE),
    (0xAA00, 0xAA36),
    (0xAA40, 0xAA4D),
    (0xAA50, 0xAA59),
    (0xAA5C, 0xAAC2),
    (0xAADB, 0xAAF6),
    (0xAB01, 0xAB06),
    (0xAB09, 0xAB0E),
    (0xAB11, 0xAB16),
    (0xAB20, 0xAB26),
    (0xAB28, 0xAB2E),
    (0xAB30, 0xAB6B),
    (0xAB70, 0xABED),
    (0xABF0, 0xABF9),
    (0xAC00, 0xD7A3),
    (0xD7B0, 0xD7C6),
    (0xD7CB, 0xD7FB),
    (0xF900, 0xFA6D),
    (0xFA70, 0xFAD9),
    (0xFB00, 0xFB06),
    (0xFB13, 0xFB17),
    (0xFB1D, 0xFB36),
    (0xFB38, 0xFB3C),
    (0xFB3E, 0xFB3E),
    (0xFB40, 0xFB41),
    (0xFB43, 0xFB44),
    (0xFB46, 0xFBC2),
    (0xFBD3, 0xFD8F),
    (0xFD92, 0xFDC7),
    (0xFDCF, 0xFDCF),
    (0xFDF0, 0xFE19),
    (0xFE20, 0xFE52),
    (0xFE54, 0xFE66),
    (0xFE68, 0xFE6B),
    (0xFE70, 0xFE74),
    (0xFE76, 0xFEFC),
    (0xFF01, 0xFFBE),
    (0xFFC2, 0xFFC7),
    (0xFFCA, 0xFFCF),
    (0xFFD2, 0xFFD7),
    (0xFFDA, 0xFFDC),
    (0xFFE0, 0xFFE6),
    (0xFFE8, 0xFFEE),
    (0xFFFC, 0xFFFD),
    (0x10000, 0x1000B),
    (0x1000D, 0x10026),
    (0x10028, 0x1003A),
    (0x1003C, 0x1003D),
    (0x1003F, 0x1004D),
    (0x10050, 0x1005D),
    (0x10080, 0x100FA),
    (0x10100, 0x10102),
    (0x10107, 0x10133),
    (0x10137, 0x1018E),
    (0x10190, 0x1019C),
    (0x101A0, 0x101A0),
    (0x101D0, 0x101FD),
    (0x10280, 0x1029C),
    (0x102A0, 0x102D0),
    (0x102E0, 0x102FB),
    (0x10300, 0x10323),
    (0x1032D, 0x1034A),
    (0x10350, 0x1037A),
    (0x10380, 0x1039D),
    (0x1039F, 0x103C3),
    (0x103C8, 0x103D5),
    (0x10400, 0x1049D),
    (0x104A0, 0x104A9),
    (0x104B0, 0x104D3),
    (0x104D8, 0x104FB),
    (0x10500, 0x10527),
    (0x10530, 0x10563),
    (0x1056F, 0x1057A),
    (0x1057C, 0x1058A),
    (0x1058C, 0x10592),
    (0x10594, 0x10595),
    (0x10597, 0x105A1),
    (0x105A3, 0x105B1),
    (0x105B3, 0x105B9),
    (0x105BB, 0x105BC),
    (0x10600, 0x10736),
    (0x10740, 0x10755),
    (0x10760, 0x10767),
    (0x10780, 0x10785),
    (0x10787, 0x107B0),
    (0x107B2, 0x107BA),
    (0x10800, 0x10805),
    (0x10808, 0x10808),
    (0x1080A, 0x10835),
    (0x10837, 0x10838),
    (0x1083C, 0x1083C),
    (0x1083F, 0x10855),
    (0x10857, 0x1089E),
    (0x108A7, 0x108AF),
    (0x108E0, 0x108F2),
    (0x108F4, 0x108F5),
    (0x108FB, 0x1091B),
    (0x1091F, 0x10939),
    (0x1093F, 0x1093F),
    (0x10980, 0x109B7),
    (0x109BC, 0x109CF),
    (0x109D2, 0x10A03),
    (0x10A05, 0x10A06),
    (0x10A0C, 0x10A13),
    (0x10A15, 0x10A17),
    (0x10A19, 0x10A35),
    (0x10A38, 0x10A3A),
    (0x10A3F, 0x10A48),
    (0x10A50, 0x10A58),
    (0x10A60, 0x10A9F),
    (0x10AC0, 0x10AE6),
    (0x10AEB, 0x10AF6),
    (0x10B00, 0x10B35),
    (0x10B39, 0x10B55),
    (0x10B58, 0x10B72),
    (0x10B78, 0x10B91),
    (0x10B99, 0x10B9C),
    (0x10BA9, 0x10BAF),
    (0x10C00, 0x10C48),
    (0x10C80, 0x10CB2),
    (0x10CC0, 0x10CF2),
    (0x10CFA, 0x10D27),
    (0x10D30, 0x10D39),
    (0x10E60, 0x10E7E),
    (0x10E80, 0x10EA9),
    (0x10EAB, 0x10EAD),
    (0x10EB0, 0x10EB1),
    (0x10EFD, 0x10F27),
    (0x10F30, 0x10F59),
    (0x10F70, 0x10F89),
    (0x10FB0, 0x10FCB),
    (0x10FE0, 0x10FF6),
    (0x11000, 0x1104D),
    (0x11052, 0x11075),
    (0x1107F, 0x110BC),
    (0x110BE, 0x110C2),
    (0x110D0, 0x110E8),
    (0x110F0, 0x110F9),
    (0x11100, 0x11134),
    (0x11136, 0x11147),
    (0x11150, 0x11176),
    (0x11180, 0x111DF),
    (0x111E1, 0x111F4),
    (0x11200, 0x11211),
    (0x11213, 0x11241),
    (0x11280, 0x11286),
    (0x11288, 0x11288),
    (0x1128A, 0x1128D),
    (0x1128F, 0x1129D),
    (0x1129F, 0x112A9),
    (0x112B0, 0x112EA),
    (0x112F0, 0x112F9),
    (0x11300, 0x11303),
    (0x11305, 0x1130C),
    (0x1130F, 0x11310),
    (0x11313, 0x11328),
    (0x1132A, 0x11330),
    (0x11332, 0x11333),
    (0x11335, 0x11339),
    (0x1133B, 0x11344),
    (0x11347, 0x11348),
    (0x1134B, 0x1134D),
    (0x11350, 0x11350),
    (0x11357, 0x11357),
    (0x1135D, 0x11363),
    (0x11366, 0x1136C),
    (0x11370, 0x11374),
    (0x11400, 0x1145B),
    (0x1145D, 0x11461),
    (0x11480, 0x114C7),
    (0x114D0, 0x114D9),
    (0x11580, 0x115B5),
    (0x115B8, 0x115DD),
    (0x11600, 0x11644),
    (0x11650, 0x11659),
    (0x11660, 0x1166C),
    (0x11680, 0x116B9),
    (0x116C0, 0x116C9),
    (0x11700, 0x1171A),
    (0x1171D, 0x1172B),
    (0x11730, 0x11746),
    (0x11800, 0x1183B),
    (0x118A0, 0x118F2),
    (0x118FF, 0x11906),
    (0x11909, 0x11909),
    (0x1190C, 0x11913),
    (0x11915, 0x11916),
    (0x11918, 0x11935),
    (0x11937, 0x11938),
    (0x1193B, 0x11946),
    (0x11950, 0x11959),
    (0x119A0, 0x119A7),
    (0x119AA, 0x119D7),
    (0x119DA, 0x119E4),
    (0x11A00, 0x11A47),
    (0x11A50, 0x11AA2),
    (0x11AB0, 0x11AF8),
    (0x11B00, 0x11B09),
    (0x11C00, 0x11C08),
    (0x11C0A, 0x11C36),
    (0x11C38, 0x11C45),
    (0x11C50, 0x11C6C),
    (0x11C70, 0x11C8F),
    (0x11C92, 0x11CA7),
    (0x11CA9, 0x11CB6),
    (0x11D00, 0x11D06),
    (0x11D08, 0x11D09),
    (0x11D0B, 0x11D36),
    (0x11D3A, 0x11D3A),
    (0x11D3C, 0x11D3D),
    (0x11D3F, 0x11D47),
    (0x11D50, 0x11D59),
    (0x11D60, 0x11D65),
    (0x11D67, 0x11D68),
    (0x11D6A, 0x11D8E),
    (0x11D90, 0x11D91),
    (0x11D93, 0x11D98),
    (0x11DA0, 0x11DA9),
    (0x11EE0, 0x11EF8),
    (0x11F00, 0x11F10),
    (0x11F12, 0x11F3A),
    (0x11F3E, 0x11F59),
    (0x11FB0, 0x11FB0),
    (0x11FC0, 0x11FF1),
    (0x11FFF, 0x12399),
    (0x12400, 0x1246E),
    (0x12470, 0x12474),
    (0x12480, 0x12543),
    (0x12F90, 0x12FF2),
    (0x13000, 0x1342F),
    (0x13440, 0x13455),
    (0x14400, 0x14646),
    (0x16800, 0x16A38),
    (0x16A40, 0x16A5E),
    (0x16A60, 0x16A69),
    (0x16A6E, 0x16ABE),
    (0x16AC0, 0x16AC9),
    (0x16AD0, 0x16AED),
    (0x16AF0, 0x16AF5),
    (0x16B00, 0x16B45),
    (0x16B50, 0x16B59),
    (0x16B5B, 0x16B61),
    (0x16B63, 0x16B77),
    (0x16B7D, 0x16B8F),
    (0x16E40, 0x16E9A),
    (0x16F00, 0x16F4A),
    (0x16F4F, 0x16F87),
    (0x16F8F, 0x16F9F),
    (0x16FE0, 0x16FE4),
    (0x16FF0, 0x16FF1),
    (0x17000, 0x187F7),
    (0x18800, 0x18CD5),
    (0x18D00, 0x18D08),
    (0x1AFF0, 0x1AFF3),
    (0x1AFF5, 0x1AFFB),
    (0x1AFFD, 0x1AFFE),
    (0x1B000, 0x1B122),
    (0x1B132, 0x1B132),
    (0x1B150, 0x1B152),
    (0x1B155, 0x1B155),
    (0x1B164, 0x1B167),
    (0x1B170, 0x1B2FB),
    (0x1BC00, 0x1BC6A),
    (0x1BC70, 0x1BC7C),
    (0x1BC80, 0x1BC88),
    (0x1BC90, 0x1BC99),
    (0x1BC9C, 0x1BC9F),
    (0x1CF00, 0x1CF2D),
    (0x1CF30, 0x1CF46),
    (0x1CF50, 0x1CFC3),
    (0x1D000, 0x1D0F5),
    (0x1D100, 0x1D126),
    (0x1D129, 0x1D172),
    (0x1D17B, 0x1D1EA),
    (0x1D200, 0x1D245),
    (0x1D2C0, 0x1D2D3),
    (0x1D2E0, 0x1D2F3),
    (0x1D300, 0x1D356),
    (0x1D360, 0x1D378),
    (0x1D400, 0x1D454),
    (0x1D456, 0x1D49C),
    (0x1D49E, 0x1D49F),
    (0x1D4A2, 0x1D4A2),
    (0x1D4A5, 0x1D4A6),
    (0x1D4A9, 0x1D4AC),
    (0x1D4AE, 0x1D4B9),
    (0x1D4BB, 0x1D4BB),
    (0x1D4BD, 0x1D4C3),
    (0x1D4C5, 0x1D505),
    (0x1D507, 0x1D50A),
    (0x1D50D, 0x1D514),
    (0x1D516, 0x1D51C),
    (0x1D51E, 0x1D539),
    (0x1D53B, 0x1D53E),
    (0x1D540, 0x1D544),
    (0x1D546, 0x1D546),
    (0x1D54A, 0x1D550),
    (0x1D552, 0x1D6A5),
    (0x1D6A8, 0x1D7CB),
    (0x1D7CE, 0x1DA8B),
    (0x1DA9B, 0x1DA9F),
    (0x1DAA1, 0x1DAAF),
    (0x1DF00, 0x1DF1E),
    (0x1DF25, 0x1DF2A),
    (0x1E000, 0x1E006),
    (0x1E008, 0x1E018),
    (0x1E01B, 0x1E021),
    (0x1E023, 0x1E024),
    (0x1E026, 0x1E02A),
    (0x1E030, 0x1E06D),
    (0x1E08F, 0x1E08F),
    (0x1E100, 0x1E12C),
    (0x1E130, 0x1E13D),
    (0x1E140, 0x1E149),
    (0x1E14E, 0x1E14F),
    (0x1E290, 0x1E2AE),
    (0x1E2C0, 0x1E2F9),
    (0x1E2FF, 0x1E2FF),
    (0x1E4D0, 0x1E4F9),
    (0x1E7E0, 0x1E7E6),
    (0x1E7E8, 0x1E7EB),
    (0x1E7ED, 0x1E7EE),
    (0x1E7F0, 0x1E7FE),
    (0x1E800, 0x1E8C4),
    (0x1E8C7, 0x1E8D6),
    (0x1E900, 0x1E94B),
    (0x1E950, 0x1E959),
    (0x1E95E, 0x1E95F),
    (0x1EC71, 0x1ECB4),
    (0x1ED01, 0x1ED3D),
    (0x1EE00, 0x1EE03),
    (0x1EE05, 0x1EE1F),
    (0x1EE21, 0x1EE22),
    (0x1EE24, 0x1EE24),
    (0x1EE27, 0x1EE27),
    (0x1EE29, 0x1EE32),
    (0x1EE34, 0x1EE37),
    (0x1EE39, 0x1EE39),
    (0x1EE3B, 0x1EE3B),
    (0x1EE42, 0x1EE42),
    (0x1EE47, 0x1EE47),
    (0x1EE49, 0x1EE49),
    (0x1EE4B, 0x1EE4B),
    (0x1EE4D, 0x1EE4F),
    (0x1EE51, 0x1EE52),
    (0x1EE54, 0x1EE54),
    (0x1EE57, 0x1EE57),
    (0x1EE59, 0x1EE59),
    (0x1EE5B, 0x1EE5B),
    (0x1EE5D, 0x1EE5D),
    (0x1EE5F, 0x1EE5F),
    (0x1EE61, 0x1EE62),
    (0x1EE64, 0x1EE64),
    (0x1EE67, 0x1EE6A),
    (0x1EE6C, 0x1EE72),
    (0x1EE74, 0x1EE77),
    (0x1EE79, 0x1EE7C),
    (0x1EE7E, 0x1EE7E),
    (0x1EE80, 0x1EE89),
    (0x1EE8B, 0x1EE9B),
    (0x1EEA1, 0x1EEA3),
    (0x1EEA5, 0x1EEA9),
    (0x1EEAB, 0x1EEBB),
    (0x1EEF0, 0x1EEF1),
    (0x1F000, 0x1F02B),
    (0x1F030, 0x1F093),
    (0x1F0A0, 0x1F0AE),
    (0x1F0B1, 0x1F0BF),
    (0x1F0C1, 0x1F0CF),
    (0x1F0D1, 0x1F0F5),
    (0x1F100, 0x1F1AD),
    (0x1F1E6, 0x1F202),
    (0x1F210, 0x1F23B),
    (0x1F240, 0x1F248),
    (0x1F250, 0x1F251),
    (0x1F260, 0x1F265),
    (0x1F300, 0x1F6D7),
    (0x1F6DC, 0x1F6EC),
    (0x1F6F0, 0x1F6FC),
    (0x1F700, 0x1F776),
    (0x1F77B, 0x1F7D9),
    (0x1F7E0, 0x1F7EB),
    (0x1F7F0, 0x1F7F0),
    (0x1F800, 0x1F80B),
    (0x1F810, 0x1F847),
    (0x1F850, 0x1F859),
    (0x1F860, 0x1F887),
    (0x1F890, 0x1F8AD),
    (0x1F8B0, 0x1F8B1),
    (0x1F900, 0x1FA53),
    (0x1FA60, 0x1FA6D),
    (0x1FA70, 0x1FA7C),
    (0x1FA80, 0x1FA88),
    (0x1FA90, 0x1FABD),
    (0x1FABF, 0x1FAC5),
    (0x1FACE, 0x1FADB),
    (0x1FAE0, 0x1FAE8),
    (0x1FAF0, 0x1FAF8),
    (0x1FB00, 0x1FB92),
    (0x1FB94, 0x1FBCA),
    (0x1FBF0, 0x1FBF9),
    (0x20000, 0x2A6DF),
    (0x2A700, 0x2B739),
    (0x2B740, 0x2B81D),
    (0x2B820, 0x2CEA1),
    (0x2CEB0, 0x2EBE0),
    (0x2F800, 0x2FA1D),
    (0x30000, 0x3134A),
    (0x31350, 0x323AF),
    (0xE0100, 0xE01EF),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed pseudo-random sequence, so the cases are the same on every run.
    fn next(state: &mut u64) -> u64 {
        *state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *state >> 33
    }

    #[test]
    fn the_finder_agrees_with_a_plain_search() {
        let mut state = 7;
        for _ in 0..20000 {
            let hay: Vec<u8> = (0..next(&mut state) % 40)
                .map(|_| b'a' + (next(&mut state) % 2) as u8)
                .collect();
            let needle: Vec<u8> = (0..next(&mut state) % 14)
                .map(|_| b'a' + (next(&mut state) % 2) as u8)
                .collect();
            let from = (next(&mut state) as usize) % (hay.len() + 1);
            let want = (from..=hay.len()).find(|&i| hay[i..].starts_with(&needle));
            assert_eq!(Finder::new(&needle).find(&needle, &hay, from), want);
        }
    }

    #[test]
    fn counts_do_not_overlap() {
        assert_eq!(Finder::new(b"aa").count(b"aa", b"aaaaa"), 2);
        assert_eq!(Finder::new(b"").count(b"", "héllo".as_bytes()), 6);
        assert_eq!(
            Finder::new(b"abcabcabcab").count(b"abcabcabcab", b"abcabcabcabcabcabcab"),
            1
        );
    }

    #[test]
    fn runes_decode_as_the_standard_does() {
        for c in ('\0'..='\u{10FFFF}').step_by(7) {
            let mut buf = [0u8; 4];
            let bytes = c.encode_utf8(&mut buf).as_bytes();
            assert_eq!(decode_rune(bytes), (u32::from(c), bytes.len()));
            assert_eq!(decode_last_rune(bytes), (u32::from(c), bytes.len()));
        }
        for bad in [
            &b"\xC3"[..],
            b"\xE2\x82",
            b"\xED\xA0\x80",
            b"\xF4\x90\x80\x80",
            b"\xC0\x80",
            b"\x80",
            b"\xFF",
        ] {
            assert_eq!(decode_rune(bad), (RUNE_ERROR, 1), "{bad:x?}");
        }
        assert_eq!(decode_last_rune(b"a\xE2\x82"), (RUNE_ERROR, 1));
        assert_eq!(decode_last_rune("\u{20AC}".as_bytes()), (0x20AC, 3));
    }

    #[test]
    fn case_mapping_is_the_simple_one() {
        let up = |c: char| char::from_u32(to_case(UPPER, u32::from(c))).unwrap();
        let low = |c: char| char::from_u32(to_case(LOWER, u32::from(c))).unwrap();
        assert_eq!((low('İ'), low('Σ'), low('\u{1E9E}')), ('i', 'σ', 'ß'));
        assert_eq!(
            (up('ß'), up('ǆ'), up('ǅ'), up('ſ'), up('ı')),
            ('ß', 'Ǆ', 'Ǆ', 'S', 'I')
        );
        assert_eq!((low('ᾈ'), up('ᾀ')), ('ᾀ', 'ᾈ'));
    }
}

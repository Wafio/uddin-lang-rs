//! Conversion and println: int float char rune println.
//!
//! Numbers in text are read the way the Go library does: an integer with an optional sign and digits only, then a
//! float that may be `inf`, `infinity` or `nan` in any case, hex, or have underscores between digits.

use uddin_vm::{BuiltinDef, Exc, Rt, Str, Stringify, Value, float_to_int, gomath, type_name};

use crate::math_float::round_half_away;

pub(crate) const TABLE: &[BuiltinDef] = &[
    BuiltinDef::new("int", to_int, 1).fast_path(),
    BuiltinDef::new("float", to_float, 1).fast_path(),
    BuiltinDef::new("char", to_char, 1).fast_path(),
    BuiltinDef::new("rune", to_rune, 1).fast_path(),
    BuiltinDef::new("println", println, -1)
        .fast_path()
        .deep()
        .stringify(Stringify::Text),
];

/// `print`, which `println` is.
fn println(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let print = crate::basics::TABLE
        .iter()
        .find(|def| def.name == "print")
        .expect("print is registered");
    (print.func)(rt, args)
}

/// A float cut to its whole part, or null when no integer holds it: not a number, infinite, or past 64 bits. The
/// reference gives the most negative integer for all of those, which is plainly the wrong number for `1e19`.
fn truncated(f: f64) -> Value {
    if (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&f) {
        Value::Int(float_to_int(f))
    } else {
        Value::Null
    }
}

fn to_int(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let [arg] = args else {
        return Err(rt.arity_error("int", 1, args.len()));
    };
    match *arg {
        Value::Int(i) => Ok(Value::Int(i)),
        Value::Float(f) => Ok(truncated(f)),
        Value::Bool(b) => Ok(Value::Int(i64::from(b))),
        Value::Str(s) => {
            let text = rt.heap.str_bytes(s);
            if let Some(i) = parse_integer(text) {
                return Ok(Value::Int(i));
            }
            Ok(match parse_float(text) {
                Parsed::Value(f) => truncated(f),
                _ => Value::Null,
            })
        }
        _ => Err(rt.type_error("int() requires an int, int64, float64, bool, or a string")),
    }
}

/// A float rounded to a number of places, as `round` rounds, and the same number when it is whole.
fn to_float(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let [value, digits] = args else {
        return Err(rt.arity_error("float", 2, args.len()));
    };
    let f = match *value {
        Value::Float(f) => f,
        Value::Int(i) => i as f64,
        Value::Str(s) => match parse_float(rt.heap.str_bytes(s)) {
            Parsed::Value(f) | Parsed::Range(f) => f,
            Parsed::Syntax => 0.0,
        },
        _ => return Err(rt.type_error("float() requires an integer or a string")),
    };
    let Value::Int(digit) = *digits else {
        return Err(rt.type_error("float() requires an integer number of digits"));
    };
    if digit < 0 {
        let text = Exc::value_error(rt.pos, "float() digit must not be negative").text();
        return rt.error_value(text.as_bytes());
    }
    let scale = gomath::pow(10.0, digit as f64);
    let rounded = round_half_away(f * scale) / scale;
    // Adding zero turns a negative zero into a positive one, as the reference does for a whole number.
    Ok(Value::Float(if rounded == rounded.trunc() {
        rounded + 0.0
    } else {
        rounded
    }))
}

fn to_char(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let [arg] = args else {
        return Err(rt.arity_error("char", 1, args.len()));
    };
    let Value::Int(code) = *arg else {
        return Err(rt.type_error(format!(
            "char() requires an integer, not {}",
            type_name(*arg)
        )));
    };
    // A code point goes through a 32-bit rune: the bits above are dropped, and what is not a character is U+FFFD.
    let c = u32::try_from(code as i32)
        .ok()
        .and_then(char::from_u32)
        .unwrap_or('\u{FFFD}');
    let mut buf = [0u8; 4];
    let bytes = c.encode_utf8(&mut buf).as_bytes();
    let s = if let [b] = bytes {
        Str::byte(*b)
    } else {
        rt.heap.new_str(bytes).map_err(rt.oom())?
    };
    Ok(Value::Str(s))
}

fn to_rune(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let [arg] = args else {
        return Err(rt.arity_error("rune", 1, args.len()));
    };
    let Value::Str(s) = *arg else {
        return Err(rt.type_error("rune() requires a string"));
    };
    let bytes = rt.heap.str_bytes(s);
    // A bad byte is one rune, U+FFFD.
    let (first, width) = decode_rune(bytes);
    if bytes.is_empty() || width != bytes.len() {
        return Err(Exc::value_error(
            rt.pos,
            "rune() requires a 1-character string",
        ));
    }
    Ok(Value::Int(i64::from(first)))
}

fn decode_rune(s: &[u8]) -> (u32, usize) {
    let Some(&b0) = s.first() else {
        return (0xFFFD, 0);
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
        _ => return (0xFFFD, 1),
    };
    if s.len() < width || s[1] < lo || s[1] > hi {
        return (0xFFFD, 1);
    }
    if s[2..width].iter().any(|b| !(0x80..=0xBF).contains(b)) {
        return (0xFFFD, 1);
    }
    let mut r = u32::from(b0) & (0x7F >> width);
    for &b in &s[1..width] {
        r = r << 6 | u32::from(b & 0x3F);
    }
    (r, width)
}

/// An integer of decimal digits with an optional sign that fits in 64 bits.
fn parse_integer(s: &[u8]) -> Option<i64> {
    let (negative, digits) = match s.first()? {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, s),
    };
    if digits.is_empty() {
        return None;
    }
    let mut n: u64 = 0;
    for &c in digits {
        if !c.is_ascii_digit() {
            return None;
        }
        n = n.checked_mul(10)?.checked_add(u64::from(c - b'0'))?;
    }
    if negative {
        (n <= 1 << 63).then(|| (n as i64).wrapping_neg())
    } else {
        i64::try_from(n).ok()
    }
}

/// What reading a float from text gave.
#[derive(Debug, PartialEq)]
enum Parsed {
    Value(f64),
    /// The text is a number too large for a float; the value is an infinity with the sign of the text.
    Range(f64),
    Syntax,
}

fn lower(c: u8) -> u8 {
    c | 0x20
}

/// Whether the underscores in a number sit between digits, or between a base prefix and a digit.
fn underscores_ok(s: &[u8]) -> bool {
    let mut saw = b'^';
    let mut i = 0;
    let mut s = s;
    if matches!(s.first(), Some(b'-' | b'+')) {
        s = &s[1..];
    }
    let mut hex = false;
    if s.len() >= 2 && s[0] == b'0' && matches!(lower(s[1]), b'b' | b'o' | b'x') {
        i = 2;
        saw = b'0';
        hex = lower(s[1]) == b'x';
    }
    while i < s.len() {
        if s[i].is_ascii_digit() || (hex && (b'a'..=b'f').contains(&lower(s[i]))) {
            saw = b'0';
        } else if s[i] == b'_' {
            if saw != b'0' {
                return false;
            }
            saw = b'_';
        } else {
            if saw == b'_' {
                return false;
            }
            saw = b'!';
        }
        i += 1;
    }
    saw != b'_'
}

/// The special values: the value and the bytes read for a leading `inf`, `infinity` or `nan`, in any case.
fn special(s: &[u8]) -> Option<(f64, usize)> {
    let prefix = |s: &[u8], p: &[u8]| {
        s.iter()
            .zip(p)
            .take_while(|(a, b)| a.to_ascii_lowercase() == **b)
            .count()
    };
    let (sign, nsign, rest) = match s.first()? {
        b'+' => (1.0, 1, &s[1..]),
        b'-' => (-1.0, 1, &s[1..]),
        b'i' | b'I' => (1.0, 0, s),
        b'n' | b'N' => return (prefix(s, b"nan") == 3).then_some((f64::NAN, 3)),
        _ => return None,
    };
    let mut n = prefix(rest, b"infinity");
    if 3 < n && n < 8 {
        n = 3;
    }
    (n == 3 || n == 8).then_some((sign * f64::INFINITY, nsign + n))
}

struct Read {
    mantissa: u64,
    exp: i64,
    negative: bool,
    truncated: bool,
    hex: bool,
    len: usize,
}

/// The longest prefix of `s` that is a float, with what a hex float needs to be built; `None` when it is
/// malformed.
fn read_float(s: &[u8]) -> Option<Read> {
    let mut underscores = false;
    let mut i = 0;
    let mut negative = false;
    match s.first()? {
        b'+' => i += 1,
        b'-' => {
            i += 1;
            negative = true;
        }
        _ => {}
    }
    let (mut base, mut max_digits, mut exp_char, mut hex) = (10u64, 19, b'e', false);
    if i + 2 < s.len() && s[i] == b'0' && lower(s[i + 1]) == b'x' {
        (base, max_digits, exp_char, hex) = (16, 16, b'p', true);
        i += 2;
    }
    let (mut saw_dot, mut saw_digits, mut truncated) = (false, false, false);
    let (mut digits, mut mantissa_digits, mut dp): (i64, i64, i64) = (0, 0, 0);
    let mut mantissa: u64 = 0;
    while i < s.len() {
        let c = s[i];
        if c == b'_' {
            underscores = true;
        } else if c == b'.' {
            if saw_dot {
                break;
            }
            saw_dot = true;
            dp = digits;
        } else if c.is_ascii_digit() {
            saw_digits = true;
            if c == b'0' && digits == 0 {
                dp -= 1;
            } else {
                digits += 1;
                if mantissa_digits < max_digits {
                    mantissa = mantissa * base + u64::from(c - b'0');
                    mantissa_digits += 1;
                } else if c != b'0' {
                    truncated = true;
                }
            }
        } else if base == 16 && (b'a'..=b'f').contains(&lower(c)) {
            saw_digits = true;
            digits += 1;
            if mantissa_digits < max_digits {
                mantissa = mantissa * 16 + u64::from(lower(c) - b'a' + 10);
                mantissa_digits += 1;
            } else {
                truncated = true;
            }
        } else {
            break;
        }
        i += 1;
    }
    if !saw_digits {
        return None;
    }
    if !saw_dot {
        dp = digits;
    }
    if base == 16 {
        dp *= 4;
        mantissa_digits *= 4;
    }
    if i < s.len() && lower(s[i]) == exp_char {
        i += 1;
        let mut esign = 1;
        match s.get(i)? {
            b'+' => i += 1,
            b'-' => {
                i += 1;
                esign = -1;
            }
            _ => {}
        }
        if !s.get(i)?.is_ascii_digit() {
            return None;
        }
        let mut e: i64 = 0;
        while i < s.len() && (s[i].is_ascii_digit() || s[i] == b'_') {
            if s[i] == b'_' {
                underscores = true;
            } else if e < 10000 {
                e = e * 10 + i64::from(s[i] - b'0');
            }
            i += 1;
        }
        dp += e * esign;
    } else if base == 16 {
        return None;
    }
    let exp = if mantissa != 0 {
        dp - mantissa_digits
    } else {
        0
    };
    if underscores && !underscores_ok(&s[..i]) {
        return None;
    }
    Some(Read {
        mantissa,
        exp,
        negative,
        truncated,
        hex,
        len: i,
    })
}

/// A hex float from its mantissa and binary exponent, rounded to nearest even; `None` when it is too large.
fn hex_float(mut mantissa: u64, mut exp: i64, negative: bool, truncated: bool) -> Option<f64> {
    const MANT_BITS: u32 = 52;
    const EXP_BITS: u32 = 11;
    const BIAS: i64 = -1023;
    let max_exp = (1i64 << EXP_BITS) + BIAS - 2;
    let min_exp = BIAS + 1;
    exp += i64::from(MANT_BITS);
    while mantissa != 0 && mantissa >> (MANT_BITS + 2) == 0 {
        mantissa <<= 1;
        exp -= 1;
    }
    if truncated {
        mantissa |= 1;
    }
    while mantissa >> (1 + MANT_BITS + 2) != 0 {
        mantissa = mantissa >> 1 | mantissa & 1;
        exp += 1;
    }
    while mantissa > 1 && exp < min_exp - 2 {
        mantissa = mantissa >> 1 | mantissa & 1;
        exp += 1;
    }
    let mut round = mantissa & 3;
    mantissa >>= 2;
    round |= mantissa & 1;
    exp += 2;
    if round == 3 {
        mantissa += 1;
        if mantissa == 1 << (1 + MANT_BITS) {
            mantissa >>= 1;
            exp += 1;
        }
    }
    if mantissa >> MANT_BITS == 0 {
        exp = BIAS;
    }
    if exp > max_exp {
        return None;
    }
    let mut bits = mantissa & ((1 << MANT_BITS) - 1);
    bits |= (((exp - BIAS) & ((1 << EXP_BITS) - 1)) as u64) << MANT_BITS;
    if negative {
        bits |= 1 << 63;
    }
    Some(f64::from_bits(bits))
}

/// A float from text, as `ParseFloat` of the Go library reads it: all of the text, and rounded correctly.
fn parse_float(s: &[u8]) -> Parsed {
    if let Some((v, n)) = special(s) {
        return if n == s.len() {
            Parsed::Value(v)
        } else {
            Parsed::Syntax
        };
    }
    let Some(read) = read_float(s) else {
        return Parsed::Syntax;
    };
    if read.len != s.len() {
        return Parsed::Syntax;
    }
    let infinity = if read.negative {
        f64::NEG_INFINITY
    } else {
        f64::INFINITY
    };
    if read.hex {
        return match hex_float(read.mantissa, read.exp, read.negative, read.truncated) {
            Some(f) => Parsed::Value(f),
            None => Parsed::Range(infinity),
        };
    }
    // The standard library reads a decimal correctly rounded; the underscores were checked above.
    let text: String = s
        .iter()
        .filter(|&&c| c != b'_')
        .map(|&c| char::from(c))
        .collect();
    match text.parse::<f64>() {
        Ok(f) if f.is_finite() => Parsed::Value(f),
        Ok(_) => Parsed::Range(infinity),
        Err(_) => Parsed::Syntax,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn float_of(text: &str) -> Parsed {
        parse_float(text.as_bytes())
    }

    #[test]
    fn integers_are_digits_with_a_sign() {
        for (text, want) in [
            ("42", Some(42)),
            ("-42", Some(-42)),
            ("+7", Some(7)),
            ("0007", Some(7)),
            ("9223372036854775807", Some(i64::MAX)),
            ("-9223372036854775808", Some(i64::MIN)),
            ("9223372036854775808", None),
            ("-9223372036854775809", None),
            ("", None),
            ("-", None),
            ("1_000", None),
            ("0x10", None),
            (" 1", None),
            ("1 ", None),
            ("1.5", None),
        ] {
            assert_eq!(parse_integer(text.as_bytes()), want, "{text:?}");
        }
    }

    #[test]
    fn floats_follow_the_library() {
        use Parsed::{Range, Syntax, Value};
        for (text, want) in [
            ("1.5", Value(1.5)),
            ("1e3", Value(1000.0)),
            (".5", Value(0.5)),
            ("5.", Value(5.0)),
            ("1_000.5", Value(1000.5)),
            ("1_23.50_0_0e+1_2", Value(1.235e14)),
            ("0x1p-2", Value(0.25)),
            ("0x_1_2.3_4_5p+1_2", Value(74565.0)),
            ("inf", Value(f64::INFINITY)),
            ("-Infinity", Value(f64::NEG_INFINITY)),
            ("1e999", Range(f64::INFINITY)),
            ("-1e999", Range(f64::NEG_INFINITY)),
            ("0x1p5000", Range(f64::INFINITY)),
            ("1e-999", Value(0.0)),
            ("", Syntax),
            ("e5", Syntax),
            ("1e", Syntax),
            ("1__0", Syntax),
            ("_1", Syntax),
            ("1_", Syntax),
            ("0x1", Syntax),
            ("infinit", Syntax),
            ("1.5x", Syntax),
            (" 1", Syntax),
            ("++1", Syntax),
        ] {
            assert_eq!(float_of(text), want, "{text:?}");
        }
        assert!(matches!(float_of("nan"), Value(f) if f.is_nan()));
    }
}

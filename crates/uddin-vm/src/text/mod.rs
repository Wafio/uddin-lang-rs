//! Text and number formatting with the exact output the language promises: UTF-8 decoding that never fails,
//! quoted strings, float text, and the float functions whose last bit scripts can see.

mod print_table;

use std::sync::OnceLock;

pub const RUNE_ERROR: u32 = 0xFFFD;

/// Decodes the first rune of `s`. A bad or truncated sequence gives `(RUNE_ERROR, 1)`; an empty slice `(RUNE_ERROR, 0)`.
pub fn decode_rune(s: &[u8]) -> (u32, usize) {
    let n = s.len();
    if n == 0 {
        return (RUNE_ERROR, 0);
    }
    let b0 = s[0];
    if b0 < 0x80 {
        return (u32::from(b0), 1);
    }
    let (lo, hi, w) = match b0 {
        0xC2..=0xDF => (0x80, 0xBF, 2),
        0xE0 => (0xA0, 0xBF, 3),
        0xE1..=0xEC | 0xEE..=0xEF => (0x80, 0xBF, 3),
        0xED => (0x80, 0x9F, 3),
        0xF0 => (0x90, 0xBF, 4),
        0xF1..=0xF3 => (0x80, 0xBF, 4),
        0xF4 => (0x80, 0x8F, 4),
        _ => return (RUNE_ERROR, 1),
    };
    if n < 2 || s[1] < lo || s[1] > hi {
        return (RUNE_ERROR, 1);
    }
    if w == 2 {
        return ((u32::from(b0 & 0x1F) << 6) | u32::from(s[1] & 0x3F), 2);
    }
    if n < 3 || !(0x80..=0xBF).contains(&s[2]) {
        return (RUNE_ERROR, 1);
    }
    if w == 3 {
        return (
            (u32::from(b0 & 0x0F) << 12) | (u32::from(s[1] & 0x3F) << 6) | u32::from(s[2] & 0x3F),
            3,
        );
    }
    if n < 4 || !(0x80..=0xBF).contains(&s[3]) {
        return (RUNE_ERROR, 1);
    }
    (
        (u32::from(b0 & 0x07) << 18)
            | (u32::from(s[1] & 0x3F) << 12)
            | (u32::from(s[2] & 0x3F) << 6)
            | u32::from(s[3] & 0x3F),
        4,
    )
}

/// True for a byte that starts a rune (not a continuation byte).
pub fn is_rune_start(b: u8) -> bool {
    b & 0xC0 != 0x80
}

pub fn push_rune(out: &mut Vec<u8>, r: u32) {
    let c = char::from_u32(r).unwrap_or('\u{FFFD}');
    let mut buf = [0u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
}

/// A bit per code point of the basic plane, built once from the ranges: text full of non-ASCII letters is quoted
/// rune by rune, and a search of the ranges for each would make a megabyte of it slow.
fn basic_plane() -> &'static [u64; 1024] {
    static BITS: OnceLock<Box<[u64; 1024]>> = OnceLock::new();
    BITS.get_or_init(|| {
        let mut bits = Box::new([0u64; 1024]);
        for &(lo, hi) in print_table::IS_PRINT
            .iter()
            .take_while(|&&(lo, _)| lo < 0x10000)
        {
            for r in lo..=hi.min(0xFFFF) {
                bits[(r >> 6) as usize] |= 1 << (r & 63);
            }
        }
        bits
    })
}

pub fn is_print(r: u32) -> bool {
    if r < 0x10000 {
        return basic_plane()[(r >> 6) as usize] >> (r & 63) & 1 == 1;
    }
    let t = &print_table::IS_PRINT;
    let i = t.partition_point(|&(_, hi)| hi < r);
    i < t.len() && t[i].0 <= r
}

const HEX: &[u8; 16] = b"0123456789abcdef";

/// Appends `s` as a double-quoted literal: `\n`-style escapes, `\xNN` for a byte that is not UTF-8, `\uNNNN`
/// and `\UNNNNNNNN` for runes that do not print.
pub fn quote(s: &[u8], out: &mut Vec<u8>) {
    out.push(b'"');
    let mut i = 0;
    while i < s.len() {
        let (r, w) = if s[i] < 0x80 {
            (u32::from(s[i]), 1)
        } else {
            decode_rune(&s[i..])
        };
        if w == 1 && r == RUNE_ERROR {
            out.extend_from_slice(&[
                b'\\',
                b'x',
                HEX[usize::from(s[i] >> 4)],
                HEX[usize::from(s[i] & 15)],
            ]);
        } else {
            quote_rune(out, r);
        }
        i += w;
    }
    out.push(b'"');
}

fn quote_rune(out: &mut Vec<u8>, r: u32) {
    if r == u32::from(b'"') || r == u32::from(b'\\') {
        out.push(b'\\');
        out.push(r as u8);
        return;
    }
    if is_print(r) {
        push_rune(out, r);
        return;
    }
    match r {
        0x07 => out.extend_from_slice(b"\\a"),
        0x08 => out.extend_from_slice(b"\\b"),
        0x0C => out.extend_from_slice(b"\\f"),
        0x0A => out.extend_from_slice(b"\\n"),
        0x0D => out.extend_from_slice(b"\\r"),
        0x09 => out.extend_from_slice(b"\\t"),
        0x0B => out.extend_from_slice(b"\\v"),
        _ if r < 0x20 || r == 0x7F => {
            out.extend_from_slice(&[b'\\', b'x', HEX[(r >> 4) as usize], HEX[(r & 15) as usize]]);
        }
        _ if r < 0x10000 => {
            out.extend_from_slice(b"\\u");
            for sh in [12, 8, 4, 0] {
                out.push(HEX[((r >> sh) & 15) as usize]);
            }
        }
        _ => {
            out.extend_from_slice(b"\\U");
            for sh in [28, 24, 20, 16, 12, 8, 4, 0] {
                out.push(HEX[((r >> sh) & 15) as usize]);
            }
        }
    }
}

/// A script value quoted into an error message, cut to 128 bytes on a rune boundary.
pub fn quote_for_error(s: &[u8], out: &mut Vec<u8>) {
    const MAX: usize = 128;
    if s.len() <= MAX {
        quote(s, out);
        return;
    }
    let mut cut = MAX;
    while cut > 0 && !is_rune_start(s[cut]) {
        cut -= 1;
    }
    quote(&s[..cut], out);
    out.extend_from_slice(format!("... ({} bytes)", s.len()).as_bytes());
}

/// Length of the longest prefix of `msg` that is at most `max` bytes and ends on a rune boundary, plus whether
/// anything was cut.
pub fn truncate_len(msg: &[u8], max: usize) -> (usize, bool) {
    if msg.len() <= max {
        return (msg.len(), false);
    }
    let mut cut = max;
    while cut > 0 && !is_rune_start(msg[cut]) {
        cut -= 1;
    }
    (cut, true)
}

/// Upper bound of `s` once quoted: `json` counts the way a JSON encoder escapes, otherwise the way `quote` does.
pub fn quoted_size(s: &[u8], json: bool) -> usize {
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
        n += if (r == RUNE_ERROR && w == 1) || (json && (r == 0x2028 || r == 0x2029)) {
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

pub fn format_int(i: i64, out: &mut Vec<u8>) {
    let mut buf = [0u8; 20];
    let mut p = buf.len();
    let mut n = i.unsigned_abs();
    loop {
        p -= 1;
        buf[p] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    if i < 0 {
        out.push(b'-');
    }
    out.extend_from_slice(&buf[p..]);
}

/// Shortest decimal digits of a finite, non-zero magnitude and the decimal point position (`0.d1d2.. * 10^dp`).
fn shortest_digits(f: f64) -> (Vec<u8>, i32) {
    let abs = f.abs();
    let (digits, dp) = parse_exp_form(&format!("{abs:e}"));
    if digits.len() >= 15 {
        // Two texts of that length can be as close to the float, when it is halfway between them: the shortest
        // text takes the larger one, the reference the one with the even last digit, which is what rounding the
        // exact value to that many digits gives.
        let exact = format!("{abs:.*e}", digits.len() - 1);
        if exact.parse::<f64>().is_ok_and(|g| g == abs) {
            return parse_exp_form(&exact);
        }
    }
    (digits, dp)
}

fn parse_exp_form(text: &str) -> (Vec<u8>, i32) {
    let (mant, exp) = text.split_once('e').unwrap_or((text, "0"));
    let digits: Vec<u8> = mant.bytes().filter(u8::is_ascii_digit).collect();
    (digits, exp.parse::<i32>().unwrap_or(0) + 1)
}

fn push_e(out: &mut Vec<u8>, digits: &[u8], dp: i32, prec: usize) {
    out.push(digits.first().copied().unwrap_or(b'0'));
    if prec > 0 {
        out.push(b'.');
        let m = digits.len().min(prec + 1);
        if m > 1 {
            out.extend_from_slice(&digits[1..m]);
        }
        for _ in m.max(1)..=prec {
            out.push(b'0');
        }
    }
    out.push(b'e');
    let exp = if digits.is_empty() { 0 } else { dp - 1 };
    out.push(if exp < 0 { b'-' } else { b'+' });
    let exp = exp.unsigned_abs();
    if exp < 10 {
        out.push(b'0');
    }
    format_int(i64::from(exp), out);
}

fn push_f(out: &mut Vec<u8>, digits: &[u8], dp: i32, prec: usize) {
    if dp > 0 {
        let m = digits.len().min(dp as usize);
        out.extend_from_slice(&digits[..m]);
        for _ in m..dp as usize {
            out.push(b'0');
        }
    } else {
        out.push(b'0');
    }
    if prec > 0 {
        out.push(b'.');
        for i in 0..prec as i32 {
            let j = dp + i;
            out.push(if j >= 0 && (j as usize) < digits.len() {
                digits[j as usize]
            } else {
                b'0'
            });
        }
    }
}

fn special_float(f: f64, out: &mut Vec<u8>) -> bool {
    if f.is_nan() {
        out.extend_from_slice(b"NaN");
    } else if f.is_infinite() {
        out.extend_from_slice(if f > 0.0 { b"+Inf" } else { b"-Inf" });
    } else {
        return false;
    }
    true
}

fn push_g(out: &mut Vec<u8>, digits: &[u8], dp: i32, prec: usize, shortest: bool) {
    let nd = digits.len() as i32;
    let mut eprec = prec as i32;
    if eprec > nd && nd >= dp {
        eprec = nd;
    }
    if shortest {
        eprec = 6;
    }
    let exp = dp - 1;
    if exp < -4 || exp >= eprec {
        let p = if prec as i32 > nd { nd as usize } else { prec };
        push_e(out, digits, dp, p.saturating_sub(1));
    } else {
        let p = if prec as i32 > dp { nd } else { prec as i32 };
        push_f(out, digits, dp, (p - dp).max(0) as usize);
    }
}

/// Shortest text that reads back as the same float, in `%g` form: an exponent below -4 or at 6 and up is
/// written as `1e+06`.
pub fn format_float_g(f: f64, out: &mut Vec<u8>) {
    if special_float(f, out) {
        return;
    }
    if f.is_sign_negative() {
        out.push(b'-');
    }
    if f == 0.0 {
        out.push(b'0');
        return;
    }
    let (digits, dp) = shortest_digits(f);
    push_g(out, &digits, dp, digits.len(), true);
}

/// A float as the JSON encoder of the reference writes it: shortest digits in the fixed form from `1e-6` up to
/// `1e21`, and in the exponent form (`1e-07`, `1e+21`) outside. Not for NaN or infinity.
pub fn format_float_json(f: f64, out: &mut Vec<u8>) {
    if f.is_sign_negative() {
        out.push(b'-');
    }
    if f == 0.0 {
        out.push(b'0');
        return;
    }
    let (digits, dp) = shortest_digits(f);
    let abs = f.abs();
    if !(1e-6..1e21).contains(&abs) {
        push_e(out, &digits, dp, digits.len() - 1);
    } else {
        push_f(out, &digits, dp, (digits.len() as i32 - dp).max(0) as usize);
    }
}

/// `%.6g`: six significant digits, trailing zeros dropped.
pub fn format_float_g6(f: f64, out: &mut Vec<u8>) {
    if special_float(f, out) {
        return;
    }
    if f.is_sign_negative() {
        out.push(b'-');
    }
    if f == 0.0 {
        out.push(b'0');
        return;
    }
    let (mut digits, dp) = parse_exp_form(&format!("{:.5e}", f.abs()));
    while digits.last() == Some(&b'0') {
        digits.pop();
    }
    push_g(out, &digits, dp, 6, false);
}

/// Float to int the way the reference platform converts: anything out of range, NaN included, is the minimum.
pub fn float_to_int(f: f64) -> i64 {
    const LIMIT: f64 = 9.223_372_036_854_776e18;
    if f.is_nan() || !(-LIMIT..LIMIT).contains(&f) {
        i64::MIN
    } else {
        f as i64
    }
}

#[cfg(test)]
mod tests;

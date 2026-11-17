//! Durations: the text `2h45m`, `-1.5h` or `300ms` as a count of nanoseconds, with the units and the limits of the
//! reference. The error is the reference's text.

use super::layout::quote;

/// The nanoseconds in a unit of `text`, if it is one.
fn unit(text: &[u8]) -> Option<u64> {
    if text == "\u{b5}s".as_bytes() || text == "\u{3bc}s".as_bytes() {
        return Some(1_000);
    }
    Some(match text {
        b"ns" => 1,
        b"us" => 1_000,
        b"ms" => 1_000_000,
        b"s" => 1_000_000_000,
        b"m" => 60_000_000_000,
        b"h" => 3_600_000_000_000,
        _ => return None,
    })
}

/// `[0-9]*` at the start of `s` and what follows it; `None` when the number is past 2^63.
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

/// The digits of a fraction as a number and the power of ten they stand for; digits past what a count can hold
/// are read and dropped.
fn leading_fraction(s: &[u8]) -> (u64, f64, &[u8]) {
    let (mut x, mut scale, mut overflow) = (0u64, 1.0f64, false);
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        if !overflow {
            if x > (u64::MAX >> 1) / 10 {
                overflow = true;
            } else {
                let y = x * 10 + u64::from(s[i] - b'0');
                if y > 1u64 << 63 {
                    overflow = true;
                } else {
                    x = y;
                    scale *= 10.0;
                }
            }
        }
        i += 1;
    }
    (x, scale, &s[i..])
}

/// The duration `text` names, in nanoseconds, or the reference's message for it.
pub(super) fn parse(text: &[u8]) -> Result<i64, Vec<u8>> {
    let invalid = |why: &[u8]| {
        let mut msg = b"time: ".to_vec();
        msg.extend_from_slice(why);
        msg.push(b' ');
        msg.extend(quote(text));
        msg
    };
    let mut s = text;
    let mut neg = false;
    if let Some(&c @ (b'-' | b'+')) = s.first() {
        neg = c == b'-';
        s = &s[1..];
    }
    if s == b"0" {
        return Ok(0);
    }
    if s.is_empty() {
        return Err(invalid(b"invalid duration"));
    }
    let mut d: u64 = 0;
    while !s.is_empty() {
        if !(s[0] == b'.' || s[0].is_ascii_digit()) {
            return Err(invalid(b"invalid duration"));
        }
        let before = s.len();
        let Some((mut v, rest)) = leading_int(s) else {
            return Err(invalid(b"invalid duration"));
        };
        s = rest;
        let pre = before != s.len();
        let (mut f, mut scale, mut post) = (0u64, 1.0f64, false);
        if s.first() == Some(&b'.') {
            s = &s[1..];
            let before = s.len();
            (f, scale, s) = leading_fraction(s);
            post = before != s.len();
        }
        if !pre && !post {
            return Err(invalid(b"invalid duration"));
        }
        let end = s
            .iter()
            .position(|&c| c == b'.' || c.is_ascii_digit())
            .unwrap_or(s.len());
        if end == 0 {
            return Err(invalid(b"missing unit in duration"));
        }
        let (name, rest) = s.split_at(end);
        s = rest;
        let Some(unit) = unit(name) else {
            let mut why = b"unknown unit ".to_vec();
            why.extend(quote(name));
            why.extend_from_slice(b" in duration");
            return Err(invalid(&why));
        };
        if v > (1u64 << 63) / unit {
            return Err(invalid(b"invalid duration"));
        }
        v *= unit;
        if f > 0 {
            v += (f as f64 * (unit as f64 / scale)) as u64;
            if v > 1u64 << 63 {
                return Err(invalid(b"invalid duration"));
            }
        }
        d += v;
        if d > 1u64 << 63 {
            return Err(invalid(b"invalid duration"));
        }
    }
    if neg {
        return Ok((d as i64).wrapping_neg());
    }
    if d > (1u64 << 63) - 1 {
        return Err(invalid(b"invalid duration"));
    }
    Ok(d as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(r: Result<i64, Vec<u8>>) -> String {
        match r {
            Ok(n) => n.to_string(),
            Err(e) => String::from_utf8_lossy(&e).into_owned(),
        }
    }

    #[test]
    fn reads_what_the_reference_reads() {
        let cases = [
            ("24h", "86400000000000"),
            ("2h45m", "9900000000000"),
            ("1h30m", "5400000000000"),
            ("-1.5h", "-5400000000000"),
            ("+300ms", "300000000"),
            ("1.5us", "1500"),
            ("1\u{b5}s", "1000"),
            ("0", "0"),
            ("-0", "0"),
            (".5s", "500000000"),
            ("5.s", "5000000000"),
            ("1s1s", "2000000000"),
            ("9223372036854775807ns", "9223372036854775807"),
            ("-9223372036854775808ns", "-9223372036854775808"),
        ];
        for (src, want) in cases {
            assert_eq!(text(parse(src.as_bytes())), want, "{src}");
        }
    }

    #[test]
    fn words_what_it_refuses_as_the_reference_does() {
        let cases = [
            ("", "time: invalid duration \"\""),
            ("-", "time: invalid duration \"-\""),
            (".s", "time: invalid duration \".s\""),
            ("5", "time: missing unit in duration \"5\""),
            ("5x", "time: unknown unit \"x\" in duration \"5x\""),
            ("1d", "time: unknown unit \"d\" in duration \"1d\""),
            ("h", "time: invalid duration \"h\""),
            (
                "9223372036854775808ns",
                "time: invalid duration \"9223372036854775808ns\"",
            ),
            (
                "9223372036854775807h",
                "time: invalid duration \"9223372036854775807h\"",
            ),
            (
                "1\u{e9}",
                "time: unknown unit \"\\xc3\\xa9\" in duration \"1\\xc3\\xa9\"",
            ),
        ];
        for (src, want) in cases {
            assert_eq!(text(parse(src.as_bytes())), want, "{src}");
        }
    }
}

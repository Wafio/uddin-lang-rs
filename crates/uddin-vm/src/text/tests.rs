use super::*;
use crate::gomath::pow;

fn g(f: f64) -> String {
    let mut out = Vec::new();
    format_float_g(f, &mut out);
    String::from_utf8(out).expect("ascii")
}

fn g6(f: f64) -> String {
    let mut out = Vec::new();
    format_float_g6(f, &mut out);
    String::from_utf8(out).expect("ascii")
}

fn q(s: &[u8]) -> String {
    let mut out = Vec::new();
    quote(s, &mut out);
    String::from_utf8(out).expect("quote output is utf-8")
}

#[test]
fn shortest_float_text() {
    for (f, want) in [
        (1.0, "1"),
        (0.1, "0.1"),
        (100000.0, "100000"),
        (1e6, "1e+06"),
        (123456789.0, "1.23456789e+08"),
        (1e-4, "0.0001"),
        (1e-5, "1e-05"),
        (-0.0, "-0"),
        (0.0, "0"),
        (std::f64::consts::PI, "3.141592653589793"),
        (1e21, "1e+21"),
        (1_435_027_691_564_067.0 + 0.25, "1.4350276915640672e+15"),
        (1_435_027_691_564_067.0 + 0.75, "1.4350276915640678e+15"),
        (1_435_027_691_564_068.0 + 0.25, "1.4350276915640682e+15"),
        (1.5e300, "1.5e+300"),
        (5e-324, "5e-324"),
        (f64::NAN, "NaN"),
        (f64::INFINITY, "+Inf"),
        (f64::NEG_INFINITY, "-Inf"),
        (0.30000000000000004, "0.30000000000000004"),
        (123456.7, "123456.7"),
        (-2.5, "-2.5"),
    ] {
        assert_eq!(g(f), want, "{f:e}");
    }
}

#[test]
fn six_digit_float_text() {
    for (f, want) in [
        (7.12345678, "7.12346"),
        (1234567.5, "1.23457e+06"),
        (0.5, "0.5"),
        (123456.7, "123457"),
        (0.000012345678, "1.23457e-05"),
        (2.5, "2.5"),
        (-1.25, "-1.25"),
        (99999.95, "99999.9"),
        (999999.5, "1e+06"),
    ] {
        assert_eq!(g6(f), want, "{f:e}");
    }
}

#[test]
fn quoting() {
    assert_eq!(q(b"a\"b\\c\n\t"), r#""a\"b\\c\n\t""#);
    assert_eq!(q("héllo".as_bytes()), "\"héllo\"");
    assert_eq!(q(b"\x00\x7f"), r#""\x00\x7f""#);
    assert_eq!(q(b"\xff"), r#""\xff""#);
    assert_eq!(q("\u{200b}".as_bytes()), format!("\"\\{}200b\"", 'u'));
    assert_eq!(q("\u{1F600}".as_bytes()), "\"\u{1F600}\"");
    assert_eq!(q("\u{E0001}".as_bytes()), r#""\U000e0001""#);
}

#[test]
fn error_quote_cuts_long_strings_on_a_rune_boundary() {
    let mut out = Vec::new();
    quote_for_error("é".repeat(100).as_bytes(), &mut out);
    let text = String::from_utf8(out).expect("utf-8");
    assert!(text.ends_with("... (200 bytes)"), "{text}");
    assert_eq!(text.matches('é').count(), 64);
}

#[test]
fn decoding() {
    assert_eq!(decode_rune(b"a"), (97, 1));
    assert_eq!(decode_rune("é".as_bytes()), (0xE9, 2));
    assert_eq!(decode_rune(&[0xC3]), (RUNE_ERROR, 1));
    assert_eq!(decode_rune(&[0xED, 0xA0, 0x80]), (RUNE_ERROR, 1));
    assert_eq!(decode_rune(&[]), (RUNE_ERROR, 0));
}

#[test]
fn integer_text() {
    let mut out = Vec::new();
    format_int(i64::MIN, &mut out);
    assert_eq!(out, b"-9223372036854775808");
    out.clear();
    format_int(0, &mut out);
    assert_eq!(out, b"0");
}

#[test]
fn float_to_int_out_of_range_is_the_minimum() {
    assert_eq!(float_to_int(f64::NAN), i64::MIN);
    assert_eq!(float_to_int(1e30), i64::MIN);
    assert_eq!(float_to_int(-3.9), -3);
    assert_eq!(float_to_int(2.9), 2);
}

#[test]
fn power() {
    assert_eq!(pow(2.0, 10.0), 1024.0);
    assert_eq!(pow(2.0, -2.0), 0.25);
    assert_eq!(pow(2.0, 0.5), std::f64::consts::SQRT_2);
    assert_eq!(pow(-8.0, 3.0), -512.0);
    assert!(pow(-8.0, 0.5).is_nan());
    assert_eq!(pow(0.0, -1.0), f64::INFINITY);
    assert!((pow(10.0, 308.0) / 1e308 - 1.0).abs() < 1e-14);
    assert_eq!(pow(10.0, 309.0), f64::INFINITY);
    assert_eq!(pow(2.0, -1074.0), 5e-324);
    assert_eq!(pow(2.0, -1075.0), 0.0);
    assert!((pow(2.0, 0.3) - 2f64.powf(0.3)).abs() < 1e-15);
    assert!((pow(10.0, 2.5) - 316.22776601683796).abs() < 1e-12);
}

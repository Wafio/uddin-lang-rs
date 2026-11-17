//! Generators aimed at the places where two implementations are likely to differ: the nesting limit, the size
//! limit, the finer points of numbers, strings, comments and characters, and where a refused builtin name can
//! sit in a program.

use crate::grammar;
use crate::mutate::INTERESTING;
use crate::names::{NEAR_KEYWORDS, Names, ORDINARY};
use crate::rng::Rng;

struct Layer {
    open: &'static str,
    close: &'static str,
    /// A rough number of nesting levels one use of the layer costs; only used to aim near the limit.
    cost: usize,
    statement: bool,
}

const fn statement_layer(open: &'static str, close: &'static str, cost: usize) -> Layer {
    Layer {
        open,
        close,
        cost,
        statement: true,
    }
}

const fn expression_layer(open: &'static str, close: &'static str, cost: usize) -> Layer {
    Layer {
        open,
        close,
        cost,
        statement: false,
    }
}

/// Constructs that hold a statement.
const STATEMENT_LAYERS: &[Layer] = &[
    statement_layer("if (x) then: ", " end ", 3),
    statement_layer("if (x) then { ", " } ", 3),
    statement_layer("while (x): ", " end ", 2),
    statement_layer("while (x) { ", " } ", 2),
    statement_layer("for (i in x): ", " end ", 2),
    statement_layer("fun f(): ", " end ", 2),
    statement_layer("memo fun f(a, b...): ", " end ", 2),
    statement_layer("try: ", " catch (e): end ", 1),
    statement_layer("try { ", " } catch (e) { } ", 1),
    statement_layer("try: x = 1 catch (e): ", " end ", 2),
    statement_layer("if (x) then: x = 1 else: ", " end ", 3),
    statement_layer("if (x) then { x = 1 } else { ", " } ", 3),
];

/// Constructs that hold an expression.
const EXPRESSION_LAYERS: &[Layer] = &[
    expression_layer("(", ")", 1),
    expression_layer("[", "]", 1),
    expression_layer("[1, ", ", 2]", 1),
    expression_layer("{a: ", "}", 1),
    expression_layer("{\"a\": ", ", b: 2}", 1),
    expression_layer("f(", ")", 1),
    expression_layer("f(1, ", ", 2)", 1),
    expression_layer("x[", "]", 1),
    expression_layer("- ", "", 1),
    expression_layer("not ", "", 1),
    expression_layer("1 ? ", " : 2", 1),
    expression_layer("1 ? 2 : ", "", 1),
    expression_layer("fun(): return ", " end", 3),
    expression_layer("fun() { return ", " }", 3),
    expression_layer("2 ** ", "", 1),
    expression_layer("1 + ", "", 1),
];

fn shuffle<T>(rng: &mut Rng, items: &mut [T]) {
    for i in (1..items.len()).rev() {
        items.swap(i, rng.below(i + 1));
    }
}

/// Where the nesting limit is: the number of levels is drawn around 1,000 most of the time.
fn depth_target(rng: &mut Rng) -> usize {
    match rng.weighted(&[70, 15, 10, 5]) {
        0 => rng.range(980, 1020),
        1 => rng.range(1, 1100),
        2 => rng.range(1000, 4000),
        _ => rng.range(4000, 20000),
    }
}

/// Statements and expressions nested to about a given depth, from one to four kinds of construct.
pub fn nesting(rng: &mut Rng) -> String {
    let target = depth_target(rng);
    let mut chosen: Vec<&Layer> = Vec::new();
    for _ in 0..rng.range(1, 4) {
        let pool = if rng.chance(40) {
            STATEMENT_LAYERS
        } else {
            EXPRESSION_LAYERS
        };
        chosen.push(rng.pick(pool));
    }
    let weights: Vec<usize> = chosen.iter().map(|_| rng.range(1, 10)).collect();
    let total: usize = weights.iter().sum();
    let mut stack: Vec<&Layer> = Vec::new();
    for (l, w) in chosen.iter().zip(&weights) {
        let count = (target * w / total / l.cost).max(1) + rng.below(3);
        stack.extend(std::iter::repeat_n(*l, count));
    }
    let (mut outer, mut inner): (Vec<&Layer>, Vec<&Layer>) =
        stack.into_iter().partition(|l| l.statement);
    shuffle(rng, &mut outer);
    shuffle(rng, &mut inner);
    let mut src = String::new();
    for l in &outer {
        src.push_str(l.open);
    }
    src.push_str("x = ");
    for l in &inner {
        src.push_str(l.open);
    }
    src.push('1');
    for l in inner.iter().rev() {
        src.push_str(l.close);
    }
    src.push(' ');
    for l in outer.iter().rev() {
        src.push_str(l.close);
    }
    src
}

/// One statement construct and one expression construct, repeated so that the depth comes out within a few
/// levels of the limit, on either side of it.
pub fn boundary(rng: &mut Rng) -> String {
    const LIMIT: usize = 1000;
    let statement = rng.pick(STATEMENT_LAYERS);
    let expression = rng.pick(EXPRESSION_LAYERS);
    let outer_count = if rng.chance(50) {
        0
    } else {
        rng.range(0, LIMIT / statement.cost)
    };
    // The cost estimates leave out the statement and expression around the layers, a few levels in all.
    let target = LIMIT - 8 + rng.below(12);
    let inner_count = target.saturating_sub(outer_count * statement.cost) / expression.cost;
    let mut src = statement.open.repeat(outer_count);
    src.push_str("x = ");
    src.push_str(&expression.open.repeat(inner_count));
    src.push('1');
    src.push_str(&expression.close.repeat(inner_count));
    src.push(' ');
    src.push_str(&statement.close.repeat(outer_count));
    src
}

/// A long chain of one kind: operators, calls, subscripts, members, unary signs, or `elif` branches.
pub fn chain(rng: &mut Rng) -> String {
    let n = match rng.weighted(&[70, 20, 10]) {
        0 => rng.range(990, 1010),
        1 => rng.range(1, 1100),
        _ => rng.range(1000, 12000),
    };
    let mut src = String::new();
    match rng.below(9) {
        0 => {
            const OPS: &[&[&str]] = &[
                &["+", "-"],
                &["*", "/", "%"],
                &["==", "!="],
                &["<", "<=", ">", ">=", "in"],
                &["or"],
                &["and"],
                &["xor"],
                &["**"],
                &["+", "*", "or", "==", "<", "**", "and", "xor", "-"],
            ];
            let ops = *rng.pick(OPS);
            src.push_str("x = 1");
            for _ in 0..n {
                src.push(' ');
                src.push_str(rng.pick(ops));
                src.push_str(" 1");
            }
        }
        1 => {
            src.push('f');
            for _ in 0..n {
                src.push_str("()");
            }
        }
        2 => {
            src.push('x');
            for _ in 0..n {
                src.push_str("[0]");
            }
        }
        3 => {
            src.push('x');
            for _ in 0..n {
                src.push_str(".a");
            }
        }
        4 => {
            src.push_str("x = ");
            let unit = *rng.pick(&["-", "not ", "- not ", "-- ", "not - "]);
            for _ in 0..n {
                src.push_str(unit);
            }
            src.push('1');
        }
        5 => {
            src.push('x');
            for _ in 0..n {
                src.push_str(rng.pick(&["()", "[0]", ".a", "(1)", "[x]"]));
            }
        }
        6 => {
            src.push_str("if (x) then: a = 1");
            for _ in 0..n {
                src.push_str(rng.pick(&[" elif (x) then: a = 1", " else if (x) then: a = 1"]));
            }
            src.push_str(" end");
        }
        7 => {
            src.push_str("x = a");
            for _ in 0..n {
                src.push_str(" ? b : a");
            }
        }
        _ => {
            // Many statements side by side: they do not nest.
            for _ in 0..n {
                src.push_str("x = 1\n");
            }
        }
    }
    src
}

/// A program padded or cut to a length around the size limit; the padding is a comment, white space, a long
/// string, a long name, or bytes that are not valid.
pub fn sized(rng: &mut Rng, names: &Names) -> Vec<u8> {
    const LIMIT: usize = 64 * 1024;
    let target = match rng.below(3) {
        0 => LIMIT + rng.range(0, 3) - 1,
        1 => LIMIT - 40 + rng.below(80),
        _ => *rng.pick(&[LIMIT - 1, LIMIT, LIMIT + 1]),
    };
    let mut src = grammar::program(rng, names).into_bytes();
    src.truncate(target);
    let missing = target.saturating_sub(src.len());
    match rng.weighted(&[30, 15, 20, 15, 10, 10]) {
        0 => {
            src.extend_from_slice(b"\n//");
            src.extend(std::iter::repeat_n(b'a', missing.saturating_sub(3)));
        }
        1 => src.extend(std::iter::repeat_n(b' ', missing)),
        2 => {
            src.extend_from_slice(b"\nx = \"");
            src.extend(std::iter::repeat_n(b'a', missing.saturating_sub(8)));
            src.extend_from_slice(b"\"\n");
        }
        3 => {
            src.extend_from_slice(b"\nx = ");
            src.extend(std::iter::repeat_n(b'a', missing.saturating_sub(5)));
        }
        4 => {
            src.push(b'\n');
            src.extend(std::iter::repeat_n(b'\xff', missing.saturating_sub(1)));
        }
        _ => {
            src.extend(std::iter::repeat_n(b'(', missing.saturating_sub(1)));
            src.push(b'1');
        }
    }
    src
}

fn digits(rng: &mut Rng, max: usize) -> String {
    (0..rng.range(1, max))
        .map(|_| char::from(b'0' + rng.below(10) as u8))
        .collect()
}

fn with_separators(rng: &mut Rng, text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        out.push(c);
        if rng.chance(8) {
            out.push('_');
        }
        if rng.chance(1) {
            out.push_str("__");
        }
    }
    out
}

/// A number-like text: valid ones, and ones that go wrong in each of the places the tokenizer checks.
fn number(rng: &mut Rng) -> String {
    const SPECIAL: &[&str] = &[
        "9223372036854775807",
        "9223372036854775808",
        "9223372036854775809",
        "18446744073709551615",
        "18446744073709551616",
        "99999999999999999999",
        "0",
        "00",
        "007",
        "1e999",
        "1e-999",
        "1e308",
        "1e309",
        "1.7976931348623157e308",
        "1.7976931348623159e308",
        "4.9e-324",
        "2.4e-324",
        "2.5e-324",
        "1e400",
        "-1e400",
        "0e0",
        "0.0",
        "0.",
        "1.",
        "1.e5",
        "1_0",
        "1__0",
        "1_",
        "1_.5",
        "1._5",
        "1.5_",
        "1e5_",
        "1e_5",
        "1e+",
        "1e-",
        "1e",
        "1E5",
        "1E+5",
        "1e5.5",
        "1.5.5",
        "1..5",
        "1.5e5e5",
        "0x1f",
        "0b101",
        "1a",
        "1_a",
        "5e",
        "09",
        "1__",
        "123456789012345678901234567890",
        "0.1e-2",
        "5e0",
        "5E-0",
    ];
    match rng.weighted(&[35, 40, 25]) {
        0 => {
            let d = digits(rng, 25);
            let d = if rng.chance(10) { format!("0{d}") } else { d };
            if rng.chance(30) {
                with_separators(rng, &d)
            } else {
                d
            }
        }
        1 => {
            let mut s = digits(rng, 22);
            if rng.chance(75) {
                s.push('.');
                if rng.chance(80) {
                    s.push_str(&digits(rng, 22));
                }
            }
            if rng.chance(50) {
                s.push(if rng.chance(50) { 'e' } else { 'E' });
                if rng.chance(50) {
                    s.push(if rng.chance(50) { '+' } else { '-' });
                }
                if rng.chance(90) {
                    s.push_str(&digits(rng, 5));
                }
            }
            if rng.chance(30) {
                with_separators(rng, &s)
            } else {
                s
            }
        }
        _ => (*rng.pick(SPECIAL)).to_owned(),
    }
}

fn string_text(rng: &mut Rng) -> Vec<u8> {
    const PIECES: &[&[u8]] = &[
        b"a",
        b"abc",
        b" ",
        b"\\n",
        b"\\t",
        b"\\r",
        b"\\\\",
        b"\\\"",
        b"\\'",
        b"\\0",
        b"\\x41",
        b"\\u00e9",
        b"\\q",
        b"\\\n",
        b"\\",
        b"\n",
        b"\r",
        b"//",
        b"/*",
        b"*/",
        b"`",
        b"\"",
        b"'",
        "é".as_bytes(),
        "日本語".as_bytes(),
        "😀".as_bytes(),
        b"\xff",
        b"\xc3",
        b"\xef\xbf\xbd",
        b"\0",
        b"\x7f",
    ];
    let quote = *rng.pick(b"\"'`");
    let mut out = vec![quote];
    for _ in 0..rng.range(0, 6) {
        out.extend_from_slice(rng.pick(PIECES));
    }
    if rng.chance(88) {
        out.push(quote);
    }
    out
}

fn comment_text(rng: &mut Rng) -> Vec<u8> {
    const COMMENTS: &[&[u8]] = &[
        b"//",
        b"// x",
        b"// x\n",
        b"//\n",
        b"//\r\n",
        b"// \xff\n",
        "// é\n".as_bytes(),
        b"/* x */",
        b"/**/",
        b"/*/",
        b"/*/ */",
        b"/* /* */",
        b"/* */ */",
        b"/* x",
        b"/*",
        b"/* \n */",
        b"/* * / */",
        b"/**",
        b"/***/",
        b"/* \xff */",
        b"/ /",
        b"/ *",
        b"/=",
        b"/",
        b"//*",
        b"/*/*/",
        b"// /* \n */",
    ];
    rng.pick(COMMENTS).to_vec()
}

fn fragment(rng: &mut Rng) -> Vec<u8> {
    match rng.weighted(&[22, 20, 14, 9, 12, 8, 9, 6]) {
        0 => number(rng).into_bytes(),
        1 => string_text(rng),
        2 => comment_text(rng),
        3 => {
            const CHARS: &[u8] = b"!=<>+-*/%.?:,&|^~@#$;\\";
            (0..rng.range(1, 4)).map(|_| *rng.pick(CHARS)).collect()
        }
        4 => {
            let seq = rng.pick(INTERESTING);
            match rng.below(4) {
                0 => seq.to_vec(),
                1 => [&b"a"[..], seq, b"b"].concat(),
                2 => [&b"x = \""[..], seq, b"\""].concat(),
                _ => [&b"// "[..], seq, b"\n"].concat(),
            }
        }
        5 => {
            let mut word = (*rng.pick(&[
                "if", "end", "in", "fun", "not", "or", "then", "else", "return", "null", "memo",
            ]))
            .to_owned();
            match rng.below(4) {
                0 => word.push_str(&digits(rng, 2)),
                1 => word.push('_'),
                2 => word.push('x'),
                _ => word.push('('),
            }
            word.into_bytes()
        }
        6 => (*rng.pick(&[
            "a.b",
            "a..b",
            "a...",
            "...",
            ".5",
            "5.",
            "a. b",
            "f(xs...)",
            "f(xs..., 1)",
            "f(...)",
            "f(a...)",
            "fun(a...): end",
            "fun(a..., b): end",
            "fun(...): end",
            "x[1...]",
            "a.1",
            "a.\"b\"",
            "{a.b: 1}",
            "a . b",
            "a .b",
            "a. b. c",
        ]))
        .as_bytes()
        .to_vec(),
        _ => rng
            .pick(&[
                &b"\x0b"[..],
                b"\x0c",
                b"\xc2\xa0",
                b"\xef\xbb\xbf",
                b"\0",
                b"\xe2\x80\x8b",
                b"\xe3\x80\x80",
                b"\r",
                b"\r\r\n",
                b"\t\t",
                b"\xc2\x85",
                b"\xe2\x80\xa9",
            ])
            .to_vec(),
    }
}

/// A few of the fragments above, joined by white space, as an assignment or on their own.
pub fn lexical(rng: &mut Rng) -> Vec<u8> {
    let mut src = Vec::new();
    if rng.chance(60) {
        src.extend_from_slice(rng.pick(NEAR_KEYWORDS).as_bytes());
        src.extend_from_slice(b" = ");
    } else if rng.chance(20) {
        src.extend_from_slice(rng.pick(ORDINARY).as_bytes());
        src.extend_from_slice(b" = [");
    }
    for _ in 0..rng.range(1, 5) {
        src.extend_from_slice(&fragment(rng));
        src.extend_from_slice(rng.pick(&[&b" "[..], b"\n", b"", b"\t", b", ", b" + "]));
    }
    if rng.chance(20) {
        src.push(b']');
    }
    src
}

const TEMPLATES: &[&str] = &[
    "x = N(1)",
    "N(1)",
    "N()",
    "x = N",
    "N = 1",
    "N += 1",
    "x[N] = 1",
    "x.N = 1",
    "x = {N: 1}",
    "x = {\"N\": 1}",
    "x = {N(): 1}",
    "x = {(N): 1}",
    "x = a.N",
    "x = a.N()",
    "x = a[\"N\"]",
    "x = \"N\"",
    "x = 'N'",
    "// N",
    "/* N */",
    "x = `N`",
    "fun N(): end",
    "memo fun N(a): end",
    "fun f(N): end",
    "fun f(a, N...): end",
    "x = fun(N): end",
    "x = fun(): N() end",
    "for (N in x): end",
    "for (i in N(x)): end",
    "try: x = 1 catch (N): end",
    "try: N() catch (e): end",
    "try { x = 1 } catch (N) { }",
    "if (N()) then: end",
    "while (N()): end",
    "return N()",
    "x = N(N(N))",
    "x = [N, N]",
    "x = N ? 1 : 2",
    "x = 1 ? N : 2",
    "x = not N",
    "x = -N",
    "x = N + 1",
    "x = N[0]",
    "x = N.a",
    "x = N(1)(2)",
    "x = (N)",
    "x = N or N",
    "import \"N\"",
    "import N",
    "x = \"import\"",
    "// import",
    "fun f(): import \"a\" end",
    "if (x) then: import \"a\" end",
    "x = fun(): import \"a\" end",
    "N",
    "N N",
    "a.N.N",
];

/// Short programs that place a name, refused or allowed, in each position the allowlist check looks at, inside
/// a wrapper of statements now and then.
pub fn allowlist(rng: &mut Rng, names: &Names) -> String {
    let mut body = String::new();
    for _ in 0..rng.range(1, 3) {
        let name = match rng.weighted(&[55, 25, 15, 5]) {
            0 => rng.pick(&names.disallowed).clone(),
            1 => rng.pick(&names.allowed).clone(),
            2 => (*rng.pick(ORDINARY)).to_owned(),
            _ => (*rng.pick(NEAR_KEYWORDS)).to_owned(),
        };
        body.push_str(&rng.pick(TEMPLATES).replace('N', &name));
        body.push('\n');
    }
    match rng.below(6) {
        0 => format!("if (x) then:\n{body}end"),
        1 => format!("while (x) {{\n{body}}}"),
        2 => format!("fun g():\n{body}end"),
        3 => format!("try:\n{body}catch (e):\nend"),
        _ => body,
    }
}

use super::*;
use crate::ast::Expr;
use crate::disallowed_builtins;

fn parse(src: &str) -> Program {
    match parse_source(src.as_bytes()) {
        Ok(program) => program,
        Err(e) => panic!("{src:?} should parse: {e}"),
    }
}

fn parse_error(src: &str) -> Error {
    match parse_source(src.as_bytes()) {
        Ok(_) => panic!("{src:?} should not parse"),
        Err(e) => e,
    }
}

fn parse_expr(src: &str, literal: bool) -> Result<ExprBox, Error> {
    let mut parser = Parser::new(src);
    parser.literal = literal;
    let result = parser.start().and_then(|()| parser.expression());
    parser.finish(result)
}

fn kind(expr: &Expr) -> &'static str {
    match expr {
        Expr::Literal { .. } => "Literal",
        Expr::Variable { .. } => "Variable",
        Expr::Binary { .. } => "Binary",
        Expr::Unary { .. } => "Unary",
        Expr::Ternary { .. } => "Ternary",
        Expr::Call { .. } => "Call",
        Expr::List { .. } => "List",
        Expr::Map { .. } => "Map",
        Expr::Function { .. } => "FunctionExpression",
        Expr::Subscript { .. } => "Subscript",
    }
}

/// An expression as a nested string, to check precedence and grouping without writing trees by hand.
fn show(expr: &Expr) -> String {
    match expr {
        Expr::Literal { value, .. } => match value {
            Literal::Int(n) => n.to_string(),
            Literal::Float(f) => f.to_string(),
            Literal::Str(s) => format!("{s:?}"),
            Literal::Bool(b) => b.to_string(),
            Literal::Null => "null".to_owned(),
        },
        Expr::Variable { name, .. } => name.clone(),
        Expr::Binary {
            op, left, right, ..
        } => format!("({} {op:?} {})", show(left), show(right)),
        Expr::Unary { op, operand, .. } => format!("({op:?} {})", show(operand)),
        Expr::Ternary {
            cond,
            then,
            otherwise,
            ..
        } => {
            format!("({} ? {} : {})", show(cond), show(then), show(otherwise))
        }
        Expr::Call {
            function,
            args,
            ellipsis,
            ..
        } => {
            let args: Vec<String> = args.iter().map(show).collect();
            format!(
                "{}({}{})",
                show(function),
                args.join(", "),
                if *ellipsis { "..." } else { "" }
            )
        }
        Expr::List { values, .. } => format!(
            "[{}]",
            values.iter().map(show).collect::<Vec<_>>().join(", ")
        ),
        Expr::Map { items, .. } => {
            let items: Vec<String> = items
                .iter()
                .map(|(k, v)| format!("{}: {}", show(k), show(v)))
                .collect();
            format!("{{{}}}", items.join(", "))
        }
        Expr::Function {
            params,
            ellipsis,
            body,
            ..
        } => {
            format!(
                "fun({}{}){{{}}}",
                params.join(","),
                if *ellipsis { "..." } else { "" },
                body.len()
            )
        }
        Expr::Subscript {
            container, index, ..
        } => format!("{}[{}]", show(container), show(index)),
    }
}

fn shape(src: &str) -> String {
    let both = (parse_expr(src, false), parse_expr(src, true));
    match both {
        (Ok(a), Ok(b)) => {
            assert_eq!(a, b, "the two parsers disagree on {src:?}");
            show(&a)
        }
        (a, b) => panic!("{src:?}: {a:?} / {b:?}"),
    }
}

// ---- ported from the Go parser tests ----

#[test]
fn expression_kinds() {
    let cases = [
        ("5", "Literal"),
        ("3.14", "Literal"),
        ("\"hello\"", "Literal"),
        ("true", "Literal"),
        ("false", "Literal"),
        ("null", "Literal"),
        ("x", "Variable"),
        ("a + b", "Binary"),
        ("a - b", "Binary"),
        ("a * b", "Binary"),
        ("a / b", "Binary"),
        ("a % b", "Binary"),
        ("a == b", "Binary"),
        ("a != b", "Binary"),
        ("a < b", "Binary"),
        ("a <= b", "Binary"),
        ("a > b", "Binary"),
        ("a >= b", "Binary"),
        ("a and b", "Binary"),
        ("a or b", "Binary"),
        ("not a", "Unary"),
        ("a in b", "Binary"),
        ("a + b * c", "Binary"),
        ("(a + b) * c", "Binary"),
        ("a + (b + c)", "Binary"),
        ("a[0]", "Subscript"),
        ("a[i + 1]", "Subscript"),
        ("f()", "Call"),
        ("f(a, b)", "Call"),
        ("f(a, b + c)", "Call"),
        ("[1, 2, 3]", "List"),
        ("{\"a\": 1, \"b\": 2}", "Map"),
        ("fun(x): return x * x end", "FunctionExpression"),
    ];
    for (src, want) in cases {
        let expr = parse_expr(src, false).unwrap_or_else(|e| panic!("{src}: {e}"));
        assert_eq!(kind(&expr), want, "{src}");
    }
}

#[test]
fn statement_counts() {
    let cases = [
        ("", 0),
        ("x = 5", 1),
        ("x = 5\ny = 10", 2),
        ("if (x > 5) then: y = 10 end", 1),
        ("while (x > 0): x = x - 1 end", 1),
        ("for (i in range(10)): print(i) end", 1),
        ("fun add(a, b): return a + b end", 1),
        ("try: x = 1/0 catch (err): print(err) end", 1),
        (
            "\n\t\tx = 5\n\t\ty = 10\n\t\tz = x + y\n\t\tprint(z)\n\t\t",
            4,
        ),
    ];
    for (src, want) in cases {
        assert_eq!(parse(src).statements.len(), want, "{src:?}");
    }
}

#[test]
fn if_statement_shapes() {
    let cases = [
        ("if (x > 5) then: y = 10 end", 1, 0),
        ("if (x > 5) then: y = 10 else: y = 5 end", 1, 1),
        ("if (x > 5) then: y = 10 z = 20 end", 2, 0),
        (
            "if (x > 5) then: y = 10 else if (x > 0) then: y = 5 else: y = 0 end",
            1,
            1,
        ),
    ];
    for (src, body_len, else_len) in cases {
        let program = parse(src);
        assert_eq!(program.statements.len(), 1, "{src}");
        let Stmt::If {
            cond,
            body,
            else_body,
            ..
        } = &program.statements[0]
        else {
            panic!("{src}: not an if");
        };
        assert_eq!(kind(cond), "Binary", "{src}");
        assert_eq!((body.len(), else_body.len()), (body_len, else_len), "{src}");
    }
}

#[test]
fn while_for_fun_try_shapes() {
    let program = parse("while (x > 0): x = x - 1 end");
    let Stmt::While { cond, body, .. } = &program.statements[0] else {
        panic!("not a while")
    };
    assert_eq!((kind(cond), body.len()), ("Binary", 1));

    let program = parse("for (i in range(10)): print(i) end");
    let Stmt::For {
        name,
        iterable,
        body,
        ..
    } = &program.statements[0]
    else {
        panic!("not a for")
    };
    assert_eq!(
        (name.as_str(), kind(iterable), body.len()),
        ("i", "Call", 1)
    );

    let program = parse("fun add(a, b): return a + b end");
    let Stmt::FunDef {
        name,
        params,
        ellipsis,
        body,
        ..
    } = &program.statements[0]
    else {
        panic!("not a fun")
    };
    assert_eq!(
        (name.as_str(), params.as_slice(), *ellipsis, body.len()),
        ("add", ["a".to_owned(), "b".to_owned()].as_slice(), false, 1)
    );

    let program = parse("try: x = 1/0 catch (err): print(err) end");
    let Stmt::TryCatch {
        try_block,
        error_name,
        catch_block,
        ..
    } = &program.statements[0]
    else {
        panic!("not a try")
    };
    assert_eq!(
        (try_block.len(), error_name.as_str(), catch_block.len()),
        (1, "err", 1)
    );
}

#[test]
fn error_messages_and_positions() {
    const VALUE: &str = "unexpected token EOF - expected a value (number, string, identifier, '(', '[', '{', 'fun', etc.)";
    const BLOCK: &str = "expected ':' to start block (after 'then', 'while(...)', 'for(...)', 'fun(...)', or 'try'), got";
    let cases = [
        ("x = ", format!("parse error at 1:5: {VALUE}")),
        ("if (x > 5) y = 10", "parse error at 1:12: expected then, but got name".to_owned()),
        ("while (x > 0) x = x-1", format!("parse error at 1:15: {BLOCK} name")),
        (
            "for (i in) print(i)",
            "parse error at 1:10: unexpected token ) - expected a value (number, string, identifier, '(', '[', '{', 'fun', etc.)".to_owned(),
        ),
        ("fun add(a, b) return a+b", format!("parse error at 1:15: {BLOCK} return")),
        ("x + ", format!("parse error at 1:5: {VALUE}")),
        ("(x + y", "parse error at 1:7: expected ), but got EOF".to_owned()),
        ("[1, 2, 3", "parse error at 1:9: expected ], but got EOF".to_owned()),
        (
            "\"a\": 1,",
            "parse error at 1:4: unexpected token : - expected a value (number, string, identifier, '(', '[', '{', 'fun', etc.)".to_owned(),
        ),
        ("try:", "parse error at 1:5: expected catch, but got EOF".to_owned()),
        ("try: x = 1 catch:", "parse error at 1:17: expected (, but got :".to_owned()),
        ("try: x = 1 catch (err) print(err)", format!("parse error at 1:24: {BLOCK} name")),
        ("fun add(a, b): return", format!("parse error at 1:22: {VALUE}")),
        ("try: x = 1", "parse error at 1:11: expected catch, but got EOF".to_owned()),
    ];
    for (src, want) in cases {
        assert_eq!(parse_error(src).to_string(), want, "{src:?}");
    }
    for src in [
        "if (x > 5) then: y = 10 end",
        "fun add(a, b): return a+b end",
        "throw x",
        "try: x = 1 catch (err): print(err) end",
    ] {
        parse(src);
    }
}

// ---- the grammar ----

#[test]
fn precedence_and_associativity() {
    let cases = [
        ("1 + 2 * 3", "(1 Add (2 Mul 3))"),
        ("1 * 2 + 3", "((1 Mul 2) Add 3)"),
        ("1 - 2 - 3", "((1 Sub 2) Sub 3)"),
        ("a ** b ** c", "(a Pow (b Pow c))"),
        ("-a ** b", "((Neg a) Pow b)"),
        ("a ** -b", "(a Pow (Neg b))"),
        ("a * b ** c", "(a Mul (b Pow c))"),
        ("a ** b * c", "((a Pow b) Mul c)"),
        ("- - a", "(Neg (Neg a))"),
        ("a == b < c", "(a Eq (b Lt c))"),
        ("a < b == c", "((a Lt b) Eq c)"),
        ("a < b in c", "((a Lt b) In c)"),
        ("a or b and c", "(a Or (b And c))"),
        ("a and b or c", "((a And b) Or c)"),
        ("a or b xor c", "(a Or (b Xor c))"),
        ("a xor b or c", "((a Xor b) Or c)"),
        ("not a == b", "(Not (a Eq b))"),
        ("not a and b", "((Not a) And b)"),
        ("a and not b", "(a And (Not b))"),
        ("not not a", "(Not (Not a))"),
        ("a == b ? c : d", "((a Eq b) ? c : d)"),
        ("a ? b : c ? d : e", "(a ? b : (c ? d : e))"),
        ("a ? b ? c : d : e", "(a ? (b ? c : d) : e)"),
        ("a or b ? c : d", "(a Or (b ? c : d))"),
        ("not a ? b : c", "(Not (a ? b : c))"),
        ("a ? b : c or d", "(a ? b : (c Or d))"),
        ("a + b ? c : d + e", "((a Add b) ? c : (d Add e))"),
        ("f(a)(b)[c].d", "f(a)(b)[c][\"d\"]"),
        ("f(a, b...)", "f(a, b...)"),
        ("-f(x)", "(Neg f(x))"),
        (
            "{a: 1, \"b\": 2, (c + 1): 3}",
            "{\"a\": 1, \"b\": 2, (c Add 1): 3}",
        ),
        ("[1, 2,]", "[1, 2]"),
        ("f(1, 2,)", "f(1, 2)"),
        ("(a)", "a"),
        ("x.y.z", "x[\"y\"][\"z\"]"),
    ];
    for (src, want) in cases {
        assert_eq!(shape(src), want, "{src}");
    }
}

#[test]
fn operators_that_cannot_start_a_right_operand() {
    for src in [
        "a == not b",
        "a + not b",
        "- not a",
        "a ** not b",
        "a * not b",
    ] {
        let e = parse_expr(src, false).expect_err(src);
        assert!(e.message.starts_with("unexpected token not"), "{src}: {e}");
        assert_eq!(parse_expr(src, true).expect_err(src), e, "{src}");
    }
}

#[test]
fn statements_in_every_form() {
    parse("x = 1\na[0] = 2\na.k = 3\nx += 1\nx -= 1\nx *= 2\nx /= 2\nx %= 2");
    parse("if (a) then { b = 1 } elif (c) then: d = 2 else { e = 3 }");
    parse("if (a) then: b = 1 elif (c) then: d = 2 elif (e) then: f = 3 else: g = 4 end");
    parse("while (a) { break continue }");
    parse("for (k in m) { print(k) }");
    parse("try { x = 1 } catch (e) { y = 2 }");
    parse("fun f(a, rest...): return rest end");
    parse("fun f(a, b,): return a end");
    parse("memo fun fib(n): return n end");
    parse("f = fun (x): return x end\nfun (y): return y end");
    parse("import \"lib.din\"");
    parse("while (true): x = 1"); // a colon block may end at the end of the input
    parse("x = 1 // comment\n/* block */ y = 2");
    assert!(matches!(
        parse("memo fun f(): return 1 end").statements[0],
        Stmt::FunDef { .. }
    ));
}

#[test]
fn bare_names_are_string_keys_and_selectors() {
    let program = parse("m = {open: 1}\nx = m.read");
    let Stmt::Assign {
        value: Expr::Map { items, .. },
        ..
    } = &program.statements[0]
    else {
        panic!("map")
    };
    assert!(matches!(&items[0].0, Expr::Literal { value: Literal::Str(s), .. } if s == "open"));
}

#[test]
fn refused_forms() {
    const TARGET: &str =
        "invalid assignment target: only variables and array/object elements can be assigned to";
    let cases = [
        ("f(x) = 1", TARGET),
        ("1 = 2", TARGET),
        (
            "a ? b",
            "missing ':' in ternary expression - expected format: condition ? true_value : false_value",
        ),
        ("f(a b)", "missing comma ',' between function arguments"),
        (
            "f(a..., b)",
            "variadic argument '...' must be the last argument in function call",
        ),
        ("[1 2]", "missing comma ',' between array elements"),
        ("{a: 1 b: 2}", "missing comma ',' between object properties"),
        (
            "fun f(a b): end",
            "missing comma ',' between function parameters",
        ),
        (
            "fun f(a..., b): end",
            "variadic parameter '...' must be the last parameter in function definition",
        ),
        (
            "import foo",
            "import statement requires a string filename, got name - example: import \"filename.din\"",
        ),
        (
            "memo fun (x): end",
            "expected function name after 'memo fun'",
        ),
        ("memo x", "expected fun, but got name"),
        (
            "end",
            "unexpected token end - expected a value (number, string, identifier, '(', '[', '{', 'fun', etc.)",
        ),
        (
            "if (a) then: b else c end",
            "expected ':' or 'if' after 'else' keyword, got name",
        ),
        (
            "try x catch (e): end",
            "expected ':' after 'try' keyword to start block, got name",
        ),
        ("x = fun f(a): end", "expected (, but got name"),
        (
            "else: x = 1",
            "unexpected token else - expected a value (number, string, identifier, '(', '[', '{', 'fun', etc.)",
        ),
        (
            "while (a): x = 1 else: y = 2 end",
            "unexpected token else - expected a value (number, string, identifier, '(', '[', '{', 'fun', etc.)",
        ),
    ];
    for (src, want) in cases {
        let e = parse_error(src);
        assert_eq!(e.kind, ErrorKind::Parse, "{src:?}");
        assert!(e.message.starts_with(want), "{src:?}: {e}");
    }
}

#[test]
fn tokenizer_errors_surface_as_tokenize_errors() {
    for src in [
        "x = @",
        "x = \"open",
        "x = 1__0",
        "x = 1 /* open",
        "x = 'a\nb'",
        "x = !y",
        "x = a .. b",
    ] {
        let e = parse_error(src);
        assert_eq!(e.kind, ErrorKind::Tokenize, "{src:?}: {e}");
    }
    assert_eq!(
        parse_error("x = @").to_string(),
        "parse error at 1:5: unexpected @"
    );
}

#[test]
fn number_literals_follow_the_reference_conversions() {
    let value = |src: &str| match &parse(src).statements[0] {
        Stmt::Assign {
            value: Expr::Literal { value, .. },
            ..
        } => value.clone(),
        other => panic!("{src}: {other:?}"),
    };
    assert_eq!(value("x = 9223372036854775807"), Literal::Int(i64::MAX));
    assert_eq!(value("x = 0123"), Literal::Int(123));
    assert_eq!(value("x = 1_000"), Literal::Int(1000));
    assert_eq!(value("x = 1."), Literal::Float(1.0));
    assert_eq!(value("x = 1.e2"), Literal::Float(100.0));
    assert_eq!(value("x = 2.5e-3"), Literal::Float(0.0025));
    assert_eq!(value("x = 1_000.5"), Literal::Float(1000.5));
    assert_eq!(value("x = 1e308"), Literal::Float(1e308));
    // Underflow is zero in both languages, a zero mantissa never overflows, and a huge exponent is read
    // without overflowing the exponent itself.
    assert_eq!(value("x = 1e-999999999999"), Literal::Float(0.0));
    assert_eq!(value("x = 0e999999999999"), Literal::Float(0.0));
    assert_eq!(
        value(&format!("x = 0.{}1", "0".repeat(400))),
        Literal::Float(0.0)
    );

    let int_over = parse_error("x = 9223372036854775808");
    assert_eq!(int_over.kind, ErrorKind::Parse);
    assert_eq!(
        int_over.to_string(),
        "parse error at 1:1: parse failed: tokenizer gave INT token that isn't an int: 9223372036854775808"
    );
    for src in [
        "x = 1e309",
        "x = 1e999999999999",
        &format!("x = 1{}.5", "0".repeat(400)),
    ] {
        let e = parse_error(src);
        assert!(
            e.message
                .starts_with("parse failed: tokenizer gave FLOAT token"),
            "{src}: {e}"
        );
    }
}

#[test]
fn utf8_is_checked_once_up_front() {
    let e = parse_source(b"x = \xff").expect_err("invalid byte");
    assert_eq!(
        (e.kind, e.position, e.message.as_str()),
        (
            ErrorKind::Tokenize,
            Position { line: 1, column: 5 },
            "invalid UTF-8 byte 0xff"
        )
    );
    let e = parse_source(b"a\nb\xc3\x28").expect_err("truncated sequence");
    assert_eq!(
        (e.kind, e.position),
        (ErrorKind::Tokenize, Position { line: 2, column: 2 })
    );
    assert_eq!(
        parse_source("x = \"h\u{e9}llo\" // \u{4e16}\u{754c}".as_bytes())
            .map(|p| p.statements.len()),
        Ok(1)
    );
    // A well-formed U+FFFD is refused as well, as the Go tokenizer does.
    let e = parse_error("x = 1 // \u{FFFD}");
    assert_eq!(e.kind, ErrorKind::Tokenize);
}

// ---- nesting depth ----

fn on_stack(bytes: usize, f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(bytes)
        .spawn(f)
        .expect("spawn")
        .join()
        .expect("thread panicked");
}

/// The nesting limit of Go: `max` levels of the construct parse, one more is refused, on a 2 MiB stack in a
/// debug build. An overflow aborts the test binary, so a stack that is too small shows up as a crash.
macro_rules! nest_test {
    ($name:ident, $max:expr, $build:expr) => {
        #[test]
        fn $name() {
            let build: fn(usize) -> String = $build;
            on_stack(2 << 20, move || {
                let max: usize = $max;
                let src = build(max);
                let program = parse_source(src.as_bytes())
                    .unwrap_or_else(|e| panic!("{max} levels must parse: {e}"));
                assert!(disallowed_builtins(&program, |_| true).is_empty());
                drop(program);
                let e =
                    parse_source(build(max + 1).as_bytes()).expect_err("one more level must fail");
                assert!(e.message.contains("nested too deeply"), "{e}");
            });
        }
    };
}

fn rep(s: &str, n: usize) -> String {
    s.repeat(n)
}

// Each count follows from where the Go parser counts a level: a statement, a block, an `if`, an expression,
// a prefix `not` or `-`, a `**`, an operator and a call, subscript or `.name` each add one level.
nest_test!(nest_parens, 998, |n| format!(
    "x = {}1{}",
    rep("(", n),
    rep(")", n)
));
nest_test!(nest_lists, 998, |n| format!(
    "x = {}1{}",
    rep("[", n),
    rep("]", n)
));
nest_test!(nest_maps, 998, |n| format!(
    "x = {}1{}",
    rep("{\"a\": ", n),
    rep("}", n)
));
nest_test!(nest_not, 998, |n| format!("x = {}true", rep("not ", n)));
nest_test!(nest_minus, 998, |n| format!("x = {}1", rep("-", n)));
nest_test!(nest_power, 998, |n| format!("x = 2{}", rep(" ** 2", n)));
nest_test!(nest_sum_chain, 998, |n| format!("x = 1{}", rep(" + 1", n)));
nest_test!(nest_or_chain, 998, |n| format!("x = a{}", rep(" or a", n)));
nest_test!(nest_call_chain, 998, |n| format!("x = f{}", rep("()", n)));
nest_test!(nest_subscripts, 997, |n| format!("x = a{}", rep("[0]", n)));
nest_test!(nest_selectors, 998, |n| format!("x = a{}", rep(".b", n)));
nest_test!(nest_call_arguments, 499, |n| format!(
    "x = {}1{}",
    rep("f(", n),
    rep(")", n)
));
nest_test!(nest_ternary, 998, |n| format!(
    "x = {}1{}",
    rep("true ? ", n),
    rep(" : 0", n)
));
nest_test!(nest_if_blocks, 333, |n| format!(
    "{}break\n{}",
    rep("if (true) then:\n", n),
    rep("end\n", n)
));
nest_test!(nest_elif_chain, 996, |n| format!(
    "if (true) then: break{} end",
    rep(" elif (true) then: break", n)
));
nest_test!(nest_else_if_chain, 996, |n| format!(
    "if (true) then: break{} end",
    rep(" else if (true) then: break", n)
));
nest_test!(nest_fun_bodies, 332, |n| format!(
    "x = {}1{}",
    rep("fun(): return ", n),
    rep(" end", n)
));
nest_test!(nest_fun_statements, 499, |n| format!(
    "{}break{}",
    rep("fun f(): ", n),
    rep(" end", n)
));
nest_test!(nest_for_loops, 499, |n| format!(
    "{}break{}",
    rep("for (i in x): ", n),
    rep(" end", n)
));
nest_test!(nest_while_braces, 499, |n| format!(
    "{}x = 1\n{}",
    rep("while (true) {\n", n),
    rep("}\n", n)
));
nest_test!(nest_try_colon, 997, |n| format!(
    "{}x = 1\n{}",
    rep("try:\n", n),
    rep("catch (e):\n    x = 2\nend\n", n)
));
nest_test!(nest_try_braces, 997, |n| format!(
    "{}x = 1\n{}",
    rep("try {\n", n),
    rep("} catch (e) {\n    x = 2\n}\n", n)
));

#[test]
fn deep_nesting_is_an_error_not_an_overflow() {
    let n = 1_000_000;
    let cases = [
        ("parens", format!("x = {}1{}", rep("(", n), rep(")", n))),
        ("lists", format!("x = {}{}", rep("[", n), rep("]", n))),
        (
            "maps",
            format!("x = {}1{}", rep("{\"a\": ", n), rep("}", n)),
        ),
        ("not", format!("x = {}true", rep("not ", n))),
        ("negative", format!("x = {}1", rep("-", n))),
        ("power", format!("x = 2{}", rep(" ** 2", n))),
        ("left_chain", format!("x = 1{}", rep(" + 1", n))),
        ("call_chain", format!("x = f{}", rep("()", n))),
        (
            "if_blocks",
            format!(
                "{}x = 1\n{}",
                rep("if (true) then:\n", 20_000),
                rep("end\n", 20_000)
            ),
        ),
        (
            "fun_bodies",
            format!(
                "x = {}1{}",
                rep("fun(): return ", 20_000),
                rep(" end", 20_000)
            ),
        ),
        (
            "ternary",
            format!("x = {}1{}", rep("true ? ", n), rep(" : 0", n)),
        ),
        ("subscripts", format!("x = a{}", rep("[0]", n))),
        (
            "args_nesting",
            format!("x = {}{}", rep("f(", n), rep(")", n)),
        ),
        (
            "try",
            format!(
                "{}x = 1\n{}",
                rep("try:\n", 4_360),
                rep("catch (e):\n    x = 2\nend\n", 4_360)
            ),
        ),
        (
            "try_braces",
            format!(
                "{}x = 1\n{}",
                rep("try {\n", 4_360),
                rep("} catch (e) {\n    x = 2\n}\n", 4_360)
            ),
        ),
        (
            "while",
            format!(
                "{}x = 1\n{}",
                rep("while (true) {\n", 4_360),
                rep("}\n", 4_360)
            ),
        ),
    ];
    on_stack(2 << 20, move || {
        for (name, src) in &cases {
            let e = parse_source(src.as_bytes()).expect_err(name);
            assert!(e.message.contains("nested too deeply"), "{name}: {e}");
        }
    });
    let ok = format!(
        "x = {}1{}\ny = 1{}\n{}x = 1\n{}",
        rep("(", 100),
        rep(")", 100),
        rep(" + 1", 100),
        rep("try:\n", 50),
        rep("catch (e):\n    x = 2\nend\n", 50)
    );
    parse(&ok);
}

/// Operator chains of different precedence levels are released one after another, so a tree can be far
/// deeper than the nesting limit. Parsing, walking and dropping it must not need that much stack.
#[test]
fn tree_deeper_than_the_limit_fits_a_small_stack() {
    on_stack(2 << 20, || {
        let ops = ["*", "+", "<", "==", "and", "xor", "or"];
        let src = format!(
            "x = 1{}",
            ops.iter()
                .map(|op| rep(&format!(" {op} 1"), 990))
                .collect::<String>()
        );
        let program = parse_source(src.as_bytes()).expect("each level stays under the limit");
        assert!(disallowed_builtins(&program, |_| true).is_empty());
        drop(program);
    });
}

/// Parentheses start a new operator chain, so chains inside chains give a left spine of about k * m nodes
/// under the nesting limit: 32,000 levels in 64 KiB. Freeing it, and freeing the half-built tree of a syntax
/// error, must not recurse once per level. Binary operators, calls and subscripts all build such spines.
#[test]
fn parenthesised_chains_at_the_size_limit_fit_a_small_stack() {
    on_stack(2 << 20, || {
        for (head, step, m) in [("1", "+1", 500), ("f", "()", 500), ("a", "[0]", 330)] {
            let k = 64;
            let chain = format!(
                "x = {}{head}{}",
                rep("(", k),
                rep(&format!("{})", rep(step, m)), k)
            );
            assert!(chain.len() <= 64 << 10, "{step}");
            let program = parse_source(chain.as_bytes()).expect("under the limit");
            assert!(disallowed_builtins(&program, |_| true).is_empty());
            drop(program);
            parse_source(format!("{chain} +").as_bytes()).expect_err("ends after an operator");
        }
    });
}

// ---- the Pratt loop against the one-function-per-level grammar ----

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % n as u64).expect("fits")
    }
    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
    fn pick(&mut self, weights: &[usize]) -> usize {
        let total: usize = weights.iter().sum();
        if total == 0 {
            return 0;
        }
        let mut r = self.below(total);
        weights
            .iter()
            .position(|&w| {
                if r < w {
                    true
                } else {
                    r -= w;
                    false
                }
            })
            .unwrap_or(0)
    }
}

const BINOPS: [&str; 16] = [
    "or", "xor", "and", "==", "!=", "<", "<=", ">", ">=", "in", "+", "-", "*", "/", "%", "**",
];

/// A random token soup with open constructs, mostly well formed so that many sources parse.
fn gen_soup(rng: &mut Rng, tokens: usize) -> String {
    let mut operand_weights: [usize; 8] =
        std::array::from_fn(|_| if rng.chance(25) { 0 } else { rng.below(100) });
    operand_weights[0] += 1;
    let after_weights: [usize; 6] =
        std::array::from_fn(|_| if rng.chance(25) { 0 } else { rng.below(100) });
    let fav: [&str; 3] = std::array::from_fn(|_| BINOPS[rng.below(BINOPS.len())]);
    let mut out = String::new();
    let mut closers: Vec<&str> = Vec::new();
    let mut want_operand = true;
    for _ in 0..tokens {
        if want_operand {
            match rng.pick(&operand_weights) {
                0 => {
                    out.push_str("a ");
                    want_operand = false;
                }
                1 => out.push_str("not "),
                2 => out.push_str("- "),
                3 => {
                    out.push_str("( ");
                    closers.push(") ");
                }
                4 => {
                    out.push_str("[ ");
                    closers.push("] ");
                }
                5 => {
                    out.push_str("f( ");
                    closers.push(") ");
                }
                6 => {
                    out.push_str("{ k : ");
                    closers.push("} ");
                }
                _ => {
                    out.push_str("1.5 ");
                    want_operand = false;
                }
            }
        } else {
            match rng.pick(&after_weights) {
                0 => {
                    let op = if rng.chance(80) {
                        fav[rng.below(3)]
                    } else {
                        BINOPS[rng.below(BINOPS.len())]
                    };
                    out.push_str(op);
                    out.push(' ');
                    want_operand = true;
                }
                1 => {
                    out.push_str("? ");
                    closers.push(": a ");
                    want_operand = true;
                }
                2 => out.push_str("[ 0 ] "),
                3 => out.push_str("( ) "),
                4 => out.push_str(". n "),
                _ => {
                    if let Some(closer) = closers.pop() {
                        out.push_str(closer);
                    }
                }
            }
        }
    }
    if want_operand {
        out.push_str("a ");
    }
    while let Some(closer) = closers.pop() {
        out.push_str(closer);
    }
    out
}

/// Long runs near the limit: one construct repeated, or a few operator chains one after another.
fn gen_runs(rng: &mut Rng) -> String {
    let count = |rng: &mut Rng| 900 + rng.below(120);
    match rng.below(3) {
        0 => {
            let (open, close) = [
                ("( ", ") "),
                ("[ ", "] "),
                ("f( ", ") "),
                ("{ k : ", "} "),
                ("not ", ""),
                ("- ", ""),
                ("a ? ", ": a "),
            ][rng.below(7)];
            let n = count(rng);
            format!("{}a {}", rep(open, n), rep(close, n))
        }
        1 => {
            let op = BINOPS[rng.below(BINOPS.len())];
            format!("a {}", rep(&format!("{op} a "), count(rng)))
        }
        _ => {
            let mut out = String::from("a ");
            for _ in 0..1 + rng.below(3) {
                let op = BINOPS[rng.below(BINOPS.len())];
                out.push_str(&rep(&format!("{op} a "), 300 + rng.below(710)));
            }
            out
        }
    }
}

#[test]
fn pratt_loop_matches_the_per_level_grammar() {
    on_stack(256 << 20, || {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let (mut accepted, mut too_deep, mut other_error) = (0, 0, 0);
        for i in 0..8000 {
            let long = 950 + rng.below(100);
            let src = match i % 5 {
                0 => gen_soup(&mut rng, 6),
                1 => gen_soup(&mut rng, 40),
                2 => gen_soup(&mut rng, 400),
                3 => gen_soup(&mut rng, long),
                _ => gen_runs(&mut rng),
            };
            let fast = parse_expr(&src, false);
            let literal = parse_expr(&src, true);
            assert!(
                fast == literal,
                "the parsers disagree on {src}\n{fast:?}\n{literal:?}"
            );
            match fast {
                Ok(_) => accepted += 1,
                Err(e) if e.message.contains("nested too deeply") => too_deep += 1,
                Err(_) => other_error += 1,
            }
        }
        // The comparison only means something if it covers all three outcomes.
        assert!(
            accepted > 1500 && too_deep > 150 && other_error > 100,
            "{accepted} {too_deep} {other_error}"
        );
    });
}

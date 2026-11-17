//! Runs of whole scripts through the public API: results, errors, and each limit at its boundary.

use uddin::{CompileFailure, LimitReason, Limits, Outcome, Profile, RequestVars, Runner, Script};

fn run_with(src: &str, edit: impl FnOnce(&mut Limits)) -> (Outcome, String) {
    let profile = Profile::embedded();
    let script = Script::compile(src.as_bytes(), &profile).expect("the script compiles");
    let mut limits = profile.limits();
    limits.capture_print = true;
    limits.deadline = None;
    edit(&mut limits);
    let mut runner = Runner::new();
    let ended = runner.run(&script, &RequestVars::default(), &limits);
    (
        ended,
        String::from_utf8_lossy(runner.printed()).into_owned(),
    )
}

fn run(src: &str) -> (Outcome, String) {
    run_with(src, |_| {})
}

fn limit(ended: &Outcome) -> Option<LimitReason> {
    match ended {
        Outcome::Limit { reason, .. } => Some(*reason),
        _ => None,
    }
}

fn nested_ifs(n: usize) -> String {
    format!(
        "x = 0\n{}x = 1\n{}",
        "if (true) then:\n".repeat(n),
        "end\n".repeat(n)
    )
}

#[test]
fn prints_and_computes() {
    let (ended, out) =
        run("x = 1 + 2\nprint(x, \"hi\", [1, \"a\"], {b: 2, a: 1.5}, 7 / 2, 7 % 3, 2 ** 10)\n");
    assert_eq!(ended, Outcome::Ok);
    assert_eq!(out, "3 hi [1, \"a\"] {\"a\": 1.5, \"b\": 2} 3 1 1024\n");
}

#[test]
fn errors_say_where_and_what() {
    let (ended, _) = run("x = 1\ny = x / 0\n");
    assert_eq!(
        ended,
        Outcome::Error {
            class: "value",
            text: "value error at 2:7: can't divide by zero".into()
        }
    );
    let (ended, out) = run("try:\n    [1][5]\ncatch (e):\n    print(e)\nend\n");
    assert_eq!(ended, Outcome::Ok);
    assert_eq!(out, "value error at 2:9: subscript 5 out of range\n");
}

#[test]
fn the_operation_limit_counts_every_statement_and_expression() {
    // `x = 1` is two operations: the statement and the literal.
    let src = "x = 1\n".repeat(5);
    assert_eq!(run_with(&src, |l| l.max_ops = 10).0, Outcome::Ok);
    let (ended, _) = run_with(&src, |l| l.max_ops = 9);
    assert_eq!(limit(&ended), Some(LimitReason::OpLimit));
    let Outcome::Limit { text, .. } = ended else {
        unreachable!()
    };
    assert_eq!(text, "5:5: execution stopped: op_limit");
}

#[test]
fn call_depth_stops_at_the_limit() {
    let src = |n: u32| {
        format!(
            "fun f(n):\n    if (n == 0) then:\n        return 0\n    end\n    return f(n - 1)\nend\nf({n})\n"
        )
    };
    assert_eq!(run(&src(63)).0, Outcome::Ok);
    assert_eq!(limit(&run(&src(64)).0), Some(LimitReason::CallDepth));
}

#[test]
fn nesting_stops_at_the_limit() {
    assert_eq!(run(&nested_ifs(256)).0, Outcome::Ok);
    assert_eq!(limit(&run(&nested_ifs(257)).0), Some(LimitReason::Nesting));
}

#[test]
fn value_depth_stops_at_the_limit() {
    let src = |d: u32| {
        format!("x = [1]\ni = 1\nwhile (i < {d}):\n    x = [x]\n    i += 1\nend\nprint(x == x)\n")
    };
    assert_eq!(run(&src(256)).0, Outcome::Ok);
    assert_eq!(limit(&run(&src(257)).0), Some(LimitReason::ValueDepth));
    let (ended, _) = run("x = [1]\nx[0] = x\nprint(x == x)\n");
    assert_eq!(limit(&ended), Some(LimitReason::ValueDepth));
}

#[test]
fn value_size_and_memory_stop_the_run() {
    let src = |n: u32| format!("s = \"a\" * {n}\nprint(len(s))\n");
    assert_eq!(run(&src(1 << 20)).0, Outcome::Ok);
    assert_eq!(
        limit(&run(&src((1 << 20) + 1)).0),
        Some(LimitReason::ValueSize)
    );
    // Three strings of 1 MiB cost 3 * (1 MiB + 16); the fourth may be 1048512 bytes and no more.
    let four = |n: u32| {
        format!("a = \"a\" * 1048576\nb = \"b\" * 1048576\nc = \"c\" * 1048576\nd = \"d\" * {n}\n")
    };
    assert_eq!(run(&four(1_048_512)).0, Outcome::Ok);
    assert_eq!(limit(&run(&four(1_048_513)).0), Some(LimitReason::Memory));
}

#[test]
fn a_limit_is_not_caught() {
    let (ended, out) =
        run("try:\n    while (true):\n    end\ncatch (e):\n    print(\"caught\")\nend\n");
    assert_eq!(limit(&ended), Some(LimitReason::OpLimit));
    assert_eq!(out, "");
}

#[test]
fn nothing_survives_from_one_run_to_the_next() {
    let profile = Profile::embedded();
    let mut limits = profile.limits();
    limits.capture_print = true;
    let (set, read) = (
        Script::compile(b"leftover = [1, 2, 3]\nprint(len(leftover))\n", &profile)
            .expect("compiles"),
        Script::compile(b"print(leftover)\n", &profile).expect("compiles"),
    );
    let mut runner = Runner::new();
    let vars = RequestVars::default();
    assert_eq!(runner.run(&set, &vars, &limits), Outcome::Ok);
    let ended = runner.run(&read, &vars, &limits);
    assert!(
        matches!(ended, Outcome::Error { class: "name", .. }),
        "{ended:?}"
    );
    assert_eq!(runner.run(&set, &vars, &limits), Outcome::Ok);
    assert_eq!(runner.printed(), b"3\n");
}

#[test]
fn a_run_that_grew_the_heap_does_not_pin_it() {
    let profile = Profile::embedded();
    let mut limits = profile.limits();
    limits.keep_heap = 64 << 10;
    let big =
        Script::compile(b"s = \"a\" * 1000000\nl = [0] * 100000\n", &profile).expect("compiles");
    let small = Script::compile(b"x = 1\n", &profile).expect("compiles");
    let mut runner = Runner::new();
    let vars = RequestVars::default();
    assert_eq!(runner.run(&big, &vars, &limits), Outcome::Ok);
    assert_eq!(runner.run(&small, &vars, &limits), Outcome::Ok);
    assert!(
        runner.retained_bytes() < 128 << 10,
        "{} bytes kept",
        runner.retained_bytes()
    );
}

#[test]
fn builtins_the_build_lacks_are_named_not_run() {
    let profile = Profile::embedded();
    assert_eq!(
        Script::compile(b"x = waf_header(\"a\")\n", &profile).err(),
        Some(CompileFailure::Unported(vec!["waf_header".into()]))
    );
    assert!(matches!(
        Script::compile(b"sleep(1)\n", &profile),
        Err(CompileFailure::Check(_))
    ));
}

#[test]
fn request_variables_are_globals() {
    let profile = Profile::embedded();
    let script = Script::compile(
        b"print(method, path, score, typeof(body), typeof(_waf_ctx))\n",
        &profile,
    )
    .expect("compiles");
    let mut limits = profile.limits();
    limits.capture_print = true;
    let mut runner = Runner::new();
    let vars = RequestVars {
        method: b"POST",
        path: b"/a b",
        score: 4.0,
        ..RequestVars::default()
    };
    assert_eq!(runner.run(&script, &vars, &limits), Outcome::Ok);
    assert_eq!(runner.printed(), b"POST /a b 4 string unknown\n");
}

#[test]
fn scripts_and_runners_cross_threads() {
    fn send<T: Send>() {}
    fn send_sync<T: Send + Sync>() {}
    send_sync::<Script>();
    send::<Runner>();
}

#[test]
fn deep_sources_compile_and_run_on_a_small_stack() {
    // The default stack of a spawned thread: neither the compiler nor the machine may recurse once per level.
    let sources: Vec<(&str, String)> = vec![
        (
            "parentheses",
            format!("print({}1{})\n", "(".repeat(900), ")".repeat(900)),
        ),
        (
            "left chain",
            format!("x = 0{}\nprint(x)\n", " + 1".repeat(990)),
        ),
        (
            "spine in parentheses",
            format!(
                "x = {}1{}\nprint(x)\n",
                "(".repeat(60),
                format!("{})", " + 1".repeat(250)).repeat(60)
            ),
        ),
        (
            "calls",
            format!(
                "fun f(x):\n    return x\nend\nprint({}1{})\n",
                "f(".repeat(400),
                ")".repeat(400)
            ),
        ),
        (
            "subscripts",
            format!(
                "x = {}1{}\nprint(x{})\n",
                "[".repeat(250),
                "]".repeat(250),
                "[0]".repeat(250)
            ),
        ),
        (
            "ternaries",
            format!("print({}1{})\n", "true ? ".repeat(300), " : 2".repeat(300)),
        ),
        ("unary", format!("print({}true)\n", "not ".repeat(400))),
        ("power", format!("print(1{})\n", " ** 1".repeat(800))),
        ("blocks", nested_ifs(250)),
        (
            "lists",
            format!(
                "x = {}{}\nprint(len(x))\n",
                "[".repeat(200),
                "]".repeat(200)
            ),
        ),
    ];
    let handle = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || {
            let profile = Profile::embedded();
            let mut runner = Runner::new();
            let mut limits = profile.limits();
            limits.deadline = None;
            for (name, src) in sources {
                let script = Script::compile(src.as_bytes(), &profile)
                    .unwrap_or_else(|e| panic!("{name}: {e}"));
                let ended = runner.run(&script, &RequestVars::default(), &limits);
                assert_eq!(ended, Outcome::Ok, "{name}");
            }
        })
        .expect("a thread starts");
    handle.join().expect("no overflow");
}

#[test]
fn the_deadline_stops_a_loop_the_operation_limit_would_not() {
    let (ended, _) = run_with("while (true):\nend\n", |l| {
        l.max_ops = 0;
        l.deadline = Some(std::time::Duration::from_millis(20));
    });
    assert_eq!(limit(&ended), Some(LimitReason::Canceled));
}

#[test]
fn captured_print_is_cut_at_the_cap() {
    let profile = Profile::embedded();
    let script = Script::compile(
        b"s = \"a\" * 1000000\nprint(s)\nprint(s)\nprint(s)\n",
        &profile,
    )
    .expect("compiles");
    let mut limits = profile.limits();
    limits.capture_print = true;
    limits.print_cap = 2_500_000;
    let mut runner = Runner::new();
    assert_eq!(
        runner.run(&script, &RequestVars::default(), &limits),
        Outcome::Ok
    );
    assert_eq!(runner.printed().len(), 2_500_000);
    assert!(runner.print_truncated());
    limits.capture_print = false;
    assert_eq!(
        runner.run(&script, &RequestVars::default(), &limits),
        Outcome::Ok
    );
    assert!(runner.printed().is_empty() && !runner.print_truncated());
}

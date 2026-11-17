use std::borrow::Cow;
use std::time::{Duration, Instant};

use super::*;

fn compile(src: &str) -> WafScript {
    WafScript::compile(src.as_bytes(), &profile()).unwrap_or_else(|e| panic!("{src}: {e}"))
}

fn limits() -> Limits {
    let mut limits = profile().limits();
    limits.capture_print = true;
    limits.deadline = None;
    limits
}

/// Runs `src` for `view` and gives the run and what it printed.
fn exec(src: &str, view: &RequestView<'_>) -> (Run, String) {
    let script = compile(src);
    let mut runner = WafRunner::new();
    let run = runner.run(&script, view, &limits(), None);
    (run, String::from_utf8_lossy(runner.printed()).into_owned())
}

fn printed(src: &str, view: &RequestView<'_>) -> String {
    let (run, out) = exec(src, view);
    assert_eq!(run.end, End::Done, "{src}\n{out}");
    out
}

fn request() -> RequestView<'static> {
    static HEADERS: [(&[u8], &[u8]); 3] = [
        (b"Content-Type", b"application/json"),
        (b"X-Forwarded-For", b"203.0.113.9"),
        (b"x-forwarded-for", b"198.51.100.1"),
    ];
    static DETECTED: [&[u8]; 2] = [b"SQLi", b"xss"];
    RequestView {
        method: b"POST",
        path: b"/admin/users",
        query: b"id=7&name=a%20b+c&bad=%zz&flag&x=1=2",
        ip: b"203.0.113.9",
        country: b"ID",
        ua: b"curl/8",
        body: b"{\"user\": \"admin\"}",
        score: 4.5,
        action: b"LOG",
        headers: &HEADERS,
        detected: &DETECTED,
        ..RequestView::default()
    }
}

#[test]
fn the_variables_hold_the_request() {
    assert_eq!(
        printed(
            "print(method, path, ip, country, ua, score, action, body)",
            &request()
        ),
        "POST /admin/users 203.0.113.9 ID curl/8 4.5 LOG {\"user\": \"admin\"}\n"
    );
}

#[test]
fn reads_headers_queries_and_the_body() {
    let script = "
print(waf_header(\"content-type\"), waf_header(\"CONTENT-TYPE\"), waf_header(\"x-forwarded-for\"), waf_header(\"x-none\"))
print(waf_query(\"id\"), waf_query(\"name\"), waf_query(\"bad\"), waf_query(\"flag\"), waf_query(\"x\"), waf_query(\"none\"))
print(waf_body_contains(\"admin\"), waf_body_contains(\"root\"), waf_body_contains(\"\"))
print(waf_header(1))
print(waf_query([]))
print(waf_body_contains(null))
";
    assert_eq!(
        printed(script, &request()),
        "application/json application/json 203.0.113.9 null\n\
         7 a b c  null 1=2 null\n\
         true false true\n\
         waf_header() requires a string argument, not integer\n\
         waf_query() requires a string argument, not array\n\
         waf_body_contains() requires a string argument, not nullable\n"
    );
}

#[test]
fn matches_paths_and_networks() {
    let script = "
print(waf_path_match(\"/admin/*\"), waf_path_match(\"/admin/user?\"), waf_path_match(\"/admin\"), waf_path_match(\"/a[\"), waf_path_match(\"*\"))
print(waf_cidr_match(ip, \"203.0.113.0/24\"), waf_cidr_match(ip, \"10.0.0.0/8\"), waf_cidr_match(\"::1\", \"::1/128\"), waf_cidr_match(\"bad\", \"10.0.0.0/8\"), waf_cidr_match(ip, \"bad\"))
print(waf_cidr_match(1, \"10.0.0.0/8\"))
print(waf_cidr_match(ip, 2))
";
    assert_eq!(
        printed(script, &request()),
        "true true false false false\n\
         true false true false false\n\
         waf_cidr_match() requires string arguments, not integer\n\
         waf_cidr_match() requires string arguments, not integer\n"
    );
}

#[test]
fn reads_the_score_the_action_and_what_was_detected() {
    let script = "
print(waf_score(), typeof(waf_score()), waf_action(), waf_detected(\"sqli\"), waf_detected(\"XSS\"), waf_detected(\"rce\"))
print(waf_detected_any(), waf_detected_list(), len(waf_detected_list()))
score = 1
print(waf_score())
";
    assert_eq!(
        printed(script, &request()),
        "4.5 float LOG true true false\ntrue [\"SQLi\", \"xss\"] 2\n4.5\n"
    );
    assert_eq!(
        printed(
            "print(waf_score(), waf_action() == \"\", waf_detected_any(), waf_detected_list())",
            &RequestView::default()
        ),
        "0 true false []\n"
    );
}

#[test]
fn reads_fields_and_values_through_the_resolvers() {
    let field = |name: &[u8]| -> Vec<u8> {
        if name == b"query:id" {
            b"7".to_vec()
        } else {
            Vec::new()
        }
    };
    let values = |name: &[u8], each: &mut dyn FnMut(&[u8])| {
        if name == b"form:tags" {
            each(b"a");
            each(b"b");
        }
    };
    let view = RequestView {
        field: Some(&field),
        field_values: Some(&values),
        ..RequestView::default()
    };
    assert_eq!(
        printed(
            "print(waf_field(\"query:id\"), waf_field(\"nope\") == \"\", waf_values(\"form:tags\"), waf_values(\"nope\"))",
            &view
        ),
        "7 true [\"a\", \"b\"] []\n"
    );
    assert_eq!(
        printed(
            "print(waf_field(\"query:id\") == \"\", waf_values(\"form:tags\"))",
            &RequestView::default()
        ),
        "true []\n"
    );
}

#[test]
fn hands_out_the_json_body_as_a_fresh_copy() {
    let doc = parse_json(br#"{"n": 5, "f": 1.5, "big": 12345678901234567890, "huge": 1e400, "s": "x", "l": [1, [true, null]]}"#)
        .expect("json");
    let view = RequestView {
        json: Some(&doc),
        ..RequestView::default()
    };
    let script = "
j = waf_json()
print(j.n, typeof(j.n), j.f, typeof(j.f), j.big, typeof(j.big), j.huge, typeof(j.huge), j.s, j.l)
j.n = 99
print(waf_json().n)
print(waf_json() == waf_json())
";
    assert_eq!(
        printed(script, &view),
        "5 integer 1.5 float 1.23457e+19 float 1e400 string x [1, [true, null]]\n5\ntrue\n"
    );
    assert_eq!(
        printed("print(waf_json())", &RequestView::default()),
        "null\n"
    );
}

#[test]
fn a_decision_ends_the_function_and_the_first_one_stands() {
    let (run, out) = exec(
        "fun f():\n    waf_block(\"why\")\n    print(\"not here\")\nend\nx = f()\nprint(x)\nwaf_allow()\nprint(\"not here either\")\n",
        &request(),
    );
    assert_eq!(out, "BLOCK\n");
    assert_eq!(run.verdict, Some(Verdict::Block));
    // At the top level it ends the script with the error `return` gives there, and the decision stands.
    assert!(
        matches!(&run.end, End::Error { class: "runtime", text } if text == "runtime error at 7:1: can't return at top level"),
        "{:?}",
        run.end
    );
    assert_eq!(run.incomplete(), None);
    for (call, want) in [
        ("waf_return(\" allow \")", Verdict::Allow),
        ("waf_return(\"Log\")", Verdict::Log),
        ("waf_log()", Verdict::Log),
    ] {
        let (run, _) = run_src(call);
        assert_eq!(run.verdict, Some(want), "{call}");
    }
    // Inside try a decision goes through `catch`.
    let (run, out) = run_src(
        "fun f():\n    try:\n        waf_block()\n    catch (e):\n        print(\"caught\")\n    end\n    print(\"not here\")\nend\nf()\nprint(\"done\")\n",
    );
    assert_eq!(
        (run.verdict, run.end, out.as_str()),
        (Some(Verdict::Block), End::Done, "done\n")
    );
}

fn run_src(src: &str) -> (Run, String) {
    exec(src, &RequestView::default())
}

#[test]
fn a_bad_decision_is_an_error_value_and_decides_nothing() {
    let script = "
print(waf_return(\"deny\"))
print(waf_return(5))
print(waf_block(1, 2))
print(waf_allow(1, 2))
print(waf_log(1, 2))
print(waf_return())
";
    let (run, out) = run_src(script);
    assert_eq!(run.verdict, None);
    assert_eq!(
        out,
        "waf_return() invalid action \"deny\": must be ALLOW, LOG, or BLOCK\n\
         waf_return() requires a string argument, not integer\n\
         waf_block() accepts at most 1 optional argument\n\
         waf_allow() accepts at most 1 optional argument\n\
         waf_log() accepts at most 1 optional argument\n"
    );
    assert!(
        matches!(run.end, End::Error { class: "type", .. }),
        "{:?}",
        run.end
    );
}

#[test]
fn a_script_that_takes_the_request_away_gets_an_empty_one() {
    let script = "
_waf_ctx = 5
print(waf_header(\"content-type\"), waf_query(\"id\"), waf_body_contains(\"admin\"), waf_path_match(\"*\"), waf_score(), waf_action() == \"\", waf_detected_any(), waf_detected_list(), waf_json(), waf_field(\"x\") == \"\", waf_values(\"x\"))
";
    assert_eq!(
        printed(script, &request()),
        "null null false false 0 true false [] null true []\n"
    );
    // The request is where the name is looked up: another name inside a function leaves the global one alone.
    assert_eq!(
        printed(
            "fun f():\n    _waf_ctx = 1\n    return waf_score()\nend\nprint(f(), waf_score())\n",
            &request()
        ),
        "0 4.5\n"
    );
    assert_eq!(
        printed(
            "print(typeof(_waf_ctx))\nprint(map([1], fun (x): return waf_score() end))\n",
            &request()
        ),
        "unknown\n[4.5]\n"
    );
}

#[test]
fn says_why_a_rule_did_not_finish() {
    let (run, _) = run_src("x = 1 / 0");
    assert_eq!(run.incomplete(), Some("error"));
    let (run, _) = run_src("while (true):\nend");
    assert_eq!(run.incomplete(), Some("op_limit"));
    let (run, _) = run_src("print(1)");
    assert_eq!((run.verdict, run.incomplete()), (None, None));
    // A decision that came before the failure is the answer.
    let (run, _) = run_src("waf_log()\n");
    assert_eq!(run.incomplete(), None);
    let script = compile("while (true):\nend");
    let mut runner = WafRunner::new();
    let mut l = limits();
    l.max_ops = 0;
    l.deadline = Some(Duration::from_millis(20));
    let run = runner.run(&script, &RequestView::default(), &l, None);
    assert_eq!(run.incomplete(), Some("timeout"));
}

#[test]
fn the_deadline_of_the_request_cuts_the_deadline_of_the_script() {
    let script = compile("while (true):\nend");
    let mut runner = WafRunner::new();
    let mut l = limits();
    l.max_ops = 0;
    l.deadline = Some(Duration::from_secs(30));
    let started = Instant::now();
    let until = started + Duration::from_millis(30);
    let run = runner.run(&script, &RequestView::default(), &l, Some(until));
    assert_eq!(run.incomplete(), Some("timeout"));
    assert!(started.elapsed() < Duration::from_secs(5));
    // A deadline that has passed means the script does not start.
    let run = runner.run(&script, &RequestView::default(), &l, Some(Instant::now()));
    assert_eq!(run.incomplete(), Some("timeout"));
}

struct Bomb;

impl RequestContext for Bomb {
    fn method(&self) -> &[u8] {
        panic!("a failure of the embedder")
    }

    fn path(&self) -> &[u8] {
        b""
    }

    fn query(&self) -> &[u8] {
        b""
    }

    fn ip(&self) -> &[u8] {
        b""
    }

    fn country(&self) -> &[u8] {
        b""
    }

    fn ua(&self) -> &[u8] {
        b""
    }

    fn body(&self) -> &[u8] {
        b""
    }

    fn score(&self) -> f64 {
        0.0
    }

    fn action(&self) -> &[u8] {
        b""
    }

    fn header_count(&self) -> usize {
        0
    }

    fn header(&self, _: usize) -> (&[u8], &[u8]) {
        (b"", b"")
    }

    fn detected_count(&self) -> usize {
        0
    }

    fn detected(&self, _: usize) -> &[u8] {
        b""
    }

    fn json(&self) -> Option<Cow<'_, Json>> {
        None
    }

    fn field(&self, _: &[u8]) -> Cow<'_, [u8]> {
        Cow::Borrowed(b"")
    }

    fn field_values(&self, _: &[u8], _: &mut dyn FnMut(&[u8])) {}
}

#[test]
fn a_panic_is_caught_and_the_runner_goes_on() {
    let script = compile("print(1)");
    let mut runner = WafRunner::new();
    let run = runner.run(&script, &Bomb, &limits(), None);
    assert_eq!(
        (run.verdict, run.end.clone(), run.incomplete()),
        (None, End::Panic, Some("panic"))
    );
    let run = runner.run(&script, &RequestView::default(), &limits(), None);
    assert_eq!(run.end, End::Done);
    assert_eq!(runner.printed(), b"1\n");
}

#[test]
fn scripts_and_runners_cross_threads_as_the_agent_needs() {
    fn send_sync<T: Send + Sync>() {}
    fn send<T: Send>() {}
    send_sync::<WafScript>();
    send::<WafRunner>();
    let script = std::sync::Arc::new(compile("waf_block()"));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let script = script.clone();
            std::thread::spawn(move || {
                let mut runner = WafRunner::new();
                (0..50)
                    .map(|_| {
                        runner
                            .run(&script, &RequestView::default(), &limits(), None)
                            .verdict
                    })
                    .all(|v| v == Some(Verdict::Block))
            })
        })
        .collect();
    for h in handles {
        assert!(h.join().expect("a thread"));
    }
}

#[test]
fn the_profile_names_every_builtin_this_build_carries() {
    let profile = profile();
    let names: Vec<&str> = builtins().iter().map(|b| b.name).collect();
    assert_eq!(names.iter().filter(|n| n.starts_with("waf_")).count(), 17);
    assert_eq!(names.len(), 133);
    for name in &names {
        assert!(profile.allows(name), "{name}");
    }
}

#[test]
fn the_request_builtins_run_under_every_limit() {
    let (run, _) = run_src(
        "while (true):\n    waf_header(\"x\")\n    waf_query(\"y\")\n    waf_path_match(\"*\")\nend\n",
    );
    assert_eq!(run.incomplete(), Some("op_limit"));
    let items = "[".to_owned() + &"1,".repeat(300_000) + "1]";
    let doc = parse_json(items.as_bytes()).expect("json");
    let view = RequestView {
        json: Some(&doc),
        ..RequestView::default()
    };
    let (run, _) = exec(
        "l = []\nwhile (true):\n    l = l + [waf_json()]\nend\n",
        &view,
    );
    assert_eq!(run.incomplete(), Some("memory"));
    let values = |_: &[u8], each: &mut dyn FnMut(&[u8])| {
        for _ in 0..200_000 {
            each(b"v");
        }
    };
    let view = RequestView {
        field_values: Some(&values),
        ..RequestView::default()
    };
    let (run, _) = exec("while (true):\n    waf_values(\"x\")\nend\n", &view);
    assert_eq!(run.incomplete(), Some("memory"));
    // A pattern that would take longer than a script may is given up, and the rule is late, not wrong.
    let path = vec![b'a'; 8_000];
    let view = RequestView {
        path: &path,
        ..RequestView::default()
    };
    let (run, _) = exec(
        "print(waf_path_match(\"*\" + \"a\" * 5000 + \"b\"))\n",
        &view,
    );
    assert_eq!(run.incomplete(), Some("timeout"));
    // A document nested deeper than a script may build is built without recursion and freed the same way.
    let deep = format!("{}1{}", "[".repeat(9_000), "]".repeat(9_000));
    let doc = parse_json(deep.as_bytes()).expect("json");
    let view = RequestView {
        json: Some(&doc),
        ..RequestView::default()
    };
    let (run, _) = exec("x = waf_json()\nprint(typeof(x))\n", &view);
    assert_eq!(run.end, End::Done);
}

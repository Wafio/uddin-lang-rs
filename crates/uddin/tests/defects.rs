//! The defects of the reference that this implementation fixes, one test each, and the limits on the code the
//! fixes added. Each block says what the reference does.

use std::time::{Duration, Instant};

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

fn out(src: &str) -> String {
    let (ended, printed) = run_with(src, |_| {});
    assert_eq!(ended, Outcome::Ok, "{src}\n{printed}");
    printed
}

fn limit(ended: &Outcome) -> Option<LimitReason> {
    match ended {
        Outcome::Limit { reason, .. } => Some(*reason),
        _ => None,
    }
}

fn refused(src: &str) -> String {
    match Script::compile(src.as_bytes(), &Profile::embedded()) {
        Err(CompileFailure::Flow(text)) => text,
        other => panic!("{src}: {other:?}"),
    }
}

#[test]
fn a_jump_outside_a_loop_of_its_own_function_is_refused_at_compile_time() {
    // The reference ends the loop of the caller, or says "break statement not within a loop" at run time.
    assert_eq!(
        refused("fun stop():\n    break\nend\nfor (i in [1, 2]):\n    stop()\nend\n"),
        "compile error at 2:5: break statement not within a loop"
    );
    assert_eq!(
        refused("print(1)\ncontinue\n"),
        "compile error at 2:1: continue statement not within a loop"
    );
    assert_eq!(
        refused("while (true):\n    f = fun ():\n        break\n    end\nend\n"),
        "compile error at 3:9: break statement not within a loop"
    );
    // The first one in the source is the one named.
    assert_eq!(
        refused("fun a():\n    continue\nend\nbreak\n"),
        "compile error at 2:5: continue statement not within a loop"
    );
}

#[test]
fn a_jump_passes_through_try_and_ends_its_own_loop() {
    // The reference's `catch` takes the jump as an error and binds its text.
    assert_eq!(
        out(
            "for (i in [1, 2, 3]):\n    try:\n        break\n    catch (e):\n        print(e)\n    end\nend\nprint(\"after\")\n"
        ),
        "after\n"
    );
    assert_eq!(
        out(
            "n = 0\nwhile (n < 3):\n    n += 1\n    try:\n        continue\n    catch (e):\n        print(e)\n    end\n    print(\"body\")\nend\nprint(n)\n"
        ),
        "3\n"
    );
    assert_eq!(
        out(
            "for (i in [1, 2]):\n    try:\n        x = 1 / 0\n    catch (e):\n        break\n    end\nend\nx = 5\nprint(x, i)\n"
        ),
        "5 1\n"
    );
    assert_eq!(
        out(
            "fun f():\n    for (i in [1, 2, 3]):\n        if (i == 2) then:\n            break\n        end\n    end\n    return i\nend\nprint(f())\n"
        ),
        "2\n"
    );
}

#[test]
fn a_return_through_try_leaves_no_scope_behind() {
    // The reference leaves a copy of the function's variables on top, which hides what the caller assigned later.
    let script = "x = 1\nfun f():\n    try:\n        return 1\n    catch (e):\n    end\nend\nx = 2\nf()\nprint(x)\n";
    assert_eq!(out(script), "2\n");
    let from_catch = "x = 1\nfun f():\n    try:\n        y = 1 / 0\n    catch (e):\n        return 5\n    end\nend\nx = 2\nprint(f())\nprint(x)\n";
    assert_eq!(out(from_catch), "5\n2\n");
    let in_loop = "x = 1\nfun f():\n    for (i in [1, 2, 3]):\n        try:\n            if (i == 2) then:\n                return i\n            end\n        catch (e):\n        end\n    end\nend\nx = 2\nprint(f(), x)\n";
    assert_eq!(out(in_loop), "2 2\n");
}

#[test]
fn faults_are_typed_errors_a_script_can_catch() {
    // The reference stops with "invalid memory address", "slice bounds out of range" or "interface conversion".
    let cases = [
        (
            "for (x in 5):\nend",
            "type error at 1:11: for requires a string, array, or object to loop over, got integer",
        ),
        (
            "print(str((5)...))",
            "type error at 1:10: spread requires a string, array, or object, got integer",
        ),
        (
            "fun v(a, rest...):\n    return a\nend\nv()",
            "type error at 4:1: v() requires at least 1 arg, got 0",
        ),
        (
            "print == len",
            "type error at 1:7: can't compare builtin functions",
        ),
        (
            "float(1.5, \"a\")",
            "type error at 1:1: float() requires an integer number of digits",
        ),
        (
            "substr(\"abc\", 2, 9)",
            "value error at 1:1: substr() range 2:9 is outside a string of 3 bytes",
        ),
    ];
    for (src, want) in cases {
        let (ended, _) = run_with(src, |_| {});
        match ended {
            Outcome::Error { text, .. } => assert_eq!(text, want, "{src}"),
            other => panic!("{src}: {other:?}"),
        }
        let caught = format!(
            "try:\n{}\ncatch (e):\n    print(e)\nend\n",
            src.replace('\n', "\n    ")
        );
        let tail = want.split_once(": ").expect("a message").1;
        assert!(out(&caught).contains(tail), "{src}");
    }
}

#[test]
fn join_writes_elements_as_text() {
    // The reference writes join(["a", "b"], "") as "a""b".
    assert_eq!(
        out("print(join([\"a\", \"b\"], \"\"), join([1, \"a\", [\"b\"]], \"-\"), join([], \",\"))"),
        "ab 1-a-[\"b\"] \n"
    );
}

#[test]
fn join_keeps_its_value_size_limit() {
    let at = "s = \"a\" * 1048575\nprint(len(join([s, \"b\"], \"\")))\n";
    assert_eq!(out(at), "1048576\n");
    let over = "s = \"a\" * 1048575\nx = join([s, \"bc\"], \"\")\n";
    let (ended, _) = run_with(over, |l| l.max_alloc_bytes = 0);
    assert_eq!(limit(&ended), Some(LimitReason::ValueSize));
    let separators = "x = join([\"a\"] * 40000, \"x\" * 30)\n";
    let (ended, _) = run_with(separators, |l| l.max_alloc_bytes = 0);
    assert_eq!(limit(&ended), Some(LimitReason::ValueSize));
}

#[test]
fn mode_breaks_a_tie_for_the_value_that_got_there_first() {
    assert_eq!(
        out(
            "print(mode([1, 2, 2, 1]), mode([\"a\", \"b\", \"b\", \"a\"]), mode([3, 1, 3, 1, 3, 2]), mode([5, 5, 6, 6, 7]))"
        ),
        "2 b 3 5\n"
    );
}

#[test]
fn sort_takes_a_key_function() {
    let script = "l = [\"ccc\", \"a\", \"bb\", \"dd\"]\nsort(l, len)\nprint(l)\nm = [3, 1, 2]\nsort(m, fun (x): return -x end)\nprint(m)\npairs = [[2, \"x\"], [1, \"y\"], [2, \"z\"], [1, \"w\"]]\nsort(pairs, fun (p): return p[0] end)\nprint(pairs)\n";
    assert_eq!(
        out(script),
        "[\"a\", \"bb\", \"dd\", \"ccc\"]\n[3, 2, 1]\n[[1, \"y\"], [1, \"w\"], [2, \"x\"], [2, \"z\"]]\n"
    );
    // Calling the key of a list of one element is not an error, and a key that is no function is.
    assert_eq!(out("l = [1]\nsort(l, 5)\nprint(l)\n"), "[1]\n");
    let (ended, _) = run_with("sort([1, 2], 5)", |_| {});
    assert!(
        matches!(ended, Outcome::Error { text, .. } if text.ends_with("sort() requires second argument to be a function"))
    );
    let (ended, _) = run_with("sort([1, 2], len, 3)", |_| {});
    assert!(
        matches!(ended, Outcome::Error { text, .. } if text.ends_with("sort() requires 1 or 2 args, got 3"))
    );
    // A comparison that fails leaves the list as it was.
    assert_eq!(
        out(
            "l = [1, \"a\", 2]\ntry:\n    sort(l, fun (x): return x end)\ncatch (e):\n    print(\"failed\")\nend\nprint(l)\n"
        ),
        "failed\n[1, \"a\", 2]\n"
    );
}

#[test]
fn the_key_of_a_sort_runs_under_every_limit() {
    let many = "l = range(0, 200)\nsort(l, fun (x): return -x end)\nprint(l[0])\n";
    assert_eq!(out(many), "199\n");
    let (ended, _) = run_with(many, |l| l.max_ops = 400);
    assert_eq!(limit(&ended), Some(LimitReason::OpLimit));
    let deep = "fun id(x):\n    return x\nend\nsort([2, 1], fun (x): return id(x) end)\n";
    let (ended, _) = run_with(deep, |l| l.max_call_depth = 1);
    assert_eq!(limit(&ended), Some(LimitReason::CallDepth));
    let nested = "fun nest(x):\n    l = [x]\n    i = 0\n    while (i < 300):\n        l = [l]\n        i += 1\n    end\n    return l\nend\nsort([2, 1], nest)\n";
    let (ended, _) = run_with(nested, |_| {});
    assert_eq!(limit(&ended), Some(LimitReason::ValueDepth));
    let big = "sort([2, 1, 3, 4, 5, 6, 7, 8], fun (x): return \"a\" * 1000000 end)\n";
    let (ended, _) = run_with(big, |_| {});
    assert_eq!(limit(&ended), Some(LimitReason::Memory));
    let (ended, _) = run_with(
        "while (true):\n    sort([2, 1], fun (x): return x end)\nend\n",
        |l| {
            l.max_ops = 0;
            l.deadline = Some(Duration::from_millis(50));
        },
    );
    assert_eq!(limit(&ended), Some(LimitReason::Canceled));
}

#[test]
fn set_remove_takes_out_the_member_filed_under_the_key() {
    // The reference takes out the first member that is == to the argument.
    assert_eq!(
        out(
            "s = set_new()\nset_add(s, 1)\nset_add(s, 1.0)\nprint(set_remove(s, 1.0), set_to_array(s), set_has(s, 1), set_has(s, 1.0))\n"
        ),
        "true [1] true false\n"
    );
    assert_eq!(
        out(
            "nan = float(\"nan\", 0)\ns = set_new()\nset_add(s, nan)\nset_add(s, 2)\nprint(set_remove(s, nan), set_size(s), set_to_array(s))\n"
        ),
        "true 1 [2]\n"
    );
}

#[test]
fn invalid_utf8_is_one_character_wherever_characters_come_back() {
    let bad = "b = \"\u{e9}\"[0]\ns = \"a\" + b + \"c\"\nprint(split(s, \"\"))\nprint(len(split(s, \"\")), len(s))\nfor (c in s):\n    print(c == split(s, \"\")[1], len(c))\nend\n";
    let got = out(bad);
    assert!(
        got.starts_with("[\"a\", \"\u{FFFD}\", \"c\"]\n3 3\n"),
        "{got:?}"
    );
    // Cutting by a separator or at white space keeps the bytes.
    assert_eq!(
        out(
            "b = \"\u{e9}\"[0]\nprint(len(split(b + \" \" + b, \" \")[0]), len(split(b + \"x\" + b, \"x\")[1]))"
        ),
        "1 1\n"
    );
}

#[test]
fn print_writes_every_integer_and_an_empty_line_through_the_capture() {
    assert_eq!(
        out(
            "m = -9223372036854775807 - 1\nprint(m)\nprint(m, 1)\nprint([m])\nprint(float(m, 0))\nprint()\nprint()\n"
        ),
        "-9223372036854775808\n-9223372036854775808 1\n[-9223372036854775808]\n-9223372036854775808\n\n\n"
    );
    // The empty line counts as an operation and is held to the output limit like any other.
    let (ended, _) = run_with("while (true):\n    print()\nend\n", |_| {});
    assert_eq!(limit(&ended), Some(LimitReason::OpLimit));
    let profile = Profile::embedded();
    let script = Script::compile(
        b"i = 0\nwhile (i < 5000):\n    print()\n    i += 1\nend\n",
        &profile,
    )
    .expect("compiles");
    let mut limits = profile.limits();
    limits.capture_print = true;
    limits.deadline = None;
    limits.print_cap = 100;
    let mut runner = Runner::new();
    assert_eq!(
        runner.run(&script, &RequestVars::default(), &limits),
        Outcome::Ok
    );
    assert_eq!(
        (runner.printed().len(), runner.print_truncated()),
        (100, true)
    );
}

#[test]
fn int_gives_null_for_what_no_integer_holds() {
    assert_eq!(
        out(
            "nan = float(\"nan\", 0)\ninf = float(\"inf\", 0)\nprint(int(1e19), int(\"9223372036854775808\"), int(nan), int(inf), int(\"1e30\"), int(\"-1.9\"), int(\"9223372036854775807\"), int(-9.2e18))"
        ),
        "null null null null null -1 9223372036854775807 -9200000000000000000\n"
    );
}

#[test]
fn dates_read_and_write_the_layouts_of_the_script() {
    let script = "
print(date_parse(\"2023-12-25\", \"2006-01-02\"))
print(date_parse(\"25 Dec 2023 10:30 PM\", \"02 Jan 2006 03:04 PM\"))
print(date_parse(\"Monday, December 4, 2023 7:05:09 am\", \"Monday, January 2, 2006 3:04:05 pm\"))
print(date_parse(\"Mon, 25 Dec 2023 10:30:00 +0530\", \"Mon, 02 Jan 2006 15:04:05 -0700\"))
print(date_parse(\"2023-12-25 10:30:00.123456\", \"2006-01-02 15:04:05.000000\"))
print(date_parse(\"23-12-25 10:30 UTC\", \"06-01-02 15:04 MST\"))
print(date_parse(\"2023 359\", \"2006 002\"))
print(date_format_new(\"2023-12-25T10:30:00Z\", \"2006-01-02T15:04:05Z07:00\", \"January 2, 2006 3:04 PM\"))
print(date_format_new(\"25/12/2023\", \"02/01/2006\", \"Mon Jan _2 2006\"))
";
    assert_eq!(
        out(script),
        "2023-12-25T00:00:00Z\n2023-12-25T22:30:00Z\n2023-12-04T07:05:09Z\n2023-12-25T10:30:00+05:30\n2023-12-25T10:30:00Z\n2023-12-25T10:30:00Z\n2023-12-25T00:00:00Z\nDecember 25, 2023 10:30 AM\nMon Dec 25 2023\n"
    );
    let errors = "
print(date_parse(\"x\", \"2006-01-02\"))
print(date_parse(\"2023-13-01\", \"2006-01-02\"))
print(date_parse(\"2023-12-25\", \"2006-01-02 15\"))
print(date_parse(\"2023-02-30\", \"2006-01-02\"))
print(date_parse(1, \"2006\"))
print(date_parse(\"2006\"))
print(date_format_new(\"2023\", \"2006\"))
";
    assert_eq!(
        out(errors),
        "date_parse() failed to parse date: parsing time \"x\" as \"2006-01-02\": cannot parse \"x\" as \"2006\"\n\
         date_parse() failed to parse date: parsing time \"2023-13-01\": month out of range\n\
         date_parse() failed to parse date: parsing time \"2023-12-25\" as \"2006-01-02 15\": cannot parse \"\" as \"15\"\n\
         date_parse() failed to parse date: parsing time \"2023-02-30\": day out of range\n\
         date_parse() requires first argument to be a string\n\
         date_parse() requires exactly 2 arguments, got 1\n\
         date_format_new() requires exactly 3 arguments, got 2\n"
    );
}

#[test]
fn dates_move_by_a_duration_text_or_an_amount_of_a_unit() {
    let script = "
d = \"2023-12-25T10:30:00Z\"
print(date_add(d, \"24h\"), date_add(d, \"1h30m\"), date_add(d, \"-90m\"), date_add(d, \"1.5h\"))
print(date_subtract(d, \"1h30m\"), date_subtract(d, \"36h\"), date_subtract(d, \"0\"))
print(date_add(d, \"days\", 3), date_add(d, \"hours\", 1.9), date_add(d, \"months\", 2), date_add(d, \"year\", -1))
print(date_subtract(d, \"days\", 3), date_subtract(d, \"hours\", 1.9), date_subtract(\"2024-03-31\", \"months\", 1))
print(date_add(d, \"xyz\"))
print(date_add(d, \"5\"))
print(date_add(d, \"5x\"))
print(date_add(d))
print(date_add(d, \"days\", \"a\"))
print(date_add(d, \"fortnights\", 1))
print(date_add(d, \"days\", 1, 2))
print(date_subtract(d, 5))
";
    assert_eq!(
        out(script),
        "2023-12-26T10:30:00Z 2023-12-25T12:00:00Z 2023-12-25T09:00:00Z 2023-12-25T12:00:00Z\n\
         2023-12-25T09:00:00Z 2023-12-23T22:30:00Z 2023-12-25T10:30:00Z\n\
         2023-12-28T10:30:00Z 2023-12-25T11:30:00Z 2024-02-25T10:30:00Z 2022-12-25T10:30:00Z\n\
         2023-12-22T10:30:00Z 2023-12-25T09:30:00Z 2024-03-02T00:00:00Z\n\
         date_add: invalid duration format: time: invalid duration \"xyz\"\n\
         date_add: invalid duration format: time: missing unit in duration \"5\"\n\
         date_add: invalid duration format: time: unknown unit \"x\" in duration \"5x\"\n\
         date_add requires at least 2 arguments\n\
         date_add: third argument must be a number\n\
         date_add: unsupported unit 'fortnights'\n\
         date_add: invalid number of arguments\n\
         date_subtract: second argument must be a duration string\n"
    );
}

#[test]
fn the_date_builtins_run_under_every_limit() {
    let looped = "while (true):\n    date_parse(\"2023-12-25\", \"2006-01-02\")\n    date_add(\"2023-12-25\", \"1h30m\")\n    date_format_new(\"2023-12-25\", \"2006-01-02\", \"Jan 2\")\nend\n";
    let (ended, _) = run_with(looped, |_| {});
    assert_eq!(limit(&ended), Some(LimitReason::OpLimit));
    // Each answer is a string charged to the memory budget, so a loop that keeps them stops there.
    let keeps = "l = []\nwhile (true):\n    l = l + [date_format_new(\"2023-12-25\", \"2006-01-02\", \"January\" * 10000)]\nend\n";
    let (ended, _) = run_with(keeps, |l| l.max_ops = 1_000_000);
    assert_eq!(limit(&ended), Some(LimitReason::Memory));
    // A layout or a duration as long as a value may be is read in time, and one past that is not made.
    let long = "layout = \"2006-01-02 \" + \"x\" * 1000000\nd = \"2023-12-25 \" + \"x\" * 1000000\nprint(len(date_parse(d, layout)))\nprint(len(date_format_new(d, layout, layout)))\nprint(date_add(\"2023-12-25\", \"1ns\" * 300000))\n";
    let started = Instant::now();
    let (ended, printed) = run_with(long, |l| {
        l.deadline = Some(Duration::from_secs(20));
        l.max_alloc_bytes = 0;
    });
    assert_eq!(ended, Outcome::Ok, "{printed}");
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(printed, "20\n1000011\n2023-12-25T00:00:00Z\n");
    let (ended, _) = run_with("x = \"2006\" * 300000\ny = x + x\n", |l| {
        l.max_value_len = 1_000_000
    });
    assert_eq!(limit(&ended), Some(LimitReason::ValueSize));
}

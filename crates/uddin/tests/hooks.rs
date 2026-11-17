//! What the machine does around the call of a builtin: the checks before it, the memory it reserves and settles,
//! the error values it returns, and the clock a long call reads. The builtins here exist for these tests.

mod common;

use std::time::{Duration, Instant};

use common::{run, table};
use uddin::{LimitReason, Outcome, Profile};
use uddin_vm::{BuiltinDef, Count, Exc, Fresh, Rt, Stringify, Value};

fn said(rt: &mut Rt<'_>, what: &[u8]) {
    rt.out.write(what);
    rt.out.write(b"\n");
}

fn int(args: &[Value], i: usize) -> i64 {
    match args.get(i) {
        Some(&Value::Int(n)) => n,
        _ => 0,
    }
}

fn nothing(_: &mut Rt<'_>, _: &[Value]) -> Result<Value, Exc> {
    Ok(Value::Null)
}

/// Says that the call ran.
fn ran(rt: &mut Rt<'_>, _: &[Value]) -> Result<Value, Exc> {
    said(rt, b"call");
    Ok(Value::Null)
}

fn guard(rt: &mut Rt<'_>, args: &[Value]) -> Result<i64, Exc> {
    said(rt, b"guard");
    rt.size_check(int(args, 0))?;
    Ok(0)
}

fn guard_regex(rt: &mut Rt<'_>, args: &[Value]) -> Result<i64, Exc> {
    said(rt, b"regex");
    rt.reserve(int(args, 1))
}

fn reserve(rt: &mut Rt<'_>, args: &[Value]) -> Result<i64, Exc> {
    said(rt, b"reserve");
    rt.reserve(int(args, 2))
}

fn ints(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let (n, cap) = (int(args, 0) as usize, int(args, 1) as usize);
    let id = rt.heap.new_list(n, cap).map_err(rt.oom())?;
    for i in 0..n {
        rt.heap
            .list_push(id, Value::Int(i as i64))
            .map_err(rt.oom())?;
    }
    Ok(Value::List(id))
}

/// A list of `n` strings of two bytes each, made by the call.
fn pieces(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let n = int(args, 0) as usize;
    let id = rt.heap.new_list(n, n).map_err(rt.oom())?;
    for _ in 0..n {
        let s = rt.heap.new_str(b"ab").map_err(rt.oom())?;
        rt.heap.list_push(id, Value::Str(s)).map_err(rt.oom())?;
    }
    Ok(Value::List(id))
}

/// The pieces of a string between commas: each one lies inside the argument.
fn comma_parts(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let Some(&Value::Str(s)) = args.first() else {
        return Err(rt.type_error("a string is required"));
    };
    let bytes = rt.heap.str_bytes(s).to_vec();
    let mut parts = Vec::new();
    let mut from = 0;
    for (i, &b) in bytes.iter().enumerate().chain([(bytes.len(), &b',')]) {
        if b == b',' {
            parts.push(uddin_vm::Str {
                off: s.off + from as u32,
                len: (i - from) as u32,
            });
            from = i + 1;
        }
    }
    let id = rt
        .heap
        .new_list(parts.len(), parts.len())
        .map_err(rt.oom())?;
    for p in parts {
        rt.heap.list_push(id, Value::Str(p)).map_err(rt.oom())?;
    }
    Ok(Value::List(id))
}

/// `{"a": [1, "xy", 2.5], "b": {}}`, the list with room for four.
fn tree(rt: &mut Rt<'_>, _: &[Value]) -> Result<Value, Exc> {
    let oom = |rt: &Rt<'_>| rt.oom();
    let list = rt.heap.new_list(3, 4).map_err(oom(rt))?;
    let xy = rt.heap.new_str(b"xy").map_err(oom(rt))?;
    for v in [Value::Int(1), Value::Str(xy), Value::Float(2.5)] {
        rt.heap.list_push(list, v).map_err(oom(rt))?;
    }
    let inner = rt.heap.new_map(0).map_err(oom(rt))?;
    let outer = rt.heap.new_map(2).map_err(oom(rt))?;
    for (key, v) in [
        (uddin_vm::Str::byte(b'a'), Value::List(list)),
        (uddin_vm::Str::byte(b'b'), Value::Map(inner)),
    ] {
        rt.heap.map_set(outer, key, v).map_err(oom(rt))?;
    }
    Ok(Value::Map(outer))
}

/// An error value of the text given, or of `n` letters x.
fn error(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    match args.first() {
        Some(&Value::Str(s)) => {
            let text = rt.heap.str_bytes(s).to_vec();
            rt.error_value(&text)
        }
        Some(&Value::Int(n)) => rt.error_value(&vec![b'x'; n as usize]),
        _ => Err(rt.type_error("a string or an integer is required")),
    }
}

/// An error value whose text is not UTF-8.
fn raw_error(rt: &mut Rt<'_>, _: &[Value]) -> Result<Value, Exc> {
    rt.error_value(&[0xff, b'a', 0xc3])
}

fn spin(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let units = int(args, 0) as u64;
    for _ in 0..4_000_000_000u64 {
        rt.work(Count::Free, units)?;
    }
    Ok(Value::Null)
}

/// A call that fails after its hooks have run.
fn fails(rt: &mut Rt<'_>, _: &[Value]) -> Result<Value, Exc> {
    Err(rt.type_error("the call failed"))
}

fn past_the_deadline() {
    std::thread::sleep(Duration::from_millis(40));
}

/// Waits until the deadline has passed, then says how many units of work it takes to notice.
fn late(rt: &mut Rt<'_>, _: &[Value]) -> Result<Value, Exc> {
    past_the_deadline();
    for i in 1..=5000 {
        if rt.work(Count::Free, 1).is_err() {
            said(rt, i.to_string().as_bytes());
            return Ok(Value::Null);
        }
    }
    Err(rt.type_error("the deadline was not seen"))
}

/// Compares two values after the deadline has passed.
fn late_equal(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    past_the_deadline();
    Ok(Value::Bool(rt.equal(args[0], args[1])?))
}

fn late_less(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    past_the_deadline();
    Ok(Value::Bool(rt.less(args[0], args[1])?))
}

/// Writes the text of a value after the deadline has passed.
fn late_text(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    past_the_deadline();
    let mut out = Vec::new();
    uddin_vm::to_text(rt.env, rt.heap, rt.meter, args[0], false, 0, &mut out)?;
    Ok(Value::Null)
}

fn ops(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    for _ in 0..int(args, 0) {
        rt.work(Count::Op, 0)?;
    }
    Ok(Value::Null)
}

fn free(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    for _ in 0..int(args, 0) {
        rt.work(Count::Free, 1)?;
    }
    Ok(Value::Null)
}

fn equal(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    Ok(Value::Bool(rt.equal(args[0], args[1])?))
}

fn less(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    Ok(Value::Bool(rt.less(args[0], args[1])?))
}

fn builtins() -> Vec<BuiltinDef> {
    table(&[
        BuiltinDef::new("t_hooked", ran, 3)
            .guard(guard)
            .guard_regex(guard_regex)
            .reserve(reserve),
        BuiltinDef::new("t_fails", fails, 3)
            .guard(guard)
            .guard_regex(guard_regex)
            .reserve(reserve),
        BuiltinDef::new("t_ints", ints, 2).fresh(Fresh::Elements),
        BuiltinDef::new("t_ints_shallow", ints, 2),
        BuiltinDef::new("t_pieces", pieces, 1).fresh(Fresh::Elements),
        BuiltinDef::new("t_parts", comma_parts, 1).fresh(Fresh::Elements),
        BuiltinDef::new("t_tree", tree, 0).fresh(Fresh::Tree),
        BuiltinDef::new("t_tree_shallow", tree, 0),
        BuiltinDef::new("t_error", error, 1),
        BuiltinDef::new("t_raw_error", raw_error, 0),
        BuiltinDef::new("t_deep", ran, 1).deep(),
        BuiltinDef::new("t_plain", ran, 1),
        BuiltinDef::new("t_text", ran, 1).stringify(Stringify::Text),
        BuiltinDef::new("t_json", ran, 1).stringify(Stringify::Json),
        BuiltinDef::new("t_spin", spin, 1),
        BuiltinDef::new("t_late", late, 0),
        BuiltinDef::new("t_late_eq", late_equal, 2),
        BuiltinDef::new("t_late_lt", late_less, 2),
        BuiltinDef::new("t_late_text", late_text, 1),
        BuiltinDef::new("t_ops", ops, 1),
        BuiltinDef::new("t_free", free, 1),
        BuiltinDef::new("t_eq", equal, 2),
        BuiltinDef::new("t_lt", less, 2),
        BuiltinDef::new("t_nothing", nothing, 0),
    ])
}

fn limit(ended: &Outcome) -> Option<LimitReason> {
    match ended {
        Outcome::Limit { reason, .. } => Some(*reason),
        _ => None,
    }
}

#[test]
fn hooks_run_in_order_and_a_refusal_stops_the_rest() {
    let t = builtins();
    let r = run(&t, "t_hooked(1, 0, 0)\n", |_| {});
    assert_eq!(r.ended, Outcome::Ok);
    assert_eq!(r.text(), "guard\nregex\nreserve\ncall\n");
    let r = run(&t, "t_hooked(-1, 0, 0)\n", |_| {});
    assert_eq!(limit(&r.ended), Some(LimitReason::ValueSize));
    assert_eq!(r.text(), "guard\n");
    let r = run(&t, "t_hooked(1, 3000000, 2000000)\n", |_| {});
    assert_eq!(limit(&r.ended), Some(LimitReason::Memory));
    assert_eq!(r.text(), "guard\nregex\nreserve\n");
    let r = run(&t, "t_hooked(1, 4194305, 0)\n", |_| {});
    assert_eq!(limit(&r.ended), Some(LimitReason::Memory));
    assert_eq!(r.text(), "guard\nregex\n");
}

#[test]
fn an_overflowed_reservation_stops_the_run() {
    let r = run(&builtins(), "t_hooked(1, -1, 0)\n", |_| {});
    assert_eq!(limit(&r.ended), Some(LimitReason::Memory));
}

#[test]
fn the_limits_decide_which_hooks_run() {
    let t = builtins();
    let r = run(&t, "t_hooked(-5, 1, 1)\n", |l| l.max_value_len = 0);
    assert_eq!(r.ended, Outcome::Ok);
    assert_eq!(r.text(), "regex\nreserve\ncall\n");
    let r = run(&t, "t_hooked(1, 1, 1)\n", |l| l.max_alloc_bytes = 0);
    assert_eq!(r.ended, Outcome::Ok);
    assert_eq!(r.text(), "guard\nregex\ncall\n");
}

#[test]
fn what_a_hook_reserved_is_given_back_after_the_call() {
    let t = builtins();
    // Four strings of 1 MiB do not fit in 4 MiB; three do, so the 4,000,000 bytes reserved must be gone.
    let strings = "a = \"a\" * 1048576\nb = \"b\" * 1048576\nc = \"c\" * 1048576\n";
    let r = run(
        &t,
        &format!("t_hooked(1, 2000000, 2000000)\n{strings}"),
        |_| {},
    );
    assert_eq!(r.ended, Outcome::Ok);
    let r = run(
        &t,
        &format!("t_hooked(1, 2000000, 2000000)\n{strings}d = \"d\" * 1048576\n"),
        |_| {},
    );
    assert_eq!(limit(&r.ended), Some(LimitReason::Memory));
    // The three strings cost 3 * 1,048,592; the reservation is added to them while the call runs.
    let used = 3 * (1 << 20) + 48;
    let src = format!("{strings}t_hooked(1, 0, 100000)\n");
    let r = run(&t, &src, |l| l.max_alloc_bytes = used + 100_000);
    assert_eq!(r.ended, Outcome::Ok);
    let r = run(&t, &src, |l| l.max_alloc_bytes = used + 100_000 - 1);
    assert_eq!(limit(&r.ended), Some(LimitReason::Memory));
}

#[test]
fn a_reservation_stays_when_the_call_fails() {
    let t = builtins();
    let two = "a = \"a\" * 1048576\nb = \"b\" * 1048576\n";
    let after =
        |call: &str| format!("try:\n    {call}\ncatch (e):\n    print(\"caught\")\nend\n{two}");
    // A call that returns settles its 3,000,000 bytes; one that fails does not, and two strings do not fit.
    let r = run(&t, &after("t_hooked(1, 0, 3000000)"), |_| {});
    assert_eq!(r.ended, Outcome::Ok);
    let r = run(&t, &after("t_fails(1, 0, 3000000)"), |_| {});
    assert_eq!(limit(&r.ended), Some(LimitReason::Memory));
    assert!(
        r.text().starts_with("guard\nregex\nreserve\ncaught\n"),
        "{}",
        r.text()
    );
}

fn list_cost(n: i64, cap: i64) -> i64 {
    24 + 16 * cap + 8 * n
}

#[test]
fn a_result_list_is_charged_for_its_capacity_and_its_numbers() {
    let t = builtins();
    for (n, cap) in [(0, 0), (3, 3), (3, 16), (1000, 1000), (1000, 1024)] {
        let cost = list_cost(n, cap);
        let src = format!("x = t_ints({n}, {cap})\n");
        assert_eq!(
            run(&t, &src, |l| l.max_alloc_bytes = cost as usize).ended,
            Outcome::Ok
        );
        let r = run(&t, &src, |l| l.max_alloc_bytes = cost as usize - 1);
        assert_eq!(limit(&r.ended), Some(LimitReason::Memory), "{n} {cap}");
        // Without the flag the numbers are not charged.
        let src = format!("x = t_ints_shallow({n}, {cap})\n");
        let shallow = 24 + 16 * cap;
        assert_eq!(
            run(&t, &src, |l| l.max_alloc_bytes = shallow as usize).ended,
            Outcome::Ok
        );
        let r = run(&t, &src, |l| l.max_alloc_bytes = shallow as usize - 1);
        assert_eq!(limit(&r.ended), Some(LimitReason::Memory));
    }
}

#[test]
fn a_string_a_call_made_costs_its_bytes_and_one_it_shares_does_not() {
    let t = builtins();
    // Ten strings of two bytes: 24 + 16 * 10 for the list and 10 * (16 + 2).
    let cost = 24 + 16 * 10 + 10 * 18;
    let src = "x = t_pieces(10)\n";
    assert_eq!(
        run(&t, src, |l| l.max_alloc_bytes = cost).ended,
        Outcome::Ok
    );
    let r = run(&t, src, |l| l.max_alloc_bytes = cost - 1);
    assert_eq!(limit(&r.ended), Some(LimitReason::Memory));
    // The parts of a string that lie inside an argument cost a header each.
    let src = "x = t_parts(\"ab,cd,ef\")\n";
    let cost = 24 + 16 * 3 + 3 * 16;
    assert_eq!(
        run(&t, src, |l| l.max_alloc_bytes = cost).ended,
        Outcome::Ok
    );
    let r = run(&t, src, |l| l.max_alloc_bytes = cost - 1);
    assert_eq!(limit(&r.ended), Some(LimitReason::Memory));
    let r = run(
        &t,
        "x = t_parts(\"ab,cd,ef\")\nprint(len(x), x[1])\n",
        |_| {},
    );
    assert_eq!(r.text(), "3 cd\n");
}

#[test]
fn a_tree_a_call_built_is_charged_node_by_node() {
    let t = builtins();
    // The map 48, its two entries 2 * (64 + 1), the list 24 + 16 * 4 with 8 + (16 + 2) + 8 inside, the empty map 48.
    let cost = 48 + 2 * 65 + (24 + 64) + 8 + 18 + 8 + 48;
    let src = "x = t_tree()\n";
    assert_eq!(
        run(&t, src, |l| l.max_alloc_bytes = cost).ended,
        Outcome::Ok
    );
    let r = run(&t, src, |l| l.max_alloc_bytes = cost - 1);
    assert_eq!(limit(&r.ended), Some(LimitReason::Memory));
    // Without the flag the map costs its own header and entries.
    let shallow = 48 + 2 * 64;
    let src = "x = t_tree_shallow()\n";
    assert_eq!(
        run(&t, src, |l| l.max_alloc_bytes = shallow).ended,
        Outcome::Ok
    );
    let r = run(&t, src, |l| l.max_alloc_bytes = shallow - 1);
    assert_eq!(limit(&r.ended), Some(LimitReason::Memory));
}

#[test]
fn a_deep_builtin_walks_its_arguments_first() {
    let t = builtins();
    let cycle = "x = [1]\nx[0] = x\n";
    let r = run(&t, &format!("{cycle}t_deep(x)\n"), |_| {});
    assert_eq!(limit(&r.ended), Some(LimitReason::ValueDepth));
    assert_eq!(r.text(), "");
    let r = run(&t, &format!("{cycle}t_plain(x)\n"), |_| {});
    assert_eq!(r.ended, Outcome::Ok);
    assert_eq!(r.text(), "call\n");
}

#[test]
fn text_is_bounded_before_the_call() {
    let t = builtins();
    let big = |n: u32| format!("s = \"a\" * {n}\n");
    // Three strings of 400,000 bytes inside a list are quoted: over a mebibyte.
    let r = run(&t, "s = \"a\" * 400000\nt_text([s, s, s])\n", |_| {});
    assert_eq!(limit(&r.ended), Some(LimitReason::ValueSize));
    assert_eq!(r.text(), "");
    // A string at the top is not quoted for text, and is for JSON.
    let r = run(&t, &format!("{}t_text(s)\n", big(1_048_575)), |_| {});
    assert_eq!(r.ended, Outcome::Ok);
    let r = run(&t, &format!("{}t_json(s)\n", big(1_048_575)), |_| {});
    assert_eq!(limit(&r.ended), Some(LimitReason::ValueSize));
    let r = run(&t, &format!("{}t_json(s)\n", big(1_048_574)), |_| {});
    assert_eq!(r.ended, Outcome::Ok);
    // JSON also escapes what text does not.
    let r = run(&t, "s = \"<\" * 200000\nt_json(s)\n", |_| {});
    assert_eq!(limit(&r.ended), Some(LimitReason::ValueSize));
    let r = run(&t, "s = \"<\" * 200000\nt_text(s)\n", |_| {});
    assert_eq!(r.ended, Outcome::Ok);
}

#[test]
fn an_error_value_is_cut_to_a_caught_errors_length() {
    let t = builtins();
    let len_of = |n: usize| {
        let r = run(
            &t,
            &format!("x = t_error({n})\nprint(len(str(x)))\n"),
            |_| {},
        );
        assert_eq!(r.ended, Outcome::Ok);
        r.text().trim().parse::<usize>().expect("a number")
    };
    assert_eq!(len_of(0), 0);
    assert_eq!(len_of(1024), 1024);
    assert_eq!(len_of(1025), 1027);
    assert_eq!(len_of(5000), 1027);
    let r = run(&t, "x = t_error(5000)\nprint(len(str(x)))\n", |l| {
        l.max_alloc_bytes = 0
    });
    assert_eq!(r.text(), "5000\n");
    // The cut lands on a rune boundary: a letter and then two-byte runes, so byte 1024 is inside one.
    let src = "x = t_error(\"a\" + \"\u{e9}\" * 600)\ns = str(x)\nprint(len(s), s[len(s) - 1])\n";
    let r = run(&t, src, |_| {});
    assert_eq!(r.text(), "1026 .\n");
}

#[test]
fn an_error_value_costs_its_text_and_a_header() {
    let t = builtins();
    let src = "x = t_error(5000)\nprint(1)\n";
    let cost = 16 + 1027;
    assert_eq!(
        run(&t, src, |l| l.max_alloc_bytes = cost).ended,
        Outcome::Ok
    );
    let r = run(&t, src, |l| l.max_alloc_bytes = cost - 1);
    assert_eq!(limit(&r.ended), Some(LimitReason::Memory));
    let src = "x = t_error(30)\nprint(1)\n";
    assert_eq!(run(&t, src, |l| l.max_alloc_bytes = 46).ended, Outcome::Ok);
    let r = run(&t, src, |l| l.max_alloc_bytes = 45);
    assert_eq!(limit(&r.ended), Some(LimitReason::Memory));
}

#[test]
fn an_error_value_is_raised_by_if_and_while_with_its_text() {
    let t = builtins();
    for src in [
        "x = t_error(\"boom\")\nif (x) then:\nend\n",
        "x = t_error(\"boom\")\nwhile (x):\nend\n",
    ] {
        let r = run(&t, src, |_| {});
        assert_eq!(
            r.ended,
            Outcome::Error {
                class: "runtime",
                text: format!(
                    "runtime error at 2:{}: unexpected runtime error: boom",
                    if src.contains("if") { 5 } else { 8 }
                )
            }
        );
    }
    // The text goes through the host-error mapping a raised fault goes through.
    let r = run(
        &t,
        "x = t_error(\"index out of range [5] with length 3\")\nif (x) then:\nend\n",
        |_| {},
    );
    assert_eq!(
        r.ended,
        Outcome::Error {
            class: "runtime",
            text: "runtime error at 2:5: array or string index is out of bounds".into()
        }
    );
}

#[test]
fn an_error_value_keeps_bytes_that_are_not_text() {
    let t = builtins();
    let r = run(
        &t,
        "x = t_raw_error()\nprint(x)\ntry:\n    if (x) then:\n    end\ncatch (e):\n    print(e)\nend\n",
        |_| {},
    );
    assert_eq!(r.ended, Outcome::Ok);
    assert_eq!(r.printed, b"\xffa\xc3\n\xffa\xc3\n");
    let r = run(&t, "x = t_raw_error()\nif (x) then:\nend\n", |_| {});
    assert_eq!(
        r.ended,
        Outcome::Error {
            class: "runtime",
            text: "runtime error at 2:5: unexpected runtime error: \u{fffd}a\u{fffd}".into()
        }
    );
}

#[test]
fn comparing_and_ordering_through_the_run() {
    let t = builtins();
    let r = run(
        &t,
        "print(t_eq([1, [2]], [1, [2]]), t_eq([1], [2]), t_eq({a: 1}, {a: 1}), t_lt([1, 2], [1, 3]), t_lt(\"b\", \"a\"))\nx = t_error(\"e\")\nprint(t_eq(x, x))\n",
        |_| {},
    );
    assert_eq!(r.text(), "true false true true false\nfalse\n");
}

#[test]
fn a_long_call_stops_at_the_deadline() {
    let t = builtins();
    for units in [1, 100, 1 << 20] {
        let start = Instant::now();
        let r = run(&t, &format!("t_spin({units})\n"), |l| {
            l.deadline = Some(Duration::from_millis(20));
            l.max_ops = 0;
        });
        assert_eq!(limit(&r.ended), Some(LimitReason::Canceled), "{units}");
        assert!(start.elapsed() < Duration::from_secs(5), "{units}");
    }
}

#[test]
fn the_clock_is_read_every_1024_units_of_work() {
    let t = builtins();
    let r = run(&t, "t_late()\n", |l| {
        l.deadline = Some(Duration::from_millis(10));
    });
    // The deadline passed before the first unit; the run counts from zero, so the 1,024th unit sees it.
    assert_eq!(r.ended, Outcome::Ok);
    assert_eq!(r.text(), "1024\n");
}

#[test]
fn work_counts_as_operations_only_when_it_says_so() {
    let t = builtins();
    // One operation for the statement, one for the call and one for the argument.
    let base = run(&t, "t_free(500)\n", |_| {}).ops;
    let r = run(&t, "t_ops(500)\n", |l| l.max_ops = 1000);
    assert_eq!(r.ended, Outcome::Ok);
    assert_eq!(r.ops, base + 500);
    let r = run(&t, "t_ops(2000)\n", |l| l.max_ops = 1000);
    assert_eq!(limit(&r.ended), Some(LimitReason::OpLimit));
    let r = run(&t, "t_free(5000)\n", |l| l.max_ops = 1000);
    assert_eq!(r.ended, Outcome::Ok);
    assert_eq!(r.ops, base);
}

#[test]
fn comparing_and_writing_text_stop_at_the_deadline() {
    let t = builtins();
    let list = format!("[{}0]", "0, ".repeat(2999));
    let other = format!("[{}0]", "0, ".repeat(2999));
    let src = |call: &str| format!("x = {list}\ny = {other}\n{call}\n");
    let past = |l: &mut uddin::Limits| l.deadline = Some(Duration::from_millis(10));
    for call in ["t_late_eq(x, y)", "t_late_lt(x, y)", "t_late_text(x)"] {
        let r = run(&t, &src(call), past);
        assert_eq!(limit(&r.ended), Some(LimitReason::Canceled), "{call}");
    }
    // A short list is far under 1,024 units, and the clock is not read.
    let r = run(&t, "t_late_eq([1, 2], [1, 2])\n", past);
    assert_eq!(r.ended, Outcome::Ok);
}

#[test]
fn the_machines_loops_over_input_read_the_clock() {
    let t = builtins();
    let long = "a".repeat(3000);
    let list = format!("[{}0]", "0, ".repeat(599));
    // A deadline that is over before the run starts: only a step that reports its work can notice it, since
    // none of these scripts counts 256 operations or calls a builtin.
    for src in [
        "x = \"a\" * 5000\n".to_owned(),
        "x = [0] * 5000\n".to_owned(),
        format!("s = \"{long}\"\nx = s + s\n"),
        format!("l = {list}\nx = l + l\n"),
        format!("s = \"{long}\"\nx = \"b\" in s\n"),
    ] {
        let over = run(&t, &src, |l| l.deadline = Some(Duration::ZERO));
        assert_eq!(limit(&over.ended), Some(LimitReason::Canceled), "{src:.40}");
        assert_eq!(run(&t, &src, |_| {}).ended, Outcome::Ok, "{src:.40}");
    }
}

#[test]
fn the_table_has_the_builtins_of_the_build_first() {
    let t = builtins();
    let names: Vec<&str> = t.iter().take(4).map(|d| d.name).collect();
    assert_eq!(names, ["len", "print", "str", "typeof"]);
}

#[test]
fn every_registered_builtin_is_in_the_profile() {
    let profile = Profile::embedded();
    for def in uddin_std::builtins() {
        assert!(
            profile.allows(def.name),
            "{} is not in the profile",
            def.name
        );
    }
}

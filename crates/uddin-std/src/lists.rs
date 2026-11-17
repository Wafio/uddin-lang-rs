//! Lists: append push pop shift unshift reverse slice range index_of last_index_of map filter reduce.
//!
//! A list is a Go slice: `append` and `push` write in place while the capacity lasts and move the list to a new
//! array when it does not, `pop` and `shift` shrink the window, `unshift` always builds a new array. The heap
//! keeps the header (`Heap::list_append` and the others), and the machine charges the growth after the call.

use uddin_vm::{
    BuiltinDef, Count, Exc, Fresh, LIST_COST, RANGE_ARG_BOUND, RANGE_ELEM_COST, Rt, Str, Value,
    checked_product, sum_estimate, truthy,
};

pub(crate) const TABLE: &[BuiltinDef] = &[
    BuiltinDef::new("append", append, -1),
    BuiltinDef::new("push", push, 2).fast_path(),
    BuiltinDef::new("pop", pop, 1).fast_path(),
    BuiltinDef::new("shift", shift, 1).fast_path(),
    BuiltinDef::new("unshift", unshift, 2).fast_path(),
    BuiltinDef::new("reverse", reverse, 1).fast_path(),
    BuiltinDef::new("slice", slice, 3),
    BuiltinDef::new("range", range, -1)
        .guard(range_guard)
        .reserve(range_reserve)
        .fresh(Fresh::Elements),
    BuiltinDef::new("index_of", index_of, 2).deep(),
    BuiltinDef::new("last_index_of", last_index_of, 2).deep(),
    BuiltinDef::new("map", map, 2).calls(),
    BuiltinDef::new("filter", filter, 2).calls(),
    BuiltinDef::new("reduce", reduce, 3).calls(),
];

/// The count check of the builtins that carry their own: `name() requires N arg(s), got M`.
fn exactly(rt: &Rt<'_>, name: &str, args: &[Value], required: usize) -> Result<(), Exc> {
    if args.len() == required {
        Ok(())
    } else {
        Err(rt.arity_error(name, required, args.len()))
    }
}

fn append(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let Some(&first) = args.first() else {
        return Err(rt.type_error("append() requires at least 1 arg, got 0"));
    };
    let Value::List(list) = first else {
        return Err(rt.type_error("append() requires first argument to be list"));
    };
    if args.len() > 1 {
        rt.heap.list_append(list, &args[1..]).map_err(rt.oom())?;
    }
    Ok(Value::Null)
}

fn push(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    if args.len() < 2 {
        return Err(rt.type_error(format!(
            "push() requires at least 2 arguments, got {}",
            args.len()
        )));
    }
    let Value::List(list) = args[0] else {
        return Err(rt.type_error("push() requires first argument to be an array"));
    };
    rt.heap.list_append(list, &args[1..]).map_err(rt.oom())?;
    Ok(Value::Null)
}

fn pop(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    exactly(rt, "pop", args, 1)?;
    let Value::List(list) = args[0] else {
        return Err(rt.type_error("pop() requires an array argument"));
    };
    Ok(rt.heap.list_pop(list).unwrap_or(Value::Null))
}

fn shift(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    exactly(rt, "shift", args, 1)?;
    let Value::List(list) = args[0] else {
        return Err(rt.type_error("shift() requires an array argument"));
    };
    Ok(rt.heap.list_shift(list).unwrap_or(Value::Null))
}

fn unshift(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    if args.len() < 2 {
        return Err(rt.type_error(format!(
            "unshift() requires at least 2 arguments, got {}",
            args.len()
        )));
    }
    let Value::List(list) = args[0] else {
        return Err(rt.type_error("unshift() requires first argument to be an array"));
    };
    rt.heap.list_unshift(list, &args[1..]).map_err(rt.oom())?;
    Ok(Value::Null)
}

fn reverse(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    exactly(rt, "reverse", args, 1)?;
    let Value::List(list) = args[0] else {
        return Err(rt.type_error("reverse() requires an array argument"));
    };
    rt.work(Count::Free, rt.heap.list(list).items.len() as u64)?;
    rt.heap.list_mut(list).items.reverse();
    Ok(Value::Null)
}

/// `slice(value, start, end)` of a string, which shares the bytes of its argument, or of a list, which copies.
fn slice(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let (Value::Int(start), Value::Int(end)) = (args[1], args[2]) else {
        return Err(rt.type_error("slice() requires start and end to be integers"));
    };
    let out_of_range = |len: i64| start < 0 || end > len || start > end;
    let pos = rt.pos;
    let bounds = move || Exc::value_error(pos, "slice() start or end out of bounds");
    match args[0] {
        Value::Str(s) => {
            if out_of_range(i64::from(s.len)) {
                return Err(bounds());
            }
            Ok(Value::Str(Str {
                off: s.off + start as u32,
                len: (end - start) as u32,
            }))
        }
        Value::List(list) => {
            if out_of_range(rt.heap.list(list).items.len() as i64) {
                return Err(bounds());
            }
            let n = (end - start) as usize;
            rt.work(Count::Free, n as u64)?;
            let copy = rt.heap.new_list(n, n).map_err(rt.oom())?;
            for i in start as usize..end as usize {
                let v = rt.heap.list(list).items[i];
                rt.heap.list_push(copy, v).map_err(rt.oom())?;
            }
            Ok(Value::List(copy))
        }
        _ => Err(rt.type_error("slice() requires first argument to be a str or array")),
    }
}

/// Refuses a range that would be longer than a value may be, before anything is allocated.
fn range_guard(rt: &mut Rt<'_>, args: &[Value]) -> Result<i64, Exc> {
    let mut ints = Vec::new();
    for a in args {
        let Value::Int(n) = *a else {
            return Ok(0);
        };
        if !(-RANGE_ARG_BOUND..=RANGE_ARG_BOUND).contains(&n) {
            rt.size_check(-1)?;
        }
        ints.push(n);
    }
    let n = match ints[..] {
        [n] => n,
        [a, b] => b - a,
        [a, b, step] if step != 0 => (b - a) / step,
        _ => 0,
    };
    if n > 0 {
        rt.size_check(n)?;
    }
    Ok(0)
}

/// How many elements `range(args...)` makes: none when an argument is not an integer.
fn range_len(args: &[Value]) -> i64 {
    if args.is_empty() || args.len() > 3 {
        return 0;
    }
    let mut ints = [0i64; 3];
    for (i, a) in args.iter().enumerate() {
        let Value::Int(n) = *a else {
            return 0;
        };
        if !(-RANGE_ARG_BOUND..=RANGE_ARG_BOUND).contains(&n) {
            return 0;
        }
        ints[i] = n;
    }
    let n = match args.len() {
        1 => ints[0],
        2 => ints[1] - ints[0],
        _ if ints[2] != 0 => {
            let span = ints[1] - ints[0];
            span / ints[2] + i64::from(span % ints[2] != 0)
        }
        _ => 0,
    };
    n.max(0)
}

fn range_reserve(rt: &mut Rt<'_>, args: &[Value]) -> Result<i64, Exc> {
    let n = range_len(args);
    let estimate = if n > 0 {
        sum_estimate(LIST_COST, checked_product(RANGE_ELEM_COST, n))
    } else {
        0
    };
    rt.reserve(estimate)
}

/// The integers from `start` to `stop` by `step`: a list of exactly its length, as the reference makes it, or,
/// for a stepped range of at most 100 elements, one grown element by element.
fn range(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    match *args {
        [first] => {
            let Value::Int(n) = first else {
                return Err(rt.type_error("range() requires an integer"));
            };
            if n < 0 {
                return Err(Exc::value_error(
                    rt.pos,
                    "range() argument must not be negative",
                ));
            }
            counting(rt, 0, n)
        }
        [first, second] => {
            let (Value::Int(start), Value::Int(stop)) = (first, second) else {
                return Err(rt.type_error("range() requires integer arguments"));
            };
            counting(rt, start, stop)
        }
        [first, second, third] => {
            let (Value::Int(start), Value::Int(stop), Value::Int(step)) = (first, second, third)
            else {
                return Err(rt.type_error("range() requires integer arguments"));
            };
            if step == 0 {
                return Err(Exc::value_error(
                    rt.pos,
                    "range() step argument must not be zero",
                ));
            }
            stepping(rt, start, stop, step)
        }
        _ => Err(Exc::value_error(
            rt.pos,
            format!("range() requires 1, 2, or 3 arguments, got {}", args.len()),
        )),
    }
}

fn counting(rt: &mut Rt<'_>, start: i64, stop: i64) -> Result<Value, Exc> {
    let n = if start >= stop {
        0
    } else {
        stop.wrapping_sub(start) as usize
    };
    let id = rt.heap.new_list(n, n).map_err(rt.oom())?;
    for i in 0..n {
        rt.work(Count::Free, 1)?;
        rt.heap
            .list_push(id, Value::Int(start.wrapping_add(i as i64)))
            .map_err(rt.oom())?;
    }
    Ok(Value::List(id))
}

fn stepping(rt: &mut Rt<'_>, start: i64, stop: i64, step: i64) -> Result<Value, Exc> {
    let estimate = if step > 0 {
        stop.wrapping_sub(start).wrapping_add(step - 1) / step
    } else {
        start.wrapping_sub(stop).wrapping_sub(step).wrapping_sub(1) / step.wrapping_neg()
    }
    .max(0);
    let id = rt.heap.new_list(0, 0).map_err(rt.oom())?;
    let mut i = start;
    let mut made = 0;
    while made <= estimate && (step > 0 && i < stop || step < 0 && i > stop) {
        rt.work(Count::Free, 1)?;
        if estimate > 100 {
            rt.heap.list_push(id, Value::Int(i)).map_err(rt.oom())?;
        } else {
            rt.heap
                .list_append(id, &[Value::Int(i)])
                .map_err(rt.oom())?;
        }
        made += 1;
        i = i.wrapping_add(step);
    }
    if estimate > 100 {
        rt.heap.list_mut(id).go_cap = made as usize;
    }
    Ok(Value::List(id))
}

fn index_of(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let Value::List(list) = args[0] else {
        return Err(rt.type_error("index_of() requires first argument to be an array"));
    };
    for i in 0..rt.heap.list(list).items.len() {
        rt.work(Count::Free, 1)?;
        let v = rt.heap.list(list).items[i];
        if rt.equal(v, args[1])? {
            return Ok(Value::Int(i as i64));
        }
    }
    Ok(Value::Int(-1))
}

fn last_index_of(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let Value::List(list) = args[0] else {
        return Err(rt.type_error("last_index_of() requires first argument to be an array"));
    };
    for i in (0..rt.heap.list(list).items.len()).rev() {
        rt.work(Count::Free, 1)?;
        let v = rt.heap.list(list).items[i];
        if rt.equal(v, args[1])? {
            return Ok(Value::Int(i as i64));
        }
    }
    Ok(Value::Int(-1))
}

/// The function argument of a builtin that calls back, or the type error of its message.
fn function_arg(rt: &Rt<'_>, name: &str, v: Value) -> Result<Value, Exc> {
    match v {
        Value::Func(_) | Value::Builtin(_) => Ok(v),
        _ => Err(rt.type_error(format!(
            "{name}() requires second argument to be a function"
        ))),
    }
}

/// The result of `map`: a list of exactly the length of the one it read, whose header was taken before the
/// first call, so what the function does to the list does not change how far or where `map` reads.
fn map(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let Value::List(list) = args[0] else {
        return Err(rt.type_error("map() requires first argument to be an array"));
    };
    let f = function_arg(rt, "map", args[1])?;
    let header = rt.heap.snapshot(list);
    let out = rt.heap.new_list(header.len, header.len).map_err(rt.oom())?;
    for i in 0..header.len {
        let v = rt.heap.header_item(&header, i);
        let r = rt.call(f, &[v])?;
        rt.heap.list_push(out, r).map_err(rt.oom())?;
    }
    Ok(Value::List(out))
}

/// The elements for which the function returns something truthy, in a list that grows by `append`.
fn filter(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let Value::List(list) = args[0] else {
        return Err(rt.type_error("filter() requires first argument to be an array"));
    };
    let f = function_arg(rt, "filter", args[1])?;
    let header = rt.heap.snapshot(list);
    let out = rt.heap.new_list(0, 0).map_err(rt.oom())?;
    for i in 0..header.len {
        let v = rt.heap.header_item(&header, i);
        let keep = rt.call(f, &[v])?;
        if truthy(rt.heap, keep) {
            rt.heap.list_append(out, &[v]).map_err(rt.oom())?;
        }
    }
    Ok(Value::List(out))
}

fn reduce(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let Value::List(list) = args[0] else {
        return Err(rt.type_error("reduce() requires first argument to be an array"));
    };
    let f = function_arg(rt, "reduce", args[1])?;
    let header = rt.heap.snapshot(list);
    let mut acc = args[2];
    for i in 0..header.len {
        let v = rt.heap.header_item(&header, i);
        acc = rt.call(f, &[acc, v])?;
    }
    Ok(acc)
}

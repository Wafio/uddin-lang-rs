//! Sets, stacks and queues: set_new set_add set_has set_remove set_size set_to_array stack_new stack_push
//! stack_pop stack_peek stack_size stack_empty queue_new queue_enqueue queue_dequeue queue_front queue_size
//! queue_empty.
//!
//! A set keeps its members under a text key that says the kind of the member (`i:1` and `f:1` are two members),
//! and in the order they were added. A stack and a queue are lists that no script can name: a push is an
//! `append`, a pop shortens the window, a dequeue moves its start, so they cost what those cost.

use uddin_vm::{BuiltinDef, Count, Exc, Rt, SetId, Value, format_float_g, format_int};

pub(crate) const TABLE: &[BuiltinDef] = &[
    BuiltinDef::new("set_new", set_new, 0),
    BuiltinDef::new("set_add", set_add, 2).fast_path(),
    BuiltinDef::new("set_remove", set_remove, 2)
        .fast_path()
        .deep(),
    BuiltinDef::new("set_has", set_has, 2).fast_path(),
    BuiltinDef::new("set_size", set_size, 1).fast_path(),
    BuiltinDef::new("set_to_array", set_to_array, 1),
    BuiltinDef::new("stack_new", stack_new, 0),
    BuiltinDef::new("stack_push", stack_push, 2).fast_path(),
    BuiltinDef::new("stack_pop", stack_pop, 1).fast_path(),
    BuiltinDef::new("stack_peek", stack_peek, 1).fast_path(),
    BuiltinDef::new("stack_size", stack_size, 1).fast_path(),
    BuiltinDef::new("stack_empty", stack_empty, 1).fast_path(),
    BuiltinDef::new("queue_new", queue_new, 0),
    BuiltinDef::new("queue_enqueue", queue_enqueue, 2).fast_path(),
    BuiltinDef::new("queue_dequeue", queue_dequeue, 1).fast_path(),
    BuiltinDef::new("queue_front", queue_front, 1).fast_path(),
    BuiltinDef::new("queue_size", queue_size, 1).fast_path(),
    BuiltinDef::new("queue_empty", queue_empty, 1).fast_path(),
];

fn exactly(rt: &Rt<'_>, name: &str, args: &[Value], required: usize) -> Result<(), Exc> {
    if args.len() == required {
        Ok(())
    } else {
        Err(rt.arity_error(name, required, args.len()))
    }
}

/// The text a set files `v` under: the kind, a colon and the value; a list, map, function or other object by
/// the identity of the object.
fn key_of(rt: &Rt<'_>, v: Value, out: &mut Vec<u8>) {
    let object = |out: &mut Vec<u8>, tag: &str, id: u32| {
        out.extend_from_slice(format!("o:{tag}{id}").as_bytes());
    };
    match v {
        Value::Str(s) => {
            out.extend_from_slice(b"s:");
            out.extend_from_slice(rt.heap.str_bytes(s));
        }
        Value::Int(i) => {
            out.extend_from_slice(b"i:");
            format_int(i, out);
        }
        Value::Float(f) => {
            out.extend_from_slice(b"f:");
            format_float_g(f, out);
        }
        Value::Bool(true) => out.extend_from_slice(b"b:true"),
        Value::Bool(false) => out.extend_from_slice(b"b:false"),
        Value::Null => out.extend_from_slice(b"n:null"),
        Value::List(l) => object(out, "L", l.0),
        Value::Map(m) => object(out, "M", m.0),
        Value::Func(f) => object(out, "F", f.0),
        Value::Builtin(b) => object(out, "B", u32::from(b)),
        Value::Host(h) => object(out, "H", u32::from(h)),
        Value::Error(s, _) => out.extend_from_slice(format!("o:E{}.{}", s.off, s.len).as_bytes()),
        Value::Set(s) => object(out, "S", s.0),
        Value::Stack(l) => object(out, "T", l.0),
        Value::Queue(l) => object(out, "Q", l.0),
    }
}

fn set_of(rt: &Rt<'_>, v: Value, message: &str) -> Result<SetId, Exc> {
    match v {
        Value::Set(s) => Ok(s),
        _ => Err(rt.type_error(message)),
    }
}

/// Runs `f` with the key of `v` built in the scratch buffer.
fn with_key<T>(
    rt: &mut Rt<'_>,
    v: Value,
    f: impl FnOnce(&mut Rt<'_>, &[u8]) -> Result<T, Exc>,
) -> Result<T, Exc> {
    let mut key = std::mem::take(&mut *rt.scratch);
    key.clear();
    key_of(rt, v, &mut key);
    let done = f(rt, &key);
    *rt.scratch = key;
    done
}

fn set_new(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    exactly(rt, "set_new", args, 0)?;
    Ok(Value::Set(rt.heap.new_set().map_err(rt.oom())?))
}

fn set_add(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    exactly(rt, "set_add", args, 2)?;
    let set = set_of(rt, args[0], "set_add() requires first argument to be a set")?;
    with_key(rt, args[1], |rt, key| {
        rt.heap.set_insert(set, key, args[1]).map_err(rt.oom())?;
        Ok(Value::Null)
    })
}

/// Takes the key out of the set and the member that was filed under it out of the order. A member that is only
/// `==` to the argument (`1.0` for `1`) is another member and stays.
fn set_remove(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    exactly(rt, "set_remove", args, 2)?;
    let set = set_of(
        rt,
        args[0],
        "set_remove() requires first argument to be a set",
    )?;
    with_key(rt, args[1], |rt, key| {
        if !rt.heap.set_forget(set, key) {
            return Ok(Value::Bool(false));
        }
        let mut member_key = Vec::new();
        for i in 0..rt.heap.set_keys(set).len() {
            rt.work(Count::Free, 1)?;
            let member = rt.heap.set_keys(set)[i];
            member_key.clear();
            key_of(rt, member, &mut member_key);
            if member_key == key {
                rt.heap.set_drop_order(set, i);
                break;
            }
        }
        Ok(Value::Bool(true))
    })
}

fn set_has(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    exactly(rt, "set_has", args, 2)?;
    let set = set_of(rt, args[0], "set_has() requires first argument to be a set")?;
    with_key(rt, args[1], |rt, key| {
        Ok(Value::Bool(rt.heap.set_has(set, key)))
    })
}

fn set_size(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    exactly(rt, "set_size", args, 1)?;
    let set = set_of(rt, args[0], "set_size() requires a set argument")?;
    Ok(Value::Int(rt.heap.set_len(set) as i64))
}

fn set_to_array(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    exactly(rt, "set_to_array", args, 1)?;
    let set = set_of(rt, args[0], "set_to_array() requires a set argument")?;
    let n = rt.heap.set_keys(set).len();
    rt.work(Count::Free, n as u64)?;
    let out = rt.heap.new_list(n, n).map_err(rt.oom())?;
    for i in 0..n {
        let member = rt.heap.set_keys(set)[i];
        rt.heap.list_push(out, member).map_err(rt.oom())?;
    }
    Ok(Value::List(out))
}

fn stack_new(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    exactly(rt, "stack_new", args, 0)?;
    Ok(Value::Stack(rt.heap.new_list(0, 0).map_err(rt.oom())?))
}

fn stack_of(rt: &Rt<'_>, v: Value, message: &str) -> Result<uddin_vm::ListId, Exc> {
    match v {
        Value::Stack(l) => Ok(l),
        _ => Err(rt.type_error(message)),
    }
}

fn stack_push(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    exactly(rt, "stack_push", args, 2)?;
    let list = stack_of(
        rt,
        args[0],
        "stack_push() requires first argument to be a stack",
    )?;
    rt.heap.list_append(list, &args[1..]).map_err(rt.oom())?;
    Ok(Value::Null)
}

fn stack_pop(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    exactly(rt, "stack_pop", args, 1)?;
    let list = stack_of(rt, args[0], "stack_pop() requires a stack argument")?;
    Ok(rt.heap.list_pop(list).unwrap_or(Value::Null))
}

fn stack_peek(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    exactly(rt, "stack_peek", args, 1)?;
    let list = stack_of(rt, args[0], "stack_peek() requires a stack argument")?;
    Ok(rt
        .heap
        .list(list)
        .items
        .last()
        .copied()
        .unwrap_or(Value::Null))
}

fn stack_size(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    exactly(rt, "stack_size", args, 1)?;
    let list = stack_of(rt, args[0], "stack_size() requires a stack argument")?;
    Ok(Value::Int(rt.heap.list(list).items.len() as i64))
}

fn stack_empty(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    exactly(rt, "stack_empty", args, 1)?;
    let list = stack_of(rt, args[0], "stack_empty() requires a stack argument")?;
    Ok(Value::Bool(rt.heap.list(list).items.is_empty()))
}

fn queue_new(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    exactly(rt, "queue_new", args, 0)?;
    Ok(Value::Queue(rt.heap.new_list(0, 0).map_err(rt.oom())?))
}

fn queue_of(rt: &Rt<'_>, v: Value, message: &str) -> Result<uddin_vm::ListId, Exc> {
    match v {
        Value::Queue(l) => Ok(l),
        _ => Err(rt.type_error(message)),
    }
}

fn queue_enqueue(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    exactly(rt, "queue_enqueue", args, 2)?;
    let list = queue_of(
        rt,
        args[0],
        "queue_enqueue() requires first argument to be a queue",
    )?;
    rt.heap.list_append(list, &args[1..]).map_err(rt.oom())?;
    Ok(Value::Null)
}

fn queue_dequeue(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    exactly(rt, "queue_dequeue", args, 1)?;
    let list = queue_of(rt, args[0], "queue_dequeue() requires a queue argument")?;
    Ok(rt.heap.list_shift(list).unwrap_or(Value::Null))
}

fn queue_front(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    exactly(rt, "queue_front", args, 1)?;
    let list = queue_of(rt, args[0], "queue_front() requires a queue argument")?;
    Ok(rt
        .heap
        .list(list)
        .items
        .first()
        .copied()
        .unwrap_or(Value::Null))
}

fn queue_size(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    exactly(rt, "queue_size", args, 1)?;
    let list = queue_of(rt, args[0], "queue_size() requires a queue argument")?;
    Ok(Value::Int(rt.heap.list(list).items.len() as i64))
}

fn queue_empty(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    exactly(rt, "queue_empty", args, 1)?;
    let list = queue_of(rt, args[0], "queue_empty() requires a queue argument")?;
    Ok(Value::Bool(rt.heap.list(list).items.is_empty()))
}

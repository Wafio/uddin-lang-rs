//! The builtins every script starts with: len, print, str, typeof.

use uddin_vm::{
    BuiltinDef, Exc, Rt, Stringify, Value, float_to_int, format_float_g6, format_int, to_text,
    type_name_str,
};

pub(crate) const TABLE: &[BuiltinDef] = &[
    BuiltinDef::new("len", len, 1).fast_path(),
    BuiltinDef::new("print", print, -1)
        .fast_path()
        .deep()
        .stringify(Stringify::Text),
    BuiltinDef::new("str", str_, 1)
        .fast_path()
        .deep()
        .stringify(Stringify::Text),
    BuiltinDef::new("typeof", typeof_, 1).fast_path(),
];

fn one_arg(rt: &Rt<'_>, name: &str, args: &[Value]) -> Result<Value, Exc> {
    match args {
        [v] => Ok(*v),
        _ => Err(rt.arity_error(name, 1, args.len())),
    }
}

fn len(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let n = match one_arg(rt, "len", args)? {
        Value::Str(s) => s.len as usize,
        Value::List(l) => rt.heap.list(l).items.len(),
        Value::Map(m) => rt.heap.map_len(m),
        _ => return Err(rt.type_error("len() requires a string, array, or object")),
    };
    Ok(Value::Int(n as i64))
}

/// The line `print` writes for `args`, without the end of line, appended to `buf`.
fn print_line(rt: &mut Rt<'_>, args: &[Value], buf: &mut Vec<u8>) -> Result<(), Exc> {
    for (i, &a) in args.iter().enumerate() {
        if i > 0 {
            buf.push(b' ');
        }
        match a {
            Value::Str(s) => buf.extend_from_slice(rt.heap.str_bytes(s)),
            Value::Int(n) => format_int(n, buf),
            Value::Float(f) => {
                let n = float_to_int(f);
                if n as f64 == f {
                    format_int(n, buf);
                } else {
                    format_float_g6(f, buf);
                }
            }
            Value::Bool(true) => buf.extend_from_slice(b"true"),
            Value::Bool(false) => buf.extend_from_slice(b"false"),
            Value::Null => buf.extend_from_slice(b"null"),
            other => to_text(rt.env, rt.heap, rt.meter, other, false, 0, buf)?,
        }
    }
    Ok(())
}

fn print(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let mut buf = std::mem::take(&mut *rt.scratch);
    buf.clear();
    let done = print_line(rt, args, &mut buf);
    if done.is_ok() {
        buf.push(b'\n');
        rt.out.write(&buf);
    }
    *rt.scratch = buf;
    done.map(|()| Value::Null)
}

fn str_(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let v = one_arg(rt, "str", args)?;
    if let Value::Str(_) = v {
        return Ok(v);
    }
    let mut buf = std::mem::take(&mut *rt.scratch);
    buf.clear();
    let done = to_text(rt.env, rt.heap, rt.meter, v, false, 0, &mut buf);
    let s = done.and_then(|()| rt.heap.new_str(&buf).map_err(rt.oom()));
    *rt.scratch = buf;
    Ok(Value::Str(s?))
}

fn typeof_(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    Ok(Value::Str(type_name_str(one_arg(rt, "typeof", args)?)))
}

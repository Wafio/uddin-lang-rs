//! A value as text, the way `str` and `print` show it.

use crate::builtin::Env;
use crate::error::Exc;
use crate::heap::Heap;
use crate::meter::{Count, Meter};
use crate::text;
use crate::value::{Value, host_type_name};

/// Past this depth the rest of a value prints as `...`.
const MAX_DEPTH: u32 = 1000;

/// Appends the text of `v`. A string is raw unless `quote_str` is set, which nested strings always are; a map
/// lists its entries sorted by their text; an error value shows its text, quoted or not. Each node and each
/// byte of a string counts as work, so the deadline stops the walk over a large value.
pub fn to_text(
    env: &Env,
    heap: &Heap,
    meter: &mut Meter,
    v: Value,
    quote_str: bool,
    depth: u32,
    out: &mut Vec<u8>,
) -> Result<(), Exc> {
    if depth > MAX_DEPTH {
        out.extend_from_slice(b"...");
        return Ok(());
    }
    meter.work(Count::Free, 1)?;
    match v {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        Value::Int(i) => text::format_int(i, out),
        Value::Float(f) => text::format_float_g(f, out),
        Value::Str(s) => {
            let b = heap.str_bytes(s);
            meter.work(Count::Free, b.len() as u64)?;
            if quote_str {
                text::quote(b, out);
            } else {
                out.extend_from_slice(b);
            }
        }
        Value::List(l) => {
            out.push(b'[');
            for i in 0..heap.list(l).items.len() {
                if i > 0 {
                    out.extend_from_slice(b", ");
                }
                to_text(
                    env,
                    heap,
                    meter,
                    heap.list(l).items[i],
                    true,
                    depth + 1,
                    out,
                )?;
            }
            out.push(b']');
        }
        Value::Map(m) => {
            meter.work(Count::Free, heap.map_len(m) as u64)?;
            let mut items: Vec<Vec<u8>> = Vec::with_capacity(heap.map_len(m));
            for i in 0..heap.map_len(m) {
                let (k, e) = heap.map_entries(m)[i];
                let mut item = Vec::new();
                text::quote(heap.str_bytes(k), &mut item);
                item.extend_from_slice(b": ");
                to_text(env, heap, meter, e, true, depth + 1, &mut item)?;
                items.push(item);
            }
            items.sort_unstable();
            out.push(b'{');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.extend_from_slice(b", ");
                }
                out.extend_from_slice(item);
            }
            out.push(b'}');
        }
        Value::Func(f) => {
            let proto = &env.prog.protos[heap.func(f).proto as usize];
            match proto.name {
                Some(n) => {
                    out.extend_from_slice(b"<fun ");
                    out.extend_from_slice(env.prog.name(n).as_bytes());
                    out.push(b'>');
                }
                None => out.extend_from_slice(b"<fun>"),
            }
        }
        Value::Host(_) => {
            out.extend_from_slice(b"str() got unexpected type map[string]interface {}")
        }
        Value::Set(_) | Value::Stack(_) | Value::Queue(_) => {
            out.extend_from_slice(b"str() got unexpected type ");
            out.extend_from_slice(host_type_name(v).as_bytes());
        }
        Value::Error(s, _) => out.extend_from_slice(heap.str_bytes(s)),
        Value::Builtin(b) => {
            out.extend_from_slice(b"<builtin ");
            out.extend_from_slice(env.builtins[usize::from(b)].name.as_bytes());
            out.push(b'>');
        }
    }
    Ok(())
}

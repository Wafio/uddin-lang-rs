//! The operators of the language over run-time values, with the checks a sandbox puts in front of them.

use uddin_syntax::Position;

use crate::error::{Exc, LimitReason};
use crate::heap::Heap;
use crate::meter::{Count, Meter};
use crate::text::{self, quote_for_error};
use crate::value::{ListId, MapId, Str, Value, as_float, host_type_name};

pub const WORD_COST: i64 = 16;
pub const SCALAR_COST: i64 = 8;
pub const LIST_COST: i64 = 24;
pub const MAP_COST: i64 = 48;
pub const MAP_ENTRY_COST: i64 = 64;
/// A set, stack or queue.
pub const OBJECT_COST: i64 = 64;

/// What the builtins that allocate far more than they return reserve before they run, per element they make:
/// a part of a split, an element of a range, a match of a regular expression, a byte of JSON text.
pub const SPLIT_PART_COST: i64 = 48;
pub const RANGE_ELEM_COST: i64 = 40;
pub const REGEX_MATCH_COST: i64 = 160;
pub const JSON_BYTE_COST: i64 = 64;
/// Compiling a regular expression costs about this much per instruction of its program plus a fixed part,
/// and parsing it about `REGEX_PARSE_BYTE_COST` per byte of the pattern; a pattern may have at most
/// `REGEX_BYTES_PER_INST` bytes for every instruction the profile allows.
pub const REGEX_BASE_COST: i64 = 256 << 10;
pub const REGEX_INST_COST: i64 = 1 << 10;
pub const REGEX_PARSE_BYTE_COST: i64 = 256;
pub const REGEX_BYTES_PER_INST: i64 = 4;
/// Arguments of `range` beyond this magnitude stop the run before any arithmetic on them could overflow.
pub const RANGE_ARG_BOUND: i64 = 1 << 40;

/// How deep lists and maps may nest in a comparison.
const MAX_COMPARE_DEPTH: u32 = 1000;
/// How deep a walked value may nest before the run stops.
pub const MAX_VALUE_DEPTH: usize = 256;

/// Only a bool, a number, a string or a non-empty map is truthy-or-not by content; every list is truthy.
pub fn truthy(heap: &Heap, v: Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => b,
        Value::Int(i) => i != 0,
        Value::Float(f) => f != 0.0,
        Value::Str(s) => s.len > 0,
        Value::Map(m) => heap.map_len(m) > 0,
        Value::List(_)
        | Value::Func(_)
        | Value::Builtin(_)
        | Value::Host(_)
        | Value::Error(..)
        | Value::Set(_)
        | Value::Stack(_)
        | Value::Queue(_) => true,
    }
}

pub fn values_equal(
    heap: &Heap,
    meter: &mut Meter,
    pos: Position,
    l: Value,
    r: Value,
    depth: u32,
) -> Result<bool, Exc> {
    if depth > MAX_COMPARE_DEPTH {
        return Err(Exc::value_error(pos, "comparison nested too deeply"));
    }
    Ok(match (l, r) {
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Int(a), Value::Float(b)) => a as f64 == b,
        (Value::Float(a), Value::Int(b)) => a == b as f64,
        (Value::Float(a), Value::Float(b)) => a == b,
        (Value::Str(a), Value::Str(b)) => heap.str_bytes(a) == heap.str_bytes(b),
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Null, Value::Null) => true,
        (Value::List(a), Value::List(b)) => {
            let (la, lb) = (heap.list(a).items.len(), heap.list(b).items.len());
            if la != lb {
                return Ok(false);
            }
            for i in 0..la {
                meter.work(Count::Free, 1)?;
                let (x, y) = (heap.list(a).items[i], heap.list(b).items[i]);
                if !values_equal(heap, meter, pos, x, y, depth + 1)? {
                    return Ok(false);
                }
            }
            true
        }
        (Value::Map(a), Value::Map(b)) => {
            if heap.map_len(a) != heap.map_len(b) {
                return Ok(false);
            }
            for i in 0..heap.map_len(a) {
                meter.work(Count::Free, 1)?;
                let (k, v) = heap.map_entries(a)[i];
                let other = heap.map_get(b, k).unwrap_or(Value::Null);
                if !values_equal(heap, meter, pos, v, other, depth + 1)? {
                    return Ok(false);
                }
            }
            true
        }
        (Value::Func(a), Value::Func(b)) => a == b,
        (Value::Builtin(_), Value::Builtin(_)) => {
            return Err(Exc::type_error(pos, "can't compare builtin functions"));
        }
        _ => false,
    })
}

pub fn value_less(
    heap: &Heap,
    meter: &mut Meter,
    pos: Position,
    l: Value,
    r: Value,
    depth: u32,
) -> Result<bool, Exc> {
    if depth > MAX_COMPARE_DEPTH {
        return Err(Exc::value_error(pos, "comparison nested too deeply"));
    }
    match (l, r) {
        (Value::Int(a), Value::Int(b)) => return Ok(a < b),
        (Value::Int(a), Value::Float(b)) => return Ok((a as f64) < b),
        (Value::Float(a), Value::Int(b)) => return Ok(a < b as f64),
        (Value::Float(a), Value::Float(b)) => return Ok(a < b),
        (Value::Str(a), Value::Str(b)) => return Ok(heap.str_bytes(a) < heap.str_bytes(b)),
        (Value::List(a), Value::List(b)) => {
            let (la, lb) = (heap.list(a).items.len(), heap.list(b).items.len());
            for i in 0..la.min(lb) {
                meter.work(Count::Free, 1)?;
                let (x, y) = (heap.list(a).items[i], heap.list(b).items[i]);
                if !values_equal(heap, meter, pos, x, y, depth + 1)? {
                    return value_less(heap, meter, pos, x, y, depth + 1);
                }
            }
            return Ok(la < lb);
        }
        _ => {}
    }
    Err(Exc::type_error(
        pos,
        "comparison requires two integers or two strings (or arrays of integers or strings)",
    ))
}

pub fn is_member(
    heap: &Heap,
    meter: &mut Meter,
    pos: Position,
    l: Value,
    r: Value,
) -> Result<bool, Exc> {
    match r {
        Value::Str(hay) => match l {
            Value::Str(needle) => {
                let (n, h) = (heap.str_bytes(needle), heap.str_bytes(hay));
                Ok(match n.len().cmp(&h.len()) {
                    std::cmp::Ordering::Less => {
                        meter.work(Count::Free, h.len() as u64)?;
                        n.is_empty() || memchr::memmem::find(h, n).is_some()
                    }
                    std::cmp::Ordering::Equal => n == h,
                    std::cmp::Ordering::Greater => false,
                })
            }
            _ => Err(Exc::type_error(
                pos,
                "in string requires string on left side",
            )),
        },
        Value::List(id) => {
            for i in 0..heap.list(id).items.len() {
                meter.work(Count::Free, 1)?;
                let v = heap.list(id).items[i];
                if values_equal(heap, meter, pos, l, v, 0)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Value::Map(id) => match l {
            Value::Str(k) => Ok(heap.map_contains(id, k)),
            _ => Err(Exc::type_error(
                pos,
                "in object requires string on left side",
            )),
        },
        _ => Err(Exc::type_error(
            pos,
            "in requires string, array, or object on right side",
        )),
    }
}

/// Bytes for strings, elements for lists, nothing for the rest.
pub fn length_of(heap: &Heap, v: Value) -> i64 {
    match v {
        Value::Str(s) => i64::from(s.len),
        Value::List(l) => heap.list(l).items.len() as i64,
        _ => 0,
    }
}

/// `size * count`, or -1 when it overflows or either is negative.
pub fn checked_product(size: i64, count: i64) -> i64 {
    if count < 0 || size < 0 {
        return -1;
    }
    if size == 0 || count == 0 {
        return 0;
    }
    if count > (1i64 << 62) / size {
        return -1;
    }
    size * count
}

/// `base + product`, or -1 when `product` is the -1 that [`checked_product`] gives for an overflow.
pub fn sum_estimate(base: i64, product: i64) -> i64 {
    if product < 0 {
        -1
    } else {
        base.saturating_add(product)
    }
}

fn concat_cost(heap: &Heap, l: Value, r: Value) -> i64 {
    match (l, r) {
        (Value::Str(a), Value::Str(b)) => WORD_COST + i64::from(a.len) + i64::from(b.len),
        (Value::List(a), Value::List(b)) => {
            LIST_COST + WORD_COST * (heap.list(a).items.len() + heap.list(b).items.len()) as i64
        }
        (Value::Map(a), Value::Map(b)) => {
            MAP_COST + MAP_ENTRY_COST * (heap.map_len(a) + heap.map_len(b)) as i64
        }
        _ => 0,
    }
}

fn repeat_cost(heap: &Heap, l: Value, r: Value) -> i64 {
    let (n, other) = match (l, r) {
        (Value::Int(n), o) | (o, Value::Int(n)) => (n, o),
        _ => return 0,
    };
    if n < 0 {
        return 0;
    }
    match other {
        Value::Str(s) => match checked_product(i64::from(s.len), n) {
            -1 => -1,
            b => WORD_COST + b,
        },
        Value::List(id) => match checked_product(WORD_COST * heap.list(id).items.len() as i64, n) {
            -1 => -1,
            b => LIST_COST + b,
        },
        _ => 0,
    }
}

pub fn plus(
    heap: &mut Heap,
    meter: &mut Meter,
    pos: Position,
    l: Value,
    r: Value,
) -> Result<Value, Exc> {
    if meter.lim.max_value_len > 0 {
        meter.check_length(pos, length_of(heap, l) + length_of(heap, r))?;
    }
    meter.charge(pos, concat_cost(heap, l, r))?;
    let oom = Exc::memory(pos);
    Ok(match (l, r) {
        (Value::Int(a), Value::Int(b)) => Value::Int(a.wrapping_add(b)),
        (Value::Int(a), Value::Float(b)) => Value::Float(a as f64 + b),
        (Value::Float(a), Value::Int(b)) => Value::Float(a + b as f64),
        (Value::Float(a), Value::Float(b)) => Value::Float(a + b),
        (Value::Str(a), Value::Str(b)) => {
            meter.work(Count::Free, u64::from(a.len) + u64::from(b.len))?;
            Value::Str(heap.concat(a, b).map_err(oom)?)
        }
        (Value::List(a), Value::List(b)) => {
            let (la, lb) = (heap.list(a).items.len(), heap.list(b).items.len());
            meter.work(Count::Free, (la + lb) as u64)?;
            let id = heap.new_list(la + lb, la + lb).map_err(oom)?;
            heap.list_extend(id, a).map_err(Exc::memory(pos))?;
            heap.list_extend(id, b).map_err(Exc::memory(pos))?;
            Value::List(id)
        }
        (Value::Map(a), Value::Map(b)) => {
            let id = heap
                .new_map(heap.map_len(a) + heap.map_len(b))
                .map_err(oom)?;
            for src in [a, b] {
                for i in 0..heap.map_len(src) {
                    meter.work(Count::Free, 1)?;
                    let (k, v) = heap.map_entries(src)[i];
                    heap.map_set(id, k, v).map_err(Exc::memory(pos))?;
                }
            }
            Value::Map(id)
        }
        _ => {
            return Err(Exc::type_error(
                pos,
                "+ requires two integers, strings, arrays, or objects",
            ));
        }
    })
}

pub fn minus(pos: Position, l: Value, r: Value) -> Result<Value, Exc> {
    Ok(match (l, r) {
        (Value::Int(a), Value::Int(b)) => Value::Int(a.wrapping_sub(b)),
        (Value::Int(a), Value::Float(b)) => Value::Float(a as f64 - b),
        (Value::Float(a), Value::Int(b)) => Value::Float(a - b as f64),
        (Value::Float(a), Value::Float(b)) => Value::Float(a - b),
        _ => {
            return Err(Exc::type_error(
                pos,
                format!(
                    "- requires two floats or integers, got {} and {}",
                    host_type_name(l),
                    host_type_name(r)
                ),
            ));
        }
    })
}

pub fn times(
    heap: &mut Heap,
    meter: &mut Meter,
    pos: Position,
    l: Value,
    r: Value,
) -> Result<Value, Exc> {
    if meter.lim.max_value_len > 0 {
        match (l, r) {
            (Value::Int(n), _) => {
                meter.check_length(pos, checked_product(length_of(heap, r), n))?
            }
            (_, Value::Int(n)) => {
                meter.check_length(pos, checked_product(length_of(heap, l), n))?
            }
            _ => {}
        }
    }
    meter.charge(pos, repeat_cost(heap, l, r))?;
    let oom = Exc::memory(pos);
    let neg =
        |what: &str| Exc::value_error(pos, format!("can't multiply {what} by a negative number"));
    match (l, r) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_mul(b))),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 * b)),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a * b as f64)),
        (Value::Int(n), Value::Str(s)) | (Value::Str(s), Value::Int(n)) => {
            if n < 0 {
                return Err(neg("string"));
            }
            meter.work(Count::Free, u64::from(s.len).saturating_mul(n as u64))?;
            Ok(Value::Str(heap.repeat(s, n as usize).map_err(oom)?))
        }
        (Value::Int(n), Value::List(src)) | (Value::List(src), Value::Int(n)) => {
            if n < 0 {
                return Err(neg("array"));
            }
            let len = heap.list(src).items.len();
            let total = len
                .checked_mul(n as usize)
                .ok_or_else(|| Exc::limit(LimitReason::Memory, pos))?;
            meter.work(Count::Free, total as u64)?;
            let id = heap.new_list(total, total).map_err(oom)?;
            heap.list_extend_repeated(id, src, n as usize)
                .map_err(Exc::memory(pos))?;
            Ok(Value::List(id))
        }
        _ => Err(Exc::type_error(
            pos,
            "* requires two integers or floats, or a string or array and an integer",
        )),
    }
}

pub fn divide(pos: Position, l: Value, r: Value) -> Result<Value, Exc> {
    match (l, r) {
        (Value::Int(a), Value::Int(b)) if b != 0 => return Ok(Value::Int(a.wrapping_div(b))),
        (Value::Int(a), Value::Float(b)) if b != 0.0 => return Ok(Value::Float(a as f64 / b)),
        (Value::Float(a), Value::Float(b)) if b != 0.0 => return Ok(Value::Float(a / b)),
        (Value::Float(a), Value::Int(b)) if b != 0 => return Ok(Value::Float(a / b as f64)),
        _ => {}
    }
    match (as_float(l), as_float(r)) {
        (Some(_), Some(_)) => Err(Exc::value_error(pos, "can't divide by zero")),
        _ => Err(Exc::type_error(pos, "/ requires two floats or integers")),
    }
}

pub fn modulo(pos: Position, l: Value, r: Value) -> Result<Value, Exc> {
    if let (Value::Int(a), Value::Int(b)) = (l, r)
        && b != 0
    {
        return Ok(Value::Int(a.wrapping_rem(b)));
    }
    match (as_float(l), as_float(r)) {
        (Some(a), Some(b)) => {
            let (a, b) = (text::float_to_int(a), text::float_to_int(b));
            if b == 0 {
                return Err(Exc::value_error(pos, "can't divide by zero"));
            }
            Ok(Value::Int(a.wrapping_rem(b)))
        }
        _ => Err(Exc::type_error(
            pos,
            "modulo operator requires two floats or integers",
        )),
    }
}

pub fn power(pos: Position, l: Value, r: Value) -> Result<Value, Exc> {
    if let (Value::Int(a), Value::Int(b)) = (l, r) {
        match b {
            0 => return Ok(Value::Int(1)),
            1 => return Ok(Value::Int(a)),
            2 => return Ok(Value::Int(a.wrapping_mul(a))),
            3 => return Ok(Value::Int(a.wrapping_mul(a).wrapping_mul(a))),
            _ => {}
        }
    }
    match (as_float(l), as_float(r)) {
        (Some(a), Some(b)) => Ok(Value::Float(crate::gomath::pow(a, b))),
        _ => Err(Exc::type_error(pos, "** requires two floats or integers")),
    }
}

pub fn negate(pos: Position, v: Value) -> Result<Value, Exc> {
    match v {
        Value::Int(i) => Ok(Value::Int(i.wrapping_neg())),
        Value::Float(f) => Ok(Value::Float(-f)),
        _ => Err(Exc::type_error(pos, "unary - requires an integer or float")),
    }
}

pub fn not(pos: Position, v: Value) -> Result<Value, Exc> {
    match v {
        Value::Bool(b) => Ok(Value::Bool(!b)),
        _ => Err(Exc::type_error(pos, "not requires a bool")),
    }
}

fn out_of_range(pos: Position, i: i64) -> Exc {
    Exc::value_error(pos, format!("subscript {i} out of range"))
}

fn key_not_found(heap: &Heap, pos: Position, key: Str) -> Exc {
    let mut text = b"key not found: ".to_vec();
    quote_for_error(heap.str_bytes(key), &mut text);
    Exc::value_error(pos, String::from_utf8_lossy(&text).into_owned())
}

pub fn subscript_get(
    heap: &Heap,
    pos: Position,
    container: Value,
    sub: Value,
) -> Result<Value, Exc> {
    match container {
        Value::Str(s) => {
            let Value::Int(mut i) = sub else {
                return Err(Exc::type_error(pos, "string subscript must be an integer"));
            };
            if i < 0 {
                i = i64::from(s.len).wrapping_add(i);
            }
            if i < 0 || i >= i64::from(s.len) {
                return Err(out_of_range(pos, i));
            }
            let byte = heap.str_bytes(s)[i as usize];
            Ok(Value::Str(Str::byte(byte)))
        }
        Value::List(id) => {
            let Value::Int(mut i) = sub else {
                return Err(Exc::type_error(pos, "array subscript must be an integer"));
            };
            let items = &heap.list(id).items;
            if i < 0 {
                i = (items.len() as i64).wrapping_add(i);
            }
            if i < 0 || i >= items.len() as i64 {
                return Err(out_of_range(pos, i));
            }
            Ok(items[i as usize])
        }
        Value::Map(id) => {
            let Value::Str(k) = sub else {
                return Err(Exc::type_error(pos, "object subscript must be a string"));
            };
            heap.map_get(id, k)
                .ok_or_else(|| key_not_found(heap, pos, k))
        }
        _ => Err(Exc::type_error(
            pos,
            "can only subscript string, array, or object",
        )),
    }
}

pub fn subscript_set(
    heap: &mut Heap,
    meter: &mut Meter,
    pos: Position,
    container: Value,
    sub: Value,
    v: Value,
) -> Result<(), Exc> {
    match container {
        Value::List(id) => {
            let Value::Int(i) = sub else {
                return Err(Exc::type_error(pos, "array subscript must be an integer"));
            };
            let items = &mut heap.list_mut(id).items;
            if i < 0 || i >= items.len() as i64 {
                return Err(out_of_range(pos, i));
            }
            items[i as usize] = v;
            Ok(())
        }
        Value::Map(id) => {
            let Value::Str(k) = sub else {
                return Err(Exc::type_error(pos, "object subscript must be a string"));
            };
            if !heap.map_contains(id, k) {
                meter.charge(pos, MAP_ENTRY_COST)?;
            }
            heap.map_set(id, k, v).map_err(Exc::memory(pos))?;
            Ok(())
        }
        _ => Err(Exc::type_error(
            pos,
            "can only assign to subscript of array or object",
        )),
    }
}

/// One container on the path of the walk in [`walk_nested`], with the children still to visit.
#[derive(Clone, Copy)]
pub struct PathEntry {
    node: Node,
    remaining: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Node {
    List(ListId),
    Map(MapId),
}

fn node_of(v: Value) -> Option<Node> {
    match v {
        Value::List(l) => Some(Node::List(l)),
        Value::Map(m) => Some(Node::Map(m)),
        _ => None,
    }
}

fn child(heap: &Heap, n: Node, i: usize) -> Value {
    match n {
        Node::List(l) => heap.list(l).items[i],
        Node::Map(m) => heap.map_entries(m)[i].1,
    }
}

fn child_count(heap: &Heap, n: Node) -> usize {
    match n {
        Node::List(l) => heap.list(l).items.len(),
        Node::Map(m) => heap.map_len(m),
    }
}

/// Walks every node an operation on these values could visit, a shared child as often as it is reachable, and
/// charges one operation for each. A value nested too deep or containing itself stops the run before the
/// operation recurses. The walk keeps its path in `path` and never recurses.
pub fn walk_nested(
    heap: &Heap,
    meter: &mut Meter,
    path: &mut Vec<PathEntry>,
    pos: Position,
    vals: &[Value],
) -> Result<(), Exc> {
    let node_cap = if meter.lim.max_value_len > 0 {
        meter.lim.max_value_len
    } else {
        1 << 20
    };
    let mut visited = 0usize;
    path.clear();
    for &root in vals.iter().rev() {
        let Some(root) = node_of(root) else {
            continue;
        };
        let mut pending = Some(root);
        loop {
            if let Some(node) = pending.take() {
                meter.work(Count::Op, 1)?;
                visited += 1;
                if visited > node_cap {
                    path.clear();
                    return Err(Exc::limit(LimitReason::ValueSize, pos));
                }
                if path.len() + 1 > MAX_VALUE_DEPTH || path.iter().any(|e| e.node == node) {
                    path.clear();
                    return Err(Exc::limit(LimitReason::ValueDepth, pos));
                }
                if path.try_reserve(1).is_err() {
                    path.clear();
                    return Err(Exc::limit(LimitReason::Memory, pos));
                }
                path.push(PathEntry {
                    node,
                    remaining: child_count(heap, node),
                });
            }
            let Some(top) = path.last_mut() else {
                break;
            };
            if top.remaining == 0 {
                path.pop();
                if path.is_empty() {
                    break;
                }
                continue;
            }
            top.remaining -= 1;
            let kid = child(heap, top.node, top.remaining);
            match node_of(kid) {
                Some(node) => pending = Some(node),
                None => {
                    meter.work(Count::Op, 1)?;
                    visited += 1;
                    if visited > node_cap {
                        path.clear();
                        return Err(Exc::limit(LimitReason::ValueSize, pos));
                    }
                }
            }
        }
    }
    Ok(())
}

/// An upper bound of the text of `v`, counted until it passes `budget`.
pub fn text_size_bound(heap: &Heap, v: Value, budget: i64, json: bool, quote_top: bool) -> i64 {
    let mut n = 0i64;
    let mut stack = vec![(v, quote_top)];
    while n <= budget {
        let Some((cur, quoted)) = stack.pop() else {
            break;
        };
        match cur {
            Value::Str(s) => {
                let b = heap.str_bytes(s);
                n += if quoted {
                    text::quoted_size(b, json) as i64
                } else {
                    b.len() as i64
                };
            }
            Value::List(l) => {
                n += 2;
                for i in 0..heap.list(l).items.len() {
                    n += 2;
                    if n > budget {
                        break;
                    }
                    stack.push((heap.list(l).items[i], true));
                }
            }
            Value::Map(m) => {
                n += 2;
                for i in 0..heap.map_len(m) {
                    let (k, e) = heap.map_entries(m)[i];
                    n += text::quoted_size(heap.str_bytes(k), json) as i64 + 4;
                    if n > budget {
                        break;
                    }
                    stack.push((e, true));
                }
            }
            _ => n += 24,
        }
    }
    n
}

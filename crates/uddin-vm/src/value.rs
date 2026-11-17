//! The value model: a 16-byte `Copy` enum whose strings, lists, maps and functions live in a per-run heap.

use std::mem::size_of;

use crate::error::ErrClass;

/// Strings at or above this offset are in the program's constant pool, the rest in the run heap.
pub const CONST_BASE: u32 = 1 << 31;

/// A span of bytes. Equal spans are equal strings; equal strings need not be equal spans.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Str {
    pub off: u32,
    pub len: u32,
}

impl Str {
    pub const EMPTY: Str = Str {
        off: CONST_BASE,
        len: 0,
    };

    /// One byte, from the constant pool, which starts with all 256 byte values.
    pub const fn byte(b: u8) -> Str {
        Str {
            off: CONST_BASE + b as u32,
            len: 1,
        }
    }

    pub fn is_const(self) -> bool {
        self.off >= CONST_BASE
    }

    /// Whether `self` lies inside `outer`, as a substring shares the bytes of its source. An empty `outer` holds
    /// nothing.
    pub fn within(self, outer: Str) -> bool {
        outer.len != 0
            && self.is_const() == outer.is_const()
            && self.off >= outer.off
            && u64::from(self.off) + u64::from(self.len)
                <= u64::from(outer.off) + u64::from(outer.len)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ListId(pub u32);
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MapId(pub u32);
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SetId(pub u32);
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuncId(pub u32);
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScopeId(pub u32);

#[derive(Clone, Copy, Debug)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Str),
    List(ListId),
    Map(MapId),
    /// A function the script defined.
    Func(FuncId),
    /// A builtin, by its index in the builtin table of the run.
    Builtin(u16),
    /// A value of the embedder that scripts cannot use as data; only builtins read it.
    Host(u8),
    /// A set, a stack and a queue: values only builtins read. A stack and a queue keep their elements in a list
    /// object of the heap that no script can name.
    Set(SetId),
    Stack(ListId),
    Queue(ListId),
    /// A failure a builtin returned as a value instead of raising it. The text is a heap string. The script can
    /// store it, print it and pass it on; `if` and `while` raise it. A failure the reference makes with a class
    /// (a type error, say) has it, and keeps it when raised.
    Error(Str, Option<ErrClass>),
}

const _: () = assert!(size_of::<Value>() == 16);

/// The type name `typeof` reports.
pub fn type_name(v: Value) -> &'static str {
    match v {
        Value::Null => "nullable",
        Value::Bool(_) => "boolean",
        Value::Int(_) => "integer",
        Value::Float(_) => "float",
        Value::Str(_) => "string",
        Value::List(_) => "array",
        Value::Map(_) => "object",
        Value::Func(_) | Value::Builtin(_) => "function",
        Value::Host(_) | Value::Error(..) | Value::Set(_) | Value::Stack(_) | Value::Queue(_) => {
            "unknown"
        }
    }
}

const TYPE_NAMES: [&str; 9] = [
    "nullable", "boolean", "integer", "float", "string", "array", "object", "function", "unknown",
];

/// The bytes after the 256 single bytes of every constant pool: the type names, so `typeof` allocates nothing.
pub fn type_name_pool() -> Vec<u8> {
    TYPE_NAMES.concat().into_bytes()
}

/// `type_name` as a constant-pool string.
pub fn type_name_str(v: Value) -> Str {
    let name = type_name(v);
    let i = TYPE_NAMES
        .iter()
        .position(|n| *n == name)
        .expect("every type name is in the table");
    let off: usize = TYPE_NAMES[..i].iter().map(|n| n.len()).sum();
    Str {
        off: CONST_BASE + 256 + off as u32,
        len: name.len() as u32,
    }
}

/// The name of the host-language type of a value, as it appears in some messages.
pub fn host_type_name(v: Value) -> &'static str {
    match v {
        Value::Null => "<nil>",
        Value::Bool(_) => "bool",
        Value::Int(_) => "int",
        Value::Float(_) => "float64",
        Value::Str(_) => "string",
        Value::List(_) => "*[]interpreter.Value",
        Value::Map(_) => "map[string]interpreter.Value",
        Value::Func(_) => "*interpreter.userFunction",
        Value::Builtin(_) => "interpreter.builtinFunction",
        Value::Host(_) => "map[string]interface {}",
        Value::Set(_) => "*interpreter.Set",
        Value::Stack(_) => "*interpreter.Stack",
        Value::Queue(_) => "*interpreter.Queue",
        Value::Error(_, None) => "*errors.errorString",
        Value::Error(_, Some(ErrClass::Type)) => "interpreter.TypeError",
        Value::Error(_, Some(ErrClass::Value)) => "interpreter.ValueError",
        Value::Error(_, Some(ErrClass::Name)) => "interpreter.NameError",
        Value::Error(_, Some(ErrClass::Runtime)) => "interpreter.RuntimeError",
    }
}

/// An int-valued or float-valued operand as a float, or `None` for any other value.
pub fn as_float(v: Value) -> Option<f64> {
    match v {
        Value::Int(i) => Some(i as f64),
        Value::Float(f) => Some(f),
        _ => None,
    }
}

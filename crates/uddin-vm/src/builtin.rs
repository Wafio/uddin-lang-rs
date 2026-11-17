//! What a builtin sees of the run, how the machine calls one, and what the machine does around the call.

use uddin_syntax::Position;

use crate::bytecode::Program;
use crate::error::{Exc, LimitReason, error_text_class};
use crate::heap::{Heap, Oom, Shape};
use crate::host::{RequestContext, Verdict};
use crate::meter::{Count, Meter, Output};
use crate::ops::{self, LIST_COST, MAP_COST, MAP_ENTRY_COST, OBJECT_COST, SCALAR_COST, WORD_COST};
use crate::text;
use crate::value::{ListId, MapId, ScopeId, SetId, Str, Value};
use crate::vm::{Lent, call_function};

pub type BuiltinFn = fn(&mut Rt<'_>, &[Value]) -> Result<Value, Exc>;

/// A step the machine takes around a call, with the arguments. It may stop the run. It returns the bytes it
/// charged to the memory budget, which the machine hands back when the call has ended, in place of what the
/// call really used; a step that charges nothing returns 0.
pub type Hook = fn(&mut Rt<'_>, &[Value]) -> Result<i64, Exc>;

/// How the machine bounds the text a call builds from its arguments, before the call.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stringify {
    No,
    /// As `str` shows a value: strings nested in lists and maps are quoted.
    Text,
    /// As a JSON encoder writes it: a string at the top is quoted too.
    Json,
}

/// How much of a result the call made itself, which is what the memory budget is charged for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fresh {
    /// A list or map costs its own size; the values inside it were made elsewhere.
    Shallow,
    /// The strings and numbers in a result list were made by the call too.
    Elements,
    /// The whole tree of lists, maps, strings and numbers was made by the call.
    Tree,
}

/// One builtin: the function, how many arguments it takes, and what the machine does around the call, in this
/// order: walk the arguments when `deep`, bound the text per `stringify`, run `guard` (with a value size limit
/// set), run `guard_regex`, then, with a memory limit set, run `reserve`, call, cut an error value that is too
/// long, and settle the memory per `fresh`.
#[derive(Clone, Copy)]
pub struct BuiltinDef {
    pub name: &'static str,
    pub func: BuiltinFn,
    /// Exact argument count, or -1 for any.
    pub arity: i8,
    /// A builtin on the fast path checks its own argument count.
    pub fast_path: bool,
    /// Arguments are walked for depth and cycles before the call.
    pub deep: bool,
    pub stringify: Stringify,
    pub fresh: Fresh,
    /// Refuses arguments whose result would be too large, before the call.
    pub guard: Option<Hook>,
    /// Bounds a regular expression pattern and charges its parse and compile.
    pub guard_regex: Option<Hook>,
    /// Charges the worst case of what the call allocates beyond its result.
    pub reserve: Option<Hook>,
    /// The builtin calls functions of the script through [`Rt::call`]. It takes at most four arguments.
    pub calls: bool,
}

impl BuiltinDef {
    pub const fn new(name: &'static str, func: BuiltinFn, arity: i8) -> BuiltinDef {
        BuiltinDef {
            name,
            func,
            arity,
            fast_path: false,
            deep: false,
            stringify: Stringify::No,
            fresh: Fresh::Shallow,
            guard: None,
            guard_regex: None,
            reserve: None,
            calls: false,
        }
    }

    pub const fn fast_path(mut self) -> BuiltinDef {
        self.fast_path = true;
        self
    }

    pub const fn deep(mut self) -> BuiltinDef {
        self.deep = true;
        self
    }

    pub const fn stringify(mut self, how: Stringify) -> BuiltinDef {
        self.stringify = how;
        self
    }

    pub const fn fresh(mut self, how: Fresh) -> BuiltinDef {
        self.fresh = how;
        self
    }

    pub const fn guard(mut self, hook: Hook) -> BuiltinDef {
        self.guard = Some(hook);
        self
    }

    pub const fn guard_regex(mut self, hook: Hook) -> BuiltinDef {
        self.guard_regex = Some(hook);
        self
    }

    pub const fn reserve(mut self, hook: Hook) -> BuiltinDef {
        self.reserve = Some(hook);
        self
    }

    pub const fn calls(mut self) -> BuiltinDef {
        self.calls = true;
        self
    }
}

/// The immutable side of a run.
pub struct Env<'a> {
    pub prog: &'a Program,
    pub builtins: &'a [BuiltinDef],
    /// The name id of the variable that holds the request context, when the run has one.
    pub opaque: Option<u32>,
}

pub struct Rt<'a> {
    pub env: &'a Env<'a>,
    pub heap: &'a mut Heap,
    pub meter: &'a mut Meter,
    pub out: &'a mut Output,
    pub scratch: &'a mut Vec<u8>,
    /// The position of the call.
    pub pos: Position,
    /// The request the run serves.
    pub(crate) ctx: Option<&'a dyn RequestContext>,
    /// The first decision the script wrote.
    pub(crate) verdict: &'a mut Option<Verdict>,
    /// The scopes of the call, for a builtin that is not lent them with `lent`.
    pub(crate) scopes: &'a [ScopeId],
    /// What a builtin that calls functions of the script is lent.
    pub(crate) lent: Option<Lent<'a>>,
}

impl<'a> Rt<'a> {
    /// The request context, as the variable the host named for it holds it: a script that gave that variable
    /// another value has no request for the builtins that read it.
    pub fn request(&self) -> Option<&'a dyn RequestContext> {
        let name = self.env.opaque?;
        let scopes = match &self.lent {
            Some(lent) => lent.scopes(),
            None => self.scopes,
        };
        let held = scopes
            .iter()
            .rev()
            .find_map(|&s| self.heap.scope_get(s, name))?;
        matches!(held, Value::Host(_)).then_some(self.ctx).flatten()
    }

    /// Records the decision of the script. The first one stands.
    pub fn decide(&mut self, v: Verdict) {
        self.verdict.get_or_insert(v);
    }

    pub fn oom(&self) -> impl FnOnce(Oom) -> Exc + use<> {
        Exc::memory(self.pos)
    }

    pub fn type_error(&self, msg: impl Into<String>) -> Exc {
        Exc::type_error(self.pos, msg)
    }

    /// The error of a call with the wrong number of arguments.
    pub fn arity_error(&self, name: &str, required: usize, got: usize) -> Exc {
        let plural = if required == 1 { "" } else { "s" };
        Exc::type_error(
            self.pos,
            format!("{name}() requires {required} arg{plural}, got {got}"),
        )
    }

    /// One step of a loop over input; see [`Meter::work`]. Every loop that runs as long as its input is long
    /// goes through this, so the deadline stops a long call.
    pub fn work(&mut self, count: Count, units: u64) -> Result<(), Exc> {
        self.meter.work(count, units)
    }

    /// A failure to return as a value. The script can keep it and print it, and `if` and `while` raise it.
    pub fn error_value(&mut self, text: &[u8]) -> Result<Value, Exc> {
        let s = self.heap.new_str(text).map_err(self.oom())?;
        Ok(Value::Error(s, error_text_class(text)))
    }

    /// `==` as the script writes it, at the same depth limit and with the same work checks.
    pub fn equal(&mut self, a: Value, b: Value) -> Result<bool, Exc> {
        ops::values_equal(self.heap, self.meter, self.pos, a, b, 0)
    }

    /// `<` as the script writes it.
    pub fn less(&mut self, a: Value, b: Value) -> Result<bool, Exc> {
        ops::value_less(self.heap, self.meter, self.pos, a, b, 0)
    }

    /// Stops the run when a value of `n` bytes or elements may not exist; a negative `n` is an overflowed size.
    pub fn size_check(&self, n: i64) -> Result<(), Exc> {
        self.meter.check_length(self.pos, n)
    }

    /// Charges `n` bytes to the memory budget and returns them, for a hook to hand back; a negative `n` is an
    /// estimate that overflowed and stops the run.
    pub fn reserve(&mut self, n: i64) -> Result<i64, Exc> {
        self.meter.charge(self.pos, n)?;
        Ok(n)
    }

    /// Walks `values` for depth and cycles as the machine walks the arguments of a `deep` builtin, one operation
    /// for each node. Only a builtin whose definition says it `calls` can use this.
    pub fn guard_deep(&mut self, values: &[Value]) -> Result<(), Exc> {
        let Some(lent) = self.lent.as_mut() else {
            return Err(Exc::fault("a builtin that walks values must say it calls"));
        };
        ops::walk_nested(self.heap, self.meter, lent.path, self.pos, values)
    }

    /// Calls a function of the script, or a builtin, with `args`, as the reference interpreter does at the
    /// position of this call: the call depth is counted, the operations of the body are, `return` ends the call
    /// and everything else (an error, a limit) ends this builtin too. Only a builtin whose
    /// definition says it `calls` can use this. `f` must be a `Func` or a `Builtin`; any other value is the
    /// caller's type error.
    pub fn call(&mut self, f: Value, args: &[Value]) -> Result<Value, Exc> {
        match f {
            Value::Func(id) => call_function(self, id, args),
            Value::Builtin(b) => {
                let def = self.env.builtins[usize::from(b)];
                self.meter.check_deadline()?;
                if let Some(err) = arity_mismatch(&def, self.pos, args.len()) {
                    return Err(err);
                }
                if def.deep {
                    let Some(lent) = self.lent.as_mut() else {
                        return Err(Exc::fault("a builtin that calls a function must say so"));
                    };
                    ops::walk_nested(self.heap, self.meter, lent.path, self.pos, args)?;
                }
                invoke(self, &def, args)
            }
            _ => Err(Exc::fault("only a function can be called")),
        }
    }
}

/// The error of a call whose argument count the definition does not take, for a builtin that does not check its
/// own.
pub(crate) fn arity_mismatch(def: &BuiltinDef, pos: Position, argc: usize) -> Option<Exc> {
    if def.fast_path || def.arity < 0 || argc == def.arity as usize {
        return None;
    }
    let plural = if def.arity == 1 { "" } else { "s" };
    Some(Exc::type_error(
        pos,
        format!(
            "{}() requires {} arg{plural}, got {argc}",
            def.name, def.arity
        ),
    ))
}

/// What a builtin can grow, as the call found it: the array of a list, stack or queue, the entries of a map or
/// a set.
#[derive(Clone, Copy)]
enum Grow {
    List(ListId, Shape),
    Map(MapId, usize),
    Set(SetId, usize),
}

/// The growable arguments of a call, in the order they came; a call with more than a few of them is rare.
struct Before {
    first: [Option<Grow>; 6],
    n: usize,
    more: Vec<Grow>,
}

impl Before {
    fn take(rt: &Rt<'_>, args: &[Value]) -> Result<Before, Exc> {
        let mut b = Before {
            first: [None; 6],
            n: 0,
            more: Vec::new(),
        };
        for &a in args {
            let g = match a {
                Value::List(l) | Value::Stack(l) | Value::Queue(l) => {
                    Grow::List(l, rt.heap.shape(l))
                }
                Value::Map(m) => Grow::Map(m, rt.heap.map_len(m)),
                Value::Set(s) => Grow::Set(s, rt.heap.set_len(s)),
                _ => continue,
            };
            if b.n < b.first.len() {
                b.first[b.n] = Some(g);
                b.n += 1;
            } else {
                b.more
                    .try_reserve(1)
                    .map_err(|_| Exc::limit(LimitReason::Memory, rt.pos))?;
                b.more.push(g);
            }
        }
        Ok(b)
    }

    fn all(&self) -> impl Iterator<Item = Grow> + '_ {
        self.first[..self.n]
            .iter()
            .flatten()
            .copied()
            .chain(self.more.iter().copied())
    }
}

/// What the call added to its growable arguments: a list that moved to a new array costs the whole array, one
/// that grew in place the elements added; a map costs an entry for each key added, a set an entry, an order slot
/// and a copy of the key (a string argument is that key).
fn growth(rt: &Rt<'_>, before: &Before, args: &[Value]) -> i64 {
    let mut total = 0;
    for g in before.all() {
        total += match g {
            Grow::List(l, old) => {
                let now = rt.heap.shape(l);
                if now.cap > 0 && now.arr != old.arr {
                    LIST_COST + WORD_COST * now.cap as i64
                } else if now.len > old.len {
                    WORD_COST * (now.len - old.len) as i64
                } else {
                    0
                }
            }
            Grow::Map(m, len) => MAP_ENTRY_COST * rt.heap.map_len(m).saturating_sub(len) as i64,
            Grow::Set(s, len) => {
                let n = rt.heap.set_len(s).saturating_sub(len) as i64;
                let keys: i64 = args
                    .iter()
                    .map(|a| match a {
                        Value::Str(k) => i64::from(k.len) + 2,
                        _ => 0,
                    })
                    .sum();
                (MAP_ENTRY_COST + WORD_COST + keys) * n
            }
        };
    }
    total
}

/// A call that the machine has checked the argument count of: the hooks, the call and the settlement.
pub(crate) fn invoke(rt: &mut Rt<'_>, def: &BuiltinDef, args: &[Value]) -> Result<Value, Exc> {
    if rt.meter.lim.max_value_len > 0 {
        bound_text(rt, def.stringify, args)?;
        if let Some(guard) = def.guard {
            guard(rt, args)?;
        }
    }
    let mut reserved = 0;
    if let Some(regex) = def.guard_regex {
        reserved += regex(rt, args)?;
    }
    if rt.meter.lim.max_alloc_bytes == 0 {
        return (def.func)(rt, args);
    }
    if let Some(reserve) = def.reserve {
        reserved += reserve(rt, args)?;
    }
    let before = Before::take(rt, args)?;
    let res = (def.func)(rt, args)?;
    let res = cut_error(rt, res)?;
    let used = growth(rt, &before, args) + result_cost(rt, res, args, def.fresh)?;
    rt.meter.replace_reservation(rt.pos, reserved, used)?;
    Ok(res)
}

/// Stops the run when the text of the arguments would be longer than a value may be.
fn bound_text(rt: &mut Rt<'_>, how: Stringify, args: &[Value]) -> Result<(), Exc> {
    let json = match how {
        Stringify::No => return Ok(()),
        Stringify::Text => false,
        Stringify::Json => true,
    };
    let max = rt.meter.lim.max_value_len as i64;
    let mut total = 0;
    for &a in args {
        total += ops::text_size_bound(rt.heap, a, max, json, json);
        if total > max {
            break;
        }
    }
    rt.meter.check_length(rt.pos, total)
}

/// An error value cut to the length a caught error may have, on a rune boundary, with `...` after the cut.
fn cut_error(rt: &mut Rt<'_>, res: Value) -> Result<Value, Exc> {
    let Value::Error(s, _) = res else {
        return Ok(res);
    };
    let bytes = rt.heap.str_bytes(s);
    let (keep, cut) = text::truncate_len(bytes, MAX_CAUGHT_ERROR);
    if !cut {
        return Ok(res);
    }
    let mut msg = bytes[..keep].to_vec();
    msg.extend_from_slice(b"...");
    // The reference makes a new plain error of the cut text: the class is gone.
    let s = rt.heap.new_str(&msg).map_err(rt.oom())?;
    Ok(Value::Error(s, None))
}

/// What a caught error or an error value a builtin returned may carry, in bytes, before the cut.
pub(crate) const MAX_CAUGHT_ERROR: usize = 1024;

/// What a call's result costs: the fresh part of it, nothing for a value it passed through.
fn result_cost(rt: &mut Rt<'_>, res: Value, args: &[Value], fresh: Fresh) -> Result<i64, Exc> {
    Ok(match res {
        Value::Null | Value::Bool(_) | Value::Func(_) | Value::Builtin(_) | Value::Host(_) => 0,
        Value::Set(_) | Value::Stack(_) | Value::Queue(_) => {
            if args.iter().any(|a| same_object(*a, res)) {
                0
            } else {
                OBJECT_COST
            }
        }
        Value::Int(_) | Value::Float(_) => SCALAR_COST,
        Value::Error(s, _) => WORD_COST + i64::from(s.len),
        Value::Str(s) => string_cost(s, args),
        Value::List(l) => {
            if args.iter().any(|a| matches!(a, Value::List(o) if *o == l)) {
                return Ok(0);
            }
            let shallow = LIST_COST + WORD_COST * rt.heap.list(l).go_cap as i64;
            match fresh {
                Fresh::Shallow => shallow,
                Fresh::Elements => shallow + element_cost(rt, l, args)?,
                Fresh::Tree => tree_cost(rt, res)?,
            }
        }
        Value::Map(m) => {
            if args.iter().any(|a| matches!(a, Value::Map(o) if *o == m)) {
                return Ok(0);
            }
            match fresh {
                Fresh::Tree => tree_cost(rt, res)?,
                _ => MAP_COST + MAP_ENTRY_COST * rt.heap.map_len(m) as i64,
            }
        }
    })
}

/// Whether two values are the same set, stack or queue.
fn same_object(a: Value, b: Value) -> bool {
    match (a, b) {
        (Value::Set(x), Value::Set(y)) => x == y,
        (Value::Stack(x), Value::Stack(y)) | (Value::Queue(x), Value::Queue(y)) => x == y,
        _ => false,
    }
}

/// A string a call made: its header, and its bytes unless it shares them with a string argument.
fn string_cost(s: Str, args: &[Value]) -> i64 {
    if args
        .iter()
        .any(|a| matches!(a, Value::Str(o) if s.within(*o)))
    {
        WORD_COST
    } else {
        WORD_COST + i64::from(s.len)
    }
}

/// What the strings and numbers of a result list cost.
fn element_cost(rt: &mut Rt<'_>, list: ListId, args: &[Value]) -> Result<i64, Exc> {
    let len = rt.heap.list(list).items.len();
    rt.work(Count::Free, len as u64)?;
    let mut total = 0;
    for &e in &rt.heap.list(list).items {
        total += match e {
            Value::Str(s) => string_cost(s, args),
            Value::Int(_) | Value::Float(_) => SCALAR_COST,
            _ => 0,
        };
    }
    Ok(total)
}

/// What a tree built by the call costs, counted until it passes the memory limit: past it the caller only needs
/// to know the run is over.
fn tree_cost(rt: &mut Rt<'_>, root: Value) -> Result<i64, Exc> {
    let budget = rt.meter.lim.max_alloc_bytes as i64;
    let memory = Exc::limit(LimitReason::Memory, rt.pos);
    let mut total = 0;
    let mut stack = vec![root];
    while total <= budget {
        let Some(v) = stack.pop() else {
            break;
        };
        rt.work(Count::Free, 1)?;
        match v {
            Value::Str(s) => total += WORD_COST + i64::from(s.len),
            Value::Int(_) | Value::Float(_) => total += SCALAR_COST,
            Value::List(l) => {
                let list = rt.heap.list(l);
                total += LIST_COST + WORD_COST * list.go_cap as i64;
                stack
                    .try_reserve(list.items.len())
                    .map_err(|_| memory.clone())?;
                stack.extend_from_slice(&list.items);
            }
            Value::Map(m) => {
                total += MAP_COST;
                let entries = rt.heap.map_entries(m);
                stack
                    .try_reserve(entries.len())
                    .map_err(|_| memory.clone())?;
                for &(k, e) in entries {
                    total += MAP_ENTRY_COST + i64::from(k.len);
                    stack.push(e);
                }
            }
            _ => {}
        }
    }
    Ok(total)
}

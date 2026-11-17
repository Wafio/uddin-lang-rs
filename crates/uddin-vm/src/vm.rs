//! The machine: runs a [`Program`] on a stack of values and a stack of scopes.
//!
//! Control flow that the language implements by unwinding (`return`, `break`, `continue`, errors) travels as a
//! [`Exc`] to the nearest handler on the handler stack, which is the same stack of active blocks, loop bodies,
//! `try` regions and calls that the script is inside. Limits are an `Exc` that no handler takes.

use uddin_syntax::Position;

use crate::builtin::{BuiltinDef, Env, MAX_CAUGHT_ERROR, Rt, arity_mismatch, invoke};
use crate::bytecode::{Insn, Op, Program};
use crate::error::{Exc, LimitReason, error_value_exc};
use crate::heap::{Heap, Oom};
use crate::host::{RequestContext, Verdict};
use crate::meter::{Count, Limits, Meter, Output};
use crate::ops::{self, LIST_COST, MAP_COST, MAP_ENTRY_COST, PathEntry, WORD_COST};
use crate::text;
use crate::value::{FuncId, ScopeId, Str, Value, host_type_name, type_name};

/// The capacity the reference implementation gives a small list literal.
const SMALL_LIST_CAP: usize = 16;
/// Handlers on the stack at once: two per active block, one per call, and a few for the top level.
const HANDLER_CAP: usize = 1024;
const STACK_CAP: usize = 256;
/// The most arguments a builtin that calls back into the script takes.
const CALLBACK_ARGS: usize = 4;

/// A value the embedder supplies to a script as a global variable.
#[derive(Clone, Copy, Debug)]
pub enum HostValue<'a> {
    Str(&'a [u8]),
    Int(i64),
    Float(f64),
    /// A value only builtins can read (the request context).
    Opaque,
}

/// How a run ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ended {
    Ok,
    /// A runtime error; `class` is `type`, `value`, `name` or `runtime`.
    Error {
        class: &'static str,
        text: String,
    },
    Limit {
        reason: LimitReason,
        text: String,
    },
}

#[derive(Clone, Copy)]
enum Handler {
    /// A block is active: the nesting counter goes back down when it ends.
    Block,
    /// The body of a loop: `break` and `continue` end here, with the scope stack cut back to `scopes`.
    Loop {
        brk: u32,
        cont: u32,
        sp: u32,
        scopes: u32,
    },
    /// A `try` region: everything but a limit, a `return`, a `break` and a `continue` becomes the text bound by the
    /// `catch`.
    Try {
        catch: u32,
        sp: u32,
        name: u32,
        pos: Position,
    },
    /// A user function call: `return` ends here. `scopes` is the depth of the scope stack before the call pushed its
    /// own; a `return` cuts the stack back to it, whatever a `try` or a `catch` left above.
    Call { ret: u32, base: u32, scopes: u32 },
    /// Marks where native code called into the script: whatever travels out of the called function goes on to
    /// the native code, which hands it to its own caller.
    Barrier,
}

/// Everything a run mutates. One lives per thread and is reused, so its vectors keep their capacity.
pub struct RunState {
    heap: Heap,
    meter: Meter,
    out: Output,
    stack: Vec<Value>,
    handlers: Vec<Handler>,
    scopes: Vec<ScopeId>,
    path: Vec<PathEntry>,
    scratch: Vec<u8>,
    seen: Vec<u32>,
    seen_gen: u32,
    verdict: Option<Verdict>,
}

/// One run of the machine over the parts of a [`RunState`]. A builtin that calls a function of the script
/// gets the parts lent through [`Lent`] and builds a second machine over them, which runs that call.
struct Machine<'a> {
    heap: &'a mut Heap,
    meter: &'a mut Meter,
    out: &'a mut Output,
    scratch: &'a mut Vec<u8>,
    stack: &'a mut Vec<Value>,
    handlers: &'a mut Vec<Handler>,
    scopes: &'a mut Vec<ScopeId>,
    path: &'a mut Vec<PathEntry>,
    seen: &'a mut Vec<u32>,
    seen_gen: &'a mut u32,
    ctx: Option<&'a dyn RequestContext>,
    verdict: &'a mut Option<Verdict>,
    pc: usize,
}

/// The parts of the machine a builtin that calls back into the script is lent: everything but what the
/// builtin itself holds.
pub(crate) struct Lent<'a> {
    stack: &'a mut Vec<Value>,
    handlers: &'a mut Vec<Handler>,
    scopes: &'a mut Vec<ScopeId>,
    pub(crate) path: &'a mut Vec<PathEntry>,
    seen: &'a mut Vec<u32>,
    seen_gen: &'a mut u32,
}

impl Lent<'_> {
    pub(crate) fn scopes(&self) -> &[ScopeId] {
        self.scopes
    }
}

/// Runs a function of the script from native code as the reference interpreter runs one from a builtin: the
/// same depth limit, the same operations counted, `return` ends the call, and an error goes on to the caller of
/// the builtin.
pub(crate) fn call_function(rt: &mut Rt<'_>, f: FuncId, args: &[Value]) -> Result<Value, Exc> {
    let Rt {
        env,
        heap,
        meter,
        out,
        scratch,
        pos,
        ctx,
        verdict,
        scopes: _,
        lent,
    } = rt;
    let Some(lent) = lent.as_mut() else {
        return Err(Exc::fault("a builtin that calls a function must say so"));
    };
    let mut machine = Machine {
        heap,
        meter,
        out,
        scratch,
        stack: &mut *lent.stack,
        handlers: &mut *lent.handlers,
        scopes: &mut *lent.scopes,
        path: &mut *lent.path,
        seen: &mut *lent.seen,
        seen_gen: &mut *lent.seen_gen,
        ctx: *ctx,
        verdict,
        pc: 0,
    };
    machine.call_nested(env, f, args, *pos)
}

impl Default for RunState {
    fn default() -> RunState {
        RunState::new()
    }
}

fn fault_text(pos: Position, fault: &[u8]) -> (&'static str, String) {
    let pos = if pos.line == 0 {
        Position { line: 1, column: 1 }
    } else {
        pos
    };
    let has = |needle: &str| memchr::memmem::find(fault, needle.as_bytes()).is_some();
    let msg = if has("nil pointer dereference") || has("invalid memory address") {
        "memory access error: attempting to access invalid or uninitialized data".to_owned()
    } else if has("index out of range") {
        "array or string index is out of bounds".to_owned()
    } else if has("slice bounds out of range") {
        "slice operation is out of bounds".to_owned()
    } else {
        format!(
            "unexpected runtime error: {}",
            String::from_utf8_lossy(fault)
        )
    };
    (
        "runtime",
        format!("runtime error at {}:{}: {}", pos.line, pos.column, msg),
    )
}

fn ended(exc: Exc, cur: Position) -> Ended {
    let error = |class: &'static str, text: String| Ended::Error { class, text };
    match exc {
        Exc::Break(p) => error(
            "runtime",
            Exc::runtime_error(p, "break statement not within a loop").text(),
        ),
        Exc::Continue(p) => error(
            "runtime",
            Exc::runtime_error(p, "continue statement not within a loop").text(),
        ),
        Exc::Return(_, p) => error(
            "runtime",
            Exc::runtime_error(p, "can't return at top level").text(),
        ),
        Exc::Error { class, .. } => error(class.word(), exc.text()),
        Exc::Fault(msg) => {
            let (class, text) = fault_text(cur, &msg);
            error(class, text)
        }
        Exc::Limit(reason, _) => Ended::Limit {
            reason,
            text: exc.text(),
        },
    }
}

impl RunState {
    pub fn new() -> RunState {
        let lim = Limits::default();
        RunState {
            heap: Heap::new(lim.heap_cap),
            meter: Meter::new(lim),
            out: Output::new(),
            stack: Vec::new(),
            handlers: Vec::new(),
            scopes: Vec::new(),
            path: Vec::new(),
            scratch: Vec::new(),
            seen: Vec::new(),
            seen_gen: 0,
            verdict: None,
        }
    }

    /// The first decision the last run wrote, if it wrote one.
    pub fn verdict(&self) -> Option<Verdict> {
        self.verdict
    }

    /// What `print` wrote, when the run captured it.
    pub fn printed(&self) -> &[u8] {
        &self.out.buf
    }

    /// Whether the captured text hit its cap and the rest was dropped.
    pub fn print_truncated(&self) -> bool {
        self.out.truncated
    }

    /// Operations the last run counted.
    pub fn ops(&self) -> u64 {
        self.meter.ops
    }

    /// Bytes the run heap holds on to for the next run.
    pub fn retained_bytes(&self) -> usize {
        self.heap.retained()
            + self.stack.capacity() * 16
            + self.handlers.capacity() * 24
            + self.scopes.capacity() * 4
            + self.path.capacity() * 16
            + self.scratch.capacity()
            + self.seen.capacity() * 4
    }

    /// Runs `prog` to its end or its first limit. `ctx` is the request that the builtins reading the request see,
    /// through the variable the host marked [`HostValue::Opaque`].
    pub fn run(
        &mut self,
        prog: &Program,
        builtins: &[BuiltinDef],
        host: &[HostValue<'_>],
        lim: &Limits,
        ctx: Option<&dyn RequestContext>,
    ) -> Ended {
        self.verdict = None;
        if self.begin(prog, host, lim).is_err() {
            return Ended::Limit {
                reason: LimitReason::Memory,
                text: Exc::Limit(LimitReason::Memory, Position::default()).text(),
            };
        }
        let opaque = host
            .iter()
            .position(|h| matches!(h, HostValue::Opaque))
            .and_then(|i| prog.host_names.get(i).copied());
        let env = Env {
            prog,
            builtins,
            opaque,
        };
        let mut machine = Machine {
            heap: &mut self.heap,
            meter: &mut self.meter,
            out: &mut self.out,
            scratch: &mut self.scratch,
            stack: &mut self.stack,
            handlers: &mut self.handlers,
            scopes: &mut self.scopes,
            path: &mut self.path,
            seen: &mut self.seen,
            seen_gen: &mut self.seen_gen,
            ctx,
            verdict: &mut self.verdict,
            pc: 0,
        };
        let mut result = machine.drive(&env);
        while let Err(exc) = result {
            result = match machine.unwind(exc) {
                Ok(()) => machine.drive(&env),
                Err(exc) => return ended(exc, machine.meter.cur_pos),
            };
        }
        Ended::Ok
    }

    fn begin(&mut self, prog: &Program, host: &[HostValue<'_>], lim: &Limits) -> Result<(), Oom> {
        self.heap
            .reset(prog.bytes.clone(), lim.heap_cap, lim.keep_heap);
        self.meter.start(lim.clone());
        self.out.start(lim.capture_print, lim.print_cap);
        self.stack.clear();
        self.handlers.clear();
        self.scopes.clear();
        self.path.clear();
        self.scratch.clear();
        self.stack.try_reserve(STACK_CAP).map_err(|_| Oom)?;
        self.handlers.try_reserve(HANDLER_CAP).map_err(|_| Oom)?;
        self.scopes.try_reserve(2 * 64 + 8).map_err(|_| Oom)?;
        if self.seen.len() < prog.names.len() {
            self.seen.try_reserve(prog.names.len()).map_err(|_| Oom)?;
            self.seen.resize(prog.names.len(), 0);
        }
        let global = self.heap.new_scope()?;
        self.heap.capture_scope(global);
        self.scopes.push(global);
        for &(name, v) in prog.seed.iter() {
            self.heap.scope_set(global, name, v)?;
        }
        for (i, h) in host.iter().enumerate() {
            let Some(&name) = prog.host_names.get(i) else {
                break;
            };
            let v = match *h {
                HostValue::Str(b) => Value::Str(self.heap.new_str(b)?),
                HostValue::Int(i) => Value::Int(i),
                HostValue::Float(f) => Value::Float(f),
                HostValue::Opaque => Value::Host(0),
            };
            self.heap.scope_set(global, name, v)?;
        }
        Ok(())
    }
}

impl Machine<'_> {
    fn top_scope(&self) -> ScopeId {
        *self
            .scopes
            .last()
            .expect("the global scope is never popped")
    }

    fn pop_scope(&mut self) {
        if let Some(id) = self.scopes.pop() {
            self.heap.release_scope(id);
        }
    }

    fn lookup(&self, name: u32) -> Option<Value> {
        self.scopes
            .iter()
            .rev()
            .find_map(|&s| self.heap.scope_get(s, name))
    }

    fn push_handler(&mut self, h: Handler, pos: Position) -> Result<(), Exc> {
        self.handlers
            .try_reserve(1)
            .map_err(|_| Exc::limit(LimitReason::Memory, pos))?;
        self.handlers.push(h);
        Ok(())
    }

    fn push_value(&mut self, v: Value, pos: Position) -> Result<(), Exc> {
        self.stack
            .try_reserve(1)
            .map_err(|_| Exc::limit(LimitReason::Memory, pos))?;
        self.stack.push(v);
        Ok(())
    }

    fn pop(&mut self) -> Value {
        self.stack.pop().expect("the compiler balances the stack")
    }

    fn name_error(&self, env: &Env, name: u32, pos: Position) -> Exc {
        let mut text = b"name ".to_vec();
        text::quote(env.prog.name(name).as_bytes(), &mut text);
        text.extend_from_slice(b" not found");
        Exc::name_error(pos, String::from_utf8_lossy(&text).into_owned())
    }

    /// Runs until the program ends or something is thrown.
    fn drive(&mut self, env: &Env) -> Result<(), Exc> {
        let prog = env.prog;
        loop {
            let Insn { op, a, b } = prog.code[self.pc];
            let pos = prog.pos[self.pc];
            self.pc += 1;
            match op {
                Op::Tick => {
                    self.meter.cur_pos = pos;
                    self.meter.tick()?;
                }
                Op::TickStmt => {
                    self.meter.tick()?;
                    self.meter.cur_pos = pos;
                }
                Op::LoopTick => self.meter.tick()?,
                Op::ConstT => {
                    self.meter.cur_pos = pos;
                    self.meter.tick()?;
                    self.push_value(prog.consts[a as usize], pos)?;
                }
                Op::LoadVarT => {
                    self.meter.cur_pos = pos;
                    self.meter.tick()?;
                    let v = self.lookup(a).ok_or_else(|| self.name_error(env, a, pos))?;
                    self.push_value(v, pos)?;
                }
                Op::LoadBuiltinT => {
                    self.meter.cur_pos = pos;
                    self.meter.tick()?;
                    self.push_value(Value::Builtin(a as u16), pos)?;
                }
                Op::NameErrT => {
                    self.meter.cur_pos = pos;
                    self.meter.tick()?;
                    return Err(self.name_error(env, a, pos));
                }
                Op::LoadVarC => {
                    let v = self.lookup(a).ok_or_else(|| self.name_error(env, a, pos))?;
                    self.push_value(v, pos)?;
                }
                Op::StoreVar => {
                    let v = self.pop();
                    let top = self.top_scope();
                    self.heap.scope_set(top, a, v).map_err(Exc::memory(pos))?;
                }
                Op::Pop => {
                    self.pop();
                }
                Op::Swap => {
                    let n = self.stack.len();
                    self.stack.swap(n - 1, n - 2);
                }
                Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Mod | Op::Pow | Op::Xor => {
                    let (r, l) = (self.pop(), self.pop());
                    let v = match op {
                        Op::Add => ops::plus(self.heap, self.meter, pos, l, r)?,
                        Op::Sub => ops::minus(pos, l, r)?,
                        Op::Mul => ops::times(self.heap, self.meter, pos, l, r)?,
                        Op::Div => ops::divide(pos, l, r)?,
                        Op::Mod => ops::modulo(pos, l, r)?,
                        Op::Pow => ops::power(pos, l, r)?,
                        _ => Value::Bool(ops::truthy(self.heap, l) != ops::truthy(self.heap, r)),
                    };
                    self.stack.push(v);
                }
                Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge | Op::In => {
                    let (r, l) = (self.pop(), self.pop());
                    ops::walk_nested(self.heap, self.meter, self.path, pos, &[l, r])?;
                    let (h, m) = (&self.heap, &mut self.meter);
                    let v = match op {
                        Op::Eq => ops::values_equal(h, m, pos, l, r, 0)?,
                        Op::Ne => !ops::values_equal(h, m, pos, l, r, 0)?,
                        Op::Lt => ops::value_less(h, m, pos, l, r, 0)?,
                        Op::Le => !ops::value_less(h, m, pos, r, l, 0)?,
                        Op::Gt => ops::value_less(h, m, pos, r, l, 0)?,
                        Op::Ge => !ops::value_less(h, m, pos, l, r, 0)?,
                        _ => ops::is_member(h, m, pos, l, r)?,
                    };
                    self.stack.push(Value::Bool(v));
                }
                Op::Not => {
                    let v = self.pop();
                    self.stack.push(ops::not(pos, v)?);
                }
                Op::Neg => {
                    let v = self.pop();
                    self.stack.push(ops::negate(pos, v)?);
                }
                Op::AndLeft | Op::OrLeft => {
                    let is_and = op == Op::AndLeft;
                    match self.pop() {
                        Value::Bool(v) if v == !is_and => {
                            self.stack.push(Value::Bool(v));
                            self.pc = a as usize;
                        }
                        Value::Bool(_) => {}
                        _ => {
                            let word = if is_and { "and" } else { "or" };
                            return Err(Exc::type_error(pos, format!("{word} requires two bools")));
                        }
                    }
                }
                Op::BoolCheckAnd | Op::BoolCheckOr => {
                    if !matches!(self.stack.last(), Some(Value::Bool(_))) {
                        let word = if op == Op::BoolCheckAnd { "and" } else { "or" };
                        return Err(Exc::type_error(pos, format!("{word} requires two bools")));
                    }
                }
                Op::TernaryJump => {
                    let v = self.pop();
                    if !ops::truthy(self.heap, v) {
                        self.pc = a as usize;
                    }
                }
                Op::Jump => self.pc = a as usize,
                Op::IfCond => match self.pop() {
                    Value::Bool(true) => {}
                    Value::Bool(false) => self.pc = a as usize,
                    Value::Error(s, class) => {
                        return Err(error_value_exc(self.heap.str_bytes(s), class));
                    }
                    v => {
                        return Err(Exc::type_error(
                            pos,
                            format!("if condition must be bool, got {}", type_name(v)),
                        ));
                    }
                },
                Op::WhileCond => match self.pop() {
                    Value::Bool(true) => {}
                    Value::Bool(false) => self.pc = a as usize,
                    Value::Error(s, class) => {
                        return Err(error_value_exc(self.heap.str_bytes(s), class));
                    }
                    v => {
                        return Err(Exc::type_error(
                            pos,
                            format!("while condition must be bool, got {}", host_type_name(v)),
                        ));
                    }
                },
                Op::EnterBlock => {
                    self.meter.nesting += 1;
                    let max = self.meter.lim.max_nesting;
                    if max > 0 && self.meter.nesting > max {
                        self.meter.nesting -= 1;
                        return Err(Exc::limit(LimitReason::Nesting, self.meter.cur_pos));
                    }
                    self.push_handler(Handler::Block, pos)?;
                }
                Op::ExitBlock => {
                    self.handlers.pop();
                    self.meter.nesting -= 1;
                }
                Op::PushLoop => {
                    let sp = self.stack.len() as u32;
                    let scopes = self.scopes.len() as u32;
                    self.push_handler(
                        Handler::Loop {
                            brk: a,
                            cont: b,
                            sp,
                            scopes,
                        },
                        pos,
                    )?;
                }
                Op::PopLoop => {
                    self.handlers.pop();
                }
                Op::ForInit => self.for_init(pos)?,
                Op::ForNext => {
                    let n = self.stack.len();
                    let (Value::Int(arr), Value::Int(idx), Value::Int(end)) =
                        (self.stack[n - 3], self.stack[n - 2], self.stack[n - 1])
                    else {
                        unreachable!("the loop keeps its position on the stack");
                    };
                    if idx >= end {
                        self.pc = a as usize;
                        continue;
                    }
                    self.meter.tick()?;
                    let (v, next) =
                        self.next_item(self.stack[n - 4], arr as u32, idx as usize, pos)?;
                    self.stack[n - 2] = Value::Int(next as i64);
                    let top = self.top_scope();
                    self.heap.scope_set(top, b, v).map_err(Exc::memory(pos))?;
                }
                Op::MakeAnon => {
                    let closure = self.top_scope();
                    self.heap.capture_scope(closure);
                    let f = self.heap.new_func(a, closure).map_err(Exc::memory(pos))?;
                    self.push_value(Value::Func(f), pos)?;
                }
                Op::DefFunc => self.define_function(a, b, pos)?,
                Op::SubGet => {
                    let (s, c) = (self.pop(), self.pop());
                    let v = ops::subscript_get(self.heap, pos, c, s)?;
                    self.stack.push(v);
                }
                Op::SubSet => {
                    let (v, s, c) = (self.pop(), self.pop(), self.pop());
                    ops::subscript_set(self.heap, self.meter, pos, c, s, v)?;
                }
                Op::SubLoad => {
                    let n = self.stack.len();
                    let v =
                        ops::subscript_get(self.heap, pos, self.stack[n - 3], self.stack[n - 2])?;
                    self.push_value(v, pos)?;
                }
                Op::ChargeList => self
                    .meter
                    .charge(pos, LIST_COST + WORD_COST * i64::from(a))?,
                Op::MakeList => {
                    let n = a as usize;
                    let cap = n.max(SMALL_LIST_CAP);
                    let id = self.heap.new_list(n, cap).map_err(Exc::memory(pos))?;
                    let from = self.stack.len() - n;
                    for i in from..self.stack.len() {
                        self.heap
                            .list_push(id, self.stack[i])
                            .map_err(Exc::memory(pos))?;
                    }
                    self.stack.truncate(from);
                    self.stack.push(Value::List(id));
                }
                Op::ChargeMap => self
                    .meter
                    .charge(pos, MAP_COST + MAP_ENTRY_COST * i64::from(a))?,
                Op::MapKey => {
                    let k = *self.stack.last().expect("a key was just evaluated");
                    if !matches!(k, Value::Str(_)) {
                        return Err(Exc::type_error(
                            pos,
                            format!("object key must be string, not {}", type_name(k)),
                        ));
                    }
                }
                Op::MakeMap => {
                    let n = a as usize;
                    let id = self.heap.new_map(n).map_err(Exc::memory(pos))?;
                    let from = self.stack.len() - 2 * n;
                    for i in 0..n {
                        let (Value::Str(k), v) =
                            (self.stack[from + 2 * i], self.stack[from + 2 * i + 1])
                        else {
                            unreachable!("map keys are checked as they are built");
                        };
                        self.heap.map_set(id, k, v).map_err(Exc::memory(pos))?;
                    }
                    self.stack.truncate(from);
                    self.stack.push(Value::Map(id));
                }
                Op::CheckCallable => {
                    let v = *self.stack.last().expect("a callee was just evaluated");
                    if !matches!(v, Value::Func(_) | Value::Builtin(_)) {
                        return Err(Exc::type_error(
                            pos,
                            format!("can't call non-function type {}", type_name(v)),
                        ));
                    }
                }
                Op::Call => self.call(env, pos, a as usize)?,
                Op::Spread => self.spread(pos, a as usize)?,
                Op::CallDyn => {
                    let Value::Int(n) = self.pop() else {
                        unreachable!("a spread leaves its argument count");
                    };
                    self.call(env, pos, n as usize)?;
                }
                Op::Return => {
                    let v = self.pop();
                    return Err(Exc::Return(v, pos));
                }
                Op::Break => return Err(Exc::Break(pos)),
                Op::Continue => return Err(Exc::Continue(pos)),
                Op::PushScope => {
                    let id = self.heap.new_scope().map_err(Exc::memory(pos))?;
                    self.scopes
                        .try_reserve(1)
                        .map_err(|_| Exc::limit(LimitReason::Memory, pos))?;
                    self.scopes.push(id);
                }
                Op::PopScope => self.pop_scope(),
                Op::TryEnter => {
                    let sp = self.stack.len() as u32;
                    self.push_handler(
                        Handler::Try {
                            catch: a,
                            sp,
                            name: b,
                            pos,
                        },
                        pos,
                    )?;
                }
                Op::TryExit => {
                    self.handlers.pop();
                    self.pop_scope();
                }
                Op::EndCall => self.finish_call(Value::Null),
                Op::CallMain => {
                    if let Some(f @ Value::Func(_)) = self.lookup(prog.main_name) {
                        self.push_value(f, pos)?;
                        self.call(env, Position::default(), 0)?;
                    }
                }
                Op::ImportErr => return Err(Exc::name_error(pos, "import is not available")),
                Op::Halt => return Ok(()),
            }
        }
    }

    fn for_init(&mut self, pos: Position) -> Result<(), Exc> {
        let v = self.pop();
        if let Value::Map(m) = v {
            self.meter
                .charge(pos, LIST_COST + WORD_COST * self.heap.map_len(m) as i64)?;
            self.meter.work(Count::Free, self.heap.map_len(m) as u64)?;
        }
        // A list is read through the header it has now: the array it points into, where the window starts and how
        // long it is. What the body changes in that array shows; what moves the list to another array does not.
        let (src, arr, start, end) = match v {
            Value::Str(s) => (v, 0, 0, s.len as usize),
            Value::List(l) => {
                let h = self.heap.snapshot(l);
                (v, h.arr, h.off, h.off + h.len)
            }
            Value::Map(m) => {
                let n = self.heap.map_len(m);
                let keys = self.heap.new_list(n, n).map_err(Exc::memory(pos))?;
                for i in 0..n {
                    let k = self.heap.map_entries(m)[i].0;
                    self.heap
                        .list_push(keys, Value::Str(k))
                        .map_err(Exc::memory(pos))?;
                }
                (Value::List(keys), 0, 0, n)
            }
            _ => {
                return Err(Exc::type_error(
                    pos,
                    format!(
                        "for requires a string, array, or object to loop over, got {}",
                        type_name(v)
                    ),
                ));
            }
        };
        for item in [
            src,
            Value::Int(i64::from(arr)),
            Value::Int(start as i64),
            Value::Int(end as i64),
        ] {
            self.push_value(item, pos)?;
        }
        Ok(())
    }

    /// The item at `idx` of the iteration state `src`, over the array `arr` of a list, and the index after it.
    fn next_item(
        &mut self,
        src: Value,
        arr: u32,
        idx: usize,
        pos: Position,
    ) -> Result<(Value, usize), Exc> {
        match src {
            Value::Str(s) => {
                let (r, w) = text::decode_rune(&self.heap.str_bytes(s)[idx..]);
                let item = if r < 0x80 {
                    Str::byte(r as u8)
                } else if r == text::RUNE_ERROR && w == 1 {
                    self.heap
                        .new_str("\u{FFFD}".as_bytes())
                        .map_err(Exc::memory(pos))?
                } else {
                    Str {
                        off: s.off + idx as u32,
                        len: w as u32,
                    }
                };
                Ok((Value::Str(item), idx + w))
            }
            Value::List(l) => Ok((self.heap.read_at(l, arr, idx), idx + 1)),
            _ => unreachable!("iteration state holds a string or a list"),
        }
    }

    fn spread(&mut self, pos: Position, fixed: usize) -> Result<(), Exc> {
        let v = self.pop();
        let n = match v {
            Value::Str(s) => 2 * count_runes(self.heap.str_bytes(s)),
            Value::List(l) => self.heap.list(l).items.len(),
            Value::Map(m) => 2 * self.heap.map_len(m),
            _ => 0,
        };
        self.meter.charge(pos, WORD_COST * n as i64)?;
        self.meter.work(Count::Free, n as u64)?;
        let before = self.stack.len();
        match v {
            Value::List(l) => {
                for i in 0..self.heap.list(l).items.len() {
                    let item = self.heap.list(l).items[i];
                    self.push_value(item, pos)?;
                }
            }
            Value::Map(m) => {
                for i in 0..self.heap.map_len(m) {
                    let k = self.heap.map_entries(m)[i].0;
                    self.push_value(Value::Str(k), pos)?;
                }
            }
            Value::Str(s) => {
                let mut idx = 0;
                while idx < s.len as usize {
                    let (item, next) = self.next_item(v, 0, idx, pos)?;
                    self.push_value(item, pos)?;
                    idx = next;
                }
            }
            _ => {
                return Err(Exc::type_error(
                    pos,
                    format!(
                        "spread requires a string, array, or object, got {}",
                        type_name(v)
                    ),
                ));
            }
        }
        let total = fixed + (self.stack.len() - before);
        self.push_value(Value::Int(total as i64), pos)
    }

    fn define_function(&mut self, proto: u32, name: u32, pos: Position) -> Result<(), Exc> {
        let top = self.top_scope();
        let f = self.heap.new_func(proto, top).map_err(Exc::memory(pos))?;
        self.heap
            .scope_set(top, name, Value::Func(f))
            .map_err(Exc::memory(pos))?;
        let snapshot = self.heap.new_scope().map_err(Exc::memory(pos))?;
        self.heap.capture_scope(snapshot);
        *self.seen_gen = self.seen_gen.wrapping_add(1);
        if *self.seen_gen == 0 {
            self.seen.iter_mut().for_each(|s| *s = 0);
            *self.seen_gen = 1;
        }
        for i in 0..self.scopes.len() {
            let scope = self.scopes[i];
            for j in 0..self.heap.scope(scope).vars.len() {
                let (n, v) = self.heap.scope(scope).vars[j];
                if self.seen[n as usize] != *self.seen_gen {
                    self.seen[n as usize] = *self.seen_gen;
                    self.heap
                        .scope_set(snapshot, n, v)
                        .map_err(Exc::memory(pos))?;
                }
            }
        }
        self.heap.set_func_closure(f, snapshot);
        Ok(())
    }

    fn call(&mut self, env: &Env, pos: Position, argc: usize) -> Result<(), Exc> {
        let base = self.stack.len() - 1 - argc;
        match self.stack[base] {
            Value::Builtin(b) => self.call_builtin(env, pos, usize::from(b), base, argc),
            Value::Func(f) => self.call_user(env, pos, f, base, argc),
            _ => unreachable!("callables are checked before the arguments are evaluated"),
        }
    }

    fn call_builtin(
        &mut self,
        env: &Env,
        pos: Position,
        id: usize,
        base: usize,
        argc: usize,
    ) -> Result<(), Exc> {
        let def = &env.builtins[id];
        self.meter.check_deadline()?;
        if let Some(err) = arity_mismatch(def, pos, argc) {
            return Err(err);
        }
        let res = if def.calls {
            // The builtin runs script functions, which need the stack, so it works on a copy of its arguments.
            let mut local = [Value::Null; CALLBACK_ARGS];
            let args = &mut local[..argc.min(CALLBACK_ARGS)];
            args.copy_from_slice(&self.stack[base + 1..base + 1 + args.len()]);
            if def.deep {
                ops::walk_nested(self.heap, self.meter, self.path, pos, args)?;
            }
            let mut rt = Rt {
                env,
                heap: &mut *self.heap,
                meter: &mut *self.meter,
                out: &mut *self.out,
                scratch: &mut *self.scratch,
                pos,
                ctx: self.ctx,
                verdict: &mut *self.verdict,
                scopes: &[],
                lent: Some(Lent {
                    stack: &mut *self.stack,
                    handlers: &mut *self.handlers,
                    scopes: &mut *self.scopes,
                    path: &mut *self.path,
                    seen: &mut *self.seen,
                    seen_gen: &mut *self.seen_gen,
                }),
            };
            invoke(&mut rt, def, args)?
        } else {
            let args = &self.stack[base + 1..base + 1 + argc];
            if def.deep {
                ops::walk_nested(self.heap, self.meter, self.path, pos, args)?;
            }
            let mut rt = Rt {
                env,
                heap: &mut *self.heap,
                meter: &mut *self.meter,
                out: &mut *self.out,
                scratch: &mut *self.scratch,
                pos,
                ctx: self.ctx,
                verdict: &mut *self.verdict,
                scopes: &*self.scopes,
                lent: None,
            };
            invoke(&mut rt, def, args)?
        };
        self.stack.truncate(base);
        self.stack.push(res);
        Ok(())
    }

    /// Runs the script function `f` on `args`, for a builtin that called it, and returns what it returned. An
    /// exception that ends the call leaves through the barrier, with the state of the machine undone as far as
    /// the barrier.
    fn call_nested(
        &mut self,
        env: &Env,
        f: FuncId,
        args: &[Value],
        pos: Position,
    ) -> Result<Value, Exc> {
        let base = self.stack.len();
        self.push_value(Value::Func(f), pos)?;
        for &a in args {
            self.push_value(a, pos)?;
        }
        self.push_handler(Handler::Barrier, pos)?;
        self.pc = env.prog.halt as usize;
        if let Err(exc) = self.call_user(env, pos, f, base, args.len()) {
            self.handlers.pop();
            self.stack.truncate(base);
            return Err(exc);
        }
        let mut result = self.drive(env);
        while let Err(exc) = result {
            result = match self.unwind(exc) {
                Ok(()) => self.drive(env),
                Err(exc) => {
                    self.stack.truncate(base);
                    return Err(exc);
                }
            };
        }
        self.handlers.pop();
        Ok(self.pop())
    }

    fn call_user(
        &mut self,
        env: &Env,
        pos: Position,
        f: crate::value::FuncId,
        base: usize,
        argc: usize,
    ) -> Result<(), Exc> {
        let max = self.meter.lim.max_call_depth;
        self.meter.call_depth += 1;
        if max > 0 && self.meter.call_depth > max {
            self.meter.call_depth -= 1;
            return Err(Exc::limit(LimitReason::CallDepth, pos));
        }
        let result = self.enter_user(env, pos, f, base, argc);
        if result.is_err() {
            self.meter.call_depth -= 1;
        }
        result
    }

    fn enter_user(
        &mut self,
        env: &Env,
        pos: Position,
        f: crate::value::FuncId,
        base: usize,
        mut argc: usize,
    ) -> Result<(), Exc> {
        let obj = self.heap.func(f);
        let (proto_id, closure) = (obj.proto, obj.closure);
        let proto = &env.prog.protos[proto_id as usize];
        let want = proto.params.len();
        if proto.ellipsis {
            let fixed = want - 1;
            if argc < fixed {
                let name = proto.name.map_or("", |n| env.prog.name(n));
                let plural = if fixed == 1 { "" } else { "s" };
                return Err(Exc::type_error(
                    pos,
                    format!("{name}() requires at least {fixed} arg{plural}, got {argc}"),
                ));
            }
            let rest_len = argc - fixed;
            let rest = self
                .heap
                .new_list(rest_len, rest_len)
                .map_err(Exc::memory(pos))?;
            for i in 0..rest_len {
                self.heap
                    .list_push(rest, self.stack[base + 1 + fixed + i])
                    .map_err(Exc::memory(pos))?;
            }
            self.stack.truncate(base + 1 + fixed);
            self.stack.push(Value::List(rest));
            argc = want;
        }
        if argc != want {
            let name = proto.name.map_or("", |n| env.prog.name(n));
            let plural = if want == 1 { "" } else { "s" };
            return Err(Exc::type_error(
                pos,
                format!("{name}() requires {want} arg{plural}, got {argc}"),
            ));
        }
        let local = self.heap.new_scope().map_err(Exc::memory(pos))?;
        self.scopes
            .try_reserve(2)
            .map_err(|_| Exc::limit(LimitReason::Memory, pos))?;
        let depth = self.scopes.len() as u32;
        self.scopes.push(closure);
        self.scopes.push(local);
        for (i, &p) in proto.params.iter().enumerate() {
            self.heap
                .scope_set(local, p, self.stack[base + 1 + i])
                .map_err(Exc::memory(pos))?;
        }
        let ret = self.pc as u32;
        self.push_handler(
            Handler::Call {
                ret,
                base: base as u32,
                scopes: depth,
            },
            pos,
        )?;
        self.pc = proto.start as usize;
        Ok(())
    }

    /// Ends the innermost call, which is on top of the handler stack, with `ret` as its result.
    fn finish_call(&mut self, ret: Value) {
        let Some(Handler::Call { ret: pc, base, .. }) = self.handlers.pop() else {
            unreachable!("a call ends with its handler on top");
        };
        self.pop_scope();
        self.pop_scope();
        self.meter.call_depth -= 1;
        self.stack.truncate(base as usize);
        self.stack.push(ret);
        self.pc = pc as usize;
    }

    /// Cuts the scope stack back to `depth`, releasing what is above.
    fn cut_scopes(&mut self, depth: u32) {
        while self.scopes.len() > depth as usize {
            self.pop_scope();
        }
    }

    /// Finds the handler for `exc`, undoing what each active construct would undo on the way out.
    fn unwind(&mut self, mut exc: Exc) -> Result<(), Exc> {
        if matches!(exc, Exc::Limit(..)) {
            return Err(exc);
        }
        loop {
            let Some(h) = self.handlers.pop() else {
                return Err(exc);
            };
            match h {
                Handler::Block => self.meter.nesting -= 1,
                Handler::Loop {
                    brk,
                    cont,
                    sp,
                    scopes,
                } => {
                    let target = match exc {
                        Exc::Break(_) => brk,
                        Exc::Continue(_) => cont,
                        _ => continue,
                    };
                    self.stack.truncate(sp as usize);
                    self.cut_scopes(scopes);
                    self.pc = target as usize;
                    return Ok(());
                }
                Handler::Try {
                    catch,
                    sp,
                    name,
                    pos,
                } => {
                    // A `return`, `break` or `continue` goes through a `try` and leaves its scope to the call or
                    // loop that ends it.
                    if matches!(exc, Exc::Return(..) | Exc::Break(_) | Exc::Continue(_)) {
                        continue;
                    }
                    self.pop_scope();
                    let fresh = self.heap.new_scope().map_err(Exc::memory(pos))?;
                    self.scopes
                        .try_reserve(1)
                        .map_err(|_| Exc::limit(LimitReason::Memory, pos))?;
                    self.scopes.push(fresh);
                    self.stack.truncate(sp as usize);
                    let msg = self.caught_error_value(&exc, pos)?;
                    let top = self.top_scope();
                    self.heap
                        .scope_set(top, name, msg)
                        .map_err(Exc::memory(pos))?;
                    self.pc = catch as usize;
                    return Ok(());
                }
                Handler::Call { ret, base, scopes } => {
                    if let Exc::Return(..) = exc {
                        self.cut_scopes(scopes);
                    } else {
                        self.pop_scope();
                        self.pop_scope();
                    }
                    self.meter.call_depth -= 1;
                    self.stack.truncate(base as usize);
                    match exc {
                        Exc::Return(v, _) => {
                            self.stack.push(v);
                            self.pc = ret as usize;
                            return Ok(());
                        }
                        // The compiler refuses a jump outside a loop of its function, so none reaches here.
                        Exc::Break(p) => {
                            exc = Exc::runtime_error(p, "break statement not within a loop");
                        }
                        Exc::Continue(p) => {
                            exc = Exc::runtime_error(p, "continue statement not within a loop");
                        }
                        _ => {}
                    }
                }
                Handler::Barrier => return Err(exc),
            }
        }
    }

    /// The text a `catch` binds: the message cut to 1,024 bytes and charged to the memory limit.
    fn caught_error_value(&mut self, exc: &Exc, pos: Position) -> Result<Value, Exc> {
        let text = exc.text_bytes();
        let bytes = text.as_ref();
        let (keep, cut) = text::truncate_len(bytes, MAX_CAUGHT_ERROR);
        let mut msg = bytes[..keep].to_vec();
        if cut {
            msg.extend_from_slice(b"...");
        }
        self.meter.charge(pos, WORD_COST + msg.len() as i64)?;
        Ok(Value::Str(
            self.heap.new_str(&msg).map_err(Exc::memory(pos))?,
        ))
    }
}

fn count_runes(b: &[u8]) -> usize {
    let (mut i, mut n) = (0, 0);
    while i < b.len() {
        i += text::decode_rune(&b[i..]).1.max(1);
        n += 1;
    }
    n
}

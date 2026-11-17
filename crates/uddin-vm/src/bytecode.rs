//! The compiled form of a script: stack-machine instructions with a parallel table of source positions.
//!
//! Operands (`a`, `b`) by instruction:
//!
//! | instruction | operands |
//! |---|---|
//! | `ConstT` | `a` constant index |
//! | `LoadVarT`, `LoadVarC`, `NameErrT`, `StoreVar` | `a` name id |
//! | `LoadBuiltinT` | `a` builtin index |
//! | `Jump`, `AndLeft`, `OrLeft`, `TernaryJump`, `IfCond`, `WhileCond` | `a` target |
//! | `PushLoop` | `a` break target, `b` continue target; the handler lives while the body runs |
//! | `ForNext` | `a` end target, `b` loop variable name id; the loop keeps four values on the stack: what it reads, the array it read first, the position and the end |
//! | `TryEnter` | `a` catch target, `b` error variable name id |
//! | `MakeAnon`, `DefFunc` | `a` prototype index |
//! | `ChargeList`, `MakeList`, `ChargeMap`, `MakeMap`, `Call`, `Spread` | `a` element or argument count |

use std::sync::Arc;

use uddin_syntax::Position;

use crate::value::Value;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    /// An expression tick: sets the current position, then counts one operation.
    Tick,
    /// A statement tick: counts one operation, then sets the current position.
    TickStmt,
    LoopTick,
    ConstT,
    LoadVarT,
    LoadBuiltinT,
    NameErrT,
    /// The read of `x += e`: no tick, and a name error is reported at the value's position.
    LoadVarC,
    StoreVar,
    Pop,
    Swap,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    In,
    Xor,
    Not,
    Neg,
    AndLeft,
    OrLeft,
    BoolCheckAnd,
    BoolCheckOr,
    TernaryJump,
    Jump,
    IfCond,
    WhileCond,
    EnterBlock,
    ExitBlock,
    PushLoop,
    PopLoop,
    ForInit,
    ForNext,
    MakeAnon,
    DefFunc,
    SubGet,
    SubSet,
    /// Reads the current value of the subscript an assignment operator updates.
    SubLoad,
    ChargeList,
    MakeList,
    ChargeMap,
    MapKey,
    MakeMap,
    CheckCallable,
    Call,
    Spread,
    CallDyn,
    Return,
    Break,
    Continue,
    PushScope,
    PopScope,
    TryEnter,
    TryExit,
    EndCall,
    CallMain,
    ImportErr,
    Halt,
}

#[derive(Clone, Copy, Debug)]
pub struct Insn {
    pub op: Op,
    pub a: u32,
    pub b: u32,
}

/// A function the script defines.
#[derive(Debug)]
pub struct Proto {
    pub name: Option<u32>,
    pub params: Box<[u32]>,
    pub ellipsis: bool,
    pub start: u32,
}

/// An immutable compiled script; any number of runs on any number of threads can share one.
#[derive(Debug)]
pub struct Program {
    pub code: Box<[Insn]>,
    pub pos: Box<[Position]>,
    pub consts: Box<[Value]>,
    /// String constants; the first 256 bytes are every byte value, so one-byte strings need no storage.
    pub bytes: Arc<[u8]>,
    pub names: Box<[Box<str>]>,
    pub protos: Box<[Proto]>,
    /// Names the script binds that are also builtins or constants: they start in the global scope.
    pub seed: Box<[(u32, Value)]>,
    /// The name ids of the variables the host supplies, in the order the host gives their values.
    pub host_names: Box<[u32]>,
    pub main_name: u32,
    /// The instruction that ends the program: a call made from native code returns to it, which ends the nested
    /// run that made the call.
    pub halt: u32,
}

impl Program {
    pub fn name(&self, id: u32) -> &str {
        &self.names[id as usize]
    }
}

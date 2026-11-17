//! From a syntax tree to a [`Program`].
//!
//! The compiler never recurses over the tree: it keeps a stack of work, because an operator chain in a 64 KiB
//! source is tens of thousands of nodes deep. Instructions come out in the order the reference interpreter
//! evaluates, tick for tick, so the operation limit stops a script at the same place.
//!
//! Names resolve in two tiers. A name the script never binds (no assignment, parameter, loop variable, catch
//! variable, function name, or host variable) can only be a builtin or a constant, and compiles to a direct
//! load. Every other name is looked up at run time through the stack of scopes, which is how the language
//! defines it.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::f64::consts;
use std::sync::Arc;

use uddin_syntax::Position;
use uddin_syntax::ast::{AssignOp, BinOp, Block, Expr, Literal, Program as Ast, Stmt, UnaryOp};

use crate::builtin::BuiltinDef;
use crate::bytecode::{Insn, Op, Program, Proto};
use crate::value::{CONST_BASE, Str, Value, type_name_pool};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    /// The script names builtins the allowlist permits but this build does not carry yet. Sorted.
    Unported(Vec<String>),
    /// The script needs more names, constants or instructions than the program format addresses.
    TooLarge,
    /// A `break` or `continue` outside a loop of its own function; `position` is the first one in the source.
    Flow {
        position: Position,
        keyword: &'static str,
    },
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompileError::Unported(names) => write!(
                f,
                "builtins not available in this build: {}",
                names.join(", ")
            ),
            CompileError::TooLarge => write!(f, "script is too large to compile"),
            CompileError::Flow { position, keyword } => write!(
                f,
                "compile error at {}:{}: {keyword} statement not within a loop",
                position.line, position.column
            ),
        }
    }
}

impl std::error::Error for CompileError {}

fn constant(name: &str) -> Option<f64> {
    Some(match name {
        "PI" => consts::PI,
        "E" => consts::E,
        "TAU" => consts::TAU,
        "PHI" => (1.0 + 5f64.sqrt()) / 2.0,
        "LN2" => consts::LN_2,
        "LN10" => consts::LN_10,
        "SQRT2" => consts::SQRT_2,
        "SQRT3" => 3f64.sqrt(),
        _ => return None,
    })
}

pub fn position_of(e: &Expr) -> Position {
    match e {
        Expr::Literal { position, .. }
        | Expr::Variable { position, .. }
        | Expr::Binary { position, .. }
        | Expr::Unary { position, .. }
        | Expr::Ternary { position, .. }
        | Expr::Call { position, .. }
        | Expr::List { position, .. }
        | Expr::Map { position, .. }
        | Expr::Function { position, .. }
        | Expr::Subscript { position, .. } => *position,
    }
}

enum Node<'a> {
    Block(&'a [Stmt]),
    Stmt(&'a Stmt),
    Expr(&'a Expr),
}

/// The names a script binds and every identifier it spells.
struct Names<'a> {
    bound: HashSet<&'a str>,
    seen: HashSet<&'a str>,
}

fn analyze(ast: &Ast) -> Names<'_> {
    let mut n = Names {
        bound: HashSet::new(),
        seen: HashSet::new(),
    };
    let mut stack = vec![Node::Block(&ast.statements)];
    while let Some(node) = stack.pop() {
        match node {
            Node::Block(b) => stack.extend(b.iter().map(Node::Stmt)),
            Node::Stmt(s) => match s {
                Stmt::Assign { target, value, .. } => {
                    if let Expr::Variable { name, .. } = target {
                        n.bound.insert(name);
                    }
                    stack.extend([Node::Expr(target), Node::Expr(value)]);
                }
                Stmt::If {
                    cond,
                    body,
                    else_body,
                    ..
                } => stack.extend([Node::Expr(cond), Node::Block(body), Node::Block(else_body)]),
                Stmt::While { cond, body, .. } => {
                    stack.extend([Node::Expr(cond), Node::Block(body)])
                }
                Stmt::For {
                    name,
                    iterable,
                    body,
                    ..
                } => {
                    n.bound.insert(name);
                    n.seen.insert(name);
                    stack.extend([Node::Expr(iterable), Node::Block(body)]);
                }
                Stmt::TryCatch {
                    try_block,
                    error_name,
                    catch_block,
                    ..
                } => {
                    n.bound.insert(error_name);
                    n.seen.insert(error_name);
                    stack.extend([Node::Block(try_block), Node::Block(catch_block)]);
                }
                Stmt::Return { result, .. } => stack.push(Node::Expr(result)),
                Stmt::Expr { expr, .. } => stack.push(Node::Expr(expr)),
                Stmt::FunDef {
                    name, params, body, ..
                } => {
                    n.bound.insert(name);
                    n.seen.insert(name);
                    for p in params {
                        n.bound.insert(p);
                        n.seen.insert(p);
                    }
                    stack.push(Node::Block(body));
                }
                Stmt::Import { .. } | Stmt::Break { .. } | Stmt::Continue { .. } => {}
            },
            Node::Expr(e) => match e {
                Expr::Literal { .. } => {}
                Expr::Variable { name, .. } => {
                    n.seen.insert(name);
                }
                Expr::Binary { left, right, .. } => {
                    stack.extend([Node::Expr(left), Node::Expr(right)])
                }
                Expr::Unary { operand, .. } => stack.push(Node::Expr(operand)),
                Expr::Ternary {
                    cond,
                    then,
                    otherwise,
                    ..
                } => stack.extend([Node::Expr(cond), Node::Expr(then), Node::Expr(otherwise)]),
                Expr::Call { function, args, .. } => {
                    stack.push(Node::Expr(function));
                    stack.extend(args.iter().map(Node::Expr));
                }
                Expr::List { values, .. } => stack.extend(values.iter().map(Node::Expr)),
                Expr::Map { items, .. } => {
                    for (k, v) in items {
                        stack.extend([Node::Expr(k), Node::Expr(v)]);
                    }
                }
                Expr::Function { params, body, .. } => {
                    for p in params {
                        n.bound.insert(p);
                        n.seen.insert(p);
                    }
                    stack.push(Node::Block(body));
                }
                Expr::Subscript {
                    container, index, ..
                } => stack.extend([Node::Expr(container), Node::Expr(index)]),
            },
        }
    }
    n
}

/// The first `break` or `continue` that is not inside a loop of its own function. A function body starts outside
/// every loop, so a jump never reaches a loop of the function that called it.
fn stray_jump(ast: &Ast) -> Option<(Position, &'static str)> {
    enum Item<'a> {
        Block(&'a [Stmt], bool),
        Expr(&'a Expr, bool),
    }
    let mut first: Option<(Position, &'static str)> = None;
    let mut note = |position: Position, keyword: &'static str| {
        if first.is_none_or(|(p, _)| (position.line, position.column) < (p.line, p.column)) {
            first = Some((position, keyword));
        }
    };
    let mut stack = vec![Item::Block(&ast.statements, false)];
    while let Some(item) = stack.pop() {
        match item {
            Item::Block(stmts, in_loop) => {
                for s in stmts {
                    match s {
                        Stmt::Assign { target, value, .. } => {
                            stack.extend([Item::Expr(target, in_loop), Item::Expr(value, in_loop)]);
                        }
                        Stmt::If {
                            cond,
                            body,
                            else_body,
                            ..
                        } => stack.extend([
                            Item::Expr(cond, in_loop),
                            Item::Block(body, in_loop),
                            Item::Block(else_body, in_loop),
                        ]),
                        Stmt::While { cond, body, .. } => {
                            stack.extend([Item::Expr(cond, in_loop), Item::Block(body, true)]);
                        }
                        Stmt::For { iterable, body, .. } => {
                            stack.extend([Item::Expr(iterable, in_loop), Item::Block(body, true)]);
                        }
                        Stmt::TryCatch {
                            try_block,
                            catch_block,
                            ..
                        } => stack.extend([
                            Item::Block(try_block, in_loop),
                            Item::Block(catch_block, in_loop),
                        ]),
                        Stmt::Return { result, .. } => stack.push(Item::Expr(result, in_loop)),
                        Stmt::Expr { expr, .. } => stack.push(Item::Expr(expr, in_loop)),
                        Stmt::FunDef { body, .. } => stack.push(Item::Block(body, false)),
                        Stmt::Import { .. } => {}
                        Stmt::Break { position } if !in_loop => note(*position, "break"),
                        Stmt::Continue { position } if !in_loop => note(*position, "continue"),
                        Stmt::Break { .. } | Stmt::Continue { .. } => {}
                    }
                }
            }
            Item::Expr(e, in_loop) => match e {
                Expr::Literal { .. } | Expr::Variable { .. } => {}
                Expr::Binary { left, right, .. } => {
                    stack.extend([Item::Expr(left, in_loop), Item::Expr(right, in_loop)]);
                }
                Expr::Unary { operand, .. } => stack.push(Item::Expr(operand, in_loop)),
                Expr::Ternary {
                    cond,
                    then,
                    otherwise,
                    ..
                } => stack.extend([
                    Item::Expr(cond, in_loop),
                    Item::Expr(then, in_loop),
                    Item::Expr(otherwise, in_loop),
                ]),
                Expr::Call { function, args, .. } => {
                    stack.push(Item::Expr(function, in_loop));
                    stack.extend(args.iter().map(|a| Item::Expr(a, in_loop)));
                }
                Expr::List { values, .. } => {
                    stack.extend(values.iter().map(|v| Item::Expr(v, in_loop)));
                }
                Expr::Map { items, .. } => {
                    for (k, v) in items {
                        stack.extend([Item::Expr(k, in_loop), Item::Expr(v, in_loop)]);
                    }
                }
                Expr::Function { body, .. } => stack.push(Item::Block(body, false)),
                Expr::Subscript {
                    container, index, ..
                } => stack.extend([Item::Expr(container, in_loop), Item::Expr(index, in_loop)]),
            },
        }
    }
    first
}

#[derive(Clone, Copy)]
struct Label(u32);

enum Task<'a> {
    Expr(&'a Expr),
    Stmt(&'a Stmt),
    Block(&'a [Stmt]),
    Emit(Op, u32, u32, Position),
    /// A jump or branch whose target is a label; patched once every label is bound.
    Jump(Op, Label, Position),
    PushLoop(Label, Label, Position),
    ForNext(Label, u32, Position),
    TryEnter(Label, u32, Position),
    Bind(Label),
}

struct Compiler<'a> {
    builtin_ids: HashMap<&'static str, u16>,
    bound: HashSet<String>,
    code: Vec<Insn>,
    pos: Vec<Position>,
    consts: Vec<Value>,
    int_consts: HashMap<i64, u32>,
    float_consts: HashMap<u64, u32>,
    str_consts: HashMap<Vec<u8>, u32>,
    special_consts: HashMap<u8, u32>,
    bytes: Vec<u8>,
    names: Vec<Box<str>>,
    name_ids: HashMap<String, u32>,
    protos: Vec<Proto>,
    queue: Vec<(u32, &'a Block)>,
    labels: Vec<u32>,
    stack: Vec<Task<'a>>,
}

impl<'a> Compiler<'a> {
    fn name(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.name_ids.get(s) {
            return id;
        }
        let id = self.names.len() as u32;
        self.names.push(s.into());
        self.name_ids.insert(s.to_owned(), id);
        id
    }

    fn int_const(&mut self, i: i64) -> u32 {
        if let Some(&c) = self.int_consts.get(&i) {
            return c;
        }
        self.consts.push(Value::Int(i));
        let c = self.consts.len() as u32 - 1;
        self.int_consts.insert(i, c);
        c
    }

    fn float_const(&mut self, f: f64) -> u32 {
        if let Some(&c) = self.float_consts.get(&f.to_bits()) {
            return c;
        }
        self.consts.push(Value::Float(f));
        let c = self.consts.len() as u32 - 1;
        self.float_consts.insert(f.to_bits(), c);
        c
    }

    fn value_const(&mut self, v: Value) -> u32 {
        self.consts.push(v);
        self.consts.len() as u32 - 1
    }

    fn str_const(&mut self, s: &str) -> u32 {
        let b = s.as_bytes();
        if let Some(&c) = self.str_consts.get(b) {
            return c;
        }
        let v = match b.len() {
            0 => Str::EMPTY,
            1 => Str::byte(b[0]),
            n => {
                let off = self.bytes.len() as u32;
                self.bytes.extend_from_slice(b);
                Str {
                    off: CONST_BASE + off,
                    len: n as u32,
                }
            }
        };
        let c = self.value_const(Value::Str(v));
        self.str_consts.insert(b.to_vec(), c);
        c
    }

    fn label(&mut self) -> Label {
        self.labels.push(u32::MAX);
        Label(self.labels.len() as u32 - 1)
    }

    fn emit(&mut self, op: Op, a: u32, b: u32, pos: Position) {
        self.code.push(Insn { op, a, b });
        self.pos.push(pos);
    }

    fn push_seq(&mut self, seq: Vec<Task<'a>>) {
        self.stack.extend(seq.into_iter().rev());
    }

    fn run(&mut self) {
        while let Some(task) = self.stack.pop() {
            match task {
                Task::Expr(e) => self.expr(e),
                Task::Stmt(s) => self.stmt(s),
                Task::Block(stmts) => self.stack.extend(stmts.iter().rev().map(Task::Stmt)),
                Task::Emit(op, a, b, pos) => self.emit(op, a, b, pos),
                Task::Jump(op, l, pos) => self.emit(op, l.0, 0, pos),
                Task::PushLoop(brk, cont, pos) => self.emit(Op::PushLoop, brk.0, cont.0, pos),
                Task::ForNext(end, name, pos) => self.emit(Op::ForNext, end.0, name, pos),
                Task::TryEnter(catch, name, pos) => self.emit(Op::TryEnter, catch.0, name, pos),
                Task::Bind(l) => self.labels[l.0 as usize] = self.code.len() as u32,
            }
        }
    }

    fn proto(
        &mut self,
        name: Option<&str>,
        params: &[String],
        ellipsis: bool,
        body: &'a Block,
    ) -> u32 {
        let name = name.map(|n| self.name(n));
        let params: Box<[u32]> = params.iter().map(|p| self.name(p)).collect();
        self.protos.push(Proto {
            name,
            params,
            ellipsis,
            start: 0,
        });
        let id = self.protos.len() as u32 - 1;
        self.queue.push((id, body));
        id
    }

    fn variable(&mut self, name: &str, pos: Position) -> Task<'a> {
        let id = self.name(name);
        if self.bound.contains(name) {
            return Task::Emit(Op::LoadVarT, id, 0, pos);
        }
        if let Some(&b) = self.builtin_ids.get(name) {
            return Task::Emit(Op::LoadBuiltinT, u32::from(b), 0, pos);
        }
        if let Some(f) = constant(name) {
            let c = self.float_const(f);
            return Task::Emit(Op::ConstT, c, 0, pos);
        }
        Task::Emit(Op::NameErrT, id, 0, pos)
    }

    fn expr(&mut self, e: &'a Expr) {
        match e {
            Expr::Literal { position, value } => {
                let c = match value {
                    Literal::Int(i) => self.int_const(*i),
                    Literal::Float(f) => self.float_const(*f),
                    Literal::Str(s) => self.str_const(s),
                    Literal::Bool(b) => self.special_const(1 + u8::from(*b), Value::Bool(*b)),
                    Literal::Null => self.special_const(0, Value::Null),
                };
                self.emit(Op::ConstT, c, 0, *position);
            }
            Expr::Variable { position, name } => {
                let t = self.variable(name, *position);
                self.stack.push(t);
            }
            Expr::Binary {
                position: pos,
                op,
                left,
                right,
            } => {
                let pos = *pos;
                let tick = Task::Emit(Op::Tick, 0, 0, pos);
                let (l, r) = (Task::Expr(left), Task::Expr(right));
                match op {
                    BinOp::And | BinOp::Or => {
                        let end = self.label();
                        let (jump, check) = if *op == BinOp::And {
                            (Op::AndLeft, Op::BoolCheckAnd)
                        } else {
                            (Op::OrLeft, Op::BoolCheckOr)
                        };
                        self.push_seq(vec![
                            tick,
                            l,
                            Task::Jump(jump, end, pos),
                            r,
                            Task::Emit(check, 0, 0, pos),
                            Task::Bind(end),
                        ]);
                    }
                    _ => {
                        let op = match op {
                            BinOp::Xor => Op::Xor,
                            BinOp::Eq => Op::Eq,
                            BinOp::Ne => Op::Ne,
                            BinOp::Lt => Op::Lt,
                            BinOp::Le => Op::Le,
                            BinOp::Gt => Op::Gt,
                            BinOp::Ge => Op::Ge,
                            BinOp::In => Op::In,
                            BinOp::Add => Op::Add,
                            BinOp::Sub => Op::Sub,
                            BinOp::Mul => Op::Mul,
                            BinOp::Div => Op::Div,
                            BinOp::Mod => Op::Mod,
                            BinOp::Pow => Op::Pow,
                            BinOp::And | BinOp::Or => unreachable!("handled above"),
                        };
                        self.push_seq(vec![tick, l, r, Task::Emit(op, 0, 0, pos)]);
                    }
                }
            }
            Expr::Unary {
                position: pos,
                op,
                operand,
            } => {
                let op = if *op == UnaryOp::Not {
                    Op::Not
                } else {
                    Op::Neg
                };
                self.push_seq(vec![
                    Task::Emit(Op::Tick, 0, 0, *pos),
                    Task::Expr(operand),
                    Task::Emit(op, 0, 0, *pos),
                ]);
            }
            Expr::Ternary {
                position: pos,
                cond,
                then,
                otherwise,
            } => {
                let (other, end) = (self.label(), self.label());
                self.push_seq(vec![
                    Task::Emit(Op::Tick, 0, 0, *pos),
                    Task::Expr(cond),
                    Task::Jump(Op::TernaryJump, other, *pos),
                    Task::Expr(then),
                    Task::Jump(Op::Jump, end, *pos),
                    Task::Bind(other),
                    Task::Expr(otherwise),
                    Task::Bind(end),
                ]);
            }
            Expr::Call {
                position: pos,
                function,
                args,
                ellipsis,
            } => {
                let fpos = position_of(function);
                let mut seq = vec![
                    Task::Emit(Op::Tick, 0, 0, *pos),
                    Task::Expr(function),
                    Task::Emit(Op::CheckCallable, 0, 0, fpos),
                ];
                seq.extend(args.iter().map(Task::Expr));
                if *ellipsis {
                    seq.push(Task::Emit(
                        Op::Spread,
                        (args.len() as u32).saturating_sub(1),
                        0,
                        *pos,
                    ));
                    seq.push(Task::Emit(Op::CallDyn, 0, 0, fpos));
                } else {
                    seq.push(Task::Emit(Op::Call, args.len() as u32, 0, fpos));
                }
                self.push_seq(seq);
            }
            Expr::List {
                position: pos,
                values,
            } => {
                let n = values.len() as u32;
                let mut seq = vec![
                    Task::Emit(Op::Tick, 0, 0, *pos),
                    Task::Emit(Op::ChargeList, n, 0, *pos),
                ];
                seq.extend(values.iter().map(Task::Expr));
                seq.push(Task::Emit(Op::MakeList, n, 0, *pos));
                self.push_seq(seq);
            }
            Expr::Map {
                position: pos,
                items,
            } => {
                let n = items.len() as u32;
                let mut seq = vec![
                    Task::Emit(Op::Tick, 0, 0, *pos),
                    Task::Emit(Op::ChargeMap, n, 0, *pos),
                ];
                for (k, v) in items {
                    seq.push(Task::Expr(k));
                    seq.push(Task::Emit(Op::MapKey, 0, 0, position_of(k)));
                    seq.push(Task::Expr(v));
                }
                seq.push(Task::Emit(Op::MakeMap, n, 0, *pos));
                self.push_seq(seq);
            }
            Expr::Function {
                position: pos,
                params,
                ellipsis,
                body,
            } => {
                let id = self.proto(None, params, *ellipsis, body);
                self.push_seq(vec![
                    Task::Emit(Op::Tick, 0, 0, *pos),
                    Task::Emit(Op::MakeAnon, id, 0, *pos),
                ]);
            }
            Expr::Subscript {
                position: pos,
                container,
                index,
            } => self.push_seq(vec![
                Task::Emit(Op::Tick, 0, 0, *pos),
                Task::Expr(container),
                Task::Expr(index),
                Task::Emit(Op::SubGet, 0, 0, position_of(index)),
            ]),
        }
    }

    fn special_const(&mut self, tag: u8, v: Value) -> u32 {
        if let Some(&c) = self.special_consts.get(&tag) {
            return c;
        }
        let c = self.value_const(v);
        self.special_consts.insert(tag, c);
        c
    }

    fn block(&mut self, stmts: &'a [Stmt], pos: Position) -> Vec<Task<'a>> {
        vec![
            Task::Emit(Op::EnterBlock, 0, 0, pos),
            Task::Block(stmts),
            Task::Emit(Op::ExitBlock, 0, 0, pos),
        ]
    }

    fn stmt(&mut self, s: &'a Stmt) {
        match s {
            Stmt::Assign {
                position: pos,
                target,
                value,
                op,
            } => {
                let compound = match op {
                    AssignOp::Assign => None,
                    AssignOp::Add => Some(Op::Add),
                    AssignOp::Sub => Some(Op::Sub),
                    AssignOp::Mul => Some(Op::Mul),
                    AssignOp::Div => Some(Op::Div),
                    AssignOp::Mod => Some(Op::Mod),
                };
                let vpos = position_of(value);
                let mut seq = vec![Task::Emit(Op::TickStmt, 0, 0, *pos)];
                match target {
                    Expr::Variable { name, .. } => {
                        let id = self.name(name);
                        seq.push(Task::Expr(value));
                        if let Some(op) = compound {
                            seq.extend([
                                Task::Emit(Op::LoadVarC, id, 0, vpos),
                                Task::Emit(Op::Swap, 0, 0, vpos),
                                Task::Emit(op, 0, 0, vpos),
                            ]);
                        }
                        seq.push(Task::Emit(Op::StoreVar, id, 0, *pos));
                    }
                    Expr::Subscript {
                        container, index, ..
                    } => {
                        seq.extend([Task::Expr(container), Task::Expr(index), Task::Expr(value)]);
                        if let Some(op) = compound {
                            seq.extend([
                                Task::Emit(Op::SubLoad, 0, 0, vpos),
                                Task::Emit(Op::Swap, 0, 0, vpos),
                                Task::Emit(op, 0, 0, vpos),
                            ]);
                        }
                        seq.push(Task::Emit(Op::SubSet, 0, 0, position_of(index)));
                    }
                    _ => unreachable!("the parser only builds variable and subscript targets"),
                }
                self.push_seq(seq);
            }
            Stmt::If {
                position: pos,
                cond,
                body,
                else_body,
            } => {
                let (other, end) = (self.label(), self.label());
                let mut seq = vec![
                    Task::Emit(Op::TickStmt, 0, 0, *pos),
                    Task::Expr(cond),
                    Task::Jump(Op::IfCond, other, position_of(cond)),
                ];
                seq.extend(self.block(body, *pos));
                if else_body.is_empty() {
                    seq.push(Task::Bind(other));
                } else {
                    seq.push(Task::Jump(Op::Jump, end, *pos));
                    seq.push(Task::Bind(other));
                    seq.extend(self.block(else_body, *pos));
                }
                seq.push(Task::Bind(end));
                self.push_seq(seq);
            }
            Stmt::While {
                position: pos,
                cond,
                body,
            } => {
                let (top, end) = (self.label(), self.label());
                let mut seq = vec![
                    Task::Emit(Op::TickStmt, 0, 0, *pos),
                    Task::Bind(top),
                    Task::Emit(Op::LoopTick, 0, 0, *pos),
                    Task::Expr(cond),
                    Task::Jump(Op::WhileCond, end, position_of(cond)),
                    Task::PushLoop(end, top, *pos),
                ];
                seq.extend(self.block(body, *pos));
                seq.push(Task::Emit(Op::PopLoop, 0, 0, *pos));
                seq.push(Task::Jump(Op::Jump, top, *pos));
                seq.push(Task::Bind(end));
                self.push_seq(seq);
            }
            Stmt::For {
                position: pos,
                name,
                iterable,
                body,
            } => {
                let (next, end) = (self.label(), self.label());
                let id = self.name(name);
                let mut seq = vec![
                    Task::Emit(Op::TickStmt, 0, 0, *pos),
                    Task::Expr(iterable),
                    Task::Emit(Op::ForInit, 0, 0, position_of(iterable)),
                    Task::Bind(next),
                    Task::ForNext(end, id, *pos),
                    Task::PushLoop(end, next, *pos),
                ];
                seq.extend(self.block(body, *pos));
                seq.push(Task::Emit(Op::PopLoop, 0, 0, *pos));
                seq.push(Task::Jump(Op::Jump, next, *pos));
                seq.push(Task::Bind(end));
                seq.extend((0..4).map(|_| Task::Emit(Op::Pop, 0, 0, *pos)));
                self.push_seq(seq);
            }
            Stmt::TryCatch {
                position: pos,
                try_block,
                error_name,
                catch_block,
            } => {
                let (catch, end) = (self.label(), self.label());
                let id = self.name(error_name);
                let mut seq = vec![
                    Task::Emit(Op::TickStmt, 0, 0, *pos),
                    Task::Emit(Op::PushScope, 0, 0, *pos),
                    Task::TryEnter(catch, id, *pos),
                ];
                seq.extend(self.block(try_block, *pos));
                seq.push(Task::Emit(Op::TryExit, 0, 0, *pos));
                seq.push(Task::Jump(Op::Jump, end, *pos));
                seq.push(Task::Bind(catch));
                seq.extend(self.block(catch_block, *pos));
                seq.push(Task::Emit(Op::PopScope, 0, 0, *pos));
                seq.push(Task::Bind(end));
                self.push_seq(seq);
            }
            Stmt::Return {
                position: pos,
                result,
            } => self.push_seq(vec![
                Task::Emit(Op::TickStmt, 0, 0, *pos),
                Task::Expr(result),
                Task::Emit(Op::Return, 0, 0, *pos),
            ]),
            Stmt::Expr {
                position: pos,
                expr,
            } => self.push_seq(vec![
                Task::Emit(Op::TickStmt, 0, 0, *pos),
                Task::Expr(expr),
                Task::Emit(Op::Pop, 0, 0, *pos),
            ]),
            Stmt::FunDef {
                position: pos,
                name,
                params,
                ellipsis,
                body,
            } => {
                let id = self.proto(Some(name), params, *ellipsis, body);
                let name_id = self.name(name);
                self.push_seq(vec![
                    Task::Emit(Op::TickStmt, 0, 0, *pos),
                    Task::Emit(Op::DefFunc, id, name_id, *pos),
                ]);
            }
            Stmt::Import { position, .. } => self.push_seq(vec![
                Task::Emit(Op::TickStmt, 0, 0, *position),
                Task::Emit(Op::ImportErr, 0, 0, *position),
            ]),
            Stmt::Break { position } => self.push_seq(vec![
                Task::Emit(Op::TickStmt, 0, 0, *position),
                Task::Emit(Op::Break, 0, 0, *position),
            ]),
            Stmt::Continue { position } => self.push_seq(vec![
                Task::Emit(Op::TickStmt, 0, 0, *position),
                Task::Emit(Op::Continue, 0, 0, *position),
            ]),
        }
    }
}

/// Compiles `ast`. `builtins` fixes the index of each builtin; `known` says which names are builtins of the
/// profile, so a script that spells one this build lacks is refused rather than run wrong; `host` lists the
/// variables the embedder supplies.
pub fn compile(
    ast: &Ast,
    builtins: &[BuiltinDef],
    known: &dyn Fn(&str) -> bool,
    host: &[&str],
) -> Result<Program, CompileError> {
    if let Some((position, keyword)) = stray_jump(ast) {
        return Err(CompileError::Flow { position, keyword });
    }
    let names = analyze(ast);
    let builtin_ids: HashMap<&'static str, u16> = builtins
        .iter()
        .enumerate()
        .map(|(i, b)| (b.name, i as u16))
        .collect();
    let mut unported = BTreeSet::new();
    for &n in &names.seen {
        if known(n) && !builtin_ids.contains_key(n) && constant(n).is_none() {
            unported.insert(n.to_owned());
        }
    }
    if !unported.is_empty() {
        return Err(CompileError::Unported(unported.into_iter().collect()));
    }
    let bound: HashSet<String> = names
        .bound
        .iter()
        .copied()
        .chain(host.iter().copied())
        .map(str::to_owned)
        .collect();
    let mut c = Compiler {
        builtin_ids,
        bound,
        code: Vec::new(),
        pos: Vec::new(),
        consts: Vec::new(),
        int_consts: HashMap::new(),
        float_consts: HashMap::new(),
        str_consts: HashMap::new(),
        special_consts: HashMap::new(),
        bytes: (0..=255u8).chain(type_name_pool()).collect(),
        names: Vec::new(),
        name_ids: HashMap::new(),
        protos: Vec::new(),
        queue: Vec::new(),
        labels: Vec::new(),
        stack: Vec::new(),
    };
    let host_names: Box<[u32]> = host.iter().map(|h| c.name(h)).collect();
    let main_name = c.name("main");
    c.stack
        .push(Task::Emit(Op::Halt, 0, 0, Position::default()));
    c.stack
        .push(Task::Emit(Op::CallMain, 0, 0, Position::default()));
    c.stack.push(Task::Block(&ast.statements));
    c.run();
    let halt = (c.code.len() - 1) as u32;
    let mut next = 0;
    while next < c.queue.len() {
        let (id, body) = c.queue[next];
        next += 1;
        c.protos[id as usize].start = c.code.len() as u32;
        let end = Position::default();
        c.stack.push(Task::Emit(Op::EndCall, 0, 0, end));
        c.stack.push(Task::Emit(Op::ExitBlock, 0, 0, end));
        c.stack.push(Task::Block(body));
        c.stack.push(Task::Emit(Op::EnterBlock, 0, 0, end));
        c.run();
    }
    let labels = std::mem::take(&mut c.labels);
    for insn in &mut c.code {
        match insn.op {
            Op::Jump
            | Op::AndLeft
            | Op::OrLeft
            | Op::TernaryJump
            | Op::IfCond
            | Op::WhileCond
            | Op::ForNext
            | Op::TryEnter => {
                insn.a = labels[insn.a as usize];
            }
            Op::PushLoop => {
                insn.a = labels[insn.a as usize];
                insn.b = labels[insn.b as usize];
            }
            _ => {}
        }
    }
    if c.code.len() >= u32::MAX as usize / 2 || c.bytes.len() >= (CONST_BASE as usize) {
        return Err(CompileError::TooLarge);
    }
    let mut seed = Vec::new();
    for name in &c.bound {
        let Some(&id) = c.name_ids.get(name.as_str()) else {
            continue;
        };
        if let Some(&b) = c.builtin_ids.get(name.as_str()) {
            seed.push((id, Value::Builtin(b)));
        } else if let Some(f) = constant(name) {
            seed.push((id, Value::Float(f)));
        }
    }
    seed.sort_by_key(|&(id, _)| id);
    Ok(Program {
        code: c.code.into_boxed_slice(),
        pos: c.pos.into_boxed_slice(),
        consts: c.consts.into_boxed_slice(),
        bytes: Arc::from(c.bytes),
        names: c.names.into_boxed_slice(),
        protos: c.protos.into_boxed_slice(),
        seed: seed.into_boxed_slice(),
        host_names,
        main_name,
        halt,
    })
}

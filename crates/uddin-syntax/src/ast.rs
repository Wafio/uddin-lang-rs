//! The syntax tree.
//!
//! A tree can be deeper than the parse depth limit: that limit counts open nesting levels, and operator chains
//! are released as soon as a parenthesis or a lower precedence level ends them. A chain of 500 operators inside
//! 64 parentheses is a left spine of 32,000 nodes in 64 KiB of source, and chains of seven precedence levels
//! add up the same way. Code that walks a tree must not assume 1,000, nor recurse once per level: the
//! analyses here keep their own stack, and `Expr` is freed without recursion (see its `Drop`).

use std::mem;

use crate::token::Position;

pub type Block = Vec<Stmt>;

#[derive(Debug, PartialEq)]
pub struct Program {
    pub statements: Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Assign,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

#[derive(Debug, PartialEq)]
pub enum Stmt {
    /// `target` is a `Variable` or a `Subscript`. `position` is where the assignment operator sits.
    Assign {
        position: Position,
        target: Expr,
        value: Expr,
        op: AssignOp,
    },
    If {
        position: Position,
        cond: Expr,
        body: Block,
        else_body: Block,
    },
    While {
        position: Position,
        cond: Expr,
        body: Block,
    },
    For {
        position: Position,
        name: String,
        iterable: Expr,
        body: Block,
    },
    TryCatch {
        position: Position,
        try_block: Block,
        error_name: String,
        catch_block: Block,
    },
    Return {
        position: Position,
        result: Expr,
    },
    Expr {
        position: Position,
        expr: Expr,
    },
    /// A named `fun`, with or without `memo` (the sandbox ignores it).
    FunDef {
        position: Position,
        name: String,
        params: Vec<String>,
        ellipsis: bool,
        body: Block,
    },
    Import {
        position: Position,
        filename: String,
    },
    Break {
        position: Position,
    },
    Continue {
        position: Position,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Or,
    Xor,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    In,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
    Neg,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Null,
}

#[derive(Debug, PartialEq)]
pub enum Expr {
    Literal {
        position: Position,
        value: Literal,
    },
    Variable {
        position: Position,
        name: String,
    },
    Binary {
        position: Position,
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Unary {
        position: Position,
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Ternary {
        position: Position,
        cond: Box<Expr>,
        then: Box<Expr>,
        otherwise: Box<Expr>,
    },
    /// `ellipsis` is set when the last argument is spread: `f(xs...)`.
    Call {
        position: Position,
        function: Box<Expr>,
        args: Vec<Expr>,
        ellipsis: bool,
    },
    List {
        position: Position,
        values: Vec<Expr>,
    },
    /// A bare identifier key is already a string `Literal`.
    Map {
        position: Position,
        items: Vec<(Expr, Expr)>,
    },
    Function {
        position: Position,
        params: Vec<String>,
        ellipsis: bool,
        body: Block,
    },
    /// `a[i]`, and `a.name` with a string `Literal` as the index.
    Subscript {
        position: Position,
        container: Box<Expr>,
        index: Box<Expr>,
    },
}

impl Expr {
    fn is_leaf(&self) -> bool {
        matches!(self, Expr::Literal { .. } | Expr::Variable { .. })
    }

    /// Moves the children that have children of their own into `pending`. A boxed child leaves a literal in
    /// its box, so a second call on the same node finds nothing to move.
    fn detach_children(&mut self, pending: &mut Vec<Expr>) {
        fn take(child: &mut Expr, pending: &mut Vec<Expr>) {
            if !child.is_leaf() {
                let leaf = Expr::Literal {
                    position: Position::default(),
                    value: Literal::Null,
                };
                pending.push(mem::replace(child, leaf));
            }
        }
        match self {
            // A function body is a block of statements, whose nesting the parse limit bounds.
            Expr::Literal { .. } | Expr::Variable { .. } | Expr::Function { .. } => {}
            Expr::Binary { left, right, .. } => {
                take(left, pending);
                take(right, pending);
            }
            Expr::Unary { operand, .. } => take(operand, pending),
            Expr::Ternary {
                cond,
                then,
                otherwise,
                ..
            } => {
                take(cond, pending);
                take(then, pending);
                take(otherwise, pending);
            }
            Expr::Call { function, args, .. } => {
                take(function, pending);
                pending.append(args);
            }
            Expr::List { values, .. } => pending.append(values),
            Expr::Map { items, .. } => pending.extend(items.drain(..).flat_map(|(k, v)| [k, v])),
            Expr::Subscript {
                container, index, ..
            } => {
                take(container, pending);
                take(index, pending);
            }
        }
    }
}

/// The derived drop would recurse once per level of the tree and overflow a small stack on a left spine of
/// thousands of nodes. This one moves the subtrees to a vector on the heap and frees them there, so the stack
/// stays as deep as the block nesting the parser limits.
impl Drop for Expr {
    fn drop(&mut self) {
        let mut pending = Vec::new();
        self.detach_children(&mut pending);
        while let Some(mut expr) = pending.pop() {
            expr.detach_children(&mut pending);
        }
    }
}

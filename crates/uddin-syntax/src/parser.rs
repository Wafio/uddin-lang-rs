//! Recursive descent parser with a Pratt loop for the binary operators.
//!
//! It accepts what the Go parser accepts and refuses what it refuses, at the same nesting depth. The depth
//! counter rises at the points where the Go parser counts a nesting level: every statement, block, expression,
//! `if`, prefix `not`, prefix `-`, `**`, and every operator and every call, subscript or `.name` of a chain.
//! An operator chain stays counted until the operators of a looser precedence level take over, like the nested
//! loops of the Go parser, which is why `chain` below is released when the precedence level changes.
//!
//! The Go parser has one function per precedence level, about twenty frames for each pair of parentheses.
//! This one has four or five. Its recursive functions are also kept small on purpose: a debug build gives every
//! temporary its own stack slot, and a 100-byte syntax node built in place or a `format!` in a function that
//! recurses costs that much at every nesting level. So the recursive functions only steer, and the nodes are
//! built by the `build_*` and `push_*` functions at the end, which are not on the stack while the parser
//! recurses. That is what lets nesting depth 1,000 fit a 2 MiB thread in a debug build.

// The `build_*` and `push_*` functions take boxes and unbox them on purpose: see above.
#![allow(clippy::boxed_local)]

use std::mem;

use crate::ast::{AssignOp, BinOp, Block, Expr, Literal, Program, Stmt, UnaryOp};
use crate::error::{Error, ErrorKind};
use crate::lexer::Lexer;
use crate::token::{Position, TokenKind};

/// Open nesting levels allowed. More is a parse error.
pub const NESTING_LEVEL_LIMIT: u32 = 1000;

const TERNARY_COLON: &str =
    "missing ':' in ternary expression - expected format: condition ? true_value : false_value";

/// A parse step failed; the error is in `Parser::error`. Keeping the payload off the `Result` halves the
/// temporaries that every `?` leaves in a debug build's frame.
struct Failed;

type ParseResult<T> = Result<T, Failed>;
type ExprBox = Box<Expr>;
type StmtBox = Box<Stmt>;

// Binding levels, loosest first. `not` and the ternary sit between `and` and `==`: both are operands of
// `and`, and the condition of a ternary is an `==` level expression.
const LEVEL_NOT: u8 = 4;
const LEVEL_TERNARY: u8 = 5;
const LEVEL_POWER: u8 = 10;

fn infix(token: TokenKind) -> Option<(u8, BinOp)> {
    Some(match token {
        TokenKind::Or => (1, BinOp::Or),
        TokenKind::Xor => (2, BinOp::Xor),
        TokenKind::And => (3, BinOp::And),
        TokenKind::EqualEqual => (6, BinOp::Eq),
        TokenKind::BangEqual => (6, BinOp::Ne),
        TokenKind::Lt => (7, BinOp::Lt),
        TokenKind::Le => (7, BinOp::Le),
        TokenKind::Gt => (7, BinOp::Gt),
        TokenKind::Ge => (7, BinOp::Ge),
        TokenKind::In => (7, BinOp::In),
        TokenKind::Plus => (8, BinOp::Add),
        TokenKind::Minus => (8, BinOp::Sub),
        TokenKind::Star => (9, BinOp::Mul),
        TokenKind::Slash => (9, BinOp::Div),
        TokenKind::Percent => (9, BinOp::Mod),
        TokenKind::StarStar => (LEVEL_POWER, BinOp::Pow),
        _ => return None,
    })
}

fn assign_op(token: TokenKind) -> Option<AssignOp> {
    Some(match token {
        TokenKind::Assign => AssignOp::Assign,
        TokenKind::PlusEqual => AssignOp::Add,
        TokenKind::MinusEqual => AssignOp::Sub,
        TokenKind::StarEqual => AssignOp::Mul,
        TokenKind::SlashEqual => AssignOp::Div,
        TokenKind::PercentEqual => AssignOp::Mod,
        _ => return None,
    })
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    position: Position,
    token: TokenKind,
    value: String,
    depth: u32,
    error: Option<Error>,
    /// Tests switch to the one-function-per-level port of the Go grammar to compare the two.
    #[cfg(test)]
    literal: bool,
}

/// Parses a whole program. The source is checked as UTF-8 once, here.
pub fn parse_source(src: &[u8]) -> Result<Program, Error> {
    let text = std::str::from_utf8(src).map_err(|e| invalid_utf8(src, e.valid_up_to()))?;
    let mut parser = Parser::new(text);
    let result = parser
        .start()
        .and_then(|()| parser.statements(TokenKind::Eof));
    Ok(Program {
        statements: parser.finish(result)?,
    })
}

fn invalid_utf8(src: &[u8], valid_up_to: usize) -> Error {
    let valid = String::from_utf8_lossy(&src[..valid_up_to]);
    let line = 1 + valid.matches('\n').count();
    let column = 1 + valid
        .rsplit('\n')
        .next()
        .map_or(0, |last| last.chars().count());
    Error {
        kind: ErrorKind::Tokenize,
        position: Position {
            line: u32::try_from(line).unwrap_or(u32::MAX),
            column: u32::try_from(column).unwrap_or(u32::MAX),
        },
        message: format!("invalid UTF-8 byte 0x{:02x}", src[valid_up_to]),
    }
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Self {
        Parser {
            lexer: Lexer::new(src),
            position: Position::START,
            token: TokenKind::Eof,
            value: String::new(),
            depth: 0,
            error: None,
            #[cfg(test)]
            literal: false,
        }
    }

    /// Reads the first token.
    fn start(&mut self) -> ParseResult<()> {
        self.next()
    }

    /// Turns the outcome of a parse into the value or the recorded error.
    fn finish<T>(&mut self, result: ParseResult<T>) -> Result<T, Error> {
        result.map_err(|Failed| {
            self.error.take().unwrap_or(Error {
                kind: ErrorKind::Parse,
                position: self.position,
                message: "parse failed".to_owned(),
            })
        })
    }

    fn record_error(&mut self, kind: ErrorKind, message: impl Into<String>) -> Failed {
        self.error = Some(Error {
            kind,
            position: self.position,
            message: message.into(),
        });
        Failed
    }

    fn record_parse_error(&mut self, message: impl Into<String>) -> Failed {
        self.record_error(ErrorKind::Parse, message)
    }

    fn enter_nesting(&mut self) -> ParseResult<()> {
        self.depth += 1;
        if self.depth > NESTING_LEVEL_LIMIT {
            return Err(self.record_parse_error(format!(
                "program nested too deeply (more than {NESTING_LEVEL_LIMIT} levels)"
            )));
        }
        Ok(())
    }

    fn next(&mut self) -> ParseResult<()> {
        let (position, token, value) = self.lexer.next_token();
        self.position = position;
        self.token = token;
        self.value = value;
        if token == TokenKind::Illegal {
            let message = self.take_value();
            return Err(self.record_error(ErrorKind::Tokenize, message));
        }
        Ok(())
    }

    fn expect(&mut self, token: TokenKind) -> ParseResult<()> {
        if self.token != token {
            return Err(
                self.record_parse_error(format!("expected {token}, but got {}", self.token))
            );
        }
        self.next()
    }

    /// The Go parser panics on a literal it cannot convert, and the panic becomes an error at 1:1.
    fn number_error(&mut self, what: &str, text: &str) -> Failed {
        self.error = Some(Error {
            kind: ErrorKind::Parse,
            position: Position::START,
            message: format!("parse failed: tokenizer gave {what}: {text}"),
        });
        Failed
    }

    fn expect_all(&mut self, toks: &[TokenKind]) -> ParseResult<()> {
        toks.iter().try_for_each(|&token| self.expect(token))
    }

    fn take_value(&mut self) -> String {
        mem::take(&mut self.value)
    }

    // ---- statements ----

    fn statements(&mut self, end: TokenKind) -> ParseResult<Block> {
        let mut out = Vec::new();
        while self.token != end && self.token != TokenKind::Eof {
            let stmt = self.statement()?;
            push_stmt(&mut out, stmt);
        }
        Ok(out)
    }

    fn statement(&mut self) -> ParseResult<StmtBox> {
        self.enter_nesting()?;
        let stmt = self.statement_kind()?;
        self.depth -= 1;
        Ok(stmt)
    }

    fn statement_kind(&mut self) -> ParseResult<StmtBox> {
        match self.token {
            TokenKind::Memo => self.memo_fun(),
            TokenKind::If | TokenKind::Elif => self.if_stmt(),
            TokenKind::While => self.while_stmt(),
            TokenKind::For => self.for_stmt(),
            TokenKind::Return => self.return_stmt(),
            TokenKind::Break | TokenKind::Continue => self.jump_stmt(),
            TokenKind::Import => self.import_stmt(),
            TokenKind::Fun => self.fun_stmt(),
            TokenKind::Try => self.try_stmt(),
            _ => self.simple_stmt(),
        }
    }

    fn simple_stmt(&mut self) -> ParseResult<StmtBox> {
        let position = self.position;
        let target = self.expression()?;
        let Some(op) = assign_op(self.token) else {
            return Ok(build_expr_stmt(position, target));
        };
        if !matches!(*target, Expr::Variable { .. } | Expr::Subscript { .. }) {
            return Err(self.record_parse_error(
                "invalid assignment target: only variables and array/object elements can be assigned to",
            ));
        }
        let op_position = self.position;
        self.next()?;
        let value = self.expression()?;
        Ok(build_assign(op_position, target, value, op))
    }

    fn jump_stmt(&mut self) -> ParseResult<StmtBox> {
        let (position, token) = (self.position, self.token);
        self.expect(token)?;
        Ok(build_jump(position, token))
    }

    /// `{ ... }` or `: ... end`. A colon block also ends, without consuming it, at `else`, `elif`, `catch` or
    /// the end of the input, so a program may leave its last block open.
    fn block(&mut self) -> ParseResult<Block> {
        self.enter_nesting()?;
        let body = match self.token {
            TokenKind::OpenBrace => self.brace_block()?,
            TokenKind::Colon => self.colon_block()?,
            _ => return Err(self.block_error()),
        };
        self.depth -= 1;
        Ok(body)
    }

    fn brace_block(&mut self) -> ParseResult<Block> {
        self.next()?;
        let body = self.statements(TokenKind::CloseBrace)?;
        self.expect(TokenKind::CloseBrace)?;
        Ok(body)
    }

    fn colon_block(&mut self) -> ParseResult<Block> {
        self.next()?;
        let mut body = Vec::new();
        while !matches!(
            self.token,
            TokenKind::End | TokenKind::Else | TokenKind::Elif | TokenKind::Catch | TokenKind::Eof
        ) {
            let stmt = self.statement()?;
            push_stmt(&mut body, stmt);
        }
        if self.token == TokenKind::End {
            self.next()?;
        }
        Ok(body)
    }

    fn block_error(&mut self) -> Failed {
        self.record_parse_error(format!(
            "expected ':' to start block (after 'then', 'while(...)', 'for(...)', 'fun(...)', or 'try'), got {}",
            self.token
        ))
    }

    fn if_stmt(&mut self) -> ParseResult<StmtBox> {
        self.enter_nesting()?;
        let position = self.position;
        let head = if self.token == TokenKind::Elif {
            TokenKind::Elif
        } else {
            TokenKind::If
        };
        self.expect_all(&[head, TokenKind::OpenParen])?;
        let cond = self.expression()?;
        self.expect_all(&[TokenKind::CloseParen, TokenKind::Then])?;
        let body = self.block()?;
        let else_body = self.else_branch()?;
        self.depth -= 1;
        Ok(build_if(position, cond, body, else_body))
    }

    fn else_branch(&mut self) -> ParseResult<Block> {
        if self.token == TokenKind::Else {
            self.next()?;
            return match self.token {
                TokenKind::OpenBrace | TokenKind::Colon => self.block(),
                TokenKind::If | TokenKind::Elif => self.nested_if(),
                _ => Err(self.else_error()),
            };
        }
        if self.token == TokenKind::Elif {
            return self.nested_if();
        }
        Ok(Vec::new())
    }

    fn nested_if(&mut self) -> ParseResult<Block> {
        let stmt = self.if_stmt()?;
        Ok(build_block_of(stmt))
    }

    fn else_error(&mut self) -> Failed {
        self.record_parse_error(format!(
            "expected ':' or 'if' after 'else' keyword, got {}",
            self.token
        ))
    }

    fn while_stmt(&mut self) -> ParseResult<StmtBox> {
        let position = self.position;
        self.expect_all(&[TokenKind::While, TokenKind::OpenParen])?;
        let cond = self.expression()?;
        self.expect(TokenKind::CloseParen)?;
        let body = self.block()?;
        Ok(build_while(position, cond, body))
    }

    fn for_stmt(&mut self) -> ParseResult<StmtBox> {
        let position = self.position;
        self.expect_all(&[TokenKind::For, TokenKind::OpenParen])?;
        let name = self.take_value();
        self.expect_all(&[TokenKind::Name, TokenKind::In])?;
        let iterable = self.expression()?;
        self.expect(TokenKind::CloseParen)?;
        let body = self.block()?;
        Ok(build_for(position, name, iterable, body))
    }

    fn try_stmt(&mut self) -> ParseResult<StmtBox> {
        let position = self.position;
        self.expect(TokenKind::Try)?;
        let try_block = self.try_body()?;
        self.catch_clause(position, try_block)
    }

    /// The try body parses its statements directly, not through `block`, so it adds no block level.
    fn try_body(&mut self) -> ParseResult<Block> {
        match self.token {
            TokenKind::OpenBrace => self.brace_block(),
            TokenKind::Colon => self.try_colon_body(),
            _ => Err(self.try_error()),
        }
    }

    fn try_colon_body(&mut self) -> ParseResult<Block> {
        self.next()?;
        self.statements(TokenKind::Catch)
    }

    fn try_error(&mut self) -> Failed {
        self.record_parse_error(format!(
            "expected ':' after 'try' keyword to start block, got {}",
            self.token
        ))
    }

    fn catch_clause(&mut self, position: Position, try_block: Block) -> ParseResult<StmtBox> {
        self.expect_all(&[TokenKind::Catch, TokenKind::OpenParen])?;
        let error_name = self.take_value();
        self.expect_all(&[TokenKind::Name, TokenKind::CloseParen])?;
        let catch_block = self.block()?;
        Ok(build_try(position, try_block, error_name, catch_block))
    }

    fn return_stmt(&mut self) -> ParseResult<StmtBox> {
        let position = self.position;
        self.expect(TokenKind::Return)?;
        let result = self.expression()?;
        Ok(build_return(position, result))
    }

    fn fun_stmt(&mut self) -> ParseResult<StmtBox> {
        let position = self.position;
        self.expect(TokenKind::Fun)?;
        if self.token == TokenKind::Name {
            return self.fun_def(position);
        }
        let (params, ellipsis) = self.params()?;
        let body = self.block()?;
        Ok(build_anonymous_fun_stmt(position, params, ellipsis, body))
    }

    /// `memo fun name(...)`: the keyword changes nothing here.
    fn memo_fun(&mut self) -> ParseResult<StmtBox> {
        let position = self.position;
        self.expect_all(&[TokenKind::Memo, TokenKind::Fun])?;
        if self.token != TokenKind::Name {
            return Err(self.record_parse_error("expected function name after 'memo fun'"));
        }
        self.fun_def(position)
    }

    /// The current token is the function name.
    fn fun_def(&mut self, position: Position) -> ParseResult<StmtBox> {
        let name = self.take_value();
        self.next()?;
        let (params, ellipsis) = self.params()?;
        let body = self.block()?;
        Ok(build_fun_def(position, name, params, ellipsis, body))
    }

    fn import_stmt(&mut self) -> ParseResult<StmtBox> {
        let position = self.position;
        self.expect(TokenKind::Import)?;
        if self.token != TokenKind::Str {
            return Err(self.record_parse_error(format!(
                "import statement requires a string filename, got {} - example: import \"filename.din\"",
                self.token
            )));
        }
        let filename = self.take_value();
        self.next()?;
        Ok(Box::new(Stmt::Import { position, filename }))
    }

    fn params(&mut self) -> ParseResult<(Vec<String>, bool)> {
        self.expect(TokenKind::OpenParen)?;
        let mut params = Vec::new();
        let mut got_comma = true;
        let mut got_ellipsis = false;
        while self.token != TokenKind::CloseParen && self.token != TokenKind::Eof && !got_ellipsis {
            if !got_comma {
                return Err(
                    self.record_parse_error("missing comma ',' between function parameters")
                );
            }
            let param = self.take_value();
            self.expect(TokenKind::Name)?;
            params.push(param);
            if self.token == TokenKind::Ellipsis {
                got_ellipsis = true;
                self.next()?;
            }
            got_comma = self.token == TokenKind::Comma;
            if got_comma {
                self.next()?;
            }
        }
        if self.token != TokenKind::CloseParen && got_ellipsis {
            return Err(self.record_parse_error(
                "variadic parameter '...' must be the last parameter in function definition",
            ));
        }
        self.expect(TokenKind::CloseParen)?;
        Ok((params, got_ellipsis))
    }

    // ---- expressions ----

    fn expression(&mut self) -> ParseResult<ExprBox> {
        self.enter_nesting()?;
        let expr = self.expression_body()?;
        self.depth -= 1;
        Ok(expr)
    }

    #[cfg(not(test))]
    #[inline(always)]
    fn expression_body(&mut self) -> ParseResult<ExprBox> {
        self.expression_from_level(0)
    }

    #[cfg(test)]
    fn expression_body(&mut self) -> ParseResult<ExprBox> {
        if self.literal {
            self.binary_level(0)
        } else {
            self.expression_from_level(0)
        }
    }

    /// Parses one operand and the operators of level `min` and tighter that follow it. Most operands are
    /// followed by none, so the loop lives in `operators` and this function stays small.
    fn expression_from_level(&mut self, min: u8) -> ParseResult<ExprBox> {
        let left = self.unary(min)?;
        if !self.operator_follows(min) {
            return Ok(left);
        }
        self.operators(left, min)
    }

    fn operator_follows(&self, min: u8) -> bool {
        match infix(self.token) {
            Some((level, _)) => level >= min,
            None => self.token == TokenKind::Question && min <= LEVEL_TERNARY,
        }
    }

    fn operators(&mut self, mut left: ExprBox, min: u8) -> ParseResult<ExprBox> {
        // `chain` operators of `level` are open and counted in `depth`.
        let mut level = 0u8;
        let mut chain = 0u32;
        loop {
            if self.token == TokenKind::Question && min <= LEVEL_TERNARY {
                self.depth -= chain;
                chain = 0;
                level = 0;
                left = self.ternary(left)?;
                continue;
            }
            let Some((operator_level, op)) = infix(self.token) else {
                break;
            };
            if operator_level < min {
                break;
            }
            if operator_level != level {
                self.depth -= chain;
                chain = 0;
                level = operator_level;
            }
            let position = self.position;
            let right = self.right_operand(operator_level)?;
            chain += 1;
            left = build_binary(position, op, left, right);
        }
        self.depth -= chain;
        Ok(left)
    }

    /// Counts the operator at the current token, moves past it and parses its right operand. `**` is right
    /// associative, and the Go parser moves past it before it counts it.
    fn right_operand(&mut self, level: u8) -> ParseResult<ExprBox> {
        if level == LEVEL_POWER {
            self.next()?;
            self.enter_nesting()?;
            self.expression_from_level(LEVEL_POWER)
        } else {
            self.enter_nesting()?;
            self.next()?;
            self.expression_from_level(level + 1)
        }
    }

    fn not_expr(&mut self) -> ParseResult<ExprBox> {
        let position = self.position;
        self.next()?;
        self.enter_nesting()?;
        let operand = self.expression_from_level(LEVEL_NOT)?;
        self.depth -= 1;
        Ok(build_unary(position, UnaryOp::Not, operand))
    }

    fn ternary(&mut self, cond: ExprBox) -> ParseResult<ExprBox> {
        let position = self.position;
        self.next()?;
        let then = self.expression()?;
        if self.token != TokenKind::Colon {
            return Err(self.record_parse_error(TERNARY_COLON));
        }
        self.next()?;
        let otherwise = self.expression()?;
        Ok(build_ternary(position, cond, then, otherwise))
    }

    /// A primary with its calls, subscripts and `.name` selections, or the same after prefix `-`, or `not`
    /// where an operand of `and` may start with it.
    fn unary(&mut self, min: u8) -> ParseResult<ExprBox> {
        if self.token == TokenKind::Not && min <= LEVEL_NOT {
            return self.not_expr();
        }
        if self.token == TokenKind::Minus {
            return self.negation();
        }
        let expr = self.primary()?;
        if matches!(
            self.token,
            TokenKind::OpenParen | TokenKind::OpenBracket | TokenKind::Dot
        ) {
            return self.postfix(expr);
        }
        Ok(expr)
    }

    /// Any number of prefix `-`, a loop here where the Go parser recurses.
    fn negation(&mut self) -> ParseResult<ExprBox> {
        let mut minus: Vec<Position> = Vec::new();
        while self.token == TokenKind::Minus {
            minus.push(self.position);
            self.next()?;
            self.enter_nesting()?;
        }
        let mut expr = self.primary()?;
        if matches!(
            self.token,
            TokenKind::OpenParen | TokenKind::OpenBracket | TokenKind::Dot
        ) {
            expr = self.postfix(expr)?;
        }
        self.depth -= u32::try_from(minus.len()).unwrap_or(u32::MAX);
        while let Some(position) = minus.pop() {
            expr = build_unary(position, UnaryOp::Neg, expr);
        }
        Ok(expr)
    }

    fn postfix(&mut self, mut expr: ExprBox) -> ParseResult<ExprBox> {
        let mut chain = 0u32;
        while matches!(
            self.token,
            TokenKind::OpenParen | TokenKind::OpenBracket | TokenKind::Dot
        ) {
            self.enter_nesting()?;
            chain += 1;
            expr = match self.token {
                TokenKind::OpenParen => self.call_args(expr)?,
                TokenKind::OpenBracket => self.subscript(expr)?,
                _ => self.selector(expr)?,
            };
        }
        self.depth -= chain;
        Ok(expr)
    }

    fn subscript(&mut self, container: ExprBox) -> ParseResult<ExprBox> {
        let position = self.position;
        self.next()?;
        let index = self.expression()?;
        self.expect(TokenKind::CloseBracket)?;
        Ok(build_subscript(position, container, index))
    }

    /// `.name` is a subscript with the name as a string.
    fn selector(&mut self, container: ExprBox) -> ParseResult<ExprBox> {
        let position = self.position;
        self.next()?;
        let name_position = self.position;
        let name = self.take_value();
        self.expect(TokenKind::Name)?;
        Ok(build_subscript(
            position,
            container,
            build_literal(name_position, Literal::Str(name)),
        ))
    }

    fn call_args(&mut self, function: ExprBox) -> ParseResult<ExprBox> {
        let position = self.position;
        self.next()?;
        let mut args = Vec::new();
        let mut got_comma = true;
        let mut got_ellipsis = false;
        while self.token != TokenKind::CloseParen && self.token != TokenKind::Eof && !got_ellipsis {
            if !got_comma {
                return Err(self.record_parse_error("missing comma ',' between function arguments"));
            }
            let arg = self.expression()?;
            push_expr(&mut args, arg);
            if self.token == TokenKind::Ellipsis {
                got_ellipsis = true;
                self.next()?;
            }
            got_comma = self.token == TokenKind::Comma;
            if got_comma {
                self.next()?;
            }
        }
        if self.token != TokenKind::CloseParen && got_ellipsis {
            return Err(self.record_parse_error(
                "variadic argument '...' must be the last argument in function call",
            ));
        }
        self.expect(TokenKind::CloseParen)?;
        Ok(build_call(position, function, args, got_ellipsis))
    }

    fn primary(&mut self) -> ParseResult<ExprBox> {
        match self.token {
            TokenKind::OpenParen => self.paren(),
            TokenKind::OpenBracket => self.list(),
            TokenKind::OpenBrace => self.map(),
            TokenKind::Fun => self.function_expr(),
            _ => self.atom(),
        }
    }

    fn paren(&mut self) -> ParseResult<ExprBox> {
        self.next()?;
        let expr = self.expression()?;
        self.expect(TokenKind::CloseParen)?;
        Ok(expr)
    }

    /// A name or a literal.
    fn atom(&mut self) -> ParseResult<ExprBox> {
        let position = self.position;
        let value = match self.token {
            TokenKind::Name => {
                let name = self.take_value();
                self.next()?;
                return Ok(build_variable(position, name));
            }
            TokenKind::Int => {
                let text = self.take_value();
                self.next()?;
                match text.parse::<i64>() {
                    Ok(n) => Literal::Int(n),
                    Err(_) => return Err(self.number_error("INT token that isn't an int", &text)),
                }
            }
            TokenKind::Float => {
                let text = self.take_value();
                self.next()?;
                match text.parse::<f64>() {
                    // Past the largest float the Go conversion fails and Rust gives infinity.
                    Ok(n) if n.is_finite() => Literal::Float(n),
                    _ => return Err(self.number_error("FLOAT token that isn't a float", &text)),
                }
            }
            TokenKind::Str => {
                let text = self.take_value();
                self.next()?;
                Literal::Str(text)
            }
            TokenKind::True => {
                self.next()?;
                Literal::Bool(true)
            }
            TokenKind::False => {
                self.next()?;
                Literal::Bool(false)
            }
            TokenKind::Null => {
                self.next()?;
                Literal::Null
            }
            _ => {
                return Err(self.record_parse_error(format!(
                    "unexpected token {} - expected a value (number, string, identifier, '(', '[', '{{', 'fun', etc.)",
                    self.token
                )));
            }
        };
        Ok(build_literal(position, value))
    }

    fn list(&mut self) -> ParseResult<ExprBox> {
        let position = self.position;
        self.expect(TokenKind::OpenBracket)?;
        let mut values = Vec::new();
        let mut got_comma = true;
        while self.token != TokenKind::CloseBracket && self.token != TokenKind::Eof {
            if !got_comma {
                return Err(self.record_parse_error("missing comma ',' between array elements"));
            }
            let value = self.expression()?;
            push_expr(&mut values, value);
            got_comma = self.token == TokenKind::Comma;
            if got_comma {
                self.next()?;
            }
        }
        self.expect(TokenKind::CloseBracket)?;
        Ok(build_list(position, values))
    }

    fn map(&mut self) -> ParseResult<ExprBox> {
        let position = self.position;
        self.expect(TokenKind::OpenBrace)?;
        let mut items = Vec::new();
        let mut got_comma = true;
        while self.token != TokenKind::CloseBrace && self.token != TokenKind::Eof {
            if !got_comma {
                return Err(self.record_parse_error("missing comma ',' between object properties"));
            }
            let key = self.map_key()?;
            self.expect(TokenKind::Colon)?;
            let value = self.expression()?;
            push_item(&mut items, key, value);
            got_comma = self.token == TokenKind::Comma;
            if got_comma {
                self.next()?;
            }
        }
        self.expect(TokenKind::CloseBrace)?;
        Ok(build_map(position, items))
    }

    /// A bare name is a string key; anything else is an expression.
    fn map_key(&mut self) -> ParseResult<ExprBox> {
        if self.token != TokenKind::Name {
            return self.expression();
        }
        let position = self.position;
        let name = self.take_value();
        self.next()?;
        Ok(build_literal(position, Literal::Str(name)))
    }

    fn function_expr(&mut self) -> ParseResult<ExprBox> {
        let position = self.position;
        self.next()?;
        let (params, ellipsis) = self.params()?;
        let body = self.block()?;
        Ok(build_function(position, params, ellipsis, body))
    }
}

// ---- node construction, kept out of the recursive functions (see the module comment) ----

fn build_variable(position: Position, name: String) -> ExprBox {
    Box::new(Expr::Variable { position, name })
}

fn build_literal(position: Position, value: Literal) -> ExprBox {
    Box::new(Expr::Literal { position, value })
}

fn build_binary(position: Position, op: BinOp, left: ExprBox, right: ExprBox) -> ExprBox {
    Box::new(Expr::Binary {
        position,
        op,
        left,
        right,
    })
}

fn build_unary(position: Position, op: UnaryOp, operand: ExprBox) -> ExprBox {
    Box::new(Expr::Unary {
        position,
        op,
        operand,
    })
}

fn build_ternary(position: Position, cond: ExprBox, then: ExprBox, otherwise: ExprBox) -> ExprBox {
    Box::new(Expr::Ternary {
        position,
        cond,
        then,
        otherwise,
    })
}

fn build_subscript(position: Position, container: ExprBox, index: ExprBox) -> ExprBox {
    Box::new(Expr::Subscript {
        position,
        container,
        index,
    })
}

fn build_call(position: Position, function: ExprBox, args: Vec<Expr>, ellipsis: bool) -> ExprBox {
    Box::new(Expr::Call {
        position,
        function,
        args,
        ellipsis,
    })
}

fn build_list(position: Position, values: Vec<Expr>) -> ExprBox {
    Box::new(Expr::List { position, values })
}

fn build_map(position: Position, items: Vec<(Expr, Expr)>) -> ExprBox {
    Box::new(Expr::Map { position, items })
}

fn build_function(position: Position, params: Vec<String>, ellipsis: bool, body: Block) -> ExprBox {
    Box::new(Expr::Function {
        position,
        params,
        ellipsis,
        body,
    })
}

fn push_expr(into: &mut Vec<Expr>, expr: ExprBox) {
    into.push(*expr);
}

fn push_item(into: &mut Vec<(Expr, Expr)>, key: ExprBox, value: ExprBox) {
    into.push((*key, *value));
}

fn push_stmt(into: &mut Block, stmt: StmtBox) {
    into.push(*stmt);
}

fn build_block_of(stmt: StmtBox) -> Block {
    vec![*stmt]
}

fn build_expr_stmt(position: Position, expr: ExprBox) -> StmtBox {
    Box::new(Stmt::Expr {
        position,
        expr: *expr,
    })
}

fn build_assign(position: Position, target: ExprBox, value: ExprBox, op: AssignOp) -> StmtBox {
    Box::new(Stmt::Assign {
        position,
        target: *target,
        value: *value,
        op,
    })
}

fn build_jump(position: Position, token: TokenKind) -> StmtBox {
    Box::new(if token == TokenKind::Break {
        Stmt::Break { position }
    } else {
        Stmt::Continue { position }
    })
}

fn build_if(position: Position, cond: ExprBox, body: Block, else_body: Block) -> StmtBox {
    Box::new(Stmt::If {
        position,
        cond: *cond,
        body,
        else_body,
    })
}

fn build_while(position: Position, cond: ExprBox, body: Block) -> StmtBox {
    Box::new(Stmt::While {
        position,
        cond: *cond,
        body,
    })
}

fn build_for(position: Position, name: String, iterable: ExprBox, body: Block) -> StmtBox {
    Box::new(Stmt::For {
        position,
        name,
        iterable: *iterable,
        body,
    })
}

fn build_try(
    position: Position,
    try_block: Block,
    error_name: String,
    catch_block: Block,
) -> StmtBox {
    Box::new(Stmt::TryCatch {
        position,
        try_block,
        error_name,
        catch_block,
    })
}

fn build_return(position: Position, result: ExprBox) -> StmtBox {
    Box::new(Stmt::Return {
        position,
        result: *result,
    })
}

fn build_fun_def(
    position: Position,
    name: String,
    params: Vec<String>,
    ellipsis: bool,
    body: Block,
) -> StmtBox {
    Box::new(Stmt::FunDef {
        position,
        name,
        params,
        ellipsis,
        body,
    })
}

fn build_anonymous_fun_stmt(
    position: Position,
    params: Vec<String>,
    ellipsis: bool,
    body: Block,
) -> StmtBox {
    Box::new(Stmt::Expr {
        position,
        expr: Expr::Function {
            position,
            params,
            ellipsis,
            body,
        },
    })
}

#[cfg(test)]
mod reference;
#[cfg(test)]
mod tests;

//! The expression grammar written the way the Go parser writes it, one function per precedence level and
//! recursion for every prefix operator. It is slow and uses far more stack than the real parser, which is
//! the point: the tests run both on the same sources and compare.

use super::*;

const LEVELS: [&[TokenKind]; 7] = [
    &[TokenKind::Or],
    &[TokenKind::Xor],
    &[TokenKind::And],
    &[TokenKind::EqualEqual, TokenKind::BangEqual],
    &[
        TokenKind::Lt,
        TokenKind::Le,
        TokenKind::Gt,
        TokenKind::Ge,
        TokenKind::In,
    ],
    &[TokenKind::Plus, TokenKind::Minus],
    &[TokenKind::Star, TokenKind::Slash, TokenKind::Percent],
];

impl Parser<'_> {
    fn operand_of_level(&mut self, level: usize) -> ParseResult<ExprBox> {
        match level {
            2 => self.not_level(),
            6 => self.power_level(),
            _ => self.binary_level(level + 1),
        }
    }

    /// The binary operators of `level`, each parsed with the operands of the next tighter level.
    pub(super) fn binary_level(&mut self, level: usize) -> ParseResult<ExprBox> {
        let mut expr = self.operand_of_level(level)?;
        let mut chain = 0;
        while LEVELS[level].contains(&self.token) {
            self.enter_nesting()?;
            chain += 1;
            let (_, op) = infix(self.token).expect("level tokens are operators");
            let position = self.position;
            self.next()?;
            let right = self.operand_of_level(level)?;
            expr = Box::new(Expr::Binary {
                position,
                op,
                left: expr,
                right,
            });
        }
        self.depth -= chain;
        Ok(expr)
    }

    fn not_level(&mut self) -> ParseResult<ExprBox> {
        if self.token != TokenKind::Not {
            return self.ternary_level();
        }
        let position = self.position;
        self.next()?;
        self.enter_nesting()?;
        let operand = self.not_level()?;
        self.depth -= 1;
        Ok(Box::new(Expr::Unary {
            position,
            op: UnaryOp::Not,
            operand,
        }))
    }

    fn ternary_level(&mut self) -> ParseResult<ExprBox> {
        let cond = self.binary_level(3)?;
        if self.token != TokenKind::Question {
            return Ok(cond);
        }
        let position = self.position;
        self.next()?;
        let then = self.expression()?;
        if self.token != TokenKind::Colon {
            return Err(self.record_parse_error(TERNARY_COLON));
        }
        self.next()?;
        let otherwise = self.expression()?;
        Ok(Box::new(Expr::Ternary {
            position,
            cond,
            then,
            otherwise,
        }))
    }

    fn power_level(&mut self) -> ParseResult<ExprBox> {
        let left = self.negation_level()?;
        if self.token != TokenKind::StarStar {
            return Ok(left);
        }
        let position = self.position;
        self.next()?;
        self.enter_nesting()?;
        let right = self.power_level()?;
        self.depth -= 1;
        Ok(Box::new(Expr::Binary {
            position,
            op: BinOp::Pow,
            left,
            right,
        }))
    }

    fn negation_level(&mut self) -> ParseResult<ExprBox> {
        if self.token != TokenKind::Minus {
            return self.postfix_level();
        }
        let position = self.position;
        self.next()?;
        self.enter_nesting()?;
        let operand = self.negation_level()?;
        self.depth -= 1;
        Ok(Box::new(Expr::Unary {
            position,
            op: UnaryOp::Neg,
            operand,
        }))
    }

    fn postfix_level(&mut self) -> ParseResult<ExprBox> {
        let expr = self.primary()?;
        if matches!(
            self.token,
            TokenKind::OpenParen | TokenKind::OpenBracket | TokenKind::Dot
        ) {
            return self.postfix(expr);
        }
        Ok(expr)
    }
}

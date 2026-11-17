//! Token kinds and source positions.

use std::fmt;

/// A 1-based line and column. The column counts characters, not bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Position {
    pub line: u32,
    pub column: u32,
}

impl Position {
    pub(crate) const START: Position = Position { line: 1, column: 1 };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Illegal,
    Eof,
    OpenParen,
    CloseParen,
    Star,
    Plus,
    Comma,
    Minus,
    Dot,
    Slash,
    Colon,
    Lt,
    Assign,
    Gt,
    Question,
    OpenBracket,
    CloseBracket,
    OpenBrace,
    CloseBrace,
    Percent,
    End,
    EqualEqual,
    Ge,
    Le,
    BangEqual,
    PlusEqual,
    MinusEqual,
    StarEqual,
    SlashEqual,
    PercentEqual,
    StarStar,
    Ellipsis,
    And,
    Break,
    Catch,
    Continue,
    Else,
    False,
    For,
    Fun,
    If,
    Import,
    In,
    Null,
    Not,
    Or,
    Return,
    Then,
    True,
    Try,
    While,
    Xor,
    Memo,
    Elif,
    Int,
    Float,
    Name,
    Str,
}

impl TokenKind {
    /// The spelling used in error messages.
    pub fn as_str(self) -> &'static str {
        match self {
            TokenKind::Illegal => "ILLEGAL",
            TokenKind::Eof => "EOF",
            TokenKind::OpenParen => "(",
            TokenKind::CloseParen => ")",
            TokenKind::Star => "*",
            TokenKind::Plus => "+",
            TokenKind::Comma => ",",
            TokenKind::Minus => "-",
            TokenKind::Dot => ".",
            TokenKind::Slash => "/",
            TokenKind::Colon => ":",
            TokenKind::Lt => "<",
            TokenKind::Assign => "=",
            TokenKind::Gt => ">",
            TokenKind::Question => "?",
            TokenKind::OpenBracket => "[",
            TokenKind::CloseBracket => "]",
            TokenKind::OpenBrace => "{",
            TokenKind::CloseBrace => "}",
            TokenKind::Percent => "%",
            TokenKind::End => "end",
            TokenKind::EqualEqual => "==",
            TokenKind::Ge => ">=",
            TokenKind::Le => "<=",
            TokenKind::BangEqual => "!=",
            TokenKind::PlusEqual => "+=",
            TokenKind::MinusEqual => "-=",
            TokenKind::StarEqual => "*=",
            TokenKind::SlashEqual => "/=",
            TokenKind::PercentEqual => "%=",
            TokenKind::StarStar => "**",
            TokenKind::Ellipsis => "...",
            TokenKind::And => "and",
            TokenKind::Break => "break",
            TokenKind::Catch => "catch",
            TokenKind::Continue => "continue",
            TokenKind::Else => "else",
            TokenKind::False => "false",
            TokenKind::For => "for",
            TokenKind::Fun => "fun",
            TokenKind::If => "if",
            TokenKind::Import => "import",
            TokenKind::In => "in",
            TokenKind::Null => "null",
            TokenKind::Not => "not",
            TokenKind::Or => "or",
            TokenKind::Return => "return",
            TokenKind::Then => "then",
            TokenKind::True => "true",
            TokenKind::Try => "try",
            TokenKind::While => "while",
            TokenKind::Xor => "xor",
            TokenKind::Memo => "memo",
            TokenKind::Elif => "elif",
            TokenKind::Int => "int",
            TokenKind::Float => "float",
            TokenKind::Name => "name",
            TokenKind::Str => "str",
        }
    }
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

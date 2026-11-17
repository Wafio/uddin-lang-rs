use std::fmt;

use crate::token::Position;

/// Which stage refused the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Bad characters, a bad escape, a bad number, an unterminated string or comment, invalid UTF-8.
    Tokenize,
    /// Everything else, including nesting past the limit and an integer literal past 64 bits.
    Parse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub position: Position,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "parse error at {}:{}: {}",
            self.position.line, self.position.column, self.message
        )
    }
}

impl std::error::Error for Error {}

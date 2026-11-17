//! Lexer, parser, syntax tree and source positions of the uddin script language.
//!
//! It follows the Go interpreter's grammar exactly: a source accepted here is accepted there, with the same
//! tree shape, and the nesting limit is the same.

#![forbid(unsafe_code)]

mod allowlist;
pub mod ast;
mod builtins;
mod error;
mod lexer;
mod parser;
mod token;

pub use allowlist::disallowed_builtins;
pub use builtins::BUILTIN_TABLE;
pub use error::{Error, ErrorKind};
#[doc(hidden)]
pub use lexer::tokenize;
pub use parser::{NESTING_LEVEL_LIMIT, parse_source};
pub use token::{Position, TokenKind};

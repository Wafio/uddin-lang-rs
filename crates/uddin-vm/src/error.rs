//! What stops or redirects a run: errors a script can catch, control flow in flight, and limits it cannot.

use std::borrow::Cow;

use uddin_syntax::Position;

use crate::heap::Oom;
use crate::value::Value;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LimitReason {
    OpLimit,
    CallDepth,
    Nesting,
    ValueSize,
    ValueDepth,
    Memory,
    Canceled,
    RegexSize,
    RegexWork,
}

impl LimitReason {
    pub fn as_str(self) -> &'static str {
        match self {
            LimitReason::OpLimit => "op_limit",
            LimitReason::CallDepth => "call_depth",
            LimitReason::Nesting => "nesting",
            LimitReason::ValueSize => "value_size",
            LimitReason::ValueDepth => "value_depth",
            LimitReason::Memory => "memory",
            LimitReason::Canceled => "canceled",
            LimitReason::RegexSize => "regex_size",
            LimitReason::RegexWork => "regex_work",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrClass {
    Type,
    Value,
    Name,
    Runtime,
}

impl ErrClass {
    pub fn word(self) -> &'static str {
        match self {
            ErrClass::Type => "type",
            ErrClass::Value => "value",
            ErrClass::Name => "name",
            ErrClass::Runtime => "runtime",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Exc {
    /// An error with a position, which `catch` binds as its text.
    Error {
        class: ErrClass,
        pos: Position,
        msg: String,
    },
    /// A fault of the host language that scripts can provoke, or an error value raised as it is; it has no
    /// position of its own and its text is bytes, since an error value may quote script input.
    Fault(Vec<u8>),
    Break(Position),
    Continue(Position),
    Return(Value, Position),
    /// Ends the run; `catch` cannot see it.
    Limit(LimitReason, Position),
}

/// The class, position and message of a text that says `<class> error at <line>:<column>: <message>`, as the
/// text of an error of the script does. `None` for any other text, and for one that would not read back the same.
pub fn error_text_parts(text: &[u8]) -> Option<(ErrClass, Position, String)> {
    let s = std::str::from_utf8(text).ok()?;
    let (class, rest) = [
        ErrClass::Type,
        ErrClass::Value,
        ErrClass::Name,
        ErrClass::Runtime,
    ]
    .into_iter()
    .find_map(|c| {
        s.strip_prefix(c.word())
            .and_then(|r| r.strip_prefix(" error at "))
            .map(|r| (c, r))
    })?;
    let (line, rest) = rest.split_once(':')?;
    let (column, msg) = rest.split_once(": ")?;
    let pos = Position {
        line: line.parse().ok()?,
        column: column.parse().ok()?,
    };
    let exc = Exc::Error {
        class,
        pos,
        msg: msg.to_owned(),
    };
    (exc.text().as_bytes() == text).then(|| (class, pos, msg.to_owned()))
}

/// The class an error value made from `text` has: the one the text names, as the error of a builtin that
/// returned a typed error says it.
pub(crate) fn error_text_class(text: &[u8]) -> Option<ErrClass> {
    error_text_parts(text).map(|(class, ..)| class)
}

/// What raising an error value does: a value of a class raises the error of that class, any other raises a
/// fault that carries its text.
pub(crate) fn error_value_exc(text: &[u8], class: Option<ErrClass>) -> Exc {
    if class.is_some()
        && let Some((class, pos, msg)) = error_text_parts(text)
    {
        return Exc::Error { class, pos, msg };
    }
    Exc::fault(text)
}

impl Exc {
    pub fn type_error(pos: Position, msg: impl Into<String>) -> Exc {
        Exc::Error {
            class: ErrClass::Type,
            pos,
            msg: msg.into(),
        }
    }

    pub fn value_error(pos: Position, msg: impl Into<String>) -> Exc {
        Exc::Error {
            class: ErrClass::Value,
            pos,
            msg: msg.into(),
        }
    }

    pub fn name_error(pos: Position, msg: impl Into<String>) -> Exc {
        Exc::Error {
            class: ErrClass::Name,
            pos,
            msg: msg.into(),
        }
    }

    pub fn runtime_error(pos: Position, msg: impl Into<String>) -> Exc {
        Exc::Error {
            class: ErrClass::Runtime,
            pos,
            msg: msg.into(),
        }
    }

    pub fn fault(msg: impl Into<Vec<u8>>) -> Exc {
        Exc::Fault(msg.into())
    }

    pub fn limit(reason: LimitReason, pos: Position) -> Exc {
        Exc::Limit(reason, pos)
    }

    pub fn memory(pos: Position) -> impl FnOnce(Oom) -> Exc {
        move |_| Exc::Limit(LimitReason::Memory, pos)
    }

    /// The text `catch` binds: [`Exc::text`] as bytes, which a fault keeps as it came.
    pub fn text_bytes(&self) -> Cow<'_, [u8]> {
        match self {
            Exc::Fault(msg) => Cow::Borrowed(msg.as_slice()),
            other => Cow::Owned(other.text().into_bytes()),
        }
    }

    /// The text a run that ends in this error reports.
    pub fn text(&self) -> String {
        match self {
            Exc::Error { class, pos, msg } => {
                format!(
                    "{} error at {}:{}: {}",
                    class.word(),
                    pos.line,
                    pos.column,
                    msg
                )
            }
            Exc::Fault(msg) => String::from_utf8_lossy(msg).into_owned(),
            Exc::Break(p) => format!("break at {}:{}", p.line, p.column),
            Exc::Continue(p) => format!("continue at {}:{}", p.line, p.column),
            Exc::Return(_, p) => format!("return at {}:{}", p.line, p.column),
            Exc::Limit(reason, p) => {
                format!(
                    "{}:{}: execution stopped: {}",
                    p.line,
                    p.column,
                    reason.as_str()
                )
            }
        }
    }
}

//! Entry point for embedding uddin: compile a script, run it under limits, get an outcome.
//!
//! [`check`] says whether a source would be accepted under a [`Profile`]; [`Script::compile`] turns it into a
//! script and a [`Runner`] runs it under [`Limits`].

#![forbid(unsafe_code)]

pub mod jsonl;
mod profile;
mod script;

use std::fmt;

pub use profile::{Profile, ProfileError};
pub use script::{CompileFailure, HOST_NAMES, RequestVars, Runner, Script};
use uddin_syntax::ast::Program as Ast;
use uddin_syntax::{ErrorKind, disallowed_builtins, parse_source};
pub use uddin_vm::{
    BuiltinDef, Ended as Outcome, Json, LimitReason, Limits, RequestContext, Verdict,
};

/// Why a source was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckError {
    /// Over the size limit; nothing was parsed.
    TooLarge { len: usize, max: usize },
    /// Not valid UTF-8 or not valid syntax, or nested past the limit.
    Syntax(uddin_syntax::Error),
    /// The script names builtins the profile does not allow, or has an `import`. Sorted, no repeats.
    Disallowed(Vec<String>),
}

impl CheckError {
    /// The first check that refused the source: `size`, `tokenize`, `parse` or `allowlist`.
    pub fn stage(&self) -> &'static str {
        match self {
            CheckError::TooLarge { .. } => "size",
            CheckError::Syntax(e) if e.kind == ErrorKind::Tokenize => "tokenize",
            CheckError::Syntax(_) => "parse",
            CheckError::Disallowed(_) => "allowlist",
        }
    }
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckError::TooLarge { len, max } => {
                write!(f, "script is {len} bytes, more than the {max} allowed")
            }
            CheckError::Syntax(e) => write!(f, "{e}"),
            CheckError::Disallowed(names) => {
                write!(f, "script rules cannot use: {}", names.join(", "))
            }
        }
    }
}

impl std::error::Error for CheckError {}

/// Checks a source the way the control plane does when a script rule is saved: size first, then the syntax,
/// then the allowlist. A blank source passes; the host decides what an empty rule means.
pub fn check(src: &[u8], profile: &Profile) -> Result<(), CheckError> {
    parse_checked(src, profile).map(|_| ())
}

/// [`check`], keeping the syntax tree of a source that passes.
pub fn parse_checked(src: &[u8], profile: &Profile) -> Result<Ast, CheckError> {
    let max = profile.max_script_bytes();
    if src.len() > max {
        return Err(CheckError::TooLarge {
            len: src.len(),
            max,
        });
    }
    let program = parse_source(src).map_err(CheckError::Syntax)?;
    let names = disallowed_builtins(&program, |name| profile.allows(name));
    if names.is_empty() {
        Ok(program)
    } else {
        Err(CheckError::Disallowed(names))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uddin_syntax::BUILTIN_TABLE;

    fn run(src: &str) -> Result<(), CheckError> {
        check(src.as_bytes(), &Profile::embedded())
    }

    #[test]
    fn embedded_profile_is_the_pinned_allowlist() {
        let profile = Profile::embedded();
        assert_eq!(profile.max_script_bytes(), 64 << 10);
        let allowed: Vec<&str> = BUILTIN_TABLE
            .iter()
            .copied()
            .filter(|n| profile.allows(n))
            .collect();
        assert_eq!(
            allowed.len(),
            133,
            "every allowed name must be a builtin of the table"
        );
        assert_eq!(BUILTIN_TABLE.len() - allowed.len(), 86);
        assert!(profile.allows("print") && profile.allows("waf_block") && !profile.allows("sleep"));
    }

    #[test]
    fn accepts_allowed_scripts() {
        assert_eq!(run(""), Ok(()));
        assert_eq!(run("  \n\t"), Ok(()));
        assert_eq!(
            run("x = 1\nif (waf_score() > 5) then: waf_block() end"),
            Ok(())
        );
        assert_eq!(run("print = 1"), Ok(()), "allowed names may be reassigned");
    }

    #[test]
    fn size_limit_is_checked_before_parsing() {
        let at = format!("x = 1 //{}", "a".repeat(65_536 - 8));
        assert_eq!(at.len(), 65_536);
        assert_eq!(run(&at), Ok(()));
        let over = format!("{at}a");
        let e = run(&over).expect_err("one byte over");
        assert_eq!(
            (e.stage(), &e),
            (
                "size",
                &CheckError::TooLarge {
                    len: 65_537,
                    max: 65_536
                }
            )
        );
        // Even a source that is not valid UTF-8 or not valid syntax is reported as too large first.
        let e = check(&vec![0xff; 65_537], &Profile::embedded()).expect_err("too large");
        assert_eq!(e.stage(), "size");
    }

    #[test]
    fn nesting_past_the_limit_is_a_parse_error() {
        assert_eq!(
            run(&format!("x = {}1{}", "(".repeat(998), ")".repeat(998))),
            Ok(())
        );
        let e = run(&format!("x = {}1{}", "(".repeat(999), ")".repeat(999)))
            .expect_err("nested too deeply");
        assert_eq!(e.stage(), "parse");
        assert!(e.to_string().contains("nested too deeply"), "{e}");
    }

    #[test]
    fn stages() {
        let stage = |src: &[u8]| {
            check(src, &Profile::embedded())
                .expect_err("refused")
                .stage()
        };
        assert_eq!(stage(b"x = \xff"), "tokenize");
        assert_eq!(stage(b"x = \"open"), "tokenize");
        assert_eq!(stage(b"x = "), "parse");
        assert_eq!(stage(b"x = 9223372036854775808"), "parse");
        assert_eq!(stage(b"sleep(1)"), "allowlist");
    }

    #[test]
    fn names_what_the_allowlist_refused() {
        assert_eq!(
            run("sleep(1)\nx = exit"),
            Err(CheckError::Disallowed(vec!["exit".into(), "sleep".into()]))
        );
        assert_eq!(
            run("import \"lib.din\""),
            Err(CheckError::Disallowed(vec!["import".into()]))
        );
        assert_eq!(
            run("sleep = 1"),
            Err(CheckError::Disallowed(vec!["sleep".into()]))
        );
        assert_eq!(run("x = {sleep: 1}.sleep"), Ok(()));
        let e = run("read_file(1) sleep(2)").expect_err("forbidden");
        assert_eq!(e.to_string(), "script rules cannot use: read_file, sleep");
    }
}

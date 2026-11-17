//! Runs scripts against a builtin table of the test's own: the builtins the build carries plus test builtins.

// Each test file uses some of these.
#![allow(dead_code)]

use uddin::{HOST_NAMES, Limits, Profile};
use uddin_vm::{BuiltinDef, Ended, RunState, compile};

/// The builtins of the build followed by `extra`.
pub fn table(extra: &[BuiltinDef]) -> Vec<BuiltinDef> {
    uddin_std::builtins().iter().chain(extra).copied().collect()
}

pub struct Ran {
    pub ended: Ended,
    pub printed: Vec<u8>,
    pub ops: u64,
}

impl Ran {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.printed).into_owned()
    }
}

/// The limits of the profile without a deadline, with print captured; `edit` changes them.
pub fn limits(edit: impl FnOnce(&mut Limits)) -> Limits {
    let mut limits = Profile::embedded().limits();
    limits.capture_print = true;
    limits.deadline = None;
    edit(&mut limits);
    limits
}

pub fn run(table: &[BuiltinDef], src: &str, edit: impl FnOnce(&mut Limits)) -> Ran {
    run_under(table, src, &limits(edit))
}

pub fn run_under(table: &[BuiltinDef], src: &str, limits: &Limits) -> Ran {
    let profile = Profile::embedded();
    let ast = uddin_syntax::parse_source(src.as_bytes()).expect("the source parses");
    let program = compile(&ast, table, &|name| profile.allows(name), &HOST_NAMES)
        .unwrap_or_else(|e| panic!("the source compiles: {e}"));
    let mut state = RunState::new();
    let ended = state.run(&program, table, &[], limits, None);
    Ran {
        ended,
        printed: state.printed().to_vec(),
        ops: state.ops(),
    }
}

//! A compiled script and the thread-local state that runs it.

use std::fmt;

use uddin_vm::{
    BuiltinDef, CompileError, Ended, HostValue, Limits, Program, RequestContext, RunState, Verdict,
    compile,
};

use crate::{CheckError, Profile, parse_checked};

/// The variables every script finds in its global scope, in the order [`RequestVars::values`] gives them.
pub const HOST_NAMES: [&str; 10] = [
    "method", "path", "query", "ip", "country", "ua", "body", "score", "action", "_waf_ctx",
];

/// What a run knows about the request.
#[derive(Clone, Copy, Debug, Default)]
pub struct RequestVars<'a> {
    pub method: &'a [u8],
    pub path: &'a [u8],
    pub query: &'a [u8],
    pub ip: &'a [u8],
    pub country: &'a [u8],
    pub ua: &'a [u8],
    pub body: &'a [u8],
    pub score: f64,
    pub action: &'a [u8],
}

impl<'a> RequestVars<'a> {
    fn values(&self) -> [HostValue<'a>; 10] {
        [
            HostValue::Str(self.method),
            HostValue::Str(self.path),
            HostValue::Str(self.query),
            HostValue::Str(self.ip),
            HostValue::Str(self.country),
            HostValue::Str(self.ua),
            HostValue::Str(self.body),
            HostValue::Float(self.score),
            HostValue::Str(self.action),
            HostValue::Opaque,
        ]
    }
}

/// Why a source did not become a script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileFailure {
    Check(CheckError),
    /// A `break` or `continue` outside a loop of its own function. The text names the place.
    Flow(String),
    /// The profile allows these builtins but this build does not carry them yet.
    Unported(Vec<String>),
}

impl fmt::Display for CompileFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompileFailure::Check(e) => e.fmt(f),
            CompileFailure::Flow(text) => f.write_str(text),
            CompileFailure::Unported(names) => write!(
                f,
                "builtins not available in this build: {}",
                names.join(", ")
            ),
        }
    }
}

impl std::error::Error for CompileFailure {}

/// A script ready to run: immutable, so any number of threads can run it at once.
pub struct Script {
    program: Program,
    builtins: &'static [BuiltinDef],
}

impl fmt::Debug for Script {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Script")
            .field("program", &self.program)
            .finish_non_exhaustive()
    }
}

impl Script {
    /// Checks `src` as a saved rule is checked, then compiles it against the builtins of the standard library.
    pub fn compile(src: &[u8], profile: &Profile) -> Result<Script, CompileFailure> {
        Script::compile_with(src, profile, uddin_std::builtins())
    }

    /// [`Script::compile`] against `builtins`, which must start with those of the standard library. The script
    /// runs with the same table, whichever runner runs it.
    pub fn compile_with(
        src: &[u8],
        profile: &Profile,
        builtins: &'static [BuiltinDef],
    ) -> Result<Script, CompileFailure> {
        let ast = parse_checked(src, profile).map_err(CompileFailure::Check)?;
        let known = |name: &str| profile.allows(name);
        match compile(&ast, builtins, &known, &HOST_NAMES) {
            Ok(program) => Ok(Script { program, builtins }),
            Err(CompileError::Unported(names)) => Err(CompileFailure::Unported(names)),
            Err(e @ CompileError::Flow { .. }) => Err(CompileFailure::Flow(e.to_string())),
            Err(CompileError::TooLarge) => Err(CompileFailure::Check(CheckError::TooLarge {
                len: src.len(),
                max: profile.max_script_bytes(),
            })),
        }
    }
}

/// The state one thread reuses from run to run.
#[derive(Default)]
pub struct Runner {
    state: RunState,
}

impl Runner {
    pub fn new() -> Runner {
        Runner::default()
    }

    pub fn run(&mut self, script: &Script, vars: &RequestVars<'_>, limits: &Limits) -> Ended {
        self.state.run(
            &script.program,
            script.builtins,
            &vars.values(),
            limits,
            None,
        )
    }

    /// Runs `script` for the request `ctx` describes: its variables are the ones `ctx` gives, and the builtins that
    /// read the request read `ctx`.
    pub fn run_request(
        &mut self,
        script: &Script,
        ctx: &dyn RequestContext,
        limits: &Limits,
    ) -> Ended {
        let values = [
            HostValue::Str(ctx.method()),
            HostValue::Str(ctx.path()),
            HostValue::Str(ctx.query()),
            HostValue::Str(ctx.ip()),
            HostValue::Str(ctx.country()),
            HostValue::Str(ctx.ua()),
            HostValue::Str(ctx.body()),
            HostValue::Float(ctx.score()),
            HostValue::Str(ctx.action()),
            HostValue::Opaque,
        ];
        self.state
            .run(&script.program, script.builtins, &values, limits, Some(ctx))
    }

    /// The first decision the last run wrote.
    pub fn verdict(&self) -> Option<Verdict> {
        self.state.verdict()
    }

    /// What the last run printed, when its limits asked to capture it.
    pub fn printed(&self) -> &[u8] {
        self.state.printed()
    }

    pub fn print_truncated(&self) -> bool {
        self.state.print_truncated()
    }

    pub fn ops(&self) -> u64 {
        self.state.ops()
    }

    /// Bytes this runner holds on to between runs.
    pub fn retained_bytes(&self) -> usize {
        self.state.retained_bytes()
    }
}

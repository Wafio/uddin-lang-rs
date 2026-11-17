//! Script rules for the WAF: the seventeen `waf_*` builtins on top of the uddin interpreter, and the calls an
//! agent makes with them. A rule is compiled once into a [`WafScript`], which any number of threads can run; each
//! thread keeps one [`WafRunner`] and runs a script for a request with limits and a deadline.

#![forbid(unsafe_code)]

mod builtins;
mod glob;
mod json;
mod net;
mod view;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::OnceLock;
use std::time::Instant;

pub use json::parse_json;
pub use uddin::{CompileFailure, LimitReason, Limits, Profile};
use uddin::{Outcome, Runner, Script};
use uddin_vm::{BuiltinDef, Exc};
pub use uddin_vm::{Json, RequestContext, Verdict};
pub use view::{FieldFn, FieldValuesFn, RequestView};

/// The builtins of a script rule: those of the standard library, then the seventeen `waf_*`.
pub fn builtins() -> &'static [BuiltinDef] {
    static ALL: OnceLock<Vec<BuiltinDef>> = OnceLock::new();
    ALL.get_or_init(|| {
        uddin_std::builtins()
            .iter()
            .chain(builtins::TABLE)
            .copied()
            .collect()
    })
}

/// The sandbox profile built into this crate: 133 builtin names, 17 of them `waf_*`, and the limits.
pub fn profile() -> Profile {
    Profile::embedded()
}

/// A rule that passed the check and compiled. It never changes, so threads share it.
#[derive(Debug)]
pub struct WafScript {
    script: Script,
}

impl WafScript {
    /// Checks `src` as a saved rule is checked, then compiles it. An error means the rule does not run: the source
    /// is too large, does not parse, names a builtin the profile does not allow, or has a `break` or `continue`
    /// outside a loop of its own function.
    pub fn compile(src: &[u8], profile: &Profile) -> Result<WafScript, CompileFailure> {
        Script::compile_with(src, profile, builtins()).map(|script| WafScript { script })
    }
}

/// How a run ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum End {
    /// The script ran to its end.
    Done,
    /// A runtime error ended it. `class` is `type`, `value`, `name` or `runtime`.
    Error { class: &'static str, text: String },
    /// A limit or the deadline ended it.
    Limit { reason: LimitReason, text: String },
    /// The interpreter failed inside the run and the failure was caught. The runner starts afresh.
    Panic,
}

/// The result of one run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    /// The first decision the script wrote, whatever way the run ended: a decision followed by an error stands.
    pub verdict: Option<Verdict>,
    pub end: End,
}

impl Run {
    /// Why the rule did not finish, when it decided nothing and did not run to its end: `error` for a runtime
    /// error, `timeout` for the deadline, `panic`, or the name of the limit (`op_limit`, `call_depth`, `nesting`,
    /// `value_size`, `value_depth`, `memory`, `regex_size`, `regex_work`). A rule that decided is never incomplete.
    pub fn incomplete(&self) -> Option<&'static str> {
        if self.verdict.is_some() {
            return None;
        }
        match &self.end {
            End::Done => None,
            End::Error { .. } => Some("error"),
            End::Limit {
                reason: LimitReason::Canceled,
                ..
            } => Some("timeout"),
            End::Limit { reason, .. } => Some(reason.as_str()),
            End::Panic => Some("panic"),
        }
    }
}

/// The state one thread keeps from run to run. Not shared between threads.
#[derive(Default)]
pub struct WafRunner {
    runner: Runner,
}

impl WafRunner {
    pub fn new() -> WafRunner {
        WafRunner::default()
    }

    /// Runs `script` for the request `ctx` describes, under `limits`. `until` is the end of the time the request
    /// may spend on script rules: the run stops at the sooner of it and `limits.deadline`, and does not start when
    /// it has passed. A panic inside the run is caught and reported as [`End::Panic`].
    pub fn run(
        &mut self,
        script: &WafScript,
        ctx: &dyn RequestContext,
        limits: &Limits,
        until: Option<Instant>,
    ) -> Run {
        let mut limits = limits.clone();
        if let Some(until) = until {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                let stopped = Exc::limit(LimitReason::Canceled, Default::default());
                return Run {
                    verdict: None,
                    end: End::Limit {
                        reason: LimitReason::Canceled,
                        text: stopped.text(),
                    },
                };
            }
            limits.deadline = Some(limits.deadline.map_or(left, |d| d.min(left)));
        }
        let ran = catch_unwind(AssertUnwindSafe(|| {
            let ended = self.runner.run_request(&script.script, ctx, &limits);
            (ended, self.runner.verdict())
        }));
        match ran {
            Ok((ended, verdict)) => Run {
                verdict,
                end: match ended {
                    Outcome::Ok => End::Done,
                    Outcome::Error { class, text } => End::Error { class, text },
                    Outcome::Limit { reason, text } => End::Limit { reason, text },
                },
            },
            Err(_) => {
                self.runner = Runner::new();
                Run {
                    verdict: None,
                    end: End::Panic,
                }
            }
        }
    }

    /// What the last run printed, when its limits asked to capture it.
    pub fn printed(&self) -> &[u8] {
        self.runner.printed()
    }

    pub fn print_truncated(&self) -> bool {
        self.runner.print_truncated()
    }

    /// Operations the last run counted.
    pub fn ops(&self) -> u64 {
        self.runner.ops()
    }
}

#[cfg(test)]
mod tests;

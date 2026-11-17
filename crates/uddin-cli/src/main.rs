//! The `uddin` command line. It reads cases on stdin and writes results to stdout.

#![forbid(unsafe_code)]

mod waf;

use std::io::{self, BufWriter, Write};
use std::process::ExitCode;
use std::time::Duration;

use uddin::jsonl::{check_jsonl, run_jsonl_with};
use uddin::{Limits, Profile};

const USAGE: &str = "usage: uddin check < cases.jsonl > results.jsonl
       uddin run [-deadline-ms N] [-limit NAME=N]... < cases.jsonl > results.jsonl
limit names: max_ops max_call_depth max_nesting max_value_len max_alloc_bytes";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("check") if args.len() == 1 => finish("check", |input, out| {
            check_jsonl(input, out, &Profile::embedded())
        }),
        Some("run") => match run_limits(&args[1..]) {
            Some(limits) => finish("run", |input, out| {
                run_jsonl_with(
                    input,
                    out,
                    &Profile::embedded(),
                    limits,
                    &mut waf::WafHost::default(),
                )
            }),
            None => usage(),
        },
        _ => usage(),
    }
}

/// The limits of the embedded profile with the command line flags applied; `None` for a flag it does not know.
fn run_limits(flags: &[String]) -> Option<Limits> {
    let mut limits = Profile::embedded().limits();
    let mut flags = flags.iter();
    while let Some(flag) = flags.next() {
        match flag.as_str() {
            "-deadline-ms" => {
                let ms: u64 = flags.next()?.parse().ok()?;
                limits.deadline = (ms > 0).then(|| Duration::from_millis(ms));
            }
            "-limit" => {
                let (name, value) = flags.next()?.split_once('=')?;
                let n: u64 = value.parse().ok()?;
                match name {
                    "max_ops" => limits.max_ops = n,
                    "max_call_depth" => limits.max_call_depth = u32::try_from(n).ok()?,
                    "max_nesting" => limits.max_nesting = u32::try_from(n).ok()?,
                    "max_value_len" => limits.max_value_len = usize::try_from(n).ok()?,
                    "max_alloc_bytes" => limits.max_alloc_bytes = usize::try_from(n).ok()?,
                    _ => return None,
                }
            }
            _ => return None,
        }
    }
    Some(limits)
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn finish(
    name: &str,
    f: impl FnOnce(io::StdinLock<'_>, &mut BufWriter<io::StdoutLock<'_>>) -> io::Result<()>,
) -> ExitCode {
    let mut out = BufWriter::new(io::stdout().lock());
    match f(io::stdin().lock(), &mut out).and_then(|()| out.flush()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("uddin {name}: {e}");
            ExitCode::from(1)
        }
    }
}

//! Tools for the acceptance comparison between this implementation and the Go interpreter.
//!
//!     parity gen --corpus DIR [--seed N] [--start N] [--count N]     cases, one JSON object per line
//!     parity compare --go FILE --rust FILE [--cases FILE] [--out FILE] [--show N] [--show-stage N] [--kinds]
//!     parity compare-run --go FILE --rust FILE [--cases FILE] [--out FILE] [--show N]   run results
//!     parity gen-run [--seed N] [--start N] [--count N]             programs for the run comparison
//!     parity seeds --corpus DIR --out DIR                           the corpus sources, one file each
//!     parity gen --enum LEN [--alphabet full|core|blocks] [--start N] [--count N]   every sequence of LEN tokens
//!     parity gen --corpus DIR --neighbors [--start N] [--count N]   every source one edit from a corpus source
//!     parity size --enum LEN [--alphabet A] | --corpus DIR --neighbors   how many sources `gen` has in those modes
//!
//! `gen` writes the case format of `oracle-go check` and `uddin check`. Case `i` of a run depends on the seed
//! and `i` only, so `--start i --count 1` rebuilds a case from its id. `compare` pairs the result files line by
//! line and fails when `ok` or `forbidden` differ; a difference in `stage` alone is counted and shown.

mod b64;
mod enumerate;
mod focus;
mod grammar;
mod mutate;
mod names;
mod rng;
mod runcmp;
mod rungen;

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::{Value, json};

use names::Names;
use rng::Rng;

fn case_source(
    seed: u64,
    index: u64,
    names: &Names,
    corpus: &[Vec<u8>],
) -> (&'static str, Vec<u8>) {
    let mut rng = Rng::for_case(seed, index);
    let from_corpus = |rng: &mut Rng| rng.pick(corpus).clone();
    match rng.weighted(&[360, 140, 90, 170, 40, 80, 30, 20, 30, 10, 30]) {
        0 => {
            let src = grammar::program(&mut rng, names).into_bytes();
            if rng.chance(4) {
                ("gb", mutate::byte_mutate(&mut rng, &src))
            } else {
                ("g", src)
            }
        }
        1 => {
            let src = grammar::program(&mut rng, names).into_bytes();
            let other = from_corpus(&mut rng);
            ("gd", mutate::token_mutate(&mut rng, names, &src, &other))
        }
        2 => {
            let src = from_corpus(&mut rng);
            ("cb", mutate::byte_mutate(&mut rng, &src))
        }
        3 => {
            let src = from_corpus(&mut rng);
            let other = from_corpus(&mut rng);
            ("ct", mutate::token_mutate(&mut rng, names, &src, &other))
        }
        4 => {
            let (a, b) = (from_corpus(&mut rng), from_corpus(&mut rng));
            let cut_a = rng.below(a.len() + 1);
            let cut_b = rng.below(b.len() + 1);
            ("sp", [&a[..cut_a], &b[cut_b..]].concat())
        }
        5 => ("lx", focus::lexical(&mut rng)),
        6 => ("ne", focus::nesting(&mut rng).into_bytes()),
        7 => ("ch", focus::chain(&mut rng).into_bytes()),
        8 => ("bd", focus::boundary(&mut rng).into_bytes()),
        9 => ("sz", focus::sized(&mut rng, names)),
        _ => ("al", focus::allowlist(&mut rng, names).into_bytes()),
    }
}

fn case_line(id: &str, src: &[u8]) -> String {
    let value = match std::str::from_utf8(src) {
        Ok(text) => json!({"id": id, "src": text}),
        Err(_) => json!({"id": id, "src_b64": b64::encode(src)}),
    };
    value.to_string()
}

pub(crate) struct Args(Vec<String>);

impl Args {
    pub(crate) fn get(&self, flag: &str) -> Option<&str> {
        let at = self.0.iter().position(|a| a == flag)?;
        self.0.get(at + 1).map(String::as_str)
    }

    pub(crate) fn path(&self, flag: &str) -> io::Result<PathBuf> {
        self.get(flag).map(PathBuf::from).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, format!("{flag} is required"))
        })
    }

    pub(crate) fn number(&self, flag: &str, default: u64) -> io::Result<u64> {
        match self.get(flag) {
            None => Ok(default),
            Some(text) => text.parse().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{flag} wants a number"),
                )
            }),
        }
    }
}

fn gen_cases(args: &Args) -> io::Result<bool> {
    if args.get("--enum").is_some() {
        return gen_enum(args);
    }
    let corpus = mutate::load_corpus(&args.path("--corpus")?)?;
    if args.0.iter().any(|a| a == "--neighbors") {
        return gen_neighbors(args, &corpus);
    }
    if corpus.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the corpus has no sources",
        ));
    }
    let (seed, start, count) = (
        args.number("--seed", 1)?,
        args.number("--start", 0)?,
        args.number("--count", 1000)?,
    );
    let names = Names::load();
    let mut out = BufWriter::new(io::stdout().lock());
    for index in start..start + count {
        let (kind, src) = case_source(seed, index, &names, &corpus);
        writeln!(
            out,
            "{}",
            case_line(&format!("{kind}/{seed}/{index}"), &src)
        )?;
    }
    out.flush()?;
    Ok(true)
}

/// `gen --corpus DIR --neighbors [--start N] [--count N]`: every source one edit from a corpus source.
/// `gen-run [--seed N] [--start N] [--count N]`: programs that mostly run, in the case format of `uddin run`.
fn gen_run(args: &Args) -> io::Result<bool> {
    let (seed, start, count) = (
        args.number("--seed", 1)?,
        args.number("--start", 0)?,
        args.number("--count", 1000)?,
    );
    let mut out = BufWriter::new(io::stdout().lock());
    for index in start..start + count {
        let src = rungen::program(&mut Rng::for_case(seed, index));
        writeln!(
            out,
            "{}",
            case_line(&format!("r/{seed}/{index}"), src.as_bytes())
        )?;
    }
    out.flush()?;
    Ok(true)
}

fn gen_neighbors(args: &Args, corpus: &[Vec<u8>]) -> io::Result<bool> {
    let neighbors = enumerate::Neighbors::new(corpus);
    let total = neighbors.size();
    let start = args.number("--start", 0)?;
    let count = args
        .number("--count", total)?
        .min(total.saturating_sub(start));
    let mut out = BufWriter::new(io::stdout().lock());
    for index in start..start + count {
        writeln!(
            out,
            "{}",
            case_line(&format!("nb/{index}"), &neighbors.case(index))
        )?;
    }
    out.flush()?;
    Ok(true)
}

fn enum_alphabet(args: &Args) -> io::Result<(u32, Vec<&'static str>)> {
    let bad = |what: &str| io::Error::new(io::ErrorKind::InvalidInput, what.to_owned());
    let len = args
        .get("--enum")
        .and_then(|l| l.parse().ok())
        .ok_or_else(|| bad("--enum wants a length"))?;
    let alphabet = enumerate::alphabet(args.get("--alphabet").unwrap_or("full"))
        .ok_or_else(|| bad("--alphabet is full, core or blocks"))?;
    Ok((len, alphabet))
}

/// `gen --enum LEN [--alphabet A] [--start N] [--count N]`: sequences of LEN tokens, see `enumerate.rs`.
fn gen_enum(args: &Args) -> io::Result<bool> {
    let (len, alphabet) = enum_alphabet(args)?;
    let total = enumerate::size(&alphabet, len);
    let start = args.number("--start", 0)?;
    let count = args
        .number("--count", total)?
        .min(total.saturating_sub(start));
    let mut out = BufWriter::new(io::stdout().lock());
    for index in start..start + count {
        let src = enumerate::sequence(&alphabet, len, index);
        writeln!(
            out,
            "{}",
            case_line(&format!("en/{len}/{index}"), src.as_bytes())
        )?;
    }
    out.flush()?;
    Ok(true)
}

/// `size`: how many sources `gen --enum` or `gen --neighbors` has.
fn size(args: &Args) -> io::Result<bool> {
    if args.0.iter().any(|a| a == "--neighbors") {
        let corpus = mutate::load_corpus(&args.path("--corpus")?)?;
        println!("{}", enumerate::Neighbors::new(&corpus).size());
        return Ok(true);
    }
    let (len, alphabet) = enum_alphabet(args)?;
    println!("{}", enumerate::size(&alphabet, len));
    Ok(true)
}

fn seeds(args: &Args) -> io::Result<bool> {
    let out = args.path("--out")?;
    fs::create_dir_all(&out)?;
    for (i, src) in mutate::load_corpus(&args.path("--corpus")?)?
        .iter()
        .enumerate()
    {
        fs::write(out.join(format!("{i:05}")), src)?;
    }
    Ok(true)
}

pub(crate) fn lines(path: &Path) -> io::Result<impl Iterator<Item = io::Result<String>> + use<>> {
    Ok(BufReader::with_capacity(1 << 20, File::open(path)?).lines())
}

fn describe(v: &Value) -> String {
    format!(
        "ok={} stage={} forbidden={}",
        v["ok"], v["stage"], v["forbidden"]
    )
}

pub(crate) fn preview(case: &Value) -> String {
    let bytes = match (case.get("src"), case.get("src_b64")) {
        (Some(Value::String(s)), _) => s.as_bytes().to_vec(),
        (_, Some(Value::String(s))) => b64::decode(s).unwrap_or_default(),
        _ => Vec::new(),
    };
    let text = String::from_utf8_lossy(&bytes);
    let head: String = text.chars().take(240).collect();
    format!("{} bytes: {head:?}", bytes.len())
}

fn compare(args: &Args) -> io::Result<bool> {
    let bad_data = |what: String| io::Error::new(io::ErrorKind::InvalidData, what);
    let (oracle_path, rust_path) = (args.path("--go")?, args.path("--rust")?);
    let mut oracle_lines = lines(&oracle_path)?;
    let mut rust = lines(&rust_path)?;
    let mut cases = match args.get("--cases") {
        Some(p) => Some(lines(Path::new(p))?),
        None => None,
    };
    let mut mismatches_out = match args.get("--out") {
        Some(p) => Some(BufWriter::new(File::create(p)?)),
        None => None,
    };
    let show = args.number("--show", 20)?;
    let stage_show = args.number("--show-stage", 0)?;
    let (mut total, mut bad) = (0u64, 0u64);
    let mut stage_only: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut by_stage: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_kind: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    loop {
        let (g, r) = (oracle_lines.next().transpose()?, rust.next().transpose()?);
        let case_line = cases.as_mut().and_then(Iterator::next).transpose()?;
        let (g, r) = match (g, r) {
            (None, None) => break,
            (Some(g), Some(r)) => (g, r),
            _ => {
                return Err(bad_data(format!(
                    "the result files differ in length after {total} lines"
                )));
            }
        };
        total += 1;
        let parse = |t: &str| {
            serde_json::from_str::<Value>(t).map_err(|e| bad_data(format!("line {total}: {e}")))
        };
        let (gv, rv) = (parse(&g)?, parse(&r)?);
        let stage = gv["stage"].as_str().unwrap_or("ok").to_owned();
        let kind = gv["id"]
            .as_str()
            .unwrap_or("")
            .split('/')
            .next()
            .unwrap_or("");
        *by_kind
            .entry(kind.to_owned())
            .or_default()
            .entry(stage.clone())
            .or_default() += 1;
        *by_stage.entry(stage).or_default() += 1;
        if gv["id"] != rv["id"] {
            return Err(bad_data(format!(
                "line {total}: ids differ: {} and {}",
                gv["id"], rv["id"]
            )));
        }
        if gv["ok"] != rv["ok"] || gv["forbidden"] != rv["forbidden"] {
            bad += 1;
            let case = case_line
                .as_deref()
                .map(serde_json::from_str::<Value>)
                .transpose()
                .map_err(|e| bad_data(e.to_string()))?;
            if bad <= show {
                println!(
                    "MISMATCH {}\n  go:   {}\n  rust: {}",
                    gv["id"],
                    describe(&gv),
                    describe(&rv)
                );
                if let Some(case) = &case {
                    println!("  src:  {}", preview(case));
                }
            }
            if let (Some(out), Some(line)) = (mismatches_out.as_mut(), case_line.as_ref()) {
                writeln!(out, "{line}")?;
            }
        } else if gv["stage"] != rv["stage"] {
            let count = stage_only
                .entry((gv["stage"].to_string(), rv["stage"].to_string()))
                .or_default();
            *count += 1;
            if *count <= stage_show {
                println!(
                    "STAGE {}: go {} rust {}",
                    gv["id"], gv["stage"], rv["stage"]
                );
                if let Some(Ok(case)) = case_line.as_deref().map(serde_json::from_str::<Value>) {
                    println!("  src:  {}", preview(&case));
                }
            }
        }
    }
    if let Some(mut out) = mismatches_out {
        out.flush()?;
    }
    for ((g, r), n) in &stage_only {
        println!("stage only: go {g} rust {r}: {n}");
    }
    let spread = |m: &BTreeMap<String, u64>| {
        m.iter()
            .map(|(stage, n)| format!("{stage} {n}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    if args.0.iter().any(|a| a == "--kinds") {
        for (kind, stages) in &by_kind {
            println!("  {kind}: {}", spread(stages));
        }
    }
    println!("Go stages: {}", spread(&by_stage));
    println!("{total} compared, {bad} mismatches");
    Ok(bad == 0)
}

fn main() -> ExitCode {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let args = Args(all.clone());
    let result = match all.first().map(String::as_str) {
        Some("gen") => gen_cases(&args),
        Some("compare") => compare(&args),
        Some("compare-run") => runcmp::compare_run(&args),
        Some("gen-run") => gen_run(&args),
        Some("seeds") => seeds(&args),
        Some("size") => size(&args),
        _ => {
            eprintln!(
                "usage: parity gen|compare|seeds|size ... (see the top of conformance/parity/src/main.rs)"
            );
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("parity: {e}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_cases(seed: u64, from: u64, count: u64) -> Vec<(&'static str, Vec<u8>)> {
        let names = Names::load();
        let corpus = vec![
            b"x = 1\nif (x > 0) then: print(x) end\n".to_vec(),
            b"fun f(a): return a end".to_vec(),
        ];
        (from..from + count)
            .map(|i| case_source(seed, i, &names, &corpus))
            .collect()
    }

    #[test]
    fn a_case_depends_on_seed_and_index_only() {
        let all = all_cases(5, 0, 300);
        assert_eq!(all[100..110], all_cases(5, 100, 10)[..]);
        assert_ne!(all[..], all_cases(6, 0, 300)[..]);
    }

    #[test]
    fn every_kind_of_case_shows_up_and_stays_in_bounds() {
        let kinds: std::collections::BTreeSet<_> = all_cases(1, 0, 4000)
            .into_iter()
            .map(|(k, src)| {
                assert!(src.len() < 400_000, "{k} made {} bytes", src.len());
                k
            })
            .collect();
        assert_eq!(kinds.len(), 12, "{kinds:?}");
    }

    #[test]
    fn case_lines_round_trip() {
        let line = case_line("a", b"x = 1\n");
        assert_eq!(line, r#"{"id":"a","src":"x = 1\n"}"#);
        let line = case_line("b", b"x = \xff");
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(
            b64::decode(v["src_b64"].as_str().unwrap()).unwrap(),
            b"x = \xff"
        );
    }
}

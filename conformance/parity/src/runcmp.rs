//! The comparison of two files of run results: what `oracle-go run` gave and what `uddin run` gives.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use serde_json::Value;

use crate::{Args, b64, lines, preview};

fn field<'a>(v: &'a Value, key: &str) -> &'a Value {
    v.get(key).unwrap_or(&Value::Null)
}

fn flag(v: &Value, key: &str) -> bool {
    field(v, key).as_bool().unwrap_or(false)
}

fn printed(v: &Value) -> Vec<u8> {
    field(v, "print_b64")
        .as_str()
        .and_then(|text| b64::decode(text).ok())
        .unwrap_or_default()
}

fn sorted_lines(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut parts: Vec<Vec<u8>> = bytes.split(|&b| b == b'\n').map(<[u8]>::to_vec).collect();
    parts.sort();
    parts
}

/// A difference of a kind the reference cannot have been right about, found by what the reference said: a fault of
/// the host language, the remainder by a float that truncates to zero, or the most negative integer printed as `-`.
/// A run that stops at the size limit of a value in another place, with the same output, differs only in the length
/// of a text a fix changed. Only a comparison of generated programs uses this, since nothing records an expectation
/// for each of those.
fn auto_known(go: &Value, rust: &Value) -> Option<&'static str> {
    let text = format!(
        "{}{}",
        field(go, "error"),
        String::from_utf8_lossy(&printed(go))
    );
    if text.contains("integer divide by zero") {
        return Some("float-modulo");
    }
    let faults = [
        "invalid memory address or nil pointer dereference",
        "memory access error: attempting to access invalid",
        "interface conversion:",
        "slice bounds out of range",
        "slice operation is out of bounds",
        "comparing uncomparable type",
    ];
    if faults.iter().any(|sig| text.contains(sig)) {
        return Some("faults");
    }
    let (go_out, rust_out) = (printed(go), printed(rust));
    if rust_out != go_out && dash(&rust_out, b"-9223372036854775808") == go_out {
        return Some("min-int");
    }
    let same = |key: &str| field(go, key) == field(rust, key);
    (field(go, "outcome") == "limit"
        && field(go, "limit") == "value_size"
        && same("limit")
        && same("print_b64")
        && !same("error"))
    .then_some("message-length")
}

/// `text` with every `needle` written as a single `-`.
fn dash(text: &[u8], needle: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if text[i..].starts_with(needle) {
            out.push(b'-');
            i += needle.len();
        } else {
            out.push(text[i]);
            i += 1;
        }
    }
    out
}

enum Verdict {
    Equal,
    /// A difference that an expectation of the group names and this implementation gives.
    Intended,
    Known(&'static str),
    Differs(String),
}

fn judge(go: &Value, rust: &Value) -> Verdict {
    let same = |key: &str| field(go, key) == field(rust, key);
    if go["outcome"] == "rejected" || rust["outcome"] == "rejected" {
        let names = |v: &Value| {
            field(v, "forbidden")
                .as_array()
                .cloned()
                .unwrap_or_default()
        };
        let text_ok = field(go, "stage") != "compile" || same("error");
        let ok = same("outcome") && same("stage") && names(go) == names(rust) && text_ok;
        return if ok {
            Verdict::Equal
        } else {
            Verdict::Differs("check result".into())
        };
    }
    if flag(go, "timing") || flag(go, "clock") {
        return if same("outcome") && same("limit") {
            Verdict::Equal
        } else {
            Verdict::Differs("outcome or limit of a timing case".into())
        };
    }
    if flag(go, "order_sensitive") {
        // Go gave several results and only one is recorded; Rust's map order is another one that Go can give.
        let ok = same("outcome")
            && same("limit")
            && same("error_class")
            && sorted_lines(&printed(go)).len() == sorted_lines(&printed(rust)).len();
        return if ok {
            Verdict::Equal
        } else {
            Verdict::Differs("order-sensitive case".into())
        };
    }
    for key in [
        "outcome",
        "verdict",
        "limit",
        "error_class",
        "error",
        "print_b64",
    ] {
        if !same(key) {
            return Verdict::Differs(key.to_owned());
        }
    }
    if flag(go, "print_truncated") != flag(rust, "print_truncated") {
        return Verdict::Differs("print_truncated".into());
    }
    Verdict::Equal
}

/// The expectations of a group: one line for each case a fix changes, with the result this implementation must
/// give and `defects`, the rows of the README table the difference belongs to.
fn read_intended(path: &Path) -> io::Result<HashMap<String, Value>> {
    let mut map = HashMap::new();
    for line in lines(path)? {
        let line = line?;
        let v: Value = serde_json::from_str(&line)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{path:?}: {e}")))?;
        let id = field(&v, "id").as_str().unwrap_or_default().to_owned();
        map.insert(id, v);
    }
    Ok(map)
}

/// The first cell of each row of the README tables: the names a `defects` list may use.
fn readme_rows(path: &Path) -> io::Result<HashSet<String>> {
    let mut rows = HashSet::new();
    for line in lines(path)? {
        let line = line?;
        if let Some(rest) = line.strip_prefix("| ")
            && let Some((cell, _)) = rest.split_once(" |")
        {
            rows.insert(cell.trim().trim_matches('`').to_owned());
        }
    }
    Ok(rows)
}

/// Judges a case that has an expectation: the reference must differ from it, or the entry is not needed, and this
/// implementation must give it.
fn judge_intended(
    go: &Value,
    want: &Value,
    rust: &Value,
    rows: Option<&HashSet<String>>,
    intends: &mut BTreeMap<String, u64>,
) -> Verdict {
    let defects: Vec<String> = field(want, "defects")
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|d| d.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    if defects.is_empty() {
        return Verdict::Differs("intended entry without a defect".into());
    }
    if let Some(rows) = rows
        && let Some(missing) = defects.iter().find(|d| !rows.contains(*d))
    {
        return Verdict::Differs(format!("defect {missing} has no row in the README"));
    }
    // The flags of the recorded run stay: a case that depends on the clock or the map order is judged as before.
    let mut expected = want.clone();
    if let (Some(e), Some(g)) = (expected.as_object_mut(), go.as_object()) {
        for flag in ["timing", "clock", "order_sensitive"] {
            if let Some(v) = g.get(flag) {
                e.insert(flag.to_owned(), v.clone());
            }
        }
    }
    if matches!(judge(go, &expected), Verdict::Equal) {
        return Verdict::Differs("intended entry equal to the reference".into());
    }
    if !matches!(judge(&expected, rust), Verdict::Equal) {
        return Verdict::Differs("result is not the intended one".into());
    }
    for d in defects {
        *intends.entry(d).or_default() += 1;
    }
    Verdict::Intended
}

fn flag_set(args: &Args, name: &str) -> bool {
    args.get(name).is_some() || std::env::args().any(|a| a == name)
}

pub fn compare_run(args: &Args) -> io::Result<bool> {
    let bad_data = |what: String| io::Error::new(io::ErrorKind::InvalidData, what);
    let (mut go, mut rust) = (lines(&args.path("--go")?)?, lines(&args.path("--rust")?)?);
    let mut cases = match args.get("--cases") {
        Some(p) => Some(lines(Path::new(p))?),
        None => None,
    };
    let mut out = match args.get("--out") {
        Some(p) => Some(BufWriter::new(File::create(p)?)),
        None => None,
    };
    // The cases that differ, in the input format, for a second look with more runs of Go.
    let mut retry = match args.get("--retry") {
        Some(p) => Some(BufWriter::new(File::create(p)?)),
        None => None,
    };
    let show = args.number("--show", 20)?;
    let auto = flag_set(args, "--auto-known");
    let intended = match args.get("--intended") {
        Some(p) => read_intended(Path::new(p))?,
        None => HashMap::new(),
    };
    let rows = match args.get("--readme") {
        Some(p) => Some(readme_rows(Path::new(p))?),
        None => None,
    };
    let mut seen: HashSet<String> = HashSet::new();
    let mut intends: BTreeMap<String, u64> = BTreeMap::new();
    let (mut total, mut unported, mut equal, mut bad, mut intended_cases) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
    let mut knowns: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut kinds: BTreeMap<String, u64> = BTreeMap::new();
    let mut shapes: BTreeMap<String, u64> = BTreeMap::new();
    loop {
        match (go.next(), rust.next()) {
            (None, None) => break,
            (Some(g), Some(r)) => {
                let (g, r) = (g?, r?);
                let case = match cases.as_mut().and_then(Iterator::next) {
                    Some(c) => serde_json::from_str::<Value>(&c?).ok(),
                    None => None,
                };
                let (gv, rv): (Value, Value) = (
                    serde_json::from_str(&g)
                        .map_err(|e| bad_data(format!("go line {}: {e}", total + 1)))?,
                    serde_json::from_str(&r)
                        .map_err(|e| bad_data(format!("rust line {}: {e}", total + 1)))?,
                );
                total += 1;
                if gv["id"] != rv["id"] {
                    return Err(bad_data(format!(
                        "line {total}: ids {} and {} differ",
                        gv["id"], rv["id"]
                    )));
                }
                if rv["outcome"] == "unported" {
                    unported += 1;
                    continue;
                }
                let shape = match (field(&gv, "outcome").as_str(), field(&gv, "limit").as_str()) {
                    (Some("limit"), Some(l)) => format!("limit:{l}"),
                    (Some("error"), _) => {
                        format!("error:{}", field(&gv, "error_class").as_str().unwrap_or(""))
                    }
                    (Some(o), _) => o.to_owned(),
                    _ => String::new(),
                };
                *shapes.entry(shape).or_default() += 1;
                let verdict = match intended.get(gv["id"].as_str().unwrap_or_default()) {
                    Some(want) => {
                        seen.insert(gv["id"].as_str().unwrap_or_default().to_owned());
                        judge_intended(&gv, want, &rv, rows.as_ref(), &mut intends)
                    }
                    None if auto => match auto_known(&gv, &rv) {
                        Some(why) if !matches!(judge(&gv, &rv), Verdict::Equal) => {
                            Verdict::Known(why)
                        }
                        _ => judge(&gv, &rv),
                    },
                    None => judge(&gv, &rv),
                };
                match verdict {
                    Verdict::Equal => equal += 1,
                    Verdict::Intended => intended_cases += 1,
                    Verdict::Known(why) => *knowns.entry(why).or_default() += 1,
                    Verdict::Differs(what) => {
                        bad += 1;
                        *kinds.entry(what.clone()).or_default() += 1;
                        if bad <= show {
                            println!("case {} differs in {what}", gv["id"]);
                            if let Some(c) = &case {
                                println!("  source:   {}", preview(c));
                            }
                            println!("  go:   {:.400}", g);
                            println!("  rust: {:.400}", r);
                        }
                        if let Some(w) = out.as_mut() {
                            writeln!(w, "{g}\n{r}")?;
                        }
                        if let (Some(w), Some(c)) = (retry.as_mut(), &case) {
                            writeln!(w, "{c}")?;
                        }
                    }
                }
            }
            _ => return Err(bad_data("the result files have different lengths".into())),
        }
    }
    for id in intended.keys().filter(|id| !seen.contains(*id)) {
        bad += 1;
        *kinds
            .entry("intended entry for a case that did not run".into())
            .or_default() += 1;
        if bad <= show {
            println!("intended entry {id} matches no case of this run");
        }
    }
    let known_total: u64 = knowns.values().sum();
    println!(
        "{total} cases: {unported} not yet ported, {equal} equal, {intended_cases} intended differences, {known_total} known differences, {bad} differ"
    );
    for (defect, n) in &intends {
        println!("  intended: {n} x {defect}");
    }
    if flag_set(args, "--shapes") {
        let line: Vec<String> = shapes.iter().map(|(k, n)| format!("{k}={n}")).collect();
        println!("  results: {}", line.join(" "));
    }
    for (why, n) in &knowns {
        println!("  known: {n} x {why}");
    }
    for (what, n) in &kinds {
        println!("  differs: {n} x {what}");
    }
    Ok(bad == 0)
}

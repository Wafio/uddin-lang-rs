//! What the Go oracle recorded for the parser and for searches, replayed against this implementation: the error
//! text and sizes of 600 patterns, and for 1,500 pairs of a pattern and a text, some of them not valid UTF-8, the
//! results of all the searches. A script cannot make a text that is not valid UTF-8, so these are the only place
//! where that part of the behaviour is compared.

use uddin_vm::{Exc, Limits, Meter};

use super::cache::{self, Ready};
use super::iter::Searcher;
use super::search::Hay;
use super::syntax;

const FACTS: &str = include_str!("../../../../conformance/corpus/regex/facts.tsv");

fn unbase64(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => 0,
    };
    let bytes: Vec<u8> = s.bytes().filter(|&c| c != b'=').collect();
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let mut acc = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            acc |= u32::from(val(c)) << (18 - 6 * i);
        }
        let n = chunk.len() * 6 / 8;
        out.extend_from_slice(&acc.to_be_bytes()[1..1 + n]);
    }
    out
}

fn show(b: &[u8]) -> String {
    format!("{:?}", String::from_utf8_lossy(b))
}

fn spans(field: &str) -> Vec<(usize, usize)> {
    if field == "-" {
        return Vec::new();
    }
    field
        .split(';')
        .map(|s| {
            let (a, b) = s.split_once(',').expect("a span");
            (a.parse().expect("number"), b.parse().expect("number"))
        })
        .collect()
}

#[test]
fn parses_as_the_oracle_does() {
    let mut bad = Vec::new();
    let mut checked = 0;
    for line in FACTS.lines().filter(|l| l.starts_with("parse\t")) {
        let f: Vec<&str> = line.split('\t').collect();
        let (p, err) = (unbase64(f[1]), unbase64(f[2]));
        let (est, prog, after): (i64, u64, u64) = (
            f[3].parse().expect("number"),
            f[4].parse().expect("number"),
            f[5].parse().expect("number"),
        );
        checked += 1;
        match syntax::parse(&p) {
            Err(e) if e.text() != err => bad.push(format!(
                "{}: error {} want {}",
                show(&p),
                show(&e.text()),
                show(&err)
            )),
            Err(_) => {}
            Ok(_) if !err.is_empty() => {
                bad.push(format!("{}: accepted, want {}", show(&p), show(&err)))
            }
            Ok(parsed) => {
                let got = (
                    super::shape::estimate(&parsed.root, 1024),
                    super::shape::program_len(&parsed.root),
                    super::shape::after_program_len(&parsed.root),
                );
                if got != (est, prog, after) {
                    bad.push(format!(
                        "{}: sizes {got:?} want {:?}",
                        show(&p),
                        (est, prog, after)
                    ));
                }
            }
        }
    }
    assert!(checked > 500, "{checked} parse facts");
    assert!(
        bad.is_empty(),
        "{} of {checked} differ:\n{}",
        bad.len(),
        bad[..bad.len().min(25)].join("\n")
    );
}

#[test]
fn searches_as_the_oracle_does() {
    let mut bad = Vec::new();
    let mut checked = 0;
    for line in FACTS.lines().filter(|l| l.starts_with("search\t")) {
        let f: Vec<&str> = line.split('\t').collect();
        let (p, text) = (unbase64(f[1]), unbase64(f[2]));
        checked += 1;
        let ran = cache::with_entry(&p, |e| -> Result<Vec<String>, Exc> {
            let Ready::Search(info, eng) = e.ready() else {
                return Ok(vec!["pattern not usable".to_owned()]);
            };
            let hay = Hay::new(&text).expect("allocates");
            let mut meter = Meter::new(Limits::default());
            let limit = |_| Exc::fault(&b"limit"[..]);
            let mut s = Searcher {
                hay: &hay,
                re: &eng.re,
                cache: &mut eng.cache,
                meter: &mut meter,
                program: info.program,
                after_program: info.after_program,
                limit: &limit,
            };
            let mut diffs = Vec::new();
            let matched = s.hay.find(s.re, s.cache, 0).is_some();
            if matched != (f[3] == "1") {
                diffs.push(format!("match {matched}"));
            }
            let find = s.next(0)?;
            let want_find = (f[4] != "-").then(|| spans(f[4])[0]);
            if find != want_find {
                diffs.push(format!("find {find:?} want {want_find:?}"));
            }
            let all: Vec<(usize, usize)> = s
                .find_all()?
                .into_iter()
                .map(|(a, b)| (a as usize, b as usize))
                .collect();
            if all != spans(f[5]) {
                diffs.push(format!("all {all:?} want {:?}", spans(f[5])));
            }
            let split: Vec<Vec<u8>> = s
                .split(p.is_empty())?
                .into_iter()
                .map(|(a, b)| text[a as usize..b as usize].to_vec())
                .collect();
            let want_split: Vec<Vec<u8>> = if f[6] == "-" {
                Vec::new()
            } else {
                f[6].split(',').map(unbase64).collect()
            };
            if split != want_split {
                diffs.push(format!("split {} want {}", split.len(), want_split.len()));
            }
            for pair in f[7].split(',') {
                let (r, out) = pair.split_once(':').expect("a pair");
                let (r, want) = (unbase64(r), unbase64(out));
                let got = s.replace(&text, &r, &info.names)?;
                if got != want {
                    diffs.push(format!(
                        "replace {} gives {} want {}",
                        show(&r),
                        show(&got),
                        show(&want)
                    ));
                }
            }
            Ok(diffs)
        });
        match ran {
            Ok(diffs) if diffs.is_empty() => {}
            Ok(diffs) => bad.push(format!(
                "{} on {}: {}",
                show(&p),
                show(&text),
                diffs.join("; ")
            )),
            Err(e) => bad.push(format!("{} on {}: {e:?}", show(&p), show(&text))),
        }
    }
    assert!(checked > 1000, "{checked} search facts");
    assert!(
        bad.is_empty(),
        "{} of {checked} differ:\n{}",
        bad.len(),
        bad[..bad.len().min(25)].join("\n")
    );
}

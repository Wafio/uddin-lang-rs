//! The patterns a thread has used lately, parsed and, once a search needs it, compiled. A pattern is looked up by
//! its bytes; the cache holds sixteen and, when full, drops the one used longest ago. A pattern that does not
//! parse is kept too, with its error, so a script that tries one over and over parses it once.
//!
//! The cache is an optimisation only: every call is still charged for the parse and the compile as if it had
//! done both.

use std::cell::RefCell;
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;

use regex_automata::meta;

use super::lower;
use super::shape;
use super::syntax::{self, Node, SynError};

/// How many patterns a thread keeps.
const ENTRIES: usize = 16;
/// The limit on the program of a pattern, which the estimate is taken against.
pub(super) const PROGRAM_LIMIT: i64 = 1024;

/// What the parser and the sizes say about a valid pattern.
pub(super) struct Info {
    pub root: Node,
    pub names: Vec<Option<Box<str>>>,
    /// The quick estimate of the program, capped at one past the limit.
    pub estimate: i64,
    /// The instructions of the program the reference compiles.
    pub program: u64,
    /// The instructions of the program of the search that resumes in the middle of the text.
    pub after_program: u64,
}

pub(super) struct Engine {
    pub re: meta::Regex,
    pub cache: meta::Cache,
}

pub(super) struct Entry {
    hash: u64,
    key: Box<[u8]>,
    used: u64,
    pub parsed: Result<Info, SynError>,
    /// `None` until a search asks for it; `Some(None)` when the compiled form would be too large.
    engine: Option<Option<Engine>>,
}

/// What a pattern is ready for.
pub(super) enum Ready<'a> {
    Syntax(&'a SynError),
    /// The compiled form would be too large.
    TooLarge,
    Search(&'a Info, &'a mut Engine),
}

impl Entry {
    /// The parse and the matcher of the pattern, which is built on first use.
    pub(super) fn ready(&mut self) -> Ready<'_> {
        let Entry { parsed, engine, .. } = self;
        let info = match parsed {
            Err(e) => return Ready::Syntax(e),
            Ok(info) => info,
        };
        let built = engine.get_or_insert_with(|| {
            lower::build(&info.root).map(|re| {
                let cache = re.create_cache();
                Engine { re, cache }
            })
        });
        match built {
            Some(eng) => Ready::Search(info, eng),
            None => Ready::TooLarge,
        }
    }
}

struct Entries {
    list: Vec<Entry>,
    tick: u64,
    hasher: RandomState,
}

thread_local! {
    static ENTRIES_OF_THREAD: RefCell<Entries> = RefCell::new(Entries {
        list: Vec::with_capacity(ENTRIES),
        tick: 0,
        hasher: RandomState::new(),
    });
}

fn parse_entry(pattern: &[u8], hash: u64, used: u64) -> Entry {
    let parsed = syntax::parse(pattern).map(|p| Info {
        estimate: shape::estimate(&p.root, PROGRAM_LIMIT),
        program: shape::program_len(&p.root),
        after_program: shape::after_program_len(&p.root),
        names: p.names,
        root: p.root,
    });
    Entry {
        hash,
        key: pattern.into(),
        used,
        parsed,
        engine: None,
    }
}

/// Runs `f` on the entry of `pattern`, parsing it first when the thread has not used it lately.
pub(super) fn with_entry<R>(pattern: &[u8], f: impl FnOnce(&mut Entry) -> R) -> R {
    ENTRIES_OF_THREAD.with(|cell| {
        let mut entries = cell.borrow_mut();
        entries.tick += 1;
        let tick = entries.tick;
        let hash = entries.hasher.hash_one(pattern);
        let at = entries
            .list
            .iter()
            .position(|e| e.hash == hash && *e.key == *pattern);
        let at = match at {
            Some(i) => {
                entries.list[i].used = tick;
                i
            }
            None => {
                let fresh = parse_entry(pattern, hash, tick);
                if entries.list.len() < ENTRIES {
                    entries.list.push(fresh);
                    entries.list.len() - 1
                } else {
                    let oldest = (0..entries.list.len())
                        .min_by_key(|&i| entries.list[i].used)
                        .unwrap_or(0);
                    entries.list[oldest] = fresh;
                    oldest
                }
            }
        };
        f(&mut entries.list[at])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_sixteen_and_drops_the_least_recent() {
        let count = || ENTRIES_OF_THREAD.with(|c| c.borrow().list.len());
        for i in 0..40 {
            with_entry(format!("a{i}").as_bytes(), |e| assert!(e.parsed.is_ok()));
        }
        assert_eq!(count(), ENTRIES);
        let hit = |p: &str| {
            ENTRIES_OF_THREAD.with(|c| c.borrow().list.iter().any(|e| &*e.key == p.as_bytes()))
        };
        assert!(hit("a39") && hit("a24") && !hit("a23"));
        with_entry(b"a24", |_| ());
        with_entry(b"b", |_| ());
        assert!(hit("a24") && !hit("a25"));
    }

    #[test]
    fn remembers_a_pattern_that_does_not_parse() {
        with_entry(b"(", |e| assert!(e.parsed.is_err()));
        with_entry(b"(", |e| assert!(matches!(e.ready(), Ready::Syntax(_))));
    }
}

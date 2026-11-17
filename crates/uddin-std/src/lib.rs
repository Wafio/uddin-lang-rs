//! The builtins every script may call. One file per group holds the registration table of the group, one line
//! per builtin; `builtins()` chains the tables.
//!
//! To add a builtin, write its function and its line in the file of its group and touch no other file of this
//! crate: the line says the argument count, whether the builtin is on the fast path, and the hooks the machine
//! runs around the call (see [`hooks`]). A test refuses a line whose hooks differ from the reference table.

#![forbid(unsafe_code)]

mod basics;
mod collections;
mod convert;
mod dates;
pub mod hooks;
mod json;
mod lists;
mod math_float;
mod math_stats;
mod paths;
mod regexp;
mod sort;
mod strings;

use std::sync::OnceLock;

use uddin_vm::BuiltinDef;

pub use strings::{lower_text, trim_space, upper_text};

/// The tables of the groups by file, in the order their builtins are indexed.
const GROUPS: [(&str, &[BuiltinDef]); 12] = [
    ("basics", basics::TABLE),
    ("collections", collections::TABLE),
    ("convert", convert::TABLE),
    ("dates", dates::TABLE),
    ("json", json::TABLE),
    ("lists", lists::TABLE),
    ("math_float", math_float::TABLE),
    ("math_stats", math_stats::TABLE),
    ("paths", paths::TABLE),
    ("regexp", regexp::TABLE),
    ("sort", sort::TABLE),
    ("strings", strings::TABLE),
];

/// The builtins this build carries, in the order their `Value::Builtin` indexes.
pub fn builtins() -> &'static [BuiltinDef] {
    static ALL: OnceLock<Vec<BuiltinDef>> = OnceLock::new();
    ALL.get_or_init(|| {
        GROUPS
            .iter()
            .flat_map(|(_, table)| table.iter().copied())
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn no_name_is_registered_twice() {
        let mut seen = HashSet::new();
        for def in builtins() {
            assert!(seen.insert(def.name), "{} is registered twice", def.name);
        }
    }

    #[test]
    fn every_builtin_has_the_hooks_the_reference_gives_it() {
        for (file, table) in GROUPS {
            for def in table {
                if let Err(why) = hooks::check(file, def) {
                    panic!("{why}");
                }
            }
        }
    }
}

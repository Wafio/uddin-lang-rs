//! Which builtins of the profile have hooks, and which: the reference table. A builtin that has no row has none.
//!
//! The machine does the following around the call of a builtin, in this order (`BuiltinDef` says how each is
//! set): walk the arguments for depth and cycles when the builtin is `deep`; bound the text the call builds
//! (`stringify`); run `guard`, which refuses arguments whose result would be too large; run `guard_regex`, which
//! bounds a pattern and charges its parse and compile; run `reserve`, which charges the worst case of what the
//! call allocates; call; cut an error value that is too long; and settle the memory, where `fresh` says how much
//! of the result the call made itself. A group writes the hooks of its builtins in its own file and puts each in
//! the line of the builtin; the shared helpers are `Rt::size_check`, `Rt::reserve`, `uddin_vm::checked_product`,
//! `uddin_vm::sum_estimate` and the cost constants of `uddin_vm`.
//!
//! The rows below are what a registered builtin must match; a test compares every registration with them. Of the
//! builtins that carry hooks in the reference, `repeat`, `str_pad`, `input` and `xml_stringify` are not in the
//! profile and have no row.

use uddin_vm::{BuiltinDef, Fresh, Stringify};

/// The hooks one builtin must have, and the file of its group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub name: &'static str,
    pub file: &'static str,
    pub deep: bool,
    pub stringify: Stringify,
    pub fresh: Fresh,
    pub guard: bool,
    pub guard_regex: bool,
    pub reserve: bool,
    pub calls: bool,
}

const fn row(name: &'static str, file: &'static str) -> Row {
    Row {
        name,
        file,
        deep: false,
        stringify: Stringify::No,
        fresh: Fresh::Shallow,
        guard: false,
        guard_regex: false,
        reserve: false,
        calls: false,
    }
}

impl Row {
    const fn deep(mut self) -> Row {
        self.deep = true;
        self
    }

    const fn text(mut self) -> Row {
        self.stringify = Stringify::Text;
        self
    }

    const fn json(mut self) -> Row {
        self.stringify = Stringify::Json;
        self
    }

    const fn elements(mut self) -> Row {
        self.fresh = Fresh::Elements;
        self
    }

    const fn tree(mut self) -> Row {
        self.fresh = Fresh::Tree;
        self
    }

    const fn guard(mut self) -> Row {
        self.guard = true;
        self
    }

    const fn guard_regex(mut self) -> Row {
        self.guard_regex = true;
        self
    }

    const fn reserve(mut self) -> Row {
        self.reserve = true;
        self
    }

    const fn calls(mut self) -> Row {
        self.calls = true;
        self
    }
}

/// Every builtin of the profile that has a hook, with the hooks.
pub const REFERENCE: &[Row] = &[
    // They walk nested arguments (`deep`), and the first five build text from the whole value.
    row("str", "basics").deep().text(),
    row("print", "basics").deep().text(),
    row("println", "convert").deep().text(),
    row("json_stringify", "json").deep().json(),
    row("mode", "math_stats").deep().text(),
    row("sort", "sort").deep().calls(),
    row("index_of", "lists").deep(),
    row("last_index_of", "lists").deep(),
    row("contains", "strings").deep(),
    row("find", "strings").deep(),
    row("set_remove", "collections").deep(),
    row("max", "math_stats").deep(),
    row("min", "math_stats").deep(),
    // They call functions of the script.
    row("map", "lists").calls(),
    row("filter", "lists").calls(),
    row("reduce", "lists").calls(),
    // Their result, or what they build on the way to it, is a function of the arguments: they refuse too large
    // a one, reserve the worst case and, for the lists they build from nothing, charge the elements too.
    row("range", "lists").guard().reserve().elements(),
    row("split", "strings").reserve().elements(),
    row("join", "strings").deep().guard(),
    row("replace", "strings").guard(),
    row("json_parse", "json").reserve().tree(),
    // A pattern is bounded and its compile charged before anything compiles it; the ones that search past
    // the first match compile a second program, which `guard_regex` charges.
    row("is_regex_match", "regexp").guard_regex(),
    row("regex_match", "regexp").guard_regex(),
    row("regex_find", "regexp").guard_regex(),
    row("regex_find_all", "regexp")
        .guard_regex()
        .reserve()
        .elements(),
    row("regex_split", "regexp")
        .guard_regex()
        .reserve()
        .elements(),
    row("regex_replace", "regexp")
        .guard()
        .guard_regex()
        .reserve(),
];

/// The row of `name`, or the row of a builtin without hooks.
fn row_of(name: &'static str, file: &'static str) -> Row {
    REFERENCE
        .iter()
        .copied()
        .find(|r| r.name == name)
        .unwrap_or(row(name, file))
}

/// Compares the hooks `def` has, registered in `file`, with the reference. The error names the first thing that
/// differs.
pub fn check(file: &'static str, def: &BuiltinDef) -> Result<(), String> {
    let want = row_of(def.name, file);
    let got = Row {
        name: def.name,
        file,
        deep: def.deep,
        stringify: def.stringify,
        fresh: def.fresh,
        guard: def.guard.is_some(),
        guard_regex: def.guard_regex.is_some(),
        reserve: def.reserve.is_some(),
        calls: def.calls,
    };
    if got == want {
        return Ok(());
    }
    Err(format!(
        "{} is registered in {file} with {got:?} and the reference has {want:?}",
        def.name
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn the_rows_name_each_builtin_once() {
        let mut seen = HashSet::new();
        for r in REFERENCE {
            assert!(seen.insert(r.name), "{} has two rows", r.name);
        }
        assert_eq!(REFERENCE.len(), 27);
    }
}

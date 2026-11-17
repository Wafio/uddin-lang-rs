//! The names a generated script can use: builtins of the Go interpreter that the profile allows, builtins it
//! refuses, and ordinary identifiers.

use std::collections::HashSet;

pub struct Names {
    pub allowed: Vec<String>,
    pub disallowed: Vec<String>,
}

pub const ORDINARY: &[&str] = &[
    "x",
    "y",
    "z",
    "i",
    "j",
    "n",
    "a",
    "b",
    "c",
    "e",
    "E",
    "s",
    "foo",
    "bar",
    "baz",
    "_",
    "_t",
    "__x",
    "tmp",
    "res",
    "val",
    "key",
    "items",
    "count",
    "data",
    "a1",
    "b_2",
    "x_y",
    "Name",
    "CamelCase",
    "main",
    "self",
    "PI",
    "TAU",
    "waf",
    "wafblock",
    "end_",
    "iff",
    "If",
    "AND",
    "True",
    "NULL",
    "none",
    "elsee",
    "then_",
    "e1",
    "en",
    "ELSE",
    "fn",
    "memo_",
    "Fun",
    "r",
    "q",
    "rest",
    "args",
    "xs",
    "ys",
    "m",
    "lst",
];

/// Names that read like keywords but are not, and one that is.
pub const NEAR_KEYWORDS: &[&str] = &[
    "End",
    "iff",
    "elsee",
    "In",
    "NULL",
    "True",
    "FALSE",
    "Not",
    "andy",
    "or_",
    "xor1",
    "_end",
    "ends",
    "fun1",
    "catch_",
    "Try",
    "while1",
    "for_",
    "returns",
    "import_",
    "memo1",
    "then2",
    "elifs",
    "break_",
    "continue1",
];

impl Names {
    pub fn load() -> Names {
        let profile: serde_json::Value =
            serde_json::from_str(include_str!("../../../profile/sandbox-profile.json"))
                .expect("the pinned profile is valid JSON");
        let listed: HashSet<&str> = profile["builtins"]
            .as_array()
            .expect("builtins is a list")
            .iter()
            .map(|v| v.as_str().expect("a builtin name"))
            .collect();
        let mut allowed = Vec::new();
        let mut disallowed = Vec::new();
        for name in include_str!("../builtins.txt")
            .lines()
            .filter(|l| !l.is_empty())
        {
            if listed.contains(name) {
                allowed.push(name.to_owned());
            } else {
                disallowed.push(name.to_owned());
            }
        }
        Names {
            allowed,
            disallowed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_pools_split_the_builtin_table() {
        let n = Names::load();
        assert_eq!(n.allowed.len(), 133);
        assert_eq!(n.allowed.len() + n.disallowed.len(), 219);
        assert!(n.disallowed.iter().any(|s| s == "sleep"));
    }
}

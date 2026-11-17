//! The sandbox profile: which builtins a script may use and how large it may be.

use std::collections::HashSet;
use std::fmt;
use std::time::Duration;

use serde_json::Value;
use uddin_vm::Limits;

/// A copy of the file the Wafio control plane exports, embedded at build time (`profile/SOURCE` says which
/// commit it came from).
const EMBEDDED: &str = include_str!("../../../profile/sandbox-profile.json");

pub struct Profile {
    builtins: HashSet<String>,
    max_script_bytes: usize,
    limits: Limits,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileError(String);

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid sandbox profile: {}", self.0)
    }
}

impl std::error::Error for ProfileError {}

impl Profile {
    /// The profile in `profile/sandbox-profile.json`. A test keeps the embedded file parseable.
    pub fn embedded() -> Profile {
        Profile::from_json(EMBEDDED).expect("the embedded sandbox profile is valid")
    }

    /// Reads `builtins` (a list of names), `limits.max_script_bytes` and the run limits that are present;
    /// a run limit the file leaves out keeps the production value.
    pub fn from_json(text: &str) -> Result<Profile, ProfileError> {
        let err = |what: &str| ProfileError(what.to_owned());
        let doc: Value = serde_json::from_str(text).map_err(|e| ProfileError(e.to_string()))?;
        let builtins = doc
            .get("builtins")
            .and_then(Value::as_array)
            .ok_or_else(|| err("builtins is not a list"))?
            .iter()
            .map(|name| {
                name.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| err("a builtin name is not a string"))
            })
            .collect::<Result<HashSet<_>, _>>()?;
        let max_script_bytes = doc
            .pointer("/limits/max_script_bytes")
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| err("limits.max_script_bytes is not a size"))?;
        let mut limits = Limits::default();
        let number = |key: &str| {
            doc.pointer(&format!("/limits/{key}"))
                .and_then(Value::as_u64)
        };
        if let Some(n) = number("max_ops") {
            limits.max_ops = n;
        }
        if let Some(n) = number("max_call_depth") {
            limits.max_call_depth =
                u32::try_from(n).map_err(|_| err("limits.max_call_depth is too large"))?;
        }
        if let Some(n) = number("max_nesting") {
            limits.max_nesting =
                u32::try_from(n).map_err(|_| err("limits.max_nesting is too large"))?;
        }
        if let Some(n) = number("max_value_len") {
            limits.max_value_len =
                usize::try_from(n).map_err(|_| err("limits.max_value_len is too large"))?;
        }
        if let Some(n) = number("max_alloc_bytes") {
            limits.max_alloc_bytes =
                usize::try_from(n).map_err(|_| err("limits.max_alloc_bytes is too large"))?;
        }
        if let Some(n) = number("max_regex_program") {
            limits.max_regex_program =
                usize::try_from(n).map_err(|_| err("limits.max_regex_program is too large"))?;
        }
        if let Some(n) = number("max_regex_work") {
            limits.max_regex_work =
                usize::try_from(n).map_err(|_| err("limits.max_regex_work is too large"))?;
        }
        if let Some(n) = number("per_script_timeout_ms") {
            limits.deadline = (n > 0).then(|| Duration::from_millis(n));
        }
        Ok(Profile {
            builtins,
            max_script_bytes,
            limits,
        })
    }

    pub fn allows(&self, name: &str) -> bool {
        self.builtins.contains(name)
    }

    pub fn max_script_bytes(&self) -> usize {
        self.max_script_bytes
    }

    /// The limits a script runs under: the file's `max_ops` to `per_script_timeout_ms`.
    pub fn limits(&self) -> Limits {
        self.limits.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_fields_it_needs() {
        let p = Profile::from_json(
            r#"{"builtins": ["a", "b"], "limits": {"max_script_bytes": 10, "other": 1}}"#,
        )
        .expect("valid");
        assert!(p.allows("a") && p.allows("b") && !p.allows("c"));
        assert_eq!(p.max_script_bytes(), 10);
    }

    #[test]
    fn refuses_a_malformed_profile() {
        for text in [
            "",
            "[]",
            r#"{"limits": {"max_script_bytes": 1}}"#,
            r#"{"builtins": [1], "limits": {"max_script_bytes": 1}}"#,
            r#"{"builtins": []}"#,
            r#"{"builtins": [], "limits": {"max_script_bytes": -1}}"#,
        ] {
            assert!(Profile::from_json(text).is_err(), "{text}");
        }
    }
}

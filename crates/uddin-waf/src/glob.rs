//! Shell patterns for `waf_path_match`: `*` for any run of characters but `/`, `?` for one, `[a-z]` and `[^a-z]`
//! for a class, and a backslash to take the next character as it is. A pattern that is not well formed matches
//! nothing, even when the part of it that was read matched.

use uddin_vm::decode_rune;

const SEPARATOR: u8 = b'/';

/// The steps a match may take before it is given up: a pattern of many stars against a long name takes the
/// product of their lengths, and a call cannot be interrupted once it runs.
const STEPS: u64 = 4_000_000;

enum Stop {
    Bad,
    Spent,
}

/// Whether `name` matches `pattern`, or `None` when the match took more than [`STEPS`] steps.
pub fn matches(pattern: &[u8], name: &[u8]) -> Option<bool> {
    let mut left = STEPS;
    match run(pattern, name, &mut left) {
        Ok(hit) => Some(hit),
        Err(Stop::Bad) => Some(false),
        Err(Stop::Spent) => None,
    }
}

fn spend(left: &mut u64, n: usize) -> Result<(), Stop> {
    *left = left.checked_sub(n as u64).ok_or(Stop::Spent)?;
    Ok(())
}

fn run(mut pattern: &[u8], mut name: &[u8], left: &mut u64) -> Result<bool, Stop> {
    'pattern: while !pattern.is_empty() {
        let (star, chunk, rest) = scan_chunk(pattern);
        pattern = rest;
        if star && chunk.is_empty() {
            return Ok(!name.contains(&SEPARATOR));
        }
        let first = match_chunk(chunk, name, left);
        if let Ok(Some(t)) = first
            && (t.is_empty() || !pattern.is_empty())
        {
            name = t;
            continue;
        }
        first?;
        if star {
            let mut i = 0;
            while i < name.len() && name[i] != SEPARATOR {
                match match_chunk(chunk, &name[i + 1..], left) {
                    Ok(Some(t)) => {
                        if pattern.is_empty() && !t.is_empty() {
                            i += 1;
                            continue;
                        }
                        name = t;
                        continue 'pattern;
                    }
                    Ok(None) => {}
                    Err(e) => return Err(e),
                }
                i += 1;
            }
        }
        return Ok(false);
    }
    Ok(name.is_empty())
}

/// The leading stars of `pattern`, the text up to the next star outside a class, and what follows.
fn scan_chunk(mut pattern: &[u8]) -> (bool, &[u8], &[u8]) {
    let mut star = false;
    while pattern.first() == Some(&b'*') {
        pattern = &pattern[1..];
        star = true;
    }
    let mut in_range = false;
    let mut i = 0;
    while i < pattern.len() {
        match pattern[i] {
            b'\\' => {
                if i + 1 < pattern.len() {
                    i += 1;
                }
            }
            b'[' => in_range = true,
            b']' => in_range = false,
            b'*' if !in_range => return (star, &pattern[..i], &pattern[i..]),
            _ => {}
        }
        i += 1;
    }
    (star, pattern, &[])
}

/// What is left of `s` after `chunk` matched it, or `None` when it did not. The whole chunk is read even when the
/// name stopped matching early, so a bad pattern is found.
fn match_chunk<'a>(
    mut chunk: &[u8],
    mut s: &'a [u8],
    left: &mut u64,
) -> Result<Option<&'a [u8]>, Stop> {
    spend(left, chunk.len() + 1)?;
    let mut failed = false;
    while !chunk.is_empty() {
        failed = failed || s.is_empty();
        match chunk[0] {
            b'[' => {
                let mut r = 0u32;
                if !failed {
                    let (c, n) = decode_rune(s);
                    r = c;
                    s = &s[n..];
                }
                chunk = &chunk[1..];
                let negated = chunk.first() == Some(&b'^');
                if negated {
                    chunk = &chunk[1..];
                }
                let mut hit = false;
                let mut ranges = 0;
                loop {
                    if chunk.first() == Some(&b']') && ranges > 0 {
                        chunk = &chunk[1..];
                        break;
                    }
                    let (lo, rest) = get_esc(chunk)?;
                    chunk = rest;
                    let mut hi = lo;
                    if chunk[0] == b'-' {
                        let (h, rest) = get_esc(&chunk[1..])?;
                        hi = h;
                        chunk = rest;
                    }
                    hit = hit || (lo <= r && r <= hi);
                    ranges += 1;
                }
                failed = failed || hit == negated;
            }
            b'?' => {
                if !failed {
                    failed = s[0] == SEPARATOR;
                    s = &s[decode_rune(s).1..];
                }
                chunk = &chunk[1..];
            }
            c => {
                if c == b'\\' {
                    chunk = &chunk[1..];
                    if chunk.is_empty() {
                        return Err(Stop::Bad);
                    }
                }
                if !failed {
                    failed = chunk[0] != s[0];
                    s = &s[1..];
                }
                chunk = &chunk[1..];
            }
        }
    }
    Ok((!failed).then_some(s))
}

/// One character of a class, after an optional backslash, and what follows it. A class that ends there is bad.
fn get_esc(mut chunk: &[u8]) -> Result<(u32, &[u8]), Stop> {
    if chunk.is_empty() || chunk[0] == b'-' || chunk[0] == b']' {
        return Err(Stop::Bad);
    }
    if chunk[0] == b'\\' {
        chunk = &chunk[1..];
        if chunk.is_empty() {
            return Err(Stop::Bad);
        }
    }
    let (r, n) = decode_rune(chunk);
    if r == 0xFFFD && n == 1 {
        return Err(Stop::Bad);
    }
    let rest = &chunk[n..];
    if rest.is_empty() {
        return Err(Stop::Bad);
    }
    Ok((r, rest))
}

#[cfg(test)]
mod tests {
    use super::matches as run;

    fn matches(p: &[u8], n: &[u8]) -> bool {
        run(p, n).expect("a match within the budget")
    }

    #[test]
    fn matches_like_a_shell_pattern() {
        let yes = [
            ("abc", "abc"),
            ("*", "abc"),
            ("a*c", "abc"),
            ("a*", "a"),
            ("a?c", "abc"),
            ("a[bc]d", "acd"),
            ("a[^bc]d", "aed"),
            ("a[b-d]e", "ace"),
            ("/admin/*", "/admin/x"),
            ("*.png", "logo.png"),
            ("a\\*b", "a*b"),
            ("[\\]]", "]"),
            ("*a*b*c", "xaxbxc"),
        ];
        for (p, n) in yes {
            assert!(matches(p.as_bytes(), n.as_bytes()), "{p} {n}");
        }
        let no = [
            ("a*c", "abd"),
            ("*", "a/b"),
            ("/admin/*", "/admin/x/y"),
            ("a?c", "a/c"),
            ("a[", "a"),
            ("[]a]", "a"),
            ("a\\", "a"),
            ("[a-", "a"),
            ("x[", "y"),
            ("", "a"),
        ];
        for (p, n) in no {
            assert!(!matches(p.as_bytes(), n.as_bytes()), "{p} {n}");
        }
        assert!(matches(b"", b""));
    }

    #[test]
    fn gives_up_on_a_pattern_that_would_take_too_long() {
        let pattern = format!("*{}b", "a".repeat(5_000));
        let name = "a".repeat(8_000);
        assert_eq!(run(pattern.as_bytes(), name.as_bytes()), None);
        assert_eq!(run(b"*a*a*a", b"aaa"), Some(true));
    }
}

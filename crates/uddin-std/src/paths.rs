//! Paths: path_basename path_dirname path_ext path_join.
//!
//! Slash-separated paths, cleaned the way the Go library does on Linux. A bad argument is an error value.

use uddin_vm::{BuiltinDef, Count, Exc, Rt, Str, Value};

pub(crate) const TABLE: &[BuiltinDef] = &[
    BuiltinDef::new("path_basename", path_basename, -1),
    BuiltinDef::new("path_dirname", path_dirname, -1),
    BuiltinDef::new("path_ext", path_ext, -1),
    BuiltinDef::new("path_join", path_join, -1),
];

/// A part of `s`: the same bytes, no copy.
fn part(s: Str, from: usize, to: usize) -> Str {
    if from == to {
        return Str::EMPTY;
    }
    Str {
        off: s.off + from as u32,
        len: (to - from) as u32,
    }
}

/// The single path argument of a builtin that takes one: its string, or the error value to return.
fn path_arg(rt: &mut Rt<'_>, args: &[Value], name: &str) -> Result<Result<Str, Value>, Exc> {
    let [arg] = args else {
        return Err(rt.arity_error(name, 1, args.len()));
    };
    match *arg {
        Value::Str(s) => Ok(Ok(s)),
        _ => rt
            .error_value(format!("{name}: path must be a string").as_bytes())
            .map(Err),
    }
}

fn path_basename(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let s = match path_arg(rt, args, "path_basename")? {
        Ok(s) => s,
        Err(error) => return Ok(error),
    };
    let bytes = rt.heap.str_bytes(s);
    if bytes.is_empty() {
        return Ok(Value::Str(Str::byte(b'.')));
    }
    let end = bytes.iter().rposition(|&b| b != b'/').map_or(0, |i| i + 1);
    if end == 0 {
        return Ok(Value::Str(Str::byte(b'/')));
    }
    let start = bytes[..end]
        .iter()
        .rposition(|&b| b == b'/')
        .map_or(0, |i| i + 1);
    Ok(Value::Str(part(s, start, end)))
}

fn path_ext(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let s = match path_arg(rt, args, "path_ext")? {
        Ok(s) => s,
        Err(error) => return Ok(error),
    };
    let bytes = rt.heap.str_bytes(s);
    for i in (0..bytes.len()).rev() {
        match bytes[i] {
            b'/' => break,
            b'.' => return Ok(Value::Str(part(s, i, bytes.len()))),
            _ => {}
        }
    }
    Ok(Value::Str(Str::EMPTY))
}

fn path_dirname(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let s = match path_arg(rt, args, "path_dirname")? {
        Ok(s) => s,
        Err(error) => return Ok(error),
    };
    rt.work(Count::Free, u64::from(s.len))?;
    let bytes = rt.heap.str_bytes(s);
    let dir = bytes.iter().rposition(|&b| b == b'/').map_or(0, |i| i + 1);
    let mut out = std::mem::take(&mut *rt.scratch);
    let cleaned = clean(&bytes[..dir], &mut out);
    let made = match cleaned {
        Cleaned::Prefix(w) => Ok(part(s, 0, w)),
        Cleaned::Dot => Ok(Str::byte(b'.')),
        Cleaned::Buffer(w) => rt.heap.new_str(&out[..w]).map_err(rt.oom()),
    };
    *rt.scratch = out;
    Ok(Value::Str(made?))
}

fn path_join(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    if args.is_empty() {
        return rt.error_value(b"path_join: expected at least 1 argument");
    }
    let mut parts = Vec::new();
    for &arg in args {
        let Value::Str(s) = arg else {
            return rt.error_value(b"path_join: all arguments must be strings");
        };
        parts.push(s);
    }
    // The first element that is not empty and all after it, joined by a slash and cleaned. One element left is
    // cleaned in place, so the result can be a part of that argument.
    let Some(first) = parts.iter().position(|s| s.len > 0) else {
        return Ok(Value::Str(Str::EMPTY));
    };
    let parts = &parts[first..];
    let total: u64 = parts.iter().map(|s| u64::from(s.len) + 1).sum();
    rt.work(Count::Free, total)?;
    if let [only] = parts {
        let mut out = std::mem::take(&mut *rt.scratch);
        let cleaned = clean(rt.heap.str_bytes(*only), &mut out);
        let made = match cleaned {
            Cleaned::Prefix(w) => Ok(part(*only, 0, w)),
            Cleaned::Dot => Ok(Str::byte(b'.')),
            Cleaned::Buffer(w) => rt.heap.new_str(&out[..w]).map_err(rt.oom()),
        };
        *rt.scratch = out;
        return Ok(Value::Str(made?));
    }
    let mut joined = Vec::new();
    joined
        .try_reserve(total as usize)
        .map_err(|_| Exc::limit(uddin_vm::LimitReason::Memory, rt.pos))?;
    for (i, s) in parts.iter().enumerate() {
        if i > 0 {
            joined.push(b'/');
        }
        joined.extend_from_slice(rt.heap.str_bytes(*s));
    }
    let mut out = std::mem::take(&mut *rt.scratch);
    let made = match clean(&joined, &mut out) {
        Cleaned::Prefix(w) => rt.heap.new_str(&joined[..w]).map_err(rt.oom()),
        Cleaned::Dot => Ok(Str::byte(b'.')),
        Cleaned::Buffer(w) => rt.heap.new_str(&out[..w]).map_err(rt.oom()),
    };
    *rt.scratch = out;
    Ok(Value::Str(made?))
}

/// Where the cleaned path is.
enum Cleaned {
    /// The first bytes of the path itself.
    Prefix(usize),
    /// The path was empty.
    Dot,
    /// The first bytes of the buffer.
    Buffer(usize),
}

/// The shortest path that names the same place: no empty or `.` elements, no `..` that a name before it can
/// take back, and no slash at the end unless the path is the root. The output stays the path itself for as long
/// as it matches the path byte for byte, and moves to `out` at the first byte that differs.
fn clean(path: &[u8], out: &mut Vec<u8>) -> Cleaned {
    let n = path.len();
    if n == 0 {
        return Cleaned::Dot;
    }
    let mut buf = Lazy {
        path,
        out,
        copied: false,
        w: 0,
    };
    let rooted = path[0] == b'/';
    let (mut r, mut dotdot) = (0, 0);
    if rooted {
        buf.push(b'/');
        (r, dotdot) = (1, 1);
    }
    while r < n {
        if path[r] == b'/' || path[r] == b'.' && (r + 1 == n || path[r + 1] == b'/') {
            // An empty element, or a `.` element.
            r += 1;
        } else if path[r] == b'.' && path[r + 1] == b'.' && (r + 2 == n || path[r + 2] == b'/') {
            r += 2;
            if buf.w > dotdot {
                buf.w -= 1;
                while buf.w > dotdot && buf.at(buf.w) != b'/' {
                    buf.w -= 1;
                }
            } else if !rooted {
                if buf.w > 0 {
                    buf.push(b'/');
                }
                buf.push(b'.');
                buf.push(b'.');
                dotdot = buf.w;
            }
        } else {
            if rooted && buf.w != 1 || !rooted && buf.w != 0 {
                buf.push(b'/');
            }
            while r < n && path[r] != b'/' {
                buf.push(path[r]);
                r += 1;
            }
        }
    }
    if buf.w == 0 {
        buf.push(b'.');
    }
    if buf.copied {
        Cleaned::Buffer(buf.w)
    } else {
        Cleaned::Prefix(buf.w)
    }
}

/// The output of `clean`, which is not copied until it differs from the path.
struct Lazy<'a> {
    path: &'a [u8],
    out: &'a mut Vec<u8>,
    copied: bool,
    w: usize,
}

impl Lazy<'_> {
    fn at(&self, i: usize) -> u8 {
        if self.copied {
            self.out[i]
        } else {
            self.path[i]
        }
    }

    fn push(&mut self, c: u8) {
        if !self.copied {
            if self.w < self.path.len() && self.path[self.w] == c {
                self.w += 1;
                return;
            }
            self.copied = true;
            self.out.clear();
            self.out.extend_from_slice(&self.path[..self.w]);
        }
        if self.w < self.out.len() {
            self.out[self.w] = c;
        } else {
            self.out.push(c);
        }
        self.w += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cleaned(path: &str) -> String {
        let mut out = Vec::new();
        let bytes = path.as_bytes();
        let made = match clean(bytes, &mut out) {
            Cleaned::Prefix(w) => bytes[..w].to_vec(),
            Cleaned::Dot => b".".to_vec(),
            Cleaned::Buffer(w) => out[..w].to_vec(),
        };
        String::from_utf8(made).expect("ascii")
    }

    #[test]
    fn clean_follows_the_reference_cases() {
        for (path, want) in [
            ("", "."),
            ("abc", "abc"),
            ("abc/def", "abc/def"),
            ("a/b/c", "a/b/c"),
            (".", "."),
            ("..", ".."),
            ("../..", "../.."),
            ("../../abc", "../../abc"),
            ("/abc", "/abc"),
            ("/", "/"),
            ("abc/", "abc"),
            ("abc/def/", "abc/def"),
            ("./", "."),
            ("../", ".."),
            ("/abc/", "/abc"),
            ("abc//def//ghi", "abc/def/ghi"),
            ("//abc", "/abc"),
            ("///abc", "/abc"),
            ("abc//", "abc"),
            ("abc/./def", "abc/def"),
            ("/./abc/def", "/abc/def"),
            ("abc/.", "abc"),
            ("abc/def/ghi/../jkl", "abc/def/jkl"),
            ("abc/def/../ghi/../jkl", "abc/jkl"),
            ("abc/def/..", "abc"),
            ("abc/def/../..", "."),
            ("/abc/def/../..", "/"),
            ("abc/def/../../..", ".."),
            ("/abc/def/../../..", "/"),
            ("abc/def/../../../ghi/jkl/../../../mno", "../../mno"),
            ("abc/./../def", "def"),
            ("abc//./../def", "def"),
            ("abc/../../././../def", "../../def"),
        ] {
            assert_eq!(cleaned(path), want, "{path:?}");
        }
    }
}

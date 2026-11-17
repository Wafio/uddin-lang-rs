//! JSON text to a [`Json`] value, the way a Go decoder that keeps numbers as text reads it: standard JSON, numbers
//! left as written, a name that appears twice keeping its last value, nesting up to 10,000 levels.

use std::collections::HashMap;

use uddin_vm::Json;

/// How deep a document may nest before the reader gives up, as Go's decoder does.
const MAX_DEPTH: usize = 10_000;

enum Open {
    Array(Vec<Json>),
    /// The entries so far, the name whose value is being read, and where each name is among the entries.
    Object(
        Vec<(Vec<u8>, Json)>,
        Option<Vec<u8>>,
        HashMap<Vec<u8>, usize>,
    ),
}

struct Reader<'a> {
    text: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn skip_space(&mut self) {
        while matches!(self.text.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.text.get(self.at).copied()
    }

    fn word(&mut self, word: &[u8]) -> Option<()> {
        self.text[self.at..]
            .starts_with(word)
            .then(|| self.at += word.len())
    }

    fn number(&mut self) -> Option<Json> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        match self.peek()? {
            b'0' => self.at += 1,
            b'1'..=b'9' => self.digits(),
            _ => return None,
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            self.some_digits()?;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            self.some_digits()?;
        }
        let text = std::str::from_utf8(&self.text[start..self.at]).ok()?;
        Some(Json::Number(text.to_owned()))
    }

    fn digits(&mut self) {
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.at += 1;
        }
    }

    fn some_digits(&mut self) -> Option<()> {
        let before = self.at;
        self.digits();
        (self.at > before).then_some(())
    }

    fn hex4(&mut self) -> Option<u32> {
        let digits = self.text.get(self.at..self.at + 4)?;
        let mut n = 0;
        for &d in digits {
            n = n * 16 + char::from(d).to_digit(16)?;
        }
        self.at += 4;
        Some(n)
    }

    /// A string, from its opening quote. An invalid UTF-8 byte and a lone surrogate escape each become U+FFFD.
    fn string(&mut self) -> Option<Vec<u8>> {
        self.at += 1;
        let mut out = Vec::new();
        loop {
            let c = self.peek()?;
            self.at += 1;
            match c {
                b'"' => return Some(out),
                b'\\' => {
                    let e = self.peek()?;
                    self.at += 1;
                    match e {
                        b'"' | b'\\' | b'/' => out.push(e),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let mut code = self.hex4()?;
                            if (0xD800..0xDC00).contains(&code)
                                && self.text[self.at..].starts_with(b"\\u")
                            {
                                let save = self.at;
                                self.at += 2;
                                match self.hex4() {
                                    Some(low) if (0xDC00..0xE000).contains(&low) => {
                                        code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                                    }
                                    _ => self.at = save,
                                }
                            }
                            let c = char::from_u32(code).unwrap_or('\u{FFFD}');
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => return None,
                    }
                }
                0..=0x1F => return None,
                0x20..=0x7F => out.push(c),
                _ => {
                    let rest = &self.text[self.at - 1..];
                    let (r, w) = uddin_vm::decode_rune(rest);
                    if r == 0xFFFD && w == 1 {
                        out.extend_from_slice("\u{FFFD}".as_bytes());
                    } else {
                        out.extend_from_slice(&rest[..w]);
                        self.at += w - 1;
                    }
                }
            }
        }
    }
}

fn add(open: &mut [Open], value: Json) -> Option<Json> {
    match open.last_mut() {
        None => Some(value),
        Some(Open::Array(items)) => {
            items.push(value);
            None
        }
        Some(Open::Object(entries, key, index)) => {
            let k = key.take()?;
            match index.get(&k) {
                Some(&at) => entries[at].1 = value,
                None => {
                    index.insert(k.clone(), entries.len());
                    entries.push((k, value));
                }
            }
            None
        }
    }
}

/// The value `text` holds, or `None` when it is not one JSON document.
pub fn parse_json(text: &[u8]) -> Option<Json> {
    let mut r = Reader { text, at: 0 };
    let mut open: Vec<Open> = Vec::new();
    loop {
        r.skip_space();
        // A value starts here, or the container on top ends.
        let value = match r.peek()? {
            b'[' => {
                r.at += 1;
                if open.len() >= MAX_DEPTH {
                    return None;
                }
                r.skip_space();
                if r.peek() == Some(b']') {
                    r.at += 1;
                    Json::Array(Vec::new())
                } else {
                    open.push(Open::Array(Vec::new()));
                    continue;
                }
            }
            b'{' => {
                r.at += 1;
                if open.len() >= MAX_DEPTH {
                    return None;
                }
                r.skip_space();
                if r.peek() == Some(b'}') {
                    r.at += 1;
                    Json::Object(Vec::new())
                } else {
                    open.push(Open::Object(Vec::new(), None, HashMap::new()));
                    if r.peek() != Some(b'"') {
                        return None;
                    }
                    let name = r.string()?;
                    r.skip_space();
                    if r.peek() != Some(b':') {
                        return None;
                    }
                    r.at += 1;
                    if let Some(Open::Object(_, key, _)) = open.last_mut() {
                        *key = Some(name);
                    }
                    continue;
                }
            }
            b'"' => Json::String(r.string()?),
            b't' => {
                r.word(b"true")?;
                Json::Bool(true)
            }
            b'f' => {
                r.word(b"false")?;
                Json::Bool(false)
            }
            b'n' => {
                r.word(b"null")?;
                Json::Null
            }
            _ => r.number()?,
        };
        // Put the value where it belongs, then close every container the text closes.
        let mut done = add(&mut open, value);
        loop {
            if let Some(v) = done {
                r.skip_space();
                return (r.at == r.text.len()).then_some(v);
            }
            r.skip_space();
            match (r.peek()?, open.last()?) {
                (b',', Open::Array(_)) => {
                    r.at += 1;
                    break;
                }
                (b',', Open::Object(..)) => {
                    r.at += 1;
                    r.skip_space();
                    if r.peek() != Some(b'"') {
                        return None;
                    }
                    let name = r.string()?;
                    r.skip_space();
                    if r.peek() != Some(b':') {
                        return None;
                    }
                    r.at += 1;
                    if let Some(Open::Object(_, key, _)) = open.last_mut() {
                        *key = Some(name);
                    }
                    break;
                }
                (b']', Open::Array(_)) | (b'}', Open::Object(..)) => {
                    r.at += 1;
                    let closed = match open.pop()? {
                        Open::Array(items) => Json::Array(items),
                        Open::Object(entries, ..) => Json::Object(entries),
                    };
                    done = add(&mut open, closed);
                }
                _ => return None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> Json {
        Json::Number(s.to_owned())
    }

    #[test]
    fn reads_a_document_with_numbers_as_written() {
        let got = parse_json(
            br#" {"a": [1, 2.50, -0, 1e400, 1E+2], "b": {"c": null}, "a": "x\u00e9\ud83d\ude00"} "#,
        );
        let want = Json::Object(vec![
            (
                b"a".to_vec(),
                Json::String("x\u{e9}\u{1F600}".as_bytes().to_vec()),
            ),
            (
                b"b".to_vec(),
                Json::Object(vec![(b"c".to_vec(), Json::Null)]),
            ),
        ]);
        assert_eq!(got, Some(want));
        assert_eq!(
            parse_json(b"[1, 2.50, -0, 1e400, 1E+2]"),
            Some(Json::Array(vec![
                n("1"),
                n("2.50"),
                n("-0"),
                n("1e400"),
                n("1E+2")
            ]))
        );
    }

    #[test]
    fn refuses_what_is_not_json() {
        for bad in [
            "",
            "[",
            "[1,]",
            "{\"a\"}",
            "01",
            "1.",
            "-",
            "+1",
            "nul",
            "[1] x",
            "'a'",
            "\"a",
            "{1:2}",
            "[\"\\x\"]",
            "[\"\n\"]",
            "{\"a\":1,}",
        ] {
            assert_eq!(parse_json(bad.as_bytes()), None, "{bad:?}");
        }
    }

    #[test]
    fn an_object_of_many_names_is_read_in_linear_time() {
        let names: Vec<String> = (0..20_000).map(|i| format!("\"k{i}\":1")).collect();
        let text = format!("{{{},\"k5\":2}}", names.join(","));
        let parsed = parse_json(text.as_bytes()).expect("a document");
        let Json::Object(entries) = &parsed else {
            panic!("an object");
        };
        assert_eq!(entries.len(), 20_000);
        assert_eq!(entries[5], (b"k5".to_vec(), n("2")));
    }

    #[test]
    fn nesting_has_a_depth_and_frees_without_recursion() {
        let deep = |n: usize| format!("{}1{}", "[".repeat(n), "]".repeat(n));
        assert!(parse_json(deep(10_000).as_bytes()).is_some());
        assert_eq!(parse_json(deep(10_001).as_bytes()), None);
    }
}

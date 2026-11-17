//! Sources taken from the corpus and damaged at the byte level or at the level of lexemes.

use std::fs;
use std::io;
use std::path::Path;

use crate::b64;
use crate::names::{NEAR_KEYWORDS, Names, ORDINARY};
use crate::rng::Rng;

/// The sources of every `*.cases.jsonl` file of a directory, in file name order.
pub fn load_corpus(dir: &Path) -> io::Result<Vec<Vec<u8>>> {
    let mut files: Vec<_> = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".cases.jsonl"))
        })
        .collect();
    files.sort();
    let mut sources = Vec::new();
    for file in files {
        for line in fs::read_to_string(&file)?.lines().filter(|l| !l.is_empty()) {
            let case: serde_json::Value = serde_json::from_str(line).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{}: {e}", file.display()),
                )
            })?;
            let src = match (case.get("src"), case.get("src_b64")) {
                (Some(s), None) => s.as_str().map(|s| s.as_bytes().to_vec()),
                (None, Some(s)) => s.as_str().and_then(|s| b64::decode(s).ok()),
                _ => None,
            };
            sources.push(src.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{}: bad case", file.display()),
                )
            })?);
        }
    }
    Ok(sources)
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

fn is_name_byte(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphanumeric()
}

/// Cuts a source into lexemes that together cover every byte: runs of white space, comments, strings, numbers,
/// names, operators and single other bytes. It is loose on purpose (an unterminated string runs to the end of
/// the line) and does not need to agree with either tokenizer.
pub fn lexemes(src: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < src.len() {
        let start = i;
        let b = src[i];
        let next = src.get(i + 1).copied();
        if is_space(b) {
            while i < src.len() && is_space(src[i]) {
                i += 1;
            }
        } else if b == b'/' && next == Some(b'/') {
            while i < src.len() && src[i] != b'\n' {
                i += 1;
            }
        } else if b == b'/' && next == Some(b'*') {
            i += 2;
            while i < src.len() && !(src[i] == b'*' && src.get(i + 1) == Some(&b'/')) {
                i += 1;
            }
            i = (i + 2).min(src.len());
        } else if b == b'"' || b == b'\'' {
            i += 1;
            while i < src.len() && src[i] != b && src[i] != b'\n' {
                i += if src[i] == b'\\' { 2 } else { 1 };
            }
            i = (i + 1).min(src.len());
        } else if b == b'`' {
            i += 1;
            while i < src.len() && src[i] != b'`' {
                i += 1;
            }
            i = (i + 1).min(src.len());
        } else if b.is_ascii_digit() {
            while i < src.len() && (is_name_byte(src[i]) || src[i] == b'.') {
                i += 1;
            }
        } else if is_name_byte(b) {
            while i < src.len() && is_name_byte(src[i]) {
                i += 1;
            }
        } else if src[i..].starts_with(b"...") {
            i += 3;
        } else if matches!(
            src[i..].get(..2),
            Some(b"==" | b"!=" | b"<=" | b">=" | b"+=" | b"-=" | b"*=" | b"/=" | b"%=" | b"**")
        ) {
            i += 2;
        } else if b >= 0x80 {
            i += 1;
            while i < src.len() && (src[i] & 0xC0) == 0x80 {
                i += 1;
            }
        } else {
            i += 1;
        }
        out.push(&src[start..i]);
    }
    out
}

const KEYWORDS: &[&str] = &[
    "and", "break", "catch", "continue", "elif", "else", "end", "false", "for", "fun", "if",
    "import", "in", "memo", "null", "not", "or", "return", "then", "true", "try", "while", "xor",
];

const PUNCTUATION: &[&str] = &[
    "(", ")", "[", "]", "{", "}", ":", ",", ".", "...", "..", "?", "=", "==", "!=", "!", "<", "<=",
    ">", ">=", "+", "-", "*", "/", "%", "**", "+=", "-=", "*=", "/=", "%=", ";", "@", "#", "$",
    "&", "|", "^", "~", "\\", "=>", "->",
];

const LITERALS: &[&str] = &[
    "0",
    "1",
    "42",
    "1.5",
    "1.",
    "1e5",
    "1e",
    "9223372036854775807",
    "9223372036854775808",
    "1_000",
    "1__0",
    "\"s\"",
    "'s'",
    "`raw`",
    "\"open",
    "'open",
    "\"a\\qb\"",
    "\"a\\nb\"",
    "/* open",
    "// line\n",
];

fn vocab_token(rng: &mut Rng, names: &Names) -> Vec<u8> {
    let text: &str = match rng.weighted(&[30, 30, 15, 10, 8, 7]) {
        0 => rng.pick(KEYWORDS),
        1 => rng.pick(PUNCTUATION),
        2 => rng.pick(LITERALS),
        3 => rng.pick(&names.disallowed),
        4 => rng.pick(&names.allowed),
        _ => {
            if rng.chance(50) {
                rng.pick(ORDINARY)
            } else {
                rng.pick(NEAR_KEYWORDS)
            }
        }
    };
    text.as_bytes().to_vec()
}

fn is_blank(piece: &[u8]) -> bool {
    piece.iter().all(|&b| is_space(b))
}

/// A random lexeme index, preferring one that is not white space.
fn pick_piece(rng: &mut Rng, pieces: &[Vec<u8>]) -> usize {
    let mut i = rng.below(pieces.len());
    for _ in 0..6 {
        if !is_blank(&pieces[i]) {
            break;
        }
        i = rng.below(pieces.len());
    }
    i
}

/// One to three edits at the level of lexemes: drop, repeat, swap, replace or insert a token, cut the source
/// short, remove a closing token, rename an identifier to a builtin, or splice in lexemes of another source.
pub fn token_mutate(rng: &mut Rng, names: &Names, src: &[u8], other: &[u8]) -> Vec<u8> {
    let mut pieces: Vec<Vec<u8>> = lexemes(src).into_iter().map(<[u8]>::to_vec).collect();
    for _ in 0..rng.range(1, 3) {
        if pieces.is_empty() {
            pieces.push(vocab_token(rng, names));
            continue;
        }
        let i = pick_piece(rng, &pieces);
        match rng.weighted(&[14, 8, 8, 16, 10, 5, 12, 12, 6, 5, 4]) {
            0 => {
                pieces.remove(i);
            }
            1 => {
                let copy = pieces[i].clone();
                pieces.insert(i, copy);
            }
            2 => {
                let j = (i + 1 + rng.below(3)).min(pieces.len() - 1);
                pieces.swap(i, j);
            }
            3 => pieces[i] = vocab_token(rng, names),
            4 => {
                let token = vocab_token(rng, names);
                pieces.insert(i + 1, b" ".to_vec());
                pieces.insert(i + 2, token);
            }
            5 => pieces.truncate(i),
            6 => {
                let closers: Vec<usize> = pieces
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| {
                        matches!(
                            p.as_slice(),
                            b"end" | b")" | b"]" | b"}" | b":" | b"catch" | b"then" | b"else"
                        )
                    })
                    .map(|(i, _)| i)
                    .collect();
                if closers.is_empty() {
                    pieces.remove(i);
                } else {
                    pieces.remove(*rng.pick(&closers));
                }
            }
            7 => {
                let is_word = |p: &[u8]| {
                    p.first()
                        .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
                        && !KEYWORDS.contains(&std::str::from_utf8(p).unwrap_or(""))
                };
                let words: Vec<usize> =
                    (0..pieces.len()).filter(|&k| is_word(&pieces[k])).collect();
                if let Some(&k) = words.get(rng.below(words.len().max(1))) {
                    let pool = if rng.chance(60) {
                        &names.disallowed
                    } else {
                        &names.allowed
                    };
                    pieces[k] = rng.pick(pool).as_bytes().to_vec();
                } else {
                    pieces[i] = vocab_token(rng, names);
                }
            }
            8 => {
                let from: Vec<&[u8]> = lexemes(other);
                if !from.is_empty() {
                    let a = rng.below(from.len());
                    let b = (a + rng.range(1, 12)).min(from.len());
                    let end = (i + rng.range(0, 6)).min(pieces.len());
                    pieces.splice(i..end, from[a..b].iter().map(|s| s.to_vec()));
                }
            }
            9 => {
                let end = (i + rng.range(1, 20)).min(pieces.len());
                let chunk: Vec<Vec<u8>> = pieces[i..end].to_vec();
                for (k, piece) in chunk.into_iter().enumerate() {
                    pieces.insert(end + k, piece);
                }
            }
            _ => {
                if pieces[i]
                    .first()
                    .is_some_and(|b| b.is_ascii_digit() || matches!(b, b'"' | b'\'' | b'`'))
                {
                    pieces[i] = rng.pick(LITERALS).as_bytes().to_vec();
                } else {
                    pieces[i] = vocab_token(rng, names);
                }
            }
        }
    }
    pieces.concat()
}

/// Bytes and short sequences that tokenizers treat in a special way.
pub const INTERESTING: &[&[u8]] = &[
    b"/*",
    b"*/",
    b"//",
    b"\"",
    b"'",
    b"`",
    b"\\",
    b"...",
    b"..",
    b"!",
    b"**",
    b"1e",
    b"0x",
    b"\r",
    b"\r\n",
    b"\n",
    b"\0",
    b"\x7f",
    b"\x0b",
    b"\x0c",
    b";",
    b"_",
    b"1_",
    b"._",
    b"\\n",
    b"\\q",
    b"\\\n",
    b"\xef\xbf\xbd",
    b"\xc2\x85",
    b"\xe2\x80\xa8",
    b"\xc2\xa0",
    b"\xef\xbb\xbf",
    b"\xc3\xa9",
    b"\xe6\x97\xa5",
    b"\xf0\x9f\x98\x80",
    b"\xff",
    b"\x80",
    b"\xc3",
    b"\xe6\x97",
    b"\xf0\x9f\x98",
    b"\xc0\xaf",
    b"\xed\xa0\x80",
    b"\xf4\x90\x80\x80",
    b"\xbf",
];

/// One to four edits of single bytes and ranges; the result need not be valid UTF-8.
pub fn byte_mutate(rng: &mut Rng, src: &[u8]) -> Vec<u8> {
    let mut data = src.to_vec();
    for _ in 0..rng.range(1, 4) {
        if data.is_empty() {
            data.extend_from_slice(rng.pick(INTERESTING));
            continue;
        }
        let i = rng.below(data.len());
        match rng.weighted(&[12, 8, 25, 12, 10, 8, 6, 6, 6, 7]) {
            0 => data[i] ^= 1 << rng.below(8),
            1 => data[i] = rng.next_u64() as u8,
            2 => {
                let seq = rng.pick(INTERESTING);
                data.splice(i..i, seq.iter().copied());
            }
            3 => {
                let seq = rng.pick(INTERESTING);
                let end = (i + seq.len()).min(data.len());
                data.splice(i..end, seq.iter().copied());
            }
            4 => {
                let end = (i + rng.range(1, 8)).min(data.len());
                data.drain(i..end);
            }
            5 => {
                let end = (i + rng.range(1, 32)).min(data.len());
                let chunk = data[i..end].to_vec();
                data.splice(end..end, chunk);
            }
            6 => data.truncate(i),
            7 => {
                let j = rng.below(data.len());
                data.swap(i, j);
            }
            8 => {
                let end = (i + rng.range(1, 3)).min(data.len());
                data.drain(i..end);
                data.insert(i.min(data.len()), b'\n');
            }
            _ => {
                let at = rng.below(data.len() + 1);
                data.insert(at, *rng.pick(b"()[]{}\"'`:"));
            }
        }
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexemes_cover_the_source() {
        for src in [
            &b"x = \"a\\\"b\" // c\n/* d */ y ... 1.5e3 ** z"[..],
            b"\"open",
            b"/* open",
            b"`raw\nstring`",
            "é日".as_bytes(),
            b"\xff\xfe",
            b"",
        ] {
            assert_eq!(lexemes(src).concat(), src);
        }
    }
}

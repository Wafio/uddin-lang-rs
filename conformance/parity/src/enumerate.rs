//! Every sequence of a few tokens over a small alphabet, in a fixed order: short programs that cover each
//! combination of keywords, brackets and operators once.

const KEYWORDS: &[&str] = &[
    "and", "break", "catch", "continue", "elif", "else", "end", "false", "for", "fun", "if",
    "import", "in", "memo", "null", "not", "or", "return", "then", "true", "try", "while", "xor",
];

const PUNCTUATION: &[&str] = &[
    "(", ")", "[", "]", "{", "}", ":", ",", ".", "...", "?", "=", "==", "!=", "<", "<=", ">", ">=",
    "+", "-", "*", "/", "%", "**", "+=", "-=", "*=", "/=", "%=",
];

const OPERANDS: &[&str] = &["x", "sleep", "print", "1", "1.5", "\"s\""];

/// A smaller alphabet that still has every block and call construct, for longer sequences.
const CORE: &[&str] = &[
    "x", "sleep", "1", "(", ")", "[", "]", "{", "}", ":", ",", ".", "=", "?", "-", "if", "then",
    "else", "end", "fun", "in", "for", "try", "catch", "return",
];

/// Enough keywords and brackets for a statement that nests: `if ( x ) then : try : x catch ( x ) : end`.
const BLOCKS: &[&str] = &[
    "x", "(", ")", ":", "{", "}", "end", "then", "else", "elif", "if", "try", "catch", "while",
];

/// `full`, `core` or `blocks`.
pub fn alphabet(name: &str) -> Option<Vec<&'static str>> {
    match name {
        "full" => Some([KEYWORDS, PUNCTUATION, OPERANDS].concat()),
        "core" => Some(CORE.to_vec()),
        "blocks" => Some(BLOCKS.to_vec()),
        _ => None,
    }
}

/// How many sequences of `len` tokens there are.
pub fn size(alphabet: &[&str], len: u32) -> u64 {
    (alphabet.len() as u64).pow(len)
}

/// Sequence number `index` of length `len`, tokens joined by a space; the first token changes slowest.
pub fn sequence(alphabet: &[&str], len: u32, mut index: u64) -> String {
    let base = alphabet.len() as u64;
    let mut picked = vec![""; len as usize];
    for slot in picked.iter_mut().rev() {
        *slot = alphabet[(index % base) as usize];
        index /= base;
    }
    picked.join(" ")
}

/// Every source one edit away from a corpus source: for each lexeme that is not white space, delete it, repeat
/// it, swap it with the next one, replace it with each token of the alphabet, or insert each token before it.
/// Sources over 32 KiB are left out.
pub struct Neighbors<'a> {
    corpus: &'a [Vec<u8>],
    tokens: Vec<&'static str>,
    /// Per source, the byte ranges of its lexemes that are not white space.
    pieces: Vec<Vec<std::ops::Range<usize>>>,
    /// Cumulative count of cases before each source.
    before: Vec<u64>,
}

impl<'a> Neighbors<'a> {
    pub fn new(corpus: &'a [Vec<u8>]) -> Neighbors<'a> {
        let tokens = alphabet("full").expect("the full alphabet exists");
        let mut pieces = Vec::new();
        let mut before = Vec::new();
        let mut total = 0u64;
        for src in corpus {
            let mut at = 0;
            let mut ranges = Vec::new();
            // A source near the size limit has only refused neighbours, and they are large.
            let lexemes = if src.len() > 32 * 1024 {
                Vec::new()
            } else {
                crate::mutate::lexemes(src)
            };
            for lexeme in lexemes {
                if !lexeme.iter().all(u8::is_ascii_whitespace) {
                    ranges.push(at..at + lexeme.len());
                }
                at += lexeme.len();
            }
            before.push(total);
            total += (ranges.len() * (3 + 2 * tokens.len())) as u64;
            pieces.push(ranges);
        }
        before.push(total);
        Neighbors {
            corpus,
            tokens,
            pieces,
            before,
        }
    }

    pub fn size(&self) -> u64 {
        *self.before.last().unwrap_or(&0)
    }

    /// Case number `index`, below `size()`.
    pub fn case(&self, index: u64) -> Vec<u8> {
        let source = self.before.partition_point(|&b| b <= index) - 1;
        let per = (3 + 2 * self.tokens.len()) as u64;
        let local = index - self.before[source];
        let range = self.pieces[source][(local / per) as usize].clone();
        let op = (local % per) as usize;
        let src = &self.corpus[source];
        let next = self.pieces[source]
            .iter()
            .find(|r| r.start >= range.end)
            .cloned();
        let (head, piece, tail) = (&src[..range.start], &src[range.clone()], &src[range.end..]);
        let t = self.tokens.len();
        match op {
            0 => [head, tail].concat(),
            1 => [head, piece, b" ", piece, tail].concat(),
            2 => match next {
                Some(n) => [
                    head,
                    &src[n.clone()],
                    &src[range.end..n.start],
                    piece,
                    &src[n.end..],
                ]
                .concat(),
                None => src.clone(),
            },
            k if k < 3 + t => [head, self.tokens[k - 3].as_bytes(), tail].concat(),
            k => [head, self.tokens[k - 3 - t].as_bytes(), b" ", piece, tail].concat(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sequence_of_a_length_is_listed_once() {
        let a = alphabet("core").expect("known alphabet");
        assert_eq!(size(&a, 3), (a.len() as u64).pow(3));
        let all: std::collections::BTreeSet<String> =
            (0..size(&a, 2)).map(|i| sequence(&a, 2, i)).collect();
        assert_eq!(all.len() as u64, size(&a, 2));
        assert_eq!(sequence(&a, 2, 0), "x x");
        assert_eq!(sequence(&a, 2, 1), "x sleep");
    }

    #[test]
    fn neighbors_are_one_edit_from_a_source() {
        let corpus = vec![b"x = 1\n".to_vec()];
        let n = Neighbors::new(&corpus);
        // Three lexemes that are not white space, each with 3 + 2 * 58 edits.
        assert_eq!(n.size(), 3 * (3 + 2 * 58));
        assert_eq!(n.case(0), b" = 1\n");
        assert_eq!(n.case(1), b"x x = 1\n");
        assert_eq!(n.case(2), b"= x 1\n");
        let all: Vec<Vec<u8>> = (0..n.size()).map(|i| n.case(i)).collect();
        assert!(all.iter().all(|c| c.len() <= b"x = 1\n".len() + 10));
    }
}

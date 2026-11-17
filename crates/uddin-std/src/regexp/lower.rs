//! From the parsed pattern to the matcher: the tree becomes a `regex-syntax` expression, which `regex-automata`
//! compiles. The tree is lowered node by node, never printed back as text, so nothing in the pattern can mean
//! something else to the second parser.

use regex_automata::MatchKind;
use regex_automata::meta;
use regex_syntax::hir::{Capture, Class, ClassUnicode, ClassUnicodeRange, Hir, Look, Repetition};

use super::syntax::{FOLD_CASE, MAX_RUNE, NON_GREEDY, Node, Op, Rune};
use super::uniclass;

/// The compiled form of a pattern may take at most this many bytes, which also bounds the time to build it.
const NFA_LIMIT: usize = 4 << 20;
/// The lazy DFA of one pattern may grow to this many bytes before the matcher falls back to a slower engine.
const DFA_CACHE: usize = 256 << 10;

fn class(pairs: &[Rune]) -> Hir {
    let mut c = ClassUnicode::empty();
    for p in pairs.chunks_exact(2) {
        // A range holds scalar values only; the surrogates in the middle of a wide range match nothing.
        for (lo, hi) in [(p[0], p[1].min(0xD7FF)), (p[0].max(0xE000), p[1])] {
            if let (true, Some(a), Some(b)) = (lo <= hi, char::from_u32(lo), char::from_u32(hi)) {
                c.push(ClassUnicodeRange::new(a, b));
            }
        }
    }
    if c.ranges().is_empty() {
        return Hir::fail();
    }
    Hir::class(Class::Unicode(c))
}

fn literal(n: &Node) -> Hir {
    let fold = n.flags & FOLD_CASE != 0;
    let mut parts: Vec<Hir> = Vec::new();
    let mut plain = String::new();
    for &r in &n.runes {
        let Some(ch) = char::from_u32(r) else {
            return Hir::fail();
        };
        if fold {
            let orbit = uniclass::fold_closure(&[(r, r)]);
            let pairs: Vec<Rune> = orbit.iter().flat_map(|&(a, b)| [a, b]).collect();
            parts.push(class(&pairs));
        } else {
            plain.push(ch);
        }
    }
    if !plain.is_empty() {
        parts.push(Hir::literal(plain.into_bytes()));
    }
    Hir::concat(parts)
}

pub(super) fn to_hir(n: &Node) -> Hir {
    let rep = |min: u32, max: Option<u32>| {
        Hir::repetition(Repetition {
            min,
            max,
            greedy: n.flags & NON_GREEDY == 0,
            sub: Box::new(to_hir(&n.subs[0])),
        })
    };
    match n.op {
        Op::NoMatch | Op::LeftParen | Op::VerticalBar => Hir::fail(),
        Op::EmptyMatch => Hir::empty(),
        Op::Literal => literal(n),
        Op::CharClass => class(&n.runes),
        Op::AnyCharNotNl => class(&[0, 0x09, 0x0B, MAX_RUNE]),
        Op::AnyChar => class(&[0, MAX_RUNE]),
        Op::BeginLine => Hir::look(Look::StartLF),
        Op::EndLine => Hir::look(Look::EndLF),
        Op::BeginText => Hir::look(Look::Start),
        Op::EndText => Hir::look(Look::End),
        Op::WordBoundary => Hir::look(Look::WordAscii),
        Op::NoWordBoundary => Hir::look(Look::WordAsciiNegate),
        Op::Capture => Hir::capture(Capture {
            index: n.cap,
            // The names stay in the parse: a pattern may give two groups the same one, which the matcher refuses.
            name: None,
            sub: Box::new(to_hir(&n.subs[0])),
        }),
        Op::Star => rep(0, None),
        Op::Plus => rep(1, None),
        Op::Quest => rep(0, Some(1)),
        Op::Repeat => rep(n.min as u32, (n.max >= 0).then_some(n.max as u32)),
        Op::Concat => Hir::concat(n.subs.iter().map(to_hir).collect()),
        Op::Alternate => Hir::alternation(n.subs.iter().map(to_hir).collect()),
    }
}

/// The matcher for a parsed pattern, or `None` when its compiled form would be too large.
pub(super) fn build(root: &Node) -> Option<meta::Regex> {
    let config = meta::Config::new()
        .match_kind(MatchKind::LeftmostFirst)
        .utf8_empty(true)
        .nfa_size_limit(Some(NFA_LIMIT))
        .hybrid_cache_capacity(DFA_CACHE)
        .dfa(false);
    meta::Builder::new()
        .configure(config)
        .build_from_hir(&to_hir(root))
        .ok()
}

#[cfg(test)]
mod tests {
    use regex_automata::Input;

    use super::super::syntax::parse;
    use super::*;

    fn find(pattern: &str, text: &str) -> Option<(usize, usize)> {
        let re = build(&parse(pattern.as_bytes()).expect("valid").root).expect("builds");
        let mut cache = re.create_cache();
        re.search_with(&mut cache, &Input::new(text.as_bytes()))
            .map(|m| (m.start(), m.end()))
    }

    #[test]
    fn matches_leftmost_first() {
        assert_eq!(find("a|ab", "ab"), Some((0, 1)));
        assert_eq!(find("a*?", "aaa"), Some((0, 0)));
        assert_eq!(find("(?U)a+", "aaa"), Some((0, 1)));
        assert_eq!(find("x*", "ab"), Some((0, 0)));
        assert_eq!(find("(?i)k", "\u{212A}"), Some((0, 3)));
        assert_eq!(find("\\bfoo\\b", "a foo b"), Some((2, 5)));
        assert_eq!(find("\\b", "é"), None);
        assert_eq!(find("(?m)^b$", "a\nb\nc"), Some((2, 3)));
        assert_eq!(find("a$", "a\n"), None);
    }
}

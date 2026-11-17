//! The parser for the pattern language of the regex builtins: Go's RE2 dialect with Perl extensions. It decides
//! which patterns are valid, words the error for an invalid one, and builds the tree the size and program
//! counts and the matcher are derived from. The tree has the shape the reference's parser gives, because the
//! counts depend on it: literals merge into strings, alternations factor their common prefixes and fold
//! single-character branches into one class.

use std::fmt;

use super::uniclass;

pub(super) type Rune = u32;

pub(super) const MAX_RUNE: Rune = 0x10FFFF;

// Parse flags and node flags share one set of bits.
pub(super) const FOLD_CASE: u16 = 1;
const CLASS_NL: u16 = 1 << 2;
const DOT_NL: u16 = 1 << 3;
const ONE_LINE: u16 = 1 << 4;
pub(super) const NON_GREEDY: u16 = 1 << 5;
const PERL_X: u16 = 1 << 6;
const UNICODE_GROUPS: u16 = 1 << 7;
pub(super) const WAS_DOLLAR: u16 = 1 << 8;
const PERL: u16 = CLASS_NL | ONE_LINE | PERL_X | UNICODE_GROUPS;

/// The number of instructions the parser may build before it refuses an expression as too large.
const MAX_SIZE: i64 = (128 << 20) / 40;
const MAX_RUNES: i64 = (128 << 20) / 4;
/// The deepest tree the parser accepts.
const MAX_HEIGHT: usize = 1000;
/// The most copies a counted repetition may make of what it repeats, nesting included.
const MAX_REPEAT: i64 = 1000;

/// The kind of a node. The order matters: the parser compares kinds when it merges classes.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(super) enum Op {
    NoMatch = 1,
    EmptyMatch,
    Literal,
    CharClass,
    AnyCharNotNl,
    AnyChar,
    BeginLine,
    EndLine,
    BeginText,
    EndText,
    WordBoundary,
    NoWordBoundary,
    Capture,
    Star,
    Plus,
    Quest,
    Repeat,
    Concat,
    Alternate,
    /// A group that is still open; only on the parser's stack.
    LeftParen,
    /// An alternation bar; only on the parser's stack.
    VerticalBar,
}

#[derive(Clone, Debug)]
pub(super) struct Node {
    pub op: Op,
    pub flags: u16,
    /// The characters of a literal, or the low and high end of each range of a class.
    pub runes: Vec<Rune>,
    pub subs: Vec<Node>,
    pub min: i32,
    pub max: i32,
    pub cap: u32,
    pub name: Option<Box<str>>,
}

impl Node {
    pub(super) fn new(op: Op) -> Node {
        Node {
            op,
            flags: 0,
            runes: Vec::new(),
            subs: Vec::new(),
            min: 0,
            max: 0,
            cap: 0,
            name: None,
        }
    }

    fn with_sub(op: Op, flags: u16, sub: Node) -> Node {
        let mut n = Node::new(op);
        n.flags = flags;
        n.subs.push(sub);
        n
    }
}

/// A pattern the parser refused. `Display` gives the text the reference gives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SynError {
    code: &'static str,
    expr: Vec<u8>,
}

impl SynError {
    fn new(code: &'static str, expr: &[u8]) -> SynError {
        SynError {
            code,
            expr: expr.to_vec(),
        }
    }

    /// The text of the error, as bytes: the expression it quotes need not be valid UTF-8.
    pub(crate) fn text(&self) -> Vec<u8> {
        let mut out = b"error parsing regexp: ".to_vec();
        out.extend_from_slice(self.code.as_bytes());
        out.extend_from_slice(b": `");
        out.extend_from_slice(&self.expr);
        out.push(b'`');
        out
    }
}

impl fmt::Display for SynError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(&self.text()))
    }
}

const INVALID_CHAR_RANGE: &str = "invalid character class range";
const INVALID_ESCAPE: &str = "invalid escape sequence";
const INVALID_NAMED_CAPTURE: &str = "invalid named capture";
const INVALID_PERL_OP: &str = "invalid or unsupported Perl syntax";
const INVALID_REPEAT_OP: &str = "invalid nested repetition operator";
const INVALID_REPEAT_SIZE: &str = "invalid repeat count";
const INVALID_UTF8: &str = "invalid UTF-8";
const MISSING_BRACKET: &str = "missing closing ]";
const MISSING_PAREN: &str = "missing closing )";
const MISSING_REPEAT_ARGUMENT: &str = "missing argument to repetition operator";
const TRAILING_BACKSLASH: &str = "trailing backslash at end of expression";
const UNEXPECTED_PAREN: &str = "unexpected )";
const NESTING_DEPTH: &str = "expression nests too deeply";
const LARGE: &str = "expression too large";

/// A class read at the start of some text: the ranges so far and the rest of the text; `None` when the text does not
/// start one.
type ClassMatch<'a> = Result<Option<(Vec<Rune>, &'a [u8])>, SynError>;

/// What a successful parse gives.
pub(super) struct Parsed {
    pub root: Node,
    /// The name of each capture group, by index; entry 0 is the whole match and has none.
    pub names: Vec<Option<Box<str>>>,
}

pub(super) fn parse(pattern: &[u8]) -> Result<Parsed, SynError> {
    let mut p = Parser {
        flags: PERL,
        stack: Vec::new(),
        num_cap: 0,
        whole: pattern,
        lim: Limits::default(),
    };
    p.run()?;
    let mut names = vec![None; p.num_cap as usize + 1];
    collect_names(&p.stack[0], &mut names);
    Ok(Parsed {
        root: p.stack.pop().expect("one expression is left"),
        names,
    })
}

fn collect_names(n: &Node, names: &mut [Option<Box<str>>]) {
    if n.op == Op::Capture {
        names[n.cap as usize].clone_from(&n.name);
    }
    for s in &n.subs {
        collect_names(s, names);
    }
}

#[derive(Default)]
struct Limits {
    num_regexp: i64,
    num_runes: i64,
    repeats: i64,
    tracking: bool,
}

struct Parser<'a> {
    flags: u16,
    stack: Vec<Node>,
    num_cap: u32,
    whole: &'a [u8],
    lim: Limits,
}

/// The character at the start of `s` and its width; an invalid byte is U+FFFD of width 1.
pub(super) fn decode(s: &[u8]) -> (Rune, usize) {
    const BAD: (Rune, usize) = (0xFFFD, 1);
    let Some(&b0) = s.first() else {
        return (0xFFFD, 0);
    };
    let cont = |i: usize| s.get(i).copied().filter(|b| b & 0xC0 == 0x80);
    match b0 {
        0..=0x7F => (Rune::from(b0), 1),
        0xC2..=0xDF => match cont(1) {
            Some(b1) => ((Rune::from(b0 & 0x1F) << 6) | Rune::from(b1 & 0x3F), 2),
            None => BAD,
        },
        0xE0..=0xEF => match (cont(1), cont(2)) {
            (Some(b1), Some(b2)) => {
                let r = (Rune::from(b0 & 0x0F) << 12)
                    | (Rune::from(b1 & 0x3F) << 6)
                    | Rune::from(b2 & 0x3F);
                if r < 0x800 || (0xD800..=0xDFFF).contains(&r) {
                    BAD
                } else {
                    (r, 3)
                }
            }
            _ => BAD,
        },
        0xF0..=0xF4 => match (cont(1), cont(2), cont(3)) {
            (Some(b1), Some(b2), Some(b3)) => {
                let r = (Rune::from(b0 & 0x07) << 18)
                    | (Rune::from(b1 & 0x3F) << 12)
                    | (Rune::from(b2 & 0x3F) << 6)
                    | Rune::from(b3 & 0x3F);
                if !(0x10000..=MAX_RUNE).contains(&r) {
                    BAD
                } else {
                    (r, 4)
                }
            }
            _ => BAD,
        },
        _ => BAD,
    }
}

fn next_rune(s: &[u8]) -> Result<(Rune, &[u8]), SynError> {
    let (c, w) = decode(s);
    if c == 0xFFFD && w == 1 {
        return Err(SynError::new(INVALID_UTF8, s));
    }
    Ok((c, &s[w..]))
}

fn check_utf8(mut s: &[u8]) -> Result<(), SynError> {
    while !s.is_empty() {
        let (c, w) = decode(s);
        if c == 0xFFFD && w == 1 {
            return Err(SynError::new(INVALID_UTF8, s));
        }
        s = &s[w..];
    }
    Ok(())
}

fn is_alnum(c: Rune) -> bool {
    (0x30..=0x39).contains(&c) || (0x41..=0x5A).contains(&c) || (0x61..=0x7A).contains(&c)
}

fn unhex(c: Rune) -> i64 {
    match c {
        0x30..=0x39 => i64::from(c - 0x30),
        0x61..=0x66 => i64::from(c - 0x61 + 10),
        0x41..=0x46 => i64::from(c - 0x41 + 10),
        _ => -1,
    }
}

/// The part of `s` that `rest` does not cover; `rest` is a suffix of `s`.
fn consumed<'a>(s: &'a [u8], rest: &[u8]) -> &'a [u8] {
    &s[..s.len() - rest.len()]
}

/// Sorts the ranges and merges those that overlap or touch.
pub(super) fn clean_class(r: &mut Vec<Rune>) {
    let mut pairs: Vec<(Rune, Rune)> = r.chunks_exact(2).map(|c| (c[0], c[1])).collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    r.clear();
    for (lo, hi) in pairs {
        let n = r.len();
        if n >= 2 && lo <= r[n - 1].saturating_add(1) {
            if hi > r[n - 1] {
                r[n - 1] = hi;
            }
            continue;
        }
        r.push(lo);
        r.push(hi);
    }
}

fn append_range(r: &mut Vec<Rune>, lo: Rune, hi: Rune) {
    r.push(lo);
    r.push(hi);
}

fn append_class(r: &mut Vec<Rune>, x: &[Rune]) {
    r.extend_from_slice(x);
}

/// The ranges of `x`, a clean class, complemented.
fn negate_clean(x: &[Rune]) -> Vec<Rune> {
    let mut out = Vec::new();
    let mut next_lo: u64 = 0;
    for c in x.chunks_exact(2) {
        if next_lo < u64::from(c[0]) {
            out.push(next_lo as Rune);
            out.push(c[0] - 1);
        }
        next_lo = u64::from(c[1]) + 1;
    }
    if next_lo <= u64::from(MAX_RUNE) {
        out.push(next_lo as Rune);
        out.push(MAX_RUNE);
    }
    out
}

/// Adds `lo..=hi` and every character that folds to one of them.
fn append_folded_range(r: &mut Vec<Rune>, lo: Rune, hi: Rune) {
    append_range(r, lo, hi);
    for (a, b) in uniclass::fold_closure(&[(lo, hi)]) {
        append_range(r, a, b);
    }
}

fn append_folded_class(r: &mut Vec<Rune>, x: &[Rune]) {
    for c in x.chunks_exact(2) {
        append_folded_range(r, c[0], c[1]);
    }
}

fn append_literal(r: &mut Vec<Rune>, x: Rune, flags: u16) {
    if flags & FOLD_CASE != 0 {
        append_folded_range(r, x, x);
    } else {
        append_range(r, x, x);
    }
}

fn fold_orbit(r: Rune) -> Vec<Rune> {
    let mut v: Vec<Rune> = uniclass::fold_closure(&[(r, r)])
        .into_iter()
        .flat_map(|(a, b)| a..=b)
        .collect();
    if !v.contains(&r) {
        v.push(r);
    }
    v.sort_unstable();
    v
}

/// The next character in the fold orbit of `r`, wrapping to the smallest; `r` itself when it has no fold.
fn simple_fold(r: Rune) -> Rune {
    let orbit = fold_orbit(r);
    orbit.iter().copied().find(|&c| c > r).unwrap_or(orbit[0])
}

fn min_fold_rune(r: Rune) -> Rune {
    if !(0x41..=0x1E943).contains(&r) {
        return r;
    }
    fold_orbit(r)[0]
}

fn is_char_class(n: &Node) -> bool {
    (n.op == Op::Literal && n.runes.len() == 1)
        || matches!(n.op, Op::CharClass | Op::AnyCharNotNl | Op::AnyChar)
}

fn match_rune(n: &Node, r: Rune) -> bool {
    match n.op {
        Op::Literal => n.runes.len() == 1 && n.runes[0] == r,
        Op::CharClass => n.runes.chunks_exact(2).any(|c| c[0] <= r && r <= c[1]),
        Op::AnyCharNotNl => r != 0x0A,
        Op::AnyChar => true,
        _ => false,
    }
}

pub(super) fn equal(x: &Node, y: &Node) -> bool {
    if x.op != y.op {
        return false;
    }
    match x.op {
        Op::EndText => x.flags & WAS_DOLLAR == y.flags & WAS_DOLLAR,
        Op::Literal | Op::CharClass => x.runes == y.runes,
        Op::Alternate | Op::Concat => {
            x.subs.len() == y.subs.len() && x.subs.iter().zip(&y.subs).all(|(a, b)| equal(a, b))
        }
        Op::Star | Op::Plus | Op::Quest => {
            x.flags & NON_GREEDY == y.flags & NON_GREEDY && equal(&x.subs[0], &y.subs[0])
        }
        Op::Repeat => {
            x.flags & NON_GREEDY == y.flags & NON_GREEDY
                && x.min == y.min
                && x.max == y.max
                && equal(&x.subs[0], &y.subs[0])
        }
        Op::Capture => x.cap == y.cap && x.name == y.name && equal(&x.subs[0], &y.subs[0]),
        _ => true,
    }
}

/// A class becomes the any-character node when it holds everything, or everything but a line feed.
fn clean_alt(n: &mut Node) {
    if n.op != Op::CharClass {
        return;
    }
    clean_class(&mut n.runes);
    if n.runes == [0, MAX_RUNE] {
        n.runes.clear();
        n.op = Op::AnyChar;
    } else if n.runes == [0, 0x09, 0x0B, MAX_RUNE] {
        n.runes.clear();
        n.op = Op::AnyCharNotNl;
    }
}

fn merge_char_class(dst: &mut Node, src: &Node) {
    match dst.op {
        Op::AnyChar => {}
        Op::AnyCharNotNl => {
            if match_rune(src, 0x0A) {
                dst.op = Op::AnyChar;
            }
        }
        Op::CharClass => {
            if src.op == Op::Literal {
                append_literal(&mut dst.runes, src.runes[0], src.flags);
            } else {
                append_class(&mut dst.runes, &src.runes);
            }
        }
        Op::Literal => {
            if src.runes[0] == dst.runes[0] && src.flags == dst.flags {
                return;
            }
            dst.op = Op::CharClass;
            let (r0, f0) = (dst.runes[0], dst.flags);
            dst.runes.clear();
            append_literal(&mut dst.runes, r0, f0);
            append_literal(&mut dst.runes, src.runes[0], src.flags);
        }
        _ => {}
    }
}

fn leading_string(n: &Node) -> (&[Rune], u16) {
    let mut n = n;
    if n.op == Op::Concat && !n.subs.is_empty() {
        n = &n.subs[0];
    }
    if n.op != Op::Literal {
        return (&[], 0);
    }
    (&n.runes, n.flags & FOLD_CASE)
}

fn remove_leading_string(mut n: Node, len: usize) -> Node {
    if n.op == Op::Concat && !n.subs.is_empty() {
        let first = remove_leading_string(n.subs.remove(0), len);
        let emptied = first.op == Op::EmptyMatch;
        n.subs.insert(0, first);
        if emptied {
            match n.subs.len() {
                0 | 1 => {
                    n.op = Op::EmptyMatch;
                    n.subs.clear();
                }
                2 => {
                    n.subs.remove(0);
                    return n.subs.pop().expect("one sub is left");
                }
                _ => {
                    n.subs.remove(0);
                }
            }
        }
        return n;
    }
    if n.op == Op::Literal {
        n.runes.drain(..len.min(n.runes.len()));
        if n.runes.is_empty() {
            n.op = Op::EmptyMatch;
        }
    }
    n
}

fn leading_regexp(n: &Node) -> Option<Node> {
    if n.op == Op::EmptyMatch {
        return None;
    }
    if n.op == Op::Concat && !n.subs.is_empty() {
        let first = &n.subs[0];
        if first.op == Op::EmptyMatch {
            return None;
        }
        return Some(first.clone());
    }
    Some(n.clone())
}

fn remove_leading_regexp(mut n: Node) -> Node {
    if n.op == Op::Concat && !n.subs.is_empty() {
        n.subs.remove(0);
        match n.subs.len() {
            0 => {
                n.op = Op::EmptyMatch;
            }
            1 => return n.subs.pop().expect("one sub is left"),
            _ => {}
        }
        return n;
    }
    Node::new(Op::EmptyMatch)
}

fn repeat_is_valid(n: &Node, mut limit: i64) -> bool {
    if n.op == Op::Repeat {
        let mut m = i64::from(n.max);
        if m == 0 {
            return true;
        }
        if m < 0 {
            m = i64::from(n.min);
        }
        if m > limit {
            return false;
        }
        if m > 0 {
            limit /= m;
        }
    }
    n.subs.iter().all(|s| repeat_is_valid(s, limit))
}

fn calc_size(n: &Node) -> i64 {
    let sub = |i: usize| calc_size(&n.subs[i]);
    let size: i64 = match n.op {
        Op::Literal => n.runes.len() as i64,
        Op::Capture | Op::Star => sub(0).saturating_add(2),
        Op::Plus | Op::Quest => sub(0).saturating_add(1),
        Op::Concat => n.subs.iter().map(calc_size).fold(0, i64::saturating_add),
        Op::Alternate => {
            let total = n.subs.iter().map(calc_size).fold(0, i64::saturating_add);
            if n.subs.len() > 1 {
                total.saturating_add(n.subs.len() as i64 - 1)
            } else {
                total
            }
        }
        Op::Repeat => {
            let s = sub(0);
            if n.max == -1 {
                if n.min == 0 {
                    s.saturating_add(2)
                } else {
                    1i64.saturating_add(i64::from(n.min).saturating_mul(s))
                }
            } else {
                i64::from(n.max)
                    .saturating_mul(s)
                    .saturating_add(i64::from(n.max - n.min))
            }
        }
        _ => 0,
    };
    size.max(1)
}

fn calc_height(n: &Node) -> usize {
    1 + n.subs.iter().map(calc_height).max().unwrap_or(0)
}

impl Limits {
    fn check(&mut self, n: &Node, stack: &[Node], whole: &[u8]) -> Result<(), SynError> {
        if self.num_runes > MAX_RUNES {
            return Err(SynError::new(LARGE, whole));
        }
        if !self.tracking {
            if self.repeats == 0 {
                self.repeats = 1;
            }
            if n.op == Op::Repeat {
                let mut c = i64::from(n.max);
                if c == -1 {
                    c = i64::from(n.min);
                }
                if c <= 0 {
                    c = 1;
                }
                if c > MAX_SIZE / self.repeats {
                    self.repeats = MAX_SIZE;
                } else {
                    self.repeats *= c;
                }
            }
            if self.num_regexp < MAX_SIZE / self.repeats {
                return self.check_height(n, stack, whole);
            }
            self.tracking = true;
            if stack.iter().any(|s| calc_size(s) > MAX_SIZE) {
                return Err(SynError::new(LARGE, whole));
            }
        }
        if calc_size(n) > MAX_SIZE {
            return Err(SynError::new(LARGE, whole));
        }
        self.check_height(n, stack, whole)
    }

    fn check_height(&self, n: &Node, stack: &[Node], whole: &[u8]) -> Result<(), SynError> {
        if (self.num_regexp as usize) < MAX_HEIGHT {
            return Ok(());
        }
        if stack.iter().any(|s| calc_height(s) > MAX_HEIGHT) || calc_height(n) > MAX_HEIGHT {
            return Err(SynError::new(NESTING_DEPTH, whole));
        }
        Ok(())
    }
}

impl Parser<'_> {
    fn new_node(&mut self, op: Op) -> Node {
        self.lim.num_regexp += 1;
        let mut n = Node::new(op);
        n.flags = self.flags;
        n
    }

    fn check_limits_top(&mut self) -> Result<(), SynError> {
        let Some((top, rest)) = self.stack.split_last() else {
            return Ok(());
        };
        self.lim.check(top, rest, self.whole)
    }

    /// Pushes `n`, turning a class of one character into a literal and joining a literal to the one below it.
    fn push(&mut self, mut n: Node) -> Result<(), SynError> {
        self.lim.num_runes += n.runes.len() as i64;
        if n.op == Op::CharClass && n.runes.len() == 2 && n.runes[0] == n.runes[1] {
            if self.maybe_concat(i64::from(n.runes[0]), self.flags & !FOLD_CASE) {
                return Ok(());
            }
            n.op = Op::Literal;
            n.runes.truncate(1);
            n.flags = self.flags & !FOLD_CASE;
        } else if n.op == Op::CharClass && is_case_pair(&n.runes) {
            if self.maybe_concat(i64::from(n.runes[0]), self.flags | FOLD_CASE) {
                return Ok(());
            }
            n.op = Op::Literal;
            n.runes.truncate(1);
            n.flags = self.flags | FOLD_CASE;
        } else {
            self.maybe_concat(-1, 0);
        }
        self.stack.push(n);
        self.check_limits_top()
    }

    /// Joins the two literals on top of the stack into one. With `r >= 0` the node left over becomes a literal
    /// of `r` and the result is true.
    fn maybe_concat(&mut self, r: i64, flags: u16) -> bool {
        let n = self.stack.len();
        if n < 2 {
            return false;
        }
        {
            let (re2, re1) = (&self.stack[n - 2], &self.stack[n - 1]);
            if re1.op != Op::Literal
                || re2.op != Op::Literal
                || re1.flags & FOLD_CASE != re2.flags & FOLD_CASE
            {
                return false;
            }
        }
        let re1 = self.stack.pop().expect("two nodes on the stack");
        self.stack[n - 2].runes.extend_from_slice(&re1.runes);
        if r >= 0 {
            let mut re1 = re1;
            re1.runes = vec![r as Rune];
            re1.flags = flags;
            self.stack.push(re1);
            return true;
        }
        false
    }

    fn literal(&mut self, r: Rune) -> Result<(), SynError> {
        let mut n = self.new_node(Op::Literal);
        n.runes = vec![if self.flags & FOLD_CASE != 0 {
            min_fold_rune(r)
        } else {
            r
        }];
        self.push(n)
    }

    fn op(&mut self, op: Op) -> Result<(), SynError> {
        let n = self.new_node(op);
        self.push(n)
    }

    /// Wraps the node on top of the stack in a repetition. `before` starts at the operator, `after` follows it.
    fn repeat<'a>(
        &mut self,
        op: Op,
        min: i32,
        max: i32,
        before: &'a [u8],
        mut after: &'a [u8],
        last_repeat: &'a [u8],
    ) -> Result<&'a [u8], SynError> {
        let mut flags = self.flags;
        if after.first() == Some(&b'?') {
            after = &after[1..];
            flags ^= NON_GREEDY;
        }
        if !last_repeat.is_empty() {
            return Err(SynError::new(
                INVALID_REPEAT_OP,
                &last_repeat[..last_repeat.len() - after.len()],
            ));
        }
        let missing = || {
            SynError::new(
                MISSING_REPEAT_ARGUMENT,
                &before[..before.len() - after.len()],
            )
        };
        let Some(sub) = self.stack.pop() else {
            return Err(missing());
        };
        if sub.op >= Op::LeftParen {
            self.stack.push(sub);
            return Err(missing());
        }
        let mut re = self.new_node(op);
        re.min = min;
        re.max = max;
        re.flags = flags;
        re.subs.push(sub);
        self.stack.push(re);
        self.check_limits_top()?;
        if op == Op::Repeat
            && (min >= 2 || max >= 2)
            && !repeat_is_valid(self.stack.last().expect("just pushed"), MAX_REPEAT)
        {
            return Err(SynError::new(
                INVALID_REPEAT_SIZE,
                &before[..before.len() - after.len()],
            ));
        }
        Ok(after)
    }

    /// Replaces what is above the topmost bar or open group by its concatenation.
    fn concat(&mut self) -> Result<(), SynError> {
        self.maybe_concat(-1, 0);
        let mut i = self.stack.len();
        while i > 0 && self.stack[i - 1].op < Op::LeftParen {
            i -= 1;
        }
        let subs = self.stack.split_off(i);
        if subs.is_empty() {
            let n = self.new_node(Op::EmptyMatch);
            return self.push(n);
        }
        let n = self.collapse(subs, Op::Concat)?;
        self.push(n)
    }

    /// Replaces what is above the topmost open group by its alternation.
    fn alternate(&mut self) -> Result<(), SynError> {
        let mut i = self.stack.len();
        while i > 0 && self.stack[i - 1].op < Op::LeftParen {
            i -= 1;
        }
        let mut subs = self.stack.split_off(i);
        if let Some(last) = subs.last_mut() {
            clean_alt(last);
        }
        if subs.is_empty() {
            let n = self.new_node(Op::NoMatch);
            return self.push(n);
        }
        let n = self.collapse(subs, Op::Alternate)?;
        self.push(n)
    }

    /// One node for `subs` under `op`; a concatenation inside a concatenation, or an alternation inside an
    /// alternation, joins its parent. An alternation also factors out what its branches share.
    fn collapse(&mut self, subs: Vec<Node>, op: Op) -> Result<Node, SynError> {
        if subs.len() == 1 {
            return Ok(subs.into_iter().next().expect("one sub"));
        }
        let mut re = self.new_node(op);
        for sub in subs {
            if sub.op == op {
                re.subs.extend(sub.subs);
            } else {
                re.subs.push(sub);
            }
        }
        if op == Op::Alternate {
            re.subs = self.factor(std::mem::take(&mut re.subs))?;
            if re.subs.len() == 1 {
                return Ok(re.subs.pop().expect("one sub"));
            }
        }
        Ok(re)
    }

    /// Factors common prefixes out of the branches of an alternation: `abc|abd|aef` becomes `a(?:b[cd]|ef)`.
    fn factor(&mut self, mut sub: Vec<Node>) -> Result<Vec<Node>, SynError> {
        if sub.len() < 2 {
            return Ok(sub);
        }

        // Round 1: a literal prefix shared by neighbouring branches.
        let mut out: Vec<Node> = Vec::with_capacity(sub.len());
        let mut common: Vec<Rune> = Vec::new();
        let mut common_flags = 0u16;
        let mut start = 0;
        let n = sub.len();
        for i in 0..=n {
            let (istr, iflags) = if i < n {
                let (s, f) = leading_string(&sub[i]);
                (s.to_vec(), f)
            } else {
                (Vec::new(), 0)
            };
            if i < n && iflags == common_flags {
                let same = common.iter().zip(&istr).take_while(|(a, b)| a == b).count();
                if same > 0 {
                    common.truncate(same);
                    continue;
                }
            }
            if i == start {
            } else if i == start + 1 {
                out.push(std::mem::replace(&mut sub[start], Node::new(Op::NoMatch)));
            } else {
                let mut prefix = self.new_node(Op::Literal);
                prefix.flags = common_flags;
                prefix.runes.clone_from(&common);
                let mut group = Vec::with_capacity(i - start);
                for slot in &mut sub[start..i] {
                    let cut = remove_leading_string(
                        std::mem::replace(slot, Node::new(Op::NoMatch)),
                        common.len(),
                    );
                    self.lim.check(&cut, &[], self.whole)?;
                    group.push(cut);
                }
                let suffix = self.collapse(group, Op::Alternate)?;
                let mut re = self.new_node(Op::Concat);
                re.subs = vec![prefix, suffix];
                out.push(re);
            }
            start = i;
            common = istr;
            common_flags = iflags;
        }
        let mut sub = out;

        // Round 2: a leading expression shared by neighbouring branches, when it is a single character.
        let mut out: Vec<Node> = Vec::with_capacity(sub.len());
        let mut start = 0;
        let mut first: Option<Node> = None;
        let n = sub.len();
        for i in 0..=n {
            let ifirst = if i < n { leading_regexp(&sub[i]) } else { None };
            if i < n
                && let (Some(f), Some(g)) = (&first, &ifirst)
                && equal(f, g)
                && (is_char_class(f)
                    || (f.op == Op::Repeat && f.min == f.max && is_char_class(&f.subs[0])))
            {
                continue;
            }
            if i == start {
            } else if i == start + 1 {
                out.push(std::mem::replace(&mut sub[start], Node::new(Op::NoMatch)));
            } else {
                let prefix = first.take().expect("a group has a first expression");
                let mut group = Vec::with_capacity(i - start);
                for slot in &mut sub[start..i] {
                    let cut =
                        remove_leading_regexp(std::mem::replace(slot, Node::new(Op::NoMatch)));
                    self.lim.check(&cut, &[], self.whole)?;
                    group.push(cut);
                }
                let suffix = self.collapse(group, Op::Alternate)?;
                let mut re = self.new_node(Op::Concat);
                re.subs = vec![prefix, suffix];
                out.push(re);
            }
            start = i;
            first = ifirst;
        }
        let mut sub = out;

        // Round 3: runs of single-character branches become one class.
        let mut out: Vec<Node> = Vec::with_capacity(sub.len());
        let mut start = 0;
        let n = sub.len();
        for i in 0..=n {
            if i < n && is_char_class(&sub[i]) {
                continue;
            }
            if i == start {
            } else if i == start + 1 {
                out.push(std::mem::replace(&mut sub[start], Node::new(Op::NoMatch)));
            } else {
                let mut max = start;
                for j in start + 1..i {
                    if sub[max].op < sub[j].op
                        || (sub[max].op == sub[j].op && sub[max].runes.len() < sub[j].runes.len())
                    {
                        max = j;
                    }
                }
                sub.swap(start, max);
                for j in start + 1..i {
                    let src = std::mem::replace(&mut sub[j], Node::new(Op::NoMatch));
                    merge_char_class(&mut sub[start], &src);
                }
                clean_alt(&mut sub[start]);
                out.push(std::mem::replace(&mut sub[start], Node::new(Op::NoMatch)));
            }
            if i < n {
                out.push(std::mem::replace(&mut sub[i], Node::new(Op::NoMatch)));
            }
            start = i + 1;
        }
        let sub = out;

        // Round 4: two empty branches in a row are one.
        let mut out: Vec<Node> = Vec::with_capacity(sub.len());
        let mut prev_empty = false;
        for s in sub {
            let empty = s.op == Op::EmptyMatch;
            if empty && prev_empty {
                continue;
            }
            prev_empty = empty;
            out.push(s);
        }
        Ok(out)
    }

    fn parse_vertical_bar(&mut self) -> Result<(), SynError> {
        self.concat()?;
        if !self.swap_vertical_bar() {
            self.op(Op::VerticalBar)?;
        }
        Ok(())
    }

    /// Moves a bar below the node on top of it, merging the two classes on either side of a bar first.
    fn swap_vertical_bar(&mut self) -> bool {
        let n = self.stack.len();
        if n >= 3
            && self.stack[n - 2].op == Op::VerticalBar
            && is_char_class(&self.stack[n - 1])
            && is_char_class(&self.stack[n - 3])
        {
            let mut re1 = self.stack.pop().expect("three nodes on the stack");
            let re3 = &mut self.stack[n - 3];
            if re1.op > re3.op {
                std::mem::swap(&mut re1, re3);
            }
            merge_char_class(re3, &re1);
            return true;
        }
        if n >= 2 && self.stack[n - 2].op == Op::VerticalBar {
            if n >= 3 {
                clean_alt(&mut self.stack[n - 3]);
            }
            self.stack.swap(n - 2, n - 1);
            return true;
        }
        false
    }

    fn parse_right_paren(&mut self) -> Result<(), SynError> {
        self.concat()?;
        if self.swap_vertical_bar() {
            self.stack.pop();
        }
        self.alternate()?;
        let n = self.stack.len();
        if n < 2 {
            return Err(SynError::new(UNEXPECTED_PAREN, self.whole));
        }
        let re1 = self.stack.pop().expect("two nodes on the stack");
        let mut re2 = self.stack.pop().expect("two nodes on the stack");
        if re2.op != Op::LeftParen {
            return Err(SynError::new(UNEXPECTED_PAREN, self.whole));
        }
        self.flags = re2.flags;
        if re2.cap == 0 {
            self.push(re1)
        } else {
            re2.op = Op::Capture;
            re2.subs = vec![re1];
            self.push(re2)
        }
    }

    fn run(&mut self) -> Result<(), SynError> {
        let mut t = self.whole;
        let mut last_repeat: &[u8] = &[];
        while !t.is_empty() {
            let mut repeat: &[u8] = &[];
            match t[0] {
                b'(' => {
                    if t.len() >= 2 && t[1] == b'?' {
                        t = self.parse_perl_flags(t)?;
                    } else {
                        self.num_cap += 1;
                        let mut n = self.new_node(Op::LeftParen);
                        n.cap = self.num_cap;
                        self.push(n)?;
                        t = &t[1..];
                    }
                }
                b'|' => {
                    self.parse_vertical_bar()?;
                    t = &t[1..];
                }
                b')' => {
                    self.parse_right_paren()?;
                    t = &t[1..];
                }
                b'^' => {
                    self.op(if self.flags & ONE_LINE != 0 {
                        Op::BeginText
                    } else {
                        Op::BeginLine
                    })?;
                    t = &t[1..];
                }
                b'$' => {
                    if self.flags & ONE_LINE != 0 {
                        let mut n = self.new_node(Op::EndText);
                        n.flags |= WAS_DOLLAR;
                        self.push(n)?;
                    } else {
                        self.op(Op::EndLine)?;
                    }
                    t = &t[1..];
                }
                b'.' => {
                    self.op(if self.flags & DOT_NL != 0 {
                        Op::AnyChar
                    } else {
                        Op::AnyCharNotNl
                    })?;
                    t = &t[1..];
                }
                b'[' => t = self.parse_class(t)?,
                b'*' | b'+' | b'?' => {
                    let op = match t[0] {
                        b'*' => Op::Star,
                        b'+' => Op::Plus,
                        _ => Op::Quest,
                    };
                    let before = t;
                    t = self.repeat(op, 0, 0, before, &t[1..], last_repeat)?;
                    repeat = before;
                }
                b'{' => match parse_repeat(t) {
                    None => {
                        self.literal(Rune::from(b'{'))?;
                        t = &t[1..];
                    }
                    Some((min, max, after)) => {
                        let before = t;
                        if !(0..=1000).contains(&min) || max > 1000 || (max >= 0 && min > max) {
                            return Err(SynError::new(
                                INVALID_REPEAT_SIZE,
                                consumed(before, after),
                            ));
                        }
                        t = self.repeat(Op::Repeat, min, max, before, after, last_repeat)?;
                        repeat = before;
                    }
                },
                b'\\' => t = self.parse_backslash(t)?,
                _ => {
                    let (c, rest) = next_rune(t)?;
                    t = rest;
                    self.literal(c)?;
                }
            }
            last_repeat = repeat;
        }
        self.concat()?;
        if self.swap_vertical_bar() {
            self.stack.pop();
        }
        self.alternate()?;
        if self.stack.len() != 1 {
            return Err(SynError::new(MISSING_PAREN, self.whole));
        }
        Ok(())
    }

    fn parse_backslash<'a>(&mut self, t: &'a [u8]) -> Result<&'a [u8], SynError> {
        if t.len() >= 2 {
            match t[1] {
                b'A' => {
                    self.op(Op::BeginText)?;
                    return Ok(&t[2..]);
                }
                b'b' => {
                    self.op(Op::WordBoundary)?;
                    return Ok(&t[2..]);
                }
                b'B' => {
                    self.op(Op::NoWordBoundary)?;
                    return Ok(&t[2..]);
                }
                b'C' => return Err(SynError::new(INVALID_ESCAPE, &t[..2])),
                b'Q' => {
                    let body = &t[2..];
                    let (mut lit, rest) = match find(body, b"\\E") {
                        Some(i) => (&body[..i], &body[i + 2..]),
                        None => (body, &body[body.len()..]),
                    };
                    while !lit.is_empty() {
                        let (c, r) = next_rune(lit)?;
                        self.literal(c)?;
                        lit = r;
                    }
                    return Ok(rest);
                }
                b'z' => {
                    self.op(Op::EndText)?;
                    return Ok(&t[2..]);
                }
                _ => {}
            }
        }
        let mut re = self.new_node(Op::CharClass);
        if t.len() >= 2
            && (t[1] == b'p' || t[1] == b'P')
            && let Some((r, rest)) = self.parse_unicode_class(t, Vec::new())?
        {
            re.runes = r;
            self.push(re)?;
            return Ok(rest);
        }
        if let Some((r, rest)) = self.parse_perl_class_escape(t, Vec::new()) {
            re.runes = r;
            self.push(re)?;
            return Ok(rest);
        }
        let (c, rest) = parse_escape(t)?;
        self.literal(c)?;
        Ok(rest)
    }

    fn parse_perl_flags<'a>(&mut self, s: &'a [u8]) -> Result<&'a [u8], SynError> {
        let starts_with_p = s.len() > 4 && s[2] == b'P' && s[3] == b'<';
        let starts_with_name = s.len() > 3 && s[2] == b'<';
        if starts_with_p || starts_with_name {
            let expr_start = if starts_with_name { 3 } else { 4 };
            let Some(end) = s.iter().position(|&b| b == b'>') else {
                check_utf8(s)?;
                return Err(SynError::new(INVALID_NAMED_CAPTURE, s));
            };
            let capture = &s[..=end];
            let name = &s[expr_start.min(end)..end];
            check_utf8(name)?;
            if !is_valid_capture_name(name) {
                return Err(SynError::new(INVALID_NAMED_CAPTURE, capture));
            }
            self.num_cap += 1;
            let mut n = self.new_node(Op::LeftParen);
            n.cap = self.num_cap;
            n.name = Some(String::from_utf8_lossy(name).into_owned().into());
            self.push(n)?;
            return Ok(&s[end + 1..]);
        }

        let mut t = &s[2..];
        let mut flags = self.flags;
        let mut sign = 1;
        let mut saw_flag = false;
        while !t.is_empty() {
            let (c, rest) = next_rune(t)?;
            t = rest;
            match c {
                0x69 => {
                    flags |= FOLD_CASE;
                    saw_flag = true;
                }
                0x6D => {
                    flags &= !ONE_LINE;
                    saw_flag = true;
                }
                0x73 => {
                    flags |= DOT_NL;
                    saw_flag = true;
                }
                0x55 => {
                    flags |= NON_GREEDY;
                    saw_flag = true;
                }
                0x2D => {
                    if sign < 0 {
                        break;
                    }
                    sign = -1;
                    flags = !flags;
                    saw_flag = false;
                }
                0x3A | 0x29 => {
                    if sign < 0 {
                        if !saw_flag {
                            break;
                        }
                        flags = !flags;
                    }
                    if c == 0x3A {
                        self.op(Op::LeftParen)?;
                    }
                    self.flags = flags;
                    return Ok(t);
                }
                _ => break,
            }
        }
        Err(SynError::new(INVALID_PERL_OP, consumed(s, t)))
    }

    fn parse_perl_class_escape<'a>(
        &self,
        s: &'a [u8],
        mut r: Vec<Rune>,
    ) -> Option<(Vec<Rune>, &'a [u8])> {
        if s.len() < 2 || s[0] != b'\\' {
            return None;
        }
        let (sign, class): (i8, &[Rune]) = match s[1] {
            b'd' => (1, &DIGIT),
            b'D' => (-1, &DIGIT),
            b's' => (1, &SPACE),
            b'S' => (-1, &SPACE),
            b'w' => (1, &WORD),
            b'W' => (-1, &WORD),
            _ => return None,
        };
        self.append_group(&mut r, sign, class);
        Some((r, &s[2..]))
    }

    fn parse_named_class<'a>(&self, s: &'a [u8], mut r: Vec<Rune>) -> ClassMatch<'a> {
        if s.len() < 2 || s[0] != b'[' || s[1] != b':' {
            return Ok(None);
        }
        let Some(i) = find(&s[2..], b":]") else {
            return Ok(None);
        };
        let i = i + 2;
        let (name, rest) = (&s[..i + 2], &s[i + 2..]);
        let Some((sign, class)) = posix_group(name) else {
            return Err(SynError::new(INVALID_CHAR_RANGE, name));
        };
        self.append_group(&mut r, sign, class);
        Ok(Some((r, rest)))
    }

    fn append_group(&self, r: &mut Vec<Rune>, sign: i8, class: &[Rune]) {
        let mut tmp = class.to_vec();
        if self.flags & FOLD_CASE != 0 {
            tmp.clear();
            append_folded_class(&mut tmp, class);
        }
        clean_class(&mut tmp);
        if sign < 0 {
            r.extend(negate_clean(&tmp));
        } else {
            r.extend(tmp);
        }
    }

    fn parse_unicode_class<'a>(&self, s: &'a [u8], mut r: Vec<Rune>) -> ClassMatch<'a> {
        if self.flags & UNICODE_GROUPS == 0
            || s.len() < 2
            || s[0] != b'\\'
            || (s[1] != b'p' && s[1] != b'P')
        {
            return Ok(None);
        }
        let mut sign: i8 = if s[1] == b'P' { -1 } else { 1 };
        let (c, mut t) = next_rune(&s[2..])?;
        let (seq, name): (&[u8], &[u8]);
        if c != 0x7B {
            seq = consumed(s, t);
            name = &seq[2..];
        } else {
            let Some(end) = s.iter().position(|&b| b == b'}') else {
                check_utf8(s)?;
                return Err(SynError::new(INVALID_CHAR_RANGE, s));
            };
            seq = &s[..=end];
            t = &s[end + 1..];
            name = &s[3..end];
            check_utf8(name)?;
        }
        let mut name = name;
        if name.first() == Some(&b'^') {
            sign = -sign;
            name = &name[1..];
        }
        let fold = self.flags & FOLD_CASE != 0;
        let Some((class, tsign)) = std::str::from_utf8(name)
            .ok()
            .and_then(|n| uniclass::lookup(n, fold))
        else {
            return Err(SynError::new(INVALID_CHAR_RANGE, seq));
        };
        if tsign < 0 {
            sign = -sign;
        }
        if sign > 0 {
            r.extend_from_slice(&class);
        } else {
            r.extend(negate_clean(&class));
        }
        Ok(Some((r, t)))
    }

    fn parse_class<'a>(&mut self, s: &'a [u8]) -> Result<&'a [u8], SynError> {
        let mut t = &s[1..];
        let mut re = self.new_node(Op::CharClass);
        let mut sign = 1;
        if t.first() == Some(&b'^') {
            sign = -1;
            t = &t[1..];
            if self.flags & CLASS_NL == 0 {
                append_range(&mut re.runes, 0x0A, 0x0A);
            }
        }
        let mut class = std::mem::take(&mut re.runes);
        let mut first = true;
        while t.is_empty() || t[0] != b']' || first {
            first = false;
            if t.len() > 2
                && t[0] == b'['
                && t[1] == b':'
                && let Some((nclass, nt)) = self.parse_named_class(t, class.clone())?
            {
                class = nclass;
                t = nt;
                continue;
            }
            if let Some((nclass, nt)) = self.parse_unicode_class(t, class.clone())? {
                class = nclass;
                t = nt;
                continue;
            }
            if let Some((nclass, nt)) = self.parse_perl_class_escape(t, class.clone()) {
                class = nclass;
                t = nt;
                continue;
            }
            let rng = t;
            let (lo, rest) = parse_class_char(t, s)?;
            t = rest;
            let mut hi = lo;
            if t.len() >= 2 && t[0] == b'-' && t[1] != b']' {
                let (h, rest) = parse_class_char(&t[1..], s)?;
                hi = h;
                t = rest;
                if hi < lo {
                    return Err(SynError::new(INVALID_CHAR_RANGE, consumed(rng, t)));
                }
            }
            if self.flags & FOLD_CASE == 0 {
                append_range(&mut class, lo, hi);
            } else {
                append_folded_range(&mut class, lo, hi);
            }
        }
        t = &t[1..];
        clean_class(&mut class);
        if sign < 0 {
            class = negate_clean(&class);
        }
        re.runes = class;
        self.push(re)?;
        Ok(t)
    }
}

/// Whether a class is exactly the two cases of one character, such as `[Aa]`.
fn is_case_pair(r: &[Rune]) -> bool {
    match r {
        [a, b, c, d] => *a == *b && *c == *d && simple_fold(*a) == *c && simple_fold(*c) == *a,
        [a, b] => *a + 1 == *b && simple_fold(*a) == *b && simple_fold(*b) == *a,
        _ => false,
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn is_valid_capture_name(name: &[u8]) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut s = name;
    while !s.is_empty() {
        let (c, w) = decode(s);
        if c != 0x5F && !is_alnum(c) {
            return false;
        }
        s = &s[w..];
    }
    true
}

/// `{n}`, `{n,}` or `{n,m}` at the start of `s`: the counts (-1 for no upper bound) and what follows. A brace
/// that does not start one of these is a literal.
fn parse_repeat(s: &[u8]) -> Option<(i32, i32, &[u8])> {
    let mut s = s.strip_prefix(b"{")?;
    let (mut min, rest) = parse_int(s)?;
    s = rest;
    let max;
    match s.first()? {
        b',' => {
            s = &s[1..];
            match s.first()? {
                b'}' => max = -1,
                _ => {
                    let (m, rest) = parse_int(s)?;
                    s = rest;
                    if m < 0 {
                        min = -1;
                    }
                    max = m;
                }
            }
        }
        _ => max = min,
    }
    let s = s.strip_prefix(b"}")?;
    Some((min, max, s))
}

/// A decimal number without leading zeros, or -1 for one past 100 million.
fn parse_int(s: &[u8]) -> Option<(i32, &[u8])> {
    if s.first().is_none_or(|b| !b.is_ascii_digit()) {
        return None;
    }
    if s.len() >= 2 && s[0] == b'0' && s[1].is_ascii_digit() {
        return None;
    }
    let digits = s.iter().take_while(|b| b.is_ascii_digit()).count();
    let mut n: i64 = 0;
    for &b in &s[..digits] {
        if n >= 100_000_000 {
            n = -1;
            break;
        }
        n = n * 10 + i64::from(b - b'0');
    }
    Some((n as i32, &s[digits..]))
}

fn parse_class_char<'a>(s: &'a [u8], whole_class: &[u8]) -> Result<(Rune, &'a [u8]), SynError> {
    if s.is_empty() {
        return Err(SynError::new(MISSING_BRACKET, whole_class));
    }
    if s[0] == b'\\' {
        return parse_escape(s);
    }
    next_rune(s)
}

/// One escape sequence at the start of `s`, which begins with a backslash.
fn parse_escape(s: &[u8]) -> Result<(Rune, &[u8]), SynError> {
    let t = &s[1..];
    if t.is_empty() {
        return Err(SynError::new(TRAILING_BACKSLASH, b""));
    }
    let (c, mut t) = next_rune(t)?;
    let bad = |t: &[u8]| Err(SynError::new(INVALID_ESCAPE, consumed(s, t)));
    match c {
        0x30..=0x37 => {
            // An octal escape: \0 and up to three digits, or \1 to \7 followed by at least one more digit.
            if c != 0x30 && t.first().is_none_or(|b| !(b'0'..=b'7').contains(b)) {
                return bad(t);
            }
            let mut r = c - 0x30;
            for _ in 1..3 {
                match t.first() {
                    Some(&b) if (b'0'..=b'7').contains(&b) => {
                        r = r * 8 + Rune::from(b - b'0');
                        t = &t[1..];
                    }
                    _ => break,
                }
            }
            Ok((r, t))
        }
        0x78 => {
            if t.is_empty() {
                return bad(t);
            }
            let (c, rest) = next_rune(t)?;
            t = rest;
            if c == 0x7B {
                let mut nhex = 0;
                let mut r: Rune = 0;
                loop {
                    if t.is_empty() {
                        return bad(t);
                    }
                    let (c, rest) = next_rune(t)?;
                    t = rest;
                    if c == 0x7D {
                        break;
                    }
                    let v = unhex(c);
                    if v < 0 {
                        return bad(t);
                    }
                    r = r * 16 + v as Rune;
                    if r > MAX_RUNE {
                        return bad(t);
                    }
                    nhex += 1;
                }
                if nhex == 0 {
                    return bad(t);
                }
                return Ok((r, t));
            }
            let x = unhex(c);
            let (c, rest) = next_rune(t)?;
            t = rest;
            let y = unhex(c);
            if x < 0 || y < 0 {
                return bad(t);
            }
            Ok(((x * 16 + y) as Rune, t))
        }
        0x61 => Ok((0x07, t)),
        0x66 => Ok((0x0C, t)),
        0x6E => Ok((0x0A, t)),
        0x72 => Ok((0x0D, t)),
        0x74 => Ok((0x09, t)),
        0x76 => Ok((0x0B, t)),
        _ if c < 0x80 && !is_alnum(c) => Ok((c, t)),
        _ => bad(t),
    }
}

const DIGIT: [Rune; 2] = [0x30, 0x39];
const SPACE: [Rune; 6] = [0x09, 0x0A, 0x0C, 0x0D, 0x20, 0x20];
const WORD: [Rune; 8] = [0x30, 0x39, 0x41, 0x5A, 0x5F, 0x5F, 0x61, 0x7A];

/// `[:name:]` or `[:^name:]` as the sign and the class.
fn posix_group(name: &[u8]) -> Option<(i8, &'static [Rune])> {
    let inner = name.strip_prefix(b"[:")?.strip_suffix(b":]")?;
    let (sign, inner) = match inner.strip_prefix(b"^") {
        Some(rest) => (-1, rest),
        None => (1, inner),
    };
    let class: &'static [Rune] = match inner {
        b"alnum" => &[0x30, 0x39, 0x41, 0x5A, 0x61, 0x7A],
        b"alpha" => &[0x41, 0x5A, 0x61, 0x7A],
        b"ascii" => &[0x00, 0x7F],
        b"blank" => &[0x09, 0x09, 0x20, 0x20],
        b"cntrl" => &[0x00, 0x1F, 0x7F, 0x7F],
        b"digit" => &[0x30, 0x39],
        b"graph" => &[0x21, 0x7E],
        b"lower" => &[0x61, 0x7A],
        b"print" => &[0x20, 0x7E],
        b"punct" => &[0x21, 0x2F, 0x3A, 0x40, 0x5B, 0x60, 0x7B, 0x7E],
        b"space" => &[0x09, 0x0D, 0x20, 0x20],
        b"upper" => &[0x41, 0x5A],
        b"word" => &[0x30, 0x39, 0x41, 0x5A, 0x5F, 0x5F, 0x61, 0x7A],
        b"xdigit" => &[0x30, 0x39, 0x41, 0x46, 0x61, 0x66],
        _ => return None,
    };
    Some((sign, class))
}

impl Node {
    /// A whole-pattern node for a parsed pattern that has the form of the multi-match search: the text from the
    /// start up to one character, a lazy run of any characters, then the pattern in group 1.
    pub(super) fn after_wrap(tree: &Node) -> Node {
        let mut star = Node::with_sub(Op::Star, NON_GREEDY, Node::new(Op::AnyChar));
        star.flags = NON_GREEDY;
        let mut cap = Node::with_sub(Op::Capture, 0, tree.clone());
        cap.cap = 1;
        let mut wrap = Node::new(Op::Concat);
        wrap.subs = vec![Node::new(Op::BeginText), Node::new(Op::AnyChar), star, cap];
        wrap
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(p: &str) -> String {
        match parse(p.as_bytes()) {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn accepts_the_dialect() {
        for p in [
            "",
            "a",
            "a|b",
            "(a)(?:b)",
            "(?P<n>a)",
            "(?<n>a)",
            "\\d+\\s*\\w",
            "[[:alpha:]]",
            "\\pL\\p{Greek}",
            "(?i)abc",
            "a{2,5}?",
            "\\Qa.b\\E",
            "[^\\n]",
            "\\x{10FFFF}",
            "(?i:a)b",
            "(?)",
            "\\z\\A\\b\\B",
        ] {
            assert_eq!(err(p), "", "{p}");
        }
    }

    #[test]
    fn words_the_errors_as_the_reference_does() {
        let cases = [
            ("(a", "error parsing regexp: missing closing ): `(a`"),
            ("a)", "error parsing regexp: unexpected ): `a)`"),
            ("[a", "error parsing regexp: missing closing ]: `[a`"),
            (
                "a**",
                "error parsing regexp: invalid nested repetition operator: `**`",
            ),
            (
                "*a",
                "error parsing regexp: missing argument to repetition operator: `*`",
            ),
            (
                "a{1001}",
                "error parsing regexp: invalid repeat count: `{1001}`",
            ),
            (
                "(a{500}){3}",
                "error parsing regexp: invalid repeat count: `{3}`",
            ),
            (
                "\\",
                "error parsing regexp: trailing backslash at end of expression: ``",
            ),
            (
                "\\C",
                "error parsing regexp: invalid escape sequence: `\\C`",
            ),
            (
                "\\1",
                "error parsing regexp: invalid escape sequence: `\\1`",
            ),
            (
                "[z-a]",
                "error parsing regexp: invalid character class range: `z-a`",
            ),
            (
                "[[:foo:]]",
                "error parsing regexp: invalid character class range: `[:foo:]`",
            ),
            (
                "(?P<n",
                "error parsing regexp: invalid named capture: `(?P<n`",
            ),
            (
                "(?P<1-a>x)",
                "error parsing regexp: invalid named capture: `(?P<1-a>`",
            ),
            (
                "(?z)",
                "error parsing regexp: invalid or unsupported Perl syntax: `(?z`",
            ),
            (
                "\\pX",
                "error parsing regexp: invalid character class range: `\\pX`",
            ),
            (
                "\\p{Old_Italic}",
                "error parsing regexp: invalid character class range: `\\p{Old_Italic}`",
            ),
        ];
        for (p, want) in cases {
            assert_eq!(err(p), want, "{p}");
        }
    }

    #[test]
    fn shapes_the_tree_as_the_reference_does() {
        let shape = |p: &str| parse(p.as_bytes()).expect("valid").root;
        let r = shape("abc");
        assert_eq!((r.op, r.runes.len()), (Op::Literal, 3));
        let r = shape("a|b|c");
        assert_eq!(
            (r.op, r.runes.as_slice()),
            (Op::CharClass, &[0x61, 0x63][..])
        );
        let r = shape("abc|abd");
        assert_eq!(r.op, Op::Concat);
        assert_eq!(r.subs[0].runes, [0x61, 0x62]);
        let r = shape("a|A");
        assert_eq!(
            (r.op, r.runes.as_slice(), r.flags & FOLD_CASE),
            (Op::Literal, &[0x41][..], FOLD_CASE)
        );
    }

    #[test]
    fn decodes_as_the_host_language_does() {
        assert_eq!(decode(b"\xE2\x82\xAC"), (0x20AC, 3));
        assert_eq!(decode(b"\xE2\x82"), (0xFFFD, 1));
        assert_eq!(decode(b"\xED\xA0\x80"), (0xFFFD, 1));
        assert_eq!(decode(b"\xC0\x80"), (0xFFFD, 1));
        assert_eq!(decode(b"\xEF\xBF\xBD"), (0xFFFD, 3));
    }
}

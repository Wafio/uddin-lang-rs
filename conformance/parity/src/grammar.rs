//! Programs built from the statement and expression rules of the language, with typed holes (name, number,
//! string, expression, statement, block) filled from pools. Most output is valid; the damage step of the
//! caller makes some of it invalid.

use crate::names::{NEAR_KEYWORDS, Names, ORDINARY};
use crate::rng::Rng;

/// The dials of one program, drawn once so that programs differ in style as well as in content.
struct Style {
    /// Percent of blocks written with braces.
    brace: u32,
    /// Percent of names drawn from the builtins the profile refuses.
    disallowed: u32,
    /// Percent of names drawn from the builtins the profile allows.
    allowed: u32,
    /// Percent of separators that carry a comment.
    comments: u32,
    /// Percent of lists, calls and maps that end with a comma.
    trailing: u32,
    /// Percent of spaces left out between punctuation.
    tight: u32,
    max_depth: u32,
}

pub struct Grammar<'a> {
    rng: &'a mut Rng,
    names: &'a Names,
    style: Style,
    out: String,
    /// Nodes still to be written; the program stops growing when it reaches zero.
    budget: i32,
}

/// A program of one to a few dozen statements.
pub fn program(rng: &mut Rng, names: &Names) -> String {
    let style = Style {
        brace: *rng.pick(&[0, 0, 20, 50, 100]),
        disallowed: *rng.pick(&[0, 0, 2, 10, 30, 70]),
        allowed: *rng.pick(&[0, 10, 25, 50]),
        comments: *rng.pick(&[0, 0, 0, 5, 30]),
        trailing: *rng.pick(&[0, 10, 50]),
        tight: *rng.pick(&[0, 30, 90]),
        max_depth: rng.range(2, 9) as u32,
    };
    let budget = match rng.below(10) {
        0..=5 => rng.range(3, 25),
        6..=8 => rng.range(20, 120),
        _ => rng.range(100, 700),
    } as i32;
    let mut g = Grammar {
        rng,
        names,
        style,
        out: String::new(),
        budget,
    };
    let count = 1 + g.rng.below(6);
    g.statements(0, count);
    g.out
}

impl Grammar<'_> {
    fn put(&mut self, text: &str) {
        self.out.push_str(text);
    }

    /// Separator between two word-like tokens: never empty.
    fn word_separator(&mut self) -> &'static str {
        if self.rng.chance(self.style.comments) {
            return self
                .rng
                .pick(&[" /* c */ ", " // c\n", "/**/", " /* a\nb */ "]);
        }
        match self.rng.weighted(&[60, 15, 5, 4, 3, 3]) {
            0 => " ",
            1 => "\n",
            2 => "  ",
            3 => "\t",
            4 => "\r\n",
            _ => "\n\n",
        }
    }

    /// Separator beside punctuation, where it may be empty.
    fn punctuation_gap(&mut self) -> &'static str {
        if self.rng.chance(self.style.tight) {
            ""
        } else {
            " "
        }
    }

    fn name(&mut self) -> String {
        let roll = self.rng.below(100) as u32;
        let (f, a) = (self.style.disallowed, self.style.allowed);
        if roll < f && !self.names.disallowed.is_empty() {
            self.rng.pick(&self.names.disallowed).clone()
        } else if roll < f + a {
            self.rng.pick(&self.names.allowed).clone()
        } else if self.rng.chance(4) {
            (*self.rng.pick(NEAR_KEYWORDS)).to_owned()
        } else {
            (*self.rng.pick(ORDINARY)).to_owned()
        }
    }

    fn number(&mut self) -> String {
        match self.rng.weighted(&[40, 15, 12, 8, 8, 5, 6, 6]) {
            0 => self.rng.below(100).to_string(),
            1 => format!("{}.{}", self.rng.below(1000), self.rng.below(1000)),
            2 => self
                .rng
                .pick(&["0", "1", "2", "10", "100", "255", "1024", "65536"])
                .to_string(),
            3 => format!("{}e{}", self.rng.below(10), self.rng.range(0, 30)),
            4 => format!("1_{}", self.rng.below(1000)),
            5 => self
                .rng
                .pick(&["0.5", "3.14159", "2.0", "1e3", "1E-3", "0.0", "00", "007"])
                .to_string(),
            6 => self
                .rng
                .pick(&[
                    "9223372036854775807",
                    "9223372036854775808",
                    "1e308",
                    "1e309",
                ])
                .to_string(),
            _ => format!("{}", self.rng.next_u64() >> self.rng.below(64)),
        }
    }

    fn string(&mut self) -> String {
        const WORDS: &[&str] = &[
            "",
            "a",
            "abc",
            "hello world",
            "/admin",
            "../etc/passwd",
            "GET",
            "POST",
            "text/html",
            "Mozilla/5.0",
            "<script>",
            "' OR 1=1",
            "a b",
            "x=1&y=2",
            "é",
            "日本語",
            "😀",
            "tab\\there",
            "q\\\"q",
            "back\\\\slash",
            "nl\\n",
            "cr\\r",
            "it's",
            "//not a comment",
            "/* nor this */",
        ];
        let body = *self.rng.pick(WORDS);
        match self.rng.weighted(&[60, 25, 15]) {
            0 => format!("\"{}\"", body.replace("\\'", "'")),
            1 => format!("'{}'", body.replace("\\\"", "\"")),
            _ => format!("`{}`", body.replace("\\\\", "\\")),
        }
    }

    fn atom(&mut self) {
        self.budget -= 1;
        match self.rng.weighted(&[36, 26, 20, 4, 4, 4, 6]) {
            0 => {
                let n = self.name();
                self.put(&n);
            }
            1 => {
                let n = self.number();
                self.put(&n);
            }
            2 => {
                let s = self.string();
                self.put(&s);
            }
            3 => self.put("true"),
            4 => self.put("false"),
            5 => self.put("null"),
            _ => {
                let c = *self
                    .rng
                    .pick(&["PI", "E", "TAU", "PHI", "LN2", "LN10", "SQRT2", "SQRT3"]);
                self.put(c);
            }
        }
    }

    fn expr(&mut self, d: u32) {
        if d >= self.style.max_depth || self.budget <= 0 {
            return self.atom();
        }
        self.budget -= 1;
        match self.rng.weighted(&[26, 20, 6, 5, 12, 6, 5, 6, 6, 4, 2]) {
            0 => self.atom(),
            1 => self.binary(d),
            2 => self.unary(d),
            3 => {
                self.expr(d + 1);
                let (a, b) = (self.punctuation_gap(), self.punctuation_gap());
                self.put(&format!("{a}?{b}"));
                self.expr(d + 1);
                self.put(&format!("{a}:{b}"));
                self.expr(d + 1);
            }
            4 => self.call(d),
            5 => {
                self.postfix(d + 1);
                self.put("[");
                self.expr(d + 1);
                self.put("]");
            }
            6 => {
                self.postfix(d + 1);
                self.put(".");
                let n = self.member_name();
                self.put(&n);
            }
            7 => self.list(d),
            8 => self.map(d),
            9 => {
                self.put("(");
                self.expr(d + 1);
                self.put(")");
            }
            _ => self.function(d, false),
        }
    }

    fn member_name(&mut self) -> String {
        if self.rng.chance(3) {
            // A keyword here is a syntax error, and a builtin name is not a reference.
            return (*self
                .rng
                .pick(&["end", "if", "in", "import", "null", "memo"]))
            .to_owned();
        }
        self.name()
    }

    fn binary(&mut self, d: u32) {
        const OPS: &[&str] = &[
            "or", "xor", "and", "==", "!=", "<", "<=", ">", ">=", "in", "+", "-", "*", "/", "%",
            "**",
        ];
        self.expr(d + 1);
        let op = *self.rng.pick(OPS);
        if op.as_bytes()[0].is_ascii_alphabetic() {
            let (s1, s2) = (self.word_separator(), self.word_separator());
            self.put(&format!("{s1}{op}{s2}"));
        } else {
            let (a, b) = (self.punctuation_gap(), self.punctuation_gap());
            self.put(&format!("{a}{op}{b}"));
        }
        self.expr(d + 1);
    }

    fn unary(&mut self, d: u32) {
        if self.rng.chance(50) {
            self.put("-");
        } else {
            let s = self.word_separator();
            self.put(&format!("not{s}"));
        }
        self.expr(d + 1);
    }

    /// An expression that may be called, indexed or followed by a member name.
    fn postfix(&mut self, d: u32) {
        self.budget -= 1;
        if d >= self.style.max_depth || self.budget <= 0 {
            return self.atom();
        }
        match self.rng.weighted(&[40, 10, 14, 10, 10, 8, 4, 4]) {
            0 => {
                let n = self.name();
                self.put(&n);
            }
            1 => {
                self.put("(");
                self.expr(d + 1);
                self.put(")");
            }
            2 => self.call(d),
            3 => {
                self.postfix(d + 1);
                self.put("[");
                self.expr(d + 1);
                self.put("]");
            }
            4 => {
                self.postfix(d + 1);
                self.put(".");
                let n = self.name();
                self.put(&n);
            }
            5 => {
                let s = self.string();
                self.put(&s);
            }
            6 => self.list(d),
            _ => self.map(d),
        }
    }

    fn call(&mut self, d: u32) {
        // Builtin names are the usual callee.
        if self.rng.chance(60) {
            let pool = if self.rng.chance(self.style.disallowed + 10)
                && !self.names.disallowed.is_empty()
            {
                &self.names.disallowed
            } else {
                &self.names.allowed
            };
            let n = self.rng.pick(pool).clone();
            self.put(&n);
        } else {
            self.postfix(d + 1);
        }
        self.put("(");
        let count = self.rng.below(4);
        for i in 0..count {
            if i > 0 {
                let p = self.punctuation_gap();
                self.put(&format!(",{p}"));
            }
            self.expr(d + 1);
        }
        if count > 0 && self.rng.chance(8) {
            self.put("...");
        } else if count > 0 && self.rng.chance(self.style.trailing) {
            self.put(",");
        }
        self.put(")");
    }

    fn list(&mut self, d: u32) {
        self.put("[");
        let count = self.rng.below(5);
        for i in 0..count {
            if i > 0 {
                let p = self.punctuation_gap();
                self.put(&format!(",{p}"));
            }
            self.expr(d + 1);
        }
        if count > 0 && self.rng.chance(self.style.trailing) {
            self.put(",");
        }
        self.put("]");
    }

    fn map(&mut self, d: u32) {
        self.put("{");
        let count = self.rng.below(4);
        for i in 0..count {
            if i > 0 {
                let p = self.punctuation_gap();
                self.put(&format!(",{p}"));
            }
            match self.rng.weighted(&[50, 30, 10, 10]) {
                0 => {
                    let n = self.name();
                    self.put(&n);
                }
                1 => {
                    let s = self.string();
                    self.put(&s);
                }
                2 => {
                    let n = self.number();
                    self.put(&n);
                }
                _ => self.expr(d + 1),
            }
            let p = self.punctuation_gap();
            self.put(&format!(":{p}"));
            self.expr(d + 1);
        }
        if count > 0 && self.rng.chance(self.style.trailing) {
            self.put(",");
        }
        self.put("}");
    }

    fn params(&mut self) {
        self.put("(");
        let count = self.rng.below(4);
        for i in 0..count {
            if i > 0 {
                let p = self.punctuation_gap();
                self.put(&format!(",{p}"));
            }
            let n = self.name();
            self.put(&n);
        }
        if count > 0 && self.rng.chance(10) {
            self.put("...");
        } else if count > 0 && self.rng.chance(self.style.trailing) {
            self.put(",");
        }
        self.put(")");
    }

    /// `fun name(...)` or an anonymous `fun (...)`, with its block.
    fn function(&mut self, d: u32, named: bool) {
        self.put("fun");
        if named {
            let s = self.word_separator();
            let n = self.name();
            self.put(&format!("{s}{n}"));
        } else {
            let p = self.punctuation_gap();
            self.put(p);
        }
        self.params();
        self.block(d + 1, true);
    }

    /// A block: `{ ... }`, or `: ... end` when `closed` (an if chain leaves out the end of inner blocks).
    fn block(&mut self, d: u32, closed: bool) {
        let count = self.rng.below(4);
        if self.rng.chance(self.style.brace) {
            let s = self.word_separator();
            self.put(&format!("{{{s}"));
            self.statements(d, count);
            self.put("}");
        } else {
            let s = self.word_separator();
            self.put(&format!(":{s}"));
            self.statements(d, count);
            if closed {
                self.put("end");
            }
        }
    }

    fn statements(&mut self, d: u32, count: usize) {
        for _ in 0..count {
            self.statement(d);
            let s = self.word_separator();
            self.put(s);
        }
    }

    fn statement(&mut self, d: u32) {
        self.budget -= 1;
        let leaf = d >= self.style.max_depth || self.budget <= 0;
        let kind = if leaf {
            self.rng
                .weighted(&[26, 8, 20, 6, 4, 0, 0, 0, 0, 0, 5, 2, 2, 0, 3])
        } else {
            self.rng
                .weighted(&[22, 6, 18, 5, 4, 12, 5, 6, 8, 2, 5, 2, 2, 6, 3])
        };
        match kind {
            0 => {
                let n = self.name();
                let op = *self.rng.pick(&["=", "="]);
                let p = self.punctuation_gap();
                self.put(&format!("{n}{p}{op}{p}"));
                self.expr(d + 1);
            }
            1 => {
                let n = self.name();
                let op = *self.rng.pick(&["+=", "-=", "*=", "/=", "%="]);
                let p = self.punctuation_gap();
                self.put(&format!("{n}{p}{op}{p}"));
                self.expr(d + 1);
            }
            2 => self.expr(d),
            3 => {
                self.postfix(d + 1);
                self.put("[");
                self.expr(d + 1);
                let (p, op) = (self.punctuation_gap(), *self.rng.pick(&["=", "+=", "-="]));
                self.put(&format!("]{p}{op}{p}"));
                self.expr(d + 1);
            }
            4 => {
                self.postfix(d + 1);
                self.put(".");
                let n = self.name();
                let (p, op) = (self.punctuation_gap(), *self.rng.pick(&["=", "+=", "*="]));
                self.put(&format!("{n}{p}{op}{p}"));
                self.expr(d + 1);
            }
            5 => self.if_chain(d),
            6 => {
                self.put("while");
                self.condition(d);
                self.block(d + 1, true);
            }
            7 => {
                let p = self.punctuation_gap();
                let n = self.name();
                self.put(&format!("for{p}("));
                self.put(&n);
                let s = self.word_separator();
                self.put(&format!("{s}in{s}"));
                self.expr(d + 1);
                self.put(")");
                self.block(d + 1, true);
            }
            8 => self.function(d, true),
            9 => {
                self.put("memo");
                let s = self.word_separator();
                self.put(s);
                self.function(d, true);
            }
            10 => {
                let s = self.word_separator();
                self.put(&format!("return{s}"));
                self.expr(d + 1);
            }
            11 => self.put("break"),
            12 => self.put("continue"),
            13 => self.try_catch(d),
            _ => {
                let s = self.word_separator();
                let text = self.string();
                self.put(&format!("import{s}{text}"));
            }
        }
    }

    /// `(expr)` after `if`, `elif` or `while`.
    fn condition(&mut self, d: u32) {
        let p = self.punctuation_gap();
        self.put(&format!("{p}("));
        self.expr(d + 1);
        self.put(")");
    }

    /// `if (c) then BLOCK` followed by `elif`/`else` branches. A colon block that another branch follows is
    /// closed by the next keyword and, now and then, by an `end` of its own; the last one needs its `end`.
    fn if_chain(&mut self, d: u32) {
        let branches = 1 + self.rng.below(3);
        for i in 0..branches {
            if i == 0 {
                self.put("if");
            } else if self.rng.chance(15) {
                self.put("else if");
            } else {
                self.put("elif");
            }
            self.condition(d);
            let s = self.word_separator();
            self.put(&format!("{s}then"));
            let last = i + 1 == branches;
            let has_else = last && self.rng.chance(40);
            let close_anyway = self.rng.chance(10);
            self.block(d + 1, last && !has_else || close_anyway);
            if i + 1 < branches {
                let s = self.word_separator();
                self.put(s);
            }
            if has_else {
                let s = self.word_separator();
                self.put(s);
                self.put("else");
                self.block(d + 1, true);
            }
        }
    }

    fn try_catch(&mut self, d: u32) {
        self.put("try");
        let count = self.rng.below(3);
        if self.rng.chance(self.style.brace) {
            self.put("{");
            self.statements(d + 1, count);
            self.put("}");
        } else {
            self.put(":");
            let s = self.word_separator();
            self.put(s);
            self.statements(d + 1, count);
        }
        let n = self.name();
        let p = self.punctuation_gap();
        self.put(&format!("catch{p}({n})"));
        self.block(d + 1, true);
    }
}

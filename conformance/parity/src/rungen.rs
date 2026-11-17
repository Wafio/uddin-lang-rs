//! Scripts for the comparison of runs: programs that mostly run, mixing every construct the core language has,
//! so that values, scopes, errors and limits come up in combinations nobody wrote by hand.
//!
//! A program binds a few variables of known kinds and a few functions, then runs a handful of statements built
//! from them. Expressions are mostly well typed, and sometimes not, so results, type errors, value errors and
//! limit stops are all common; the point is that both implementations agree on which, and on the text.

use crate::rng::Rng;

const INTS: &[&str] = &[
    "0",
    "1",
    "2",
    "3",
    "5",
    "7",
    "10",
    "100",
    "255",
    "1000",
    "9223372036854775807",
];
const FLOATS: &[&str] = &["0.5", "1.5", "2.0", "0.1", "3.25", "100.0", "0.0"];
const STRS: &[&str] = &[
    "\"\"",
    "\"a\"",
    "\"hello\"",
    "\"h\u{e9}llo\"",
    "\"line\\n\"",
    "'x'",
    "\"a\\\"b\"",
    "\"abc def\"",
    "`raw\\n`",
];
const KEYS: &[&str] = &["a", "b", "k", "n", "\"s t\"", "\"a\""];
const NUM_VARS: &[&str] = &["v0", "v1"];
const STR_VARS: &[&str] = &["v2", "v3"];
const CONSTS: &[&str] = &["PI", "E", "TAU", "SQRT2"];

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Num,
    Str,
    Bool,
    List,
    Map,
    Any,
}

struct Gen<'a> {
    rng: &'a mut Rng,
    locals: Vec<String>,
    /// Whether `break` and `continue` may be written here: inside a loop, and not inside a `try` body that is
    /// inside it. The reference lets a `catch` take a jump out of a `try` as text, and `uddin` does not.
    jumps: bool,
}

impl Gen<'_> {
    fn any_name(&mut self) -> String {
        if !self.locals.is_empty() && self.rng.chance(35) {
            let i = self.rng.below(self.locals.len());
            return self.locals[i].clone();
        }
        format!("v{}", self.rng.below(6))
    }

    fn var_of(&mut self, kind: Kind) -> String {
        if self.rng.chance(7) {
            return self.any_name();
        }
        match kind {
            Kind::Num => self.rng.pick(NUM_VARS).to_string(),
            Kind::Str => self.rng.pick(STR_VARS).to_string(),
            Kind::List => "v4".to_owned(),
            Kind::Map => "v5".to_owned(),
            _ => self.any_name(),
        }
    }

    fn typed(&mut self, kind: Kind, d: u32) -> String {
        let leaf = d >= 3 || self.rng.chance(30);
        match kind {
            Kind::Any => {
                let k = [
                    Kind::Num,
                    Kind::Num,
                    Kind::Str,
                    Kind::Str,
                    Kind::Bool,
                    Kind::List,
                    Kind::Map,
                ][self.rng.below(7)];
                if self.rng.chance(4) {
                    return "null".to_owned();
                }
                self.typed(k, d)
            }
            Kind::Num if leaf => match self.rng.weighted(&[40, 15, 30, 5, 5]) {
                0 => self.rng.pick(INTS).to_string(),
                1 => self.rng.pick(FLOATS).to_string(),
                2 => self.var_of(Kind::Num),
                3 => self.rng.pick(CONSTS).to_string(),
                _ => format!("(-{})", self.rng.pick(&INTS[..8])),
            },
            Kind::Str if leaf => {
                if self.rng.chance(60) {
                    self.rng.pick(STRS).to_string()
                } else {
                    self.var_of(Kind::Str)
                }
            }
            Kind::Bool if leaf => ["true", "false"][self.rng.below(2)].to_owned(),
            Kind::List if leaf => {
                if self.rng.chance(50) {
                    self.var_of(Kind::List)
                } else {
                    self.list(d)
                }
            }
            Kind::Map if leaf => {
                if self.rng.chance(50) {
                    self.var_of(Kind::Map)
                } else {
                    self.map(d)
                }
            }
            Kind::Num => match self.rng.weighted(&[40, 10, 6, 8, 10, 8, 10, 8]) {
                0 => {
                    let op = ["+", "-", "*", "**"][self.rng.below(4)];
                    format!(
                        "({} {op} {})",
                        self.typed(Kind::Num, d + 1),
                        self.typed(Kind::Num, d + 1)
                    )
                }
                1 => {
                    let op = ["/", "%"][self.rng.below(2)];
                    let rhs = if self.rng.chance(85) {
                        self.rng.pick(&INTS[1..10]).to_string()
                    } else {
                        self.typed(Kind::Num, d + 1)
                    };
                    format!("({} {op} {rhs})", self.typed(Kind::Num, d + 1))
                }
                2 => format!("(-{})", self.typed(Kind::Num, d + 1)),
                3 => {
                    let k = [Kind::Str, Kind::List, Kind::Map][self.rng.below(3)];
                    format!("len({})", self.typed(k, d + 1))
                }
                4 => format!(
                    "({} ? {} : {})",
                    self.typed(Kind::Bool, d + 1),
                    self.typed(Kind::Num, d + 1),
                    self.typed(Kind::Num, d + 1)
                ),
                5 => format!(
                    "f0({}, {})",
                    self.typed(Kind::Num, d + 1),
                    self.typed(Kind::Any, d + 1)
                ),
                6 => format!("({})[{}]", self.var_of(Kind::List), self.rng.range(0, 3)),
                _ => format!(
                    "g0({}, {})",
                    self.typed(Kind::Num, d + 1),
                    self.typed(Kind::Any, d + 1)
                ),
            },
            Kind::Str => match self.rng.weighted(&[40, 15, 10, 15, 10, 10]) {
                0 => format!(
                    "({} + {})",
                    self.typed(Kind::Str, d + 1),
                    self.typed(Kind::Str, d + 1)
                ),
                1 => format!("str({})", self.typed(Kind::Any, d + 1)),
                2 => format!("typeof({})", self.typed(Kind::Any, d + 1)),
                3 => format!(
                    "({} * {})",
                    self.typed(Kind::Str, d + 1),
                    self.rng.range(0, 4)
                ),
                4 => format!(
                    "({})[{}]",
                    self.typed(Kind::Str, d + 1),
                    self.rng.pick(&["0", "1", "(-1)", "2", "5"])
                ),
                _ => format!(
                    "({} ? {} : {})",
                    self.typed(Kind::Bool, d + 1),
                    self.typed(Kind::Str, d + 1),
                    self.typed(Kind::Str, d + 1)
                ),
            },
            Kind::Bool => match self.rng.weighted(&[25, 20, 15, 10, 10, 10, 10]) {
                0 => {
                    let op = ["==", "!=", "<", "<=", ">", ">="][self.rng.below(6)];
                    let k = [Kind::Num, Kind::Str, Kind::List, Kind::Any][self.rng.below(4)];
                    format!("({} {op} {})", self.typed(k, d + 1), self.typed(k, d + 1))
                }
                1 => {
                    let op = ["and", "or", "xor"][self.rng.below(3)];
                    format!(
                        "({} {op} {})",
                        self.typed(Kind::Bool, d + 1),
                        self.typed(Kind::Bool, d + 1)
                    )
                }
                2 => format!("(not {})", self.typed(Kind::Bool, d + 1)),
                3 => format!(
                    "({} in {})",
                    self.typed(Kind::Str, d + 1),
                    self.typed(Kind::Str, d + 1)
                ),
                4 => format!(
                    "({} in {})",
                    self.typed(Kind::Any, d + 1),
                    self.typed(Kind::List, d + 1)
                ),
                5 => format!(
                    "({} in {})",
                    self.typed(Kind::Str, d + 1),
                    self.typed(Kind::Map, d + 1)
                ),
                _ => format!(
                    "({} == {})",
                    self.typed(Kind::Any, d + 1),
                    self.typed(Kind::Any, d + 1)
                ),
            },
            Kind::List => match self.rng.weighted(&[30, 20, 15, 10, 15, 10]) {
                0 => self.list(d),
                1 => format!(
                    "({} + {})",
                    self.typed(Kind::List, d + 1),
                    self.typed(Kind::List, d + 1)
                ),
                2 => format!(
                    "({} * {})",
                    self.typed(Kind::List, d + 1),
                    self.rng.range(0, 3)
                ),
                3 => format!("f1({})", self.typed(Kind::Any, d + 1)),
                4 => format!(
                    "({} ? {} : {})",
                    self.typed(Kind::Bool, d + 1),
                    self.typed(Kind::List, d + 1),
                    self.typed(Kind::List, d + 1)
                ),
                _ => format!("mk({})", self.typed(Kind::Any, d + 1)),
            },
            Kind::Map => match self.rng.weighted(&[40, 25, 20, 15]) {
                0 => self.map(d),
                1 => format!(
                    "({} + {})",
                    self.typed(Kind::Map, d + 1),
                    self.typed(Kind::Map, d + 1)
                ),
                2 => format!(
                    "({} ? {} : {})",
                    self.typed(Kind::Bool, d + 1),
                    self.typed(Kind::Map, d + 1),
                    self.typed(Kind::Map, d + 1)
                ),
                _ => format!(
                    "({}).{}",
                    self.typed(Kind::Map, d + 1),
                    ["a", "b", "k"][self.rng.below(3)]
                ),
            },
        }
    }

    fn list(&mut self, d: u32) -> String {
        let n = self.rng.weighted(&[15, 30, 30, 15, 10]);
        let items: Vec<String> = (0..n).map(|_| self.typed(Kind::Any, d + 1)).collect();
        format!("[{}]", items.join(", "))
    }

    fn map(&mut self, d: u32) -> String {
        let n = self.rng.weighted(&[15, 40, 30, 15]);
        let items: Vec<String> = (0..n)
            .map(|_| format!("{}: {}", self.rng.pick(KEYS), self.typed(Kind::Any, d + 1)))
            .collect();
        format!("{{{}}}", items.join(", "))
    }

    fn call_stmt(&mut self) -> String {
        match self.rng.weighted(&[30, 20, 20, 10, 10, 10]) {
            0 => format!(
                "f0({}, {})",
                self.typed(Kind::Num, 1),
                self.typed(Kind::Any, 1)
            ),
            1 => format!(
                "g0({}, {})",
                self.typed(Kind::Num, 1),
                self.typed(Kind::Any, 1)
            ),
            2 => format!(
                "g0({}, {}...)",
                self.typed(Kind::Num, 1),
                self.typed(Kind::List, 1)
            ),
            3 => format!(
                "mk({})({})",
                self.typed(Kind::Num, 1),
                self.typed(Kind::Num, 1)
            ),
            4 => format!("f1({})", self.typed(Kind::Num, 1)),
            _ => format!("str({}...)", self.typed(Kind::List, 1)),
        }
    }

    /// The body of a loop, where `break` and `continue` are allowed.
    fn loop_block(&mut self, depth: u32, out: &mut String, indent: usize) {
        let before = std::mem::replace(&mut self.jumps, true);
        self.block(depth, out, indent);
        self.jumps = before;
    }

    fn block(&mut self, depth: u32, out: &mut String, indent: usize) {
        for _ in 0..self.rng.range(1, 3) {
            self.stmt(depth + 1, out, indent);
        }
    }

    fn assign_kind(&mut self) -> (String, Kind) {
        let i = self.rng.below(6);
        let kind = [
            Kind::Num,
            Kind::Num,
            Kind::Str,
            Kind::Str,
            Kind::List,
            Kind::Map,
        ][i];
        let kind_or_any = if self.rng.chance(10) { Kind::Any } else { kind };
        (format!("v{i}"), kind_or_any)
    }

    fn stmt(&mut self, depth: u32, out: &mut String, indent: usize) {
        let pad = " ".repeat(indent);
        let weights: &[u32] = if depth >= 3 {
            &[30, 8, 25, 6, 0, 0, 0, 0, 3, 2, 8]
        } else {
            &[22, 8, 16, 9, 8, 7, 5, 9, 3, 2, 8]
        };
        match self.rng.weighted(weights) {
            0 => {
                let (name, kind) = self.assign_kind();
                let e = self.typed(kind, 0);
                out.push_str(&format!("{pad}{name} = {e}\n"));
            }
            1 => {
                let op = ["+=", "-=", "*=", "/=", "%="][self.rng.below(5)];
                let (name, kind) = self.assign_kind();
                let kind = if kind == Kind::Any { Kind::Num } else { kind };
                out.push_str(&format!("{pad}{name} {op} {}\n", self.typed(kind, 1)));
            }
            2 => {
                let args: Vec<String> = (0..self.rng.range(0, 3))
                    .map(|_| self.typed(Kind::Any, 1))
                    .collect();
                out.push_str(&format!("{pad}print({})\n", args.join(", ")));
            }
            3 => {
                let target = if self.rng.chance(50) {
                    format!(
                        "{}[{}]",
                        ["v4", "v5", "v2"][self.rng.below(3)],
                        self.typed(Kind::Any, 2)
                    )
                } else {
                    format!("v5.{}", ["a", "b", "k"][self.rng.below(3)])
                };
                let op = ["=", "=", "+=", "-="][self.rng.below(4)];
                out.push_str(&format!(
                    "{pad}{target} {op} {}\n",
                    self.typed(Kind::Any, 1)
                ));
            }
            4 => {
                out.push_str(&format!("{pad}if ({}) then:\n", self.typed(Kind::Bool, 1)));
                self.block(depth, out, indent + 2);
                if self.rng.chance(40) {
                    out.push_str(&format!(
                        "{pad}elif ({}) then:\n",
                        self.typed(Kind::Bool, 1)
                    ));
                    self.block(depth, out, indent + 2);
                }
                if self.rng.chance(50) {
                    out.push_str(&format!("{pad}else:\n"));
                    self.block(depth, out, indent + 2);
                }
                out.push_str(&format!("{pad}end\n"));
            }
            5 => {
                let var = ["i", "k", "ch"][self.rng.below(3)];
                let k = [Kind::List, Kind::Str, Kind::Map, Kind::Any][self.rng.below(4)];
                let src = self.typed(k, 1);
                out.push_str(&format!("{pad}for ({var} in {src}):\n"));
                self.locals.push(var.to_owned());
                self.loop_block(depth, out, indent + 2);
                out.push_str(&format!("{pad}end\n"));
            }
            6 => {
                let bound = self.rng.range(1, 6);
                out.push_str(&format!(
                    "{pad}c = 0\n{pad}while (c < {bound}):\n{pad}  c += 1\n"
                ));
                self.loop_block(depth, out, indent + 2);
                out.push_str(&format!("{pad}end\n"));
            }
            7 => {
                out.push_str(&format!("{pad}try:\n"));
                let before = std::mem::replace(&mut self.jumps, false);
                self.block(depth, out, indent + 2);
                self.jumps = before;
                out.push_str(&format!(
                    "{pad}catch (e):\n{pad}  print(\"caught\", str(e))\n"
                ));
                if self.rng.chance(40) {
                    self.block(depth, out, indent + 2);
                }
                out.push_str(&format!("{pad}end\n"));
            }
            8 if self.jumps => out.push_str(&format!(
                "{pad}{}\n",
                ["break", "continue"][self.rng.below(2)]
            )),
            9 => out.push_str(&format!("{pad}return {}\n", self.typed(Kind::Any, 1))),
            _ => {
                let e = self.call_stmt();
                out.push_str(&format!("{pad}{e}\n"));
            }
        }
    }
}

fn prelude(g: &mut Gen<'_>, out: &mut String) {
    out.push_str("v0 = 3\nv1 = 2.5\nv2 = \"abc\"\nv3 = \"x y\"\nv4 = [1, \"a\", [2]]\nv5 = {a: 1, b: \"two\", k: [3]}\n");
    if g.rng.chance(92) {
        out.push_str("fun f0(a, b):\n  if (a < 2) then:\n    return a\n  end\n  return f0(a - 1, b) + 1\nend\n");
    }
    if g.rng.chance(85) {
        out.push_str("fun f1(a):\n  v0 = a\n  r = []\n  try:\n    r = [v4[a], v0]\n  catch (e):\n    r = [e]\n  end\n  return r\nend\n");
    }
    if g.rng.chance(85) {
        out.push_str("fun g0(a, rest...):\n  return len(rest) + a\nend\n");
    }
    if g.rng.chance(85) {
        out.push_str("mk = fun (n): return fun (m): n += m\n return [n] end end\n");
    }
}

/// One program.
pub fn program(rng: &mut Rng) -> String {
    let mut g = Gen {
        rng,
        locals: Vec::new(),
        jumps: false,
    };
    let mut out = String::new();
    prelude(&mut g, &mut out);
    let n = g.rng.range(3, 10);
    for _ in 0..n {
        g.stmt(0, &mut out, 0);
    }
    out
}

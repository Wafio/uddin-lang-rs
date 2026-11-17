//! The sizes of a parsed pattern: the quick estimate of its program that the guard checks before anything is
//! built, and the length of the program the reference compiles for it, which is what a search is charged by.

use uddin_vm::checked_product;

use super::syntax::{NON_GREEDY, Node, Op};

/// The instruction count of `n`, roughly as the compiler would count it, without expanding repetitions; once it
/// passes `limit` the answer is `limit + 1`.
pub(super) fn estimate(n: &Node, limit: i64) -> i64 {
    let cap_at = |v: i64| if v < 0 || v > limit { limit + 1 } else { v };
    match n.op {
        Op::Literal => cap_at(n.runes.len() as i64),
        Op::Capture => cap_at(2 + estimate(&n.subs[0], limit)),
        Op::Star | Op::Plus | Op::Quest => cap_at(1 + estimate(&n.subs[0], limit)),
        Op::Concat | Op::Alternate => {
            let mut total = n.subs.len() as i64;
            for sub in &n.subs {
                total = cap_at(total + estimate(sub, limit));
                if total > limit {
                    return total;
                }
            }
            total
        }
        Op::Repeat => {
            let mut count = i64::from(n.max);
            if count < 0 {
                count = i64::from(n.min) + 1;
            }
            count = count.max(1);
            cap_at(checked_product(count, 1 + estimate(&n.subs[0], limit)))
        }
        _ => 1,
    }
}

/// What the compiler makes of one node: how many instructions, whether it can match the empty string, and the
/// kind and greediness of what the simplifier left at its root (a repetition of a repetition of the same kind
/// is the inner one).
#[derive(Clone, Copy)]
struct Piece {
    insts: u64,
    nullable: bool,
    op: Op,
    non_greedy: bool,
}

const EMPTY: Piece = Piece {
    insts: 1,
    nullable: true,
    op: Op::EmptyMatch,
    non_greedy: false,
};

fn repeat1(op: Op, flags: u16, sub: Piece) -> Piece {
    let non_greedy = flags & NON_GREEDY != 0;
    if sub.op == Op::EmptyMatch || (sub.op == op && sub.non_greedy == non_greedy) {
        return sub;
    }
    let extra = match op {
        Op::Star if sub.nullable => 2,
        _ => 1,
    };
    Piece {
        insts: sub.insts.saturating_add(extra),
        nullable: op != Op::Plus || sub.nullable,
        op,
        non_greedy,
    }
}

fn concat(subs: &[Piece]) -> Piece {
    if subs.is_empty() {
        return EMPTY;
    }
    Piece {
        insts: subs.iter().fold(0, |a, p| a.saturating_add(p.insts)),
        nullable: subs.iter().all(|p| p.nullable),
        op: Op::Concat,
        non_greedy: false,
    }
}

fn piece(n: &Node) -> Piece {
    let one = |nullable| Piece {
        insts: 1,
        nullable,
        op: n.op,
        non_greedy: false,
    };
    match n.op {
        Op::NoMatch => one(false),
        Op::EmptyMatch => EMPTY,
        Op::Literal => {
            if n.runes.is_empty() {
                Piece {
                    insts: 1,
                    nullable: true,
                    ..one(true)
                }
            } else {
                Piece {
                    insts: n.runes.len() as u64,
                    ..one(false)
                }
            }
        }
        Op::CharClass | Op::AnyCharNotNl | Op::AnyChar => one(false),
        Op::BeginLine
        | Op::EndLine
        | Op::BeginText
        | Op::EndText
        | Op::WordBoundary
        | Op::NoWordBoundary => one(true),
        Op::Capture => {
            let sub = piece(&n.subs[0]);
            Piece {
                insts: sub.insts.saturating_add(2),
                nullable: sub.nullable,
                op: Op::Capture,
                non_greedy: false,
            }
        }
        Op::Star | Op::Plus | Op::Quest => repeat1(n.op, n.flags, piece(&n.subs[0])),
        Op::Repeat => piece_repeat(n),
        Op::Concat => concat(&n.subs.iter().map(piece).collect::<Vec<_>>()),
        Op::Alternate => {
            let subs: Vec<Piece> = n.subs.iter().map(piece).collect();
            let total = subs.iter().fold(0u64, |a, p| a.saturating_add(p.insts));
            Piece {
                insts: total.saturating_add(subs.len().saturating_sub(1) as u64),
                nullable: subs.iter().any(|p| p.nullable),
                op: Op::Alternate,
                non_greedy: false,
            }
        }
        Op::LeftParen | Op::VerticalBar => one(false),
    }
}

/// A counted repetition as the simplifier expands it: `x{2,5}` is `xx(x(x(x)?)?)?`.
fn piece_repeat(n: &Node) -> Piece {
    let (min, max) = (n.min, n.max);
    if min == 0 && max == 0 {
        return EMPTY;
    }
    let sub = piece(&n.subs[0]);
    if max == -1 {
        if min == 0 {
            return repeat1(Op::Star, n.flags, sub);
        }
        if min == 1 {
            return repeat1(Op::Plus, n.flags, sub);
        }
        let mut parts = vec![sub; (min - 1) as usize];
        parts.push(repeat1(Op::Plus, n.flags, sub));
        return concat(&parts);
    }
    if min == 1 && max == 1 {
        return sub;
    }
    let mut parts = Vec::new();
    if min > 0 {
        parts = vec![sub; min as usize];
    }
    if max > min {
        let mut suffix = repeat1(Op::Quest, n.flags, sub);
        for _ in min + 1..max {
            suffix = repeat1(Op::Quest, n.flags, concat(&[sub, suffix]));
        }
        if parts.is_empty() {
            return suffix;
        }
        parts.push(suffix);
    }
    if parts.is_empty() {
        return Piece {
            insts: 1,
            nullable: false,
            op: Op::NoMatch,
            non_greedy: false,
        };
    }
    concat(&parts)
}

/// The number of instructions in the program the reference compiles for `n`, counting the failure and match
/// instructions at the two ends.
pub(super) fn program_len(n: &Node) -> u64 {
    piece(n).insts.saturating_add(2)
}

/// The program length of the pattern wrapped for a search from the middle of the text: one character of
/// context, a lazy run of any characters, then the pattern in a group.
pub(super) fn after_program_len(n: &Node) -> u64 {
    program_len(&Node::after_wrap(n))
}

#[cfg(test)]
mod tests {
    use super::super::syntax::parse;
    use super::*;

    fn len(p: &str) -> u64 {
        program_len(&parse(p.as_bytes()).expect("valid").root)
    }

    #[test]
    fn counts_the_program_as_the_compiler_does() {
        assert_eq!(len("a"), 3);
        assert_eq!(len("abc"), 5);
        assert_eq!(len("a*"), 4);
        assert_eq!(len("(a*)*"), 8);
        assert_eq!(len("a{2,5}"), 10);
        assert_eq!(len("(a)"), 5);
        assert_eq!(len("a|bc"), 2 + 1 + 2 + 1);
    }

    #[test]
    fn estimates_without_expanding() {
        let root = parse(b"(?:a{100}){10}").expect("valid").root;
        assert_eq!(estimate(&root, 1024), 1025);
        let root = parse(b"ab(c|d)").expect("valid").root;
        assert_eq!(estimate(&root, 1024), 2 + 2 + 3);
    }
}

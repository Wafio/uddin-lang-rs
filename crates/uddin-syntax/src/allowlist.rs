//! The allowlist check: which builtin names a program refers to that the host does not allow.

use std::collections::BTreeSet;

use crate::ast::{Block, Expr, Program, Stmt};
use crate::builtins::is_builtin;

/// Returns, sorted and without repeats, every builtin name the program mentions that `allowed` refuses,
/// plus `"import"` when the program has an import statement.
///
/// Any identifier spelled like a forbidden builtin counts, even one the program assigns itself, because a
/// sandbox leaves that name out of scope and a script could not tell the two apart. A bare map key or a
/// name after a dot is a string, not an identifier, and does not count.
///
/// The walk keeps its own stack: a tree can be thousands of levels deep (see `ast`), and recursion over it
/// would need a stack the embedder may not have.
pub fn disallowed_builtins(prog: &Program, allowed: impl Fn(&str) -> bool) -> Vec<String> {
    enum Node<'a> {
        Block(&'a Block),
        Stmt(&'a Stmt),
        Expr(&'a Expr),
    }

    fn note(found: &mut BTreeSet<String>, allowed: &impl Fn(&str) -> bool, name: &str) {
        if is_builtin(name) && !allowed(name) && !found.contains(name) {
            found.insert(name.to_owned());
        }
    }

    let mut found = BTreeSet::new();
    let mut stack = vec![Node::Block(&prog.statements)];
    while let Some(node) = stack.pop() {
        match node {
            Node::Block(block) => stack.extend(block.iter().map(Node::Stmt)),
            Node::Stmt(stmt) => match stmt {
                Stmt::Assign { target, value, .. } => {
                    stack.extend([Node::Expr(target), Node::Expr(value)])
                }
                Stmt::If {
                    cond,
                    body,
                    else_body,
                    ..
                } => {
                    stack.extend([Node::Expr(cond), Node::Block(body), Node::Block(else_body)]);
                }
                Stmt::While { cond, body, .. } => {
                    stack.extend([Node::Expr(cond), Node::Block(body)])
                }
                Stmt::For {
                    name: var,
                    iterable,
                    body,
                    ..
                } => {
                    note(&mut found, &allowed, var);
                    stack.extend([Node::Expr(iterable), Node::Block(body)]);
                }
                Stmt::TryCatch {
                    try_block,
                    error_name,
                    catch_block,
                    ..
                } => {
                    note(&mut found, &allowed, error_name);
                    stack.extend([Node::Block(try_block), Node::Block(catch_block)]);
                }
                Stmt::Return { result, .. } => stack.push(Node::Expr(result)),
                Stmt::Expr { expr, .. } => stack.push(Node::Expr(expr)),
                Stmt::FunDef {
                    name: fun,
                    params,
                    body,
                    ..
                } => {
                    note(&mut found, &allowed, fun);
                    params.iter().for_each(|p| note(&mut found, &allowed, p));
                    stack.push(Node::Block(body));
                }
                Stmt::Import { .. } => {
                    found.insert("import".to_owned());
                }
                Stmt::Break { .. } | Stmt::Continue { .. } => {}
            },
            Node::Expr(expr) => match expr {
                Expr::Literal { .. } => {}
                Expr::Variable { name: var, .. } => note(&mut found, &allowed, var),
                Expr::Binary { left, right, .. } => {
                    stack.extend([Node::Expr(left), Node::Expr(right)])
                }
                Expr::Unary { operand, .. } => stack.push(Node::Expr(operand)),
                Expr::Ternary {
                    cond,
                    then,
                    otherwise,
                    ..
                } => {
                    stack.extend([Node::Expr(cond), Node::Expr(then), Node::Expr(otherwise)]);
                }
                Expr::Call { function, args, .. } => {
                    stack.push(Node::Expr(function));
                    stack.extend(args.iter().map(Node::Expr));
                }
                Expr::List { values, .. } => stack.extend(values.iter().map(Node::Expr)),
                Expr::Map { items, .. } => stack.extend(
                    items
                        .iter()
                        .flat_map(|(k, v)| [Node::Expr(k), Node::Expr(v)]),
                ),
                Expr::Function { params, body, .. } => {
                    params.iter().for_each(|p| note(&mut found, &allowed, p));
                    stack.push(Node::Block(body));
                }
                Expr::Subscript {
                    container, index, ..
                } => stack.extend([Node::Expr(container), Node::Expr(index)]),
            },
        }
    }
    found.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_source;

    fn refused(src: &str, allowed: &[&str]) -> Vec<String> {
        let program = parse_source(src.as_bytes()).unwrap_or_else(|e| panic!("{src:?}: {e}"));
        disallowed_builtins(&program, |n| allowed.contains(&n))
    }

    #[test]
    fn names_the_disallowed_builtins_once_each_in_order() {
        let src = "
fun check(input):
    x = read_file(\"/etc/wafio/certs/waf/agent.key\")
    f = http_post
    y = [sleep, {\"k\": exit}]
    try:
        z = input
    catch (e):
        print(e)
    end
end
import \"lib.din\"
waf_block()
";
        assert_eq!(
            refused(src, &["print", "waf_block"]),
            ["exit", "http_post", "import", "input", "read_file", "sleep"]
        );
        assert!(
            refused(
                "fun f(a):\n    print(a)\nend\nf(1)\nwaf_block()\n",
                &["print", "waf_block"]
            )
            .is_empty()
        );
        assert_eq!(refused("sleep(1) sleep(2) a = sleep", &[]), ["sleep"]);
    }

    #[test]
    fn an_assigned_name_still_counts() {
        assert_eq!(refused("exit = 1", &[]), ["exit"]);
        assert_eq!(refused("a[exit] = 1", &[]), ["exit"]);
        assert_eq!(refused("exit += 1", &[]), ["exit"]);
    }

    #[test]
    fn every_binding_position_counts() {
        assert_eq!(refused("for (input in x): end", &[]), ["input"]);
        assert_eq!(refused("try: x = 1 catch (input): end", &[]), ["input"]);
        assert_eq!(refused("fun f(input): end", &[]), ["input"]);
        assert_eq!(refused("fun input(): end", &[]), ["input"]);
        assert_eq!(refused("g = fun (exit): end", &[]), ["exit"]);
        assert_eq!(refused("memo fun exit(): end", &[]), ["exit"]);
        assert_eq!(
            refused(
                "if (a) then: b = sleep elif (c) then: d = exit else: e = input end",
                &[]
            ),
            ["exit", "input", "sleep"]
        );
        assert_eq!(
            refused("while (sleep) { x = f(exit, y...) }", &[]),
            ["exit", "sleep"]
        );
        assert_eq!(
            refused("x = a ? sleep : (exit or -input)", &[]),
            ["exit", "input", "sleep"]
        );
        assert_eq!(refused("x = {k: sleep}[exit]", &[]), ["exit", "sleep"]);
    }

    #[test]
    fn strings_keys_and_selectors_are_not_identifiers() {
        assert!(refused("x = \"exit\"", &[]).is_empty());
        assert!(refused("x = {exit: 1}", &[]).is_empty());
        assert!(refused("x = a.exit", &[]).is_empty());
        assert!(refused("x = {\"sleep\": 1}", &[]).is_empty());
    }

    #[test]
    fn unknown_names_are_not_builtins() {
        assert!(refused("my_function(1)\nvalue = other", &[]).is_empty());
        assert_eq!(refused("print(1)", &[]), ["print"]);
    }

    #[test]
    fn deep_trees_need_no_recursion() {
        let src = format!("x = 1{}{}", " * 1".repeat(900), " + sleep".repeat(900));
        assert_eq!(refused(&src, &[]), ["sleep"]);
    }
}

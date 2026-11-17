//! Native code that calls script functions: the recursion it allows must fit a small thread stack.

mod common;

use common::{run, table};
use uddin::{LimitReason, Outcome};

/// A function that calls itself through `map`, each level a native frame and two calls of the script.
const RECURSION: &str = "fun f(n):\n    return map([1], fun (x):\n        return f(n + 1)\n    end)\nend\nprint(f(0))\n";

#[test]
fn recursion_through_map_stops_at_the_call_depth_on_a_two_megabyte_stack() {
    let ended = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(|| run(&table(&[]), RECURSION, |_| {}).ended)
        .expect("a thread")
        .join()
        .expect("no overflow");
    match ended {
        Outcome::Limit { reason, .. } => assert_eq!(reason, LimitReason::CallDepth),
        other => panic!("{other:?}"),
    }
}

#[test]
fn recursion_through_filter_and_reduce_stops_the_same_way() {
    for call in ["filter([1], g)", "reduce([1], g, 0)"] {
        let src = format!(
            "fun f(n):\n    g = fun (a, b):\n        return f(n + 1)\n    end\n    h = fun (a):\n        return f(n + 1)\n    end\n    return {}\nend\nprint(f(0))\n",
            call.replace('g', "h")
        );
        let ended = std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(move || run(&table(&[]), &src, |_| {}).ended)
            .expect("a thread")
            .join()
            .expect("no overflow");
        assert!(
            matches!(
                ended,
                Outcome::Limit {
                    reason: LimitReason::CallDepth,
                    ..
                } | Outcome::Error { .. }
            ),
            "{call}: {ended:?}"
        );
    }
}

package main

import (
	"fmt"
	"strings"
)

// The groups below are written by hand, and expanded by loops, for the core of the language: scopes, control
// flow that unwinds, the operators over every pair of kinds of value, text and number formats, and the sandbox
// limits at and around each boundary. They need no builtin beyond print, len, str and typeof.

func coreGroups() []*group {
	return []*group{handGroup("lang", langCases()), handGroup("core-limits", coreLimitCases())}
}

// pool is a value of every kind, as source text. f is a user function, m a map, l a list, e an empty list.
var pool = []string{"1", "(-2)", "2.5", "0", "\"ab\"", "\"b\"", "\"\"", "[1, 2]", "[]", "{a: 1}", "{}", "true", "false", "null", "f", "print"}

const poolPrelude = "fun f():\nend\n"

func tryPrint(expr string) string {
	return "try:\n    print(" + expr + ")\ncatch (e):\n    print(\"caught: \" + e)\nend\n"
}

func tryStmt(stmt string) string {
	return "try:\n    " + stmt + "\ncatch (e):\n    print(\"caught: \" + e)\nend\n"
}

func langCases() []handCase {
	var cs []handCase
	add := func(name, src string) { cs = append(cs, handCase{name, nil, src}) }

	// Scopes: where an assignment goes, what a read finds, and what a function takes with it.
	add("scope/function_assigns_local", "x = 1\nfun f():\n    x = 2\n    print(x)\nend\nf()\nprint(x)\n")
	add("scope/function_reads_global", "x = 1\nfun f():\n    return x\nend\nprint(f())\n")
	add("scope/named_function_copies_at_definition", "x = 1\nfun f():\n    return x\nend\nx = 2\nprint(f())\nprint(x)\n")
	add("scope/anonymous_function_sees_live_scope", "x = 1\ng = fun ():\n    return x\nend\nx = 2\nprint(g())\n")
	add("scope/anonymous_in_function", "fun mk():\n    n = 0\n    return fun ():\n        n += 1\n        return n\n    end\nend\nc = mk()\nprint(c())\nprint(c())\nd = mk()\nprint(d())\nprint(c())\n")
	add("scope/outer_wins_in_snapshot", "x = \"global\"\nfun outer():\n    x = \"outer\"\n    fun inner():\n        return x\n    end\n    return inner()\nend\nprint(outer())\n")
	add("scope/snapshot_has_itself", "fun fact(n):\n    if (n < 2) then:\n        return 1\n    end\n    return n * fact(n - 1)\nend\nprint(fact(10))\n")
	add("scope/function_defined_after_use_fails", tryStmt("print(later())")+"fun later():\n    return 1\nend\nprint(later())\n")
	add("scope/function_sees_later_global_through_caller", "fun f():\n    return y\nend\ny = 5\nprint(f())\n")
	add("scope/caller_locals_visible", "fun g():\n    return secret\nend\nfun f():\n    secret = 42\n    return g()\nend\nprint(f())\n")
	add("scope/caller_locals_beat_globals", "x = 1\nfun g():\n    return x\nend\nfun f():\n    x = 2\n    return g()\nend\nprint(f())\n")
	add("scope/try_assignment_is_local", "x = 1\ntry:\n    x = 2\n    y = 3\ncatch (e):\nend\nprint(x)\n"+tryPrint("y"))
	add("scope/catch_variable_is_local", "try:\n    z = 1 / 0\ncatch (e):\n    print(\"caught\")\nend\n"+tryPrint("e"))
	add("scope/catch_assignment_is_local", "x = 1\ntry:\n    q = 1 / 0\ncatch (e):\n    x = 2\n    w = 3\nend\nprint(x)\n"+tryPrint("w"))
	add("scope/for_variable_stays", "for (i in [1, 2, 3]):\nend\nprint(i)\n")
	add("scope/for_variable_in_function", "fun f():\n    for (i in [1, 2]):\n    end\n    return i\nend\nprint(f())\n"+tryPrint("i"))
	add("scope/loop_body_assignments_stay", "n = 0\nwhile (n < 3):\n    last = n\n    n += 1\nend\nprint(last)\n")
	add("scope/shadowing_a_builtin", "len = 5\nprint(len)\n"+tryPrint("len(\"abc\")"))
	add("scope/shadowing_in_function_keeps_global_builtin", "fun f():\n    return len(\"abc\")\nend\nlen = 5\nprint(f())\nprint(len)\n")
	add("scope/shadowed_builtin_restored_by_other_name", "p = print\nprint = 7\np(print)\n")
	add("scope/constant_names", "print(PI)\nPI = 3\nprint(PI)\nprint(E, TAU, LN2, LN10, SQRT2, SQRT3, PHI)\n")
	add("scope/parameter_shadows", "x = 1\nfun f(x):\n    x = x + 1\n    return x\nend\nprint(f(10))\nprint(x)\n")
	add("scope/parameter_named_like_function", "fun f(f):\n    return f\nend\nprint(f(3))\nprint(typeof(f))\n")
	add("scope/closure_over_loop_variable", "fs = []\nfor (i in [1, 2, 3]):\n    fs = fs + [fun ():\n        return i\n    end]\nend\nprint(fs[0](), fs[1](), fs[2]())\n")
	add("scope/closure_over_parameter", "fun adder(n):\n    return fun (m):\n        return n + m\n    end\nend\nprint(adder(1)(2))\na = adder(10)\nprint(a(1), a(2))\n")
	add("scope/closure_mutates_enclosing", "fun counter():\n    n = 0\n    inc = fun ():\n        n = n + 1\n    end\n    inc()\n    inc()\n    return n\nend\nprint(counter())\n")
	add("scope/nested_function_definitions", "fun a():\n    fun b():\n        fun c():\n            return 1\n        end\n        return c() + 1\n    end\n    return b() + 1\nend\nprint(a())\n")
	add("scope/main_runs_after_top_level", "print(\"top\")\nfun main():\n    print(\"main\")\nend\nprint(\"end of top\")\n")
	add("scope/main_with_parameter", "fun main(a):\n    print(a)\nend\nprint(\"top\")\n")
	add("scope/main_not_a_function", "main = 5\nprint(\"top\")\n")
	add("scope/main_error", "fun main():\n    return 1 / 0\nend\nprint(\"top\")\n")
	add("scope/main_return", "fun main():\n    return 5\nend\n")

	// The scope that a return leaves behind when it unwinds through a try block.
	add("scope/return_in_try", "fun f():\n    local = 1\n    try:\n        return local\n    catch (e):\n        return 0\n    end\nend\nprint(f())\nz = 10\nprint(z)\nprint(local)\n")
	add("scope/return_in_try_then_assign", "fun f():\n    try:\n        return 1\n    catch (e):\n    end\nend\nf()\nz = 10\nfun h():\n    return z\nend\nprint(h())\nf()\nprint(z)\n")
	add("scope/return_in_catch", "fun f():\n    try:\n        x = 1 / 0\n    catch (e):\n        return \"from catch\"\n    end\nend\nprint(f())\ny = 3\nprint(y)\n")
	add("scope/return_in_try_in_loop", "fun f():\n    for (i in [1, 2, 3]):\n        try:\n            if (i == 2) then:\n                return i\n            end\n        catch (e):\n        end\n    end\nend\nprint(f())\nprint(f())\nk = 1\nprint(k)\n")
	add("scope/return_in_nested_try", "fun f():\n    try:\n        try:\n            return 1\n        catch (e):\n        end\n    catch (e):\n    end\nend\nprint(f())\nq = 2\nprint(q)\n")
	add("scope/return_in_try_many_calls", "fun f(n):\n    try:\n        return n\n    catch (e):\n    end\nend\ni = 0\nwhile (i < 40):\n    f(i)\n    i += 1\nend\nprint(i)\nx = 5\nprint(x)\n")
	add("scope/return_in_try_assign_in_caller", "fun f():\n    try:\n        return 1\n    catch (e):\n    end\nend\nfun g():\n    f()\n    leak = 1\n    return leak\nend\nprint(g())\nprint(leak)\n")
	add("scope/error_in_catch_leaks", "try:\n    try:\n        x = 1 / 0\n    catch (e):\n        y = 1 / 0\n    end\ncatch (e2):\n    print(e2)\nend\nz = 1\nprint(z)\n")

	// Unwinding: break, continue, return and errors against loops, calls and try blocks.
	add("flow/break_in_function_breaks_callers_loop", "fun stop():\n    break\nend\nfor (i in [1, 2, 3]):\n    print(i)\n    stop()\nend\nprint(\"after\")\n")
	add("flow/continue_in_function", "fun skip():\n    continue\nend\nfor (i in [1, 2, 3]):\n    skip()\n    print(i)\nend\nprint(\"after\")\n")
	add("flow/break_in_try_is_caught", "for (i in [1, 2, 3]):\n    try:\n        break\n    catch (e):\n        print(e)\n    end\nend\nprint(\"after\")\n")
	add("flow/continue_in_try_is_caught", "n = 0\nwhile (n < 2):\n    n += 1\n    try:\n        continue\n    catch (e):\n        print(e)\n    end\n    print(\"body\", n)\nend\n")
	add("flow/break_outside_loop", "print(\"a\")\nbreak\nprint(\"b\")\n")
	add("flow/continue_outside_loop", "print(\"a\")\ncontinue\n")
	add("flow/break_in_function_outside_loop", "fun f():\n    break\nend\nf()\nprint(\"after\")\n")
	add("flow/break_in_if", "i = 0\nwhile (true):\n    i += 1\n    if (i > 3) then:\n        break\n    end\nend\nprint(i)\n")
	add("flow/nested_loops_break_inner", "for (i in [1, 2]):\n    for (j in [1, 2, 3]):\n        if (j == 2) then:\n            break\n        end\n        print(i, j)\n    end\nend\n")
	add("flow/nested_loops_continue_inner", "for (i in [1, 2]):\n    for (j in [1, 2, 3]):\n        if (j == 2) then:\n            continue\n        end\n        print(i, j)\n    end\nend\n")
	add("flow/break_in_condition_function", "fun f():\n    break\nend\nwhile (true):\n    while (f()):\n    end\nend\nprint(\"x\")\n")
	add("flow/break_in_iterable_function", "fun f():\n    break\nend\nwhile (true):\n    for (i in f()):\n    end\nend\nprint(\"x\")\n")
	add("flow/return_in_loop", "fun f():\n    for (i in [1, 2, 3]):\n        if (i == 2) then:\n            return i\n        end\n    end\n    return 0\nend\nprint(f())\n")
	add("flow/return_at_top_level", "print(\"a\")\nreturn 5\n")
	add("flow/return_in_nested_blocks_at_top", "if (true) then:\n    while (true):\n        return 1\n    end\nend\n")
	add("flow/no_return_is_null", "fun f():\nend\nprint(f())\nprint(typeof(f()))\n")
	add("flow/return_null", "fun f():\n    return null\nend\nprint(f())\n")
	add("flow/error_in_called_function_caught", "fun f():\n    return 1 / 0\nend\ntry:\n    f()\ncatch (e):\n    print(e)\nend\nprint(\"after\")\n")
	add("flow/error_after_partial_work", "x = []\ntry:\n    x = x + [1]\n    x = x + [1 / 0]\ncatch (e):\n    print(e)\nend\nprint(x)\n")
	add("flow/rethrow", "try:\n    try:\n        x = 1 / 0\n    catch (e):\n        y = [][1]\n    end\ncatch (e):\n    print(e)\nend\n")
	add("flow/catch_text_has_position", "try:\n    x = undefined_name\ncatch (e):\n    print(e)\nend\n")
	add("flow/catch_in_loop_continues", "for (i in [1, 0, 2]):\n    try:\n        print(10 / i)\n    catch (e):\n        print(\"err\")\n    end\nend\n")
	add("flow/catch_variable_is_string", "try:\n    x = 1 / 0\ncatch (e):\n    print(typeof(e), len(e) > 0)\nend\n")
	add("flow/catch_long_message_cut", "try:\n    x = "+strings.Repeat("a", 1200)+"\ncatch (e):\n    print(len(e))\n    print(str([e]) == str([e + \"\"]))\nend\n")
	add("flow/catch_message_at_the_cut", "try:\n    x = "+strings.Repeat("a", 990)+"\ncatch (e):\n    print(len(e))\nend\ntry:\n    x = "+strings.Repeat("a", 1000)+"\ncatch (e):\n    print(len(e))\nend\ntry:\n    x = "+strings.Repeat("a", 1008)+"\ncatch (e):\n    print(len(e))\nend\n")
	add("flow/many_catches_charge_memory", "i = 0\nwhile (i < 5000):\n    try:\n        x = 1 / 0\n    catch (e):\n        s = e\n    end\n    i += 1\nend\nprint(i)\n")
	add("flow/while_with_break_and_else", "i = 0\nwhile (i < 10):\n    i += 1\n    if (i == 3) then:\n        continue\n    elif (i == 6) then:\n        break\n    else:\n        print(i)\n    end\nend\nprint(\"done\", i)\n")
	add("flow/if_chain", "fun cls(n):\n    if (n < 0) then:\n        return \"neg\"\n    elif (n == 0) then:\n        return \"zero\"\n    elif (n < 10) then:\n        return \"small\"\n    else:\n        return \"big\"\n    end\nend\nprint(cls(-1), cls(0), cls(5), cls(50))\n")
	add("flow/short_circuit", "fun t():\n    print(\"t\")\n    return true\nend\nfun f():\n    print(\"f\")\n    return false\nend\nprint(f() and t())\nprint(t() or f())\nprint(t() and f())\nprint(f() or t())\nprint(t() xor t())\nprint(f() xor t())\n")
	add("flow/ternary_laziness", "fun p(x):\n    print(\"eval\", x)\n    return x\nend\nprint(true ? p(1) : p(2))\nprint(false ? p(1) : p(2))\nprint(0 ? \"y\" : \"n\")\nprint(\"\" ? \"y\" : \"n\")\nprint([] ? \"y\" : \"n\")\nprint({} ? \"y\" : \"n\")\nprint(null ? \"y\" : \"n\")\nprint(0.0 ? \"y\" : \"n\")\n")
	add("flow/condition_types", tryStmt("if (1) then:\n        print(1)\n    end")+tryStmt("if (\"a\") then:\n        print(1)\n    end")+tryStmt("if (null) then:\n        print(1)\n    end")+tryStmt("if ([]) then:\n        print(1)\n    end")+tryStmt("if (print) then:\n        print(1)\n    end")+tryStmt("while (1):\n        break\n    end")+tryStmt("while (\"a\"):\n        break\n    end")+tryStmt("while ([]):\n        break\n    end")+tryStmt("while ({}):\n        break\n    end")+tryStmt("while (null):\n        break\n    end")+tryStmt("while (1.5):\n        break\n    end")+tryStmt("while (f):\n        break\n    end")+tryStmt("while (print):\n        break\n    end"))

	// Calls: arity, variadic functions, spread.
	add("call/arity_errors", "fun f(a):\n    return a\nend\nfun g(a, b):\n    return a\nend\nh = fun ():\n    return 1\nend\n"+tryPrint("f()")+tryPrint("f(1, 2)")+tryPrint("g(1)")+tryPrint("h(1)")+tryPrint("len()")+tryPrint("len(1, 2)")+tryPrint("str()")+tryPrint("typeof(1, 2)")+tryPrint("typeof()"))
	add("call/variadic", "fun v(a, rest...):\n    return [a, rest]\nend\nprint(v(1))\nprint(v(1, 2))\nprint(v(1, 2, 3, 4))\n"+tryPrint("v()"))
	add("call/variadic_only", "fun v(rest...):\n    return rest\nend\nprint(v())\nprint(v(1, 2))\nprint(len(v(1, 2, 3)))\n")
	add("call/variadic_two_fixed", "fun v(a, b, rest...):\n    return [a, b, rest]\nend\nprint(v(1, 2))\nprint(v(1, 2, 3))\n"+tryPrint("v(1)")+tryPrint("v()"))
	add("call/spread_list", "fun add3(a, b, c):\n    return a + b + c\nend\nxs = [1, 2, 3]\nprint(add3(xs...))\nprint(add3(0, [1, 2]...))\n"+tryPrint("add3([1, 2, 3, 4]...)")+tryPrint("add3([]...)"))
	add("call/spread_string", "fun two(a, b):\n    return a + b\nend\nprint(two(\"hi\"...))\nprint(two(\"h\u00e9\"...))\n"+tryPrint("two(\"abc\"...)"))
	add("call/spread_map", "fun two(a, b):\n    return [a, b]\nend\nprint(two({x: 1, y: 2}...) == [\"x\", \"y\"] or two({x: 1, y: 2}...) == [\"y\", \"x\"])\n")
	add("call/spread_invalid", tryPrint("str((5)...)")+tryPrint("str(null...)")+tryPrint("str(true...)")+tryPrint("str(f...)")+"fun f():\nend\n")
	add("call/spread_into_builtin", "print([1, 2, 3]...)\nprint(\"ab\"...)\nprint([]...)\nprint(len([[1, 2]]...))\n")
	add("call/spread_variadic", "fun v(a, rest...):\n    return rest\nend\nprint(v([1, 2, 3]...))\nprint(v(1, [2, 3]...))\n")
	add("call/non_function", tryPrint("1(2)")+tryPrint("\"a\"()")+tryPrint("[1]()")+tryPrint("{a: 1}()")+tryPrint("null()")+tryPrint("true()")+tryPrint("1.5()"))
	add("call/call_result", "fun mk():\n    return fun (x):\n        return x * 2\n    end\nend\nprint(mk()(21))\nprint(mk()(mk()(2)))\n")
	add("call/recursion_depth", "fun f(n):\n    if (n == 0) then:\n        return 0\n    end\n    return 1 + f(n - 1)\nend\nprint(f(10))\nprint(f(30))\n")
	add("call/mutual_recursion", "fun even(n):\n    if (n == 0) then:\n        return true\n    end\n    return odd(n - 1)\nend\nfun odd(n):\n    if (n == 0) then:\n        return false\n    end\n    return even(n - 1)\nend\nprint(even(10), odd(7), even(7))\n")
	add("call/function_values", "fun f():\n    return 1\nend\ng = f\nprint(g())\nprint(f == g)\nprint(f == f)\nh = fun ():\n    return 1\nend\nprint(f == h)\nprint(typeof(f), typeof(print), typeof(h))\nprint(str(f), str(h), str(print))\n")
	add("call/builtin_as_value", "p = print\np(1, 2)\nl = len\nprint(l(\"abc\"))\nfs = [len, str, typeof]\nprint(fs[1](5))\nprint(typeof(fs))\n")
	add("call/builtin_equality", tryPrint("print == print")+tryPrint("print == len")+tryPrint("print == 1")+tryPrint("1 == print")+tryPrint("f == print")+tryPrint("print == f")+tryPrint("[print] == [print]")+tryPrint("print in [print]")+tryPrint("1 in [print]")+"fun f():\nend\n")

	// Operators over every pair of kinds of value; each pair in its own try block.
	for _, op := range []string{"+", "-", "*", "/", "%", "**", "==", "!=", "<", "<=", ">", ">=", "in", "and", "or", "xor"} {
		var b strings.Builder
		b.WriteString(poolPrelude)
		for _, l := range pool {
			for _, r := range pool {
				b.WriteString(tryPrint(fmt.Sprintf("str(%s %s %s)", l, op, r)))
			}
		}
		name := map[string]string{"+": "plus", "-": "minus", "*": "times", "/": "divide", "%": "modulo", "**": "power", "==": "eq", "!=": "ne", "<": "lt", "<=": "le", ">": "gt", ">=": "ge", "in": "in", "and": "and", "or": "or", "xor": "xor"}[op]
		add("matrix/binary_"+name, b.String())
	}
	var un strings.Builder
	un.WriteString(poolPrelude)
	for _, v := range pool {
		un.WriteString(tryPrint("str(-" + v + ")"))
		un.WriteString(tryPrint("str(not " + v + ")"))
		un.WriteString(tryPrint("str(" + v + " ? \"t\" : \"f\")"))
		un.WriteString(tryPrint("typeof(" + v + ")"))
		un.WriteString(tryPrint("str(" + v + ")"))
		un.WriteString(tryPrint("len(" + v + ")"))
	}
	add("matrix/unary_and_conversions", un.String())
	var sub strings.Builder
	sub.WriteString(poolPrelude)
	for _, c := range []string{"\"héllo\"", "[10, 20, 30]", "{a: 1, \"b c\": 2}", "5", "null", "f"} {
		for _, i := range []string{"0", "1", "2", "(-1)", "(-3)", "(-4)", "3", "100", "\"a\"", "\"b c\"", "\"z\"", "1.5", "null", "true", "[0]"} {
			sub.WriteString(tryPrint("str(" + c + "[" + i + "])"))
		}
	}
	add("matrix/subscript_read", sub.String())
	var set strings.Builder
	set.WriteString(poolPrelude)
	for _, c := range []string{"[10, 20, 30]", "{a: 1}", "\"abc\"", "5", "null", "f"} {
		for _, i := range []string{"0", "2", "3", "(-1)", "\"a\"", "\"n\"", "1.5", "null"} {
			set.WriteString("c = " + c + "\n")
			set.WriteString(tryStmt("c[" + i + "] = 99\n    print(c)"))
		}
	}
	add("matrix/subscript_write", set.String())
	var cmp strings.Builder
	cmp.WriteString(poolPrelude)
	for _, op := range []string{"+=", "-=", "*=", "/=", "%="} {
		for _, v := range []string{"7", "2.5", "\"ab\"", "[1]", "{a: 1}", "null", "0"} {
			for _, r := range []string{"2", "0", "1.5", "\"x\"", "[2]", "{b: 2}", "null"} {
				cmp.WriteString("x = " + v + "\n")
				cmp.WriteString(tryStmt("x " + op + " " + r + "\n    print(x)"))
			}
		}
	}
	add("matrix/compound_assignment", cmp.String())
	var cmps strings.Builder
	for _, op := range []string{"+=", "*=", "-="} {
		cmps.WriteString("l = [1, 2, 3]\nm = {a: 1}\ns = \"abc\"\n")
		cmps.WriteString(tryStmt("l[0] " + op + " 5\n    print(l)"))
		cmps.WriteString(tryStmt("l[(-1)] " + op + " 5\n    print(l)"))
		cmps.WriteString(tryStmt("l[7] " + op + " 5\n    print(l)"))
		cmps.WriteString(tryStmt("m.a " + op + " 5\n    print(m)"))
		cmps.WriteString(tryStmt("m.zz " + op + " 5\n    print(m)"))
		cmps.WriteString(tryStmt("s[0] " + op + " 5\n    print(s)"))
	}
	add("matrix/compound_subscript", cmps.String())
	var fr strings.Builder
	fr.WriteString(poolPrelude)
	for _, v := range pool {
		fr.WriteString(tryStmt("for (x in " + v + "):\n        print(x)\n    end"))
	}
	add("matrix/for_over_each", fr.String())
	var pr strings.Builder
	pr.WriteString(poolPrelude)
	for _, a := range pool {
		pr.WriteString("print(" + a + ")\n")
		pr.WriteString("print(" + a + ", " + a + ")\n")
		pr.WriteString("print(str([" + a + "]))\n")
		pr.WriteString("print(str({k: " + a + "}))\n")
	}
	add("matrix/print_and_str", pr.String())

	// Numbers: wrapping, truncation, text.
	add("num/int_edges", "m = 9223372036854775807\nprint(m, m + 1, -m - 1, -m - 2)\nlo = -m - 1\nprint(lo, lo / -1, lo % -1, lo * -1, -lo, lo - 1, lo * lo)\nprint(str(lo), typeof(lo))\nprint(m * 2, m * m)\n")
	add("num/division", "print(7 / 2, -7 / 2, 7 / -2, -7 / -2, 0 / 5, 6 / 3)\nprint(7.0 / 2, 7 / 2.0, 1 / 3, 2 / 3, 10 / 4.0)\n"+tryPrint("1 / 0")+tryPrint("1.5 / 0")+tryPrint("0 / 0")+tryPrint("1 / 0.0")+tryPrint("0.0 / 0")+tryPrint("0.0 / 0.0"))
	add("num/modulo", "print(7 % 3, -7 % 3, 7 % -3, -7 % -3, 0 % 5, 5 % 5)\nprint(7.5 % 2, 7 % 2.5, -7.5 % 2, 7.9 % 2.9, 5.0 % 3)\n"+tryPrint("1 % 0")+tryPrint("1.5 % 0")+tryPrint("1 % 0.0")+tryPrint("1 % 1.5")+tryPrint("1 % 0.9")+tryPrint("5 % 0.5")+tryPrint("0.5 % 2")+tryPrint("1e1 % 3"))
	add("num/power", "print(2 ** 0, 2 ** 1, 2 ** 2, 2 ** 3, 2 ** 4, 2 ** 10, 2 ** 62, 2 ** 63, 2 ** 64)\nprint(10 ** 18, 10 ** 19, 10 ** 20, (-2) ** 3, (-2) ** 4, 3 ** 3, 3 ** 4)\nprint(2 ** -1, 2 ** -2, 2 ** 0.5, 4 ** 0.5, 8 ** (1 / 3), 2.0 ** 3, 2 ** 3.0, 0 ** 0, 0 ** 1, 0 ** -1, 1 ** 1000, (-1) ** 1001)\nprint(9223372036854775807 ** 2, 9223372036854775807 ** 3, 10 ** 308, 10 ** 309, 10 ** (-5), 1.0000001 ** 1000000)\n"+tryPrint("2 ** \"a\"")+tryPrint("2 ** null"))
	add("num/power_fractional", "print(2 ** 0.3, 10 ** 2.5, 7 ** 1.7, 0.5 ** 1.5, 100 ** 0.1, 1.5 ** 2.5, 3 ** 0.333, 2 ** 100.5, 2 ** (-100.5), 1e10 ** 0.5, 5 ** 0.25)\nprint(str(2 ** 0.3), str(10 ** 2.5), str(7 ** 1.7))\n")
	add("num/nan_and_inf", "inf = 10 ** 400\nnan = inf - inf\nprint(inf, -inf, nan)\nprint(str(inf), str(-inf), str(nan))\nprint(nan == nan, nan != nan, nan < 1, nan > 1, nan <= 1, nan >= 1, 1 < nan, 1 >= nan)\nprint(inf == inf, inf > 1, -inf < 1, inf - 1, inf * 0)\nprint([nan] == [nan], [inf] == [inf], [nan] < [1], nan in [nan])\nprint(typeof(nan), nan ? 1 : 2, inf ? 1 : 2)\nprint(7 % inf, inf % 7, nan % 7)\n")
	add("num/float_text", "print(0.1 + 0.2, 1.0, 100.0, 1e6, 123456789.0, 0.000001, 1.5e-7)\nprint(str(0.1 + 0.2), str(1.0), str(100.0), str(1.5), str(1e6), str(1e20), str(1e21), str(123456789.123456789))\nprint(0.5, 1.5, 3.14159265358979, 2.718281828, 1234567.0, 12345678.9, 0.0001234, 100000.5)\nprint(-0.0, 0 * -1.0, str(-0.0), str(0.0), 0.0 == -0.0)\nprint(1e15, 1e16, 1e17, 1e18, 1e19, 1e20, 123456789012345678.0, 9007199254740993.0)\nprint(str(1e15), str(1e16), str(123456789012345678.0), str(5e-324), str(1.7976931348623157e308))\nprint(float_big, 1)\n"+"")
	add("num/float_int_equality", "print(1 == 1.0, 1.0 == 1, 0 == 0.0, 0 == -0.0, 1 < 1.5, 2 > 1.5, 9007199254740993 == 9007199254740992.0)\nprint(1 in [1.0], 1.0 in [1], 2 in [1, 2])\nprint([1, 2] == [1.0, 2.0], {a: 1} == {a: 1.0})\n")
	add("num/mixed_arithmetic", "print(1 + 1.5, 1.5 + 1, 2 * 2.5, 2.5 * 2, 5 - 2.5, 2.5 - 5, 3 + 4, 3 - 4, 3 * 4)\nprint(typeof(1 + 1), typeof(1 + 1.0), typeof(2 * 2), typeof(2.0 * 2), typeof(4 / 2), typeof(4 / 2.0), typeof(2 ** 3), typeof(2 ** -1), typeof(7 % 2), typeof(7.0 % 2))\n")
	add("num/big_literals", "print(9223372036854775807, 1.0e3, 0.5)\n")
	for i, lit := range []string{"1_000", "0x10", "0b101", "0o17", "1e3", "1E3", "1e-3", "5.", ".5", "1.5e+3", "09", "007", "9223372036854775808", "1__0", "0x", "1e", "1.2.3"} {
		add(fmt.Sprintf("num/literal_%d", i), "print("+lit+")\n")
	}
	add("num/print_vs_str_floats", "xs = [0.5, 100.0, 1e15, 1e16, 1e18, 1e19, 0.000001, 123456.789, 1234567.891, 0.1]\nfor (x in xs):\n    print(x, str(x))\nend\n")
	add("num/int_float_conversion_in_print", "print(1e18, 9.2e18, 9.3e18, -9.2e18, -9.3e18, 2 ** 62 * 1.0, 2 ** 63 * 1.0, -(2 ** 63) * 1.0)\nprint(str(-9223372036854775807 - 1))\nprint(-9223372036854775807 - 1)\nprint(-9223372036854775807 - 1, 1)\nprint([-9223372036854775807 - 1])\n")

	// Strings, lists, maps.
	add("text/index_and_unicode", "s = \"h\u00e9llo w\u00f6rld \u65e5\u672c\"\nprint(len(s))\nprint(s[0], s[1] == \"\", s[-1] == \"\", len(s[1]))\nfor (c in s):\n    print(c, len(c))\nend\nprint(str([s[1]]))\nprint(s[1] + s[2] == \"\u00e9\")\n")
	add("text/quoting_in_containers", "print(str([\"a\\\"b\", \"tab\\t\", \"nl\\n\", \"back\\\\slash\", \"\u00e9\", \"\u65e5\", \"\\r\"]))\nprint(str({\"k\\\"ey\": 1, \"\u00e9\": 2, \"b\": [\"x\"], \"B\": 3, \"a b\": 4, \"10\": 5, \"9\": 6}))\n")
	add("text/raw_and_escapes", "print(`raw \\n \"q\" `)\nprint('single \\' quote')\nprint(\"a\\tb\\\\c\\\"d\")\nprint(len(`\\n`), len(\"\\n\"))\n")
	add("text/nested_structures", "x = [1, [2, [3, [4]]], {a: [5, {b: 6}]}, \"s\", null, true, 1.5, f]\nprint(x)\nprint(str(x))\nprint(len(x), len(x[1]), len(x[2]))\nfun f():\nend\n")
	add("text/map_order_in_str", "m = {z: 1, a: 2, m: 3, \"A\": 4, \"_\": 5, \"1\": 6}\nprint(str(m))\nprint(m)\nm.q = 7\nprint(m)\n")
	add("text/map_key_equal_to_names", "m = {a: 1}\nprint(m.a, m[\"a\"], \"a\" in m, \"b\" in m)\nm.b = 2\nm[\"c\"] = 3\nprint(m)\nprint(len(m))\n"+tryPrint("m.zz")+tryPrint("m[\"x y\"]")+tryPrint("m[1]")+tryPrint("m[null]"))
	add("text/map_key_quoting_in_errors", tryPrint("{}[\"a\\\"b\"]")+tryPrint("{}[\"\\n\\t\"]")+tryPrint("{}[\"\u00e9\u65e5\"]")+tryPrint("{}[\""+strings.Repeat("k", 200)+"\"]")+tryPrint("{}[\""+strings.Repeat("\u00e9", 100)+"\"]"))
	add("text/list_aliasing", "a = [1, 2, 3]\nb = a\nb[0] = 9\nprint(a, b)\nc = a + []\nc[1] = 8\nprint(a, c)\nd = [a, a]\nd[0][2] = 7\nprint(a, d)\nm = {l: a}\nm.l[0] = 0\nprint(a, m)\nn = m + {}\nn.l[1] = -1\nprint(a, m, n)\nn.x = 1\nprint(m, n)\n")
	add("text/list_ops", "a = [1, 2]\nprint(a + [3], a * 3, [] + [], [] * 5, [1] * 0, 2 * [7])\nprint([1, 2] < [1, 3], [1, 2] < [1, 2, 0], [] < [1], [2] < [1, 5], [[1]] < [[2]], [\"a\"] < [\"b\"], [1, \"a\"] < [1, \"b\"])\nprint([1, [2]] == [1, [2]], [1] == [1, 2], [] == [], [] == {}, [{a: 1}] == [{a: 1}])\nprint(1 in [1, 2], \"a\" in [\"a\"], [1] in [[1]], null in [null], 3 in [])\n"+tryPrint("[1] < [\"a\"]")+tryPrint("[[1]] < [[\"a\"]]")+tryPrint("[1, 2] < 3"))
	add("text/map_ops", "print({a: 1} + {b: 2}, {a: 1} + {a: 2}, {} + {}, {a: 1} == {a: 1}, {a: 1} == {a: 2}, {a: 1} == {b: 1}, {a: null} == {b: null}, {a: 1, b: 2} == {b: 2, a: 1})\nprint(\"a\" in {a: 1}, \"b\" in {a: 1}, \"\" in {})\n"+tryPrint("1 in {a: 1}")+tryPrint("{a: 1} < {a: 2}"))
	add("text/string_ops", "print(\"a\" + \"b\", \"ab\" * 3, 3 * \"ab\", \"\" * 5, \"ab\" * 0, \"a\" < \"b\", \"a\" < \"B\", \"abc\" < \"abd\", \"\" < \"a\", \"b\" > \"a\", \"a\" <= \"a\", \"h\u00e9\" < \"hf\")\nprint(\"b\" in \"abc\", \"\" in \"abc\", \"abc\" in \"ab\", \"\u00e9\" in \"h\u00e9\", \"a\" in \"\")\n"+tryPrint("\"a\" * (-1)")+tryPrint("[1] * (-1)")+tryPrint("1 in \"a\"")+tryPrint("null in \"a\"")+tryPrint("\"a\" + 1")+tryPrint("1 + \"a\""))
	add("text/object_keys", tryPrint("{1: 2}")+tryPrint("{null: 2}")+tryPrint("{[1]: 2}")+tryPrint("{(1 + 1): 2}")+tryPrint("{(\"a\" + \"b\"): 2}")+tryPrint("{f: 1}")+"fun f():\nend\nprint({a: 1, a: 2})\nprint(len({a: 1, a: 2, b: 3}))\n")
	add("text/many_keys", func() string {
		var b strings.Builder
		b.WriteString("m = {}\n")
		for i := 0; i < 40; i++ {
			fmt.Fprintf(&b, "m[\"key%d\"] = %d\n", i, i)
		}
		b.WriteString("print(len(m), m.key0, m.key17, m.key39)\n")
		b.WriteString("m.key17 = \"changed\"\nprint(m.key17, len(m), \"key39\" in m, \"key40\" in m)\n")
		b.WriteString("n = 0\nfor (k in m):\n    n += 1\nend\nprint(n)\n")
		b.WriteString("print(m == m + {})\n")
		return b.String()
	}())
	add("text/typeof_all", "fun f():\nend\nprint(typeof(1), typeof(1.5), typeof(\"a\"), typeof(true), typeof(null), typeof([]), typeof({}), typeof(f), typeof(print), typeof(fun ():\nend), typeof(_waf_ctx))\n")

	// Evaluation order and the argument checks of the builtins.
	add("order/binary_operands_left_to_right", "fun p(x):\n    print(x)\n    return x\nend\nprint(p(1) + p(2) * p(3))\nprint(p(\"a\") + p(\"b\"))\n"+tryPrint("p(1) + p(\"x\")"))
	add("order/call_arguments", "fun p(x):\n    print(x)\n    return x\nend\nfun f(a, b, c):\n    return a\nend\nf(p(1), p(2), p(3))\n"+tryPrint("p(1)(p(2))")+tryPrint("undefined_fn(p(5))"))
	add("order/assignment_value_then_target", "fun p(x):\n    print(x)\n    return x\nend\na = [0, 0]\na[p(1)] = p(5)\nprint(a)\nx = 1\nx += p(4)\nprint(x)\na[p(0)] += p(3)\nprint(a)\n")
	add("order/map_literal", "fun p(x):\n    print(x)\n    return x\nend\nm = {a: p(1), b: p(2)}\nm2 = {(p(\"c\")): p(3), (p(\"d\")): p(4)}\n"+tryPrint("{(p(\"c\")): p(3), (p(7)): p(4), (p(\"e\")): p(5)}"))
	add("order/compound_name_error", "x += 1\n")
	add("order/compound_name_error_caught", tryStmt("x += undefined_value")+tryStmt("x += 1"))
	add("order/subscript_assign_errors", "a = [1]\n"+tryStmt("a[5] = 1")+tryStmt("a[\"x\"] = 1")+tryStmt("undefined_target[0] = 1")+tryStmt("a[0][0] = 1")+tryStmt("a[undefined_index] = 1")+tryStmt("a[0] = undefined_value"))
	add("order/builtin_argument_errors", tryPrint("len(1)")+tryPrint("len(null)")+tryPrint("len(true)")+tryPrint("len(f)")+tryPrint("len(len)")+tryPrint("len(1.5)")+tryPrint("len([1], [2])")+"fun f():\nend\n")
	add("order/print_forms", "print()\nprint(\"\")\nprint(\"\", \"\")\nprint(1, \"a\", 2.5, true, null, [1], {a: 1})\nprint(\"multi\\nline\")\nprint(\"a\", [\"b\"], \"c\")\n")
	add("order/comments_and_layout", "// line comment\nx = 1 /* block */ + 2\n/* multi\nline */ print(x) // trailing\nif (x == 3) then: print(\"three\") end\nwhile (x < 5): x += 1 end\nprint(x)\n")
	add("order/braces_if", "if (true) { print(1) } else { print(2) }\n")
	add("order/braces_try", "try { print(3 / 0) } catch (e) { print(\"c\") }\n")
	add("order/braces_while", "i = 0\nwhile (i < 2) { i += 1 }\nprint(i)\n")
	add("order/braces_for_fun", "fun f(a) { return a * 2 }\nfor (i in [1, 2]) { print(f(i)) }\n")
	add("order/chained_subscripts_and_calls", "m = {f: fun (x):\n    return [x, {k: [x * 2]}]\nend}\nprint(m.f(3)[1].k[0])\nprint(m[\"f\"](1)[0])\n")
	add("order/strings_as_subscript_on_calls", "fun f():\n    return \"abc\"\nend\nprint(f()[1], f()[-1])\n")

	// Source shapes the compiler must survive.
	add("shape/long_left_chain", "x = 1"+strings.Repeat(" + 1", 900)+"\nprint(x)\n")
	add("shape/spine_in_parentheses", "x = "+strings.Repeat("(", 60)+"1"+strings.Repeat(strings.Repeat(" + 1", 250)+")", 60)+"\nprint(x)\n")
	add("shape/spine_in_parentheses_strings", "x = "+strings.Repeat("(", 30)+"\"a\""+strings.Repeat(strings.Repeat(" + \"b\"", 250)+")", 30)+"\nprint(len(x))\n")
	add("shape/long_string_chain", "s = \"a\""+strings.Repeat(" + \"a\"", 900)+"\nprint(len(s))\n")
	add("shape/long_call_chain", "fun f(x):\n    return x\nend\nprint(f(1)"+strings.Repeat("", 0)+")\nprint("+strings.Repeat("f(", 400)+"1"+strings.Repeat(")", 400)+")\n")
	add("shape/long_subscript_chain", "x = "+strings.Repeat("[", 300)+"1"+strings.Repeat("]", 300)+"\ny = x"+strings.Repeat("[0]", 300)+"\nprint(y)\n")
	add("shape/deep_parentheses", "print("+strings.Repeat("(", 900)+"1"+strings.Repeat(")", 900)+")\n")
	add("shape/deep_unary", "print("+strings.Repeat("-", 1)+strings.Repeat("(-", 450)+"1"+strings.Repeat(")", 450)+")\nprint("+strings.Repeat("not ", 400)+"true)\n")
	add("shape/deep_ternary", "print("+strings.Repeat("true ? ", 300)+"1"+strings.Repeat(" : 2", 300)+")\n")
	add("shape/deep_power", "print(1"+strings.Repeat(" ** 1", 800)+")\n")
	add("shape/deep_list_literal", "x = "+strings.Repeat("[", 200)+strings.Repeat("]", 200)+"\nprint(len(x))\n")
	add("shape/long_chains_by_parts", "x = 0"+strings.Repeat(" + 1", 990)+"\ny = x"+strings.Repeat(" + 1", 990)+"\nprint(y)\n")
	add("shape/chain_just_over", "x = 0"+strings.Repeat(" + 1", 1000)+"\nprint(x)\n")
	add("shape/many_statements", func() string {
		var b strings.Builder
		for i := 0; i < 3000; i++ {
			fmt.Fprintf(&b, "v%d = %d\n", i%50, i)
		}
		b.WriteString("print(v0, v49)\n")
		return b.String()
	}())
	add("shape/many_functions", func() string {
		var b strings.Builder
		for i := 0; i < 300; i++ {
			fmt.Fprintf(&b, "fun f%d(x):\n    return x + %d\nend\n", i, i)
		}
		b.WriteString("print(f0(1), f299(1))\n")
		return b.String()
	}())
	add("shape/many_globals_then_function", func() string {
		var b strings.Builder
		for i := 0; i < 400; i++ {
			fmt.Fprintf(&b, "g%d = %d\n", i, i)
		}
		b.WriteString("fun f():\n    return g0 + g399\nend\nprint(f())\nprint(f())\n")
		return b.String()
	}())
	return cs
}

// coreLimitCases put each limit of the profile at, one below and one above its boundary.
func coreLimitCases() []handCase {
	var cs []handCase
	add := func(name, src string) { cs = append(cs, handCase{name, nil, src}) }

	// Operations: a loop of known cost, swept across the budget of 100,000. Each construct counts differently.
	for k := 12490; k <= 12512; k++ {
		add(fmt.Sprintf("ops/while_%d", k), fmt.Sprintf("i = 0\nwhile (i < %d):\n    i = i + 1\nend\nprint(i)\n", k))
	}
	for k := 9990; k <= 10010; k++ {
		add(fmt.Sprintf("ops/while_compound_%d", k), fmt.Sprintf("i = 0\nwhile (i < %d):\n    i += 1\n    x = i * 2\nend\nprint(i)\n", k))
	}
	for k := 30; k <= 60; k += 1 {
		add(fmt.Sprintf("ops/for_over_list_%d", k), fmt.Sprintf("xs = [0] * %d\nn = 0\nwhile (n < 40):\n    for (x in xs):\n        n = n + 0\n    end\n    n += 1\nend\nprint(n)\n", k*100))
	}
	for k := 3320; k <= 3345; k++ {
		add(fmt.Sprintf("ops/calls_%d", k), fmt.Sprintf("fun f(x):\n    return x\nend\ni = 0\nwhile (i < %d):\n    f(i)\n    i += 1\nend\nprint(i)\n", k))
	}
	for k := 2490; k <= 2510; k++ {
		add(fmt.Sprintf("ops/try_catch_%d", k), fmt.Sprintf("i = 0\nwhile (i < %d):\n    try:\n        i = i + 1\n        x = 1 / 0\n    catch (e):\n        y = 1\n    end\nend\nprint(i)\n", k))
	}
	add("ops/straight_line_over", "x = 0\n"+strings.Repeat("x = x + 1\n", 30000)+"print(x)\n")
	add("ops/straight_line_under", "x = 0\n"+strings.Repeat("x = x + 1\n", 24990)+"print(x)\n")
	add("ops/straight_line_edge", "x = 0\n"+strings.Repeat("x = x + 1\n", 24999)+"print(x)\n")
	add("ops/straight_line_edge_1", "x = 0\n"+strings.Repeat("x = x + 1\n", 25000)+"print(x)\n")
	add("ops/limit_is_not_catchable", "try:\n    while (true):\n    end\ncatch (e):\n    print(\"caught\")\nend\nprint(\"after\")\n")
	add("ops/limit_inside_call", "fun f():\n    while (true):\n    end\nend\ntry:\n    f()\ncatch (e):\n    print(\"caught\")\nend\n")
	add("ops/print_before_limit", "i = 0\nwhile (true):\n    print(i)\n    i += 1\nend\n")
	add("ops/expression_stops_midway", "print(\"start\")\nx = 0\nwhile (true):\n    x = x + (1 + (2 + (3 + (4 + 5))))\nend\n")

	// Call depth: 64.
	for n := 60; n <= 68; n++ {
		add(fmt.Sprintf("depth/recursion_%d", n), fmt.Sprintf("fun f(n):\n    if (n == 0) then:\n        return 0\n    end\n    return 1 + f(n - 1)\nend\nprint(f(%d))\n", n))
		add(fmt.Sprintf("depth/recursion_caught_%d", n), fmt.Sprintf("fun f(n):\n    if (n == 0) then:\n        return 0\n    end\n    return 1 + f(n - 1)\nend\ntry:\n    print(f(%d))\ncatch (e):\n    print(\"caught\")\nend\n", n))
		add(fmt.Sprintf("depth/anonymous_%d", n), fmt.Sprintf("f = fun (n):\n    if (n == 0) then:\n        return 0\n    end\n    return 1 + f(n - 1)\nend\nprint(f(%d))\n", n))
		add(fmt.Sprintf("depth/mutual_%d", n), fmt.Sprintf("fun a(n):\n    if (n == 0) then:\n        return 0\n    end\n    return b(n - 1)\nend\nfun b(n):\n    return a(n)\nend\nprint(a(%d))\n", n/2))
	}
	add("depth/main_recursion", "fun main():\n    return main()\nend\n")
	add("depth/error_at_depth_caught_above", "fun f(n):\n    if (n == 0) then:\n        return 1 / 0\n    end\n    return f(n - 1)\nend\ntry:\n    f(60)\ncatch (e):\n    print(e)\nend\n")

	// Nesting: 256 blocks at once, in the shapes that the profile of limits already covers, plus the top level of a call.
	for n := 250; n <= 258; n++ {
		add(fmt.Sprintf("nesting/in_function_%d", n), "fun f():\n"+strings.Repeat("if (true) then:\n", n-1)+"x = 1\n"+strings.Repeat("end\n", n-1)+"end\nf()\nprint(\"ok\")\n")
		add(fmt.Sprintf("nesting/in_try_%d", n), strings.Repeat("try:\n", n)+"x = 1\n"+strings.Repeat("catch (e):\nend\n", n)+"print(\"ok\")\n")
	}

	// Value depth: 256 levels of lists or maps in a comparison, a stringification or a membership test.
	for _, d := range []int{254, 255, 256, 257, 258} {
		nest := fmt.Sprintf("x = [1]\ni = 1\nwhile (i < %d):\n    x = [x]\n    i += 1\nend\n", d)
		add(fmt.Sprintf("value_depth/eq_%d", d), nest+"print(x == x)\n")
		add(fmt.Sprintf("value_depth/lt_%d", d), nest+"print(x < x)\n")
		add(fmt.Sprintf("value_depth/str_%d", d), nest+"print(len(str(x)))\n")
		add(fmt.Sprintf("value_depth/print_%d", d), nest+"print(x)\n")
		add(fmt.Sprintf("value_depth/in_%d", d), nest+"print(x in [x])\n")
		add(fmt.Sprintf("value_depth/maps_%d", d), fmt.Sprintf("x = {k: 1}\ni = 1\nwhile (i < %d):\n    x = {k: x}\n    i += 1\nend\nprint(x == x)\n", d))
		add(fmt.Sprintf("value_depth/mixed_%d", d), fmt.Sprintf("x = [1]\ni = 1\nwhile (i < %d):\n    if (i %% 2 == 0) then:\n        x = {k: x}\n    else:\n        x = [x]\n    end\n    i += 1\nend\nprint(x == x)\n", d))
		add(fmt.Sprintf("value_depth/caught_%d", d), nest+"try:\n    print(x == x)\ncatch (e):\n    print(\"caught\")\nend\n")
	}
	add("value_depth/self_list", "x = [1]\nx[0] = x\nprint(x == x)\n")
	add("value_depth/self_list_str", "x = [1]\nx[0] = x\nprint(str(x))\n")
	add("value_depth/self_list_print", "x = [1]\nx[0] = x\nprint(x)\n")
	add("value_depth/self_map", "x = {a: 1}\nx.a = x\nprint(x == x)\n")
	add("value_depth/self_in", "x = [1]\nx[0] = x\nprint(1 in x)\n")
	add("value_depth/self_lt", "x = [1]\nx[0] = x\nprint(x < [1])\n")
	add("value_depth/cycle_of_two", "a = [1]\nb = [a]\na[0] = b\nprint(a == b)\n")
	add("value_depth/self_not_compared", "x = [1]\nx[0] = x\nprint(len(x))\ny = x[0][0][0]\nprint(len(y))\n")
	add("value_depth/shared_halves", "x = [1]\ni = 0\nwhile (i < 16):\n    x = [x, x]\n    i += 1\nend\nprint(x == x)\n")
	add("value_depth/shared_halves_str", "x = [1]\ni = 0\nwhile (i < 20):\n    x = [x, x]\n    i += 1\nend\nprint(len(str(x)))\n")
	add("value_depth/shared_halves_deep", "x = [1]\ni = 0\nwhile (i < 40):\n    x = [x, x]\n    i += 1\nend\nprint(x == x)\n")
	add("value_depth/wide_list_eq", "x = [0] * 90000\nprint(x == x)\n")
	add("value_depth/wide_list_eq_small", "x = [0] * 20000\nprint(x == x)\n")

	// Value length: 1 MiB bytes or elements.
	for _, n := range []int{1048575, 1048576, 1048577} {
		add(fmt.Sprintf("value_size/repeat_%d", n), fmt.Sprintf("s = \"a\" * %d\nprint(len(s))\n", n))
		add(fmt.Sprintf("value_size/list_repeat_%d", n), fmt.Sprintf("s = [0] * %d\nprint(len(s))\n", n))
		add(fmt.Sprintf("value_size/repeat_caught_%d", n), fmt.Sprintf("try:\n    s = \"a\" * %d\n    print(len(s))\ncatch (e):\n    print(\"caught\")\nend\n", n))
	}
	for _, n := range []int{524287, 524288, 524289} {
		add(fmt.Sprintf("value_size/concat_%d", n), fmt.Sprintf("s = \"a\" * %d\nt = s + s\nprint(len(t))\n", n))
		add(fmt.Sprintf("value_size/list_concat_%d", n), fmt.Sprintf("s = [0] * %d\nt = s + s\nprint(len(t))\n", n))
	}
	add("value_size/concat_unequal", "s = \"a\" * 1000000\nt = \"b\" * 48576\nprint(len(s + t))\nprint(len(s + t + \"c\"))\n")
	add("value_size/concat_mixed_kinds", "s = \"a\" * 1048576\n"+tryPrint("len(s + 1)")+tryPrint("len(1 + s)")+tryPrint("len(s + [1])")+tryPrint("len(s - 1)"))
	add("value_size/times_negative_huge", "print(1)\n"+tryPrint("\"a\" * (-9223372036854775807)")+tryPrint("9223372036854775807 * \"a\"")+tryPrint("9223372036854775807 * \"\"")+tryPrint("9223372036854775807 * []")+tryPrint("\"ab\" * 9223372036854775807"))
	add("value_size/str_of_big", "s = \"a\" * 1048576\nprint(len(str(s)))\nprint(len(str([s])))\nprint(len(str({k: s})))\n")
	add("value_size/print_of_big", "s = \"a\" * 1048576\nprint(len(s))\nprint([s])\n")
	add("value_size/print_list_of_shared", "s = \"a\" * 100000\nl = [s, s, s, s, s, s, s, s, s, s, s, s]\nprint(len(str(l)))\n")
	add("value_size/spread_big", "xs = [0] * 100000\nfun f(a):\n    return a\nend\nprint(len([xs...]))\n")
	add("value_size/quoted_expansion", "s = \"\\n\" * 200000\nprint(len(str([s])))\n")
	add("value_size/quoted_expansion_fit", "s = \"\\n\" * 100000\nprint(len(str([s])))\n")
	add("value_size/quoted_unicode", "s = \"\u00e9\" * 200000\nprint(len(str([s])))\n")

	// Memory: 4 MiB of charged allocation per run, cumulative. Three strings of 1 MiB leave room for one of
	// 1048512 bytes (each costs its length plus 16).
	for _, n := range []int{1048509, 1048510, 1048511, 1048512, 1048513, 1048514} {
		add(fmt.Sprintf("memory/strings_%d", n), fmt.Sprintf("a = \"a\" * 1048576\nb = \"b\" * 1048576\nc = \"c\" * 1048576\nd = \"d\" * %d\nprint(len(d))\n", n))
		add(fmt.Sprintf("memory/strings_caught_%d", n), fmt.Sprintf("a = \"a\" * 1048576\nb = \"b\" * 1048576\nc = \"c\" * 1048576\ntry:\n    d = \"d\" * %d\n    print(len(d))\ncatch (e):\n    print(\"caught\")\nend\n", n))
	}
	// A list of n elements costs 24 + 16n, and the list literal [0] that is repeated costs 40.
	for _, n := range []int{262138, 262139, 262140, 262141, 262142} {
		add(fmt.Sprintf("memory/list_%d", n), fmt.Sprintf("x = [0] * %d\nprint(len(x))\n", n))
	}
	// 3 x 1 MiB of list (three times 65536... use elements) and then single allocations of each kind.
	big := "a = [0] * 262000\n"
	add("memory/after_big_list_literal", big+"b = [1, 2, 3]\nc = [4]\nprint(len(a))\n")
	add("memory/after_big_list_literal_concat", big+"b = a + [1]\nprint(len(b))\n")
	add("memory/after_big_list_map_literal", big+"b = {a: 1, b: 2}\nprint(len(b))\n")
	add("memory/map_keys", "m = {}\ni = 0\nwhile (i < 20000):\n    m[str(i)] = i\n    i += 1\nend\nprint(len(m))\n")
	add("memory/map_keys_overwrite", "m = {}\ni = 0\nwhile (i < 20000):\n    m[\"k\"] = i\n    i += 1\nend\nprint(len(m), m.k)\n")
	add("memory/map_literal_big", "m = {"+strings.Join(func() []string {
		var ks []string
		for i := 0; i < 2000; i++ {
			ks = append(ks, fmt.Sprintf("k%d: %d", i, i))
		}
		return ks
	}(), ", ")+"}\nprint(len(m))\n")
	add("memory/string_accumulate", "s = \"\"\ni = 0\nwhile (i < 100000):\n    s = s + \"abcdefghij\"\n    i += 1\nend\nprint(len(s))\n")
	add("memory/string_accumulate_small", "s = \"\"\ni = 0\nwhile (i < 20000):\n    s = s + \"abcdefghij\"\n    i += 1\nend\nprint(len(s))\n")
	add("memory/list_accumulate", "l = []\ni = 0\nwhile (i < 30000):\n    l = l + [i]\n    i += 1\nend\nprint(len(l))\n")
	add("memory/list_accumulate_small", "l = []\ni = 0\nwhile (i < 3000):\n    l = l + [i]\n    i += 1\nend\nprint(len(l))\n")
	add("memory/map_merge_accumulate", "m = {}\ni = 0\nwhile (i < 3000):\n    m = m + {k: i}\n    i += 1\nend\nprint(len(m))\n")
	add("memory/for_over_map_keys", "m = {}\ni = 0\nwhile (i < 2000):\n    m[str(i)] = 1\n    i += 1\nend\nn = 0\nj = 0\nwhile (j < 100):\n    for (k in m):\n        n += 1\n    end\n    j += 1\nend\nprint(n)\n")
	add("memory/spread_charges", "xs = [0] * 100000\nfun f(a):\n    return a\nend\nn = 0\nwhile (n < 100):\n    try:\n        f(xs...)\n    catch (e):\n        n += 0\n    end\n    n += 1\nend\nprint(n)\n")
	add("memory/spread_string_charges", "s = \"abcdefghij\" * 50000\nfun f(a):\n    return a\nend\nn = 0\nwhile (n < 20):\n    try:\n        f(s...)\n    catch (e):\n        n += 0\n    end\n    n += 1\nend\nprint(n)\n")
	add("memory/str_results", "s = \"a\" * 100000\nn = 0\nwhile (n < 60):\n    t = str(s)\n    u = str([s])\n    n += 1\nend\nprint(n)\n")
	add("memory/str_of_numbers", "n = 0\nwhile (n < 20000):\n    t = str(n)\n    n += 1\nend\nprint(n)\n")
	add("memory/int_results_of_builtins", "n = 0\nwhile (n < 30000):\n    t = len(\"abc\")\n    n += 1\nend\nprint(n)\n")
	add("memory/typeof_results", "n = 0\nwhile (n < 30000):\n    t = typeof(n)\n    n += 1\nend\nprint(n)\n")
	add("memory/repeated_list_literals", "n = 0\nwhile (n < 20000):\n    t = [n, n]\n    n += 1\nend\nprint(n)\n")
	add("memory/repeated_map_literals", "n = 0\nwhile (n < 20000):\n    t = {a: n}\n    n += 1\nend\nprint(n)\n")
	add("memory/caught_errors_accumulate", "n = 0\nwhile (n < 30000):\n    try:\n        x = 1 / 0\n    catch (e):\n        n += 1\n    end\nend\nprint(n)\n")
	add("memory/caught_long_errors_accumulate", "n = 0\nwhile (n < 4000):\n    try:\n        x = "+strings.Repeat("b", 1100)+"\n    catch (e):\n        n += 1\n    end\nend\nprint(n)\n")
	add("memory/in_is_free", "xs = [1, 2, 3]\nn = 0\nwhile (n < 20000):\n    t = 2 in xs\n    n += 1\nend\nprint(n)\n")
	return cs
}

package main

import (
	"fmt"
	"strings"
)

func init() {
	libRegistry = append(libRegistry, handGroup("lib-lists", listCases()))
}

// The scripts of the list builtins: append push pop shift unshift reverse slice range index_of last_index_of map
// filter reduce. They cover the value of each call, the error of each wrong argument, the header model (a loop
// that changes the list it reads), callbacks, and the memory the growth of a list is charged.

// goCap is the capacity the Go runtime gives a slice of interface values that grows from oldCap to hold newLen.
func goCap(oldCap, newLen int) int {
	s := make([]any, oldCap, oldCap)
	s = append(s, make([]any, newLen-oldCap)...)
	return cap(s)
}

// capSteps is every capacity a slice reaches by appending one element at a time, up to limit elements.
func capSteps(limit int) []int {
	var caps []int
	c := 0
	for n := 1; n <= limit; n++ {
		if n > c {
			c = goCap(c, n)
			caps = append(caps, c)
		}
	}
	return caps
}

const (
	memBudget = 4194304
	// The ledger spends what is left of the memory budget in chunks of 30,016 bytes (a string of 30,000 bytes),
	// then in steps of 8 (the int that len returns), and prints how many steps it got in.
	ledgerChunk  = 30016
	ledgerMargin = 8000
)

// ledger is a script that runs setup, then uses up the memory budget, and prints how many 8-byte steps it
// made after the big ones: the count tells within eight bytes what the setup charged. spent is a guess of that
// charge, only used to size the big steps; a wrong guess makes the script less exact, not wrong.
func ledger(setup string, spent int) string {
	n := (memBudget - spent - ledgerMargin) / ledgerChunk
	if n < 0 {
		n = 0
	}
	return setup + fmt.Sprintf("j = 0\nwhile (j < %d):\n    s = \"a\" * 30000\n    j += 1\nend\nk = 0\nwhile (true):\n    len(\"\")\n    k += 1\n    print(k)\nend\n", n)
}

// padScript allocates exactly n bytes of the memory budget, n at least 16, in strings of a million bytes and one
// more (a string of m bytes costs 16 + m).
func padScript(n int) string {
	var b strings.Builder
	chunks := 0
	for n-1000016 >= 16 {
		fmt.Fprintf(&b, "pad%d = \"a\" * 1000000\n", chunks)
		chunks++
		n -= 1000016
	}
	if n >= 16 {
		fmt.Fprintf(&b, "pad%d = \"a\" * %d\n", chunks, n-16)
	}
	return b.String()
}

// edgeCases adds the scripts that run setup, spend all of the memory budget but d bytes more than op needs, and
// run op: for d at and below zero the op fits, from one up it stops the run. spent and cost are the guesses of
// what setup and op charge, so that d = 0 is the edge; a wrong guess moves the edge, which the neighbouring
// values of d still cross.
func edgeCases(add func(name, src string), name, setup string, spent int, op string, cost int) {
	for _, d := range []int{-24, -16, -8, -3, -2, -1, 0, 1, 2, 3, 8, 16, 24} {
		pad := memBudget - spent - cost + d
		if pad < 16 {
			continue
		}
		add(fmt.Sprintf("edge/%s_%+d", name, d), setup+padScript(pad)+op+"\nprint(\"ok\")\n")
	}
}

// rangeSpent is what l = range(k) charges: the list with room for k and eight bytes for each integer.
func rangeSpent(k int) int { return 24 + 24*k }

const listPrelude = "fun f():\nend\nfun inc(x):\n    return x + 1\nend\nfun add(a, b):\n    return a + b\nend\nfun big(x):\n    return x > 1\nend\nfun bad(x):\n    return x / 0\nend\nfun even(x):\n    return x % 2 == 0\nend\n"

func listCases() []handCase {
	var cs []handCase
	add := func(name, src string) { cs = append(cs, handCase{name, nil, src}) }

	// Values of each builtin on ordinary input.
	add("basic/append_push", "l = [1, 2]\nprint(append(l, 3), l)\nprint(append(l, 4, 5), l)\nprint(append(l), l)\nprint(push(l, 6), l)\nprint(push(l, 7, 8, 9), l)\nm = [[1]]\nappend(m, [2], {a: 1}, \"s\", null, true, 1.5)\nprint(m, len(m))\nappend(l, l)\nprint(len(l), len(l[len(l) - 1]))\n")
	add("basic/pop_shift", "l = [1, 2, 3, 4]\nprint(pop(l), l, shift(l), l)\nprint(pop(l), shift(l), l, pop(l), shift(l), l)\nprint(typeof(pop([])), typeof(shift([])))\nm = [\"a\", [1], {k: 2}]\nprint(pop(m), shift(m), m)\n")
	add("basic/unshift", "l = [3]\nunshift(l, 2)\nunshift(l, 0, 1)\nprint(l)\nunshift(l, [9], null)\nprint(l, len(l))\ne = []\nunshift(e, 1)\nprint(e)\nprint(unshift([1], 2, 3, 4))\n")
	add("basic/reverse", "l = [1, 2, 3, 4]\nreverse(l)\nprint(l)\nreverse(l)\nprint(l)\ne = []\nreverse(e)\nprint(e)\no = [7]\nreverse(o)\nprint(o)\nprint(reverse([1, 2]))\nr = [1, [2, 3], \"x\"]\nreverse(r)\nprint(r)\n")
	add("basic/slice_list", "l = [10, 20, 30, 40]\nprint(slice(l, 0, 4), slice(l, 1, 3), slice(l, 2, 2), slice(l, 4, 4), slice(l, 0, 0))\ns = slice(l, 1, 3)\ns[0] = 99\nprint(l, s)\nn = [[1], [2]]\nt = slice(n, 0, 2)\nt[0][0] = 7\nprint(n, t)\n")
	add("basic/slice_string", "s = \"héllo\"\nprint(slice(s, 0, 2), slice(s, 1, 3) == \"é\", slice(s, 0, len(s)), slice(\"abc\", 3, 3) == \"\", len(slice(s, 2, 4)))\nprint(slice(\"\", 0, 0) == \"\")\n")
	add("basic/range", "print(range(0), range(1), range(5), range(-0))\nprint(range(2, 6), range(6, 2), range(3, 3))\nprint(range(0, 10, 3), range(10, 0, -3), range(0, 10, 10), range(0, 10, 11), range(5, 0, 1), range(0, 5, -1))\nprint(range(0, 9, 3), range(0, 10, 2), range(9, -1, -1), range(1, 2, 5))\nprint(len(range(1000)), range(1000)[999], len(range(0, 1000, 7)), len(range(5, 505, 1)))\n")
	add("basic/index_of", "l = [1, 2, 3, 2, \"a\", [1], {a: 1}, 2.0, null, true]\nprint(index_of(l, 2), index_of(l, 3), index_of(l, 9), index_of(l, \"a\"), index_of(l, [1]), index_of(l, null), index_of(l, true), index_of(l, 2.0))\nprint(last_index_of(l, 2), last_index_of(l, 3), last_index_of(l, 9), last_index_of(l, [1]), last_index_of(l, {a: 1}), last_index_of(l, null))\nprint(index_of([], 1), last_index_of([], 1), index_of([[1, [2]]], [1, [2]]), index_of([1], 1.0))\n")
	add("basic/map_filter_reduce", listPrelude+"l = [1, 2, 3, 4, 5]\nprint(map(l, inc), map(l, str), map([], inc), map(l, typeof))\nprint(filter(l, big), filter(l, even), filter([], big), filter(l, fun (x):\n    return 0\nend), filter(l, fun (x):\n    return x\nend))\nprint(reduce(l, add, 0), reduce(l, add, 10), reduce([], add, 7), reduce([\"a\", \"b\"], add, \"\"), reduce(l, fun (a, x):\n    return a + [x]\nend, []))\nprint(l)\n")
	add("basic/filter_truthiness", "fun f():\nend\ninf = 10 ** 400\nnan = inf - inf\nxs = [0, 1, -1, \"\", \"a\", [], [1], {}, {a: 1}, null, true, false, 0.0, 2.5, nan, inf, f, print, set_new(), stack_new()]\nr = filter(xs, fun (x):\n    return x\nend)\nprint(len(r), str(r))\nfor (x in xs):\n    print(len(filter([x], fun (y):\n        return y\n    end)))\nend\n")
	add("basic/uncomparable_members", "l = [print, len, 1]\n"+tryPrint("index_of(l, print)")+tryPrint("index_of(l, len)")+tryPrint("index_of(l, 1)")+tryPrint("last_index_of(l, print)")+tryPrint("last_index_of(l, len)")+tryPrint("last_index_of(l, 1)")+tryPrint("index_of([[print]], [print])")+tryPrint("index_of([1, [len]], [len])")+tryPrint("index_of([f], f)")+"fun f():\nend\n")
	add("basic/callback_builtins", "print(map([\"a\", \"bc\"], len), map([1, 2.5, \"s\", null], typeof), map([[1], []], len), map([1, 2], str))\nprint(filter([[], [1], \"\", \"a\", 0, 1, null, {}, {a: 1}], len) == [[1], \"a\", {a: 1}])\nprint(reduce([1, 2, 3], max, 0))\n")

	// Arguments of the wrong kind or number, each call in its own try block.
	lpool := []string{"[]", "[1, 2, 3]", "\"abc\"", "1", "2.5", "null", "true", "{a: 1}", "f", "print", "len", "(-1)"}
	for _, name := range []string{"append", "push", "pop", "shift", "unshift", "reverse", "index_of", "last_index_of", "map", "filter", "reduce", "slice", "range"} {
		var b strings.Builder
		b.WriteString(listPrelude)
		b.WriteString(tryPrint(name + "()"))
		for _, a := range lpool {
			b.WriteString("l = [1, 2, 3]\n")
			b.WriteString(tryStmt(fmt.Sprintf("print(%s(%s))", name, a)))
		}
		for _, a := range lpool {
			for _, c := range lpool {
				b.WriteString(tryStmt(fmt.Sprintf("print(%s(%s, %s))", name, a, c)))
			}
		}
		for _, a := range []string{"[1, 2, 3]", "\"abc\"", "1", "null"} {
			for _, c := range []string{"0", "1", "(-1)", "5", "1.5", "\"a\"", "inc", "null"} {
				for _, d := range []string{"0", "2", "3", "(-1)", "7", "\"a\"", "add", "inc", "1.5", "null"} {
					b.WriteString(tryStmt(fmt.Sprintf("x = %s\n    print(%s(x, %s, %s), x)", a, name, c, d)))
				}
			}
		}
		b.WriteString(tryStmt(fmt.Sprintf("print(%s([1, 2], 1, 2, 3))", name)))
		b.WriteString(tryStmt(fmt.Sprintf("print(%s([1, 2], 1, 2, 3, 4, 5))", name)))
		add("args/"+name, b.String())
	}
	add("args/slice_bounds", "l = [1, 2, 3]\ns = \"abc\"\n"+tryPrint("slice(l, -1, 2)")+tryPrint("slice(l, 0, 4)")+tryPrint("slice(l, 2, 1)")+tryPrint("slice(l, 3, 3)")+tryPrint("slice(l, 4, 4)")+tryPrint("slice(l, 0, -1)")+tryPrint("slice(s, -1, 2)")+tryPrint("slice(s, 0, 4)")+tryPrint("slice(s, 2, 1)")+tryPrint("slice(s, 4, 4)")+tryPrint("slice(l, 0.0, 2)")+tryPrint("slice(l, 0, 2.0)")+tryPrint("slice(l, \"0\", 2)")+tryPrint("slice(null, 0, 0)")+tryPrint("slice({a: 1}, 0, 0)")+tryPrint("slice(5, 0, 0)")+tryPrint("slice(5, \"a\", 0)")+tryPrint("slice([], 0, 0)")+tryPrint("slice(\"\", 0, 1)"))
	add("args/range_forms", tryPrint("range(3, 1, 1)")+tryPrint("range(1, 3, 0)")+tryPrint("range(0, 0, 0)")+tryPrint("range(0, 5, 2.0)")+tryPrint("range(0.0, 5, 1)")+tryPrint("range(\"a\", 5, 1)")+tryPrint("range(1, 2, 3, 4)")+tryPrint("range(-3)")+tryPrint("range(-3, -1)")+tryPrint("range(-1, -4, -1)")+tryPrint("range(3, 0, -2)")+tryPrint("range(0, 1, 100)")+tryPrint("range(7, 8, 1)")+tryPrint("range(10, 0, -4)"))

	// Where the argument count is checked: by the call, or by the builtin itself.
	add("args/count_by_builtin", "l = [1, 2, 3]\n"+tryPrint("push(l)")+tryPrint("push()")+tryPrint("push(l, 1, 2, 3)")+"print(l)\n"+tryPrint("unshift(l)")+tryPrint("unshift(l, 0, 0, 0)")+"print(l)\n"+tryPrint("pop(l, l)")+tryPrint("shift(l, 1)")+tryPrint("reverse(l, 1)")+tryPrint("pop()")+tryPrint("shift()")+tryPrint("reverse()")+tryPrint("append()")+tryPrint("append(1)")+tryPrint("append(l)")+"print(l)\n")
	add("args/count_by_call", "l = [1, 2, 3]\n"+tryPrint("slice(l)")+tryPrint("slice(l, 1)")+tryPrint("slice(l, 1, 2, 3)")+tryPrint("index_of(l)")+tryPrint("index_of(l, 1, 2)")+tryPrint("last_index_of(l)")+tryPrint("map(l)")+tryPrint("map(l, len, 1)")+tryPrint("filter(l)")+tryPrint("reduce(l, max)")+tryPrint("reduce(l, max, 0, 1)")+tryPrint("sort()")+tryPrint("sort(l, l, l)"))

	// The header model: a loop reads the list through the header it had at its start. What the body writes into
	// that array shows, what moves the list to another array does not.
	inits := []struct{ name, src string }{
		{"literal", "l = [1, 2, 3, 4]\n"},
		{"range", "l = range(1, 5)\n"},
		{"concat", "l = [1, 2] + [3, 4]\n"},
		{"slice", "l = slice([0, 1, 2, 3, 4, 5], 1, 5)\n"},
		{"after_pop", "l = [1, 2, 3, 4, 5]\npop(l)\n"},
		{"after_shift", "l = [0, 1, 2, 3, 4]\nshift(l)\n"},
		{"empty", "l = range(0)\n"},
	}
	muts := []struct{ name, src string }{
		{"pop", "pop(l)"},
		{"shift", "shift(l)"},
		{"unshift", "unshift(l, 9)"},
		{"append", "append(l, 9)"},
		{"push_two", "push(l, 8, 9)"},
		{"set_first", "if (len(l) > 0) then:\n        l[0] = 99\n    end"},
		{"set_last", "if (len(l) > 0) then:\n        l[len(l) - 1] = 99\n    end"},
		{"reverse", "reverse(l)"},
		{"sort_desc_first", "l[0] = 50\n    sort(l)"},
		{"reassign_plus", "l = l + [9]"},
		{"reassign_empty", "l = []"},
		{"pop_then_append", "pop(l)\n    append(l, 7)"},
		{"shift_then_append", "shift(l)\n    append(l, 7)"},
		{"unshift_then_set", "unshift(l, 5)\n    l[0] = 6"},
		{"append_then_set", "append(l, 5)\n    l[0] = 6"},
	}
	for _, in := range inits {
		for _, m := range muts {
			add("header/for_"+in.name+"_"+m.name, in.src+"for (x in l):\n    "+m.src+"\n    print(x, len(l))\nend\nprint(l, len(l))\n")
		}
		add("header/for_"+in.name+"_nested", in.src+"for (x in l):\n    for (y in l):\n        pop(l)\n        print(x, y)\n    end\nend\nprint(l)\n")
		add("header/for_"+in.name+"_break_then_reuse", in.src+"for (x in l):\n    append(l, x)\n    if (x == 2) then:\n        break\n    end\nend\nprint(l)\nfor (y in l):\n    print(y)\nend\n")
	}
	add("header/for_over_list_in_function", "fun g(l):\n    for (x in l):\n        pop(l)\n        print(x)\n    end\nend\nm = [1, 2, 3, 4]\ng(m)\nprint(m)\n")
	add("header/for_growing_by_append_past_cap", "l = range(3)\nfor (x in l):\n    append(l, x + 10)\n    l[0] = -1\n    print(x, l)\nend\nprint(l)\n")
	add("header/for_two_loops_one_list", "l = [1, 2, 3]\nfor (x in l):\n    for (y in l):\n        append(l, 0)\n        print(x, y, len(l))\n    end\nend\n")
	add("header/for_map_keys_unaffected", "m = {a: 1, b: 2}\nfor (k in m):\n    m.c = 3\n    m[\"d\"] = 4\n    print(k)\nend\nprint(len(m))\n")
	add("header/spread_takes_a_copy", "fun f(a, b, c):\n    return [a, b, c]\nend\nl = [1, 2, 3]\nprint(f(l...))\nl2 = [1, 2]\npush(l2, 3)\nprint(f(l2...))\n")
	add("header/popped_then_pushed_in_loop", "l = [1, 2, 3, 4]\nfor (x in l):\n    if (x == 1) then:\n        pop(l)\n        pop(l)\n        append(l, 20)\n        append(l, 30)\n    end\n    print(x)\nend\n")
	add("header/shift_in_loop_then_append_within_capacity", "l = [1, 2, 3, 4]\nfor (x in l):\n    if (x == 1) then:\n        shift(l)\n        append(l, 20)\n    end\n    print(x)\nend\nprint(l)\n")

	// Callbacks that change the list they were given: map, filter and reduce read the header of their start.
	for _, m := range []struct{ name, body string }{
		{"pop", "pop(l)"}, {"shift", "shift(l)"}, {"unshift", "unshift(l, 9)"}, {"append", "append(l, 9)"},
		{"set_last", "l[len(l) - 1] = 99"}, {"set_first", "l[0] = 99"}, {"reverse", "reverse(l)"},
		{"reassign", "l = [7]"}, {"push_big", "push(l, 1, 2, 3, 4, 5, 6)"},
	} {
		for _, fn := range []string{"map", "filter"} {
			add("callback/"+fn+"_that_does_"+m.name, fmt.Sprintf("l = [1, 2, 3, 4]\nr = %s(l, fun (x):\n    %s\n    return x\nend)\nprint(r, l)\n", fn, m.body))
		}
		add("callback/reduce_that_does_"+m.name, fmt.Sprintf("l = [1, 2, 3, 4]\nr = reduce(l, fun (a, x):\n    %s\n    return a + x\nend, 0)\nprint(r, l)\n", m.body))
		add("callback/map_range_that_does_"+m.name, fmt.Sprintf("l = range(4)\nr = map(l, fun (x):\n    %s\n    return x\nend)\nprint(r, l)\n", m.body))
	}

	// Callbacks: results, errors, control flow and depth.
	add("callback/results", listPrelude+"print(map([1, 2], fun (x):\n    return [x, x]\nend))\nprint(map([1, 2], fun (x):\nend))\nprint(filter([1, 2, 3], fun (x):\n    if (x == 2) then:\n        return true\n    end\nend))\nprint(reduce([1, 2, 3], fun (a, x):\nend, 5))\nprint(reduce([], fun (a, x):\nend, 5))\nprint(reduce([1], f, 5))\n")
	add("callback/error_inside", listPrelude+tryPrint("map([1, 0, 2], fun (x):\n    return 10 / x\nend)")+tryPrint("filter([1, 0], bad)")+tryPrint("reduce([1, 2], fun (a, x):\n    return a / 0\nend, 1)")+tryPrint("map([1], fun (x):\n    return undefined_name\nend)")+tryPrint("map([1], fun (x):\n    return [][3]\nend)")+tryPrint("map([1, 2], fun (x):\n    return x.y\nend)")+"x = 5\n"+tryStmt("map([1], fun (x):\n        x = x / 0\n    end)")+"print(x)\n")
	add("callback/error_uncaught", "r = map([1, 2], fun (x):\n    return 1 / 0\nend)\nprint(r)\n")
	add("callback/error_uncaught_in_filter", "r = filter([1, 2], fun (x):\n    return undefined_name\nend)\n")
	add("callback/error_in_nested_callback", "r = map([1, 2], fun (x):\n    return map([1], fun (y):\n        return y / 0\n    end)\nend)\n")
	add("callback/wrong_arity", listPrelude+tryPrint("map([1], add)")+tryPrint("map([1], fun ():\n    return 1\nend)")+tryPrint("filter([1], add)")+tryPrint("reduce([1], inc, 0)")+tryPrint("reduce([1], fun (a, b, c):\n    return 1\nend, 0)")+tryPrint("map([1], fun (x, rest...):\n    return rest\nend)")+tryPrint("reduce([1, 2], fun (acc, rest...):\n    return rest\nend, 0)")+tryPrint("map([1], len)")+tryPrint("map([[1]], len)")+tryPrint("map([1], add)")+tryPrint("reduce([[1]], map, [])"))
	add("callback/break_continue", "for (i in [1, 2, 3]):\n    print(\"i\", i)\n    map([1, 2], fun (x):\n        if (i == 2) then:\n            break\n        end\n        return x\n    end)\n    print(\"after\", i)\nend\nprint(\"done\")\nfor (i in [1, 2, 3]):\n    filter([1, 2], fun (x):\n        if (i == 2) then:\n            continue\n        end\n        return true\n    end)\n    print(\"after\", i)\nend\n")
	add("callback/break_outside_loop", "map([1, 2], fun (x):\n    break\nend)\nprint(\"after\")\n")
	add("callback/break_in_try", "for (i in [1, 2]):\n    try:\n        map([1], fun (x):\n            break\n        end)\n    catch (e):\n        print(\"caught\", e)\n    end\n    print(i)\nend\n")
	add("callback/break_inside_callback_loop", "r = map([1, 2], fun (x):\n    n = 0\n    for (i in [1, 2, 3]):\n        if (i == 2) then:\n            break\n        end\n        n += i\n    end\n    return n\nend)\nprint(r)\n")
	add("callback/return_in_loop", "r = map([1, 2, 3], fun (x):\n    for (i in [1, 2, 3]):\n        if (i == x) then:\n            return i * 10\n        end\n    end\n    return -1\nend)\nprint(r)\n")
	add("callback/return_in_try", "r = map([1, 2], fun (x):\n    try:\n        return x + 1\n    catch (e):\n        return 0\n    end\nend)\nprint(r)\nr = map([1, 0], fun (x):\n    try:\n        return 10 / x\n    catch (e):\n        return \"err\"\n    end\nend)\nprint(r)\nz = 3\nprint(z)\n")
	add("callback/closure_state", "n = 0\nr = map([1, 2, 3], fun (x):\n    n += x\n    return n\nend)\nprint(r, n)\nmk = fun ():\n    c = 0\n    return fun (x):\n        c += 1\n        return c * x\n    end\nend\nprint(map([5, 5, 5], mk()))\n")
	add("callback/nested", "print(map([[1, 2], [3]], fun (r):\n    return map(r, fun (x):\n        return x * 2\n    end)\nend))\nprint(reduce([1, 2, 3], fun (a, x):\n    return a + reduce([x, x], fun (p, q):\n        return p + q\n    end, 0)\nend, 0))\nprint(filter(map([1, 2, 3, 4], fun (x):\n    return x * x\nend), fun (x):\n    return x > 4\nend))\n")
	add("callback/recursive_function", "fun fact(n):\n    if (n < 2) then:\n        return 1\n    end\n    return n * fact(n - 1)\nend\nprint(map([1, 2, 5, 10], fact))\nfun deep(n):\n    if (n == 0) then:\n        return 0\n    end\n    return 1 + map([n - 1], deep)[0]\nend\nprint(deep(5))\n")
	for _, n := range []int{14, 15, 16, 17, 18, 30, 31, 32, 33, 34, 60} {
		add(fmt.Sprintf("callback/depth_%d", n), fmt.Sprintf("fun deep(n):\n    if (n == 0) then:\n        return 0\n    end\n    return 1 + map([n - 1], deep)[0]\nend\nprint(deep(%d))\n", n))
		add(fmt.Sprintf("callback/depth_plain_%d", n), fmt.Sprintf("fun deep(n):\n    if (n == 0) then:\n        return 0\n    end\n    return 1 + deep(n - 1)\nend\nprint(deep(%d))\n", n+n/2))
	}
	add("callback/depth_infinite", "fun f(x):\n    return map([x], f)\nend\nf(1)\n")
	add("callback/depth_infinite_caught", "fun f(x):\n    return map([x], f)\nend\ntry:\n    f(1)\ncatch (e):\n    print(\"caught\")\nend\nprint(\"after\")\n")
	add("callback/depth_through_all_three", "fun f(x):\n    return filter([x], f)\nend\nfun g(x):\n    return reduce([x], fun (a, y):\n        return g(y)\n    end, 0)\nend\ng(1)\n")
	nest := strings.Repeat("if (true) then:\n", 12) + "return f(n - 1)\n" + strings.Repeat("end\n", 12)
	for _, n := range []int{10, 15, 17, 18, 19, 20, 21, 22, 25} {
		add(fmt.Sprintf("callback/nesting_limit_%d", n), fmt.Sprintf("fun f(n):\n    if (n == 0) then:\n        return 0\n    end\n    return map([1], fun (x):\n%s    end)[0]\nend\nprint(f(%d))\n", nest, n))
	}
	add("callback/builtin_with_error_value", "x = map([\"a\"], fun (s):\n    return date_add(s, \"x\", 1)\nend)\nprint(typeof(x[0]), len(x))\n")
	add("callback/shadowed_builtin", "len = 5\nprint(map([\"a\", \"bb\"], fun (s):\n    return len\nend))\n")
	add("callback/called_with_the_header_of_the_start", "l = [1, 2, 3]\nr = map(l, fun (x):\n    l = [9, 9, 9, 9]\n    return x\nend)\nprint(r, l)\n")
	add("callback/same_list_twice", "l = [3, 1, 2]\nprint(map(l, fun (x):\n    return len(l)\nend))\nprint(reduce(l, fun (a, x):\n    return a + [index_of(l, x)]\nend, []))\n")
	add("callback/error_value_returned_by_callback", "r = map([1, 2], fun (x):\n    return str(x) + date_add(1, \"x\", 1)\nend)\n")

	// The operations a callback costs: the op limit stops a map at the same element, at the same place.
	for _, n := range []int{20000, 30000, 40000, 44000, 46000, 48000, 49000, 49500, 49900, 49990, 50000, 50010, 50100, 50500, 51000, 52000, 60000} {
		add(fmt.Sprintf("ops/map_%d", n), fmt.Sprintf("r = map(range(%d), fun (x):\n    return x\nend)\nprint(len(r))\n", n))
	}
	for n := 49991; n < 50000; n++ {
		add(fmt.Sprintf("ops/map_%d", n), fmt.Sprintf("r = map(range(%d), fun (x):\n    return x\nend)\nprint(len(r))\n", n))
	}
	var filterScan []int
	for n := 16660; n <= 16670; n++ {
		filterScan = append(filterScan, n)
	}
	for n := 24980; n <= 25000; n++ {
		filterScan = append(filterScan, n)
	}
	filterScan = append(filterScan, 10000, 15000, 20000, 30000)
	for _, n := range filterScan {
		add(fmt.Sprintf("ops/filter_%d", n), fmt.Sprintf("r = filter(range(%d), fun (x):\n    return x %% 2 == 0\nend)\nprint(len(r))\n", n))
		add(fmt.Sprintf("ops/reduce_%d", n), fmt.Sprintf("r = reduce(range(%d), fun (a, x):\n    return a + x\nend, 0)\nprint(r)\n", n))
	}
	for n := 10444; n <= 10448; n++ {
		add(fmt.Sprintf("ops/loop_with_mutation_%d", n), fmt.Sprintf("l = range(%d)\nn = 0\nfor (x in l):\n    n += x\n    if (x %% 7 == 0) then:\n        pop(l)\n    end\nend\nprint(n, len(l))\n", n))
	}
	for n := 33320; n <= 33360; n++ {
		add(fmt.Sprintf("ops/index_of_%d", n), fmt.Sprintf("l = range(%d)\nprint(index_of(l, %d), last_index_of(l, 0), index_of(l, -1))\n", n, n-1))
	}

	// Memory. The growth of a list is charged to the budget: a list that moves to a new array costs the whole
	// array. The ledger scripts spend the rest of the budget and print how much of it was left.
	steps := capSteps(100000)
	var interesting []int
	for _, c := range steps {
		for _, d := range []int{-1, 0, 1} {
			if k := c + d; k > 0 && k <= 100000 {
				interesting = append(interesting, k)
			}
		}
	}
	interesting = append(interesting, 0)
	ledgerOps := []struct {
		name  string
		src   string
		spent func(k int) int
	}{
		{"append_one", "append(l, 0)", func(k int) int { return 24 + 16*goCap(k, k+1) }},
		{"push_two", "push(l, 0, 0)", func(k int) int { return 24 + 16*goCap(k, k+2) }},
		{"append_then_append", "append(l, 0)\nappend(l, 0)", func(k int) int { return 24 + 16*goCap(k, k+1) + 16 }},
		{"unshift_one", "unshift(l, 0)", func(k int) int { return 24 + 16*(k+1) }},
		{"unshift_three", "unshift(l, 0, 1, 2)", func(k int) int { return 24 + 16*(k+3) }},
		{"pop_then_append", "pop(l)\nappend(l, 0)", func(k int) int { return 8 + 16 }},
		{"shift_then_append", "shift(l)\nappend(l, 0)", func(k int) int { return 8 + 24 + 16*goCap(k-1, k) }},
		{"shift_shift_append_append", "shift(l)\nshift(l)\nappend(l, 0)\nappend(l, 0)", func(k int) int { return 16 + 24 + 16*goCap(k-2, k-1) }},
		{"slice_all", "t = slice(l, 0, len(l))", func(k int) int { return 24 + 16*k + 8 }},
		{"plus_one", "t = l + [0]", func(k int) int { return 24 + 16*(k+1) }},
		{"reverse_nothing", "reverse(l)", func(k int) int { return 0 }},
	}
	for _, k := range interesting {
		if k > 90000 {
			continue
		}
		ops := ledgerOps
		if k > 5000 {
			ops = ledgerOps[:4]
		}
		for _, op := range ops {
			if k < 3 && strings.Contains(op.name, "shift") && !strings.HasPrefix(op.name, "unshift") {
				continue
			}
			add(fmt.Sprintf("ledger/%s_%d", op.name, k), ledger(fmt.Sprintf("l = range(%d)\n%s\n", k, op.src), rangeSpent(k)+op.spent(k)))
		}
	}
	for _, k := range []int{1, 3, 5, 6, 15, 16, 17, 18, 31, 32, 33, 64, 65, 70, 71, 72, 100, 101, 142, 143, 144, 255, 256, 257, 300, 500, 1000} {
		add(fmt.Sprintf("ledger/grow_by_append_%d", k), ledger(fmt.Sprintf("l = range(0)\ni = 0\nwhile (i < %d):\n    append(l, i)\n    i += 1\nend\n", k), 24+k*16+24*capCount(k)+16*k))
		add(fmt.Sprintf("ledger/grow_by_push_pairs_%d", k), ledger(fmt.Sprintf("l = range(0)\ni = 0\nwhile (i < %d):\n    push(l, i, i)\n    i += 1\nend\n", k), 24+k*32+24*capCount(2*k)+16*k))
		add(fmt.Sprintf("ledger/grow_by_unshift_%d", k), ledger(fmt.Sprintf("l = range(0)\ni = 0\nwhile (i < %d):\n    unshift(l, i)\n    i += 1\nend\n", k), 24*k+8*k*k))
		add(fmt.Sprintf("ledger/queue_pattern_%d", k), ledger(fmt.Sprintf("l = range(%d)\ni = 0\nwhile (i < %d):\n    shift(l)\n    append(l, i)\n    i += 1\nend\n", k, 3*k+5), rangeSpent(k)+queueSpent(k, 3*k+5)))
	}
	for _, k := range []int{0, 1, 2, 5, 20, 100, 101, 102, 1000} {
		add(fmt.Sprintf("ledger/range_three_args_%d", k), ledger(fmt.Sprintf("l = range(0, %d, 1)\n", k), 24+24*k))
		add(fmt.Sprintf("ledger/range_three_args_step_%d", k), ledger(fmt.Sprintf("l = range(0, %d, 2)\n", 2*k), 24+24*k))
		add(fmt.Sprintf("ledger/range_two_args_%d", k), ledger(fmt.Sprintf("l = range(5, %d)\n", 5+k), 24+24*k))
		add(fmt.Sprintf("ledger/range_negative_step_%d", k), ledger(fmt.Sprintf("l = range(%d, 0, -1)\n", k), 24+24*k))
	}
	for _, n := range []int{50, 99, 100, 101, 102, 140, 150, 200, 1000} {
		add(fmt.Sprintf("ledger/filter_all_%d", n), ledger(fmt.Sprintf("l = range(%d)\nr = filter(l, fun (x):\n    return true\nend)\n", n), rangeSpent(n)+24+16*goCap(0, n)+8*n))
		add(fmt.Sprintf("ledger/filter_half_%d", n), ledger(fmt.Sprintf("l = range(%d)\nr = filter(l, fun (x):\n    return x %% 2 == 0\nend)\n", n), rangeSpent(n)+24+16*goCap(0, n/2)+8*n))
		add(fmt.Sprintf("ledger/map_%d", n), ledger(fmt.Sprintf("l = range(%d)\nr = map(l, fun (x):\n    return x\nend)\n", n), rangeSpent(n)+24+16*n+8*n))
		add(fmt.Sprintf("ledger/reduce_%d", n), ledger(fmt.Sprintf("l = range(%d)\nr = reduce(l, fun (a, x):\n    return a + x\nend, 0)\n", n), rangeSpent(n)+8))
		add(fmt.Sprintf("ledger/slice_%d", n), ledger(fmt.Sprintf("l = range(%d)\nr = slice(l, 1, %d)\n", n, n), rangeSpent(n)+24+16*(n-1)+8*(n-1)))
	}
	for _, k := range []int{1, 2, 3, 4, 15, 16, 17, 31, 32, 33, 70, 71, 72, 142, 143, 144, 1000, 5000} {
		setup := fmt.Sprintf("l = range(%d)\n", k)
		edgeCases(add, fmt.Sprintf("append_realloc_%d", k), setup, rangeSpent(k), "append(l, 0)", 24+16*goCap(k, k+1))
		edgeCases(add, fmt.Sprintf("push_two_realloc_%d", k), setup, rangeSpent(k), "push(l, 0, 0)", 24+16*goCap(k, k+2))
		edgeCases(add, fmt.Sprintf("unshift_%d", k), setup, rangeSpent(k), "unshift(l, 0)", 24+16*(k+1))
		edgeCases(add, fmt.Sprintf("pop_then_append_%d", k), setup, rangeSpent(k), "pop(l)\nappend(l, 0)", 24)
		edgeCases(add, fmt.Sprintf("slice_%d", k), setup, rangeSpent(k), "t = slice(l, 0, len(l))", 24+16*k+8)
		edgeCases(add, fmt.Sprintf("plus_%d", k), setup, rangeSpent(k), "t = l + l", 24+16*2*k)
	}
	for _, k := range []int{0, 1, 5, 20, 100, 101, 1000} {
		edgeCases(add, fmt.Sprintf("range_%d", k), "", 0, fmt.Sprintf("l = range(%d)", k), 24+24*k)
		edgeCases(add, fmt.Sprintf("range_step_%d", k), "", 0, fmt.Sprintf("l = range(0, %d, 1)", k), 24+24*k)
		edgeCases(add, fmt.Sprintf("map_identity_%d", k), fmt.Sprintf("l = range(%d)\n", k), rangeSpent(k), "r = map(l, fun (x):\n    return x\nend)", 24+16*k)
		edgeCases(add, fmt.Sprintf("filter_all_%d", k), fmt.Sprintf("l = range(%d)\n", k), rangeSpent(k), "r = filter(l, fun (x):\n    return true\nend)", 24+16*goCap(0, k))
	}
	for _, n := range []int{0, 1, 2, 10, 1000, 100000} {
		add(fmt.Sprintf("ledger/slice_string_%d", n), ledger(fmt.Sprintf("s = \"a\" * %d\nt = slice(s, 0, len(s))\nu = slice(s, len(s), len(s))\nv = slice(s, 0, 0)\nw = slice(s, len(s) - len(s) / 2, len(s))\n", n), 16+n+16*4+8*4))
		edgeCases(add, fmt.Sprintf("slice_string_%d", n), fmt.Sprintf("s = \"a\" * %d\n", n), 16+n, "t = slice(s, 0, len(s))", 16+8)
	}
	add("ledger/list_literal_then_append", ledger("l = [1, 2, 3]\nappend(l, 4)\n", 100))
	add("ledger/list_literal_sixteen_then_append", ledger("l = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]\nappend(l, 17)\n", 400))
	add("ledger/list_literal_fifteen_then_appends", ledger("l = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]\nappend(l, 16)\nappend(l, 17)\n", 600))
	add("ledger/two_lists_one_call", ledger("a = range(10)\nb = range(10)\npush(a, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12)\n", 800))
	add("ledger/append_to_itself", ledger("l = range(10)\nappend(l, l)\n", 800))
	add("ledger/append_list_to_itself_twice", ledger("l = range(10)\nappend(l, l, l)\n", 800))
	add("ledger/same_list_as_two_arguments", ledger("l = range(5)\nappend(l, l, l, l, l, l, l, l)\n", 800))
	add("ledger/set_and_list_arguments", ledger("l = range(5)\nm = {a: 1}\nappend(l, m, [1], m, [2], m, [3], m, [4], l)\n", 800))

	// The fill loops: how many pushes of a long list fit before the limit, which follows the growth rule at
	// every step.
	for _, k := range []int{100, 500, 1000, 2000, 3000, 5000, 10000} {
		add(fmt.Sprintf("fill/append_spread_%d", k), fmt.Sprintf("l = range(0)\nxs = range(%d)\ni = 0\nwhile (true):\n    append(l, xs...)\n    i += 1\n    print(i, len(l))\nend\n", k))
		add(fmt.Sprintf("fill/unshift_spread_%d", k/10+1), fmt.Sprintf("l = range(0)\nxs = range(%d)\ni = 0\nwhile (true):\n    unshift(l, xs...)\n    i += 1\n    print(i, len(l))\nend\n", k/10+1))
		add(fmt.Sprintf("fill/push_shift_spread_%d", k), fmt.Sprintf("l = range(%d)\nxs = range(%d)\ni = 0\nwhile (true):\n    shift(l)\n    push(l, xs...)\n    i += 1\n    print(i, len(l))\nend\n", k, k/50+1))
		add(fmt.Sprintf("fill/plus_%d", k), fmt.Sprintf("l = range(0)\nxs = range(%d)\ni = 0\nwhile (true):\n    l = l + xs\n    i += 1\n    print(i, len(l))\nend\n", k))
	}
	return cs
}

// queueSpent is what `iters` rounds of shift and append charge a list of k elements: eight bytes for the
// integer shift returns, and for the append the whole new array when the capacity ran out, else sixteen.
func queueSpent(k, iters int) int {
	total, length, c := 0, k, k
	for i := 0; i < iters; i++ {
		total += 8
		length--
		c--
		length++
		if length > c {
			c = goCap(c, length)
			total += 24 + 16*c
		} else {
			total += 16
		}
	}
	return total
}

// capCount is how many times a slice that grows one element at a time moves to a new array on the way to n.
func capCount(n int) int {
	return len(capSteps(n))
}

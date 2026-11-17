package main

import "fmt"

func init() {
	libRegistry = append(libRegistry, handGroup("lib-shared", sharedCases()))
}

// sharedCases exercise what every group of builtins relies on: an error a builtin returns as a value, and the
// guards and reservations the sandbox puts around a call. date_add returns errors as values (the first argument
// is not a date string, or no layout parses it, with the input quoted in the text) and range has guards, a
// reservation and fresh elements.

const errValue = "x = date_add(1, \"x\", 1)\n"

func sharedCases() []handCase {
	var cs []handCase
	add := func(name, src string) { cs = append(cs, handCase{name, nil, src}) }

	add("errvalue/print_and_type", errValue+"print(x)\nprint(str(x))\nprint(typeof(x))\nprint(len(str(x)))\nprint(typeof(str(x)))\n")
	add("errvalue/in_containers", errValue+"print([x, 1, \"a\"])\nprint({k: x})\nprint(str([[x]]))\nprint(typeof([x]))\nprint(len([x, x]))\n")
	add("errvalue/stored_and_passed", errValue+"fun f(v):\n    return v\nend\nprint(f(x))\nl = [x]\nl[0] = x\nprint(l[0])\nm = {}\nm[\"k\"] = x\nprint(m.k)\nprint(typeof(m.k))\nprint(typeof(f(x)))\n")
	add("errvalue/if_raises", errValue+"if (x) then:\n    print(\"then\")\nend\nprint(\"after\")\n")
	add("errvalue/if_raises_in_function", errValue+"fun f():\n    if (x) then:\n        return 1\n    end\n    return 2\nend\nprint(f())\n")
	add("errvalue/if_else_raises", errValue+"if (x) then:\n    print(\"then\")\nelse:\n    print(\"else\")\nend\n")
	add("errvalue/elif_raises", errValue+"if (false) then:\n    print(\"then\")\nelif (x) then:\n    print(\"elif\")\nend\n")
	add("errvalue/while_raises", errValue+"while (x):\n    print(\"loop\")\nend\nprint(\"after\")\n")
	add("errvalue/while_raises_later", errValue+"n = 0\nwhile (n < 3 or x):\n    n += 1\nend\nprint(n)\n")
	add("errvalue/if_caught", errValue+tryStmt("if (x) then:\n        print(\"then\")\n    end")+"print(\"after\")\n")
	add("errvalue/while_caught", errValue+tryStmt("while (x):\n        print(\"loop\")\n    end")+"print(\"after\")\n")
	add("errvalue/caught_text_is_a_string", errValue+"try:\n    if (x) then:\n    end\ncatch (e):\n    print(typeof(e), len(e), e == str(x))\nend\n")
	add("errvalue/raise_twice", errValue+"c = [0]\nwhile (c[0] < 3):\n    try:\n        if (x) then:\n        end\n    catch (e):\n        c[0] = c[0] + 1\n    end\nend\nprint(c[0])\n")
	add("errvalue/equality", errValue+"print(x == x)\nprint(x != x)\nprint(x == str(x))\nprint(str(x) == x)\nprint(x == null)\nprint(null == x)\nprint(x == 1)\nprint([x] == [x])\nprint({k: x} == {k: x})\nprint(x in [x])\nprint(x in [1, 2])\nprint(1 in [x])\ny = x\nprint(x == y)\nprint([x] == [y])\n")
	var arith []string
	for _, op := range []string{"+", "-", "*", "/", "%", "**"} {
		for _, e := range []string{"x " + op + " 1", "1 " + op + " x", "x " + op + " x", "x " + op + " \"a\"", "[1] " + op + " x"} {
			arith = append(arith, e)
		}
	}
	src := errValue
	for _, e := range arith {
		src += tryPrint(e)
	}
	add("errvalue/arithmetic", src)
	src = errValue
	for _, e := range []string{"x < 1", "1 < x", "x <= x", "x > \"a\"", "x >= x", "[x] < [x]", "[1] < [x]", "-x", "not x", "x and true", "true and x", "x or false", "false or x", "x xor true", "true xor x", "x xor x", "x ? 1 : 2", "true ? x : 1", "1 in x", "x in \"abc\"", "\"a\" in x", "x in {a: 1}", "\"a\" in {a: x}"} {
		src += tryPrint(e)
	}
	add("errvalue/comparison_logic_membership", src)
	src = errValue + "fun f(a):\n    return a\nend\n"
	for _, e := range []string{"x[0]", "[1][x]", "{a: 1}[x]", "\"a\"[x]", "len(x)", "x()", "f(x...)", "str(x...)", "{x: 1}"} {
		src += tryPrint(e)
	}
	for _, st := range []string{"x[0] = 1", "l = [1]\n    l[x] = 2", "for (c in x):\n        print(c)\n    end", "x.k = 1", "x += 1", "x -= 1"} {
		src += tryStmt(st)
	}
	add("errvalue/subscripts_calls_loops", src)
	add("errvalue/typeof_of_each_kind", errValue+"print(typeof(x), typeof(str(x)), typeof(len), typeof(null), typeof(x == x))\n")

	// The text of an error value is cut to 1,024 bytes, "..." after the cut, once a builtin returns it. The text
	// of date_add is 99 bytes and two per byte of the input.
	for _, n := range []int{400, 462, 463, 464, 600, 3000} {
		add(fmt.Sprintf("errvalue/cut_%d", n), fmt.Sprintf("x = date_add(\"a\" * %d, \"x\", 1)\ns = str(x)\nprint(len(s))\nprint(s[len(s) - 3], s[len(s) - 2], s[len(s) - 1])\nprint(len(s) == len(str(x)))\n", n))
	}
	add("errvalue/cut_text_raised", "x = date_add(\"a\" * 600, \"x\", 1)\ntry:\n    if (x) then:\n    end\ncatch (e):\n    print(len(e))\nend\n")
	// What an error value costs: its text and a header. 600 bytes in, 1,027 out.
	add("errvalue/memory_long", "i = 0\nwhile (i < 100000):\n    x = date_add(\"a\" * 600, \"x\", 1)\n    i += 1\nend\nprint(i)\n")
	add("errvalue/memory_long_caught", "i = 0\nwhile (i < 100000):\n    try:\n        x = date_add(\"a\" * 600, \"x\", 1)\n        i += 1\n    catch (e):\n        i += 0\n    end\nend\nprint(i)\n")
	add("errvalue/many_short", "i = 0\nwhile (i < 5000):\n    x = date_add(1, \"x\", 1)\n    i += 1\nend\nprint(i)\n")
	add("errvalue/memory_kept_in_a_list", "l = []\ni = 0\nwhile (i < 100000):\n    l = l + [date_add(\"a\" * 450, \"x\", 1)]\n    i += 1\nend\nprint(i)\n")

	// Guards, reservations and fresh elements of range: pre-call size checks, a worst-case charge of 24 + 40 per
	// element settled to 24 + 24 per element after the call.
	for _, n := range []int{1048575, 1048576, 1048577, 2000000, 104856, 104857, 104858} {
		add(fmt.Sprintf("range/one_arg_%d", n), fmt.Sprintf("x = range(%d)\nprint(len(x))\n", n))
	}
	add("range/two_args", "print(range(2, 5), range(5, 2), range(-3, 0), range(0, 0), len(range(100, 300)))\n")
	add("range/zero_and_small", "print(range(0), range(1), range(3))\nprint(typeof(range(0)))\n")
	// The arguments of range are bounded by 2^40 before any arithmetic on them, and a span over the value length
	// limit stops the run before the reservation is made. A limit ends the run, so each expression has a case.
	for k, e := range []string{
		"range(1099511627775, 1099511627776)",
		"range(1099511627777)",
		"range(-1099511627777, 0)",
		"range(0, 2199023255552)",
		"range(0, 2199023255552, 1)",
		"range(-2199023255552, 5, 1)",
		"range(0, 2000000)",
		"range(-1000000, 1000000)",
		"range(0, 1048577, 1)",
		"range(0, 1048576, 1)",
		"range(0, 2000000, 2)",
		"range(0, 2097152, 2)",
		"range(0, 2097153, 2)",
		"range(0, 4000000, 4)",
	} {
		add(fmt.Sprintf("range/args_%02d", k), fmt.Sprintf("x = %s\nprint(typeof(x))\n", e))
	}
	add("range/non_integer", tryPrint("range(\"a\")")+tryPrint("range(2.5)")+tryPrint("range(null)")+tryPrint("range(1, \"a\")")+tryPrint("range(0.5, 3)")+tryPrint("range(\"a\", \"b\")"))
	add("range/negative", tryPrint("range(-1)")+tryPrint("range(-5)")+"print(len(range(-3, -1)))\n")
	add("range/reservation_is_settled", "x = range(50000)\ny = range(50000)\nprint(len(x) + len(y))\nz = range(10000)\nprint(len(z))\n")
	add("range/reservation_is_settled_then_over", "x = range(50000)\ny = range(50000)\nz = range(50000)\nprint(len(z))\n")
	add("range/after_a_big_range", "x = range(104857)\nprint(len(x))\ny = range(60000)\nprint(len(y))\n")
	add("range/after_a_big_range_fits", "x = range(104857)\ny = range(30000)\nprint(len(y))\n")
	add("range/repeated", "i = 0\nwhile (i < 3000):\n    x = range(100)\n    i += 1\nend\nprint(i)\n")
	add("range/repeated_large", "i = 0\nwhile (i < 100):\n    x = range(5000)\n    i += 1\nend\nprint(i)\n")
	add("range/memory_caught", "try:\n    x = range(104858)\ncatch (e):\n    print(\"caught\")\nend\nprint(\"after\")\n")
	add("range/guard_caught", "try:\n    x = range(2000000)\ncatch (e):\n    print(\"caught\")\nend\nprint(\"after\")\n")
	return cs
}

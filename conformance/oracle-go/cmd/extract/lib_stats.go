package main

import (
	"fmt"
	"strings"
)

func init() {
	libRegistry = append(libRegistry, handGroup("lib-stats", statsCases()))
}

// pureNumPool are the numbers every numeric builtin is tried with: zeros of both signs, the edges of the
// integers and of the floats that count, and the special floats. Expressions are used where a literal would not
// parse (the most negative integer) or would not be the value (negative zero, NaN).
var pureNumPool = []string{
	"0", "1", "-1", "2", "7", "12", "-12", "18", "100", "0.0", "-0.0", "0.5", "-0.5", "2.5", "-2.5", "12.7", "-3.9", "1e-300", "5e-324",
	"1e300", "1.7976931348623157e308", "9007199254740992", "9007199254740993", "-9007199254740993", "4611686018427387904",
	"9223372036854775807", "(-9223372036854775807 - 1)", "1e19", "-1e19", "9.223372036854775807e18", "inf", "-inf", "nan",
}

var pureNumSmall = []string{"0", "1", "-1", "5", "2.5", "-0.0", "1e300", "9223372036854775807", "(-9223372036854775807 - 1)", "9007199254740993", "inf", "nan"}

func statsCases() []handCase {
	var cs []handCase
	add := func(name, src string) { cs = append(cs, handCase{name, nil, pureUni(src)}) }
	addAll := func(more []handCase) { cs = append(cs, more...) }
	notNumbers := []string{`"a"`, `""`, "null", "true", "[1]", "[]", "{a: 1}", "f", "len", "lead", "mixed"}

	// One argument.
	for _, fn := range []string{"abs", "sign"} {
		var calls []string
		for _, a := range append(append([]string{}, pureNumPool...), notNumbers...) {
			calls = append(calls, fn+"("+a+")")
		}
		calls = append(calls, fn+"()", fn+"(1, 2)", fn+"(1, 2, 3)")
		addAll(purePack("one/"+fn, pureInvalid, calls, 50))
	}

	// clamp, lerp: three arguments.
	for _, fn := range []string{"clamp", "lerp"} {
		calls := pureCalls(fn, pureNumSmall, pureNumSmall, pureNumSmall)
		for _, a := range notNumbers {
			calls = append(calls, fn+"("+a+", 1, 2)", fn+"(1, "+a+", 2)", fn+"(1, 2, "+a+")", fn+"("+a+", "+a+", "+a+")")
		}
		calls = append(calls, fn+"()", fn+"(1)", fn+"(1, 2)", fn+"(1, 2, 3, 4)")
		addAll(purePack("three/"+fn, pureInvalid, calls, 60))
	}
	var more []string
	for _, t := range []string{"0", "1", "0.25", "0.5", "-1", "2", "0.1", "0.3", "1e-9", "1e308", "nan", "inf"} {
		for _, ab := range [][2]string{{"0", "10"}, {"1", "2"}, {"0.1", "0.2"}, {"-5", "5"}, {"1e308", "-1e308"}, {"0.1", "0.7"}, {"1e16", "1"}, {"3", "3"}} {
			more = append(more, fmt.Sprintf("lerp(%s, %s, %s)", ab[0], ab[1], t))
		}
	}
	for _, c := range [][3]string{{"5", "1", "10"}, {"-5", "1", "10"}, {"15", "1", "10"}, {"5.5", "1", "10"}, {"5", "1.5", "10"}, {"5", "1", "10.5"}, {"15", "1", "10.5"},
		{"9007199254740993", "0", "9007199254740995"}, {"9223372036854775807", "0", "9223372036854775807"}, {"(-9223372036854775807 - 1)", "(-9223372036854775807 - 1)", "0"},
		{"4611686018427387905", "0", "4611686018427387905"}, {"4", "5", "5"}, {"6", "5", "5"}, {"5", "10", "1"}, {"5", "nan", "1"}, {"nan", "1", "5"}, {"nan", "nan", "nan"}, {"0", "-0.0", "0"}, {"-0.0", "0", "5"}, {"3", "0", "3"}} {
		more = append(more, fmt.Sprintf("clamp(%s, %s, %s)", c[0], c[1], c[2]), fmt.Sprintf("typeof(clamp(%s, %s, %s))", c[0], c[1], c[2]))
	}
	addAll(purePack("three/special", pureInvalid, more, 60))

	// gcd, lcm: two numbers.
	for _, fn := range []string{"gcd", "lcm"} {
		calls := pureCalls(fn, pureNumPool, pureNumPool)
		for _, a := range notNumbers {
			calls = append(calls, fn+"("+a+", 1)", fn+"(1, "+a+")", fn+"("+a+", "+a+")")
		}
		calls = append(calls, fn+"()", fn+"(1)", fn+"(1, 2, 3)")
		addAll(purePack("two/"+fn, pureInvalid, calls, 60))
	}

	// max, min: arguments or a list.
	pool := []string{"1", "2", "-3", "2.5", "1.0", "0.0", "-0.0", "nan", "inf", `"a"`, `"b"`, `"ab"`, `""`, "[1]", "[1, 2]", "[2]", "[]", `["a"]`, "null", "true", "{a: 1}", "f", "len", "lead", "mixed"}
	for _, fn := range []string{"max", "min"} {
		calls := pureCalls(fn, pool)
		calls = append(calls, pureCalls(fn, pool, pool)...)
		for _, l := range []string{"[]", "[5]", "[1, 2, 3]", "[3, 2, 1]", "[2, 3, 1]", "[1, 2.5, 2]", "[1, 1.0]", "[1.0, 1]", "[0.0, -0.0]", "[-0.0, 0.0]", "[nan, 1]", "[1, nan]", "[nan, nan]", "[inf, -inf]",
			`["b", "a", "c"]`, `["a", "A", ""]`, `[lead, cont, "a"]`, `[1, "a"]`, `["a", 1]`, "[[1, 2], [1, 3]]", "[[1, 2], [1]]", "[[2], [1, 5]]", `[[1], ["a"]]`, "[null, null]", "[true, false]", "[f, f]", "[len, len]", "[[]]", "[[], []]", "[{a: 1}, {a: 2}]", "[9223372036854775807, 9.223372036854775807e18]", "[9007199254740993, 9007199254740992.0]"} {
			calls = append(calls, fn+"("+l+")", fn+"("+l+", 100)", fn+"(100, "+l+")")
		}
		for _, args := range []string{"1, 2, 3", "3, 2, 1", "2, 3, 1", "1, 2.5, 2", "1, 1.0", "1.0, 1", `"b", "a", "c"`, "1, nan, 2", "nan, 1, 2", "1, 2, nan", "-0.0, 0.0", "0.0, -0.0", "[1], [2], [0]", "1, 2, 3, 4, 5, 6, 7, 8", `1, "a", 2`, "1, 2, null", "9223372036854775807, 9.223372036854775807e18"} {
			calls = append(calls, fn+"("+args+")")
		}
		calls = append(calls, fn+"()")
		addAll(purePack("extreme/"+fn, pureInvalid, calls, 60))
		add("extreme/"+fn+"_results", pureInvalid+fmt.Sprintf(`a = [1, 2.5, 2]
r = %s(a)
print(r, typeof(r))
r = %s(1, 1.0)
print(r, typeof(r))
r = %s([1.0, 1])
print(r, typeof(r))
r = %s(0.0, -0.0)
print(str(r))
r = %s(-0.0, 0.0)
print(str(r))
n = %s([[1, 2], [1, 3]])
print(n)
n[0] = 99
print(n)
`, fn, fn, fn, fn, fn, fn))
	}

	// Lists: sum mean median mode std_dev variance.
	lists := []string{"[]", "[5]", "[0.1]", "[1, 2, 3]", "[1, 2, 3, 4]", "[3, 1, 2]", "[1.5, 2.5]", "[1, 2.5]", "[2.5, 1]", "[1.0, 2.0]", "[1, 2, 2, 3, 3, 3]", "[1, 1.0, 2, 2.0, 2]", `[1, "1", 1, "1", "1"]`,
		"[0.1, 0.2, 0.3]", "[0.3, 0.2, 0.1]", "[0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0]", "[1e16, 1, -1e16]", "[1, 1e16, -1e16]", "[-1e16, 1e16, 1]", "[9007199254740993, 1]", "[9007199254740993, 9007199254740993]",
		"[9223372036854775807, 1]", "[9223372036854775807, 9223372036854775807]", "[-9223372036854775807, -9223372036854775807, -9223372036854775807]", "[4611686018427387904, 4611686018427387904]", "[1e308, 1e308]", "[1e308, 1e308, -1e308]",
		"[nan]", "[1, nan]", "[nan, 1, 2]", "[inf]", "[inf, -inf]", "[inf, 1]", "[-inf, 1, 2]", "[0.0, -0.0]", "[-0.0, 0.0]", "[-0.0]", "[-0.0, -0.0]", "[0.0, -0.0, 0.0]", "[-0.0, 0.0, -0.0]", "[1e-300, 1e-300]", "[5e-324, 5e-324, 5e-324]",
		`["a"]`, `[1, "a"]`, `["a", 1]`, "[1, null]", "[true]", "[[1]]", "[{a: 1}]", "[f]", "[len]", "[lead]", `["a", "b", "a"]`, "[[1], [1], [2]]", `[null, null, 1]`, "[{a: 1}, {a: 1}, {b: 2}]", "[[1, [2]], [1, [2]], 3]",
		"[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]", "[13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1]", "[5, 3, 8, 1, 9, 2, 7, 4, 6, 0, 11, 10, 15, 13, 12, 14]", "[2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3]"}
	for _, fn := range []string{"sum", "mean", "median", "std_dev", "variance"} {
		var calls []string
		for _, l := range lists {
			calls = append(calls, fn+"("+l+")")
		}
		for _, a := range append(append([]string{}, pureNumPool[:12]...), notNumbers...) {
			calls = append(calls, fn+"("+a+")")
		}
		calls = append(calls, fn+"()", fn+"([1], [2])", fn+"(1, 2, 3)")
		addAll(purePack("list/"+fn, pureInvalid, calls, 60))
	}
	// mode: a tie goes to any of the values with the top count, so a list whose top count is shared is a case of its
	// own, compared by shape; the others have one value that occurs most.
	modeUnique := []string{"[]", "[5]", "[0.1]", "[1, 2, 2, 3, 3, 3]", "[1, 1.0, 2, 2.0, 2]", `[1, "1", 1, "1", "1"]`, "[9007199254740993, 9007199254740993]", "[9223372036854775807, 9223372036854775807]",
		"[-9223372036854775807, -9223372036854775807, -9223372036854775807]", "[4611686018427387904, 4611686018427387904]", "[1e308, 1e308]", "[1e308, 1e308, -1e308]", "[nan]", "[inf]", "[-0.0]", "[-0.0, -0.0]",
		"[0.0, -0.0, 0.0]", "[-0.0, 0.0, -0.0]", "[1e-300, 1e-300]", "[5e-324, 5e-324, 5e-324]", `["a"]`, "[true]", "[[1]]", "[{a: 1}]", "[f]", "[len]", "[lead]", `["a", "b", "a"]`, "[[1], [1], [2]]", `[null, null, 1]`,
		"[{a: 1}, {a: 1}, {b: 2}]", "[[1, [2]], [1, [2]], 3]", "[2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3]"}
	isUnique := map[string]bool{}
	var modes []string
	for _, l := range modeUnique {
		isUnique[l] = true
		modes = append(modes, "mode("+l+")")
	}
	for _, a := range append(append([]string{}, pureNumPool[:12]...), notNumbers...) {
		modes = append(modes, "mode("+a+")")
	}
	modes = append(modes, "mode()", "mode([1], [2])", `mode([3, 1, 3, 1, 3, 2])`, `mode(["a", "b", "b", "c", "b", "a"])`, `mode([1, 2, 2, 1.0, 1.0, 1])`, `mode([[1], [2], [2], [1], [2]])`, `mode(["1", 1, 1, "1", "1"])`, `mode([0.1, 0.1, 0.2])`, `mode([-0.0, 0.0, 0.0, -0.0, -0.0])`)
	addAll(purePack("list/mode", pureInvalid, modes, 60))
	// Only the ties among at least seven values go in: Go picks one at random, and a tie of two would repeat the
	// same pick in all eight recorded runs one time in 128.
	for k, l := range lists {
		if !isUnique[l] && strings.Count(l, ",") >= 9 {
			add(fmt.Sprintf("list/mode_tie_%02d", k), pureInvalid+"print(mode("+l+"))\n")
		}
	}
	add("list/mode_returns_the_last_equal", pureInvalid+"r = mode([1, 1.0, 2])\nprint(r, typeof(r))\nr = mode([1.0, 1, 2])\nprint(r, typeof(r))\nr = mode([\"a\", \"b\", \"b\"])\nprint(r)\nr = mode([[1], [1], [2]])\nprint(r)\nl = [[1], [1]]\nr = mode(l)\nr[0] = 7\nprint(l)\n")
	add("list/mode_wide_ties", pureInvalid+"print(mode([1, 2, 3, 4, 5, 6, 7, 8]))\n")
	add("list/mode_wide_ties_in_pairs", pureInvalid+"print(mode([1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7]))\n")
	add("list/mode_wide_ties_of_strings", pureInvalid+"print(mode([\"a\", \"b\", \"c\", \"d\", \"e\", \"f\", \"g\", \"h\", \"i\"]))\n")
	add("list/results_kinds", pureInvalid+"print(typeof(sum([1, 2])), typeof(sum([1, 2.0])), typeof(sum([])), typeof(mean([1, 2])), typeof(median([1, 2])), typeof(std_dev([1])), typeof(variance([])), typeof(std_dev([1, 3])))\nprint(sum([1, 2]), sum([1, 2.0]), sum([]), mean([1, 2]), median([1, 3]), median([1, 2, 4]), std_dev([1]), variance([]), std_dev([1, 3]), variance([1, 2, 3, 4]))\n")

	// Work in the order of the list: the sums of a list in another order differ in the last bits.
	add("list/order_of_addition", pureInvalid+`a = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0, 1e16, -1e16, 3.3, 7.7]
b = [7.7, 3.3, -1e16, 1e16, 1.0, 0.9, 0.8, 0.7, 0.6, 0.5, 0.4, 0.3, 0.2, 0.1]
print(str(sum(a)), str(sum(b)), str(mean(a)), str(mean(b)), str(variance(a)), str(variance(b)), str(std_dev(a)), str(median(a)), str(median(b)))
`)

	// The median sorts: not-a-number first, and the two zeros in the order the sort leaves them.
	for _, n := range []int{1, 2, 3, 5, 7, 9, 11, 12, 13, 14, 15, 21, 31, 49, 50, 51, 99, 100, 101, 200, 201} {
		add(fmt.Sprintf("median/zeros_%d", n), pureInvalid+fmt.Sprintf(`n = %d
i = 0
while (i < 5):
    l = [0.0] * n
    j = 0
    while (j < n):
        if ((j * (i + 1) + i) %% (i + 2) < 1 + i %% 2) then:
            l[j] = -0.0
        end
        j += 1
    end
    print(i, str(median(l)), str(median(l + [1.0])))
    i += 1
end
`, n))
	}
	for _, pat := range []struct{ name, expr string }{
		{"ascending", "j"}, {"descending", "n - j"}, {"constant", "7"}, {"sawtooth", "j % 7"}, {"organ_pipe", "min(j, n - j)"}, {"pseudo_random", "(j * 7919 + 13) % 1009"},
		{"few_values", "j % 3"}, {"nan_sprinkled", "(j % 5 == 0) ? nan : j"}, {"inf_and_zero", "(j % 4 == 0) ? inf : ((j % 4 == 1) ? -inf : ((j % 4 == 2) ? 0.0 : -0.0))"},
	} {
		for _, n := range []int{10, 13, 60, 300, 1001} {
			add(fmt.Sprintf("median/%s_%d", pat.name, n), pureInvalid+fmt.Sprintf(`n = %d
l = [0.0] * n
j = 0
while (j < n):
    l[j] = %s
    j += 1
end
print(str(median(l)), str(median(l + [0.5])), str(median([-0.0] + l)))
`, n, pat.expr))
		}
	}

	// Limits: the walk of a deep builtin costs an operation per element and node, and what a result costs.
	add("limits/max_walks_the_list", pureInvalid+"l = [3] * 40000\ni = 0\nwhile (i < 5):\n    print(i)\n    print(max(l), min(l))\n    i += 1\nend\n")
	add("limits/max_of_arguments_is_not_walked", pureInvalid+"i = 0\nwhile (i < 5000):\n    t = max(1, 2, 3)\n    i += 1\nend\nprint(i)\n")
	add("limits/sum_is_not_walked", pureInvalid+"l = [3] * 90000\ni = 0\nwhile (i < 5):\n    print(sum(l), mean(l), median(l), variance(l), std_dev(l))\n    i += 1\nend\n")
	add("limits/mode_walks_the_list", pureInvalid+"l = [3] * 30000\ni = 0\nwhile (i < 5):\n    print(i)\n    print(mode(l))\n    i += 1\nend\n")
	add("limits/mode_text_size_bound", pureInvalid+"l = [\"a\" * 1000] * 600\nprint(len(mode(l)))\n")
	add("limits/mode_text_size_bound_over", pureInvalid+"l = [\"a\" * 1000] * 1100\nprint(len(mode(l)))\n")
	add("limits/mode_nested_depth", pureInvalid+"l = [1]\ni = 0\nwhile (i < 255):\n    l = [l]\n    i += 1\nend\nprint(typeof(mode([l])))\nl = [l]\nprint(typeof(mode(l)))\n")
	add("limits/mode_self_reference", pureInvalid+"l = [1]\nl[0] = l\nprint(mode(l))\n")
	add("limits/max_self_reference", pureInvalid+"l = [1, 2]\nl[0] = l\nprint(max(l))\n")
	add("limits/max_nested_depth", pureInvalid+"l = [1]\ni = 0\nwhile (i < 300):\n    l = [l]\n    i += 1\nend\nprint(typeof(max(l, l)))\n")
	add("limits/max_of_nested_lists", pureInvalid+"l = [[1, 2], [1, 3], [0, 9]]\nprint(max(l), min(l), max(l, [5]), min([[1], [1, 0]]))\n")
	add("limits/result_costs", pureInvalid+"i = 0\nwhile (i < 1000):\n    a = abs(-5)\n    b = sum([1, 2])\n    c = mean([1, 2])\n    d = max([\"x\" * 100, \"y\"])\n    i += 1\nend\nprint(i, d == \"y\")\n")
	add("limits/big_strings_through_max_cost_bytes", pureInvalid+"l = [\"a\" * 100000, \"b\" * 100000]\ni = 0\nwhile (i < 100):\n    t = max(l)\n    print(i)\n    i += 1\nend\n")
	add("limits/error_values_cost_their_text", pureInvalid+"i = 0\nwhile (i < 3000):\n    t = sign(\"a\")\n    u = gcd(null, 1)\n    i += 1\nend\nprint(i, t)\n")
	add("limits/error_value_is_cut_to_1024", pureInvalid+"x = clamp([\"a\" * 5000], 1, 2)\nprint(len(str(x)))\n")
	add("errors/as_values", pureInvalid+`x = sign("a")
y = clamp(1, "b", 3)
z = gcd(1, null)
w = lcm([], 2)
v = lerp(1, 2, {})
print(x)
print(y)
print(z)
print(w)
print(v)
print(typeof(x), typeof(y), [x, y], len(str(z)))
`)
	add("errors/caught_as_text", pureInvalid+"x = lerp(1, 2, \"t\")\ntry:\n    if (x) then:\n    end\ncatch (e):\n    print(e)\nend\n")
	add("errors/arithmetic_on_a_value", pureInvalid+"x = sign(\"a\")\ntry:\n    print(x + 1)\ncatch (e):\n    print(e)\nend\n")
	return cs
}

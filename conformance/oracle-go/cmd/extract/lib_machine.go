package main

import (
	"fmt"
	"strings"
)

func init() {
	libRegistry = append(libRegistry, handGroup("lib-machine", machineCases()))
}

// The scripts of two things the machine does for every builtin. An error a builtin returns as a value has the
// class the reference gave it (a type error, a value error), and an `if` or `while` that raises it uncaught ends
// the run with that class, where a plain error ends it as an unexpected runtime error. And `**` is the same
// function as `pow`, whose logarithm reads the exponent bits of a subnormal as they are.

func machineCases() []handCase {
	var cs []handCase
	add := func(name, src string) { cs = append(cs, handCase{name, nil, src}) }

	bad := []string{"\"a\"", "null", "[]", "true", "{}"}
	type call struct {
		name string
		args []string
	}
	var calls []call
	for _, n := range []string{"sign", "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh", "exp", "exp2", "log", "log10", "log2", "cbrt", "sqrt", "radians", "degrees", "ceil", "floor", "trunc", "round"} {
		for _, b := range bad {
			calls = append(calls, call{n, []string{b}})
		}
	}
	for _, n := range []string{"atan2", "logb", "pow", "gcd", "lcm", "round", "float"} {
		for _, p := range [][2]string{{"\"a\"", "1"}, {"1", "\"a\""}, {"null", "1"}, {"1", "null"}, {"[]", "[]"}, {"1.5", "(-1)"}, {"1.5", "(-100)"}, {"1.5", "sign(\"a\")"}, {"1.5", "sin(null)"}, {"1.5", "gcd(null, 1)"}, {"1.5", "[]"}} {
			calls = append(calls, call{n, []string{p[0], p[1]}})
		}
	}
	for _, n := range []string{"clamp", "lerp"} {
		for _, t := range [][3]string{{"\"a\"", "1", "2"}, {"1", "\"a\"", "2"}, {"1", "2", "\"a\""}, {"null", "null", "null"}, {"[]", "1", "2"}} {
			calls = append(calls, call{n, []string{t[0], t[1], t[2]}})
		}
	}
	for i, c := range calls {
		expr := fmt.Sprintf("%s(%s)", c.name, strings.Join(c.args, ", "))
		add(fmt.Sprintf("class/if_%03d_%s", i, c.name), fmt.Sprintf("x = %s\nprint(typeof(x))\nif (x) then:\n    print(1)\nend\nprint(2)\n", expr))
		if i%2 == 0 {
			add(fmt.Sprintf("class/while_%03d_%s", i, c.name), fmt.Sprintf("x = %s\nprint(typeof(x))\nwhile (x):\n    print(1)\nend\nprint(2)\n", expr))
		}
	}

	// The same values caught, printed and passed on: the text and the type name are those of the class.
	var caught strings.Builder
	for _, c := range calls {
		expr := fmt.Sprintf("%s(%s)", c.name, strings.Join(c.args, ", "))
		caught.WriteString(fmt.Sprintf("x = %s\nprint(typeof(x), str(x))\n", expr))
		caught.WriteString("try:\n    if (x) then:\n        print(1)\n    end\ncatch (e):\n    print(\"caught\", e)\nend\n")
		if caught.Len() > 40000 {
			break
		}
	}
	add("class/caught_and_printed", caught.String())
	add("class/kept_in_values", "x = sign(\"a\")\nl = [x, 1]\nm = {k: x}\nf = fun ():\n    return x\nend\nprint(typeof(l[0]), typeof(m.k), typeof(f()), str(l))\nif (l[0]) then:\n    print(1)\nend\n")
	add("class/kept_in_map", "x = clamp(\"a\", 1, 2)\nm = {k: x}\nwhile (m.k):\n    print(1)\nend\n")
	add("class/raised_in_a_function", "x = sign(null)\nfun check(v):\n    if (v) then:\n        return 1\n    end\n    return 2\nend\nprint(check(x))\n")
	add("class/raised_in_a_callback", "x = lerp(\"a\", 1, 2)\nprint(map([1], fun (n):\n    if (x) then:\n        return 1\n    end\nend))\n")
	add("class/raised_in_a_loop", "x = float(1.5, -1)\nfor (i in [1, 2]):\n    print(i)\n    if (x) then:\n        print(\"no\")\n    end\nend\n")
	add("class/value_error_from_float", "x = float(1.5, -1)\nprint(typeof(x), str(x))\nif (x) then:\n    print(1)\nend\n")
	add("class/plain_errors_are_unexpected", "x = path_join(1)\nprint(typeof(x))\nif (x) then:\n    print(1)\nend\n")
	add("class/plain_errors_while", "x = path_join()\nwhile (x):\n    print(1)\nend\n")
	add("class/error_value_as_argument", "x = sign(\"a\")\nfor (f in [str, typeof, len, print, abs, sign, sin, float, int, max, sum, json_stringify, upper, join, set_new, stack_push]):\n    try:\n        print(f(x))\n    catch (e):\n        print(\"caught\", e)\n    end\nend\n")
	add("class/type_assertion_names_the_class", "for (v in [sign(\"a\"), float(1.5, -1), path_join(1), sin(null), clamp(null, 1, 2)]):\n    try:\n        print(float(1.5, v))\n    catch (e):\n        print(\"caught\", e)\n    end\nend\nprint(float(1.5, sign(\"a\")))\n")
	add("class/type_assertion_uncaught", "print(float(1.5, sin(\"a\")))\n")
	add("class/type_assertion_uncaught_plain", "print(float(1.5, path_join(1)))\n")
	add("class/type_assertion_uncaught_value", "print(float(1.5, float(1.5, -1)))\n")

	// `**` and `pow` give the same results, bit for bit, whatever the base: a subnormal one, a binade edge.
	bases := []string{"1e-310", "5e-324", "1e-320", "1e-315", "2.225073858507201e-308", "2.2250738585072014e-308", "1e-300", "1e-200", "1e-100", "0.5", "0.7071067811865476", "0.7071067811865475", "0.7071067811865477", "0.35355339059327373", "0.3535533905932737", "1.4142135623730951", "1.414213562373095", "1.4142135623730954", "0.1", "0.9", "1.1", "1.5", "2", "3", "7", "10", "1e100", "1e300", "1.7976931348623157e308", "(-1e-310)", "(-5e-324)", "(-2)", "(-0.5)", "(-1.5)", "(-1e300)"}
	exps := []string{"0.3", "1.5", "2.5", "(-0.3)", "(-1.5)", "0.3333333333333333", "100.7", "(-100.7)", "0.0001", "7.3", "0.001", "0.9999999999999999", "1.0000000000000002", "2.9999999999999996", "0.1", "123.456", "(-0.7)", "0.75", "1.25", "(-1.25)", "50.5", "(-50.5)", "1000.5", "(-1000.5)", "2", "3", "4", "5", "10", "(-2)", "(-3)", "(-5)", "100", "1000", "(-1000)", "0.5", "(-0.5)", "1", "0", "1e-300", "1e300", "(-1e300)", "0.9", "3.5", "12.75"}
	for i := 0; i < len(bases); i += 5 {
		end := i + 5
		if end > len(bases) {
			end = len(bases)
		}
		var b strings.Builder
		for _, base := range bases[i:end] {
			for _, e := range exps {
				b.WriteString(fmt.Sprintf("print(str(%s ** %s), str(pow(%s, %s)))\n", base, e, base, e))
			}
		}
		add(fmt.Sprintf("pow/matrix_%02d", i/5), b.String())
	}
	var logs strings.Builder
	for _, x := range []string{"1e-310", "5e-324", "1e-320", "2.225073858507201e-308", "2.2250738585072014e-308", "0.7071067811865476", "0.7071067811865475", "0.7071067811865477", "1.4142135623730951", "1.414213562373095", "0.35355339059327373", "0.3535533905932737", "0.5", "2", "1e-300", "1e300"} {
		logs.WriteString(fmt.Sprintf("print(str(log(%s)), str(log2(%s)), str(log10(%s)), str(exp(%s)), str(exp(log(%s))), str(%s ** 0.3), str(%s ** 2.5))\n", x, x, x, x, x, x, x))
	}
	add("pow/log_and_exp_at_the_edges", logs.String())
	add("pow/subnormal_walk", "x = 5e-324\nfor (i in range(60)):\n    print(str(x ** 0.3), str(pow(x, 0.3)), str(log(x)))\n    x = x * 2\nend\n")
	add("pow/subnormal_base_integer_exponents", "for (e in [2, 3, 4, 5, 6, 7, 8, 9, 10, 20, 50, 100, 300, 1000]):\n    print(str(1e-310 ** e), str(pow(1e-310, e)), str(5e-324 ** e), str(1e-5 ** e), str(1e-5 ** (-e)))\nend\n")

	// Text of a float that is halfway between two texts of the same length: the one with the even digit.
	add("floattext/ties", "for (b in [1435027691564067.0, 4503599627370496.0, 72057594037927936.0, 9007199254740992.0, 1125899906842624.0, 281474976710656.0, 562949953421312.0]):\n    for (i in range(16)):\n        x = b + i * 0.25\n        print(str(x), str(x * 2), str(x * 4), str(x * 3), json_stringify(x))\n    end\nend\n")
	add("floattext/ties_by_halves", "for (b in [4503599627370496.0, 2251799813685248.0, 1125899906842624.0, 140737488355328.0]):\n    for (i in range(12)):\n        x = b + i * 0.5\n        print(str(x), str(x / 3), str(x * 5), json_stringify(x + 0.25))\n    end\nend\n")
	add("floattext/pow_result", "print(str(1.4142135623730954 ** 100.7), str(1.4142135623730954 ** 101), str(1.4142135623730951 ** 100.7))\n")
	return cs
}

package main

import "fmt"

func init() {
	libRegistry = append(libRegistry, handGroup("lib-math", mathCases()))
}

// The values of the float functions' arguments: the pool of the aggregates, and the arguments at which the
// algorithms change branch (the range of exp, the thresholds of the hyperbolic and arc functions, the places
// where a trigonometric argument is reduced).
var pureMathPool = append(append([]string{}, pureNumPool...),
	"2.0", "10", "3.0", "1e10", "123456.789", "-123456.789", "0.1", "0.7071067811865476", "0.7", "-0.7", "2.414213562373095", "0.66", "21", "22", "709", "710", "-745", "-746", "1e-10", "1024", "1075", "1e15",
	"3.141592653589793", "1.5707963267948966", "6.283185307179586", "536870912", "1e22", "0.625", "44.014845965556527", "-0.99999999999999989", "1.0000000000000002")

func mathCases() []handCase {
	var cs []handCase
	add := func(name, src string) { cs = append(cs, handCase{name, nil, pureUni(src)}) }
	addAll := func(more []handCase) { cs = append(cs, more...) }
	notNumbers := []string{`"a"`, `""`, "null", "true", "[1]", "[]", "{a: 1}", "f", "len", "lead", "mixed"}

	// One argument: the result with every digit, and what the kinds that are not numbers give.
	unary := []string{"acos", "asin", "atan", "cbrt", "cos", "cosh", "exp", "exp2", "log", "log10", "log2", "radians", "degrees", "sin", "sinh", "sqrt", "tan", "tanh"}
	for _, fn := range unary {
		var calls []string
		for _, a := range pureMathPool {
			calls = append(calls, "str("+fn+"("+a+"))", "typeof("+fn+"("+a+"))")
		}
		for _, a := range notNumbers {
			calls = append(calls, fn+"("+a+")")
		}
		calls = append(calls, fn+"()", fn+"(1, 2)", fn+"(1, 2, 3)")
		addAll(purePack("one/"+fn, pureInvalid, calls, 50))
	}
	for _, fn := range []string{"ceil", "floor", "trunc", "is_nan", "is_infinite"} {
		var calls []string
		for _, a := range pureMathPool {
			calls = append(calls, "str("+fn+"("+a+"))", "typeof("+fn+"("+a+"))")
		}
		for _, a := range notNumbers {
			calls = append(calls, fn+"("+a+")")
		}
		calls = append(calls, fn+"()", fn+"(1, 2)", fn+"(1, 2, 3)")
		addAll(purePack("one/"+fn, pureInvalid, calls, 50))
	}

	// Two arguments: atan2, logb, pow over the pool in both places.
	grid := []string{"0", "-0.0", "1", "-1", "0.5", "-0.5", "2", "-2", "2.5", "10", "1e-300", "1e300", "5e-324", "inf", "-inf", "nan", "9007199254740993", "9223372036854775807", "(-9223372036854775807 - 1)", "1e19", "0.1", "3", "100", "0.7071067811865476", "2.414213562373095", "1024", "-1075", "0.3"}
	for _, fn := range []string{"atan2", "logb", "pow"} {
		calls := pureCalls("str("+fn, grid, grid)
		for i := range calls {
			calls[i] += ")"
		}
		more := pureCalls("typeof("+fn, []string{"0", "1", "2", "2.0", "-1", "10", "0.5", "nan"}, []string{"0", "1", "2", "2.0", "-1", "3", "62", "63", "64", "0.5", "nan", "inf"})
		for i := range more {
			more[i] += ")"
		}
		calls = append(calls, more...)
		for _, a := range notNumbers {
			calls = append(calls, fn+"("+a+", 1)", fn+"(1, "+a+")", fn+"("+a+", "+a+")")
		}
		calls = append(calls, fn+"()", fn+"(1)", fn+"(1, 2, 3)")
		addAll(purePack("two/"+fn, pureInvalid, calls, 60))
	}
	var powers []string
	for _, base := range []string{"2", "3", "10", "-2", "7", "0", "1", "-1", "2.0", "10.0", "0.5"} {
		for _, e := range []string{"0", "1", "2", "3", "10", "20", "31", "32", "52", "53", "62", "63", "64", "100", "-1", "-2", "-10", "0.5", "-0.5", "1.5", "300", "308", "309", "400", "1e10", "inf", "-inf", "nan"} {
			powers = append(powers, fmt.Sprintf("pow(%s, %s)", base, e))
		}
	}
	addAll(purePack("two/pow_ints", pureInvalid, powers, 60))

	// round: one argument, then a number of places.
	var rounds []string
	for _, a := range pureMathPool {
		rounds = append(rounds, "round("+a+")", "typeof(round("+a+"))")
	}
	for _, a := range []string{"0", "1", "2.5", "-2.5", "0.5", "1.005", "2.675", "1234.5678", "-1234.5678", "1e300", "1e-300", "9007199254740993", "inf", "-inf", "nan", "0.49999999999999994", "4503599627370497.5", "123456789.987654321"} {
		for _, p := range []string{"0", "1", "2", "3", "5", "10", "15", "16", "20", "22", "100", "300", "308", "309", "400", "-1", "2.9", "-0.5", "1e19", "(-9223372036854775807 - 1)"} {
			rounds = append(rounds, fmt.Sprintf("str(round(%s, %s))", a, p))
		}
	}
	for _, a := range notNumbers {
		rounds = append(rounds, "round("+a+")", "round("+a+", 2)", "round(1.5, "+a+")")
	}
	rounds = append(rounds, "round()", "round(1, 2, 3)", "round(1, 2, 3, 4)")
	addAll(purePack("round", pureInvalid, rounds, 60))

	// The operator that raises to a power and pow() are the same function in the Go interpreter.
	add("pow/operator_and_builtin", pureInvalid+`print(str(2 ** 0.5), str(pow(2, 0.5)), str(10 ** -2), str(pow(10, -2)), str(2.5 ** 3.5), str(pow(2.5, 3.5)), str(0.1 ** 0.3), str(pow(0.1, 0.3)))
print(str(7 ** 21), str(pow(7, 21)), 7 ** 21 == pow(7, 21), 2 ** 62, pow(2, 62), 2 ** 63, pow(2, 63))
`)

	// Errors that a builtin returns as a value, and what a script can do with one.
	add("errors/as_values", pureInvalid+`x = sin("a")
y = atan2(1, null)
z = round("q", 2)
w = pow([], 2)
print(x)
print(y)
print(z)
print(w)
print(typeof(x), len(str(y)), [z, w])
`)
	add("errors/arithmetic_on_a_value", pureInvalid+"x = exp(\"a\")\ntry:\n    print(x * 2)\ncatch (e):\n    print(e)\nend\n")
	add("errors/caught_as_text", pureInvalid+"x = cos([1])\ntry:\n    if (x) then:\n    end\ncatch (e):\n    print(e)\nend\n")
	add("errors/domain", pureInvalid+tryPrint("sqrt(-1)")+tryPrint("log(0)")+tryPrint("log(-0.0)")+tryPrint("log10(-5)")+tryPrint("log2(0)")+tryPrint("asin(2)")+tryPrint("acos(-1.0000000000000002)")+
		tryPrint("logb(0, 2)")+tryPrint("logb(2, 1)")+tryPrint("logb(2, 0)")+tryPrint("logb(-2, 3)")+tryPrint("logb(2, -3)")+tryPrint("round(1, -1)")+tryPrint("sqrt(nan)")+tryPrint("log(nan)")+tryPrint("asin(nan)")+tryPrint("logb(nan, 2)")+tryPrint("logb(2, nan)"))

	// Memory and operations: a float result is eight bytes, an integer result as well, an error value its text.
	add("limits/float_results_cost_eight_bytes", pureInvalid+"i = 0\nwhile (i < 5000):\n    a = sin(i)\n    b = floor(2.5)\n    c = pow(2, 3)\n    i += 1\nend\nprint(i, a, b, c)\n")
	add("limits/error_values_cost_their_text", pureInvalid+"i = 0\nwhile (i < 3000):\n    t = sin(\"a\")\n    i += 1\nend\nprint(i, t)\n")
	add("limits/error_value_is_cut_to_1024", pureInvalid+"x = exp([\"a\" * 5000])\nprint(len(str(x)))\n")
	add("limits/error_value_with_a_long_type_name_is_not_cut", pureInvalid+"x = exp({})\nprint(x)\n")
	add("limits/function_values_in_a_list", pureInvalid+"fs = [sin, cos, tan]\nprint(typeof(fs[0]), len(fs), fs)\nprint(str(fs[1](0)))\n")
	add("limits/builtin_as_a_value", pureInvalid+"g = sqrt\nprint(typeof(g), str(g(2)), g)\n")
	// A sum of the first terms of a series: the results of sin and cos at many arguments, to the last bit.
	add("series/sums", pureInvalid+`s = 0.0
c = 0.0
i = 1
while (i <= 400):
    s = s + sin(i * 0.37) * exp(-i / 300) + atan(i / 7.0)
    c = c + cos(i * 1.3) / (1 + tanh(i / 50.0)) + log(i) * sinh(i / 400.0) + cbrt(i)
    i += 1
end
print(str(s), str(c))
`)
	return cs
}

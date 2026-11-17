package main

import "fmt"

func init() {
	libRegistry = append(libRegistry, handGroup("lib-conv", convCases()))
}

// pureNumericStrings are texts that may or may not be a number: the forms an integer and a float can take in the
// Go library, the ones it refuses, and the edges of 64 bits.
var pureNumericStrings = []string{
	"0", "1", "-1", "+1", "42", "-42", "+42", "007", "-007", "0000000000000000000000000001", "9223372036854775807", "9223372036854775808", "-9223372036854775808", "-9223372036854775809",
	"18446744073709551615", "18446744073709551616", "123456789012345678901234567890", "-123456789012345678901234567890", "99999999999999999999999999999999999999999999",
	"1.5", "-1.5", "+1.5", "0.5", ".5", "-.5", "5.", "-5.", "0.", ".", "-", "+", "", " ", " 1", "1 ", "1 2", "\t1", "1\n", "1.5.5", "1.5e", "1e", "e5", "E5", "1e5", "1E5", "1e+5", "1e-5", "1e+", "1e-", "1e5.5", "1.e5", ".e5",
	"1e308", "1.7976931348623157e308", "1.7976931348623159e308", "1e309", "-1e309", "1e999", "1e-324", "4.9e-324", "2.4703282292062327e-324", "2.4703282292062328e-324", "1e-325", "1e-999", "0e999999", "0e-999999",
	"2.2250738585072011e-308", "2.2250738585072012e-308", "4503599627370496.5", "9007199254740993", "9007199254740993.0", "9223372036854775807.5", "9223372036854775808.0", "9.223372036854775807e18", "9.3e18", "-9.3e18", "1e19", "-1e19",
	"1_000", "1_000.5", "1_0_0", "1__0", "_1", "1_", "1_.5", "1._5", "1.5_", "1e1_0", "1e_1", "1e1_", "-_1", "+_1", "0_1", "1_2.3_4e1_0",
	"0x10", "0X10", "0x", "0xg", "-0x10", "+0x10", "0b101", "0o17", "017", "0x1p-2", "0x1P+2", "0x1.8p1", "0x.8p1", "0x1p", "0x1.8", "0x1p5000", "-0x1p5000", "0x1p-5000", "0x_1p1", "0x_1_2.3_4_5p+1_2", "0x1_0p1", "0x1p1_0", "0x0.0000000000001p-1022", "0xFFFFFFFFFFFFFFFFFFFFp0",
	"inf", "Inf", "INF", "+inf", "-inf", "-Inf", "infinity", "Infinity", "INFINITY", "-infinity", "+Infinity", "infinit", "infinityx", "infx", "in", "i", "nan", "NaN", "NAN", "+nan", "-nan", "nanx", "na", "n", "nan nan",
	"１２３", "١٢٣", "1٢", "1,000", "1,5", "$5", "5%", "5px", "0.1", "0.2", "0.3", "0.1e1", "0.30000000000000004", "1.0000000000000002", "0.9999999999999999", "1.00000000000000011102230246251565404236316680908203125", "1.00000000000000011102230246251565404236316680908203126",
	"3.141592653589793", "2.718281828459045", "123456.789e3", "-0", "-0.0", "+0", "0e0", "0.0e0", "00", "00.00", "0.00000000000000000000000000000000000001", "100000000000000000000", "1e20", "123e-2", "1.23e+2",
	"true", "false", "null", "abc", "NaNa", "1a", "a1", "--1", "++1", "+-1", "-+1", "1-", "1+", "1e5e5", "1ee5", "0x1p1p1", "\u00a01", "1\u00a0", "\u20291", "1\ufeff",
}

func convCases() []handCase {
	var cs []handCase
	add := func(name, src string) { cs = append(cs, handCase{name, nil, pureUni(src)}) }
	addAll := func(more []handCase) { cs = append(cs, more...) }
	notNumbers := []string{"null", "[]", "[1]", "{}", "{a: 1}", "f", "len"}

	// int
	var ints []string
	for _, a := range append(append([]string{}, pureNumPool...), "true", "false", "nan", "inf", "-inf", "1e18", "9.2e18", "-9.2e18", "2.9", "-2.9", "0.9999999999999999") {
		ints = append(ints, "int("+a+")", "typeof(int("+a+"))")
	}
	for _, a := range append(append([]string{}, notNumbers...), "lead", "mixed") {
		ints = append(ints, "int("+a+")")
	}
	for _, s := range pureNumericStrings {
		ints = append(ints, "str(int("+pureLit(s)+"))")
	}
	ints = append(ints, "int()", "int(1, 2)", "int(1, 2, 3)", `int("1", "2")`)
	addAll(purePack("int", pureInvalid, ints, 60))

	// float: a value and a number of places.
	var floats []string
	values := []string{"1.5", "2.675", "1.005", "-1.005", "0.0", "-0.0", "5", "-5", "0", "123456.789", "0.1", "0.25", "0.35", "2.5", "-2.5", "1e300", "-1e300", "1e-300", "5e-324", "nan", "inf", "-inf",
		"9007199254740993", "9223372036854775807", "(-9223372036854775807 - 1)", "1.7976931348623157e308", "4503599627370497.5", "0.49999999999999994", "0.5", "1234567.891", "-0.4", "-0.04", "-0.004"}
	places := []string{"0", "1", "2", "3", "5", "10", "15", "16", "17", "20", "22", "23", "100", "300", "308", "309", "400"}
	for _, v := range values {
		for _, p := range places {
			floats = append(floats, fmt.Sprintf("str(float(%s, %s))", v, p))
		}
	}
	for _, s := range pureNumericStrings {
		floats = append(floats, fmt.Sprintf("str(float(%s, 3))", pureLit(s)), fmt.Sprintf("str(float(%s, 0))", pureLit(s)))
	}
	for _, v := range []string{"1.5", "5", `"2.5"`, "nan", "-0.0", `"abc"`} {
		for _, p := range []string{"-1", "-100", "(-9223372036854775807 - 1)", "9223372036854775807"} {
			floats = append(floats, fmt.Sprintf("float(%s, %s)", v, p))
		}
	}
	// The second argument has to be an integer: any other kind is a failed type assertion.
	for _, v := range []string{"1.5", "5", `"2.5"`} {
		for _, p := range []string{"2.5", "2.0", `"2"`, `""`, "null", "true", "[1]", "[]", "{}", "{a: 1}", "f", "len", "nan", "lead", "1e300"} {
			floats = append(floats, fmt.Sprintf("float(%s, %s)", v, p))
		}
	}
	for _, v := range append(append([]string{}, notNumbers...), "true", "false", "lead", "mixed") {
		floats = append(floats, "float("+v+", 2)", "float("+v+", null)", "float("+v+", -1)")
	}
	floats = append(floats, "float()", "float(1)", "float(1.5, 2, 3)", `float("1.5")`, "float(1.5, 2, 3, 4)")
	addAll(purePack("float", pureInvalid, floats, 60))
	add("float/error_value_as_the_places", pureInvalid+tryPrint("float(1.5, path_join())")+tryPrint("float(1.5, [path_join()][0])"))
	add("float/negative_zero_becomes_zero", pureInvalid+"print(str(float(-0.0, 2)), str(float(-0.004, 2)), str(float(-0.0, 0)), str(float(0.0, 1)), str(float(-1e-300, 5)), str(float(\"-0\", 3)))\n")
	add("float/error_value_for_negative_digits", pureInvalid+"x = float(1.5, -1)\nprint(x)\nprint(typeof(x), len(str(x)))\n")
	add("float/results_cost_eight_bytes", pureInvalid+"i = 0\nwhile (i < 5000):\n    a = float(i, 2)\n    b = int(\"12\")\n    i += 1\nend\nprint(i, a, b)\n")

	// char and rune
	var chars []string
	for _, a := range []string{"0", "1", "9", "10", "31", "32", "65", "97", "126", "127", "128", "160", "173", "191", "192", "255", "256", "383", "1000", "2047", "2048", "4095", "8232", "8233", "55295", "55296", "56319", "57343", "57344", "65279", "65533", "65534", "65535", "65536", "128512",
		"917999", "1114111", "1114112", "1114113", "2097151", "2097152", "-1", "-2", "-65", "-1114112", "2147483647", "2147483648", "2147483649", "4294967295", "4294967296", "4294967361", "4294967297", "-4294967231", "-2147483648", "-2147483649", "9223372036854775807", "(-9223372036854775807 - 1)",
		"1e3", "65.0", "65.5", `"65"`, `"a"`, `""`, "null", "true", "[65]", "{}", "f", "len", "nan", "lead"} {
		chars = append(chars, "char("+a+")", "len(char("+a+"))")
	}
	chars = append(chars, "char()", "char(1, 2)", "char(1, 2, 3)")
	addAll(purePack("char", pureInvalid, chars, 60))
	var runes []string
	for _, a := range append(append([]string{}, pureLits("a", "A", "é", "€", "日", "😀", "", "ab", "éa", "日本", "\n", "\u0000", "\u00a0", "\u2028", "\U0010ffff", "\U0001f600x", " ", "\\")...), "lead", "cont", "cut3", "cut4", "sur", "big", "mixed", "rep", `lead + lead`, `"a" + lead`, `lead + "a"`,
		`"é"[0]`, `"é"[1]`, `substr("€", 0, 1)`, `substr("€", 1, 3)`, `substr("😀", 2, 4)`, `substr("😀", 3, 4)`, `substr("日", 0, 3)`, "1", "65", "2.5", "null", "true", "[]", "[\"a\"]", "{}", "f", "len", "nan") {
		runes = append(runes, "rune("+a+")")
	}
	runes = append(runes, "rune()", `rune("a", "b")`, `rune("a", "b", "c")`)
	addAll(purePack("rune", pureInvalid, runes, 60))
	add("rune/inverse_of_char", pureInvalid+`i = 0
while (i < 1200):
    c = char(i * 931)
    if (rune(c) != i * 931 and i * 931 < 1114112 and (i * 931 < 55296 or i * 931 > 57343)) then:
        print("differs", i * 931)
    end
    i += 1
end
print(i, rune(char(55296)), rune(char(-5)), rune(char(1114112)), rune(char(4294967361)))
`)

	// println prints what print prints.
	add("println/values", pureInvalid+`println()
println("a", 1, 2.5, true, null, [1, "b", [2.5]], {k: "v", a: 1})
println(0.1, 1.5e300, 1e-7, 123456789.5, 1234567.5, 100000.0, 1e6, -0.0, 0.30000000000000004, 2.0, 1e21, nan, inf, -inf)
println(9223372036854775807, (-9223372036854775807 - 1), -1, 0)
println("é", lead, cont, mixed)
println(f, len, [f, len], {a: f})
x = path_join()
println(x, [x], {a: x})
println("a" * 3, [1] * 3, ("ab" + "cd") * 2)
print("a")
println("b")
print()
println()
println("end")
`)
	add("println/same_as_print_on_nested_depth_over", pureInvalid+"l = [1]\ni = 0\nwhile (i < 257):\n    l = [l]\n    i += 1\nend\nprintln(l)\n")
	add("println/same_as_print_on_nested_depth", pureInvalid+"l = [1]\ni = 0\nwhile (i < 200):\n    l = [l]\n    i += 1\nend\nprintln(len(str(l)))\nprintln(l)\n")
	add("println/self_reference", pureInvalid+"l = [1]\nl[0] = l\nprintln(l)\n")
	add("println/size_bound_at_the_limit", pureInvalid+"x = \"a\" * 524283\nprintln(len(x))\nprintln([x])\n")
	add("println/size_bound_over", pureInvalid+"x = \"a\" * 524284\nprintln([x, x])\n")
	add("println/many_arguments", pureInvalid+"println(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, \"a\" * 100, [1] * 10)\n")
	add("println/output_over_the_cap", pureInvalid+"x = \"abcdefgh\" * 1000\ni = 0\nwhile (i < 200):\n    println(x)\n    i += 1\nend\nprintln(i)\n")

	// Every code point through lower, upper and trim, to the end of the planes that have letters or spaces.
	for start := 0; start < 0x20000; start += 2400 {
		end := min(start+2400, 0x20000)
		add(fmt.Sprintf("unicode/lower_upper_trim_%05x", start), pureInvalid+fmt.Sprintf(`i = %d
while (i < %d):
    c = char(i)
    u = upper(c)
    l = lower(c)
    if (u != c or l != c) then:
        print(i, rune(u), rune(l))
    end
    if (trim(c + "x") != c + "x") then:
        print("space", i)
    end
    i += 1
end
`, start, end))
	}
	add("unicode/other_planes_sampled", pureInvalid+`i = 131072
while (i < 1114112):
    c = char(i)
    if (upper(c) != c or lower(c) != c or trim(c + "x") != c + "x") then:
        print(i)
    end
    i += 997
end
print(i)
`)
	return cs
}

package main

import (
	"fmt"
	"regexp"
	"strconv"
	"strings"
	"unicode/utf8"
)

func init() {
	libRegistry = append(libRegistry, handGroup("lib-json", jsonCases()))
}

// The scripts of json_parse and json_stringify: the documents the reader takes and the text of each error it
// gives, the numbers it turns into integers, what the writer does to strings, floats and keys, and what both
// cost.

// ustr is a script expression for the bytes b: runs of printable ASCII in quotes, every other byte through
// char, and the bytes of a UTF-8 sequence that no script can type, by slicing a character that has them. Only
// the bytes c0, c1 and f5 to ff, which no character has, are not supported.
func ustr(b []byte) string {
	if len(b) == 0 {
		return `""`
	}
	var parts []string
	var run []byte
	flush := func() {
		if len(run) > 0 {
			parts = append(parts, `"`+string(run)+`"`)
			run = nil
		}
	}
	for i := 0; i < len(b); {
		r, w := utf8.DecodeRune(b[i:])
		if r >= 0x80 && (r != utf8.RuneError || w > 1) {
			run = append(run, b[i:i+w]...)
			i += w
			continue
		}
		c := b[i]
		i++
		switch {
		case c == '"' || c == '\\':
			run = append(run, '\\', c)
		case c >= 0x20 && c < 0x7f:
			run = append(run, c)
		case c == '\n':
			run = append(run, '\\', 'n')
		case c == '\r':
			run = append(run, '\\', 'r')
		case c == '\t':
			run = append(run, '\\', 't')
		case c < 0x80:
			run = append(run, c)
		case c <= 0xbf:
			flush()
			parts = append(parts, fmt.Sprintf("slice(%s, 2, 3)", chr(0x800+int(c-0x80))))
		case c >= 0xc2 && c <= 0xdf:
			flush()
			parts = append(parts, fmt.Sprintf("slice(%s, 0, 1)", chr(int(c&0x1f)<<6)))
		case c >= 0xe0 && c <= 0xef:
			cp := int(c&0xf) << 12
			if c == 0xe0 {
				cp = 0x800
			}
			flush()
			parts = append(parts, fmt.Sprintf("slice(%s, 0, 1)", chr(cp)))
		case c >= 0xf0 && c <= 0xf4:
			cp := int(c&7) << 18
			if c == 0xf0 {
				cp = 0x10000
			}
			flush()
			parts = append(parts, fmt.Sprintf("slice(%s, 0, 1)", chr(cp)))
		default:
			panic(fmt.Sprintf("ustr: byte %#x has no script spelling", c))
		}
	}
	flush()
	return strings.Join(parts, " + ")
}

// chr is a script literal of the character with code point cp.
func chr(cp int) string { return ustr([]byte(string(rune(cp)))) }

var charCall = regexp.MustCompile(`char\((\d+)\)`)

// noChar writes each call of char with a number as the literal of that character, so that a case does not need
// the builtin.
func noChar(src string) string {
	return charCall.ReplaceAllStringFunc(src, func(m string) string {
		n, _ := strconv.Atoi(charCall.FindStringSubmatch(m)[1])
		return chr(n)
	})
}

func ustrs(docs []string) string {
	var b strings.Builder
	b.WriteString("[")
	for i, d := range docs {
		if i > 0 {
			b.WriteString(",\n  ")
		}
		b.WriteString(ustr([]byte(d)))
	}
	b.WriteString("]")
	return b.String()
}

// parseEach is a script that reads each document and prints what it came to, or the error.
func parseEach(docs []string) string {
	return "docs = " + ustrs(docs) + "\nfor (d in docs):\n    try:\n        r = json_parse(d)\n        print(str(r), typeof(r), json_stringify(r))\n    catch (e):\n        print(\"ERR\", e)\n    end\nend\n"
}

func jsonCases() []handCase {
	var cs []handCase
	add := func(name, src string) { cs = append(cs, handCase{name, nil, noChar(src)}) }

	// Numbers: an integer when the float is one, ParseFloat's rules for what a number may look like.
	nums := []string{"0", "-0", "0.0", "-0.0", "1", "-1", "1.0", "1.5", "-1.5", "0.5", "0.1", "0.2", "0.30000000000000004", "1e0", "1e1", "1e2", "1E2", "1e+2", "1e-2", "1.5e3", "1.5E-3", "2.5e+10", "12345678901234567890", "123456789012345678901234567890", "9007199254740991", "9007199254740992", "9007199254740993", "9007199254740994", "-9007199254740993", "9223372036854774784", "9223372036854775295", "9223372036854775296", "9223372036854775807", "9223372036854775808", "9223372036854775809", "18446744073709551615", "18446744073709551616", "-9223372036854775807", "-9223372036854775808", "-9223372036854775809", "-9223372036854777856", "-9223372036854777857", "1e18", "1e19", "1e20", "1e21", "1e22", "1e100", "1e308", "1.7976931348623157e308", "1.7976931348623158e308", "1.7976931348623159e308", "1e309", "-1e309", "1e-5", "1e-6", "1e-7", "1e-300", "1e-323", "5e-324", "2e-324", "1e-324", "1e-400", "4.9e-324", "00", "01", "007", "-01", "1.", "1.e3", "-.5", "-.e", "1e5.", "1.2.3", "1..2", "1e", "1e+", "1e-", "1ee2", "1e2e3", "1e2.5", "--1", "-+1", "+-1", "1-", "1+", "1-2", "1+2", "1e2-3", "-", "-e", "0e", "0x1", "0b1", "1_000", "1,5", "NaN", "Inf", "inf", "-inf", "Infinity", "-Infinity", "+1", "+0", ".5", ".", "e5", "E5", "1 ", " 1", "\t1\n", "1\x01"}
	for i := 0; i < len(nums); i += 40 {
		end := i + 40
		if end > len(nums) {
			end = len(nums)
		}
		add(fmt.Sprintf("parse/numbers_%d", i/40), parseEach(nums[i:end]))
	}
	var inArrays []string
	for _, n := range nums {
		inArrays = append(inArrays, "["+n+"]", "{\"k\":"+n+"}", "["+n+",1]", "[1,"+n+"]", "["+n+" ,1]", "["+n+" ]", "{\"k\":"+n+",\"j\":1}")
	}
	for i := 0; i < len(inArrays); i += 60 {
		end := i + 60
		if end > len(inArrays) {
			end = len(inArrays)
		}
		add(fmt.Sprintf("parse/numbers_in_containers_%d", i/60), parseEach(inArrays[i:end]))
	}

	// Literals, values of each kind, white space, and where a document may end.
	basic := []string{"true", "false", "null", "\"s\"", "\"\"", "[]", "{}", "[1,2,3]", "[1, 2, 3]", "{\"a\":1}", "{\"a\": 1, \"b\": [1, 2, {\"c\": null}]}", "[[[]]]", "[{}]", "{\"a\":{}}", "[[],[],[]]", "[null,true,false]", "[\"a\",\"b\"]", "{\"\":1}", "{\"a\":\"\"}", " true", "true ", " true ", "\ntrue\r\n", "\t[\t1\t,\t2\t]\t", "{ \"a\" : 1 , \"b\" : 2 }", "{\n\"a\"\n:\n1\n}", "[\n]", "{\n}", "[ ]", "{ }", "[1,\n2]",
		"tru", "tr", "t", "truee", "truex", "true1", "true,", "false1", "falsey", "fals", "f", "nul", "nu", "n", "nulll", "null1", "nullx", "True", "TRUE", "False", "Null", "NULL", "undefined", "none", "[tru]", "[nul]", "[fals]", "[truex]", "[true false]", "[true,false]", "[truefalse]", "{\"a\":tru}", "{\"a\":truex}", "{\"a\":nul}",
		"", " ", "\n", "\t", "  \n ", "\x00", "\x00 1", "1\x00", "1 \x00 garbage", "[1\x00]", "{}\x00{}", "[\x00]", "\"a\x00\"", "{\"a\x00\":1}", "true\x00false", "tr\x00ue"}
	for i := 0; i < len(basic); i += 50 {
		end := i + 50
		if end > len(basic) {
			end = len(basic)
		}
		add(fmt.Sprintf("parse/basic_%d", i/50), parseEach(basic[i:end]))
	}

	// Syntax errors, each in the part of a document where a different check catches it.
	bad := []string{"[", "{", "[1", "[1,", "[1,2", "[1,2,", "[1,]", "[,]", "[,1]", "[1 2]", "[1;2]", "[1:2]", "[1}", "[}", "[1]]", "[1]]]", "[[1]", "[[1]]]", "[{]", "[{}", "[{\"a\"]", "[{\"a\":]", "[{\"a\":1]", "[{\"a\":1}", "[{\"a\":1},]",
		"{", "{,", "{}", "{}}", "{{}}", "{[}", "{\"a\"", "{\"a\":", "{\"a\":1", "{\"a\":1,", "{\"a\":1,}", "{\"a\":1,,\"b\":2}", "{\"a\" 1}", "{\"a\"::1}", "{\"a\":1 \"b\":2}", "{\"a\":1;\"b\":2}", "{\"a\":}", "{:1}", "{\"a\":1]", "{\"a\":[1}", "{\"a\":{\"b\":1}", "{'a':1}", "{a:1}", "{1:2}", "{-1:2}", "{1.5:2}", "{true:1}", "{false:1}", "{null:1}", "{null:null}", "{nul:1}", "{[]:1}", "{[1]:2}", "{{}:1}", "{{\"a\":1}:2}", "{\"a\":1,null:2}", "{\"a\":1,\"a\":2}", "{ \"a\" : 1 , null : 2 }",
		"\"", "\"abc", "\"abc\\", "\"abc\\\"", "\"\\", "\"\\u", "\"\\u1", "\"\\u12", "\"\\u123", "\"\\u1234", "\"\\u1234\"", "\"\\uZZZZ\"", "\"\\u12G4\"", "\"\\u123g\"", "\"\\ug234\"", "\"\\U1234\"", "\"\\x41\"", "\"\\a\"", "\"\\0\"", "\"\\'\"", "\"\\ \"", "\"\\\n\"", "\"\\/\"", "\"\\\\\"", "\"\\\"\"", "\"\\b\\f\\n\\r\\t\"", "\"\\u0041\\u0042\"", "\"\\u00e9\"", "\"\\u00E9\"", "\"\\u20ac\"", "\"\\u0000\"", "\"\\ud800\"", "\"\\udc00\"", "\"\\ud800\\udc00\"", "\"\\ud83d\\ude00\"", "\"\\ud83d\\ude00x\"", "\"x\\ud83d\\ude00\"", "\"\\ud83d\\u0041\"", "\"\\ud83d\\\\u0041\"", "\"\\ude00\\ud83d\"", "\"\\ud83d\\ud83d\"", "\"\\ud83d\\ude0\"", "\"\\ud83d\\ude00\\ud83d\\ude00\"", "\"\\uD83D\\uDE00\"", "\"\\ud83d\\uDE00\"", "\"\\udbff\\udfff\"", "\"\\udbff\\ue000\"", "\"\\ud7ff\\udc00\"",
		"'a'", "'", "a", "abc", "@", "#", "$", "%", "&", "*", "(", ")", ",", ":", ";", "<", ">", "=", "?", "\\", "/", "|", "~", "`", "_", "\u00e9", "\u65e5", "\"\u00e9\"", "\"\u65e5\u672c\"", "\u00a0", "\u00a01", "1\u00a0", "\ufeff1", "\ufeff[]", "[\u00e9]", "{\u00e9:1}", "[1]\u00e9", "[1]\u65e5", "[1] x", "[1] [2]", "{} {}", "1 1", "\"a\" \"b\"", "[] x", "{} x", "true x", "null null", "[1],", "[1];", "[1]:", "[1]}"}
	for i := 0; i < len(bad); i += 50 {
		end := i + 50
		if end > len(bad) {
			end = len(bad)
		}
		add(fmt.Sprintf("parse/syntax_%d", i/50), parseEach(bad[i:end]))
	}

	// Strings: control characters and raw bytes inside a string are kept; the escapes are replaced.
	var raw []string
	for c := 0; c < 0x80; c++ {
		raw = append(raw, "\""+string([]byte{byte(c)})+"\"")
	}
	for i := 0; i < len(raw); i += 32 {
		add(fmt.Sprintf("parse/ascii_in_string_%d", i/32), parseEach(raw[i:i+32]))
	}
	var esc []string
	for c := 0; c < 0x100; c += 5 {
		esc = append(esc, fmt.Sprintf("\"\\u%04x\"", c), fmt.Sprintf("\"a\\u%04Xb\"", c+0x100))
	}
	for i := 0; i < len(esc); i += 40 {
		end := i + 40
		if end > len(esc) {
			end = len(esc)
		}
		add(fmt.Sprintf("parse/unicode_escapes_%d", i/40), parseEach(esc[i:end]))
	}
	var sur []string
	for _, hi := range []string{"d800", "d83d", "dbff", "DBFF", "dc00", "dfff", "e000", "d7ff", "0041"} {
		for _, lo := range []string{"dc00", "de00", "dfff", "DFFF", "dbff", "d800", "e000", "0041", "zzzz", "dc0", "dc"} {
			sur = append(sur, fmt.Sprintf("\"\\u%s\\u%s\"", hi, lo), fmt.Sprintf("\"\\u%s\\u%sx\"", hi, lo), fmt.Sprintf("\"\\u%s\\\\u%s\"", hi, lo))
		}
	}
	for i := 0; i < len(sur); i += 40 {
		end := i + 40
		if end > len(sur) {
			end = len(sur)
		}
		add(fmt.Sprintf("parse/surrogates_%d", i/40), parseEach(sur[i:end]))
	}
	// Raw bytes that are not UTF-8 stay as they are.
	invalid := [][]byte{{0x80}, {0xbf}, {0xc3}, {0xe6, 0x97}, {0xe6, 0x97, 0xa5}, {0xf0, 0x9f, 0x98}, {0xf0, 0x9f, 0x98, 0x80}, {0xe0, 0x80, 0x80}, {0xed, 0xa0, 0x80}, {0xf4, 0x90, 0x80, 0x80}, {0xc3, 0xa9, 0x80}, {0x80, 0xc3, 0xa9}, {0xe2, 0x80, 0xa8}, {0xe2, 0x80, 0xa9}, {0xf0, 0x80, 0x80, 0x80}, {'a', 0x80, 'b'}, {0xbf, 0xbf, 0xbf, 0xbf, 0xbf}, {0xc2}, {0xdf, 0xbf}, {0xef, 0xbf, 0xbf}, {0xef, 0xbf, 0xbe}}
	var b strings.Builder
	for _, in := range invalid {
		doc := append(append([]byte("\""), in...), '"')
		keydoc := append(append([]byte("{\""), in...), []byte("\":1}")...)
		b.WriteString(fmt.Sprintf("d = %s\n", ustr(doc)))
		b.WriteString("r = json_parse(d)\nprint(len(r), r == slice(d, 1, len(d) - 1), json_stringify(r))\n")
		b.WriteString(fmt.Sprintf("k = json_parse(%s)\nprint(len(k), json_stringify(k))\n", ustr(keydoc)))
		b.WriteString(fmt.Sprintf("a = json_parse(%s)\nprint(len(a), json_stringify(a))\n", ustr(append(append([]byte("[\"x"), in...), []byte("y\",1]")...))))
	}
	add("parse/invalid_utf8_kept", b.String())

	// Whole documents: duplicate keys, order of keys, nesting, shapes.
	add("parse/duplicate_keys", parseEach([]string{"{\"a\":1,\"a\":2}", "{\"a\":1,\"b\":2,\"a\":3}", "{\"a\":{\"x\":1},\"a\":{\"y\":2}}", "{\"a\":1,\"a\":null,\"a\":[1]}", "{\"a\":[1],\"a\":1}", "{\"\":1,\"\":2}", "{\"a\":1,\"\\u0061\":2}", "{\"a\\n\":1,\"a\\n\":2}", "{\"k\":{\"k\":{\"k\":1,\"k\":2},\"k\":3},\"k\":4}", "{\"1\":1,\"01\":2,\"1\":3}", "{null:1,\"\":2}", "{\"\":1,null:2}", "{\"a\":1,\"A\":2,\"a\":3}"}))
	add("parse/keys_with_escapes_and_unicode", parseEach([]string{"{\"a\\\"b\":1}", "{\"\\u00e9\":1,\"\u00e9\":2}", "{\"a\\\\b\":1}", "{\"a/b\":1,\"a\\/b\":2}", "{\"\\n\":1,\"\\t\":2}", "{\"\u65e5\u672c\":{\"\u8a9e\":[1,2]}}", "{\"a b\":1,\"a  b\":2}", "{\" \":1}", "{\"\\u0000\":1}", "{\"z\":1,\"y\":2,\"x\":3,\"w\":4,\"v\":5}", "{\"b\":2,\"a\":1,\"B\":3,\"A\":4,\"_\":5,\"1\":6,\"\":7}"}))
	var many strings.Builder
	many.WriteString("{")
	for i := 0; i < 120; i++ {
		if i > 0 {
			many.WriteString(",")
		}
		fmt.Fprintf(&many, "\"key%d\":%d", i%90, i)
	}
	many.WriteString("}")
	add("parse/many_keys_with_repeats", "r = json_parse("+ustr([]byte(many.String()))+")\nprint(len(r), r.key0, r.key29, r.key30, r.key89)\nprint(json_stringify(r) == json_stringify(json_parse(json_stringify(r))))\nn = 0\nfor (k in r):\n    n += 1\nend\nprint(n)\n")
	add("parse/shapes", parseEach([]string{"[[1,2],[3,[4,[5,[6]]]]]", "[{\"a\":[{\"b\":[{\"c\":1}]}]}]", "{\"a\":{\"b\":{\"c\":{\"d\":{\"e\":{}}}}}}", "[1,\"a\",null,true,false,1.5,[],{}]", "[[],[[]],[[],[]]]", "[{},{\"a\":{}},{\"a\":{},\"b\":[]}]", "{\"a\":[],\"b\":{},\"c\":\"\",\"d\":0,\"e\":null,\"f\":false}", "[\"\",\"\",\"\"]", "[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]", "[1.5,2.5,3.5,-1.5,1e3,1e-3]"}))
	for _, n := range []int{1, 2, 3, 16, 17, 18, 63, 64, 65, 100, 101, 143, 144, 300} {
		var arr strings.Builder
		arr.WriteString("[")
		for i := 0; i < n; i++ {
			if i > 0 {
				arr.WriteString(",")
			}
			fmt.Fprintf(&arr, "%d", i)
		}
		arr.WriteString("]")
		add(fmt.Sprintf("parse/array_of_%d", n), "r = json_parse("+ustr([]byte(arr.String()))+")\nprint(len(r), r[0], r[len(r) - 1])\nappend(r, 0)\nprint(len(r))\n")
	}

	// Nesting: 10,000 levels are read, one more is refused; the value is walked only where the machine allows.
	for _, n := range []int{1, 2, 255, 256, 257, 1000, 5000, 9999, 10000, 10001, 10002, 20000} {
		add(fmt.Sprintf("parse/nested_arrays_%d", n), fmt.Sprintf("s = \"[\" * %d + \"1\" + \"]\" * %d\ntry:\n    r = json_parse(s)\n    print(typeof(r), len(r))\ncatch (e):\n    print(e)\nend\n", n, n))
		add(fmt.Sprintf("parse/nested_objects_%d", n), fmt.Sprintf("s = \"{\\\"a\\\":\" * %d + \"1\" + \"}\" * %d\ntry:\n    r = json_parse(s)\n    print(typeof(r), len(r))\ncatch (e):\n    print(e)\nend\n", n, n))
		add(fmt.Sprintf("parse/nested_mixed_%d", n), fmt.Sprintf("s = \"[{\\\"a\\\":\" * %d + \"1\" + \"}]\" * %d\ntry:\n    r = json_parse(s)\n    print(typeof(r), len(r))\ncatch (e):\n    print(e)\nend\n", n/2, n/2))
		add(fmt.Sprintf("parse/nested_unclosed_%d", n), fmt.Sprintf("s = \"[\" * %d\ntry:\n    r = json_parse(s)\n    print(typeof(r))\ncatch (e):\n    print(e)\nend\n", n))
	}
	add("parse/nested_then_walk", "s = \"[\" * 300 + \"]\" * 300\nr = json_parse(s)\nprint(typeof(r))\nprint(len(str(r)) > 0)\n")
	add("parse/nested_then_stringify", "s = \"[\" * 200 + \"]\" * 200\nr = json_parse(s)\nprint(len(json_stringify(r)))\ns = \"[\" * 257 + \"]\" * 257\nr = json_parse(s)\nprint(len(json_stringify(r)))\n")

	// Arguments.
	apool := []string{"1", "1.5", "true", "null", "[]", "[\"{}\"]", "{a: 1}", "f", "print", "set_new()", "stack_new()", "queue_new()", "\"1\"", "\"\"", "\"[]\"", "\"{\\\"a\\\":1}\""}
	var args strings.Builder
	args.WriteString("fun f():\nend\n")
	for _, a := range apool {
		args.WriteString(tryPrint(fmt.Sprintf("str(json_parse(%s))", a)))
		args.WriteString(tryPrint(fmt.Sprintf("json_stringify(%s)", a)))
		args.WriteString(tryPrint(fmt.Sprintf("json_parse(%s, %s)", a, a)))
		args.WriteString(tryPrint(fmt.Sprintf("json_stringify(%s, 1)", a)))
	}
	args.WriteString(tryPrint("json_parse()") + tryPrint("json_stringify()") + tryPrint("json_parse(1, 2, 3)") + tryPrint("json_stringify(1, 2, 3)"))
	add("args/kinds_and_counts", args.String())

	// The writer.
	add("stringify/scalars", "print(json_stringify(0), json_stringify(-0), json_stringify(1), json_stringify(-1), json_stringify(123456789), json_stringify(9223372036854775807), json_stringify(-9223372036854775807 - 1))\nprint(json_stringify(true), json_stringify(false), json_stringify(null), json_stringify(\"\"), json_stringify(\"a\"), json_stringify([]), json_stringify({}))\nprint(json_stringify([1, [2, [3, []]], {}]), json_stringify({a: {b: {c: []}}}))\n")
	var fl strings.Builder
	fl.WriteString("xs = [0.5, -0.5, 1.0, -1.0, 0.0, -0.0, 1.5, 100.0, 1234567.0, 12345678.9, 0.1, 0.2, 0.1 + 0.2, 1 / 3, 2 / 3, 3.14159, 2.718281828459045, 1e1, 1e5, 1e6, 1e7, 1e14, 1e15, 1e16, 1e17, 1e18, 1e19, 1e20, 1e21, 1e22, 1e23, 1e100, 1e300, 1.7976931348623157e308, 2.5e-5, 1e-3, 1e-4, 1e-5, 1e-6, 1e-7, 1e-8, 1.5e-7, 1.2345e-6, 1.2345e-7, 9.999999e-7, 9.9999999e-7, 0.000001, 0.0000012, 0.00000099, 5e-324, 2.2250738585072014e-308, 123456789.123456789, 4.35, 0.3, 0.7, 2.675, 1.005, 8.41, 2 ** 53 * 1.0, 2 ** 53 * 1.0 + 2, 2 ** 62 * 1.0, 2 ** 63 * 1.0, 2 ** 64 * 1.0, -(2 ** 63) * 1.0, 1e20 + 0.5, 999999999999999.9, 99999999999999990000.0, 100000000000000000000.0, 999999999999999900000.0, 1e21 - 1e5]\nfor (x in xs):\n    print(json_stringify(x), json_stringify([x]), json_stringify({k: x}))\nend\n")
	add("stringify/floats", fl.String())
	var allf strings.Builder
	allf.WriteString("xs = []\n")
	for e := -30; e <= 30; e += 3 {
		allf.WriteString(fmt.Sprintf("append(xs, 1.2345 * 10.0 ** %d, 9.5 * 10.0 ** %d, 1.0 * 10.0 ** %d)\n", e, e, e))
	}
	allf.WriteString("for (x in xs):\n    print(json_stringify(x), str(x))\nend\n")
	add("stringify/float_exponents", allf.String())
	add("stringify/nan_inf", "inf = 10 ** 400\nnan = inf - inf\nfor (v in [nan, inf, -inf, [1, nan], [inf], {a: nan}, {a: -inf}, [[[nan]]], {a: [1, {b: inf}]}, [1, [2, [nan, inf]]]]):\n    try:\n        print(json_stringify(v))\n    catch (e):\n        print(e)\n    end\nend\n")
	add("stringify/unsupported", "fun f():\nend\ng = fun (x):\n    return x\nend\nfor (v in [f, g, print, len, [f], [g], [print, 1], {a: f}, {a: [g]}, [1, [2, [f]]], _waf_ctx, [_waf_ctx], {a: _waf_ctx}, map, [map]]):\n    try:\n        print(json_stringify(v))\n    catch (e):\n        print(e)\n    end\nend\n")
	add("stringify/error_value_is_an_empty_object", "e = date_add(1, \"x\", 1)\nfor (v in [e, [e], {k: e}, [[e, 1]]]):\n    print(json_stringify(v))\nend\n")
	add("stringify/collections_are_empty_objects", "for (v in [set_new(), stack_new(), queue_new(), [set_new(), 1], {a: stack_new()}, [queue_new()], [[[set_new()]]]]):\n    print(json_stringify(v))\nend\ns = set_new()\nset_add(s, 1)\nprint(json_stringify(s), json_stringify([s, s]))\nk = stack_new()\nstack_push(k, 1)\nprint(json_stringify(k))\n")
	var ch strings.Builder
	ch.WriteString("cs = [")
	for c := 0; c < 128; c++ {
		if c > 0 {
			ch.WriteString(", ")
		}
		ch.WriteString(ustr([]byte{byte(c)}))
	}
	ch.WriteString("]\ni = 0\nfor (c in cs):\n    print(i, json_stringify(c), json_stringify([c, c]), json_stringify({\"k\" + c: 1}))\n    i += 1\nend\n")
	add("stringify/every_ascii_character", ch.String())
	for _, cp := range []int{127, 128, 159, 160, 8232, 8233, 8234, 8231, 65279, 65533, 65534, 65535, 55295, 57344, 128512, 1114111, 917760, 0xa9, 0xe9, 0x65e5} {
		add(fmt.Sprintf("stringify/unicode_%x", cp), fmt.Sprintf("c = char(%d)\nprint(len(c), json_stringify(c), json_stringify(\"a\" + c + \"b\"), json_stringify([c]), json_stringify(c + c))\n", cp))
	}
	var badText strings.Builder
	for _, in := range invalid {
		badText.WriteString(fmt.Sprintf("v = %s\nprint(len(v), json_stringify(v), json_stringify(\"a\" + v + \"b\"), json_stringify([v, v]), json_stringify({\"k\" + v: v}))\n", ustr(in)))
	}
	add("stringify/invalid_utf8", badText.String())
	add("stringify/html_and_separators", "for (s in [\"<\", \">\", \"&\", \"<script>alert(1)</script>\", \"a&b&c\", \"<>&<>&\", \"a\" + char(8232) + \"b\", char(8232) + char(8233), \"x\" + char(8232), char(8233) + \"x\", \"<\" + char(8232) + \">\", \"'\", \"\\\"\", \"\\\\\", \"/\", \"\\\\/\", \"\\\\\\\\\"]):\n    print(json_stringify(s))\nend\n")
	add("stringify/long_runs", "for (n in [1, 2, 7, 8, 9, 15, 16, 17, 31, 32, 33, 100]):\n    print(len(json_stringify(\"a\" * n)), len(json_stringify(\"<\" * n)), len(json_stringify(\"a\" * n + \"<\")), len(json_stringify(\"\u00e9\" * n)), len(json_stringify(\"a\" * n + char(1))), len(json_stringify(\"\\n\" * n)))\nend\n")
	add("stringify/key_order", "for (m in [{b: 1, a: 2}, {a: 1, b: 2, c: 3}, {B: 1, a: 2, A: 3, b: 4}, {\"a b\": 1, a: 2, \"a\": 3, \"a-\": 4, \"a_\": 5, \"a.\": 6}, {\"a<\": 1, \"a=\": 2, \"a>\": 3, \"a;\": 4, \"a?\": 5, \"a&\": 6, \"a%\": 7}, {\"\": 1, \"a\": 2, \" \": 3, \"0\": 4, \"9\": 5, \"10\": 6, \"2\": 7}, {\"\u00e9\": 1, \"e\": 2, \"f\": 3, \"\u65e5\": 4, \"z\": 5}]):\n    print(json_stringify(m))\nend\nbig = {}\nfor (i in range(60)):\n    big[\"k\" + str((i * 37) % 61)] = i\nend\nprint(json_stringify(big))\n")
	add("stringify/key_order_escapes", "m = {}\nfor (c in [\"\\\\\", \"\\\"\", \"/\", \"\", \"\\n\", \"\\t\", char(1), char(127), char(8232), char(8233), char(8234), \"<\", \">\", \"&\", \"=\", \"~\", \" \", \"!\"]):\n    m[\"a\" + c] = len(m)\nend\nprint(json_stringify(m))\nm2 = {}\nfor (c in [\"b\", \"\", char(2), \"<\", \"a\"]):\n    m2[c + \"z\"] = 1\nend\nprint(json_stringify(m2))\n")
	add("stringify/invalid_utf8_keys", "k1 = \"a\" + slice(\"\u00e9\", 1, 2)\nk2 = \"a\" + slice(\"\u00e9\", 0, 1)\nm = {}\nm[k1] = 1\nm[k2] = 2\nm[\"a\"] = 3\nprint(json_stringify(m))\nprint(len(m))\n")
	add("stringify/roundtrip", "docs = [\"[1,2,3]\", \"{\\\"a\\\":[1,{\\\"b\\\":null}],\\\"c\\\":\\\"x\\\"}\", \"[1.5,-2.5e-9,1e300,0.1,100000000000000000000]\", \"\\\"\\\\u00e9\\\\ud83d\\\\ude00\\\\n\\\"\", \"[[],{},\\\"\\\"]\", \"{\\\"<\\\":\\\">\\\",\\\"&\\\":1}\", \"[9007199254740993,-0,1e21,1e-7]\"]\nfor (d in docs):\n    r = json_parse(d)\n    s = json_stringify(r)\n    print(s, json_stringify(json_parse(s)) == s)\nend\n")
	add("stringify/shared_and_aliased", "a = [1, 2]\nb = [a, a, {k: a}]\nprint(json_stringify(b))\na[0] = 9\nprint(json_stringify(b))\nm = {x: a, y: a}\nprint(json_stringify(m))\n")
	add("stringify/header_state", "l = [1, 2, 3, 4]\npop(l)\nshift(l)\nprint(json_stringify(l))\nappend(l, 9)\nprint(json_stringify(l))\nunshift(l, 0)\nprint(json_stringify(l), len(l))\n")
	for _, d := range []int{1, 100, 254, 255, 256, 257, 258, 300, 1000} {
		add(fmt.Sprintf("stringify/nested_%d", d), fmt.Sprintf("v = 1\nfor (i in range(%d)):\n    v = [v]\nend\ntry:\n    print(len(json_stringify(v)))\ncatch (e):\n    print(e)\nend\nw = 1\nfor (i in range(%d)):\n    w = {k: w}\nend\ntry:\n    print(len(json_stringify(w)))\ncatch (e):\n    print(e)\nend\n", d, d))
	}
	add("stringify/cycles", "a = [1]\nappend(a, a)\ntry:\n    print(json_stringify(a))\ncatch (e):\n    print(e)\nend\nm = {k: 1}\nm.self = m\ntry:\n    print(json_stringify(m))\ncatch (e):\n    print(e)\nend\n")
	add("stringify/shared_doubling", "l = [0]\nfor (i in range(25)):\n    l = [l, l]\nend\ntry:\n    print(len(json_stringify(l)))\ncatch (e):\n    print(e)\nend\n")

	// The size of the text is bounded before it is built: a string by its quoted length, a number by 24.
	for _, n := range []int{174760, 174761, 174762, 174763, 174764, 262143, 262144, 349525, 349526, 524287, 524288, 1048574, 1048575, 1048576} {
		add(fmt.Sprintf("stringify/size_lt_%d", n), fmt.Sprintf("s = \"<\" * %d\ntry:\n    print(len(json_stringify(s)))\ncatch (e):\n    print(e)\nend\n", n))
	}
	for _, n := range []int{1048574, 1048575, 1048576, 1048577} {
		add(fmt.Sprintf("stringify/size_plain_%d", n), fmt.Sprintf("s = \"a\" * %d\nprint(len(json_stringify(s)))\n", n))
		add(fmt.Sprintf("stringify/size_control_%d", n/6), fmt.Sprintf("s = char(1) * %d\nprint(len(json_stringify(s)))\n", n/6))
	}
	for _, n := range []int{40000, 40320, 40328, 40329, 40330, 40331, 40340, 45000} {
		add(fmt.Sprintf("stringify/size_ints_%d", n), fmt.Sprintf("l = range(%d)\ntry:\n    print(len(json_stringify(l)))\ncatch (e):\n    print(e)\nend\n", n))
	}
	for _, n := range []int{10000, 20000, 25000, 26000, 30000} {
		add(fmt.Sprintf("stringify/size_map_%d", n), fmt.Sprintf("m = {}\nfor (i in range(%d)):\n    m[\"k\" + str(i)] = i\nend\ntry:\n    print(len(json_stringify(m)))\ncatch (e):\n    print(e)\nend\n", n/10))
	}

	// What the reader reserves and charges: 48 and 64 for each byte before, the tree after.
	docsFor := func(k int) map[string]string {
		var nums, strs, objs, nest strings.Builder
		nums.WriteString("[0")
		strs.WriteString("[\"a\"")
		objs.WriteString("{\"k0\":0")
		nest.WriteString(strings.Repeat("[", k) + strings.Repeat("]", k))
		for i := 1; i < k; i++ {
			nums.WriteString(",1")
			strs.WriteString(",\"bc\"")
			fmt.Fprintf(&objs, ",\"k%d\":%d", i, i)
		}
		nums.WriteString("]")
		strs.WriteString("]")
		objs.WriteString("}")
		return map[string]string{"ints": nums.String(), "strings": strs.String(), "object": objs.String(), "nested": nest.String()}
	}
	for _, k := range []int{1, 2, 3, 15, 16, 17, 100, 255, 256, 257, 1000, 1001} {
		for name, doc := range docsFor(k) {
			if name == "nested" && k > 255 {
				continue
			}
			spent := 16 + len(doc) + 24 + 24*k
			add(fmt.Sprintf("ledger/parse_%s_%d", name, k), ledger(fmt.Sprintf("d = %s\nr = json_parse(d)\n", ustr([]byte(doc))), spent))
		}
	}
	for _, k := range []int{0, 1, 2, 3, 7, 15, 16, 17, 31, 32, 33, 64, 100} {
		add(fmt.Sprintf("ledger/stringify_%d", k), ledger(fmt.Sprintf("l = range(%d)\ns = json_stringify(l)\nt = json_stringify({a: l})\n", k), rangeSpent(k)+2*(16+3*k)))
	}
	add("ledger/parse_nested_values", ledger("r = json_parse(\"{\\\"a\\\":[1,2,{\\\"b\\\":\\\"cd\\\"}],\\\"e\\\":{\\\"f\\\":null,\\\"g\\\":true,\\\"h\\\":1.5}}\")\n", 700))
	add("ledger/parse_empty_containers", ledger("a = json_parse(\"[]\")\nb = json_parse(\"{}\")\nc = json_parse(\"[[],{},[[]]]\")\n", 400))
	add("ledger/parse_scalars", ledger("a = json_parse(\"1\")\nb = json_parse(\"1.5\")\nc = json_parse(\"\\\"abc\\\"\")\nd = json_parse(\"null\")\ne = json_parse(\"true\")\n", 200))
	add("ledger/parse_duplicate_keys", ledger("r = json_parse(\"{\\\"a\\\":1,\\\"a\\\":2,\\\"a\\\":3,\\\"b\\\":1,\\\"b\\\":1}\")\n", 400))
	add("ledger/parse_then_append", ledger("r = json_parse(\"[1,2,3,4,5]\")\nappend(r, 6)\nappend(r, 7)\nr2 = json_parse(\"[]\")\nappend(r2, 1)\n", 700))
	for _, l := range []int{1000, 30000, 63000, 63540, 63544, 63546, 63547, 63548, 63549, 63550, 63552, 63560, 64000, 64527, 65536, 70000, 100000} {
		add(fmt.Sprintf("memory/reserve_boundary_%d", l), fmt.Sprintf("d = \" \" * %d + \"[0]\"\ntry:\n    r = json_parse(d)\n    print(len(r))\ncatch (e):\n    print(e)\nend\n", l-3))
	}
	edgeCases(add, "parse_small", "", 0, "r = json_parse(\"[1,2,3]\")", 48+64*7)
	edgeCases(add, "parse_object", "", 0, "r = json_parse(\"{\\\"a\\\":1}\")", 48+64*7)
	edgeCases(add, "parse_empty", "", 0, "r = json_parse(\"{}\")", 48+64*2)
	edgeCases(add, "parse_scalar", "", 0, "r = json_parse(\"1\")", 48+64)
	edgeCases(add, "parse_long_document", "d = \"[\" + \"1,\" * 1000 + \"1]\"\n", 16+2000+16+2000+3+16, "r = json_parse(d)", 48+64*2003)
	edgeCases(add, "stringify_list", "l = [1, 2, 3]\n", 24+48, "s = json_stringify(l)", 16+7)
	edgeCases(add, "stringify_scalar", "", 0, "s = json_stringify(1)", 16+1)
	for _, l := range []int{1000, 10000, 60000, 64000, 64500, 65000, 100000, 500000, 1000000} {
		add(fmt.Sprintf("memory/reserve_after_other_charges_%d", l), fmt.Sprintf("x = range(10000)\nd = \" \" * %d + \"[1]\"\ntry:\n    r = json_parse(d)\n    print(len(r))\ncatch (e):\n    print(e)\nend\n", l))
	}
	add("memory/parse_in_a_loop", "d = \"[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20]\"\ni = 0\nwhile (true):\n    r = json_parse(d)\n    i += 1\n    print(i)\nend\n")
	add("memory/parse_objects_in_a_loop", "d = \"{\\\"a\\\":1,\\\"bb\\\":[1,2],\\\"c\\\":{\\\"d\\\":\\\"eee\\\"}}\"\ni = 0\nwhile (true):\n    r = json_parse(d)\n    i += 1\n    print(i)\nend\n")
	add("memory/stringify_in_a_loop", "l = range(100)\ni = 0\nwhile (true):\n    s = json_stringify(l)\n    i += 1\n    print(i)\nend\n")
	add("memory/stringify_big_strings_in_a_loop", "l = \"x\" * 100000\ni = 0\nwhile (true):\n    s = json_stringify(l)\n    i += 1\n    print(i)\nend\n")
	return cs
}

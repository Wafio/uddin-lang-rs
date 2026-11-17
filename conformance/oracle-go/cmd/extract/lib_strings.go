package main

import (
	"fmt"
	"regexp"
	"strconv"
	"strings"
)

func init() {
	libRegistry = append(libRegistry, handGroup("lib-strings", stringsCases()))
}

// The helpers whose names start with pure are shared by the lib-strings, lib-conv, lib-paths, lib-stats and
// lib-math groups.

// pureLit writes s, which is valid UTF-8, as a double-quoted literal of the language.
func pureLit(s string) string {
	var b strings.Builder
	b.WriteByte('"')
	for _, r := range s {
		switch r {
		case '\\':
			b.WriteString(`\\`)
		case '"':
			b.WriteString(`\"`)
		case '\n':
			b.WriteString(`\n`)
		case '\r':
			b.WriteString(`\r`)
		case '\t':
			b.WriteString(`\t`)
		default:
			b.WriteRune(r)
		}
	}
	b.WriteByte('"')
	return b.String()
}

var pureEscape = regexp.MustCompile(`\\u([0-9a-fA-F]{4})`)

// pureUni turns the \uXXXX written in a raw string into the character: the language has no such escape, and a
// source file may not hold some of these characters.
func pureUni(s string) string {
	return pureEscape.ReplaceAllStringFunc(s, func(m string) string {
		n, _ := strconv.ParseUint(m[2:], 16, 32)
		return string(rune(n))
	})
}

func pureLits(ss ...string) []string {
	out := make([]string, len(ss))
	for i, s := range ss {
		out[i] = pureLit(s)
	}
	return out
}

// pureTry prints the value of expr under a number, or the text of the error it raises.
func pureTry(i int, expr string) string {
	return fmt.Sprintf("try:\n    print(%d, %s)\ncatch (e):\n    print(%d, \"caught: \" + e)\nend\n", i, expr, i)
}

// purePack makes cases of at most per calls each from the call expressions, after a prelude of definitions.
func purePack(name, prelude string, exprs []string, per int) []handCase {
	var cs []handCase
	for start, n := 0, 0; start < len(exprs); start, n = start+per, n+1 {
		end := min(start+per, len(exprs))
		src := prelude
		for i := start; i < end; i++ {
			src += pureTry(i, exprs[i])
		}
		cs = append(cs, handCase{fmt.Sprintf("%s/m%02d", name, n), nil, pureUni(src)})
	}
	return cs
}

// pureCalls writes name(args...) for every combination of the argument lists.
func pureCalls(name string, lists ...[]string) []string {
	out := []string{""}
	for _, l := range lists {
		var next []string
		for _, prefix := range out {
			for _, a := range l {
				if prefix == "" {
					next = append(next, a)
				} else {
					next = append(next, prefix+", "+a)
				}
			}
		}
		out = next
	}
	for i := range out {
		out[i] = name + "(" + out[i] + ")"
	}
	return out
}

// pureOther are values of every kind but a string and a number, then the numbers.
var pureOther = []string{"null", "true", "false", "[]", `["a", 1]`, "[[1], [2, [3]]]", "{}", "{a: 1}", "f", "len"}
var pureNumbers = []string{"0", "1", "-1", "2.5", "-0.5", "9223372036854775807", "(-9223372036854775807 - 1)", "1e300"}

// pureInvalid is the prelude that builds strings that are not UTF-8 from bytes of valid ones, which a script
// can do with a subscript or a cut: a lone lead byte, a lone continuation byte, cut sequences, a surrogate and
// a code point past the last one.
const pureInvalid = `lead = "é"[0]
cont = "é"[1]
rep = lower(lead)
cut3 = substr("€", 0, 2)
cut4 = substr("😀", 0, 3)
sur = "퟿"[0] + "à"[1] + "Ā"[1]
big = "􀀀"[0] + "ᐐ"[2] + "Ā"[1] + "Ā"[1]
mixed = "ab" + lead + "é" + cont + "c" + cut3 + "d"
inf = 1e308 * 10
nan = inf - inf
fun f():
    return 1
end
`

var pureBad = []string{"lead", "cont", "cut3", "cut4", "sur", "big", "mixed"}

func stringsCases() []handCase {
	var cs []handCase
	add := func(name, src string) { cs = append(cs, handCase{name, nil, pureUni(src)}) }
	addAll := func(more []handCase) { cs = append(cs, more...) }

	valid := pureLits("", "a", "abc", "Hello, World", "  padded \t\n", "aAbB zZ", "ÄÖÜ äöü ß ẞ ÿ Ÿ", "İıIi", "ǅǆǄ ǈǉǇ",
		"ΣΑΣ ΟΔΥΣΣΕΎΣ", "ᾈᾀ ᾳ ῼ ᾼ", "日本語", "😀 smile", "\u00a0nbsp\u00a0", "\u2003em\u2003\u3000", "\u0085nel\u0085", "\u200bzw\u200b",
		"\ufeffbom", "a\u0000b", "\u1680ogham\u1680", "\u180eMongolian\u180e", "ﬁ ﬂ ŉ ǰ", "Ⅷ ⅷ Ⓐ ⓐ", "ꭰ Ꭰ", "𐐀𐐨 𞤀𞤢")
	valid = append(valid, `rep + "replacement" + rep`)
	short := append(pureLits("", "a", "ab", "aXa", "héllo", "日本"), "rep")
	bad := pureBad
	allStr := append(append([]string{}, valid...), bad...)
	all := append(append(append([]string{}, allStr...), pureOther...), pureNumbers...)

	// One argument: lower upper trim reverse_str.
	for _, fn := range []string{"lower", "upper", "trim", "reverse_str"} {
		var calls []string
		for _, a := range all {
			calls = append(calls, fn+"("+a+")")
		}
		calls = append(calls, fn+"()", fn+`("a", "b")`, fn+"(1, 2, 3)")
		addAll(purePack("lower_upper_trim_reverse/"+fn, pureInvalid, calls, 40))
	}
	// What they return for strings that need no change, and for a bad byte.
	add("case/identity_and_length", pureInvalid+`print(lower("abc") == "abc", upper("ABC") == "ABC", len(lower(lead)), len(upper(cont)), len(lower(mixed)), len(reverse_str(mixed)))
print(len(trim(mixed)), len(trim("  " + mixed + "  ")), typeof(lower(lead)))
print(lower("İ"), len(lower("İ")), len(lower(rep)), len(upper(rep)), upper("ß"), upper("ŉ"), lower("ẞ"), upper("ǆ"), upper("ᾀ"), lower("ᾈ"), upper("ﬁ"))
print(lower("Σ"), lower("ΑΣ"), upper("σς"), upper("ſ"), lower("K"), upper("ı"), lower("Ω"))
print(reverse_str("héllo, 日本語 😀"), reverse_str(cut4), len(reverse_str(cut4)))
print(len(trim("\u00a0 x \u2003")), len(trim("\u200bx")), len(trim("\u0085x\u0085")), len(trim("\u180ex")), len(trim("\u1680x")))
`)

	// Two arguments, strings: contains find starts_with ends_with.
	pairs := func(fn string, left, right []string) []string { return pureCalls(fn, left, right) }
	strPairLeft := append(append([]string{}, short...), "mixed", "lead", "cut3", "sur")
	strPairRight := append(append([]string{}, short...), "lead", "cont", "cut3", `"é"`, `"€"`)
	for _, fn := range []string{"contains", "find", "starts_with", "ends_with"} {
		calls := pairs(fn, strPairLeft, strPairRight)
		for _, a := range []string{`"abc"`, `"héllo"`, "mixed"} {
			for _, b := range append(append([]string{}, pureOther...), pureNumbers...) {
				calls = append(calls, fn+"("+a+", "+b+")")
			}
		}
		for _, a := range append(append([]string{}, pureOther[:3]...), pureNumbers[:4]...) {
			calls = append(calls, fn+"("+a+`, "a")`, fn+"("+a+", 1)")
		}
		calls = append(calls, fn+"()", fn+`("a")`, fn+`("a", "b", "c")`)
		addAll(purePack("search/"+fn, pureInvalid, calls, 50))
	}
	// A list as the haystack: == per element, in the order needle first.
	lists := []string{"[]", "[1, 2, 3]", `[1, "1", 1.0, true, null]`, `["a", "b", "a"]`, "[[1], [1, 2], [1, [2]]]", "[{a: 1}, {a: 1}]", "[nan, inf, -inf]", "[0.0, -0.0]", "[f, len]", "[null, [null]]", `[1.5, 2, "2", [2]]`}
	needles := []string{"1", "1.0", `"1"`, "true", "null", `"a"`, "[1]", "[1, 2]", "[[1]]", "{a: 1}", "nan", "inf", "0", "-0.0", "2", "[2]", "f", "len", "[null]", "[]", `""`}
	var listCalls []string
	for _, fn := range []string{"contains", "find"} {
		listCalls = append(listCalls, pureCalls(fn, lists, needles)...)
	}
	add2 := purePack("search/lists", pureInvalid, listCalls, 60)
	addAll(add2)
	add("search/self_reference_direct", pureInvalid+"l = [1]\nl[0] = l\nprint(contains(l, 1))\n")
	add("search/self_reference_nested", pureInvalid+"l = [1, [2]]\nl[1][0] = l\ntry:\n    print(contains(l, 1))\ncatch (e):\n    print(e)\nend\nprint(1)\n")
	add("search/deep_nesting_257", pureInvalid+"l = [1]\ni = 0\nwhile (i < 257):\n    l = [l]\n    i += 1\nend\ntry:\n    print(contains(l, 1))\ncatch (e):\n    print(e)\nend\nprint(2)\n")
	add("search/deep_nesting_255", pureInvalid+"l = [1]\ni = 0\nwhile (i < 254):\n    l = [l]\n    i += 1\nend\nprint(contains(l, 1), find(l, 1), find(l, l))\n")
	add("search/not_found_nested", pureInvalid+"l = [[[1, 2], [3]], [4]]\nprint(contains(l, 4), find(l, [4]), find(l, [[3]]), contains(l, [[1, 2], [3]]))\n")
	// The walk of a big list costs one operation per element, so the third call is over the budget.
	add("search/ops_for_a_big_list", pureInvalid+"l = [0] * 40000\ni = 0\nwhile (i < 5):\n    print(i)\n    print(contains(l, 39999), find(l, 5))\n    i += 1\nend\n")
	add("search/ops_for_a_big_string_is_none", pureInvalid+"s = \"ab\" * 400000\ni = 0\nwhile (i < 300):\n    c = contains(s, \"ba\") and find(s, \"bb\") == -1\n    i += 1\nend\nprint(i, c)\n")

	// split
	splitCalls := pureCalls("split", append(append([]string{}, valid...), bad...), pureLits("", " ", ",", "ab", "é", "日", "\n", "aa", "\u00a0"))
	splitCalls = append(splitCalls, pureCalls("split", append(append([]string{}, valid...), bad...), []string{"null", "lead", "cut3"})...)
	splitCalls = append(splitCalls, pureCalls("split", append(append([]string{}, valid...), bad...))...)
	for _, a := range []string{`"a b"`, `"a,b"`, "mixed"} {
		for _, b := range append(append(append([]string{}, pureOther[1:]...), pureNumbers...), "true") {
			splitCalls = append(splitCalls, "split("+a+", "+b+")")
		}
	}
	for _, a := range append(append([]string{}, pureOther...), pureNumbers...) {
		splitCalls = append(splitCalls, "split("+a+")", "split("+a+`, ",")`, "split("+a+", null)")
	}
	splitCalls = append(splitCalls, "split()", `split("a", ",", 1)`, `split("a", ",", 1, 2)`)
	addAll(purePack("split/matrix", pureInvalid, splitCalls, 45))
	add("split/runes_and_fields", pureInvalid+`print(split("héllo日本😀"))
print(split("héllo日本😀", ""))
print(split(mixed, ""), len(split(mixed, "")), len(split(cut3, "")), split("", ""), split("", ","), split("", null), split("   ", null))
print(split(" a  b\t\nc\u00a0d\u2003e\u0085f\u200bg ", null))
print(split("a,b,,c,", ","), split(",", ","), split("aaa", "aa"), split("aaaa", "aa"), split("abc", "abcd"))
print(split("a" + lead + "b" + lead, lead), split(mixed, cont), split(mixed, "é"))
x = split("hello world")
x[0] = "changed"
print(x, len(x))
`)

	// join
	elems := [][]string{{}, {`"a"`}, {`"a"`, `"b"`}, {`"a"`, "1", "2.5", "true", "null"}, {`["a", 1]`, `{a: "b"}`, "[[]]"}, {"f", "len"}, {"lead", "mixed", "cut3"}, {"nan", "inf", "-0.0", "1e21", "1e-7", "100000", "1e6"}, {`"é\n\t\"\\"`, `"日"`, `"\u00a0"`}, {`"x"`, `"y"`, `"z"`}}
	var lists2 []string
	for _, e := range elems {
		lists2 = append(lists2, "["+strings.Join(e, ", ")+"]")
	}
	joinCalls := pureCalls("join", lists2, pureLits("", ",", ", ", "é", "日本", "\n"))
	joinCalls = append(joinCalls, pureCalls("join", lists2, append([]string{"lead", "mixed"}, "1", "null", "[]", "true"))...)
	joinCalls = append(joinCalls, pureCalls("join", append(append([]string{}, pureOther...), pureNumbers...), []string{`","`, "1", "null"})...)
	joinCalls = append(joinCalls, pureCalls("join", pureLits("abc", ""), []string{`","`, "1"})...)
	joinCalls = append(joinCalls, "join()", `join([1])`, `join([1], ",", 3)`)
	addAll(purePack("join/matrix", pureInvalid, joinCalls, 45))

	// replace
	reps := append(pureLits("", "a", "ab", "é", "日本", "aa"), "rep")
	repCalls := pureCalls("replace", append(pureLits("", "aaa", "abab", "héllo wörld", "日本語日本"), `rep + "a"`, "mixed", "lead", "cut3"), append(reps, "lead", "cont"), append(reps, "lead", "cut3"))
	for _, a := range append(append([]string{}, pureOther...), pureNumbers[:3]...) {
		repCalls = append(repCalls, `replace("abc", "b", `+a+`)`, `replace("abc", `+a+`, "x")`, "replace("+a+`, "b", "x")`)
	}
	repCalls = append(repCalls, "replace()", `replace("a")`, `replace("a", "b")`, `replace("a", "b", "c", "d")`)
	addAll(purePack("replace/matrix", pureInvalid, repCalls, 50))
	add("replace/insertions_between_runes", pureInvalid+`print(replace("héllo", "", "-"), replace("", "", "-"), replace(mixed, "", "|"), replace("日本", "", "😀"))
print(replace("aaaa", "aa", "b"), replace("aaa", "aa", "b"), replace("abc", "abc", ""), replace("abc", "x", "yy"), replace("abc", "c", "c"))
print(len(replace(lead, lead, "")), len(replace(mixed, lead, rep)), len(replace("a" + rep + "b", rep, "é")))
`)

	// substr
	var subCalls []string
	idx := []string{"(-9223372036854775807 - 1)", "-100", "-2", "-1", "0", "1", "2", "3", "4", "5", "6", "9", "9223372036854775807"}
	for _, s := range []string{`""`, `"abc"`, `"héllo"`} {
		subCalls = append(subCalls, pureCalls("substr", []string{s}, idx, idx)...)
	}
	for _, s := range []string{"mixed", "lead", `"日本語"`} {
		subCalls = append(subCalls, pureCalls("substr", []string{s}, []string{"0", "1", "2", "4", "7"}, []string{"0", "1", "3", "5", "7", "12"})...)
	}
	for _, a := range append(append([]string{}, pureOther...), "2.5", "-0.5", "1e300") {
		subCalls = append(subCalls, `substr("abc", `+a+", 2)", `substr("abc", 0, `+a+")", "substr("+a+", 0, 1)")
	}
	subCalls = append(subCalls, "substr()", `substr("abc")`, `substr("abc", 1)`, `substr("abc", 0, 1, 2)`, `substr(1, 2, 3)`, `substr("abc", "a", 1)`, `substr("abc", 0, "b")`, `substr(1, "a", "b")`, `substr("abc", 1.0, 2)`, `substr("abc", 0, 2.0)`)
	addAll(purePack("substr/matrix", pureInvalid, subCalls, 60))
	add("substr/results_share_and_cut_runes", pureInvalid+`s = "héllo wörld"
print(substr(s, 0, 1), substr(s, 1, 2) == lead, len(substr(s, 1, 2)), substr(s, 1, 3), substr(s, 0, len(s)) == s, substr(s, 3, 3) == "", len(substr(s, 0, 0)))
t = substr(s, 1, 2) + substr(s, 2, 3)
print(t, len(t), typeof(t))
`)

	// What the results cost in memory. A substring of a string costs a header, not its bytes.
	big := `s = "x" * 100000
`
	add("memory/substr_shares", pureInvalid+big+"i = 0\nwhile (i < 5000):\n    t = substr(s, 1, 99999)\n    i += 1\nend\nprint(i, len(t))\n")
	add("memory/trim_shares", pureInvalid+"s = \" \" + \"x\" * 100000 + \" \"\ni = 0\nwhile (i < 5000):\n    t = trim(s)\n    i += 1\nend\nprint(i, len(t))\n")
	add("memory/trim_unchanged_shares", pureInvalid+big+"i = 0\nwhile (i < 5000):\n    t = trim(s)\n    i += 1\nend\nprint(i, len(t))\n")
	add("memory/lower_unchanged_shares", pureInvalid+big+"i = 0\nwhile (i < 5000):\n    t = lower(s)\n    u = upper(\"X\" * 5)\n    i += 1\nend\nprint(i, len(t))\n")
	add("memory/lower_changed_costs_bytes", pureInvalid+"s = \"X\" * 100000\ni = 0\nwhile (i < 100):\n    t = lower(s)\n    print(i)\n    i += 1\nend\n")
	add("memory/upper_changed_costs_bytes", pureInvalid+"s = \"é\" * 50000\ni = 0\nwhile (i < 100):\n    t = upper(s)\n    print(i)\n    i += 1\nend\n")
	add("memory/lower_bad_bytes_cost_bytes", pureInvalid+"s = \"a\" + lead\ns = s * 50000\ni = 0\nwhile (i < 100):\n    t = lower(s)\n    print(i)\n    i += 1\nend\n")
	add("memory/replace_unchanged_shares", pureInvalid+big+"i = 0\nwhile (i < 5000):\n    t = replace(s, \"y\", \"z\")\n    u = replace(s, \"x\", \"x\")\n    i += 1\nend\nprint(i, len(t))\n")
	add("memory/replace_changed_costs_bytes", pureInvalid+big+"i = 0\nwhile (i < 100):\n    t = replace(s, \"x\", \"y\")\n    print(i)\n    i += 1\nend\n")
	add("memory/reverse_costs_bytes", pureInvalid+big+"i = 0\nwhile (i < 100):\n    t = reverse_str(s)\n    print(i)\n    i += 1\nend\n")
	add("memory/join_costs_bytes", pureInvalid+"l = [\"x\" * 1000] * 100\ni = 0\nwhile (i < 100):\n    t = join(l, \"\")\n    print(i)\n    i += 1\nend\n")
	add("memory/split_elements_are_charged", pureInvalid+"s = \"ab,\" * 20000\ni = 0\nwhile (i < 100):\n    t = split(s, \",\")\n    print(i)\n    i += 1\nend\n")
	add("memory/split_runes_share", pureInvalid+"s = \"ab\" * 20000\ni = 0\nwhile (i < 100):\n    t = split(s, \"\")\n    print(i)\n    i += 1\nend\n")
	add("memory/split_bad_bytes_make_strings", pureInvalid+"s = lead * 20000\ni = 0\nwhile (i < 100):\n    t = split(s, \"\")\n    print(i)\n    i += 1\nend\n")
	add("memory/split_fields_share", pureInvalid+"s = \"ab \" * 20000\ni = 0\nwhile (i < 100):\n    t = split(s)\n    print(i)\n    i += 1\nend\n")
	for _, p := range []int{1000, 10000, 30000, 60000, 87380} {
		add(fmt.Sprintf("memory/split_reserve_%d", p), pureInvalid+fmt.Sprintf("x = \",\" * %d\ni = 0\nwhile (i < 10):\n    l = split(x, \",\")\n    i += 1\n    print(i, len(l))\nend\n", p-1))
	}
	for _, p := range []int{85595, 85596, 85597, 85598, 85599} {
		add(fmt.Sprintf("memory/split_reserve_edge_%d", p), pureInvalid+fmt.Sprintf("x = \",\" * %d\nl = split(x, \",\")\nprint(len(l))\n", p-1))
	}
	add("memory/split_whitespace_estimate", pureInvalid+"x = \"a \" * 40000\nl = split(x)\nprint(len(l))\ny = \"a\" * 100000\nm = split(y)\nprint(len(m))\n")
	add("memory/split_estimate_by_separator_count", pureInvalid+"x = \"a\" * 100000\nl = split(x, \"b\")\nprint(len(l))\nm = split(x, \"\")\nprint(len(m))\n")
	add("memory/split_runes_estimate_over", pureInvalid+"x = \"é\" * 50000\nl = split(x, \"\")\nprint(len(l))\n")
	add("memory/split_runes_estimate_over_2", pureInvalid+"x = \"a\" * 100000\nl = split(x, \"\")\nprint(len(l))\n")
	add("memory/split_max_parts", pureInvalid+"x = \"a\" * 1048576\nl = split(x, \"\")\nprint(len(l))\n")
	add("memory/split_error_caught_after_reserve", pureInvalid+"x = \"a b \" * 20000\ntry:\n    split(x, 5)\ncatch (e):\n    print(e)\nend\nprint(len(split(x)))\n")

	// The guards: the length a result may have is checked before the call.
	for _, k := range []int{524287, 524288, 524289} {
		add(fmt.Sprintf("guard/replace_doubles_%d", k), pureInvalid+fmt.Sprintf("s = \"a\" * %d\nprint(len(s))\nt = replace(s, \"a\", \"bb\")\nprint(len(t))\n", k))
	}
	add("guard/replace_grows_empty_old_524287", pureInvalid+"s = \"a\" * 524287\nt = replace(s, \"\", \"bb\")\nprint(len(t))\n")
	add("guard/replace_grows_empty_old_349525", pureInvalid+"s = \"a\" * 349525\nprint(len(replace(s, \"\", \"bb\")))\n")
	add("guard/replace_grows_empty_old_349526", pureInvalid+"s = \"a\" * 349526\nprint(len(replace(s, \"\", \"bb\")))\n")
	add("guard/replace_longer_old_does_not_guard", pureInvalid+"s = \"ab\" * 100000\nt = replace(s, \"ab\", \"c\")\nprint(len(t))\n")
	add("guard/replace_equal_length_does_not_guard", pureInvalid+"s = \"a\" * 1000000\nprint(len(replace(s, \"a\", \"b\")))\n")
	add("guard/replace_huge_replacement_no_match", pureInvalid+"s = \"a\" * 1000\nr = \"b\" * 1000000\nprint(len(replace(s, \"zz\", r)))\n")
	add("guard/replace_huge_replacement_with_matches", pureInvalid+"s = \"a\" * 1000\nr = \"b\" * 1000000\nprint(len(replace(s, \"a\", r)) > 0)\n")
	// A bad argument counts as an empty string in the guard, so these stop before the call reports the type.
	add("guard/replace_first_not_a_string", pureInvalid+"r = \"b\" * 1000000\nprint(replace(5, \"a\", r))\n")
	add("guard/replace_old_not_a_string", pureInvalid+"r = \"b\" * 1000000\nprint(replace(\"a\" * 1000000, 5, r))\n")
	add("guard/replace_new_not_a_string", pureInvalid+"print(replace(\"a\" * 1000000, \"a\", 5))\n")
	add("guard/replace_old_not_a_string_small", pureInvalid+"r = \"b\" * 1000\ntry:\n    print(replace(\"a\" * 1000, 5, r))\ncatch (e):\n    print(e)\nend\n")
	// n elements whose quoted text and separators come to the value length limit, one byte under, at and over.
	// A limit ends the run, so each size is a case of its own.
	for _, n := range []int{1, 100, 1000, 10000} {
		for _, sep := range []int{0, 3} {
			for _, d := range []int{-1, 0, 1} {
				ll := (1048576-2)/n - 4 - sep + d
				add(fmt.Sprintf("guard/join_n%d_sep%d_%+d", n, sep, d), pureInvalid+fmt.Sprintf("x = \"a\" * %d\nl = [x] * %d\nprint(len(join(l, %s)))\n", ll, n, pureLit(strings.Repeat("-", sep))))
			}
		}
	}
	add("guard/join_nested_counts_quoted", pureInvalid+"x = \"a\" * 262140\nl = [[x]]\nprint(len(join(l, \"\")))\nl = [[x, x]]\nprint(len(join(l, \"\")))\n")
	add("guard/join_separator_counts", pureInvalid+"l = [\"a\"] * 40000\nprint(len(join(l, \"\")))\nprint(len(join(l, \"x\")))\n")
	for _, sep := range []int{20, 21, 22} {
		// 40,000 elements of quoted length 3 and a separator: 2 + 40000*(3+2) + 40000*sep.
		add(fmt.Sprintf("guard/join_separator_counts_%d", sep), pureInvalid+fmt.Sprintf("l = [\"a\"] * 40000\nprint(len(join(l, %s)))\n", pureLit(strings.Repeat("x", sep))))
	}
	add("guard/join_not_a_list_does_not_guard", pureInvalid+"x = \"a\" * 1000000\ntry:\n    join(x, x)\ncatch (e):\n    print(e)\nend\ntry:\n    join([x], 5)\ncatch (e):\n    print(e)\nend\n")
	add("guard/join_quoting_costs_more", pureInvalid+"x = \"\\\\\" * 500000\ntry:\n    print(len(join([x], \"\")))\ncatch (e):\n    print(e)\nend\nprint(1)\n")
	add("guard/join_bad_bytes_cost_more", pureInvalid+"x = lead * 200000\nprint(len(x))\nprint(len(join([x], \"\")))\n")
	add("guard/join_shared_list_doubled", pureInvalid+"l = [\"a\" * 1000]\ni = 0\nwhile (i < 40):\n    l = [l, l]\n    i += 1\n    print(i)\nend\n")
	// A rune that does not print counts ten bytes in the bound, one that does only its width.
	for _, c := range []struct {
		name, rune string
		n          int
	}{{"soft_hyphen", "\u00ad", 104857}, {"line_separator", "\u2028", 104857}, {"e_acute", "é", 524285}} {
		for _, d := range []int{0, 1} {
			add(fmt.Sprintf("guard/join_%s_%d", c.name, d), pureInvalid+fmt.Sprintf("x = %s * %d\nprint(len(join([x], \"\")))\n", pureLit(c.rune), c.n+d))
		}
	}
	add("search/long_needle_in_a_repetitive_haystack", pureInvalid+"s = \"a\" * 1000000\nn = \"a\" * 500000 + \"b\"\nprint(contains(s, n), find(s, n))\nm = \"a\" * 1000 + \"b\"\nprint(find(s + \"b\", m), find(s + \"b\", \"ab\"), contains(s, \"b\"))\n")
	add("search/needle_longer_than_a_short_one", pureInvalid+"s = \"abcabcabdabcabcabcabe\"\nprint(find(s, \"abcabcabe\"), find(s, \"abcabcabcabe\"), find(s, \"abcabd\"), find(s, \"bcabcabcabe\"), find(s, \"abcabcabcabeX\"), find(s, \"\"), contains(s, \"\"))\nprint(split(s, \"abcabcab\"), replace(s, \"abcabcab\", \"-\"), split(\"aaaaaaaaaaaaaaaa\", \"aaaaaaaaaa\"))\n")
	add("guard/join_nested_shared", pureInvalid+"l = [\"abc\"]\ni = 0\nwhile (i < 14):\n    l = [l, l]\n    i += 1\nend\nprint(len(join(l, \",\")))\n")
	add("guard/contains_nested_shared", pureInvalid+"l = [1]\ni = 0\nwhile (i < 14):\n    l = [l, l]\n    i += 1\nend\nprint(contains(l, 2))\nprint(find(l, 2))\n")
	add("guard/contains_nested_shared_over", pureInvalid+"l = [1]\ni = 0\nwhile (i < 17):\n    l = [l, l]\n    i += 1\nend\nprint(contains(l, 2))\n")
	return cs
}

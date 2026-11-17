package main

import (
	"fmt"
	"strings"
)

func init() {
	libRegistry = append(libRegistry, handGroup("lib-regex", regexCases()), handGroup("lib-regex-limits", regexLimitCases()))
	registerRegexExtras()
}

// uq writes s as a double-quoted uddin string: the tokenizer knows \" \\ \t \r \n and takes anything else as it
// is.
func uq(s string) string {
	var b strings.Builder
	b.WriteByte('"')
	for _, r := range s {
		switch r {
		case '"':
			b.WriteString(`\"`)
		case '\\':
			b.WriteString(`\\`)
		case '\t':
			b.WriteString(`\t`)
		case '\r':
			b.WriteString(`\r`)
		case '\n':
			b.WriteString(`\n`)
		default:
			b.WriteRune(r)
		}
	}
	b.WriteByte('"')
	return b.String()
}

// regexAll prints what each of the six builtins gives for one text and pattern (and a replacement), a call to
// each in its own try so that a type error does not hide the rest.
func regexAll(text, pattern, repl string) string {
	t, p, r := uq(text), uq(pattern), uq(repl)
	var b strings.Builder
	for _, e := range []string{
		"is_regex_match(" + p + ", " + t + ")",
		"regex_match(" + t + ", " + p + ")",
		"regex_find(" + t + ", " + p + ")",
		"regex_find_all(" + t + ", " + p + ")",
		"regex_replace(" + t + ", " + p + ", " + r + ")",
		"regex_split(" + t + ", " + p + ")",
	} {
		b.WriteString(tryPrint(e))
	}
	return b.String()
}

var regexInvalid = []string{
	"(", "(a", "a)", ")", "(()", "())", "[", "[a", "[^", "[]", "[]a", "[^]", "[a-", "[z-a]", "[a-\\d]", "[[:foo:]]", "[[:alpha:]",
	"[[:alpha:]-z]", "*", "+", "?", "a**", "a++", "a+*", "a?*", "a*+", "a{2}{3}", "a{1001}", "a{2,1}", "a{1000,1001}", "{1}a", "(a{500}){3}",
	"(?z)", "(?i", "(?i-", "(?i-)", "(?--i)", "(?P<n>a", "(?P<>a)", "(?P<1n>a)", "(?P<n-m>a)", "(?P<n", "(?P=n)", "(?P>n)", "(?<n>a", "(?<>a)",
	"(?=a)", "(?!a)", "(?<=a)", "(?<!a)", "(?>a)", "(?#c)", "(?", "(?:", "(?:a", "\\", "a\\", "\\C", "\\Z", "\\E", "\\G", "\\X", "\\R", "\\H", "\\K", "\\q",
	"\\1", "\\8", "\\9", "\\x", "\\x1", "\\xZZ", "\\x{", "\\x{}", "\\x{110000}", "\\x{FFFFFFFF}", "\\x{1g}", "\\u0041", "\\p", "\\pX", "\\p{", "\\p{}", "\\p{Foo}",
	"\\p{L&}", "\\P{Foo}", "\\p{Old_Italic}", "\\p{^}", "[\\p]", "[\\pX]", "[\\p{Foo}]", "\\é", "\\ ", "a|*", "(|*)", "|+", "a{,2}{", "x{2", "x{2,",
	"(?i)(?-i", "[\\Q]", "\\Qabc", "[a-a-a]", "[--a]", "[a--]", "[\\d-z]", "(?P<n>a)(?P<n>b)", "(?P<name>", "a{99999999999}", "a{1,99999999999}",
}

var regexValid = []string{
	"", "a", "abc", "a|b", "a|ab|abc", "(a)(b)(c)", "(?:a)(?:b)", "(?P<x>a)(?P<y>b)", "(?<x>a)(?<y>b)", "a*", "a+", "a?", "a*?", "a+?", "a??",
	"a{2}", "a{2,}", "a{2,3}", "a{2}?", "a{0}", "a{0,1}", "a{1,}", "a{,2}", "a{2,x}", "a{", "a{}", "a{1", "{", "{a}", "x{1,2}{", "(?i)abc", "(?i:a)b",
	"(?i)k", "(?i)s", "(?i)[a-z]+", "(?s).", "(?m)^a", "(?m)a$", "(?U)a+", "(?U)a+?", "(?i-s)a", "(?is)a.b", "(?-i)a", "(?i)(?-i)a", "(?)", "(?:)", "()",
	".", "^", "$", "^$", "\\A", "\\z", "\\b", "\\B", "\\bfoo\\b", "\\Bo", "(?m)^", "(?m)$", "(?m)^$", "^.*$", "(?s)^.*$", "\\d", "\\D", "\\s", "\\S", "\\w", "\\W",
	"\\d+", "\\w+", "\\s+", "[abc]", "[^abc]", "[a-c]", "[^a-c]", "[a-cx-z]", "[]]", "[^]]", "[]a]", "[a-]", "[-a]", "[a\\-z]", "[\\]]", "[\\\\]", "[\\d]", "[\\D]",
	"[\\w-]", "[\\s\\S]", "[^\\s\\S]", "[[:alpha:]]", "[[:^alpha:]]", "[[:digit:][:space:]]", "[[:word:]]", "[[:upper:]]", "[[:punct:]]", "[[:xdigit:]]",
	"[[:alpha:]-z]", "\\pL", "\\PL", "\\p{L}", "\\p{^L}", "\\P{^L}", "\\p{Lu}", "\\p{lu}", "\\p{Greek}", "\\p{greek}", "\\p{Han}", "\\p{Cyrillic}", "\\pN", "\\p{Nd}",
	"\\p{Zs}", "\\p{Any}", "\\p{Ascii}", "\\p{Assigned}", "\\p{Lc}", "\\p{Letter}", "\\p{Uppercase_Letter}", "[\\pL\\pN]", "[^\\pL]", "[\\p{Lu}\\d]", "(?i)\\p{Lu}", "(?i)\\P{Lu}",
	"\\x41", "\\x{41}", "\\x{263A}", "\\x{1F600}", "\\101", "\\0", "\\012", "\\a\\f\\n\\r\\t\\v", "\\.", "\\*", "\\+", "\\?", "\\(", "\\)", "\\[", "\\]", "\\{", "\\}", "\\|", "\\\\", "\\/", "\\-", "\\_", "\\ ",
	"\\Qa.b\\E", "\\Qa.b", "\\Q\\E", "a\\Q*\\Eb", "\\Q\\\\E", "[a-z]+@[a-z]+\\.com", "(a+)+", "(a*)*", "(a|b)*c", "(a*)+", "(|a)+", "(a|)+", "(a||b)", "((a))", "(a(b(c)))",
	"(foo|foobar)", "(foobar|foo)", "foo|foobar|food", "abc|abd|abe", "ab|ac|ad", "a|b|c|d", "ab+|ab*|abc", "^(a|b)$", "a.c", "a.*c", "a.*?c", "(.*)(.*)", "(.+)(.+)",
	"(\\w+)\\s(\\w+)", "(\\d+)-(\\d+)", "(?P<year>\\d{4})-(?P<month>\\d{2})-(?P<day>\\d{2})", "[0-9]{1,3}(\\.[0-9]{1,3}){3}", "^/api/[a-z]+$", "^(GET|POST)$",
	"(?i)select.+from", "union\\s+select", "<script[^>]*>", "\\x00", "\\x7f", "é", "日本", "[é日]", "[^é]", "(?i)é", "(?i)É", "(?i)ǆ", "(?i)Ǆ", "ǅ", "(?i)ſ", "(?i)K", "(?i)Σ", "(?i)ς",
}

func regexCases() []handCase {
	var cs []handCase
	add := func(name, src string) { cs = append(cs, handCase{name, nil, src}) }

	// Each builtin on ordinary input.
	add("basic/is_regex_match", "print(is_regex_match(\"^-?\\\\d+\\\\.\\\\d+$\", \"3.14\"))\nprint(is_regex_match(\"^-?\\\\d+\\\\.\\\\d+$\", \"3.\"))\nprint(is_regex_match(\"\", \"\"))\nprint(is_regex_match(\"a\", \"\"))\nprint(is_regex_match(\"\", \"abc\"))\n")
	add("basic/regex_match", "print(regex_match(\"hello@example.com\", \"^[\\\\w._%+-]+@[\\\\w.-]+\\\\.[a-zA-Z]{2,}$\"))\nprint(regex_match(\"nope\", \"^[\\\\w._%+-]+@[\\\\w.-]+\\\\.[a-zA-Z]{2,}$\"))\nprint(regex_match(\"\", \"\"))\nprint(regex_match(\"abc\", \"b\"))\n")
	add("basic/regex_find", "print(regex_find(\"hello world 123\", \"\\\\d+\"))\nprint(regex_find(\"hello\", \"\\\\d+\"))\nprint(regex_find(\"hello\", \"x*\"))\nprint(typeof(regex_find(\"hello\", \"x*\")))\nprint(regex_find(\"hello\", \"l+\"))\nprint(regex_find(\"\", \"\"))\n")
	add("basic/regex_find_all", "print(regex_find_all(\"hello 123 world 456\", \"\\\\d+\"))\nprint(regex_find_all(\"abc\", \"x\"))\nprint(regex_find_all(\"abc\", \"\"))\nprint(regex_find_all(\"abc\", \"x*\"))\nprint(len(regex_find_all(\"aaaa\", \"a\")))\nprint(regex_find_all(\"\", \"\"))\nprint(regex_find_all(\"\", \"a\"))\n")
	add("basic/regex_replace", "print(regex_replace(\"hello 123 world\", \"\\\\d+\", \"XXX\"))\nprint(regex_replace(\"abc\", \"x\", \"y\"))\nprint(regex_replace(\"abc\", \"\", \"-\"))\nprint(regex_replace(\"\", \"\", \"-\"))\nprint(regex_replace(\"aaa\", \"a*\", \"X\"))\nprint(regex_replace(\"baaac\", \"a*\", \"X\"))\nprint(regex_replace(\"abc\", \"b\", \"\"))\n")
	add("basic/regex_split", "print(regex_split(\"hello,world;test\", \"[,;]\"))\nprint(regex_split(\"abc\", \"x\"))\nprint(regex_split(\"abc\", \"\"))\nprint(regex_split(\"\", \"x\"))\nprint(regex_split(\"\", \"\"))\nprint(regex_split(\",a,,b,\", \",\"))\nprint(regex_split(\"a1b22c\", \"\\\\d*\"))\nprint(len(regex_split(\"a b  c\", \" +\")))\n")

	// The argument forms: every builtin with the wrong number of arguments (the forms with a limit fail with an
	// arity error in the sandbox) and with each argument of a wrong type.
	for _, name := range []string{"is_regex_match", "regex_match", "regex_find", "regex_find_all", "regex_replace", "regex_split"} {
		var calls []string
		for _, args := range []string{"", "\"a\"", "\"a\", \"b\"", "\"a\", \"b\", \"c\"", "\"a\", \"b\", 1", "\"a\", \"b\", \"c\", \"d\"", "\"a\", \"b\", 1, 2"} {
			calls = append(calls, name+"("+args+")")
		}
		src := ""
		for _, c := range calls {
			src += tryPrint(c)
		}
		add("arity/"+name, src)
		src = ""
		third := ""
		if name == "regex_replace" {
			third = ", \"x\""
		}
		for _, bad := range []string{"1", "1.5", "null", "true", "[1]", "{a: 1}", "len", "fun(x): return x end"} {
			src += tryPrint(name + "(" + bad + ", \"a\"" + third + ")")
			src += tryPrint(name + "(\"a\", " + bad + third + ")")
			if name == "regex_replace" {
				src += tryPrint(name + "(\"a\", \"a\", " + bad + ")")
			}
		}
		add("types/"+name, src)
	}
	add("types/both_wrong", tryPrint("is_regex_match(1, 2)")+tryPrint("regex_match(1, 2)")+tryPrint("regex_find(null, null)")+tryPrint("regex_find_all([], {})")+tryPrint("regex_replace(1, 2, 3)")+tryPrint("regex_split(1.5, true)"))
	add("types/error_values_are_values", "x = regex_match(1, \"a\")\nprint(typeof(x))\nprint(x)\ny = regex_match(\"a\", \"(\")\nprint(typeof(y))\nprint(len(str(y)))\nif (y) then:\n    print(\"then\")\nend\n")
	add("types/error_value_raised_by_if", "x = regex_find(\"a\", \"[\")\ntry:\n    if (x) then:\n        print(\"then\")\n    end\ncatch (e):\n    print(\"caught: \" + e)\nend\n")

	// Invalid patterns, by the error each gives. is_regex_match says false for all of them.
	for i, p := range regexInvalid {
		add(fmt.Sprintf("invalid/%03d", i), regexAll("abc", p, "x"))
	}

	// Valid patterns the parser has to accept, with a text they match in different ways.
	texts := []string{"abc", "aaa", "foo bar", "Hello, World 2023-12-25", "kK K ſS", "é日本 É", "line1\nline2\nline3\n", ""}
	for i, p := range regexValid {
		for j, t := range texts[:2] {
			add(fmt.Sprintf("valid/%03d_%d", i, j), regexAll(t, p, "<$0|$1>"))
		}
		add(fmt.Sprintf("valid/%03d_x", i), regexAll(texts[4+i%4], p, "[${0}]"))
	}

	// Replacement templates.
	repls := []string{"", "x", "$", "$$", "$$$", "$0", "$1", "${1}", "$1x", "${1}x", "$x", "${x}", "$name", "${name}", "${name", "${", "$1$2", "${1}${2}", "$01", "${01}", "$10", "$ 1", "$-1",
		"\\1", "\\$1", "<$0>", "é$1é", "$é", "${é}", "$_", "${_}", "$1_", "$word", "$a1", "$9", "$99999999999", "${99999999999}", "[$0][$0]", "$$1", "$${1}"}
	for i, r := range repls {
		add(fmt.Sprintf("template/%02d", i), tryPrint("regex_replace("+uq("John Smith")+", "+uq("(?P<name>\\w+) (\\w+)")+", "+uq(r)+")")+
			tryPrint("regex_replace("+uq("aXbXc")+", "+uq("(X)|(Y)")+", "+uq(r)+")")+
			tryPrint("regex_replace("+uq("abc")+", "+uq("")+", "+uq(r)+")")+
			tryPrint("regex_replace("+uq("a.b")+", "+uq("(?P<name>\\.)")+", "+uq(r)+")"))
	}

	// Empty matches and the way a search moves on after one.
	for i, p := range []string{"", "x*", "a*", "a*?", "\\b", "\\B", "^", "$", "(?m)^", "(?m)$", "a|", "|a", "(a|)", "(|a)*", "\\w*", "\\W*", ".*", ".*?", "(?s).*", "[^a]*", "b*"} {
		for j, t := range []string{"", "a", "abc", "baaac", "aaa", "a b", "é日", "ab\nab\n"} {
			add(fmt.Sprintf("empty/%02d_%d", i, j), regexAll(t, p, "-"))
		}
	}

	// Text that is not ASCII.
	add("unicode/dot_is_a_character", "print(regex_find(\"日本語\", \".\"))\nprint(len(regex_find(\"日本語\", \".\")))\nprint(regex_find_all(\"日本語\", \".\"))\nprint(regex_find_all(\"日本語\", \"\"))\nprint(regex_split(\"日本語\", \"\"))\nprint(regex_replace(\"日本語\", \"\", \"-\"))\n")
	add("unicode/case_folding", regexAll("KkK ſsS Σσς ǄǅǆİıiI", "(?i)[ksσǆi]+", "_"))
	add("unicode/word_boundary_is_ascii", regexAll("é a é", "\\b", "|")+regexAll("naïve café", "\\b\\w+\\b", "|"))
	add("unicode/classes", regexAll("aé日1٣ٱ Ωλ", "\\pL+", "_")+regexAll("aé日1٣ٱ Ωλ", "\\p{Greek}+", "_")+regexAll("aé日1٣ٱ Ωλ", "\\d+", "_")+regexAll("aé日1٣ٱ Ωλ", "\\p{Nd}+", "_")+regexAll("aé日1٣ٱ Ωλ", "\\w+", "_"))
	add("unicode/astral", regexAll("x😀y😀", "😀", "_")+regexAll("x😀y😀", "[😀-😁]", "_")+regexAll("x😀y", "\\x{1F600}", "_")+regexAll("x😀y", ".y", "_"))

	// Anchors, flags and lines.
	add("anchors/lines", regexAll("a\nb\nc", "(?m)^.", "_")+regexAll("a\nb\nc", "(?m).$", "_")+regexAll("a\nb\nc", "^.", "_")+regexAll("a\nb\nc", ".$", "_")+regexAll("a\r\nb", "(?m)$", "|")+regexAll("a\nb\n", "$", "|")+regexAll("a\nb\n", "(?m)$", "|"))
	add("anchors/text", regexAll("ab", "\\Aa", "_")+regexAll("ab", "b\\z", "_")+regexAll("ab\n", "b\\z", "_")+regexAll("ab\n", "b$", "_")+regexAll("ab", "^", "|")+regexAll("ab", "$", "|"))
	add("flags/scoped", regexAll("aAbB", "(?i)a(?-i)A", "_")+regexAll("aAbB", "(?i:a)A", "_")+regexAll("aAbB", "(?i)a((?-i)A)b", "_")+regexAll("a\nb", "(?s)a.b", "_")+regexAll("a\nb", "a.b", "_")+regexAll("a\nb", "(?s:a.)b", "_")+regexAll("aaa", "(?U)a+", "_")+regexAll("aaa", "(?U)a+?", "_")+regexAll("aaa", "(?U:a+)a", "_"))
	add("greedy/lazy", regexAll("<a><b>", "<.+>", "_")+regexAll("<a><b>", "<.+?>", "_")+regexAll("aaa", "a{1,2}", "_")+regexAll("aaa", "a{1,2}?", "_")+regexAll("aaa", "a*?$", "_")+regexAll("ab", "(a+?)(b*)", "$1|$2"))
	add("alternation/leftmost_first", regexAll("foobar", "foo|foobar", "_")+regexAll("foobar", "foobar|foo", "_")+regexAll("abcd", "ab|abc|abcd", "_")+regexAll("xyz", "x|xy|xyz", "_")+regexAll("aaab", "(a|aa)*b", "_")+regexAll("aaab", "(a*)(a*)b", "$1|$2"))
	add("groups/capture_numbers", regexAll("abcdef", "(a)(b(c)(d))(e)(?:f)", "$1/$2/$3/$4/$5/$6"))
	add("groups/names", regexAll("2023-12-25", "(?P<year>\\d+)-(?P<month>\\d+)-(?P<day>\\d+)", "${day}/${month}/${year}")+regexAll("ab", "(?P<x>a)(?P<x>b)", "$x")+regexAll("b", "(?P<x>a)|(?P<x>b)", "[$x]")+regexAll("2023-12-25", "(?<year>\\d+)-(?<month>\\d+)", "${month}.${year}"))
	add("groups/unmatched", regexAll("b", "(a)?b", "[$1]")+regexAll("b", "(a)|b", "[$1]")+regexAll("ab", "(a)|(b)", "[$1$2]")+regexAll("b", "(?:(a)|b)+", "[$1]"))
	add("groups/nested_repeat", regexAll("aaa", "(a*)*", "[$1]")+regexAll("aaa", "(a*)+", "[$1]")+regexAll("aaa", "(a|b)*", "[$1]")+regexAll("ab", "(a*)*b", "[$1]")+regexAll("", "(a*)*", "[$1]")+regexAll("b", "(a*)+", "[$1]"))

	// Several calls in one run, and more patterns than the cache holds.
	var many strings.Builder
	for i := 0; i < 40; i++ {
		many.WriteString(fmt.Sprintf("print(regex_find(\"abc%d\", \"c%d\"))\n", i, i))
	}
	for i := 0; i < 40; i += 3 {
		many.WriteString(fmt.Sprintf("print(regex_find(\"abc%d\", \"c%d\"))\nprint(is_regex_match(\"%d\", \"x%d\"))\n", i, i, i, i))
	}
	add("cache/more_than_sixteen", many.String())
	add("cache/same_pattern_many_times", "i = 0\nn = 0\nwhile (i < 500):\n    if (is_regex_match(\"^a+b$\", \"aaab\")) then:\n        n += 1\n    end\n    i += 1\nend\nprint(n)\n")
	add("cache/invalid_pattern_many_times", "i = 0\nwhile (i < 300):\n    x = regex_match(\"a\", \"(\")\n    i += 1\nend\nprint(x)\n")
	add("cache/dynamic_patterns", "i = 0\nwhile (i < 100):\n    x = regex_find(\"abcdefghij\", str(i) + \"|c.e\")\n    i += 1\nend\nprint(x)\n")
	add("results/types", "x = regex_find_all(\"a1b2\", \"\\\\d\")\nprint(typeof(x))\nprint(len(x))\nprint(typeof(x[0]))\nx[0] = 5\nprint(x)\ny = regex_split(\"a,b\", \",\")\nprint(y + [\"c\"])\ns = regex_find(\"abc\", \"b\")\nprint(s + \"!\")\nprint(s == \"b\")\nprint(len(s))\n")
	add("results/shared_with_argument", "s = \"abcdef\" * 3\nx = regex_find_all(s, \"cd\")\nprint(x)\nprint(len(s))\nt = regex_replace(s, \"cd\", \"\")\nprint(t)\nprint(regex_split(s, \"e\"))\n")
	add("results/in_a_function", "fun words(s):\n    return regex_find_all(s, \"[a-z]+\")\nend\nprint(words(\"ab cd, ef\"))\nprint(len(words(\"\")))\nfun f(a, b):\n    return regex_match(a, b)\nend\nprint(f(\"abc\", \"b\"))\nprint(map([\"a1\", \"b\"], fun(s): return is_regex_match(\"\\\\d\", s) end))\n")
	add("results/join_and_len", "print(join(regex_find_all(\"a1b22c333\", \"\\\\d+\"), \"+\"))\nprint(len(regex_split(\"a,b,c\", \",\")))\n")
	add("errors/caught_text_is_cut", "p = \"(\" + (\"a\" * 1500)\nx = regex_match(\"a\", p)\nprint(len(str(x)))\ny = regex_find(\"a\", p)\nprint(typeof(y))\ntry:\n    if (y) then:\n    end\ncatch (e):\n    print(len(e))\nend\n")
	add("errors/pattern_in_error_is_whole_pattern", regexAll("a", "a)", "x")+regexAll("a", "(b(c)d", "x")+regexAll("a", "[a-z", "x")+regexAll("a", "x\\", "x"))
	add("errors/escapes_in_error_text", regexAll("a", "\\x{110000}", "x")+regexAll("a", "\\xZ1", "x")+regexAll("a", "[\\q]", "x")+regexAll("a", "\\8", "x")+regexAll("a", "a\\1b", "x")+regexAll("a", "(?i)\\pX", "x"))
	return cs
}

// regexLimitCases are the cases that reach the limits of the sandbox on a regular expression: the size of a
// pattern, the memory its parse and compile cost, the work a search may do, and what a result may weigh.
func regexLimitCases() []handCase {
	var cs []handCase
	add := func(name, src string) { cs = append(cs, handCase{name, nil, src}) }

	// The size of a pattern: four bytes to an instruction of the 1,024 the profile allows, then the program.
	for _, n := range []int{1022, 1023, 1024, 1025, 1026, 2048} {
		add(fmt.Sprintf("size/literal_%d", n), "p = \"a\" * "+fmt.Sprint(n)+"\nprint(is_regex_match(p, \"a\" * "+fmt.Sprint(n)+"))\nprint(regex_find(\"b\", p))\n")
	}
	for _, n := range []int{340, 341, 342, 500, 511, 512, 513} {
		add(fmt.Sprintf("size/group_alternation_%d", n), "p = \"(a|b)\" * "+fmt.Sprint(n)+"\nprint(is_regex_match(p, \"ab\"))\n")
		add(fmt.Sprintf("size/starred_group_%d", n), "p = \"(?:ab*)\" * "+fmt.Sprint(n)+"\nprint(is_regex_match(p, \"ab\"))\n")
	}
	for _, n := range []int{100, 101, 102, 200, 300, 1000} {
		add(fmt.Sprintf("size/repeat_%d", n), "print(is_regex_match(\"a{"+fmt.Sprint(n)+"}\", \"a\"))\nprint(is_regex_match(\"(?:a{"+fmt.Sprint(n)+"})\", \"a\"))\nprint(is_regex_match(\"(?:ab){"+fmt.Sprint(n)+",}\", \"a\"))\n")
	}
	add("size/nested_repeat", "print(is_regex_match(\"(?:a{10}){10}\", \"a\"))\nprint(is_regex_match(\"(?:a{10}){10}{1}\", \"a\"))\n")
	add("size/pattern_too_long_for_the_ratio", "p = \"x\" * 4100\nprint(is_regex_match(p, \"a\"))\n")
	add("size/pattern_at_the_ratio", "p = \"x\" * 4099\nx = is_regex_match(p, \"a\")\nprint(x)\n")
	add("size/invalid_pattern_is_not_sized", "p = \"(\" * 3000\nprint(regex_match(\"a\", p))\n")
	add("size/deep_nesting", "p = \"(\" * 1500 + \")\" * 1500\nprint(regex_match(\"a\", p))\nq = \"(?:\" * 900 + \"a\" + \")\" * 900\nprint(regex_match(\"a\", q))\n")

	// Memory: a parse costs 256 bytes per byte of pattern, a compile 256 KiB and 1 KiB per instruction, twice for
	// the builtins that search past the first match, and each match 160 bytes before the call.
	add("memory/charges_are_returned", "i = 0\nwhile (i < 2000):\n    x = regex_find_all(\"abc\", \"b\")\n    y = regex_replace(\"abc\", \"b\", \"c\")\n    i += 1\nend\nprint(i)\n")
	add("memory/big_pattern_repeated", "p = \"a\" * 1000\ni = 0\nwhile (i < 100):\n    x = regex_find_all(\"abc\", p)\n    i += 1\nend\nprint(i)\n")
	add("memory/pattern_charge_with_other_data", "l = range(30000)\np = \"a\" * 1000\nprint(is_regex_match(p, \"b\"))\n")
	add("memory/pattern_charge_with_other_data_multi", "l = range(30000)\np = \"a\" * 1000\nprint(regex_find_all(\"b\", p))\n")
	add("memory/multi_costs_twice", "l = range(40000)\np = \"a\" * 500\nprint(regex_find(\"b\", p))\nprint(regex_split(\"b\", p))\n")
	for _, n := range []int{26000, 26100, 26200, 26205, 26210, 26213, 26214, 26215, 26220, 26300, 27000, 100000} {
		add(fmt.Sprintf("memory/find_all_reserve_%d", n), "s = \"a\" * "+fmt.Sprint(n)+"\nx = regex_find_all(s, \"b\")\nprint(len(x))\n")
		add(fmt.Sprintf("memory/split_reserve_%d", n), "s = \"a\" * "+fmt.Sprint(n)+"\nx = regex_split(s, \"b\")\nprint(len(x))\n")
	}
	for _, n := range []int{1000, 5000, 10000, 20000, 26000, 100000} {
		add(fmt.Sprintf("memory/find_all_every_position_%d", n), "s = \"a\" * "+fmt.Sprint(n)+"\nx = regex_find_all(s, \"\")\nprint(len(x))\n")
		add(fmt.Sprintf("memory/replace_every_position_%d", n), "s = \"a\" * "+fmt.Sprint(n)+"\nx = regex_replace(s, \"\", \"--\")\nprint(len(x))\n")
	}
	for _, n := range []int{100, 1000, 10000, 100000, 500000, 1000000} {
		add(fmt.Sprintf("memory/replace_%d", n), "s = \"a\" * "+fmt.Sprint(n)+"\nx = regex_replace(s, \"a\", \"b\")\nprint(len(x))\n")
		add(fmt.Sprintf("memory/find_%d", n), "s = \"a\" * "+fmt.Sprint(n)+"\nx = regex_find(s, \"a\")\nprint(len(x))\ny = regex_match(s, \"^a+$\")\nprint(y)\n")
	}
	add("memory/replace_value_size", "s = \"a\" * 1000000\nx = regex_replace(s, \"a\", \"bb\")\nprint(len(x))\n")
	add("memory/replace_value_size_guard_empty_repl", "s = \"a\" * 1000000\nx = regex_replace(s, \"a\", \"\")\nprint(len(x))\n")
	add("memory/replace_guard_boundary", "s = \"a\" * 1000\nx = regex_replace(s, \"a\", \"b\" * 1000)\nprint(len(x))\n")
	add("memory/replace_guard_boundary_over", "s = \"a\" * 1024\nx = regex_replace(s, \"a\", \"b\" * 1024)\nprint(len(x))\n")
	add("memory/replace_guard_caught", "s = \"a\" * 1500\ntry:\n    x = regex_replace(s, \"q\", \"b\" * 1500)\n    print(len(x))\ncatch (e):\n    print(\"caught\")\nend\nprint(\"after\")\n")

	// Work: a search charges the program length for each character it reads. The budget of a run is 524,288.
	add("work/long_scan_no_match", "s = \"a\" * 100000\nprint(regex_find(s, \"b\"))\n")
	add("work/long_scan_no_match_is_match", "s = \"a\" * 100000\nprint(is_regex_match(\"b\", s))\n")
	add("work/leftmost_stops_early", "s = \"ab\" + \"a\" * 1000000\nprint(regex_find(s, \"ab\"))\nprint(is_regex_match(\"ab\", s))\n")
	add("work/loop_accumulates", "s = \"a\" * 20000\ni = 0\nwhile (i < 100):\n    x = regex_find(s, \"b\")\n    i += 1\nend\nprint(i)\n")
	add("work/loop_accumulates_small", "s = \"abcdefghij\" * 10\ni = 0\nwhile (i < 100):\n    x = regex_find(s, \"[a-c]+j\")\n    i += 1\nend\nprint(i)\n")
	add("work/quadratic_find_all", "s = \"a\" * 16384\nx = regex_find_all(s, \"(?:a*b)|a\")\nprint(len(x))\n")
	add("work/quadratic_replace", "s = \"a\" * 16384\nx = regex_replace(s, \"(?:a*b)|a\", \"x\")\nprint(len(x))\n")
	add("work/quadratic_split", "s = \"a\" * 16384\nx = regex_split(s, \"(?:a*b)|a\")\nprint(len(x))\n")
	add("work/quadratic_small", "s = \"a\" * 200\nx = regex_find_all(s, \"(?:a*b)|a\")\nprint(len(x))\n")
	add("work/many_matches", "s = \"ab\" * 5000\nx = regex_find_all(s, \"a\")\nprint(len(x))\ny = regex_split(s, \"b\")\nprint(len(y))\nz = regex_replace(s, \"a\", \"c\")\nprint(len(z))\n")
	add("work/body_like_input", "s = \"word1 word2 \" * 342\nx = is_regex_match(\"(?i)(?:\\\\w+\\\\s*){50}x\", s)\nprint(x)\n")
	add("work/body_like_small_input", "s = \"word1 word2 \" * 4\nx = is_regex_match(\"(?i)(?:\\\\w+\\\\s*){50}x\", s)\nprint(x)\n")
	add("work/compile_work_alone", "i = 0\nwhile (i < 400):\n    x = is_regex_match(\"a{50}\", \"\")\n    i += 1\nend\nprint(i)\n")
	add("work/compile_work_multi", "i = 0\nwhile (i < 400):\n    x = regex_find_all(\"\", \"a{50}\")\n    i += 1\nend\nprint(i)\n")
	add("work/budget_is_per_run_not_per_call", "s = \"a\" * 5000\nprint(regex_find(s, \"b\"))\nprint(regex_find(s, \"b\"))\nprint(regex_find(s, \"b\"))\nprint(regex_find(s, \"b\"))\nprint(regex_find(s, \"b\"))\n")
	add("work/caught_limit_still_ends_the_run", "s = \"a\" * 1000000\ntry:\n    print(regex_find(s, \"b\"))\ncatch (e):\n    print(\"caught\")\nend\nprint(\"after\")\n")
	return cs
}

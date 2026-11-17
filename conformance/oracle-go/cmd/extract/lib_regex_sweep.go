package main

import (
	"encoding/base64"
	"fmt"
	"math/rand"
	"os"
	"regexp"
	"regexp/syntax"
	"strconv"
	"strings"
	"unicode/utf8"
)

// registerRegexExtras adds what is too big or too tied to the host to live in the corpus: the generated sweep
// (UDDIN_REGEX_SWEEP=<pairs>, written by `-only lib-regex-sweep -out <dir>`) and the facts file the unit tests
// of the pattern parser read (UDDIN_REGEX_FACTS=<path>, which writes the file and exits).
func registerRegexExtras() {
	if v := os.Getenv("UDDIN_REGEX_SWEEP"); v != "" {
		pairs, err := strconv.Atoi(v)
		if err != nil || pairs <= 0 {
			fmt.Fprintln(os.Stderr, "UDDIN_REGEX_SWEEP: not a count:", v)
			os.Exit(2)
		}
		seed := int64(1)
		if s := os.Getenv("UDDIN_REGEX_SEED"); s != "" {
			seed, _ = strconv.ParseInt(s, 10, 64)
		}
		libRegistry = append(libRegistry, handGroup("lib-regex-sweep", regexSweepCases(pairs, seed)))
	}
	if path := os.Getenv("UDDIN_REGEX_FACTS"); path != "" {
		if err := writeRegexFacts(path); err != nil {
			fmt.Fprintln(os.Stderr, "UDDIN_REGEX_FACTS:", err)
			os.Exit(1)
		}
		os.Exit(0)
	}
}

// ----- the sweep -----

var (
	sweepAtoms = []string{
		"a", "b", "c", "x", "1", "0", " ", "-", "_", "é", "日", "K", "ſ", "\\.", "\\*", "\\+", "\\?", "\\(", "\\)", "\\[", "\\]", "\\{", "\\}", "\\|", "\\\\", "\\/", "\\-",
		"\\d", "\\D", "\\w", "\\W", "\\s", "\\S", ".", ".", "\\x41", "\\x{e9}", "\\x{65E5}", "\\n", "\\t", "\\012", "\\Qa.\\E", "\\Q*\\E",
		"[abc]", "[^abc]", "[a-c]", "[^a-c]", "[a-cx-z0-9]", "[\\d\\s]", "[[:alpha:]]", "[[:^digit:]]", "[[:upper:][:digit:]]", "[\\pL]", "[^\\pL]", "[a\\-z]", "[\\]x]", "[]a]", "[a-]", "[^\\n]", "[\\x{e9}-\\x{65E5}]",
		"\\pL", "\\PL", "\\p{Lu}", "\\p{^Ll}", "\\p{Greek}", "\\p{Han}", "\\pN", "\\p{Nd}", "\\p{Zs}", "\\p{Latin}", "\\p{Any}", "\\p{Ascii}", "\\P{N}", "\\p{Letter}", "\\p{lu}",
	}
	// Forms that look like what other engines have and RE2 refuses, and rare corners of the syntax.
	sweepCorners = []string{
		"(?>a)", "(?=a)", "(?!a)", "(?<=a)", "(?<!a)", "\\1", "\\k<n>", "(?P=n)", "a*+", "a++", "a?+", "a{2}+", "a*?+", "[[:alpha:]-z]", "\\pZ", "\\p{Cs}", "\\p{Zl}", "[\\p{L}&&[^a]]",
		"[a-\\d]", "[\\Q-\\E]", "\\Q\\E", "(?i)(?-i)", "(?:)", "()", "(|)", "(?s:.)", "(?U)(a+)(b*?)", "x{0,}", "x{1,1}?", "(?i:(?-i:a))", "[^\\x00-\\x{10FFFF}]", "\\pN\\PN", "[\\d-\\s]",
	}
	sweepSimple  = []string{"a", "b", "c", "x", "1", ".", "\\d", "\\w", "\\s", "\\b", "^", "$", "[abc]", "[^a]", "é", "日", "(a)", "(a|b)", "(?:ab)", "(?P<name>a)", "(?i)a", "a|b", "ab|ac", "a|", "|a"}
	sweepAnchors = []string{"^", "$", "\\A", "\\z", "\\b", "\\B", "(?m:^)", "(?m:$)"}
	sweepFlags   = []string{"(?i)", "(?s)", "(?m)", "(?U)", "(?i-s)", "(?ms)", "(?-i)", "(?is)", "(?mi)"}
	sweepQuants  = []string{"*", "+", "?", "*?", "+?", "??", "{2}", "{1,3}", "{2,}", "{0,2}", "{0}", "{1}", "{2}?", "{1,2}?"}
	sweepTexts   = []string{
		"", "a", "abc", "aaa", "abcabc", "a1b2c3", "Hello World", "foo bar baz", "  lead and trail  ", "line1\nline2\nline3", "x\n", "\n", "é日本語", "KkKſS",
		"ab-cd_ef", "1234567890", "AbCdEf", "a.b.c", "2023-12-25", "foo@bar.com", "<a><b>", "tab\there", "aXbxC", "ſ", "É é", "Σσς", "a\r\nb", "()[]{}*+?|\\^$.", "00 11 22", "x😀y",
	}
	sweepRepls = []string{"", "X", "[$0]", "$1", "${1}x", "$name", "${name}", "$$", "$1$2", "<$0>", "$", "${", "$a1", "é$0", "$10", "${0}${0}", "$x"}
)

func genAtom(r *rand.Rand) string {
	if r.Intn(100) < 7 {
		return sweepCorners[r.Intn(len(sweepCorners))]
	}
	return sweepAtoms[r.Intn(len(sweepAtoms))]
}

func genQuant(r *rand.Rand) string {
	if r.Intn(100) < 40 {
		return sweepQuants[r.Intn(len(sweepQuants))]
	}
	return ""
}

func genTerm(r *rand.Rand, depth int) string {
	switch n := r.Intn(100); {
	case n < 52:
		return genAtom(r) + genQuant(r)
	case n < 60:
		return sweepAnchors[r.Intn(len(sweepAnchors))]
	case n < 85 && depth < 3:
		inner := genExpr(r, depth+1)
		var open string
		switch r.Intn(6) {
		case 0, 1:
			open = "("
		case 2:
			open = "(?:"
		case 3:
			open = "(?P<name>"
		case 4:
			open = "(?<n" + strconv.Itoa(r.Intn(3)) + ">"
		default:
			open = []string{"(?i:", "(?s:", "(?U:", "(?m:", "(?i-s:", "(?-i:"}[r.Intn(6)]
		}
		return open + inner + ")" + genQuant(r)
	default:
		return genAtom(r) + genQuant(r)
	}
}

func genConcat(r *rand.Rand, depth int) string {
	var b strings.Builder
	for i, n := 0, 1+r.Intn(3); i < n; i++ {
		b.WriteString(genTerm(r, depth))
	}
	return b.String()
}

func genExpr(r *rand.Rand, depth int) string {
	e := genConcat(r, depth)
	for r.Intn(100) < 25 {
		if r.Intn(10) == 0 {
			e += "|"
		} else {
			e += "|" + genConcat(r, depth)
		}
	}
	return e
}

// genPattern is mostly a valid pattern, and sometimes one a few bytes away from it.
func genPattern(r *rand.Rand) string {
	var p string
	if r.Intn(100) < 40 {
		for i, n := 0, 1+r.Intn(3); i < n; i++ {
			p += sweepSimple[r.Intn(len(sweepSimple))] + genQuant(r)
		}
	} else {
		p = genExpr(r, 0)
	}
	if r.Intn(100) < 18 {
		p = sweepFlags[r.Intn(len(sweepFlags))] + p
	}
	if r.Intn(100) < 14 && len(p) > 0 {
		i := r.Intn(len(p))
		for i > 0 && !utf8.RuneStart(p[i]) {
			i--
		}
		switch r.Intn(5) {
		case 0:
			_, w := utf8.DecodeRuneInString(p[i:])
			p = p[:i] + p[i+w:]
		case 1:
			p = p[:i] + []string{"(", ")", "[", "]", "{", "}", "*", "+", "?", "|", "\\", "^", "$", ".", "**", "(?", "[:", "\\p", "\\x", "{2,1}", "(?P<", "\\1"}[r.Intn(22)] + p[i:]
		case 2:
			p = p[:i]
		case 3:
			p += []string{"*", "+", "?", "{2}", "{", "\\", ")", "[a", "|"}[r.Intn(9)]
		default:
			p = []string{"(", "["}[r.Intn(2)] + p
		}
	}
	return p
}

func genText(r *rand.Rand) string {
	if r.Intn(100) < 55 {
		return sweepTexts[r.Intn(len(sweepTexts))]
	}
	alphabet := []rune("abcxX01 -_é日K.\n\tAB")
	n := r.Intn(14)
	out := make([]rune, n)
	for i := range out {
		out[i] = alphabet[r.Intn(len(alphabet))]
	}
	return string(out)
}

// regexSweepCases makes scripts of 25 pairs each; every pair goes through the six builtins. The scripts are kept
// small so that the work of a run stays far from the budget, which the sweep does not test.
func regexSweepCases(pairs int, seed int64) []handCase {
	r := rand.New(rand.NewSource(seed))
	var cs []handCase
	const perScript = 25
	for i := 0; i < pairs; i += perScript {
		var b strings.Builder
		for j := i; j < i+perScript && j < pairs; j++ {
			b.WriteString(regexAll(genText(r), genPattern(r), sweepRepls[r.Intn(len(sweepRepls))]))
		}
		cs = append(cs, handCase{fmt.Sprintf("%06d", i), nil, b.String()})
	}
	return cs
}

// ----- the facts file -----

func b64(s string) string { return base64.StdEncoding.EncodeToString([]byte(s)) }

// regexProgramEstimate is the guard's estimate of the program (see the sandbox of the fork).
func regexProgramEstimate(re *syntax.Regexp, limit int) int {
	capAt := func(n int) int {
		if n < 0 || n > limit {
			return limit + 1
		}
		return n
	}
	mul := func(a, b int) int {
		if a < 0 || b < 0 {
			return -1
		}
		if a == 0 || b == 0 {
			return 0
		}
		if b > (1<<62)/a {
			return -1
		}
		return a * b
	}
	switch re.Op {
	case syntax.OpLiteral:
		return capAt(len(re.Rune))
	case syntax.OpCapture:
		return capAt(2 + regexProgramEstimate(re.Sub[0], limit))
	case syntax.OpStar, syntax.OpPlus, syntax.OpQuest:
		return capAt(1 + regexProgramEstimate(re.Sub[0], limit))
	case syntax.OpConcat, syntax.OpAlternate:
		n := len(re.Sub)
		for _, sub := range re.Sub {
			n = capAt(n + regexProgramEstimate(sub, limit))
			if n > limit {
				return n
			}
		}
		return n
	case syntax.OpRepeat:
		count := re.Max
		if count < 0 {
			count = re.Min + 1
		}
		if count < 1 {
			count = 1
		}
		return capAt(mul(count, 1+regexProgramEstimate(re.Sub[0], limit)))
	default:
		return 1
	}
}

func programLen(re *syntax.Regexp) int {
	prog, err := syntax.Compile(re.Simplify())
	if err != nil || len(prog.Inst) == 0 {
		return 1
	}
	return len(prog.Inst)
}

func writeRegexFacts(path string) error {
	f, err := os.Create(path)
	if err != nil {
		return err
	}
	defer f.Close()
	out := func(fields ...string) error {
		_, err := fmt.Fprintln(f, strings.Join(fields, "\t"))
		return err
	}

	patterns := append(append([]string{}, regexInvalid...), regexValid...)
	for _, p := range sweepAtoms {
		patterns = append(patterns, p, p+"*", "("+p+")", "(?i)"+p)
	}
	for _, p := range []string{"(" + strings.Repeat("(", 1100) + strings.Repeat(")", 1100) + ")", strings.Repeat("(", 1200) + strings.Repeat(")", 1200), "a{1000}", "(a{1000}){1}", "((a{10}){10}){10}", "((a{10}){10}){11}", "(a{2}){500}", "(a{2}){501}", "(?:a{1000}){1}b", "x{0}", "(?:a{0}){5}", "(?:a*){3,7}", "(?:a?){2,}", "(?:a+){2}", "(?:(?:a*)*)*", "[\xff]", "a\xffb", "\\p{L\xff}", "(?P<\xff>a)", "\\Q\xff\\E", "\xff", "(?i)\xe2\x82"} {
		patterns = append(patterns, p)
	}
	for _, p := range patterns {
		fact := []string{"parse", b64(p), "", "0", "0", "0"}
		re, err := syntax.Parse(p, syntax.Perl)
		if err != nil {
			fact[2] = b64(err.Error())
		} else {
			fact[3] = strconv.Itoa(regexProgramEstimate(re, 1024))
			fact[4] = strconv.Itoa(programLen(re))
			wrap := &syntax.Regexp{Op: syntax.OpConcat, Sub: []*syntax.Regexp{
				{Op: syntax.OpBeginText},
				{Op: syntax.OpAnyChar},
				{Op: syntax.OpStar, Flags: syntax.NonGreedy, Sub: []*syntax.Regexp{{Op: syntax.OpAnyChar}}},
				{Op: syntax.OpCapture, Cap: 1, Sub: []*syntax.Regexp{re}},
			}}
			fact[5] = strconv.Itoa(programLen(wrap))
		}
		if err := out(fact...); err != nil {
			return err
		}
	}

	bad := []string{"\xff", "a\xffb", "\xe2\x82", "\xc3\x28", "ab\xed\xa0\x80cd", "\xf0\x9f\x98", "\xff\xfe\xfd", "é\xc3", "\x80abc", "abc\x80", "\xc0\x80x", "a\xf4\x90\x80\x80b", "日\xe6\x97", "\xef\xbf\xbd\xff"}
	texts := append(append([]string{}, sweepTexts...), bad...)
	r := rand.New(rand.NewSource(7))
	repls := []string{"X", "[$0]", "$1", "${name}", "$$", "é$0", ""}
	var searchPatterns []string
	for _, p := range regexValid {
		if _, err := regexp.Compile(p); err == nil {
			searchPatterns = append(searchPatterns, p)
		}
	}
	for _, p := range []string{".", "\\x{FFFD}", "[^a]", "\\w*", "\\W", "\\b", "(?s).", "[\\x{FFFD}]", "\\pL", "\\PL", "(?i)k", ".*", "x*", "^", "$", "\\B", "\\s*", "[^\\x{FFFD}]+", "(?m)^", "(?m)$"} {
		for _, t := range bad {
			texts = append(texts, t)
		}
		searchPatterns = append(searchPatterns, p)
	}
	for n := 0; n < 1500; n++ {
		p := searchPatterns[r.Intn(len(searchPatterns))]
		var t string
		if r.Intn(100) < 40 {
			t = bad[r.Intn(len(bad))]
		} else {
			t = texts[r.Intn(len(texts))]
		}
		if r.Intn(4) == 0 {
			t += texts[r.Intn(len(texts))]
		}
		re := regexp.MustCompile(p)
		matched := "0"
		if re.MatchString(t) {
			matched = "1"
		}
		span := func(l []int) string { return strconv.Itoa(l[0]) + "," + strconv.Itoa(l[1]) }
		find := "-"
		if loc := re.FindStringIndex(t); loc != nil {
			find = span(loc)
		}
		var all []string
		for _, loc := range re.FindAllStringIndex(t, -1) {
			all = append(all, span(loc))
		}
		if len(all) == 0 {
			all = []string{"-"}
		}
		split := "-"
		if parts := re.Split(t, -1); len(parts) > 0 {
			var enc []string
			for _, part := range parts {
				enc = append(enc, b64(part))
			}
			split = strings.Join(enc, ",")
		}
		var repl []string
		for _, rp := range repls {
			repl = append(repl, b64(rp)+":"+b64(re.ReplaceAllString(t, rp)))
		}
		if err := out("search", b64(p), b64(t), matched, find, strings.Join(all, ";"), split, strings.Join(repl, ",")); err != nil {
			return err
		}
	}
	return nil
}

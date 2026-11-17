package main

import (
	"fmt"
	"strings"
)

func init() {
	libRegistry = append(libRegistry, handGroup("lib-sort", sortCases()))
}

// The scripts of sort: the order it gives, the order of its comparisons (a comparison that fails stops the sort,
// and the list keeps what the swaps had done), the kinds of value it compares, and what it costs.

const sortPrelude = "fun f():\nend\ninf = 10 ** 400\nnan = inf - inf\n"

// perm is a deterministic permutation of 0..n-1 (n and 7 are coprime when n is not a multiple of 7; otherwise
// the step is 11, and a multiple of 77 uses 13).
func perm(n int) []int {
	step := 7
	for _, s := range []int{7, 11, 13, 17} {
		if n%s != 0 {
			step = s
			break
		}
	}
	out := make([]int, n)
	for i := range out {
		out[i] = (i * step) % n
	}
	return out
}

func listSrc(vals []string) string { return "[" + strings.Join(vals, ", ") + "]" }

func intsSrc(vs []int) string {
	s := make([]string, len(vs))
	for i, v := range vs {
		s[i] = fmt.Sprint(v)
	}
	return listSrc(s)
}

func sortCases() []handCase {
	var cs []handCase
	add := func(name, src string) { cs = append(cs, handCase{name, nil, src}) }

	add("basic/ints", "l = [5, 2, 8, 1, 9, 3, 3, -4, 0]\nsort(l)\nprint(l)\nsort(l)\nprint(l)\nprint(sort(l))\n")
	add("basic/floats_and_mixed_numbers", "l = [2.5, 1, 3, 0.5, 2, 1.0, -1.5, 10, 2.5]\nsort(l)\nprint(l)\nm = [1, 1.0, 1, 1.0, 0.5, 1]\nsort(m)\nprint(map(m, typeof))\nn = [1.0, 1, 1.0, 1, 2, 1.0]\nsort(n)\nprint(map(n, typeof))\n")
	add("basic/strings", "l = [\"pear\", \"apple\", \"fig\", \"\", \"Zed\", \"apple\", \"éclair\", \"zed\", \"a b\"]\nsort(l)\nprint(l)\n")
	add("basic/lists", "l = [[2, 1], [1, 5], [1, 2], [], [1], [2], [1, 2, 0], [0, 9]]\nsort(l)\nprint(l)\nm = [[\"b\"], [\"a\", \"z\"], [\"a\"]]\nsort(m)\nprint(m)\nn = [[2, [1]], [2, [0]], [1, [9]], [2, []]]\nsort(n)\nprint(n)\n")
	add("basic/small_sizes", "a = []\nb = [1]\nc = [2, 1]\nd = [1, 2]\nsort(a)\nsort(b)\nsort(c)\nsort(d)\nprint(a, b, c, d)\nprint(sort([]), sort([1]))\n")
	add("basic/stability", "l = [[1, \"b\"], [0, \"z\"], [1, \"a\"]]\nsort(l)\nprint(l)\nm = [3, 3.0, 2, 3, 2.0, 1, 3.0]\nsort(m)\nprint(map(m, typeof), m)\nxs = []\nfor (i in range(60)):\n    xs = xs + [i % 3 + 0.0 * i]\nend\nsort(xs)\nprint(xs)\n")
	add("basic/aliasing", "a = [3, 1, 2]\nb = a\nsort(b)\nprint(a, b)\nc = [a, a]\nsort(c)\nprint(c)\nd = a + []\nreverse(d)\nsort(a)\nprint(a, d)\n")
	add("basic/keeps_header", "l = [3, 1, 2]\nfor (x in l):\n    sort(l)\n    print(x, l)\nend\nprint(l)\n")
	add("basic/after_pop_and_shift", "l = [5, 4, 3, 2, 1]\npop(l)\nshift(l)\nsort(l)\nprint(l, len(l))\nappend(l, 0)\nsort(l)\nprint(l)\nunshift(l, 9)\nsort(l)\nprint(l)\n")

	// Kinds that do not compare, alone and inside lists.
	for i, e := range []string{
		"[1, \"a\"]", "[\"a\", 1]", "[true, false]", "[null, 1]", "[1, null]", "[{a: 1}, {a: 2}]", "[f, f]", "[print, len]", "[[1], 1]", "[1, [1]]", "[[1, \"a\"], [1, 2]]", "[[1, f], [1, f]]",
		"[[1], \"a\"]", "[{}, []]", "[[], {}]", "[nan, \"a\"]", "[1, 2, \"a\", 3]", "[\"a\", \"b\", 1]", "[[1, 2], [1, true]]", "[[true], [false]]", "[[[1]], [[\"a\"]]]",
		"[\"a\", \"b\", \"c\", null]", "[1.5, \"1.5\"]", "[f]", "[true]", "[null]",
	} {
		add(fmt.Sprintf("kinds/%02d", i), sortPrelude+fmt.Sprintf("l = %s\n", e)+tryStmt("sort(l)\n    print(l)")+"print(l)\n")
	}

	// NaN and infinity: the order of a relation that is not total.
	nan := []string{"nan", "1", "2", "0.5", "inf", "-inf"}
	for n := 2; n <= 12; n++ {
		for _, at := range []int{0, 1, n / 2, n - 1} {
			if at >= n {
				continue
			}
			vals := make([]string, n)
			for i, p := range perm(n) {
				vals[i] = nan[1+p%5]
			}
			vals[at] = "nan"
			add(fmt.Sprintf("nan/n%d_at%d", n, at), sortPrelude+fmt.Sprintf("l = %s\nsort(l)\nprint(str(l))\n", listSrc(vals)))
		}
	}
	add("nan/two_nans", sortPrelude+"l = [nan, nan, 1, nan, 0]\nsort(l)\nprint(str(l))\nm = [1, nan, nan, 0]\nsort(m)\nprint(str(m))\n")
	add("nan/all_nan", sortPrelude+"l = [nan, nan, nan, nan]\nsort(l)\nprint(str(l))\n")
	add("nan/infinities", sortPrelude+"l = [inf, 1, -inf, 0, inf, -inf, 5]\nsort(l)\nprint(str(l))\n")
	add("nan/in_lists", sortPrelude+"l = [[nan], [1], [nan, 1], [0, nan], [nan, nan]]\nsort(l)\nprint(str(l))\nm = [[1, nan], [1, 0], [1, nan]]\nsort(m)\nprint(str(m))\n")
	for _, n := range []int{25, 40, 41, 60} {
		vals := make([]string, n)
		for i, p := range perm(n) {
			if p%9 == 4 {
				vals[i] = "nan"
			} else {
				vals[i] = fmt.Sprint(p)
			}
		}
		add(fmt.Sprintf("nan/scattered_%d", n), sortPrelude+fmt.Sprintf("l = %s\nsort(l)\nprint(str(l))\n", listSrc(vals)))
	}

	// A comparison that fails: the list keeps the order the sort had reached. A string at each position of lists
	// of 25 and 45 numbers, in three orders, and a number among strings.
	for _, n := range []int{25, 45} {
		orders := map[string][]int{"asc": nil, "desc": nil, "shuffled": perm(n)}
		for i := 0; i < n; i++ {
			orders["asc"] = append(orders["asc"], i)
			orders["desc"] = append(orders["desc"], n-1-i)
		}
		for _, name := range []string{"asc", "desc", "shuffled"} {
			var b strings.Builder
			for p := 0; p < n; p++ {
				vals := make([]string, n)
				for i, v := range orders[name] {
					vals[i] = fmt.Sprint(v)
				}
				vals[p] = "\"s\""
				b.WriteString(fmt.Sprintf("l = %s\n", listSrc(vals)))
				b.WriteString(tryStmt("sort(l)\n    print(\"sorted\")"))
				b.WriteString("print(str(l))\n")
			}
			add(fmt.Sprintf("failure/string_at_each_position_%s_%d", name, n), b.String())
			var c strings.Builder
			for p := 0; p < n; p++ {
				vals := make([]string, n)
				for i, v := range orders[name] {
					vals[i] = fmt.Sprintf("\"k%02d\"", v)
				}
				vals[p] = "7"
				c.WriteString(fmt.Sprintf("l = %s\n", listSrc(vals)))
				c.WriteString(tryStmt("sort(l)\n    print(\"sorted\")"))
				c.WriteString("print(str(l))\n")
			}
			add(fmt.Sprintf("failure/number_among_strings_%s_%d", name, n), c.String())
		}
	}
	for _, n := range []int{21, 22, 40, 41, 100} {
		var b strings.Builder
		for _, p := range []int{0, 1, n / 3, n / 2, n - 2, n - 1} {
			vals := make([]string, n)
			for i, v := range perm(n) {
				vals[i] = fmt.Sprint(v)
			}
			vals[p] = "null"
			vals[(p+n/2)%n] = "true"
			b.WriteString(fmt.Sprintf("l = %s\n", listSrc(vals)))
			b.WriteString(tryStmt("sort(l)"))
			b.WriteString("print(str(l))\n")
		}
		add(fmt.Sprintf("failure/two_bad_values_%d", n), b.String())
	}
	add("failure/shared_list_observed", "l = [3, 1, 2, \"x\", 0]\nm = l\ntry:\n    sort(l)\ncatch (e):\n    print(e)\nend\nprint(str(m), str(l))\nfor (x in m):\n    print(x)\nend\n")
	add("failure/uncaught", "l = [5, 4, 3, \"x\", 2]\nsort(l)\nprint(\"unreachable\")\n")
	add("failure/nested_lists_error", "l = [[1, \"a\"], [1, 2], [0, 1]]\ntry:\n    sort(l)\ncatch (e):\n    print(e)\nend\nprint(str(l))\n")
	add("failure/error_position", "l = [1,\n  \"a\"]\ntry:\n    sort(\n        l)\ncatch (e):\n    print(e)\nend\n")

	// Every length, in a few shapes: the algorithm switches at blocks of 20 and then merges by doubling.
	for n := 0; n <= 130; n++ {
		if n > 70 && n%3 != 0 && n != 128 && n != 129 && n != 130 {
			continue
		}
		p := perm(n)
		var b strings.Builder
		b.WriteString(fmt.Sprintf("a = %s\nsort(a)\nprint(str(a))\n", intsSrc(p)))
		rev := make([]int, n)
		for i := range rev {
			rev[i] = n - 1 - i
		}
		b.WriteString(fmt.Sprintf("b = %s\nsort(b)\nprint(str(b))\n", intsSrc(rev)))
		ties := make([]string, n)
		for i, v := range p {
			ties[i] = fmt.Sprintf("%d.0", v%4)
			if i%2 == 0 {
				ties[i] = fmt.Sprint(v % 4)
			}
		}
		b.WriteString(fmt.Sprintf("c = %s\nsort(c)\nprint(map(c, typeof), str(c))\n", listSrc(ties)))
		add(fmt.Sprintf("sizes/n%03d", n), b.String())
	}
	for _, n := range []int{200, 500, 1000, 2000} {
		add(fmt.Sprintf("sizes/large_%d", n), fmt.Sprintf("l = []\nfor (i in range(%d)):\n    append(l, (i * 7919) %% %d)\nend\nsort(l)\nprint(l[0], l[%d], l[%d], len(l))\nok = true\nfor (i in range(%d)):\n    if (l[i] != i) then:\n        ok = false\n    end\nend\nprint(ok)\n", n, n+1, n/2, n-1, n-1))
	}
	add("sizes/large_stability_pairs", "l = []\nfor (i in range(400)):\n    append(l, [i % 7, i])\nend\nsort(l)\nprint(l[0], l[1], l[57], l[58], l[399])\nm = []\nfor (i in range(300)):\n    append(m, (i % 5) + 0.0)\n    append(m, i % 5)\nend\nsort(m)\nkinds = \"\"\nfor (x in m):\n    kinds = kinds + slice(typeof(x), 0, 1)\nend\nprint(kinds)\n")

	// Cycles and depth.
	add("depth/self_contained", "a = [0, 1]\na[0] = a\nb = [0]\nb[0] = b\nsort([a, b])\n")
	add("depth/self_contained_caught", "a = [0, 1]\na[0] = a\ntry:\n    sort(a)\ncatch (e):\n    print(e)\nend\nprint(\"after\")\n")
	add("depth/two_lists_each_other", "a = [1]\nb = [2, a]\na[0] = b\ntry:\n    sort([a, b])\ncatch (e):\n    print(e)\nend\n")
	nest := func(d int) string {
		return strings.Repeat("[", d) + "1" + strings.Repeat("]", d)
	}
	for _, d := range []int{3, 20, 100, 254, 255, 256, 257, 300} {
		add(fmt.Sprintf("depth/nested_%d", d), fmt.Sprintf("a = %s\nb = %s\nl = [a, b]\nsort(l)\nprint(len(l))\n", nest(d), nest(d)))
		add(fmt.Sprintf("depth/nested_one_%d", d), fmt.Sprintf("a = %s\nl = [a, [0]]\nsort(l)\nprint(len(l))\n", nest(d)))
	}
	add("depth/shared_halves", "l = [0]\nfor (i in range(14)):\n    l = [l, l]\nend\nm = [l, l]\nsort(m)\nprint(len(m))\n")
	add("depth/shared_halves_too_many", "l = [0]\nfor (i in range(20)):\n    l = [l, l]\nend\nm = [l, l]\nsort(m)\nprint(len(m))\n")

	// Operations: every element of the list is a node of the walk that precedes the sort.
	for _, n := range []int{50000, 90000, 99980, 99981, 99982, 99983, 99984, 99985, 99986, 99987, 99988, 99989, 99990, 99995, 99996, 99997, 99998, 99999, 100000, 100001} {
		add(fmt.Sprintf("ops/sort_range_%d", n), fmt.Sprintf("l = range(%d)\nsort(l)\nprint(len(l))\n", n))
	}
	for _, n := range []int{1000, 5000, 6000, 7000, 7600, 7650, 7690, 7691, 7692, 7693, 7694, 7695, 7696, 7697, 7698, 7699, 7700, 7750, 7800, 9000, 10000, 20000} {
		add(fmt.Sprintf("ops/sort_nested_pairs_%d", n), fmt.Sprintf("l = []\nfor (i in range(%d)):\n    append(l, [i %% 10, i])\nend\nsort(l)\nprint(len(l))\n", n))
	}

	// What it charges: nothing, so the budget left is the same.
	for _, k := range []int{0, 1, 2, 20, 21, 100, 1000, 20000} {
		add(fmt.Sprintf("ledger/sort_%d", k), ledger(fmt.Sprintf("l = range(%d)\nreverse(l)\nsort(l)\n", k), rangeSpent(k)))
		if k <= 1000 {
			add(fmt.Sprintf("ledger/sort_strings_%d", k), ledger(fmt.Sprintf("l = map(range(%d), str)\nsort(l)\n", k), rangeSpent(k)+24+16*k+(16+3)*k))
		}
	}
	for _, k := range []int{2, 20, 21, 40, 41, 100} {
		edgeCases(add, fmt.Sprintf("sort_%d", k), fmt.Sprintf("l = range(%d)\nreverse(l)\n", k), rangeSpent(k), "sort(l)", 0)
	}
	add("ledger/sort_then_append", ledger("l = range(40)\nsort(l)\nappend(l, 0)\n", 24+24*40+24+16*goCap(40, 41)))
	return cs
}

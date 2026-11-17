package main

import (
	"fmt"
	"strings"
)

func init() {
	libRegistry = append(libRegistry, handGroup("lib-paths", pathsCases()))
}

func pathsCases() []handCase {
	var cs []handCase
	add := func(name, src string) { cs = append(cs, handCase{name, nil, pureUni(src)}) }
	addAll := func(more []handCase) { cs = append(cs, more...) }

	shapes := pureLits("", ".", "..", "...", "/", "//", "///", "a", "a/", "a//", "//a//b//", "a//b", "/a", "/a/b/c.txt", "a/./b", "a/../b",
		"../a", "../../a/b", "/../a", "/a/../..", "a/b/../../..", "a/b/..", "a/b/../c/./d//e/", "./", "./a", "a/.", "a/..", "a/../..",
		".hidden", ".hidden/", "a.b.c", "a/b.c/d", "a/b.c/", "file.", "/file", "x/y/z.tar.gz", ".bashrc", "dir/.bashrc", "a b/c d.txt",
		"é/日本.語", "日本語/ファイル.txt", "\\", "a\\b.c", "a/b\\c.d", "a\tb/c\nd.e", "😀/😀.😀", "  /  ", " ./ ", "a/b/c/../../../..", "/a/b/../../../..",
		"foo.bar/baz", "foo.bar/", "/.", "/..", "/./.", "./.", "../..", "..a", "a..", ".../a", "a/...", "a/.b/.c", "//./..//a")
	bad := []string{"lead", "cont", "cut3", "sur", "mixed", `"a/" + lead`, `lead + "/b.c"`, `"x" + cont + "." + lead`, `mixed + "/" + mixed`}
	all := append(append([]string{}, shapes...), bad...)

	for _, fn := range []string{"path_basename", "path_dirname", "path_ext"} {
		var calls []string
		for _, a := range all {
			calls = append(calls, fn+"("+a+")")
		}
		for _, a := range append(append([]string{}, pureOther...), pureNumbers...) {
			calls = append(calls, fn+"("+a+")")
		}
		calls = append(calls, fn+"()", fn+`("a", "b")`, fn+`("a", "b", "c")`)
		addAll(purePack("one/"+fn, pureInvalid, calls, 50))
	}

	// path_join with one argument, then two over a smaller set.
	var joins []string
	for _, a := range all {
		joins = append(joins, "path_join("+a+")")
	}
	addAll(purePack("join/one", pureInvalid, joins, 50))
	small := pureLits("", ".", "..", "/", "a", "a/", "/a", "b/c", "../b", "./b", "a/..", "//", "x.y")
	small = append(small, "lead", `"é"`)
	addAll(purePack("join/two", pureInvalid, pureCalls("path_join", small, small), 50))
	var three []string
	for _, a := range []string{`""`, `"a"`, `"/a"`, `"a/.."`} {
		for _, b := range []string{`""`, `"b"`, `".."`, `"b/"`, `"/"`} {
			for _, c := range []string{`""`, `"c"`, `".."`, `"../.."`, `"/c"`, `"c/"`} {
				three = append(three, fmt.Sprintf("path_join(%s, %s, %s)", a, b, c))
			}
		}
	}
	three = append(three, `path_join("", "", "")`, `path_join("", "", "", "x")`, `path_join("a", "", "", "b")`, `path_join("a", "b", "c", "d", "e", "f")`,
		`path_join("a/b", "../..", "../c")`, `path_join("/", "..", "..")`, `path_join("é", "日本", "😀.txt")`, `path_join(lead, cont)`, `path_join("a", mixed, "b")`)
	addAll(purePack("join/three", pureInvalid, three, 50))
	var wrong []string
	for _, v := range append(append([]string{}, pureOther...), pureNumbers...) {
		wrong = append(wrong, "path_join("+v+")", `path_join("a", `+v+`)`, "path_join("+v+`, "a")`, `path_join("a", "b", `+v+`)`)
	}
	wrong = append(wrong, "path_join()", `path_join("a", 1, 2)`, `path_join(1, "a", "b")`)
	addAll(purePack("join/wrong", pureInvalid, wrong, 50))

	// The result as a value: an error from a builtin can be printed, kept and tested.
	add("errors/as_values", pureInvalid+`x = path_join()
y = path_join(1)
z = path_basename(null)
print(x)
print(y)
print(z)
print(typeof(x), typeof(y), len(str(z)))
l = [x, y, z]
print(l)
print(path_ext(x), path_basename(path_join("a", "b")))
`)
	add("errors/if_raises", pureInvalid+"x = path_join()\nif (x) then:\n    print(1)\nend\nprint(2)\n")
	add("errors/while_raises", pureInvalid+"x = path_dirname(5)\nwhile (x):\n    print(1)\nend\nprint(2)\n")
	add("errors/caught_as_text", pureInvalid+"x = path_ext([])\ntry:\n    if (x) then:\n    end\ncatch (e):\n    print(e)\nend\n")

	// The result shares the bytes of an argument when Go does, so it costs a header and no bytes.
	long := "x = \"a/\" * 50000\n"
	add("memory/basename_shares", pureInvalid+long+"i = 0\nwhile (i < 5000):\n    t = path_basename(x)\n    i += 1\nend\nprint(i, len(t))\n")
	add("memory/basename_suffix_shares", pureInvalid+"x = \"a/\" * 50000 + \"b\"\ni = 0\nwhile (i < 5000):\n    t = path_basename(x)\n    i += 1\nend\nprint(i, t)\n")
	add("memory/ext_shares", pureInvalid+"x = \"a\" * 50000 + \".txt\"\ni = 0\nwhile (i < 5000):\n    t = path_ext(x)\n    i += 1\nend\nprint(i, t)\n")
	add("memory/dirname_prefix_shares", pureInvalid+"x = \"a/\" * 40000 + \"b\"\ni = 0\nwhile (i < 5000):\n    t = path_dirname(x)\n    i += 1\nend\nprint(i, len(t))\n")
	add("memory/dirname_cleaned_costs_bytes", pureInvalid+"x = \"a//\" * 30000 + \"b\"\ni = 0\nwhile (i < 100):\n    t = path_dirname(x)\n    print(i)\n    i += 1\nend\n")
	add("memory/dirname_dotdot_costs_bytes", pureInvalid+"x = \"a/../\" * 20000 + \"b/c\"\ni = 0\nwhile (i < 100):\n    t = path_dirname(x)\n    print(i)\n    i += 1\nend\n")
	add("memory/join_single_shares", pureInvalid+"x = \"a/\" * 40000\ni = 0\nwhile (i < 5000):\n    t = path_join(x)\n    i += 1\nend\nprint(i, len(t))\n")
	add("memory/join_single_after_empty_shares", pureInvalid+"x = \"a/\" * 40000\ni = 0\nwhile (i < 5000):\n    t = path_join(\"\", x)\n    i += 1\nend\nprint(i, len(t))\n")
	add("memory/join_two_costs_bytes", pureInvalid+"x = \"a/\" * 20000\ni = 0\nwhile (i < 100):\n    t = path_join(x, \"b\")\n    print(i)\n    i += 1\nend\n")
	add("memory/join_cleaned_costs_bytes", pureInvalid+"x = \"a//\" * 20000\ni = 0\nwhile (i < 100):\n    t = path_join(x)\n    print(i)\n    i += 1\nend\n")
	add("memory/dot_results_cost_a_byte_each", pureInvalid+"i = 0\nwhile (i < 3000):\n    t = path_basename(\"\")\n    u = path_dirname(\"\")\n    i += 1\nend\nprint(i, t, u)\n")
	add("memory/slash_results", pureInvalid+"i = 0\nwhile (i < 3000):\n    t = path_basename(\"///\")\n    u = path_dirname(\"///\")\n    i += 1\nend\nprint(i, t, u)\n")
	add("memory/error_values_cost_their_text", pureInvalid+"i = 0\nwhile (i < 4000):\n    t = path_join()\n    i += 1\nend\nprint(i)\n")
	add("memory/error_value_kept_in_a_list", pureInvalid+"l = []\ni = 0\nwhile (i < 20000):\n    l = l + [path_dirname(i)]\n    i += 1\nend\nprint(i)\n")
	// Repeated results over many shapes: how many fit in the budget shows what each costs.
	var b strings.Builder
	b.WriteString(pureInvalid + "n = 0\nwhile (n < 500):\n")
	for _, s := range []string{`"a/b/c.txt"`, `"/x//y/./z/../w"`, `"a/../../b"`, `""`, `"."`, `"/"`, `"../a"`, `"a/b/"`} {
		fmt.Fprintf(&b, "    t = path_dirname(%s)\n    t = path_basename(%s)\n    t = path_ext(%s)\n    t = path_join(%s, \"q\")\n", s, s, s, s)
	}
	b.WriteString("    n += 1\n    print(n)\nend\n")
	add("memory/many_shapes", b.String())
	return cs
}

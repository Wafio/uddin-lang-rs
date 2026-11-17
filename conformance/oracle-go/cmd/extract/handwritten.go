package main

import (
	"fmt"
	"strings"

	"oracle-go/internal/oracle"
)

// The groups below are written by hand: no Go test of the fork or of the control plane runs a script with a
// request context, or a script that reaches the nesting limit or the deadline, in a form the extractor can fold.
// fn_wafio_test.go calls the waf_* builtins directly from Go, and the engine tests build their contexts and
// scripts in helper functions.

type handCase struct {
	name string
	ctx  *oracle.Ctx
	src  string
}

func handGroup(name string, cases []handCase) *group {
	g := &group{name: name}
	for _, c := range cases {
		g.cases = append(g.cases, oracle.NewCase(name+"/"+c.name, []byte(c.src), c.ctx))
	}
	return g
}

func handwritten() []*group {
	groups := append([]*group{handGroup("waf-ctx", wafCtxCases()), handGroup("limits", limitCases()), handGroup("timing", timingCases())}, coreGroups()...)
	return append(groups, libRegistry...)
}

// libRegistry holds the groups that exercise the builtins of the library, one per file named lib_<group>.go: each
// file adds its own group in an init function, so adding a group touches no other file.
var libRegistry []*group

// requestCtx is the context of the control plane's script tests: a GET with a query, two headers, a JSON body,
// a score, an action and one detected category.
func requestCtx() *oracle.Ctx {
	return &oracle.Ctx{
		Method: "GET", Path: "/admin/settings", Query: "debug=1&user=admin", IP: "185.220.101.45", Country: "US",
		UA:      "curl/7.68",
		Headers: map[string]string{"x-forwarded-for": "185.220.101.45", "content-type": "application/json"},
		Body:    `{"user":"admin","pass":"secret"}`, Score: 3.5, Action: "LOG", Detected: []string{"sql"},
	}
}

func with(c *oracle.Ctx, edit func(*oracle.Ctx)) *oracle.Ctx {
	n := *c
	edit(&n)
	return &n
}

func queryCtx(q string) *oracle.Ctx { return &oracle.Ctx{Query: q} }
func pathCtx(p string) *oracle.Ctx  { return &oracle.Ctx{Path: p} }
func detectedCtx(d ...string) *oracle.Ctx {
	return &oracle.Ctx{Detected: d}
}
func headerCtx(kv ...string) *oracle.Ctx {
	h := map[string]string{}
	for i := 0; i < len(kv); i += 2 {
		h[kv[i]] = kv[i+1]
	}
	return &oracle.Ctx{Headers: h}
}
func jsonCtx(doc string) *oracle.Ctx { return &oracle.Ctx{JSON: []byte(doc)} }

// guard runs expr inside try/catch and prints the error text, so that the message a builtin raises is part of
// the result whether it raises or returns an error value.
func guard(expr string) string {
	return "try:\n    print(" + expr + ")\ncatch (e):\n    print(\"caught: \" + str(e))\nend\n"
}

func wafCtxCases() []handCase {
	req := requestCtx()
	var cs []handCase
	add := func(name string, ctx *oracle.Ctx, src string) { cs = append(cs, handCase{name, ctx, src}) }

	// The variables of the request.
	add("vars/all", req, "print(method)\nprint(path)\nprint(query)\nprint(ip)\nprint(country)\nprint(ua)\nprint(body)\nprint(score)\nprint(action)\n")
	add("vars/types", req, "print(typeof(method))\nprint(typeof(score))\nprint(typeof(body))\nprint(typeof(_waf_ctx))\n")
	add("vars/empty_context", nil, "print(method)\nprint(path)\nprint(score)\nprint(action)\nprint(typeof(score))\nprint(waf_score())\nprint(len(body))\n")
	add("vars/body_4kib", &oracle.Ctx{Body: strings.Repeat("a", 4090) + "needle"},
		"print(len(body))\nprint(waf_body_contains(\"needle\"))\nprint(waf_body_contains(\"needlf\"))\n")
	add("vars/unicode", &oracle.Ctx{Path: "/café/日本", UA: "\U0001F600 bot", Query: "q=ü"},
		"print(len(path))\nprint(upper(ua))\nprint(waf_path_match(\"/café/*\"))\nprint(waf_query(\"q\"))\n")
	add("vars/assignment_stays_in_the_run", req, "method = \"POST\"\nscore = 9\nprint(method)\nprint(waf_score())\nprint(_waf_ctx != null)\n")

	// _waf_ctx is not a script map: only the builtins read it.
	m := &oracle.Ctx{Method: "POST"}
	add("ctx/subscript", m, "print(_waf_ctx[\"method\"])\n")
	add("ctx/subscript_caught", m, guard(`_waf_ctx["method"]`))
	add("ctx/len", m, "print(len(_waf_ctx))\n")
	add("ctx/len_caught", m, guard(`len(_waf_ctx)`))
	add("ctx/for_in", m, "for (k in _waf_ctx):\n    print(k)\nend\n")
	add("ctx/typeof", m, "print(typeof(_waf_ctx))\n")
	add("ctx/str", m, guard(`str(_waf_ctx)`))
	add("ctx/index_assignment", m, "try:\n    _waf_ctx[\"method\"] = \"GET\"\n    print(\"assigned\")\ncatch (e):\n    print(\"caught: \" + str(e))\nend\n")
	// The builtins find the context through the variable of that name, so a script can replace it.
	add("ctx/replaced_at_top_level", req, "_waf_ctx = {}\nprint(waf_header(\"content-type\"))\nprint(waf_score())\nprint(waf_action())\nprint(waf_path_match(\"/admin/*\"))\nprint(waf_detected_any())\n")
	add("ctx/replaced_by_a_number", req, "_waf_ctx = 5\nprint(waf_score())\nprint(waf_body_contains(\"\"))\nprint(waf_detected_list())\n")
	add("ctx/replaced_in_a_function", req, "fun f():\n    _waf_ctx = {}\n    return waf_score()\nend\nprint(f())\nprint(waf_score())\n")

	// waf_header
	add("header/exists", headerCtx("content-type", "application/json"), "print(waf_header(\"content-type\"))\n")
	add("header/name_case_folded", headerCtx("x-forwarded-for", "1.2.3.4"), "print(waf_header(\"X-Forwarded-For\"))\nprint(waf_header(\"x-FORWARDED-for\"))\n")
	add("header/key_case_folded", headerCtx("X-Api-Key", "k1"), "print(waf_header(\"x-api-key\"))\nprint(waf_header(\"X-API-KEY\"))\n")
	add("header/missing", headerCtx(), "print(waf_header(\"authorization\"))\nprint(typeof(waf_header(\"authorization\")))\n")
	add("header/no_headers", nil, "print(waf_header(\"content-type\"))\n")
	add("header/empty_name", headerCtx("a", "b"), "print(waf_header(\"\"))\n")
	add("header/empty_value", headerCtx("x-empty", ""), "print(waf_header(\"x-empty\"))\nprint(typeof(waf_header(\"x-empty\")))\nprint(waf_header(\"x-empty\") == \"\")\nprint(waf_header(\"x-none\") == null)\n")
	add("header/unicode_folding", headerCtx("X-Ünï", "v1", "X-K", "v2"), "print(waf_header(\"x-ünï\"))\nprint(waf_header(\"x-k\"))\nprint(waf_header(\"X-K\"))\n")
	add("header/value_kept_as_sent", headerCtx("x-a", "  Mixed Case \t"), "print(\"[\" + waf_header(\"x-a\") + \"]\")\n")
	add("header/not_a_string", req, guard(`waf_header(1)`))
	add("header/no_argument", req, guard(`waf_header()`))
	add("header/two_arguments", req, guard(`waf_header("a", "b")`))
	add("header/null_argument", req, guard(`waf_header(null)`))
	add("header/via_variable", req, "name = \"Content-Type\"\nh = waf_header(name)\nprint(lower(h))\nprint(contains(h, \"json\"))\n")

	// waf_query
	add("query/exists", queryCtx("a=1&b=2"), "print(waf_query(\"a\"))\n")
	add("query/second_param", queryCtx("a=1&b=2"), "print(waf_query(\"b\"))\n")
	add("query/missing", queryCtx("a=1&b=2"), "print(waf_query(\"c\"))\nprint(typeof(waf_query(\"c\")))\n")
	add("query/empty_query", queryCtx(""), "print(waf_query(\"a\"))\nprint(waf_query(\"\"))\n")
	add("query/unescaped", queryCtx("a%20b=c%2Bd&e=%E2%82%AC"), "print(waf_query(\"a b\"))\nprint(waf_query(\"e\"))\nprint(waf_query(\"a%20b\"))\n")
	add("query/plus_is_a_space", queryCtx("a+b=c+d%2B"), "print(waf_query(\"a b\"))\nprint(waf_query(\"a+b\"))\n")
	add("query/first_of_repeated", queryCtx("e=f&e=g"), "print(waf_query(\"e\"))\n")
	add("query/pair_without_equals", queryCtx("flag&x=1&flag2"), "print(waf_query(\"flag\"))\nprint(waf_query(\"x\"))\nprint(waf_query(\"flag2\"))\n")
	add("query/empty_value", queryCtx("k=&j=1"), "print(\"[\" + waf_query(\"k\") + \"]\")\nprint(typeof(waf_query(\"k\")))\n")
	add("query/value_with_equals", queryCtx("k=a=b=c"), "print(waf_query(\"k\"))\n")
	add("query/bad_escape_in_value", queryCtx("bad=%zz&ok=1"), "print(\"[\" + waf_query(\"bad\") + \"]\")\nprint(typeof(waf_query(\"bad\")))\nprint(waf_query(\"ok\"))\n")
	add("query/truncated_escape", queryCtx("a=%4&b=%"), "print(\"[\" + waf_query(\"a\") + \"]\")\nprint(\"[\" + waf_query(\"b\") + \"]\")\n")
	add("query/bad_escape_in_key", queryCtx("%zz=1&=2"), "print(waf_query(\"\"))\nprint(waf_query(\"%zz\"))\n")
	add("query/empty_key", queryCtx("=x&a=1"), "print(waf_query(\"\"))\nprint(waf_query(\"a\"))\n")
	add("query/semicolon_is_not_a_separator", queryCtx("a=1;b=2&c=3"), "print(waf_query(\"a\"))\nprint(waf_query(\"b\"))\nprint(waf_query(\"c\"))\n")
	add("query/invalid_utf8_after_unescape", queryCtx("n=%ff%fe&m=a"), "v = waf_query(\"n\")\nprint(len(v))\nprint(waf_query(\"m\"))\n")
	add("query/question_mark_is_part_of_the_key", queryCtx("?a=1&b=2"), "print(waf_query(\"a\"))\nprint(waf_query(\"?a\"))\n")
	add("query/trailing_ampersand", queryCtx("a=1&"), "print(waf_query(\"a\"))\nprint(waf_query(\"\"))\n")
	add("query/not_a_string", req, guard(`waf_query(1)`))
	add("query/no_argument", req, guard(`waf_query()`))
	add("query/variable_and_builtin_agree", req, "print(query)\nprint(waf_query(\"debug\"))\nprint(waf_query(\"user\"))\n")

	// waf_body_contains
	add("body/found", &oracle.Ctx{Body: "hello secret world"}, "print(waf_body_contains(\"secret\"))\n")
	add("body/not_found", &oracle.Ctx{Body: "hello world"}, "print(waf_body_contains(\"secret\"))\n")
	add("body/empty_body", nil, "print(waf_body_contains(\"x\"))\nprint(waf_body_contains(\"\"))\n")
	add("body/case_sensitive", &oracle.Ctx{Body: "Secret"}, "print(waf_body_contains(\"secret\"))\nprint(waf_body_contains(\"Secret\"))\n")
	add("body/multibyte", &oracle.Ctx{Body: "café 日本"}, "print(waf_body_contains(\"é 日\"))\nprint(waf_body_contains(\"è\"))\n")
	add("body/not_a_string", req, guard(`waf_body_contains(1)`))
	add("body/no_argument", req, guard(`waf_body_contains()`))

	// waf_path_match and waf_cidr_match
	for _, pc := range []struct{ name, path, pattern string }{
		{"exact", "/admin/settings", "/admin/settings"},
		{"wildcard_segment", "/admin/settings", "/admin/*"},
		{"wildcard_does_not_cross_a_slash", "/admin/a/b", "/admin/*"},
		{"non_matching", "/api/users", "/admin/*"},
		{"double_wildcard", "/admin/a/b", "/admin/**"},
		{"double_wildcard_one_segment", "/admin/a", "/admin/**"},
		{"question_mark", "/a/b", "/a/?"},
		{"class", "/a/c", "/a/[bc]"},
		{"negated_class", "/a/d", "/a/[^bc]"},
		{"range", "/a/5", "/a/[0-9]"},
		{"escaped_star", "/a/*", "/a/\\*"},
		{"invalid_pattern", "/foo", "[invalid"},
		{"unterminated_class", "/a/[", "/a/["},
		{"trailing_backslash", "/a/b", "/a/b\\"},
		{"empty_both", "", ""},
		{"empty_path", "", "*"},
		{"empty_pattern", "/a", ""},
		{"unicode", "/café/日", "/caf?/*"},
		{"case_sensitive", "/Admin", "/admin"},
	} {
		add("path_match/"+pc.name, pathCtx(pc.path), fmt.Sprintf("print(waf_path_match(%q))\n", pc.pattern))
	}
	add("path_match/no_ctx", nil, "print(waf_path_match(\"*\"))\n")
	add("path_match/not_a_string", req, guard(`waf_path_match(1)`))
	add("path_match/no_argument", req, guard(`waf_path_match()`))

	for _, cc := range []struct{ name, ip, cidr string }{
		{"ipv4_in_range", "10.0.0.5", "10.0.0.0/8"},
		{"ipv4_out_of_range", "192.168.1.1", "10.0.0.0/8"},
		{"invalid_ip", "not-an-ip", "10.0.0.0/8"},
		{"invalid_cidr", "10.0.0.5", "not-a-cidr"},
		{"cidr_without_mask", "10.0.0.5", "10.0.0.5"},
		{"tor_exit_node", "185.220.101.45", "185.220.101.0/24"},
		{"ipv6_in_range", "2001:db8::1", "2001:db8::/32"},
		{"ipv6_out_of_range", "2001:db9::1", "2001:db8::/32"},
		{"ipv4_mapped_in_ipv4_range", "::ffff:10.0.0.5", "10.0.0.0/8"},
		{"ipv4_in_ipv6_range", "10.0.0.5", "2001:db8::/32"},
		{"host_bits_set_in_cidr", "10.1.2.3", "10.1.2.3/8"},
		{"slash_32", "10.0.0.5", "10.0.0.5/32"},
		{"slash_0", "203.0.113.9", "0.0.0.0/0"},
		{"leading_zeros", "010.0.0.5", "10.0.0.0/8"},
		{"empty_ip", "", "10.0.0.0/8"},
		{"empty_cidr", "10.0.0.5", ""},
		{"ipv6_zone", "fe80::1%eth0", "fe80::/10"},
		{"mask_too_long", "10.0.0.5", "10.0.0.0/33"},
	} {
		add("cidr_match/"+cc.name, nil, fmt.Sprintf("print(waf_cidr_match(%q, %q))\n", cc.ip, cc.cidr))
	}
	add("cidr_match/request_ip", req, "print(waf_cidr_match(ip, \"185.220.0.0/16\"))\nprint(waf_cidr_match(ip, \"10.0.0.0/8\"))\n")
	add("cidr_match/not_a_string", req, guard(`waf_cidr_match(1, "10.0.0.0/8")`))
	add("cidr_match/second_not_a_string", req, guard(`waf_cidr_match("10.0.0.1", 8)`))
	add("cidr_match/one_argument", req, guard(`waf_cidr_match("10.0.0.1")`))

	// waf_score and waf_action
	add("score/float", &oracle.Ctx{Score: 3.5}, "print(waf_score())\nprint(typeof(waf_score()))\nprint(score == waf_score())\n")
	add("score/default", nil, "print(waf_score())\nprint(typeof(waf_score()))\nprint(waf_score() == 0)\n")
	add("score/whole_number", &oracle.Ctx{Score: 7}, "print(waf_score())\nprint(str(waf_score()))\nprint(waf_score() == 7)\nprint(typeof(waf_score()))\n")
	add("score/negative_and_small", &oracle.Ctx{Score: -0.000001}, "print(waf_score())\nprint(waf_score() < 0)\n")
	add("score/large", &oracle.Ctx{Score: 1e21}, "print(waf_score())\nprint(str(waf_score()))\n")
	add("score/arithmetic", &oracle.Ctx{Score: 2.5}, "print(waf_score() * 2)\nprint(waf_score() + 1)\nprint(waf_score() / 2)\n")
	add("action/string", &oracle.Ctx{Action: "BLOCK"}, "print(waf_action())\nprint(action == waf_action())\n")
	add("action/default", nil, "print(\"[\" + waf_action() + \"]\")\n")
	add("action/case_kept", &oracle.Ctx{Action: "Log"}, "print(waf_action())\nprint(waf_action() == \"LOG\")\n")
	add("score_action/with_arguments", req, guard(`waf_score(1)`)+guard(`waf_action(1)`))

	// waf_detected, waf_detected_any and waf_detected_list
	add("detected/found", detectedCtx("sql", "xss"), "print(waf_detected(\"sql\"))\nprint(waf_detected(\"xss\"))\n")
	add("detected/not_found", detectedCtx("sql"), "print(waf_detected(\"xss\"))\nprint(waf_detected(\"\"))\n")
	add("detected/case_insensitive", detectedCtx("sql", "RCE"), "print(waf_detected(\"SQL\"))\nprint(waf_detected(\"rce\"))\nprint(waf_detected(\"Rce\"))\n")
	add("detected/unicode_folding", detectedCtx("Ärger", "K"), "print(waf_detected(\"ärger\"))\nprint(waf_detected(\"k\"))\n")
	add("detected/none", nil, "print(waf_detected(\"sql\"))\nprint(waf_detected_any())\nprint(waf_detected_list())\nprint(len(waf_detected_list()))\n")
	add("detected/any", detectedCtx("a"), "print(waf_detected_any())\n")
	add("detected/list_keeps_case_and_order", detectedCtx("sql", "xss", "RCE"), "l = waf_detected_list()\nprint(len(l))\nprint(l[0])\nprint(l[1])\nprint(l[2])\n")
	add("detected/duplicates_kept", detectedCtx("sql", "sql"), "print(len(waf_detected_list()))\n")
	add("detected/list_is_a_copy", detectedCtx("sql"), "l = waf_detected_list()\npush(l, \"x\")\nl[0] = \"y\"\nprint(waf_detected_list())\nprint(waf_detected(\"sql\"))\nprint(waf_detected(\"x\"))\n")
	add("detected/not_a_string", req, guard(`waf_detected(1)`))
	add("detected/no_argument", req, guard(`waf_detected()`))
	add("detected/any_with_argument", req, guard(`waf_detected_any(1)`))
	add("detected/list_with_argument", req, guard(`waf_detected_list(1)`))

	// waf_return, waf_block, waf_allow and waf_log
	for _, rc := range []struct{ name, arg string }{
		{"block", `"BLOCK"`}, {"allow", `"ALLOW"`}, {"log", `"LOG"`},
		{"lower_case", `"block"`}, {"mixed_case", `"Allow"`}, {"padded", `"  log\t"`}, {"padded_newline", `"\nBLOCK\n"`},
		{"unicode_case", `"blöck"`}, {"continue", `"CONTINUE"`}, {"deny", `"DENY"`}, {"empty", `""`},
		{"number", `1`}, {"null", `null`}, {"list", `["BLOCK"]`},
	} {
		add("return/"+rc.name, req, "fun check():\n    waf_return("+rc.arg+")\nend\ncheck()\nprint(\"after\")\n")
		add("return/"+rc.name+"_caught", req, "try:\n    fun check():\n        return waf_return("+rc.arg+")\n    end\n    print(check())\ncatch (e):\n    print(\"caught: \" + str(e))\nend\n")
	}
	add("return/no_argument", req, guard(`waf_return()`))
	add("return/two_arguments", req, guard(`waf_return("BLOCK", "x")`))
	add("return/at_top_level", req, "waf_return(\"block\")\nprint(\"after\")\n")
	add("return/top_level_caught", req, "try:\n    waf_return(\"block\")\ncatch (e):\n    print(\"caught: \" + str(e))\nend\nprint(\"after\")\n")
	for _, name := range []string{"block", "allow", "log"} {
		add(name+"/no_message", req, "waf_"+name+"()\n")
		add(name+"/message", req, "waf_"+name+"(\"because\")\n")
		add(name+"/two_messages", req, "waf_"+name+"(1, 2)\nprint(\"after\")\n")
		add(name+"/in_function", req, "fun f():\n    waf_"+name+"()\n    print(\"unreachable in the function\")\nend\nf()\n")
		add(name+"/in_condition", req, "if (waf_path_match(\"/admin/*\") and waf_detected(\"sql\")) then:\n    waf_"+name+"()\nend\n")
	}
	add("verdict/first_wins", req, "fun a():\n    waf_log()\nend\nfun b():\n    waf_block()\nend\na()\nb()\n")
	add("verdict/first_wins_after_print", req, "fun a():\n    println(ua)\n    waf_block()\nend\na()\nwaf_allow()\n")
	add("verdict/print_is_never_a_verdict", with(req, func(c *oracle.Ctx) { c.UA = "allow" }), "println(ua)\nprint(\"BLOCK\")\nprint(\"ALLOW\")\n")
	add("verdict/no_verdict", req, "x = 1\n")
	add("verdict/error_after_verdict_keeps_it", req, "fun a():\n    waf_block()\nend\na()\nx = 1 + \"a\"\n")
	add("verdict/in_loop", req, "fun f(n):\n    i = 0\n    while (i < 10):\n        if (i == n) then:\n            waf_block()\n        end\n        i = i + 1\n    end\nend\nf(3)\nprint(\"done\")\n")
	add("verdict/in_catch_handler", req, "fun f():\n    try:\n        x = 1 + \"a\"\n    catch (e):\n        waf_log()\n    end\nend\nf()\n")
	add("verdict/in_try_body", req, "fun f():\n    try:\n        waf_block()\n    catch (e):\n        print(\"caught\")\n    end\n    print(\"after try\")\nend\nf()\n")

	// waf_field and waf_values: the host's resolver, here a table.
	fv := &oracle.Ctx{
		Fields:      map[string]string{"param:user.email": "a@b.c", "query": "q=1&tag=a", "header:x-api-key": "k1", "empty": "", "json:/é": "ü"},
		FieldValues: map[string][]string{"form:tags": {"a", "b"}, "json:/items/*/sku": {"ok", "<b>", ""}, "one": {"x"}, "none": {}},
	}
	add("field/found", fv, "print(waf_field(\"param:user.email\"))\nprint(waf_field(\"query\"))\nprint(waf_field(\"header:x-api-key\"))\nprint(waf_field(\"json:/é\"))\n")
	add("field/absent_is_an_empty_string", fv, "v = waf_field(\"nosuch\")\nprint(\"[\" + v + \"]\")\nprint(typeof(v))\nprint(waf_field(\"empty\") == \"\")\n")
	add("field/no_resolver_data", nil, "print(\"[\" + waf_field(\"param:x\") + \"]\")\nprint(typeof(waf_field(\"param:x\")))\n")
	add("field/exact_name_only", fv, "print(\"[\" + waf_field(\"QUERY\") + \"]\")\nprint(\"[\" + waf_field(\" query\") + \"]\")\n")
	add("field/not_a_string", fv, guard(`waf_field(1)`))
	add("field/no_argument", fv, guard(`waf_field()`))
	add("values/found", fv, "l = waf_values(\"form:tags\")\nprint(len(l))\nprint(l[0])\nprint(l[1])\nprint(typeof(l))\n")
	add("values/wildcard_with_an_empty_entry", fv, "l = waf_values(\"json:/items/*/sku\")\nprint(len(l))\nprint(l[1])\nprint(\"[\" + l[2] + \"]\")\n")
	add("values/absent_is_an_empty_list", fv, "l = waf_values(\"nosuch\")\nprint(len(l))\nprint(l)\nprint(typeof(l))\nprint(len(waf_values(\"none\")))\n")
	add("values/single", fv, "print(len(waf_values(\"one\")))\nprint(waf_values(\"one\")[0])\n")
	add("values/is_a_fresh_list_each_call", fv, "l = waf_values(\"form:tags\")\npush(l, \"c\")\nl[0] = \"z\"\nprint(waf_values(\"form:tags\"))\nprint(len(l))\n")
	add("values/no_resolver_data", nil, "print(waf_values(\"form:tags\"))\n")
	add("values/in_a_loop", fv, "for (v in waf_values(\"form:tags\")):\n    print(upper(v))\nend\n")
	add("values/not_a_string", fv, guard(`waf_values(1)`))
	add("values/no_argument", fv, guard(`waf_values()`))

	// waf_json: exact integers stay integers.
	add("json/no_body", nil, "print(waf_json())\nprint(typeof(waf_json()))\nprint(waf_json() == null)\n")
	add("json/object", jsonCtx(`{"user":{"name":"admin","Role":"root"},"items":[{"sku":"ok"},{"sku":"<b>"}],"t":true,"f":false,"z":null}`),
		"d = waf_json()\nprint(d[\"user\"][\"name\"])\nprint(d[\"user\"][\"Role\"])\nprint(len(d[\"items\"]))\nprint(d[\"items\"][1][\"sku\"])\nprint(d[\"t\"])\nprint(d[\"f\"])\nprint(d[\"z\"])\nprint(typeof(d))\nprint(len(d))\n")
	add("json/numbers", jsonCtx(`{"i":3,"f":1.5,"neg":-7,"zero":0,"negzero":-0,"exp":1e3,"expf":1.5e2,"whole_float":2.0,"big":12345678901234567890,"maxint":9223372036854775807,"over":9223372036854775808,"minint":-9223372036854775808,"tiny":1e-5,"huge":1e400,"long":0.1234567890123456789}`),
		"d = waf_json()\nfor (k in [\"i\", \"f\", \"neg\", \"zero\", \"negzero\", \"exp\", \"expf\", \"whole_float\", \"big\", \"maxint\", \"over\", \"minint\", \"tiny\", \"huge\", \"long\"]):\n    print(k + \" \" + typeof(d[k]) + \" \" + str(d[k]))\nend\n")
	add("json/number_arithmetic", jsonCtx(`{"items":[{"qty":3,"price":1.5}]}`),
		"it = waf_json()[\"items\"][0]\nprint(it[\"qty\"] * 2)\nprint(it[\"qty\"] / 2)\nprint(it[\"price\"] * it[\"qty\"])\nprint(it[\"qty\"] == 3)\nprint(it[\"price\"] == 1.5)\n")
	add("json/scalar_bodies", jsonCtx(`"text"`), "print(waf_json())\nprint(typeof(waf_json()))\n")
	add("json/number_body", jsonCtx(`42`), "print(waf_json())\nprint(typeof(waf_json()))\n")
	add("json/float_body", jsonCtx(`42.5`), "print(waf_json())\nprint(typeof(waf_json()))\n")
	add("json/bool_body", jsonCtx(`true`), "print(waf_json())\nprint(typeof(waf_json()))\n")
	add("json/null_body", jsonCtx(`null`), "print(waf_json())\nprint(waf_json() == null)\n")
	add("json/array_body", jsonCtx(`[1,"a",[2],{"k":3}]`), "l = waf_json()\nprint(len(l))\nprint(l[0])\nprint(l[1])\nprint(l[2][0])\nprint(l[3][\"k\"])\nprint(typeof(l))\n")
	add("json/empty_containers", jsonCtx(`{"a":[],"b":{}}`), "d = waf_json()\nprint(len(d[\"a\"]))\nprint(len(d[\"b\"]))\nprint(len(d))\n")
	add("json/unicode_and_escapes", jsonCtx(`{"k":"é😀\n\"q\"","é":"ü"}`), "d = waf_json()\nprint(d[\"k\"])\nprint(len(d[\"k\"]))\nprint(d[\"é\"])\n")
	add("json/duplicate_key_last_wins", jsonCtx(`{"dup":1,"dup":2}`), "print(waf_json()[\"dup\"])\n")
	add("json/is_a_fresh_copy", jsonCtx(`{"items":[{"qty":3}],"n":1}`),
		"a = waf_json()\na[\"items\"][0][\"qty\"] = 9\na[\"n\"] = 2\npush(a[\"items\"], 1)\nb = waf_json()\nprint(b[\"items\"][0][\"qty\"])\nprint(b[\"n\"])\nprint(len(b[\"items\"]))\nprint(a[\"items\"][0][\"qty\"])\n")
	add("json/deep_nesting", jsonCtx(strings.Repeat("[", 100)+"1"+strings.Repeat("]", 100)),
		"d = waf_json()\nn = 0\nwhile (typeof(d) == \"array\"):\n    d = d[0]\n    n = n + 1\nend\nprint(n)\nprint(d)\n")
	add("json/with_arguments", req, guard(`waf_json(1)`))
	add("json/key_order_free_access", jsonCtx(`{"b":2,"a":1,"c":3}`), "d = waf_json()\nprint(d[\"a\"] + d[\"b\"] + d[\"c\"])\nprint(\"a\" in d)\nprint(\"x\" in d)\n")
	return cs
}

// nestedIfs is n blocks inside each other, the innermost running one statement.
func nestedIfs(n int) string {
	return "x = 0\n" + strings.Repeat("if (true) then:\n", n) + "x = 1\n" + strings.Repeat("end\n", n)
}

// nestedMixed alternates the constructs that open a block. Every while loop ends with a break.
func nestedMixed(n int) string {
	kinds := [][2]string{
		{"if (true) then:\n", "end\n"},
		{"while (true):\n", "    break\nend\n"},
		{"for (i in [1]):\n", "end\n"},
		{"try:\n", "catch (e):\n    x = 2\nend\n"},
	}
	var open, rev strings.Builder
	for i := 0; i < n; i++ {
		open.WriteString(kinds[i%len(kinds)][0])
	}
	for i := n - 1; i >= 0; i-- {
		rev.WriteString(kinds[i%len(kinds)][1])
	}
	return "x = 0\n" + open.String() + "x = 1\n" + rev.String()
}

// recursion is a function whose every frame holds a function body and `blocks` nested ifs around the call to
// itself.
func recursion(blocks int) string {
	return "fun f(n):\n" + strings.Repeat("if (true) then:\n", blocks) + "    f(n + 1)\n" + strings.Repeat("end\n", blocks) + "end\n"
}

// loopRecursion is a function pair in which one function holds `loops` nested loops around the call back.
func loopRecursion(loops int) string {
	return "fun f(k):\n    g(k - 1)\nend\nfun g(k):\n" + strings.Repeat("while (true):\n", loops) + "f(k)\n" + strings.Repeat("end\n", loops) + "end\ng(100)\n"
}

func limitCases() []handCase {
	var cs []handCase
	add := func(name, src string) { cs = append(cs, handCase{name, nil, src}) }

	// Blocks active at once: the limit is 256 under the production profile. The top level is a block of its own.
	for _, n := range []int{1, 100, 253, 254, 255, 256, 257, 300} {
		add(fmt.Sprintf("nesting/ifs_%d", n), nestedIfs(n))
	}
	add("nesting/for_loops_300", "x = 0\n"+strings.Repeat("for (i in [1]):\n", 300)+"x = 1\n"+strings.Repeat("end\n", 300))
	add("nesting/while_loops_300", "x = 0\n"+strings.Repeat("while (x < 1):\n", 300)+"x = 1\n"+strings.Repeat("end\n", 300))
	add("nesting/try_blocks_300", "x = 0\n"+strings.Repeat("try:\n", 300)+"x = 1\n"+strings.Repeat("catch (e):\n    x = 2\nend\n", 300))
	add("nesting/catch_does_not_catch_it", "try:\n"+nestedIfs(300)+"catch (e):\n    print(\"caught\")\nend\nprint(\"after\")\n")
	add("nesting/catch_inside_the_nesting", nestedIfs(250)+"try:\n"+nestedIfs(20)+"catch (e):\n    print(\"caught\")\nend\n")
	add("nesting/not_reached", "if (false) then:\n"+strings.Repeat("if (true) then:\n", 300)+strings.Repeat("end\n", 300)+"end\nprint(\"skipped\")\n")
	add("nesting/after_the_blocks_end", nestedIfs(200)+nestedIfs(200)+"print(\"ok\")\n")
	// Blocks of the frames that are active at once count together, so the nesting limit can come before the
	// call depth limit (64).
	add("nesting/calls_5_blocks_per_frame", recursion(4)+"f(0)\n")
	add("nesting/calls_4_blocks_per_frame", recursion(3)+"f(0)\n")
	add("nesting/calls_3_blocks_per_frame", recursion(2)+"f(0)\n")
	add("nesting/calls_2_blocks_per_frame", recursion(1)+"f(0)\n")
	add("nesting/calls_caught", recursion(4)+"try:\n    f(0)\ncatch (e):\n    print(\"caught\")\nend\nprint(\"after\")\n")
	add("nesting/mixed_300", nestedMixed(300))
	add("nesting/mixed_250", nestedMixed(250))
	add("nesting/loops_in_calls_40", loopRecursion(40))
	add("nesting/loops_in_calls_400", loopRecursion(400))
	add("nesting/loops_in_calls_3", loopRecursion(3))
	add("nesting/function_bodies", "fun a():\n    fun b():\n        fun c():\n            return 1\n        end\n        return c()\n    end\n    return b()\nend\nprint(a())\n")
	return cs
}

// timingCases are scripts that only the deadline can stop. Whether one ends in a timeout depends on the speed of
// the host, so the result is recorded with the production deadline and marked as timing dependent.
func timingCases() []handCase {
	return []handCase{
		{"scan_loop", nil, "s = \"a\" * 500000\nwhile (true):\n    x = contains(s, \"b\")\nend\n"},
		{"scan_loop_in_a_call", nil, "s = \"a\" * 500000\nfun f():\n    while (true):\n        x = contains(s, \"b\")\n    end\nend\nf()\n"},
		{"scan_loop_caught", nil, "s = \"a\" * 500000\ntry:\n    while (true):\n        x = contains(s, \"b\")\n    end\ncatch (e):\n    print(\"caught\")\nend\nprint(\"after\")\n"},
	}
}

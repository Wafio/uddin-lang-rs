package main

import (
	"fmt"
	"strings"
)

func init() {
	libRegistry = append(libRegistry, handGroup("lib-collections", collectionCases()))
}

// The scripts of sets, stacks and queues: every operation on empty and full ones, the kinds of value a set keeps
// apart, what a wrong argument says, and what growth costs.

const collPrelude = "fun f():\nend\ninf = 10 ** 400\nnan = inf - inf\n"

// members is a value of every kind a set can hold, as source text. l and m are lists that are equal and not the
// same list.
var members = []string{"1", "1.0", "0", "0.0", "(-0.0)", "2.5", "\"1\"", "\"\"", "\"a\"", "\"s:a\"", "\"i:1\"", "true", "false", "null", "[1]", "[]", "l", "m", "{a: 1}", "{}", "f", "print", "len", "nan", "inf", "(-inf)", "1e21", "1e-7", "9223372036854775807", "(-9223372036854775807 - 1)", "set_new()", "stack_new()", "queue_new()"}

func collectionCases() []handCase {
	var cs []handCase
	add := func(name, src string) { cs = append(cs, handCase{name, nil, src}) }
	setup := collPrelude + "l = [1]\nm = [1]\ns = set_new()\nk = stack_new()\nq = queue_new()\n"

	add("set/basic", "s = set_new()\nprint(set_size(s), set_to_array(s))\nset_add(s, 1)\nset_add(s, \"a\")\nset_add(s, 1)\nset_add(s, [2])\nprint(set_size(s), set_to_array(s))\nprint(set_has(s, 1), set_has(s, \"a\"), set_has(s, \"b\"), set_has(s, 2), set_has(s, [2]))\nprint(set_remove(s, 1), set_remove(s, 1), set_remove(s, \"zz\"))\nprint(set_size(s), set_to_array(s))\nprint(set_add(s, 5))\n")
	add("set/order_after_removals", "s = set_new()\nfor (i in range(10)):\n    set_add(s, i)\nend\nset_remove(s, 3)\nset_remove(s, 0)\nset_remove(s, 9)\nset_add(s, 3)\nprint(set_to_array(s), set_size(s))\nset_add(s, 0)\nprint(set_to_array(s))\n")
	add("set/kinds_are_kept_apart", "s = set_new()\nset_add(s, 1)\nset_add(s, 1.0)\nset_add(s, \"1\")\nset_add(s, true)\nset_add(s, null)\nset_add(s, \"null\")\nset_add(s, \"true\")\nprint(set_size(s), set_to_array(s))\nprint(set_has(s, 1), set_has(s, 1.0), set_has(s, 1.5), set_has(s, \"1\"), set_has(s, true), set_has(s, null), set_has(s, false))\n")
	add("set/colliding_keys", "s = set_new()\nset_add(s, \"i:1\")\nset_add(s, 1)\nset_add(s, \"s:a\")\nset_add(s, \"a\")\nset_add(s, \"b:true\")\nset_add(s, true)\nset_add(s, \"n:null\")\nset_add(s, null)\nset_add(s, \"f:1\")\nset_add(s, 1.0)\nprint(set_size(s), set_to_array(s))\n")
	add("set/float_keys", collPrelude+"s = set_new()\nfor (x in [1.0, 1.5, 0.1, 1e6, 1e21, 1e20, 123456789.0, 1e-5, 0.0001, 100000.0, 1000000.0, 0.000001, -0.0, 0.0, nan, nan, inf, -inf, 2.0 ** 62, 1.0e15, 3.14159265358979, 5e-324, 1.7976931348623157e308]):\n    set_add(s, x)\nend\nprint(set_size(s), str(set_to_array(s)))\nprint(set_has(s, 0.0), set_has(s, -0.0), set_has(s, nan), set_has(s, 1e6), set_has(s, 1000000.0), set_has(s, 1000000))\n")
	add("set/int_keys", "s = set_new()\nfor (x in [0, -1, 1, 9223372036854775807, -9223372036854775807 - 1, 10, 100, -0]):\n    set_add(s, x)\nend\nprint(set_size(s), set_to_array(s))\nprint(set_has(s, 9223372036854775807), set_has(s, 0), set_has(s, -0), set_has(s, 0.0))\n")
	add("set/string_keys_with_odd_bytes", "s = set_new()\nfor (x in [\"\", \" \", \"\\n\", \"\u00e9\", \"\u65e5\u672c\", \"a\", \"A\", \"s:\"]):\n    set_add(s, x)\nend\nprint(set_size(s), set_has(s, \"\"), set_has(s, \"\u00e9\"), set_has(s, \"b\"))\nprint(str(set_to_array(s)))\n")
	add("set/removal_by_equality_not_by_key", "s = set_new()\nset_add(s, 1)\nset_add(s, 1.0)\nset_add(s, 2)\nprint(set_remove(s, 1.0), set_size(s), set_to_array(s))\nprint(set_has(s, 1), set_has(s, 1.0))\nprint(set_remove(s, 1), set_size(s), set_to_array(s))\nt = set_new()\nset_add(t, 1.0)\nset_add(t, 1)\nprint(set_remove(t, 1), set_to_array(t), set_size(t))\nprint(set_remove(t, 1.0), set_to_array(t), set_size(t))\n")
	add("set/removal_of_nan", collPrelude+"s = set_new()\nset_add(s, nan)\nset_add(s, 1)\nprint(set_remove(s, nan), set_size(s), str(set_to_array(s)))\nprint(set_has(s, nan), set_remove(s, nan), set_size(s))\n")
	add("set/removal_of_lists", "a = [1, [2]]\nb = [1, [2]]\ns = set_new()\nset_add(s, a)\nset_add(s, b)\nprint(set_size(s), set_has(s, a), set_has(s, b), set_has(s, [1, [2]]))\nprint(set_remove(s, a), set_size(s), set_to_array(s))\nprint(set_remove(s, a), set_size(s), set_to_array(s))\nset_add(s, a)\nprint(set_to_array(s))\n")
	add("set/removal_of_functions_and_builtins", "fun f():\nend\nfun g():\nend\ns = set_new()\nset_add(s, f)\nset_add(s, g)\nset_add(s, print)\nset_add(s, len)\nprint(set_size(s))\nprint(set_has(s, f), set_has(s, g), set_has(s, print), set_has(s, str))\n"+tryPrint("set_remove(s, f)")+tryPrint("set_remove(s, print)")+tryPrint("set_remove(s, len)")+tryPrint("set_remove(s, str)")+"print(set_size(s))\n")
	add("set/members_that_are_collections", "s = set_new()\nt = set_new()\nk = stack_new()\nq = queue_new()\nset_add(s, t)\nset_add(s, t)\nset_add(s, k)\nset_add(s, q)\nset_add(s, set_new())\nset_add(s, s)\nprint(set_size(s), set_has(s, t), set_has(s, k), set_has(s, q), set_has(s, s), set_has(s, set_new()))\nprint(set_remove(s, t), set_size(s))\nprint(str(set_to_array(s)))\n")
	add("set/member_is_an_error_value", "e = date_add(1, \"x\", 1)\ns = set_new()\nset_add(s, e)\nset_add(s, e)\nset_add(s, date_add(1, \"x\", 1))\nprint(set_size(s), set_has(s, e))\n")
	add("set/to_array_is_a_copy", "s = set_new()\nset_add(s, 1)\nset_add(s, 2)\na = set_to_array(s)\nappend(a, 3)\na[0] = 99\nprint(a, set_to_array(s), set_size(s))\nb = set_to_array(s)\nset_add(s, 4)\nprint(b, set_to_array(s))\nprint(len(b), typeof(b))\n")
	add("set/identity", "s = set_new()\nt = s\nset_add(t, 1)\nprint(set_size(s), s == t, s == s, set_new() == set_new(), s != t, typeof(s), s in [s], [s] == [s])\nu = [s, s]\nprint(typeof(u[0]), set_size(u[1]))\nfun add2(x):\n    set_add(x, 2)\nend\nadd2(s)\nprint(set_to_array(t))\n")
	add("set/as_value", "s = set_new()\nprint(s)\nprint(str(s))\nprint([s])\nprint({k: s})\nprint(str([s, 1]))\nprint(typeof(s), typeof(stack_new()), typeof(queue_new()))\nprint(stack_new(), queue_new())\nprint([stack_new(), queue_new()])\nm = {a: stack_new()}\nprint(str(m))\n")
	add("set/operators_and_builtins_on_collections", "s = set_new()\nk = stack_new()\nq = queue_new()\nfor (v in [s, k, q]):\n    "+tryStmt("print(len(v))")+"\n    "+tryStmt("print(v + 1)")+"\n    "+tryStmt("print(v + v)")+"\n    "+tryStmt("print(v == v)")+"\n    "+tryStmt("print(v != v)")+"\n    "+tryStmt("print(v < v)")+"\n    "+tryStmt("print(v[0])")+"\n    "+tryStmt("v[0] = 1")+"\n    "+tryStmt("print(v ? 1 : 2)")+"\n    "+tryStmt("print(not v)")+"\n    "+tryStmt("print(-v)")+"\n    "+tryStmt("print(v and true)")+"\n    "+tryStmt("print(1 in v)")+"\n    "+tryStmt("print(v in [v])")+"\n    "+tryStmt("print(\"a\" in v)")+"\n    "+tryStmt("for (x in v):\n        print(x)\n    end")+"\n    "+tryStmt("if (v) then:\n        print(1)\n    end")+"\n    "+tryStmt("while (v):\n        print(1)\n    end")+"\n    "+tryStmt("print(v...)")+"\n    "+tryStmt("print(str(v), typeof(v), index_of([v], v))")+"\n    "+tryStmt("print(sort([v, v]))")+"\n    "+tryStmt("print(reverse(v))")+"\n    "+tryStmt("print(append(v, 1))")+"\n    "+tryStmt("print(pop(v))")+"\nend\n")
	add("set/in_a_loop_over_a_list_of_collections", "xs = [set_new(), stack_new(), queue_new(), [1], {a: 1}]\nfor (x in xs):\n    print(typeof(x), str(x))\nend\nprint(map(xs, typeof))\nprint(filter(xs, fun (x):\n    return typeof(x) == \"unknown\"\nend) == [])\nprint(len(map(xs, str)))\n")

	for _, name := range []string{"set_has", "set_add", "set_remove"} {
		var b strings.Builder
		b.WriteString(setup)
		for _, v := range members {
			b.WriteString("s = set_new()\n")
			b.WriteString(tryStmt(fmt.Sprintf("print(%s(s, %s), set_size(s), str(set_to_array(s)))", name, v)))
			b.WriteString(tryStmt(fmt.Sprintf("set_add(s, %s)\n    print(%s(s, %s), set_size(s), str(set_to_array(s)))", v, name, v)))
			b.WriteString(tryStmt(fmt.Sprintf("set_add(s, %s)\n    set_add(s, %s)\n    print(%s(s, %s), set_size(s), str(set_to_array(s)))", v, v, name, v)))
		}
		add("matrix/"+name, b.String())
	}
	var pairs strings.Builder
	pairs.WriteString(setup)
	for i, a := range members[:24] {
		pairs.Reset()
		pairs.WriteString(setup)
		for _, b := range members[:24] {
			pairs.WriteString(tryStmt(fmt.Sprintf("s = set_new()\n    set_add(s, %s)\n    print(set_has(s, %s), set_remove(s, %s), set_size(s), str(set_to_array(s)))", a, b, b)))
		}
		add(fmt.Sprintf("matrix/set_pairs_%02d", i), pairs.String())
	}

	// Stacks and queues.
	add("stack/basic", "k = stack_new()\nprint(stack_size(k), stack_empty(k), stack_pop(k), stack_peek(k))\nstack_push(k, 1)\nstack_push(k, \"b\")\nstack_push(k, [3])\nprint(stack_size(k), stack_empty(k), stack_peek(k))\nprint(stack_pop(k), stack_pop(k), stack_size(k), stack_peek(k))\nprint(stack_pop(k), stack_pop(k), stack_pop(k), stack_size(k), stack_empty(k))\nprint(stack_push(k, 7), stack_peek(k))\nstack_push(k, null)\nprint(stack_size(k), stack_peek(k), stack_pop(k), stack_pop(k), stack_pop(k))\n")
	add("queue/basic", "q = queue_new()\nprint(queue_size(q), queue_empty(q), queue_dequeue(q), queue_front(q))\nqueue_enqueue(q, 1)\nqueue_enqueue(q, \"b\")\nqueue_enqueue(q, [3])\nprint(queue_size(q), queue_empty(q), queue_front(q))\nprint(queue_dequeue(q), queue_dequeue(q), queue_size(q), queue_front(q))\nprint(queue_dequeue(q), queue_dequeue(q), queue_size(q), queue_empty(q))\nprint(queue_enqueue(q, 7), queue_front(q))\nqueue_enqueue(q, null)\nprint(queue_size(q), queue_front(q), queue_dequeue(q), queue_dequeue(q), queue_dequeue(q))\n")
	add("stack/many", "k = stack_new()\nfor (i in range(300)):\n    stack_push(k, i)\nend\nprint(stack_size(k), stack_peek(k))\nn = 0\nwhile (not stack_empty(k)):\n    n += stack_pop(k)\nend\nprint(n, stack_size(k))\nfor (i in range(40)):\n    stack_push(k, i)\n    if (i % 3 == 0) then:\n        stack_pop(k)\n    end\nend\nprint(stack_size(k), stack_peek(k))\n")
	add("queue/many", "q = queue_new()\nfor (i in range(300)):\n    queue_enqueue(q, i)\nend\nprint(queue_size(q), queue_front(q))\nn = 0\nwhile (not queue_empty(q)):\n    n = n * 3 % 1000003 + queue_dequeue(q)\nend\nprint(n, queue_size(q))\nfor (i in range(40)):\n    queue_enqueue(q, i)\n    if (i % 3 == 0) then:\n        queue_dequeue(q)\n    end\nend\nprint(queue_size(q), queue_front(q))\n")
	add("queue/order", "q = queue_new()\nfor (c in [\"a\", \"b\", \"c\"]):\n    queue_enqueue(q, c)\nend\nout = \"\"\nwhile (queue_size(q) > 0):\n    out = out + queue_dequeue(q)\n    if (len(out) == 1) then:\n        queue_enqueue(q, \"d\")\n    end\nend\nprint(out)\n")
	add("stack/identity_and_sharing", "k = stack_new()\nj = k\nstack_push(j, 1)\nprint(stack_size(k), k == j, k == stack_new())\nfun fill(s):\n    stack_push(s, 2)\n    stack_push(s, 3)\nend\nfill(k)\nprint(stack_size(j), stack_peek(j))\nq = queue_new()\nfun fq(x):\n    queue_enqueue(x, 5)\nend\nfq(q)\nfq(q)\nprint(queue_size(q))\n")
	add("stack/holding_collections", "k = stack_new()\nstack_push(k, k)\nstack_push(k, stack_new())\nstack_push(k, queue_new())\nstack_push(k, set_new())\nprint(stack_size(k))\nprint(typeof(stack_peek(k)), stack_pop(k) == stack_new())\nprint(stack_size(stack_pop(stack_pop(k))))\n"+tryPrint("stack_size(stack_pop(k))"))
	add("queue/holding_lists_shared", "q = queue_new()\nl = [1, 2]\nqueue_enqueue(q, l)\nqueue_enqueue(q, l)\nl[0] = 9\nprint(queue_dequeue(q), queue_dequeue(q), queue_dequeue(q))\n")

	// Wrong arguments, a call at a time: every builtin with every kind of value in each place, and the counts.
	cpool := []string{"[1]", "\"a\"", "1", "null", "true", "{a: 1}", "f", "s", "k", "q", "set_new()", "stack_new()", "queue_new()"}
	for _, name := range []string{"set_new", "set_add", "set_remove", "set_has", "set_size", "set_to_array", "stack_new", "stack_push", "stack_pop", "stack_peek", "stack_size", "stack_empty", "queue_new", "queue_enqueue", "queue_dequeue", "queue_front", "queue_size", "queue_empty"} {
		var b strings.Builder
		b.WriteString(setup)
		b.WriteString(tryPrint(name + "()"))
		for _, a := range cpool {
			b.WriteString(tryPrint(fmt.Sprintf("%s(%s)", name, a)))
			for _, c := range []string{"1", "\"x\"", "null", "[1]", "s", "k"} {
				b.WriteString(tryPrint(fmt.Sprintf("%s(%s, %s)", name, a, c)))
			}
			b.WriteString(tryPrint(fmt.Sprintf("%s(%s, 1, 2)", name, a)))
		}
		b.WriteString(tryPrint(fmt.Sprintf("%s(s, 1, 2, 3, 4)", name)))
		add("args/"+name, b.String())
	}
	add("args/wrong_collection_for_each", setup+tryPrint("set_add(k, 1)")+tryPrint("set_add(q, 1)")+tryPrint("stack_push(s, 1)")+tryPrint("stack_push(q, 1)")+tryPrint("queue_enqueue(s, 1)")+tryPrint("queue_enqueue(k, 1)")+tryPrint("stack_pop(q)")+tryPrint("queue_dequeue(k)")+tryPrint("stack_size(s)")+tryPrint("queue_size(k)")+tryPrint("set_size(q)")+tryPrint("stack_empty(q)")+tryPrint("queue_empty(s)")+tryPrint("set_to_array(k)")+tryPrint("stack_peek(q)")+tryPrint("queue_front(k)"))

	// Memory: the growth of a set, a stack and a queue is charged as the growth of a list is, and a fresh one
	// costs 64 bytes. The ledger prints how much of the budget was left after setup.
	steps := capSteps(6000)
	var ks []int
	for _, c := range steps {
		for _, d := range []int{-1, 0, 1} {
			if c+d >= 0 {
				ks = append(ks, c+d)
			}
		}
	}
	ks = append(ks, 0)
	seen := map[int]bool{}
	for _, k := range ks {
		if seen[k] || k > 5000 {
			continue
		}
		seen[k] = true
		add(fmt.Sprintf("ledger/stack_pushes_%d", k), ledger(fmt.Sprintf("k = stack_new()\ni = 0\nwhile (i < %d):\n    stack_push(k, i)\n    i += 1\nend\n", k), 64+16*k+24*capCount(k)))
		add(fmt.Sprintf("ledger/stack_pushes_then_pops_%d", k), ledger(fmt.Sprintf("k = stack_new()\ni = 0\nwhile (i < %d):\n    stack_push(k, i)\n    i += 1\nend\nstack_pop(k)\nstack_pop(k)\nstack_push(k, 1)\nstack_push(k, 2)\nstack_push(k, 3)\n", k), 64+16*k+24*capCount(k)+100))
		add(fmt.Sprintf("ledger/queue_enqueues_%d", k), ledger(fmt.Sprintf("q = queue_new()\ni = 0\nwhile (i < %d):\n    queue_enqueue(q, i)\n    i += 1\nend\n", k), 64+16*k+24*capCount(k)))
		add(fmt.Sprintf("ledger/queue_dequeues_then_enqueues_%d", k), ledger(fmt.Sprintf("q = queue_new()\ni = 0\nwhile (i < %d):\n    queue_enqueue(q, i)\n    i += 1\nend\nqueue_dequeue(q)\nqueue_dequeue(q)\nqueue_enqueue(q, 1)\nqueue_enqueue(q, 2)\nqueue_enqueue(q, 3)\n", k), 64+16*k+24*capCount(k)+100))
		if k <= 1200 {
			add(fmt.Sprintf("ledger/set_adds_%d", k), ledger(fmt.Sprintf("s = set_new()\ni = 0\nwhile (i < %d):\n    set_add(s, i)\n    i += 1\nend\n", k), 64+80*k))
			add(fmt.Sprintf("ledger/set_adds_strings_%d", k), ledger(fmt.Sprintf("s = set_new()\ni = 0\nwhile (i < %d):\n    set_add(s, \"key\" + str(i))\n    i += 1\nend\n", k), 64+(80+6+16+8)*k+(16+10)*k))
		}
	}
	edgeCases(add, "set_new", "", 0, "s = set_new()", 64)
	edgeCases(add, "stack_new", "", 0, "k = stack_new()", 64)
	edgeCases(add, "queue_new", "", 0, "q = queue_new()", 64)
	edgeCases(add, "set_add_int", "s = set_new()\n", 64, "set_add(s, 5)", 80)
	edgeCases(add, "set_add_string", "s = set_new()\n", 64, "set_add(s, \"key\")", 85)
	edgeCases(add, "set_add_present", "s = set_new()\nset_add(s, 5)\n", 144, "set_add(s, 5)", 0)
	edgeCases(add, "stack_push_first", "k = stack_new()\n", 64, "stack_push(k, 1)", 24+16)
	for _, n := range []int{1, 2, 3, 4, 7, 8, 15, 16, 17, 70, 71} {
		setup := fmt.Sprintf("k = stack_new()\ni = 0\nwhile (i < %d):\n    stack_push(k, i)\n    i += 1\nend\n", n)
		spent := 64
		length, c := 0, 0
		for i := 0; i < n; i++ {
			length++
			if length > c {
				c = goCap(c, length)
				spent += 24 + 16*c
			} else {
				spent += 16
			}
		}
		next := 16
		if n+1 > c {
			next = 24 + 16*goCap(c, n+1)
		}
		edgeCases(add, fmt.Sprintf("stack_push_%d", n), setup, spent, "stack_push(k, 0)", next)
		edgeCases(add, fmt.Sprintf("queue_after_dequeues_%d", n), strings.Replace(strings.Replace(setup, "stack_new", "queue_new", 1), "stack_push", "queue_enqueue", 1)+"queue_dequeue(k)\n", spent+8, "queue_enqueue(k, 0)", 0)
	}
	for _, n := range []int{1, 2, 10, 100, 1000} {
		add(fmt.Sprintf("ledger/fresh_objects_%d", n), ledger(fmt.Sprintf("i = 0\nwhile (i < %d):\n    s = set_new()\n    k = stack_new()\n    q = queue_new()\n    i += 1\nend\n", n), 192*n))
		add(fmt.Sprintf("ledger/to_array_%d", n), ledger(fmt.Sprintf("s = set_new()\ni = 0\nwhile (i < %d):\n    set_add(s, i)\n    i += 1\nend\na = set_to_array(s)\nb = set_to_array(s)\n", n), 64+80*n+2*(24+16*n)))
	}
	add("ledger/set_removals", ledger("s = set_new()\ni = 0\nwhile (i < 100):\n    set_add(s, i)\n    i += 1\nend\ni = 0\nwhile (i < 100):\n    set_remove(s, i)\n    set_add(s, i + 1000)\n    i += 1\nend\n", 64+80*200))
	add("ledger/pops_and_dequeues_of_big_things", ledger("k = stack_new()\nq = queue_new()\nstack_push(k, \"a\" * 1000)\nstack_push(k, range(100))\nqueue_enqueue(q, \"b\" * 500)\nqueue_enqueue(q, [1, 2, 3])\nx = stack_pop(k)\ny = stack_pop(k)\nz = queue_dequeue(q)\nw = queue_dequeue(q)\nv = stack_peek(k)\nu = queue_front(q)\n", 2000))
	for _, n := range []int{10000, 100000, 300000, 500000, 700000, 1000000} {
		add(fmt.Sprintf("fill/set_add_long_keys_%d", n), fmt.Sprintf("s = set_new()\ni = 0\nwhile (true):\n    set_add(s, \"a\" * %d + str(i))\n    i += 1\n    print(i, set_size(s))\nend\n", n))
		add(fmt.Sprintf("fill/set_add_long_key_variable_%d", n), fmt.Sprintf("s = set_new()\nbase = \"b\" * %d\ni = 0\nwhile (true):\n    set_add(s, base + str(i))\n    i += 1\n    print(i)\nend\n", n))
		add(fmt.Sprintf("fill/stack_of_long_strings_%d", n), fmt.Sprintf("k = stack_new()\nbase = \"b\" * %d\ni = 0\nwhile (true):\n    stack_push(k, base + str(i))\n    i += 1\n    print(i)\nend\n", n))
	}
	return cs
}

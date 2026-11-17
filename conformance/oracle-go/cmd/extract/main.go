// Command extract builds the conformance corpus from Go test sources and .din files. It reads the Go files with
// go/parser and folds the string expressions it can evaluate without running anything (literals, +,
// strings.Repeat, fmt.Sprintf, names bound once to such an expression), so the output is the same on every run.
//
//	extract -fork DIR -engine DIR [-profile FILE] [-out DIR]
//
// DIR of -fork is a checkout of the Go fork (with its tests and examples), DIR of -engine is the engine
// directory of the control plane. Three more groups are written by hand (handwritten.go). Each group becomes out/check/<group>.cases.jsonl with every source and
// out/run/<group>.cases.jsonl with the sources that pass the check.
package main

import (
	"flag"
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"io/fs"
	"os"
	"path/filepath"
	"regexp"
	"slices"
	"sort"
	"strconv"
	"strings"

	"oracle-go/internal/oracle"
)

// maxSourceBytes drops the generated megabyte sources of the nesting tests; the size bound is covered by the
// sources just over 64 KiB.
const maxSourceBytes = 256 << 10

type group struct {
	name  string
	cases []oracle.Case
	stats stats
}

type stats struct{ files, candidates, notProgram, tooBig, duplicate int }

func main() {
	forkDir := flag.String("fork", "", "checkout of the Go fork")
	engineDir := flag.String("engine", "", "engine directory of the control plane")
	profilePath := flag.String("profile", "../../profile/sandbox-profile.json", "sandbox profile")
	outDir := flag.String("out", "../corpus", "output directory")
	only := flag.String("only", "", "comma-separated groups to write; with only hand-written ones, -fork and -engine are not needed")
	flag.Parse()
	if err := run(*forkDir, *engineDir, *profilePath, *outDir, *only); err != nil {
		fmt.Fprintln(os.Stderr, "extract:", err)
		os.Exit(1)
	}
}

func run(forkDir, engineDir, profilePath, outDir, only string) error {
	prof, err := oracle.LoadProfile(profilePath)
	if err != nil {
		return err
	}
	if only != "" && forkDir == "" && engineDir == "" {
		return writeGroups(prof, handwritten(), outDir, only)
	}
	if forkDir == "" || engineDir == "" {
		return fmt.Errorf("-fork and -engine are required")
	}
	var forkTests []string
	for _, pat := range []string{"interpreter/*_test.go", "*_test.go", "cmd/*/*_test.go"} {
		m, _ := filepath.Glob(filepath.Join(forkDir, pat))
		forkTests = append(forkTests, m...)
	}
	engineAll, _ := filepath.Glob(filepath.Join(engineDir, "*_test.go"))
	sort.Strings(forkTests)
	sort.Strings(engineAll)

	fork, err := fromGoFiles("fork-tests", forkDir, "fork", forkTests, forkTests)
	if err != nil {
		return err
	}
	// The files the corpus takes scripts from are those that call the script entry points.
	engineUse, err := filesCalling(engineAll, "EvalScriptRules", "EvalScriptRulesWithBudget", "CompileScript")
	if err != nil {
		return err
	}
	fmt.Fprintf(os.Stderr, "engine files: %s\n", strings.Join(baseNames(engineUse), " "))
	// The benchmark file has its own group, with the request context of its scripts.
	benchPath := filepath.Join(engineDir, "custom_rules_bench_test.go")
	engine, err := fromGoFiles("engine-tests", engineDir, "engine", engineAll, slices.DeleteFunc(slices.Clone(engineUse), func(p string) bool { return p == benchPath }))
	if err != nil {
		return err
	}
	bench, err := benchGroup(benchPath)
	if err != nil {
		return err
	}
	examples, err := dinGroup(forkDir)
	if err != nil {
		return err
	}

	groups := append([]*group{fork, engine, bench, examples}, handwritten()...)
	return writeGroups(prof, groups, outDir, only)
}

// writeGroups writes the check and run cases of the groups named in only (all of them when only is empty).
func writeGroups(prof *oracle.Profile, groups []*group, outDir, only string) error {
	for _, sub := range []string{"check", "run"} {
		if err := os.MkdirAll(filepath.Join(outDir, sub), 0o755); err != nil {
			return err
		}
	}
	fmt.Printf("%-14s %6s %6s %6s   %s\n", "group", "check", "run", "files", "dropped: not-a-program too-big duplicate")
	for _, g := range groups {
		if only != "" && !slices.Contains(strings.Split(only, ","), g.name) {
			continue
		}
		var runCases []oracle.Case
		for _, c := range g.cases {
			src, _ := c.Source()
			if _, stage, _ := prof.Check(src); stage == "" {
				runCases = append(runCases, c)
			}
		}
		if err := writeCases(filepath.Join(outDir, "check", g.name+".cases.jsonl"), g.cases); err != nil {
			return err
		}
		if err := writeCases(filepath.Join(outDir, "run", g.name+".cases.jsonl"), runCases); err != nil {
			return err
		}
		fmt.Printf("%-14s %6d %6d %6d   %d %d %d\n", g.name, len(g.cases), len(runCases), g.stats.files,
			g.stats.notProgram, g.stats.tooBig, g.stats.duplicate)
	}
	return nil
}

func writeCases(path string, cases []oracle.Case) error {
	f, err := os.Create(path)
	if err != nil {
		return err
	}
	for _, c := range cases {
		if err := oracle.WriteLine(f, c); err != nil {
			f.Close()
			return err
		}
	}
	return f.Close()
}

func baseNames(paths []string) []string {
	var out []string
	for _, p := range paths {
		out = append(out, filepath.Base(p))
	}
	return out
}

// filesCalling returns the files that contain a call to one of the named functions or methods.
func filesCalling(paths []string, names ...string) ([]string, error) {
	var out []string
	fset := token.NewFileSet()
	for _, p := range paths {
		f, err := parser.ParseFile(fset, p, nil, 0)
		if err != nil {
			return nil, err
		}
		found := false
		ast.Inspect(f, func(n ast.Node) bool {
			if call, ok := n.(*ast.CallExpr); ok {
				name := ""
				switch fn := call.Fun.(type) {
				case *ast.Ident:
					name = fn.Name
				case *ast.SelectorExpr:
					name = fn.Sel.Name
				}
				for _, want := range names {
					found = found || name == want
				}
			}
			return !found
		})
		if found {
			out = append(out, p)
		}
	}
	return out, nil
}

// fromGoFiles takes the program-like string expressions of the emit files. The files in all are parsed for the
// names bound at package level.
func fromGoFiles(name, root, prefix string, all, emit []string) (*group, error) {
	fset := token.NewFileSet()
	parsed := map[string]*ast.File{}
	pkgScopes := map[string]*scope{} // by directory and package name
	for _, p := range all {
		f, err := parser.ParseFile(fset, p, nil, 0)
		if err != nil {
			return nil, err
		}
		parsed[p] = f
		key := filepath.Dir(p) + "\x00" + f.Name.Name
		if pkgScopes[key] == nil {
			pkgScopes[key] = newScope(nil)
		}
		for _, d := range f.Decls {
			if gd, ok := d.(*ast.GenDecl); ok {
				pkgScopes[key].collect(gd)
			}
		}
	}
	g := &group{name: name}
	seen := map[string]bool{}
	for _, p := range emit {
		f := parsed[p]
		g.stats.files++
		rel, _ := filepath.Rel(root, p)
		pkg := pkgScopes[filepath.Dir(p)+"\x00"+f.Name.Name]
		emitFn := func(pos token.Pos, s string) {
			g.stats.candidates++
			switch {
			case !looksLikeProgram(s):
				g.stats.notProgram++
			case len(s) > maxSourceBytes:
				g.stats.tooBig++
			case seen[s]:
				g.stats.duplicate++
			default:
				seen[s] = true
				at := fset.Position(pos)
				id := fmt.Sprintf("%s/%s:%d:%d", prefix, filepath.ToSlash(rel), at.Line, at.Column)
				g.cases = append(g.cases, oracle.NewCase(id, []byte(s), nil))
			}
		}
		for _, d := range f.Decls {
			switch d := d.(type) {
			case *ast.FuncDecl:
				if d.Body == nil {
					continue
				}
				sc := newScope(pkg)
				sc.collect(d)
				walk(d.Body, sc, emitFn)
			case *ast.GenDecl:
				walk(d, pkg, emitFn)
			}
		}
	}
	return g, nil
}

// A source is program-like when it has a line break, or a line that starts with a call, an assignment or a
// statement keyword, and is not a format string. Test names, paths, error messages and expectations have none of
// them.
var (
	programLike = regexp.MustCompile(`(?m)\n|^\s*([A-Za-z_]\w*(\[[^\]]*\])?\s*[-+*/%]?=[^=]|[A-Za-z_]\w*\(|(if|elif|else|while|for|fun|try|catch|return|import|break|continue|end)\b)`)
	formatVerb  = regexp.MustCompile(`%[-+#0-9.]*[vqsdT]`)
)

func looksLikeProgram(s string) bool { return programLike.MatchString(s) && !formatVerb.MatchString(s) }

// reporters are the testing methods whose arguments are messages.
var reporters = map[string]bool{"Errorf": true, "Fatalf": true, "Logf": true, "Skipf": true, "Fatal": true, "Error": true, "Log": true, "Skip": true}

// walk calls emit for every maximal string expression under n that can be folded. A concatenation or call
// that cannot be folded is skipped whole: its literal operands are fragments of a program, not programs.
func walk(n ast.Node, sc *scope, emit func(token.Pos, string)) {
	ast.Inspect(n, func(n ast.Node) bool {
		e, ok := n.(ast.Expr)
		if !ok {
			return true
		}
		switch e := e.(type) {
		case *ast.Ident:
			return false
		case *ast.BasicLit, *ast.BinaryExpr, *ast.CallExpr:
			if call, ok := e.(*ast.CallExpr); ok {
				if sel, ok := call.Fun.(*ast.SelectorExpr); ok && reporters[sel.Sel.Name] {
					return false
				}
			}
			if v, ok := sc.eval(e); ok {
				if v.isString {
					emit(e.Pos(), v.s)
				}
				return false
			}
			if b, ok := e.(*ast.BinaryExpr); ok && b.Op == token.ADD {
				return false
			}
		}
		return true
	})
}

// value is a folded constant: a string or an integer.
type value struct {
	isString bool
	s        string
	n        int64
}

// scope holds the bindings of a function or a package. A name counts as a constant only when it is bound once,
// anywhere in the scope, to an expression that folds.
type scope struct {
	parent *scope
	defs   map[string][]ast.Expr // nil entry: bound to something that is not a plain expression
	memo   map[string]*value
	busy   map[string]bool
}

func newScope(parent *scope) *scope {
	return &scope{parent: parent, defs: map[string][]ast.Expr{}, memo: map[string]*value{}, busy: map[string]bool{}}
}

func (s *scope) bind(name string, rhs ast.Expr) {
	if name != "_" {
		s.defs[name] = append(s.defs[name], rhs)
	}
}

// collect records every binding under n.
func (s *scope) collect(n ast.Node) {
	ast.Inspect(n, func(n ast.Node) bool {
		switch n := n.(type) {
		case *ast.AssignStmt:
			for i, l := range n.Lhs {
				if id, ok := l.(*ast.Ident); ok {
					var rhs ast.Expr
					if len(n.Lhs) == len(n.Rhs) && (n.Tok == token.DEFINE || n.Tok == token.ASSIGN) {
						rhs = n.Rhs[i]
					}
					s.bind(id.Name, rhs)
				}
			}
		case *ast.ValueSpec:
			for i, id := range n.Names {
				var rhs ast.Expr
				if len(n.Values) == len(n.Names) {
					rhs = n.Values[i]
				}
				s.bind(id.Name, rhs)
			}
		case *ast.RangeStmt:
			for _, e := range []ast.Expr{n.Key, n.Value} {
				if id, ok := e.(*ast.Ident); ok {
					s.bind(id.Name, nil)
				}
			}
		case *ast.IncDecStmt:
			if id, ok := n.X.(*ast.Ident); ok {
				s.bind(id.Name, nil)
			}
		case *ast.FuncType:
			for _, fl := range []*ast.FieldList{n.Params, n.Results} {
				if fl == nil {
					continue
				}
				for _, f := range fl.List {
					for _, id := range f.Names {
						s.bind(id.Name, nil)
					}
				}
			}
		}
		return true
	})
}

func (s *scope) lookup(name string) (*value, bool) {
	for sc := s; sc != nil; sc = sc.parent {
		rhs, bound := sc.defs[name]
		if !bound {
			continue
		}
		if v, ok := sc.memo[name]; ok {
			return v, v != nil
		}
		if len(rhs) != 1 || rhs[0] == nil || sc.busy[name] {
			return nil, false
		}
		sc.busy[name] = true
		v, ok := sc.eval(rhs[0])
		delete(sc.busy, name)
		if !ok {
			sc.memo[name] = nil
			return nil, false
		}
		sc.memo[name] = &v
		return &v, true
	}
	return nil, false
}

// maxRepeat keeps a folded string from exhausting memory; the sources it would build are dropped anyway.
const maxRepeat = 1 << 24

func (s *scope) eval(e ast.Expr) (value, bool) {
	switch e := e.(type) {
	case *ast.BasicLit:
		switch e.Kind {
		case token.STRING:
			u, err := strconv.Unquote(e.Value)
			return value{isString: true, s: u}, err == nil
		case token.INT:
			n, err := strconv.ParseInt(strings.ReplaceAll(e.Value, "_", ""), 0, 64)
			return value{n: n}, err == nil
		}
	case *ast.ParenExpr:
		return s.eval(e.X)
	case *ast.Ident:
		if v, ok := s.lookup(e.Name); ok {
			return *v, true
		}
	case *ast.BinaryExpr:
		l, lok := s.eval(e.X)
		r, rok := s.eval(e.Y)
		if !lok || !rok || l.isString != r.isString {
			return value{}, false
		}
		switch {
		case l.isString && e.Op == token.ADD:
			return value{isString: true, s: l.s + r.s}, true
		case !l.isString && e.Op == token.ADD:
			return value{n: l.n + r.n}, true
		case !l.isString && e.Op == token.SUB:
			return value{n: l.n - r.n}, true
		case !l.isString && e.Op == token.MUL:
			return value{n: l.n * r.n}, true
		}
	case *ast.CallExpr:
		return s.evalCall(e)
	}
	return value{}, false
}

func (s *scope) evalCall(c *ast.CallExpr) (value, bool) {
	// []byte(x) and string(x)
	if at, ok := c.Fun.(*ast.ArrayType); ok && len(c.Args) == 1 {
		if id, ok := at.Elt.(*ast.Ident); ok && id.Name == "byte" {
			return s.eval(c.Args[0])
		}
	}
	if id, ok := c.Fun.(*ast.Ident); ok && id.Name == "string" && len(c.Args) == 1 {
		return s.eval(c.Args[0])
	}
	sel, ok := c.Fun.(*ast.SelectorExpr)
	if !ok {
		return value{}, false
	}
	pkg, ok := sel.X.(*ast.Ident)
	if !ok {
		return value{}, false
	}
	switch pkg.Name + "." + sel.Sel.Name {
	case "strings.Repeat":
		if len(c.Args) != 2 {
			return value{}, false
		}
		str, sok := s.eval(c.Args[0])
		n, nok := s.eval(c.Args[1])
		if !sok || !nok || !str.isString || n.isString || n.n < 0 || int64(len(str.s))*n.n > maxRepeat {
			return value{}, false
		}
		return value{isString: true, s: strings.Repeat(str.s, int(n.n))}, true
	case "fmt.Sprintf":
		return s.sprintf(c.Args)
	}
	return value{}, false
}

// sprintf folds the verbs %s %q %d %v and %%, with no flags or widths.
func (s *scope) sprintf(args []ast.Expr) (value, bool) {
	if len(args) == 0 {
		return value{}, false
	}
	format, ok := s.eval(args[0])
	if !ok || !format.isString {
		return value{}, false
	}
	rest := args[1:]
	var b strings.Builder
	for i := 0; i < len(format.s); i++ {
		if format.s[i] != '%' {
			b.WriteByte(format.s[i])
			continue
		}
		i++
		if i == len(format.s) {
			return value{}, false
		}
		if format.s[i] == '%' {
			b.WriteByte('%')
			continue
		}
		if len(rest) == 0 {
			return value{}, false
		}
		a, ok := s.eval(rest[0])
		rest = rest[1:]
		if !ok {
			return value{}, false
		}
		switch verb := format.s[i]; {
		case verb == 's' && a.isString, verb == 'v' && a.isString:
			b.WriteString(a.s)
		case verb == 'q' && a.isString:
			b.WriteString(strconv.Quote(a.s))
		case (verb == 'd' || verb == 'v') && !a.isString:
			b.WriteString(strconv.FormatInt(a.n, 10))
		default:
			return value{}, false
		}
	}
	return value{isString: true, s: b.String()}, len(rest) == 0
}

// benchGroup reads the six benchmark scripts and their shared request context from the benchmark file. It fails
// when the file no longer has that shape.
func benchGroup(path string) (*group, error) {
	fset := token.NewFileSet()
	f, err := parser.ParseFile(fset, path, nil, 0)
	if err != nil {
		return nil, err
	}
	var ctxLit, table *ast.CompositeLit
	ast.Inspect(f, func(n ast.Node) bool {
		cl, ok := n.(*ast.CompositeLit)
		if !ok {
			return true
		}
		switch t := cl.Type.(type) {
		case *ast.Ident:
			if t.Name == "WAFRuleContext" {
				ctxLit = cl
			}
		case *ast.ArrayType:
			if st, ok := t.Elt.(*ast.StructType); ok && len(st.Fields.List) == 1 && len(st.Fields.List[0].Names) == 3 {
				table = cl
			}
		}
		return true
	})
	if ctxLit == nil || table == nil {
		return nil, fmt.Errorf("%s: benchmark context or script table not found", path)
	}
	sc := newScope(nil)
	ctx := new(oracle.Ctx)
	for _, el := range ctxLit.Elts {
		kv := el.(*ast.KeyValueExpr)
		key := kv.Key.(*ast.Ident).Name
		str := func() string {
			v, ok := sc.eval(kv.Value)
			if !ok || !v.isString {
				err = fmt.Errorf("%s: context field %s is not a string literal", path, key)
			}
			return v.s
		}
		switch key {
		case "Method":
			ctx.Method = str()
		case "Path":
			ctx.Path = str()
		case "Query":
			ctx.Query = str()
		case "IP":
			ctx.IP = str()
		case "Country":
			ctx.Country = str()
		case "UA":
			ctx.UA = str()
		case "Body":
			ctx.Body = str()
		case "Action":
			ctx.Action = str()
		case "Score":
			var n float64
			n, err = strconv.ParseFloat(strings.ReplaceAll(kv.Value.(*ast.BasicLit).Value, "_", ""), 64)
			ctx.Score = n
		case "Headers":
			ctx.Headers = map[string]string{}
			for _, h := range kv.Value.(*ast.CompositeLit).Elts {
				hv := h.(*ast.KeyValueExpr)
				k, _ := sc.eval(hv.Key)
				v, _ := sc.eval(hv.Value)
				ctx.Headers[k.s] = v.s
			}
		case "Detected":
			for _, d := range kv.Value.(*ast.CompositeLit).Elts {
				v, _ := sc.eval(d)
				ctx.Detected = append(ctx.Detected, v.s)
			}
		default:
			err = fmt.Errorf("%s: unexpected context field %s", path, key)
		}
		if err != nil {
			return nil, err
		}
	}
	g := &group{name: "bench"}
	g.stats.files = 1
	for _, row := range table.Elts {
		cl := row.(*ast.CompositeLit)
		name, nok := sc.eval(cl.Elts[0])
		script, sok := sc.eval(cl.Elts[1])
		if !nok || !sok {
			return nil, fmt.Errorf("%s: benchmark row is not made of string literals", path)
		}
		g.cases = append(g.cases, oracle.NewCase("bench/"+name.s, []byte(script.s), ctx))
	}
	if len(g.cases) != 6 {
		return nil, fmt.Errorf("%s: %d benchmark scripts, want 6", path, len(g.cases))
	}
	return g, nil
}

// dinGroup takes every .din file of the fork's examples and example_usages.
func dinGroup(forkDir string) (*group, error) {
	g := &group{name: "examples"}
	for _, sub := range []string{"examples", "example_usages"} {
		root := filepath.Join(forkDir, sub)
		err := filepath.WalkDir(root, func(p string, d fs.DirEntry, err error) error {
			if err != nil || d.IsDir() || filepath.Ext(p) != ".din" {
				return err
			}
			src, err := os.ReadFile(p)
			if err != nil {
				return err
			}
			rel, _ := filepath.Rel(forkDir, p)
			g.stats.files++
			g.cases = append(g.cases, oracle.NewCase("fork/"+filepath.ToSlash(rel), src, nil))
			return nil
		})
		if err != nil {
			return nil, err
		}
	}
	return g, nil
}

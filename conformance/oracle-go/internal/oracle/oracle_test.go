package oracle

import (
	"encoding/base64"
	"strings"
	"testing"
	"time"
)

const profilePath = "../../../../profile/sandbox-profile.json"

func loadProfile(t *testing.T) *Profile {
	t.Helper()
	p, err := LoadProfile(profilePath)
	if err != nil {
		t.Fatal(err)
	}
	return p
}

func TestCheckStages(t *testing.T) {
	p := loadProfile(t)
	for _, tc := range []struct {
		name, src, stage string
		forbidden        []string
	}{
		{"ok", "x = 1\nprint(x)\n", "", nil},
		{"empty", "", "", nil},
		{"size", strings.Repeat(" ", p.Limits.MaxScriptBytes+1), StageSize, nil},
		{"tokenize", "x = \"abc\n", StageTokenize, nil},
		{"parse", "if then end", StageParse, nil},
		{"parse before illegal token", "if then end\nx = \"abc\n", StageParse, nil},
		{"forbidden call", "read_file(\"/etc/passwd\")\nsleep(1)\n", StageAllowlist, []string{"read_file", "sleep"}},
		{"forbidden assigned name", "exit = 1\n", StageAllowlist, []string{"exit"}},
		{"import", "import \"lib.din\"\n", StageAllowlist, []string{"import"}},
	} {
		t.Run(tc.name, func(t *testing.T) {
			_, stage, forbidden := p.Check([]byte(tc.src))
			if stage != tc.stage || strings.Join(forbidden, ",") != strings.Join(tc.forbidden, ",") {
				t.Fatalf("got stage %q forbidden %v, want %q %v", stage, forbidden, tc.stage, tc.forbidden)
			}
		})
	}
}

func TestCheckCaseShape(t *testing.T) {
	p := loadProfile(t)
	ok := p.CheckCase("a", []byte("x = 1"))
	if !ok.OK || ok.Stage != nil || ok.Forbidden == nil || len(ok.Forbidden) != 0 {
		t.Fatalf("accepted source: %+v", ok)
	}
	bad := p.CheckCase("b", []byte("exit(1)"))
	if bad.OK || bad.Stage == nil || *bad.Stage != StageAllowlist || len(bad.Forbidden) != 1 {
		t.Fatalf("rejected source: %+v", bad)
	}
}

func runSrc(t *testing.T, src string, ctx *Ctx) RunResult {
	t.Helper()
	c := NewCase("t", []byte(src), ctx)
	res, err := loadProfile(t).RunCase(c, 2, 30*time.Second)
	if err != nil {
		t.Fatal(err)
	}
	return res
}

func TestRunPrintAndVerdict(t *testing.T) {
	res := runSrc(t, "print(\"hi\")\nwaf_block()\n", nil)
	// A verdict at top level ends the script with a runtime error; the verdict still stands.
	if res.Verdict != "block" || res.Outcome != OutcomeError || res.ErrorClass != "runtime" {
		t.Fatalf("%+v", res)
	}
	if got, _ := base64.StdEncoding.DecodeString(res.PrintB64); string(got) != "hi\n" {
		t.Fatalf("print %q", got)
	}
}

func TestRunLimitAndContext(t *testing.T) {
	if res := runSrc(t, "x = 0\nwhile (true):\n    x = x + 1\nend\n", nil); res.Outcome != OutcomeLimit || res.Limit != "op_limit" {
		t.Fatalf("%+v", res)
	}
	ctx := &Ctx{Method: "POST", Score: 7, Headers: map[string]string{"X-A": "b"}, JSON: []byte(`{"n": 5, "f": 1.5}`),
		Fields: map[string]string{"query:id": "9"}}
	src := "if (method == \"POST\" and waf_score() == 7 and waf_header(\"x-a\") == \"b\" and waf_json()[\"n\"] == 5 and waf_field(\"query:id\") == \"9\") then:\n    waf_log()\nend\n"
	if res := runSrc(t, src, ctx); res.Verdict != "log" {
		t.Fatalf("%+v", res)
	}
}

func TestRunRejectedAndClock(t *testing.T) {
	if res := runSrc(t, "sleep(1)", nil); res.Outcome != OutcomeRejected || res.Stage != StageAllowlist {
		t.Fatalf("%+v", res)
	}
	if res := runSrc(t, "t = time_now()\n", nil); !res.Clock {
		t.Fatalf("%+v", res)
	}
}

func TestRunTimeoutIsTimingDependent(t *testing.T) {
	c := NewCase("t", []byte("s = \"a\" * 500000\nwhile (true):\n    x = contains(s, \"b\")\nend\n"), nil)
	res, err := loadProfile(t).RunCase(c, 2, 20*time.Millisecond)
	if err != nil {
		t.Fatal(err)
	}
	if res.Outcome != OutcomeLimit || res.Limit != "timeout" || !res.Timing {
		t.Fatalf("%+v", res)
	}
	if res := runSrc(t, "x = 0\nwhile (true):\n    x = x + 1\nend\n", nil); res.Timing {
		t.Fatalf("an op limit is not timing dependent: %+v", res)
	}
}

func TestRunOrderSensitive(t *testing.T) {
	// Go starts a map iteration at a random slot: 16 runs over 8 keys all agreeing has a chance below 1e-13.
	src := "m = {\"a\": 1, \"b\": 2, \"c\": 3, \"d\": 4, \"e\": 5, \"f\": 6, \"g\": 7, \"h\": 8}\nfor (k in m):\n    print(k)\nend\n"
	c := NewCase("t", []byte(src), nil)
	res, err := loadProfile(t).RunCase(c, 16, 30*time.Second)
	if err != nil {
		t.Fatal(err)
	}
	if res.Outcome != OutcomeOK {
		t.Fatalf("%+v", res)
	}
	if !res.OrderSensitive {
		t.Fatal("a map iterated 16 times gave one order")
	}
}

func TestRunPrintWithoutArguments(t *testing.T) {
	results, restore, err := CaptureFd1()
	if err != nil {
		t.Fatal(err)
	}
	defer results.Close()
	res := runSrc(t, "print(\"a\")\nprint()\nprint(\"b\")\nprint()\n", nil)
	restore()
	if got, _ := base64.StdEncoding.DecodeString(res.PrintB64); string(got) != "a\n\nb\n\n" {
		t.Fatalf("print output %q", got)
	}
}

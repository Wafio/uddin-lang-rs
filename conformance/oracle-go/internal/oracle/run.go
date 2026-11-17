package oracle

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"maps"
	"slices"
	"strings"
	"time"

	uddin "github.com/bonkzero404/uddin-lang"
	interp "github.com/bonkzero404/uddin-lang/interpreter"
)

// maxPrintBytes bounds the captured print output of one run. The agent discards print, so nothing bounds it
// there; a loop that prints a 1 MiB string 100,000 times must not exhaust the memory of the host that runs the
// oracle.
const maxPrintBytes = 1 << 20

// Ctx is the request context of a run, the fields of engine.WAFRuleContext that scripts can read. Absent fields
// are empty strings, zero, no headers, no detections and no JSON body.
type Ctx struct {
	Method   string            `json:"method,omitempty"`
	Path     string            `json:"path,omitempty"`
	Query    string            `json:"query,omitempty"`
	IP       string            `json:"ip,omitempty"`
	Country  string            `json:"country,omitempty"`
	UA       string            `json:"ua,omitempty"`
	Body     string            `json:"body,omitempty"`
	Score    float64           `json:"score,omitempty"`
	Action   string            `json:"action,omitempty"`
	Headers  map[string]string `json:"headers,omitempty"`
	Detected []string          `json:"detected,omitempty"`
	// JSON is the parsed request body (waf_json). It is decoded with UseNumber, as the agent does.
	JSON json.RawMessage `json:"json,omitempty"`
	// Fields and FieldValues stand in for the resolver of GUI rule conditions behind waf_field and waf_values:
	// a table from the field name to its value, or to its list of values.
	Fields      map[string]string   `json:"fields,omitempty"`
	FieldValues map[string][]string `json:"field_values,omitempty"`
}

// vars builds the interpreter variables as engine.scriptVars does.
func (c *Ctx) vars() (map[string]any, error) {
	if c == nil {
		c = new(Ctx)
	}
	detected := slices.Clone(c.Detected)
	if detected == nil {
		detected = []string{}
	}
	headers := make(map[string]string, len(c.Headers))
	maps.Copy(headers, c.Headers)
	var body any
	if len(c.JSON) > 0 {
		dec := json.NewDecoder(bytes.NewReader(c.JSON))
		dec.UseNumber()
		if err := dec.Decode(&body); err != nil {
			return nil, fmt.Errorf("ctx.json: %w", err)
		}
	}
	fields, fieldValues := c.Fields, c.FieldValues
	return map[string]any{
		"_waf_ctx": map[string]any{
			"field":        func(f string) string { return fields[f] },
			"field_values": func(f string) []string { return fieldValues[f] },
			"json":         body,
			"method":       c.Method,
			"path":         c.Path,
			"query":        c.Query,
			"ip":           c.IP,
			"country":      c.Country,
			"ua":           c.UA,
			"headers":      headers,
			"body":         c.Body,
			"score":        c.Score,
			"action":       c.Action,
			"detected":     detected,
		},
		"method":  c.Method,
		"path":    c.Path,
		"query":   c.Query,
		"ip":      c.IP,
		"country": c.Country,
		"ua":      c.UA,
		"body":    c.Body,
		"score":   c.Score,
		"action":  c.Action,
	}, nil
}

// Outcomes of a run.
const (
	OutcomeOK       = "ok"       // the script ran to its end
	OutcomeError    = "error"    // a runtime error ended it
	OutcomeLimit    = "limit"    // a sandbox limit or the deadline stopped it
	OutcomePanic    = "panic"    // a Go panic escaped the interpreter (a bug of the Go side)
	OutcomeRejected = "rejected" // the source failed the check, so it never ran
)

// RunResult is one output line of run mode. Verdict, Limit, ErrorClass, Error and PrintB64 are always present;
// the other fields are omitted unless they apply.
type RunResult struct {
	ID      string `json:"id"`
	Outcome string `json:"outcome"`
	// Stage and Forbidden carry the check result of a rejected source.
	Stage     string   `json:"stage,omitempty"`
	Forbidden []string `json:"forbidden,omitempty"`
	// Verdict is the first verdict written by waf_return, waf_block, waf_allow or waf_log, lower case, decided as
	// engine.evalScript does and independent of how the script ended; "" when there is none.
	Verdict string `json:"verdict"`
	// Limit is the reason of a LimitError: op_limit, call_depth, nesting, value_size, value_depth, memory,
	// regex_size, regex_work, or timeout when the deadline stopped the run.
	Limit string `json:"limit"`
	// ErrorClass is type, value, name or runtime for the interpreter's own errors, limit for a LimitError, other
	// for any other error value.
	ErrorClass string `json:"error_class"`
	// Error is the error text, including its position, as err.Error() returns it.
	Error string `json:"error"`
	// PrintB64 is the captured print output, base64.
	PrintB64       string `json:"print_b64"`
	PrintTruncated bool   `json:"print_truncated,omitempty"`
	// Clock marks a source that names date_now or time_now: its output depends on the time of the run.
	Clock bool `json:"clock,omitempty"`
	// Timing marks a run that the deadline stopped: whether it ends in a timeout depends on the speed of the host.
	Timing bool `json:"timing,omitempty"`
	// OrderSensitive marks a case whose result differed between runs (Go map iteration order). The other fields
	// then hold the smallest of the results seen, by their JSON text.
	OrderSensitive bool `json:"order_sensitive,omitempty"`
}

// verdictSink keeps the first verdict written, as engine.verdictSink does.
type verdictSink struct{ v string }

func (s *verdictSink) Write(p []byte) (int, error) {
	if s.v == "" && len(p) <= 8 {
		s.v = string(p)
	}
	return len(p), nil
}

type capture struct {
	buf       []byte
	truncated bool
}

func (c *capture) append(p []byte) {
	if room := maxPrintBytes - len(c.buf); len(p) > room {
		c.truncated = true
		p = p[:room]
	}
	c.buf = append(c.buf, p...)
}

// Write takes what print writes, after whatever it wrote to descriptor 1 before (see CaptureFd1).
func (c *capture) Write(p []byte) (int, error) {
	drainFd1(c)
	c.append(p)
	return len(p), nil
}

// RunCase runs one case: the check first, then runs executions of the program, each on fresh variables, each
// under a deadline of timeout. Results that differ between the runs mark the case order sensitive.
func (p *Profile) RunCase(c Case, runs int, timeout time.Duration) (RunResult, error) {
	src, err := c.Source()
	if err != nil {
		return RunResult{}, err
	}
	prog, stage, forbidden := p.Check(src)
	if stage != "" {
		return RunResult{ID: c.ID, Outcome: OutcomeRejected, Stage: stage, Forbidden: forbidden, PrintB64: ""}, nil
	}
	seen := map[string]RunResult{}
	for range runs {
		vars, err := c.Ctx.vars()
		if err != nil {
			return RunResult{}, err
		}
		r := p.runOnce(prog, vars, timeout)
		key, _ := json.Marshal(r)
		seen[string(key)] = r
	}
	keys := slices.Sorted(maps.Keys(seen))
	res := seen[keys[0]]
	res.ID = c.ID
	res.Clock = usesClock(src)
	res.Timing = res.Limit == "timeout"
	res.OrderSensitive = len(keys) > 1
	return res, nil
}

func (p *Profile) runOnce(prog *interp.Program, vars map[string]any, timeout time.Duration) (res RunResult) {
	ctx, cancel := context.WithTimeout(context.Background(), timeout)
	defer cancel()
	verdict := new(verdictSink)
	out := new(capture)
	defer func() {
		if rec := recover(); rec != nil {
			drainFd1(out)
			res = RunResult{Outcome: OutcomePanic, Error: fmt.Sprint(rec), PrintB64: base64.StdEncoding.EncodeToString(out.buf)}
		}
	}()
	eng := uddin.NewSandboxed(p.sandbox.WithContext(ctx))
	eng.SetStdout(out)
	eng.SetVerdict(verdict)
	eng.SetVariables(vars)
	drainFd1(new(capture)) // nothing of an earlier run may leak in
	_, err := eng.ExecuteProgram(prog)
	drainFd1(out)

	res = RunResult{
		Outcome:        OutcomeOK,
		PrintB64:       base64.StdEncoding.EncodeToString(out.buf),
		PrintTruncated: out.truncated,
	}
	if v := strings.ToUpper(verdict.v); v == "BLOCK" || v == "ALLOW" || v == "LOG" {
		res.Verdict = strings.ToLower(v)
	}
	if err == nil {
		return res
	}
	res.Error = err.Error()
	var (
		limit interp.LimitError
		te    interp.TypeError
		ve    interp.ValueError
		ne    interp.NameError
		re    interp.RuntimeError
	)
	switch {
	case errors.As(err, &limit):
		res.Outcome, res.ErrorClass, res.Limit = OutcomeLimit, "limit", limit.Reason
		if limit.Reason == interp.LimitCanceled && errors.Is(ctx.Err(), context.DeadlineExceeded) {
			res.Limit = "timeout"
		}
	case errors.As(err, &te):
		res.Outcome, res.ErrorClass = OutcomeError, "type"
	case errors.As(err, &ve):
		res.Outcome, res.ErrorClass = OutcomeError, "value"
	case errors.As(err, &ne):
		res.Outcome, res.ErrorClass = OutcomeError, "name"
	case errors.As(err, &re):
		res.Outcome, res.ErrorClass = OutcomeError, "runtime"
	default:
		res.Outcome, res.ErrorClass = OutcomeError, "other"
	}
	return res
}

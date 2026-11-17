// Package oracle runs uddin sources through the Go interpreter the way the Wafio control plane and agent do, so
// that another implementation can be compared with it. It imports nothing from Wafio: the builtin allowlist and
// the limits come from a profile file.
package oracle

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"time"

	interp "github.com/bonkzero404/uddin-lang/interpreter"
)

// Profile is the sandbox profile: the allowed builtin names and the limits a run is held to.
type Profile struct {
	Builtins []string `json:"builtins"`
	Limits   struct {
		MaxScriptBytes     int `json:"max_script_bytes"`
		MaxOps             int `json:"max_ops"`
		MaxCallDepth       int `json:"max_call_depth"`
		MaxNesting         int `json:"max_nesting"`
		MaxValueLen        int `json:"max_value_len"`
		MaxAllocBytes      int `json:"max_alloc_bytes"`
		MaxRegexProgram    int `json:"max_regex_program"`
		MaxRegexWork       int `json:"max_regex_work"`
		PerScriptTimeoutMs int `json:"per_script_timeout_ms"`
		// The total over all rules of one request has no meaning for one script; it is read so that a profile
		// with a missing key is refused.
		TotalTimeoutMs int `json:"total_timeout_ms"`
	} `json:"limits"`

	sandbox *interp.Sandbox
	allowed map[string]bool
}

// LoadProfile reads and validates a profile. An unknown key, a missing key or a zero limit is an error, so that a
// profile that grew a limit this oracle does not apply cannot go unnoticed.
func LoadProfile(path string) (*Profile, error) {
	raw, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.DisallowUnknownFields()
	p := new(Profile)
	if err := dec.Decode(p); err != nil {
		return nil, fmt.Errorf("profile %s: %w", path, err)
	}
	l := p.Limits
	for name, v := range map[string]int{
		"max_script_bytes": l.MaxScriptBytes, "max_ops": l.MaxOps, "max_call_depth": l.MaxCallDepth,
		"max_nesting": l.MaxNesting, "max_value_len": l.MaxValueLen, "max_alloc_bytes": l.MaxAllocBytes,
		"max_regex_program": l.MaxRegexProgram, "max_regex_work": l.MaxRegexWork,
		"per_script_timeout_ms": l.PerScriptTimeoutMs, "total_timeout_ms": l.TotalTimeoutMs,
	} {
		if v <= 0 {
			return nil, fmt.Errorf("profile %s: limit %s is missing or not positive", path, name)
		}
	}
	if len(p.Builtins) == 0 {
		return nil, fmt.Errorf("profile %s: no builtins", path)
	}
	// Built the way engine/script_sandbox.go builds scriptSandbox.
	sb := interp.NewSandbox(p.Builtins)
	sb.MaxOps = l.MaxOps
	sb.MaxCallDepth = l.MaxCallDepth
	sb.MaxNesting = l.MaxNesting
	sb.MaxValueLen = l.MaxValueLen
	sb.MaxAllocBytes = l.MaxAllocBytes
	sb.MaxRegexProgram = l.MaxRegexProgram
	sb.MaxRegexWork = l.MaxRegexWork
	p.sandbox = sb
	p.allowed = sb.Allowed
	return p, nil
}

// ScriptTimeout is the per-script deadline of the profile.
func (p *Profile) ScriptTimeout() time.Duration {
	return time.Duration(p.Limits.PerScriptTimeoutMs) * time.Millisecond
}

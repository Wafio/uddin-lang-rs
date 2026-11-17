package oracle

import (
	"errors"

	interp "github.com/bonkzero404/uddin-lang/interpreter"
)

// Stages of the check, in the order they run.
const (
	StageSize      = "size"
	StageTokenize  = "tokenize"
	StageParse     = "parse"
	StageAllowlist = "allowlist"
)

// CheckResult is one output line of check mode.
type CheckResult struct {
	ID        string   `json:"id"`
	OK        bool     `json:"ok"`
	Stage     *string  `json:"stage"`
	Forbidden []string `json:"forbidden"`
}

// Check applies the save-time checks of engine.CompileScript: the size bound, ParseProgram, then
// ForbiddenReferences against the allowlist. It returns the program, or the stage that rejected the source and
// the forbidden names (only for StageAllowlist). The empty-script rule of CompileScript is a policy of the
// embedder and is not applied: ParseProgram decides.
func (p *Profile) Check(src []byte) (prog *interp.Program, stage string, forbidden []string) {
	if len(src) > p.Limits.MaxScriptBytes {
		return nil, StageSize, nil
	}
	prog, err := interp.ParseProgram(src)
	if err != nil {
		return nil, parseStage(src, err), nil
	}
	if names := interp.ForbiddenReferences(prog, p.allowed); len(names) > 0 {
		return nil, StageAllowlist, names
	}
	return prog, "", nil
}

// CheckCase builds the output line for one case.
func (p *Profile) CheckCase(id string, src []byte) CheckResult {
	_, stage, forbidden := p.Check(src)
	res := CheckResult{ID: id, OK: stage == "", Forbidden: []string{}}
	if stage != "" {
		res.Stage = &stage
		res.Forbidden = append(res.Forbidden, forbidden...)
	}
	return res
}

// parseStage tells a tokenizer error from a parser error. The parser pulls tokens one at a time and reports an
// ILLEGAL token as its own error, so the failure is a tokenizer one when it is the first ILLEGAL token of the
// stream and carries that token's position and message.
func parseStage(src []byte, err error) string {
	var pe interp.Error
	if !errors.As(err, &pe) {
		return StageParse
	}
	t := interp.NewTokenizer(src)
	for {
		pos, tok, val := t.Next()
		switch tok {
		case interp.ILLEGAL:
			if pos == pe.Position && val == pe.Message {
				return StageTokenize
			}
			return StageParse
		case interp.EOF:
			return StageParse
		}
	}
}

// usesClock reports whether the source names a builtin that reads the clock.
func usesClock(src []byte) bool {
	t := interp.NewTokenizer(src)
	for {
		_, tok, val := t.Next()
		switch tok {
		case interp.ILLEGAL, interp.EOF:
			return false
		case interp.NAME:
			if val == "date_now" || val == "time_now" {
				return true
			}
		}
	}
}

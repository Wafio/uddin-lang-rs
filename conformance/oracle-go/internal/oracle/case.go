package oracle

import (
	"bufio"
	"bytes"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"unicode/utf8"
)

// Case is one input line: a source in src (UTF-8) or src_b64 (any bytes), and for run mode an optional request
// context.
type Case struct {
	ID     string  `json:"id"`
	Src    *string `json:"src,omitempty"`
	SrcB64 *string `json:"src_b64,omitempty"`
	Ctx    *Ctx    `json:"ctx,omitempty"`
}

// Source returns the source bytes of the case.
func (c *Case) Source() ([]byte, error) {
	switch {
	case c.Src != nil && c.SrcB64 != nil:
		return nil, errors.New("both src and src_b64 are set")
	case c.Src != nil:
		return []byte(*c.Src), nil
	case c.SrcB64 != nil:
		return base64.StdEncoding.DecodeString(*c.SrcB64)
	}
	return nil, errors.New("neither src nor src_b64 is set")
}

// NewCase builds a case from source bytes, using src_b64 when the bytes are not valid UTF-8.
func NewCase(id string, src []byte, ctx *Ctx) Case {
	c := Case{ID: id, Ctx: ctx}
	if utf8.Valid(src) {
		s := string(src)
		c.Src = &s
	} else {
		b := base64.StdEncoding.EncodeToString(src)
		c.SrcB64 = &b
	}
	return c
}

// ReadCases calls fn for every line of r, in order. A malformed line stops the read with its line number.
func ReadCases(r io.Reader, fn func(Case) error) error {
	br := bufio.NewReaderSize(r, 1<<20)
	for line := 1; ; line++ {
		raw, err := br.ReadBytes('\n')
		if len(raw) > 0 {
			var c Case
			dec := json.NewDecoder(bytes.NewReader(raw))
			dec.DisallowUnknownFields()
			if derr := dec.Decode(&c); derr != nil {
				if errors.Is(derr, io.EOF) {
					derr = errors.New("empty line")
				}
				return fmt.Errorf("line %d: %w", line, derr)
			}
			if dec.More() {
				return fmt.Errorf("line %d: more than one JSON value", line)
			}
			if err := fn(c); err != nil {
				return fmt.Errorf("line %d (id %q): %w", line, c.ID, err)
			}
		}
		if err == io.EOF {
			return nil
		}
		if err != nil {
			return err
		}
	}
}

// WriteLine writes v as one JSON line without HTML escaping.
func WriteLine(w io.Writer, v any) error {
	var b bytes.Buffer
	enc := json.NewEncoder(&b)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(v); err != nil {
		return err
	}
	_, err := w.Write(b.Bytes())
	return err
}

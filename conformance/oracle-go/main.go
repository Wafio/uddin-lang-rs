// Command oracle-go runs JSONL cases through the Go uddin interpreter. See conformance/README.md for the two
// contracts.
//
//	oracle-go check [-profile FILE] < cases.jsonl > results.jsonl
//	oracle-go run   [-profile FILE] [-runs N] [-deadline-ms N] < cases.jsonl > results.jsonl
package main

import (
	"bufio"
	"flag"
	"fmt"
	"os"
	"time"

	"oracle-go/internal/oracle"
)

func main() {
	if err := run(os.Args[1:]); err != nil {
		fmt.Fprintln(os.Stderr, "oracle-go:", err)
		os.Exit(2)
	}
}

func run(args []string) error {
	if len(args) == 0 || (args[0] != "check" && args[0] != "run") {
		return fmt.Errorf("usage: oracle-go check|run [flags] < cases.jsonl > results.jsonl")
	}
	mode := args[0]
	fs := flag.NewFlagSet(mode, flag.ContinueOnError)
	profilePath := fs.String("profile", "../../profile/sandbox-profile.json", "sandbox profile")
	runs := fs.Int("runs", 8, "run mode: executions per case, to detect order-sensitive results")
	deadlineMs := fs.Int("deadline-ms", 0, "run mode: deadline of one execution in ms (0: per_script_timeout_ms of the profile)")
	if err := fs.Parse(args[1:]); err != nil {
		return err
	}
	if *runs < 1 || *deadlineMs < 0 {
		return fmt.Errorf("-runs must be at least 1 and -deadline-ms not negative")
	}
	prof, err := oracle.LoadProfile(*profilePath)
	if err != nil {
		return err
	}
	timeout := prof.ScriptTimeout()
	if *deadlineMs > 0 {
		timeout = time.Duration(*deadlineMs) * time.Millisecond
	}
	stdout := os.Stdout
	if mode == "run" {
		// The interpreter writes some output to descriptor 1 itself; see oracle.CaptureFd1.
		f, restore, err := oracle.CaptureFd1()
		if err != nil {
			return err
		}
		defer restore()
		stdout = f
	}
	out := bufio.NewWriter(stdout)
	err = oracle.ReadCases(os.Stdin, func(c oracle.Case) error {
		if mode == "run" {
			res, err := prof.RunCase(c, *runs, timeout)
			if err != nil {
				return err
			}
			return oracle.WriteLine(out, res)
		}
		src, err := c.Source()
		if err != nil {
			return err
		}
		return oracle.WriteLine(out, prof.CheckCase(c.ID, src))
	})
	if ferr := out.Flush(); err == nil {
		err = ferr
	}
	return err
}

# Conformance

The Go interpreter is the reference for this repo. `oracle-go` runs sources through it the way the Wafio control
plane and agent do, and `corpus/` holds sources together with the results Go gave. Another implementation is
correct when it gives the same results.

    conformance/
      oracle-go/    Go module. fork/ is a copy of the Go fork, main.go the command, internal/oracle the logic,
                    cmd/extract the corpus extractor
      corpus/       check/ and run/: <group>.cases.jsonl (input) and <group>.go.jsonl (what Go answered)
      corpus.sh     records or verifies the Go results

`oracle-go` imports nothing from the Wafio repositories. The builtin allowlist and the limits are read from
`../../profile/sandbox-profile.json` (flag `-profile`). `oracle-go/SOURCE` names the fork commit that `fork/` copies.

## Build and use

    cd conformance/oracle-go
    GOPROXY=off go build -o oracle-go .
    ./oracle-go check < cases.jsonl > results.jsonl
    ./oracle-go run   < cases.jsonl > results.jsonl

The fork is not fetched from anywhere: `go.mod` replaces it with `./fork`. The other modules come from the module
cache, pinned by `go.sum`; `GOPROXY=off` turns any fetch into a build failure, and `scripts/ci-local.sh` builds
that way. The toolchain is the one `go.mod` names. The oracle runs on Linux only (see `print()` below).

## Input

One case per line, in order, UTF-8 JSON:

    {"id": "name", "src": "x = 1\n"}
    {"id": "name", "src_b64": "eCA9IDEK"}

`src_b64` carries sources that are not valid UTF-8. Exactly one of the two is required. Unknown keys, a blank line
and a malformed line stop the oracle with a message that names the line and exit status 2. `check` ignores a
`ctx` key.

## Check mode

Output, one line per case, in input order:

    {"id": "name", "ok": true, "stage": null, "forbidden": []}

`stage` names the first check that rejected the source and is `null` when `ok` is true:

| stage | check |
|---|---|
| `size` | the source is longer than `max_script_bytes` (64 KiB) |
| `tokenize` | `ParseProgram` failed at an illegal token of the tokenizer, such as an unterminated string or a malformed number |
| `parse` | `ParseProgram` failed in any other way, nesting deeper than 1,000 levels included |
| `allowlist` | `ForbiddenReferences` found names that are builtins of the fork but not on the allowlist, or an `import` |

`forbidden` is always an array. It holds the sorted, unique names the allowlist check refused, `import` included,
and is empty unless `stage` is `allowlist`. Any identifier spelled like a forbidden builtin counts, one the
program assigns itself too.

The order is that of `engine.CompileScript`: size, `ParseProgram`, `ForbiddenReferences`. The one rule of
`CompileScript` that is not applied is the refusal of an empty or blank script: the check has no stage for it, and
`ParseProgram` accepts such a source. A comparison of two implementations looks at `ok` and `forbidden`; a
difference in `stage` alone is worth reporting, not failing.

## Run mode

    oracle-go run [-profile FILE] [-runs 8] [-deadline-ms 0]

A case runs as `engine/script_sandbox.go` and `engine/custom_rules.go` run a script rule: the source is checked as
above, then executed by `NewSandboxed` with the limits of the profile, and the verdicts of `waf_return`,
`waf_block`, `waf_allow` and `waf_log` go to a sink that keeps the first one written. Print output is captured,
where the agent discards it. A source that fails the check is not run.

The optional `ctx` object sets the request. Absent keys are an empty string, 0, no headers, no detections, no
JSON body, no fields.

| key | type | used by |
|---|---|---|
| `method`, `path`, `query`, `ip`, `country`, `ua`, `body`, `action` | string | the variables of the same name, `waf_query`, `waf_body_contains`, `waf_path_match`, `waf_cidr_match`, `waf_action` |
| `score` | number | the variable `score`, `waf_score` |
| `headers` | object of strings | `waf_header` |
| `detected` | array of strings | `waf_detected`, `waf_detected_any`, `waf_detected_list` |
| `json` | any JSON value | `waf_json`. Decoded as the agent does, with exact integers staying integers |
| `fields` | object of strings | `waf_field(name)` returns the entry of that name, `""` when absent |
| `field_values` | object of arrays of strings | `waf_values(name)` returns the entry of that name, `[]` when absent |

`fields` and `field_values` replace the resolver of GUI rule conditions in the agent, which this module does not
carry. The variables are built as `engine.scriptVars` builds them: `method path query ip country ua body score
action` and `_waf_ctx`. `_waf_ctx` is a Go `map[string]any`, which is not a uddin map: no script operation can use
it, scalar entries included. A subscript or `len` is a type error, `for (k in _waf_ctx)` fails inside the
interpreter, and `typeof` says `unknown`. Only the `waf_*` builtins read it, and they find it through the
variable of that name, so a script that assigns its own `_waf_ctx` changes what they see.

Output, one line per case, in input order:

    {"id": "name", "outcome": "ok", "verdict": "", "limit": "", "error_class": "", "error": "", "print_b64": ""}

| field | meaning |
|---|---|
| `outcome` | `ok` the script ran to its end; `error` a runtime error ended it; `limit` a sandbox limit or the deadline ended it; `panic` a Go panic escaped the interpreter (a bug of the Go side); `rejected` the source failed the check |
| `stage`, `forbidden` | only for `rejected`: the result of the check, as in check mode |
| `verdict` | `block`, `allow`, `log` or `""`: the first verdict written, decided as `evalScript` decides it, whatever way the script ended |
| `limit` | the reason of a stopped run: `op_limit`, `call_depth`, `nesting`, `value_size`, `value_depth`, `memory`, `regex_size`, `regex_work`, or `timeout` when the deadline fired |
| `error_class` | `type`, `value`, `name` or `runtime` for the interpreter's error kinds, `limit` for a limit, `other` for anything else; `""` when the run did not fail |
| `error` | `err.Error()` of the run, with its position; `""` when it did not fail |
| `print_b64` | the captured print output, base64 |
| `print_truncated` | only when true: the output passed 1 MiB and the rest was dropped |
| `clock` | only when true: the source names `date_now` or `time_now`, so its output depends on the time of the run |
| `timing` | only when true: the deadline stopped the run (`limit` is `timeout`). Whether a script gets that far depends on the speed of the host, so compare such a case on `outcome` and `limit` only |
| `order_sensitive` | only when true: the result differed between runs (Go map order). The other fields then hold the smallest result seen, by its JSON text. Compare such a case as a set |

A verdict at the top level of a script ends it with the runtime error "can't return at top level", and the
verdict still stands: a case can be `error` and have verdict `block`.

Each case runs `-runs` times (default 8), each time with fresh variables, so that a script that changes `_waf_ctx`
cannot influence its next run. One script runs per case, so the cap on runs in flight and the budget of a whole
request are not modelled.

`-deadline-ms` is the deadline of one execution. The default 0 means `per_script_timeout_ms` of the profile
(5 ms), as in production. A run near the op limit can take longer than that on a loaded host, which turns an
`op_limit` into a `timeout`; the corpus is therefore recorded with 30000. The `timing` group is the exception:
its scripts only the deadline can stop, so it is recorded with 5.

### print()

The fork's `print()` with no argument writes its newline to file descriptor 1 with a raw system call, even in a
sandbox that has its own stdout. In production it goes to the agent's own stdout. Here it would land in the
result stream, so run mode points descriptor 1 at a pipe and puts what it reads, in order, into the captured
output. The captured output is therefore what `print()` is meant to produce. At most 1 MiB written that way
between two reads is kept.

## Corpus

`corpus/check/<group>.cases.jsonl` holds every source of a group, `corpus/run/<group>.cases.jsonl` those that
pass the check, each next to a `<group>.go.jsonl` with the results above. `corpus/SOURCE` names the inputs.

| group | source | check | run |
|---|---|---|---|
| `fork-tests` | the `*_test.go` files of the fork | 340 | 281 |
| `engine-tests` | 7 test files of the control plane's engine package that call `EvalScriptRules` or `CompileScript`; the eighth such file is the benchmark file of the next row | 101 | 82 |
| `bench` | the 6 scripts of the engine benchmark, each with its request context | 6 | 6 |
| `examples` | the 53 `.din` files of the fork's `examples/` and `example_usages/` | 53 | 28 |
| `waf-ctx` | written by hand: every `waf_*` builtin and request variable with a request context, from the tests of `fn_wafio_test.go` and the probes of the control plane's script golden | 213 | 213 |
| `limits` | written by hand: the nesting limit at its boundary, alone and across calls, and `catch` around it | 26 | 26 |
| `timing` | written by hand: scripts only the 5 ms deadline stops | 3 | 3 |

No test of the fork or the control plane runs a script with a request context in a form the extractor can fold:
`fn_wafio_test.go` calls the builtins from Go, and the control plane builds its contexts and scripts in helper
functions. The three hand-written groups (`cmd/extract/handwritten.go`) fill that gap, with the context on the
case line. The cases that print instead of deciding show the value a builtin returned, error text included.

`cmd/extract` reads the Go files with `go/parser` and folds the string expressions it can evaluate without
running them: literals, `+`, `strings.Repeat`, `fmt.Sprintf` with `%s %q %d %v`, and names bound once to such an
expression. A folded string is kept when it looks like a program (it has a line break, or a line that starts with
a call, an assignment or a statement keyword, and it is not a format string), is at most 256 KiB, and has not
been seen in the group. The nesting tests build sources of megabytes and are dropped by the size rule. Some kept
strings are not scripts, such as query strings with an `=`; they are still valid inputs for the check. A
script built by a helper function is not folded. The ids are `<source>/<file>:<line>:<column>`, or the path of the
`.din` file, or `bench/<name>`.

Recorded results are reproducible except for the lines marked `order_sensitive`, `clock` or `timing`.
`corpus.sh verify` runs the oracle again and compares line by line, skipping a line marked in either file.

### Regenerating

The oracle needs Go on Linux. Stage the inputs into the ignored
`conformance/.stage/`, then extract and record:

    rsync -a --exclude .git <fork checkout at the commit of oracle-go/SOURCE>/ conformance/.stage/fork/
    rsync -a --include '*_test.go' --exclude '*' <control plane>/internal/engine/ conformance/.stage/engine/
    (cd conformance/oracle-go && go run ./cmd/extract -fork ../.stage/fork -engine ../.stage/engine)
    conformance/corpus.sh record

Update `corpus/SOURCE` with the commits used.

## Run comparison

`runs.sh` runs every group of the run corpus through `uddin run` and compares each result with the one Go gave
(`corpus/run/<group>.go.jsonl`); it needs cargo, not go. `parity compare-run` pairs two result files by line and
compares `outcome`, `verdict`, `limit`, `error_class`, `error`, `print_b64` and `print_truncated`, so error text and
its position, printed output and the point where a limit stops a script must all be equal. Two things are not
compared byte for byte:

- a case Go gave different results for in its runs (map iteration order) is compared on outcome, limit and error
  class, and on the number of output lines;
- a case marked `clock` or `timing` is compared on outcome and limit.

A case that names a builtin this build does not carry is counted as not ported and left out. Every builtin of the
profile is carried, so the count is 0 and the gate would show a build that lost one.

**Intended differences.** Where this implementation fixes a defect of the reference (see "Differences from the
reference" in the top README), a case can give another result on purpose. `corpus/run/<group>.intended.jsonl`
has one line for each such case: the result line this implementation must give, and `defects`, the rows of the
README table the difference belongs to. `parity compare-run --intended FILE --readme README.md` then requires, for
the cases in the file, that the reference differs from the expectation (a line that no longer differs is stale and
fails), that this implementation gives the expectation, and that every name in `defects` is a row of the table.
Any other difference fails. The report says how many cases are equal, how many are intended differences, and how
many of them each row accounts for. To record a new difference, run `runs.sh` with `WORKDIR` set, read the
`<group>.mismatch.jsonl` pairs, and copy the Rust line of each case you have decided is a fix into the file.

`runs-gen.sh SEED COUNT` does the same for programs from `parity gen-run`, which draws typed expressions and every
construct of the core language, except that it writes `break` and `continue` only in a loop and outside a `try`
body, and no `return` inside `try`: there the reference and this implementation differ on purpose and nothing records
an expectation for a generated program. Instead the comparison runs with `--auto-known`, which counts as known, and
reports apart, a case where the reference ended in a fault of its own host language (the texts it gave for a `for`
over a number, a bad `substr` range and the like), printed `-` for the most negative integer, took the remainder by a
float that truncates to zero, or where a longer error text moved the place a run stopped at the value size limit.
Each program runs once under the production profile and once under a small one (300 operations, 4 calls, 6 blocks,
64-byte values, 2 KiB of memory), so that each limit is reached by ordinary programs. Go runs each generated case
three times.

The `lang` group (language semantics: scopes, unwinding, operators over every pair of kinds of value, number and
text formats) and the `core-limits` group (each limit at, below and above its boundary) are written in
`cmd/extract/corelang.go`; `go run ./cmd/extract -only lang,core-limits` rewrites their cases and
`corpus.sh record lang core-limits` their results.

The groups `lib-<name>` hold the scripts of one group of library builtins. Each lives in its own file
`cmd/extract/lib_<name>.go`, which adds the group to `libRegistry` in an `init` function, so a new group touches no
other file; `go run ./cmd/extract -only lib-<name>` writes its cases and `corpus.sh record lib-<name>` records
the results. A case that names a builtin this build does not carry is reported as not yet ported until it does.
`lib-shared` covers what every builtin relies on: an error returned as a value (`date_add` returns them), and the
guards, the reservation and the charge for fresh elements around `range`. `crates/uddin/tests/shared_corpus.rs`
replays its recorded results against the machine with a test version of the two builtins, so these checks run
before the real ones are carried.

The groups `lib-lists`, `lib-sort`, `lib-collections` and `lib-json` measure memory without a case for each
boundary by hand. A `ledger` script runs its setup, spends what is left of the 4 MiB budget in strings of 30,000
bytes and then in steps of 8 (the integer `len` returns), and prints how many small steps it made: that count is
what the setup charged, to eight bytes, so one array element more or less in a growth rule shows. An `edge`
script leaves the budget short of one operation by `d` bytes, for `d` from -24 to 24, so the operation fits on
one side of zero and stops the run on the other. A `fill` script repeats an operation until the limit and prints
its progress. The extractor takes the capacities a list grows through from the `append` of the Go toolchain that
runs it, and the scans of the operation limit (`ops/`) from the element the limit fires at. A case that names
a builtin of another group is not compared until that group is carried.

## Acceptance comparison

`parity/` is a small Rust tool (workspace member `uddin-parity`, nothing in `crates/` depends on it) that makes
sources and compares result files. `parity.sh` runs the whole comparison: it makes sources, runs `oracle-go
check` and `uddin check` over them, and compares `ok` and `forbidden`. A difference in `stage` alone is counted
and shown, not failed.

    conformance/parity.sh SEED COUNT [WORKDIR]
    PARITY_ENUM=4 conformance/parity.sh 0 0 [WORKDIR]
    cargo run -p uddin-parity -- gen --corpus conformance/corpus/check --seed 1 --start 0 --count 10
    cargo run -p uddin-parity -- compare --go go.jsonl --rust rust.jsonl --cases cases.jsonl
    cargo run -p uddin-parity -- seeds --corpus conformance/corpus/check --out DIR

Case `i` of a run depends only on the seed and `i`, and its id is `<kind>/<seed>/<i>`, so `gen --seed S --start i
--count 1` rebuilds it. The kinds:

| kind | source |
|---|---|
| `g`, `gb` | a program built from the statement and expression rules with typed holes (name, number, string, expression, block) filled from pools; `gb` has byte damage |
| `gd` | the same, with lexeme-level damage |
| `cb`, `ct` | a corpus source with byte-level or lexeme-level damage |
| `sp` | the start of one corpus source joined to the end of another |
| `lx` | numbers, strings, comments, operator runs and special characters |
| `ne`, `bd`, `ch` | nesting around the 1,000 level limit: mixed constructs, one statement and one expression construct aimed at the limit, and long chains of operators, calls, subscripts and `elif` |
| `sz` | length around the 64 KiB limit, padded with a comment, spaces, a string, a name or bytes that are not UTF-8 |
| `al` | refused and allowed builtin names in every position the allowlist check looks at |

`PARITY_ENUM=LEN` replaces the generated sources by every sequence of LEN tokens over one of three alphabets
(`full` has 58 tokens: the keywords, the operators and brackets, a few names and literals; `core` has 25 and
`blocks` 14, the keywords and brackets that make a statement nest; `PARITY_ALPHABET` picks one, `parity size`
says how many sequences there are).

The name pools come from `parity/builtins.txt` (every name of the Go builtin table) and the pinned profile. The
tool reads files by design, so `parity/clippy.toml` lifts the file ban of the repo-level one.

One class of difference in `stage` is known: a source that is not valid UTF-8 and has a syntax error before the
bad byte. Go parses up to the error and reports `parse`; this implementation checks the encoding first and
reports `tokenize`. Both refuse the source.

`scripts/ci-local.sh` runs `uddin check` over every corpus group against the recorded Go results, and 20,000
generated sources from seed 1.

# uddin-lang-rs

A Rust implementation of the uddin script language, ported from a Go interpreter, for running script rules inside
the Wafio WAF agent without a separate process. It is a library and a small command line; it runs wherever its
embedder runs.

Status: scripts compile and run under the sandbox limits of the profile, with every builtin of the profile, the
seventeen `waf_*` builtins included (crate `uddin-waf`). `uddin check` is complete. Where the Go interpreter has a
defect, this one fixes it (see "Differences from the reference").

## Place in the platform

| Repo | Contents |
|---|---|
| wafio-cp-backend | Go control plane, migrations, `shared/` module |
| wafio-cp-frontend | SvelteKit dashboard |
| wafio-agent-waf | Rust WAF agent |
| wafio-agent-host | Go host agent, BPF C |
| wafio-portal | wafio.cloud site and blog (SvelteKit), portal API (Go) (private) |
| **uddin-lang-rs (this repo)** | Rust uddin interpreter |
| wafio-release | installers, release bundle layout, release and publish scripts (private) |
| wafio-docs | skills, role agents, specs, findings, plans, ops notes (private) |

No crate in this repo depends on a Wafio repository. Wafio facts enter only through `profile/`, a pinned copy of the
script sandbox profile that wafio-cp-backend exports (`shared/script/profile.json`).

## Requirements

- Rust 1.97.1, pinned by `channel` in `rust-toolchain.toml` (with `clippy` and `rustfmt`); the workspace uses
  edition 2024. rustup installs the pinned toolchain on first use.
- Linux for the gate and for the oracle script. The Go oracle runs on Linux only (see `conformance/README.md`).
- No Go for the gate (`scripts/ci-local.sh`). Only the oracle script, `scripts/ci-oracle.sh`, needs it: the
  toolchain named in `conformance/oracle-go/go.mod` (`go 1.26.0`, `toolchain go1.26.11`).
- For the short fuzz session of the gate and for fuzzing by hand: a nightly toolchain and `cargo-fuzz` (see
  `fuzz/README.md`); the gate skips the session when they are missing.
- For `benches/measure.sh`, which is not part of the gate: valgrind (cachegrind) and Go.

## Build

    cargo build --locked --release -p uddin-cli

The workspace has no build step beyond cargo.

## Test

On a Linux host with `rustup`, from the repo root:

    scripts/ci-local.sh

This is the gate, and it needs no Go. It runs in the time zone the corpus was recorded in (`TZ=Asia/Jakarta`),
because the date builtins read the zone of the host. For `cargo test` by hand in another zone, set
`TZ=Asia/Jakarta` too.

It checks that no agent material is tracked, then runs `cargo fmt --all --check`, `cargo clippy -D warnings` and
`cargo test`, compares `uddin check` with the recorded Go results of every corpus group, runs every script of the
run corpus through `uddin run` (with the `waf_*` builtins) and compares it with the recorded Go result and the
intended differences of its group, and fuzzes the machine for a short time when a nightly toolchain and
`cargo-fuzz` are there (see `conformance/README.md` and `fuzz/README.md`). In a copy of the tree without `.git`
the gate skips the agent-material check; run `scripts/check-ai.sh` in the checkout then.

The steps that run the Go reference live are a second script, and they need Go:

    scripts/ci-oracle.sh

It builds, formats, vets and tests the oracle in `conformance/oracle-go`, checks that the oracle still gives the
recorded Go results of the corpus, compares `uddin check` with the oracle on 20,000 generated sources, and
compares runs of 10,000 generated programs under the production limits and under a small set. Run it when a
change touches `crates/`, `conformance/`, `profile/`, `Cargo.toml`, `Cargo.lock` or `rust-toolchain.toml`. CI
runs it in `.github/workflows/oracle.yml` on such changes (and on changes to that workflow), once a week and on
demand. It sets the same time zone as the gate.

`fuzz/` holds four cargo-fuzz targets: `lexer`, `parser`, `check` and `vm`. The gate runs `vm` for `FUZZ_SECONDS`
(default 30) when a nightly toolchain and `cargo-fuzz` are there; the other three run by hand (see `fuzz/README.md`).

## Run

    uddin check < cases.jsonl > results.jsonl
    uddin run [-deadline-ms N] [-limit NAME=N]... < cases.jsonl > results.jsonl

Input is one JSON object per line: `{"id": "a", "src": "x = 1"}`, or `{"id": "a", "src_b64": "..."}` for a source
that is not valid UTF-8 (standard base64 with padding). `run` also reads an optional `ctx` object that describes the
request: the strings `method`, `path`, `query`, `ip`, `country`, `ua`, `body` and `action` and the number `score`
become the variables of the same names (an absent one is an empty string or 0), and `headers` (an object of
strings), `detected` (an array of strings), `json` (any value), `fields` (an object of strings) and `field_values`
(an object of arrays of strings) are what the `waf_*` builtins read. Output is one line per case, in input order.

`check` writes `{"id":"a","ok":true,"stage":null,"forbidden":[]}`. A refused source gets `ok: false` and a
`stage`, the first check that refused it: `size` (over the profile's `max_script_bytes`, checked before anything
is parsed), `tokenize`, `parse` (nesting past 1,000 levels is one) or `allowlist`. For `allowlist`, `forbidden`
holds the refused builtin names, `import` included, sorted and without repeats. A blank source passes: the host
decides what an empty rule means.

`run` checks the source the same way, compiles it and runs it under the limits of the profile, and writes
`{"id","outcome","verdict","limit","error_class","error","print_b64"}`:

| field | meaning |
|---|---|
| `outcome` | `ok`: the script ran to its end. `error`: a runtime error ended it. `limit`: a sandbox limit or the deadline ended it. `panic`: the interpreter failed and the failure was caught. `rejected`: the check refused the source (`stage` and `forbidden` as in `check`), or the compiler did (`stage` is `compile` and `error` says where a `break` or `continue` stands outside a loop) |
| `limit` | `op_limit`, `call_depth`, `nesting`, `value_size`, `value_depth`, `memory`, `regex_size`, `regex_work`, or `timeout` for the deadline |
| `error_class` | `type`, `value`, `name` or `runtime`; `limit` for a limit; empty when the run did not fail |
| `error` | the error text with its position, `L:C: execution stopped: <reason>` for a limit |
| `print_b64` | what `print` wrote, base64, cut at 1 MiB (`print_truncated` is then true) |

`verdict` is the first decision the script wrote (`allow`, `log` or `block`), or empty, whatever way the run ended.
`-deadline-ms` replaces the profile's per-script timeout
(0 means none) and `-limit` one of `max_ops`, `max_call_depth`, `max_nesting`, `max_value_len` and
`max_alloc_bytes`.

The grammar, the nesting limit, the allowlist rule, the semantics and the text of every error follow the Go
interpreter this port is based on, except for the defects listed below.

Any other command prints a usage line and exits with status 2. A failure while reading cases or writing results
exits with status 1.

## Embedding

    let profile = uddin::Profile::embedded();
    let script = uddin::Script::compile(src, &profile)?;          // checks the source, then compiles it
    let mut runner = uddin::Runner::new();                        // one per thread; keeps its memory between runs
    let vars = uddin::RequestVars { method: b"GET", path: b"/", ..Default::default() };
    match runner.run(&script, &vars, &profile.limits()) {
        uddin::Outcome::Ok => {}
        uddin::Outcome::Error { class, text } => { /* a runtime error of the script */ }
        uddin::Outcome::Limit { reason, text } => { /* the script hit a limit */ }
    }

A `Script` is immutable and can be shared by every thread. A `Runner` is one thread's state: its vectors keep their
capacity from run to run, and it drops them when a run grew them past 256 KiB. `Limits` has the limits of the
table below and `capture_print`, which keeps what `print` writes for `Runner::printed`. This `Script::compile`
knows the builtins of the standard library only; a script that names a `waf_*` builtin is refused as `unported`.

### Script rules for the WAF agent (`uddin-waf`)

The agent depends on `uddin-waf` alone (it re-exports `Profile`, `Limits`, `LimitReason`, `Verdict`, `Json` and
`RequestContext`).

Types:

- `WafScript`: a rule that passed the check and compiled. `WafScript::compile(src, &profile)` fails with
  `CompileFailure` when the source is too large, does not parse, names a builtin outside the profile, or has a
  `break` or `continue` outside a loop of its function. The agent drops that rule and counts it.
- `RequestContext`: the request, as a trait of borrowed accessors (method, path, query, ip, country, ua, body, score,
  action, headers, detected categories, the JSON body, and the two resolvers behind `waf_field` and `waf_values`).
  The agent implements it over its own request type, so nothing is copied before a script asks. `RequestView<'a>`
  is a ready implementation made of slices; `parse_json(bytes)` turns a JSON text into the `Json` it hands out.
- `WafRunner`: one thread's state. `run(&script, &ctx, &limits, until) -> Run`, where `until` is an `Option<Instant>`.
- `Run { verdict: Option<Verdict>, end: End }`, with `End` one of `Done`, `Error { class, text }`,
  `Limit { reason, text }` and `Panic`, and `Run::incomplete() -> Option<&'static str>`.

Call sequence: compile every enabled rule when the bundle is applied and keep the `WafScript` in an `Arc`. For a
request, in rule order: if the request budget is spent, mark the rule `budget` and skip it. Otherwise
`runner.run(&script, &ctx, &limits, Some(request_deadline))`, where `ctx` is any `&dyn RequestContext`. `limits` is
`profile.limits()`, whose `deadline` is the 5 ms of one script; the run stops at the sooner of that and `until`, and
does not start when `until` has passed.

Thread model: `WafScript` is `Send + Sync`; `WafRunner` is `Send`, not `Sync`, and runs one script at a time. Keep
one per worker thread (or per `spawn_blocking` task, taken from a pool). The context is only read during a call and
must outlive it. A run is CPU work of up to the deadline: run it where it cannot stall other connections.

What the request gets, as the reference decides it (`evalScript`):

- `verdict` is the first of `waf_return`, `waf_block`, `waf_allow` and `waf_log` that ran, and it stands whatever
  came after: a script that blocks and then fails still blocked. A verdict ends the function that called it, as
  `return` does; at the top level the run then ends with a runtime error, with the verdict kept.
- Without a verdict, `Run::incomplete()` is `None` when the script ran to its end (no decision), else the reason the
  rule did not finish: `error` (runtime error), `timeout` (the deadline), `panic`, or the limit (`op_limit`,
  `call_depth`, `nesting`, `value_size`, `value_depth`, `memory`, `regex_size`, `regex_work`). The agent marks the
  rule incomplete with that reason and goes on to the next rule; it never takes `Error`, `Limit` or `Panic` for
  `allow`. An `allow` that comes after an incomplete rule is dropped, a `block` or `log` is kept.
- The outcome is one of the three verdicts, no decision, or incomplete: a rule cannot change the score or anything
  else of the request, since the variables are copies and `waf_*` only reads. `print` output is discarded unless
  `limits.capture_print` is set.
- After `End::Panic` the runner has been reset and can be used again; count it and report the rule. A panic is
  caught with `catch_unwind`, so the agent must build with `panic = "unwind"`. Time spent waiting for a free runner
  or slot (`busy`) and a request that has run out of budget (`budget`) are the agent's reasons, not the library's.

## Configuration

`uddin` has no flags other than those above and reads no environment variables. It uses the profile embedded at
build time from `profile/sandbox-profile.json`: the builtin allowlist (`builtins`) and the limits a script runs
under (`limits`). Library code uses no files, sockets, processes or environment variables (`clippy.toml` enforces
it).

| Limit in the profile | Value | Read |
|---|---|---|
| `max_script_bytes` | 65536 | yes |
| `max_ops` | 100000 | yes |
| `max_call_depth` | 64 | yes |
| `max_nesting` | 256 | yes |
| `max_alloc_bytes` | 4194304 | yes |
| `max_value_len` | 1048576 | yes |
| `max_regex_program` | 1024 | yes |
| `max_regex_work` | 524288 | yes |
| `per_script_timeout_ms` | 5 | yes |
| `total_timeout_ms` | 15 | no |

The total timeout is the embedder's.

The profile is a pinned copy of a file the Wafio control plane exports, not the source. `profile/SOURCE` names the
repository, commit and sha256 of that copy. To update it, copy the new file and change `profile/SOURCE` in the same
commit.

## Differences from the reference

This implementation gives what the Go interpreter gives, except for its defects. A defect is a builtin that cannot
do what its documentation or its name says, a crash or panic, a sandbox escape, a wrong or nondeterministic result,
or control flow that leaves its construct. A language design choice is not one and is kept. Each row names a fix;
`conformance/corpus/run/<group>.intended.jsonl` records the result this implementation must give for every case of
the corpus that a row changes, and `conformance/runs.sh` fails on any other difference. The last column counts those
cases (a case can be in two rows), 0 where the corpus has none and a test of `crates/` holds the check instead.

| Row | The reference | Here | Why | Cases |
|---|---|---|---|---|
| `date-forms` | `date_parse` is declared to take 1 argument and needs 2, `date_format_new` 2 and needs 3, `date_add` and `date_subtract` 3 where a duration text makes 2, so each documented form fails with an argument count; `date_add` refuses an integer amount | `date_parse(text, layout)` and `date_format_new(text, layout, new_layout)` read and write any layout of Go's `time` package; `date_add(date, "2h45m")` and `date_subtract` read a duration text; the amount of the unit form is an integer or a float | each builtin could never do what its documentation says | 6 |
| `date-int` | `date_diff` and `date_compare` give an integer that `typeof` calls `unknown` and arithmetic refuses | a plain integer | a result a script cannot use | 0 |
| `float-modulo` | `5 % 0.5` panics with "integer divide by zero" | "can't divide by zero", an error a script can catch | a crash | 1 |
| `print-no-args` | `print()` writes its line to the process's standard output, outside the capture and the output limit | the line goes through the capture and the limit like every other print | a sandbox escape | 0 |
| `break-continue` | a `break` or `continue` in a function ends the loop of the caller, and one outside a loop is a runtime error; one in a loop's condition or iterable escapes that loop; `catch` binds one as text | a `break` or `continue` outside a loop of its own function is refused when the script is compiled, with the place; a jump passes through `try` and ends its own loop | control flow that crosses a function and a `try` | 18 |
| `return-scope` | a `return` inside `try` or `catch` leaves the scope of the call on the stack, so a stale copy of the function's variables hides the caller's and later assignments land in it | the scope stack goes back to its depth at the call | control flow that leaks | 0 |
| `join-plain` | `join(["a", "b"], "")` is `"a""b"`: every string element is quoted | elements are written as `str` writes them, a string as it is | a wrong result | 42 |
| `mode-tie` | a tie for the most frequent value picks a random one | the value that reached the highest count first | a nondeterministic result | 0 |
| `sort-key` | `sort(list, key)` cannot be called: the argument table allows 1 | `sort(list, key)` calls the function once for each element, in order, then sorts a copy stably by the keys and replaces the list; a failure leaves the list as it was | a documented form that never worked | 1 |
| `min-int` | `print` of the most negative integer writes `-` | its digits | a wrong result | 79 |
| `int-range` | `int(1e19)`, `int("9223372036854775808")`, `int(nan)` and `int(inf)` give the most negative integer | null, as `int` gives for any text that is not a number | a wrong result | 5 |
| `faults` | a `for` over a number, a spread of a number, a variadic function called with too few arguments, `==` between two builtins, `float(x, "a")` and a `substr` range outside the string end the run with an internal fault ("invalid memory address", "slice bounds out of range", "interface conversion") | each is a typed error with a clear text that `catch` binds | a panic | 44 |
| `set-remove` | `set_remove` takes out the first member that is `==` to the argument, not the one filed under its key (`1.0` for `1`, one of two equal lists, a `nan`) | the member filed under the key | a wrong result | 7 |
| `split-invalid-utf8` | `split(s, "")` keeps an invalid byte as it is, where `for`, `reverse_str`, `upper` and `lower` read it as U+FFFD | it is U+FFFD (the rule is in `crates/uddin-std/src/strings.rs`) | one rule | 4 |
| `map-order` | a script that walks a map sees a random order, and `json_stringify` of a map with several values it cannot write names one of them at random | the order the keys were inserted, and the first | a nondeterministic result | 0 |

The cases of each row are counted over the groups of the corpus; 202 distinct cases differ in all. Not changed, with
a recommendation each:

- `print` writes a float with six significant digits (`print(0.1 + 0.2)` is `0.3`) where `str` writes all of them.
  Recommend leaving it: scripts use `print` for display, and a change moves every printed float.
- `float("abc", 2)` is 0 and `float("1e999", 2)` is infinite where `int("abc")` is null. Recommend null for text
  that is not a number, but a rule that scripts may rely on.
- `abs`, `-x` and the arithmetic operators wrap at 64 bits, so `abs(-9223372036854775807 - 1)` is negative.
  Recommend an error for `abs`; the operators wrapping is the language.
- `5.5 % 2.5` takes the remainder of the truncated operands (1). Recommend documenting it.
- `date_format_new` is documented with two arguments in `docs/docs/reference/builtin-functions.md`, a file of the
  Go fork that this repo does not carry, and written with three. Recommend fixing the table of that documentation.
- `s[i]` of a string is one byte, which can be half of a character, where `for` reads characters. Recommend
  documenting it.
- A `try` that catches an error thrown by a nested `catch` keeps the outer try's variables in sight in the reference
  and here. Recommend leaving it.
- `int(float)` conversions elsewhere (a float as a date amount, the operands of `%`) give the most negative integer
  for a float no integer holds. Recommend saturating.

Also different, and not defects, because this is another implementation:

- The Unicode tables of the regular expression builtins are those of `regex-syntax`, which are newer than the
  reference's.
- The matcher of the regular expression builtins has a cap of 4 MiB.
- The estimate of regex work after a match close to the budget is not the reference's.
- Dates past the year 9999 are not handled.
- A zone abbreviation in a layout (`MST`) is looked up in the host's zone from the rules around the instant, where
  the reference reads every rule of the zone.

## Release

There is no bundle and nothing to upload. The crates have `publish = false`, so a consumer takes them as Cargo git
dependencies pinned to a tag. Tags are `vX.Y.Z`, set by hand with `git tag`; `v0.1.0` is the first.

## Layout

- `crates/uddin-syntax`: lexer, parser, syntax tree, source positions and the allowlist check.
- `crates/uddin-vm`: the compiler, the value model, the run heap, the limits and the machine.
- `crates/uddin-std`: the builtins, one module for each group (strings, lists, sort, sets, stacks and queues, JSON,
  math, paths, conversion, regular expressions, dates, and `print`, `len`, `str`, `typeof`).
- `crates/uddin`: the library an embedder of the plain interpreter uses (`check`, `Script`, `Runner`).
- `crates/uddin-waf`: the seventeen `waf_*` builtins, the request context, and the calls a WAF agent makes.
- `crates/uddin-cli`: the `uddin` command line.
- `conformance/`: the Go oracle, the corpus with the results Go gave, and the tools that compare this
  implementation with it. `fuzz/`: fuzz targets. `benches/`: the bench scripts and their measurement.
- `profile/`: `sandbox-profile.json` and `SOURCE`.
- `scripts/`: `ci-local.sh` (the gate, no Go), `ci-oracle.sh` (the steps that run the Go reference), `fuzz.sh`,
  `check-ai.sh`.

## License

MIT. The original notice of the Go interpreter is kept in `LICENSE` next to the notice of this port.

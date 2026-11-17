#!/usr/bin/env bash
# Fails when agent material is tracked, or when plan, phase, finding or wave IDs sit in a tracked file or a
# commit message. Needs a git checkout (git ls-files, git grep, git log).
set -euo pipefail
cd "$(dirname "$0")/.."

ai_files='(^|/)(AGENTS|CLAUDE|GEMINI)\.md$|(^|/)\.claude/|copilot-instructions|port-status|PORTING\.md'
ai_ids='(^|[^A-Za-z0-9_\\-])(K-[0-9]{2,3}|(CU|SB|TU|CF|WP|DEC|FSD|BR)-[0-9]+|OD-U?[0-9]+|RK-U?[0-9]+|[MDSF]-[0-9]{1,2}|U[0-6]|[Ww]ave[- ]?[0-9]+)([^0-9A-Za-z_]|$)'

# No \b (macOS git grep -E matches nothing with it) and no grep -q after a pipe (SIGPIPE under pipefail).
# The samples are built with printf so this file does not match its own pattern.
[ "$(printf 'x K-%s y\n' 12 | grep -cE "$ai_ids")" = 1 ] || { echo 'ai check: pattern self-test failed' >&2; exit 1; }
[ "$(printf 'x CU-%s y\n' 1 | grep -cE "$ai_ids")" = 1 ] || { echo 'ai check: pattern self-test failed' >&2; exit 1; }
[ "$(printf 'x U%s y\n' 1 | grep -cE "$ai_ids")" = 1 ] || { echo 'ai check: pattern self-test failed' >&2; exit 1; }
[ "$(printf 'x \\U%s y\n' 00012345 | grep -cE "$ai_ids")" = 0 ] || { echo 'ai check: pattern self-test failed' >&2; exit 1; }
if git ls-files | grep -E "$ai_files"; then echo 'ai check: agent files tracked' >&2; exit 1; fi
if git grep -nE "$ai_ids"; then echo 'ai check: IDs in tracked files' >&2; exit 1; fi
if git log --format='%h %B' | grep -E "$ai_ids"; then echo 'ai check: IDs in commit messages' >&2; exit 1; fi
echo 'ai check: clean'

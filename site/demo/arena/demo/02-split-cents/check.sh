#!/bin/sh
# Runs in Glut's check copy: the bench's own tests (with -P), nothing changed beside the task.
set -eu
only() {
  bad=$(git -c core.excludesFile=/dev/null status --porcelain --untracked-files=all --ignored | cut -c4- \
    | grep -v '^\.arena/' | grep -vxE "$1" || true)
  if [ -n "$bad" ]; then echo "changed beyond the task: $bad" >&2; exit 1; fi
}
ran() {
  out=$("$@" 2>&1) || { printf '%s\n' "$out" >&2; exit 1; }
  printf '%s\n' "$out" | grep -qE '^Ran [1-9][0-9]* tests?' && printf '%s\n' "$out" | grep -qx OK
}
only 'money\.py|test_money\.py|(.*/)?__pycache__/.*'
# A model's tests leave bytecode behind; none of it is run: a planted unchecked .pyc would stand in for the source.
find . -name __pycache__ -type d -prune -exec rm -rf {} +
cp .arena/check/test_arena.py arena_check_money.py
ran python3 -B -P arena_check_money.py

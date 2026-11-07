#!/bin/sh
# The system prompt's measure (T35; later part of TUI10): the same questions about a project,
# asked in plan mode (it only reads), each counted by its tool calls until the answer and its input tokens. Run it
# before and after a change to the prompt, on the same model.
#
#   scripts/prompt-bench.sh [FOLDER] [glut run flags…]     e.g.  scripts/prompt-bench.sh ../Axiomata-OS --model m
#
# Each question is a paid model call: the default provider's, or the one the flags name.
set -eu
folder="${1:-.}"
[ "$#" -gt 0 ] && shift
glut="${GLUT:-$(dirname "$0")/../target/release/glut}"
printf '%-36s %6s %10s %s\n' "question" "calls" "input tok" "answered"
for question in \
    "Where do we stand?" \
    "What is the next step of the newest plan?" \
    "Where is the memory kept?" \
    "Which cards or to-dos are open?"; do
    "$glut" run --project --permission-mode plan --cwd "$folder" "$@" "$question" 2>/dev/null |
        python3 -I -c '
import json, sys
calls = tokens = 0
answered = "no"
for line in sys.stdin:
    try:
        event = json.loads(line)
    except ValueError:
        continue
    kind = event.get("event")
    if kind == "tool_call":
        calls += 1
    elif kind == "finished":
        tokens = event.get("usage", {}).get("input_tokens", 0)
        answered = "yes" if event.get("reason") == "answered" else event.get("reason", "?")
print(f"{sys.argv[1][:36]:<36} {calls:>6} {tokens:>10} {answered}")
' "$question"
done

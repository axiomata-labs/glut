"""Glut's results and speed on fixed tasks (docs/plans/glut-tui.md, T47): run it before and after a change to the system
prompt or the tools, on the same models — a change stays only where every task is solved as often as before and Glut
needs no more calls. Each run works in a fresh copy of `fixture/` and is checked by what it left there or answered.

    python3 -I scripts/bench/task-bench.py MODELS [REPEATS] [--base-url URL] [--label NAME] [--tasks a,b]
        [--glut PATH] [--keep DIR] [--no-mcp]
    e.g. python3 -I scripts/bench/task-bench.py qwen3.8:27b-mlx,gemma4:12b-mlx 3 --label before

Every run is a model call or more on the given API (a local Ollama by default). Results go to `results.jsonl` beside
this file, one line a run, and a summary is printed. `--glut` runs another build of Glut (the one before a change);
`--keep` keeps each run's events and its folder there, to read why a task failed; `--no-mcp` runs without the
owner's MCP servers (where `codemode` is offered only for them, without it).
"""

import json
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
GLUT = HERE.parent.parent / "target" / "release" / "glut"


def sources(folder: str) -> str:
    return "".join(path.read_text() for path in Path(folder).rglob("*.py"))


def tests_pass(folder: str) -> bool:
    done = subprocess.run(
        [sys.executable, "-I", "-m", "unittest", "-q"],
        check=False,
        cwd=folder,
        capture_output=True,
        timeout=120,
    )
    return done.returncode == 0


TASKS = {
    "read": (
        (
            "What number of seconds does parse_duration in src/timeparse.py return for '2h30m'? "
            "Read the code and answer in one short sentence."
        ),
        lambda folder, said: any(
            number in said for number in ("9000", "9,000", "9.000")
        ),
    ),
    "find": (
        (
            "Which environment variable overrides where this project keeps its config file? "
            "Find it in the code and answer with its name."
        ),
        lambda folder, said: "TIMETRACK_CONFIG" in said,
    ),
    "rename": (
        (
            "Rename the function parse_duration to duration_of everywhere in this project, "
            "its definition and every use. Change nothing else."
        ),
        lambda folder, said: (
            "parse_duration" not in sources(folder)
            and "duration_of" in (Path(folder) / "src" / "report.py").read_text()
        ),
    ),
    "fix": (
        (
            "A test of this project fails. Run the tests with `python3 -m unittest`, find the bug in the code "
            "(not in the tests) and fix it, so that every test passes."
        ),
        lambda folder, said: (
            tests_pass(folder)
            and "write" in (Path(folder) / "tests" / "test_report.py").read_text()
        ),
    ),
    "feature": (
        (
            "Add a function format_duration(seconds) to src/timeparse.py that writes a number of seconds the way "
            "parse_duration reads them, e.g. 9000 as '2h30m' and 45 as '45s'. Then add summary(lines) to src/report.py "
            "returning one line 'task: duration' per task, in the order the tasks first appear, using by_task and "
            "format_duration. Add a test for each to the tests folder and make sure every test passes."
        ),
        lambda folder, said: (
            "def format_duration" in sources(folder)
            and "def summary" in sources(folder)
            and tests_pass(folder)
        ),
    ),
}


def run(
    model: str,
    task: str,
    base_url: str,
    glut: Path,
    keep: Path | None,
    servers: list[str],
) -> dict:
    prompt, check = TASKS[task]
    work = tempfile.mkdtemp(prefix=f"glut-bench-{task}-")
    shutil.copytree(
        HERE / "fixture",
        work,
        dirs_exist_ok=True,
        ignore=shutil.ignore_patterns("__pycache__"),
    )
    started = time.time()
    try:
        done = subprocess.run(
            [
                str(glut),
                "run",
                "--base-url",
                base_url,
                "--model",
                model,
                "--cwd",
                work,
                "--permission-mode",
                "acceptEdits",
                *servers,
                prompt,
            ],
            check=False,
            cwd=work,
            capture_output=True,
            text=True,
            timeout=900,
            stdin=subprocess.DEVNULL,
        )
        out = done.stdout
    except subprocess.TimeoutExpired:
        out = ""
    seconds = time.time() - started
    tools = model_calls = input_tokens = output_tokens = 0
    said = []
    reason = "?"
    for line in out.splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        kind = event.get("event")
        if kind == "tool_call":
            tools += 1
        elif kind == "usage":
            model_calls += 1
        elif kind == "assistant_text":
            said.append(event.get("text", ""))
        elif kind == "finished":
            reason = event.get("reason", "?")
            usage = event.get("usage", {})
            input_tokens = usage.get("input_tokens", 0)
            output_tokens = usage.get("output_tokens", 0)
    try:
        ok = bool(check(work, "\n".join(said)))
    except (OSError, UnicodeDecodeError, subprocess.TimeoutExpired):
        ok = False
    if keep is not None:
        kept = keep / f"{model.replace(':', '_')}-{task}-{int(started)}"
        shutil.copytree(work, kept / "folder")
        (kept / "events.jsonl").write_text(out)
    shutil.rmtree(work, ignore_errors=True)
    return {
        "model": model,
        "task": task,
        "ok": ok,
        "reason": reason,
        "seconds": round(seconds, 1),
        "tools": tools,
        "calls": model_calls,
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
    }


def main() -> None:
    args = sys.argv[1:]
    base_url = "http://127.0.0.1:11434"
    label = "run"
    glut = GLUT
    keep = None
    tasks = list(TASKS)
    servers: list[str] = []
    if "--no-mcp" in args:
        args.remove("--no-mcp")
        empty = Path(tempfile.mkdtemp(prefix="glut-bench-mcp-")) / "mcp.json"
        empty.write_text('{"mcpServers": {}}')
        servers = ["--mcp-config", str(empty)]
    if "--glut" in args:
        at = args.index("--glut")
        glut = Path(args[at + 1]).resolve()
        del args[at : at + 2]
    if "--keep" in args:
        at = args.index("--keep")
        keep = Path(args[at + 1]).resolve()
        del args[at : at + 2]
    if "--tasks" in args:
        at = args.index("--tasks")
        tasks = args[at + 1].split(",")
        del args[at : at + 2]
    if "--base-url" in args:
        at = args.index("--base-url")
        base_url = args[at + 1]
        del args[at : at + 2]
    if "--label" in args:
        at = args.index("--label")
        label = args[at + 1]
        del args[at : at + 2]
    if not args or args[0].startswith("-"):
        sys.exit(__doc__)
    models = args[0].split(",")
    repeats = int(args[1]) if len(args) > 1 else 3
    lines = []
    for model in models:
        for repeat in range(1, repeats + 1):
            for task in tasks:
                line = {
                    "label": label,
                    "repeat": repeat,
                    **run(model, task, base_url, glut, keep, servers),
                }
                lines.append(line)
                with open(HERE / "results.jsonl", "a") as out:
                    out.write(json.dumps(line) + "\n")
                print(json.dumps(line), flush=True)
    print(
        f"\n{'model':<22} {'task':<8} {'solved':>6} {'seconds':>8} {'calls':>6} {'tools':>6} {'in tok':>8}"
    )
    for model in models:
        for task in tasks:
            rows = [
                line
                for line in lines
                if line["model"] == model and line["task"] == task
            ]
            solved = sum(line["ok"] for line in rows)
            print(
                f"{model[:22]:<22} {task:<8} {solved:>3}/{len(rows):<2} "
                f"{statistics.median(line['seconds'] for line in rows):>8.1f} "
                f"{statistics.median(line['calls'] for line in rows):>6} "
                f"{statistics.median(line['tools'] for line in rows):>6} "
                f"{statistics.median(line['input_tokens'] for line in rows):>8}"
            )


if __name__ == "__main__":
    main()

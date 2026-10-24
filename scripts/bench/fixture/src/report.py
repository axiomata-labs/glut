"""Sums of a log file."""

from .timeparse import parse_duration


def total(lines: list[str]) -> int:
    """Seconds of every line of the form 'task: 1h20m'."""
    seconds = 0
    for line in lines:
        if ":" in line:
            seconds += parse_duration(line.split(":", 1)[1])
    return seconds


def by_task(lines: list[str]) -> dict[str, int]:
    """Seconds per task, the task's name as written without the spaces around it."""
    out: dict[str, int] = {}
    for line in lines:
        if ":" in line:
            task, spent = line.split(":", 1)
            out[task] = out.get(task, 0) + parse_duration(spent)
    return out

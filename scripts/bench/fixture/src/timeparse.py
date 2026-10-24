"""Durations as people write them."""

import re

_PART = re.compile(r"(\d+)([hms])")
_SECONDS = {"h": 3600, "m": 60, "s": 1}


def parse_duration(text: str) -> int:
    """Seconds in a duration like '2h30m' or '45s'."""
    total = 0
    for amount, unit in _PART.findall(text.strip().lower()):
        total += int(amount) * _SECONDS[unit]
    if total == 0 and text.strip() not in ("0", "0s"):
        raise ValueError(f"no duration in {text!r}")
    return total

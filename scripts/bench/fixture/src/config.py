"""Where the tool keeps its settings."""

import os
from pathlib import Path

DEFAULT = Path.home() / ".config" / "timetrack" / "config.toml"


def config_path() -> Path:
    """The settings file: TIMETRACK_CONFIG when set, else the default."""
    override = os.environ.get("TIMETRACK_CONFIG")
    return Path(override) if override else DEFAULT

#!/usr/bin/python3
"""Persist proof results without changing the trusted base gate definitions."""

import json
import subprocess
import sys
from pathlib import Path

CARGO = "/usr/local/cargo/bin/cargo"
REPORTS = Path("/work/ci-reports")
LIMIT_SECS = 3600

# The signal boundary of the 2026-11-13 host incident. When a change selects
# no mutant, mutating it keeps the report nonempty and the run bounded.
CANARY = "crates/cli/src/command_step/run.rs"


def selected(args: list[str]) -> list:
    """The mutants the diff selects; cargo-mutants prints nothing for none."""
    listed = subprocess.run(
        [CARGO, *args, "--list", "--json"],
        check=True,
        capture_output=True,
        text=True,
        timeout=LIMIT_SECS,
    )
    (REPORTS / "selected-mutants.json").write_text(listed.stdout)

    if not listed.stdout.strip():
        return []

    mutants = json.loads(listed.stdout)

    if not isinstance(mutants, list):
        raise ValueError("mutation selection is not a list")

    return mutants


def arguments(args: list[str]) -> list[str]:
    """Export Kani's results; never let an empty diff yield an empty report."""
    if args and args[0] == "kani" and args[1:] != ["setup"]:
        export = ["-Z", "unstable-options", "--export-json"]
        return [*args, *export, str(REPORTS / "kani.json")]

    if args and args[0] == "mutants" and "--in-diff" in args:
        if selected(args):
            return args

        if not Path(CANARY).is_file():
            raise ValueError(f"no mutant selected and no {CANARY} to mutate")

        position = args.index("--in-diff")
        return [*args[:position], "--file", CANARY, *args[position + 2 :]]

    return args


def main() -> int:
    REPORTS.mkdir(exist_ok=True)
    args = arguments(sys.argv[1:])
    return subprocess.run([CARGO, *args], check=False, timeout=LIMIT_SECS).returncode


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/python3
"""Persist proof results without changing the trusted base gate definitions."""

import json
import os
import subprocess
import sys
from pathlib import Path

# reports.py owns the canary's path: beside bin/ in the image, beside this
# file in the repository.
HERE = Path(__file__).resolve().parent
sys.path[:0] = [str(HERE), str(HERE.parent)]
from reports import CANARY  # noqa: E402

CARGO = "/usr/local/cargo/bin/cargo"
REPORTS = Path("/work/ci-reports")
LIMIT_SECS = 3600


def listed(args: list[str]) -> list:
    """The mutants `args` select; cargo-mutants prints nothing for none."""
    output = subprocess.run(
        [CARGO, *args, "--list", "--json"],
        check=True,
        capture_output=True,
        text=True,
        timeout=LIMIT_SECS,
    ).stdout

    if not output.strip():
        return []

    mutants = json.loads(output)

    if not isinstance(mutants, list):
        raise ValueError("mutation selection is not a list")

    return mutants


def reason() -> str:
    """Why the diff selected nothing, in the words the job summary shows."""
    note = REPORTS / "base.txt"

    if note.is_file():
        return note.read_text(encoding="utf-8").strip()

    return f"the diff against {os.environ.get('ATELIER_BASE', 'the base')} selects no mutant"


def select(args: list[str]) -> tuple[list[str], dict]:
    """The diff's mutants, or the canary's when the diff selects none."""
    diff = listed(args)

    if diff:
        return args, {"kind": "diff", "diff_selected": len(diff)}

    if not Path(CANARY).is_file():
        raise ValueError(f"no mutant selected and no canary {CANARY} to mutate")

    position = args.index("--in-diff")
    canary = [*args[:position], "--file", CANARY, *args[position + 2 :]]
    mutants = listed(canary)

    # A canary that shrinks to nothing is no evidence either.
    if not mutants:
        raise ValueError(f"the canary {CANARY} yields no mutant")

    selection = {
        "kind": "canary",
        "diff_selected": 0,
        "canary": CANARY,
        "canary_listed": len(mutants),
        "reason": reason(),
    }
    return canary, selection


def arguments(args: list[str]) -> list[str]:
    """Export Kani's results; record which mutants a run tests, and why."""
    if args and args[0] == "kani" and args[1:] != ["setup"]:
        export = ["-Z", "unstable-options", "--export-json"]
        return [*args, *export, str(REPORTS / "kani.json")]

    if not args or args[0] != "mutants" or "--list" in args:
        return args

    if "--in-diff" in args:
        args, selection = select(args)
    else:
        selection = {"kind": "configured"}

    (REPORTS / "mutants-selection.json").write_text(json.dumps(selection), encoding="utf-8")
    print(f"mutants selection: {json.dumps(selection)}", file=sys.stderr)
    return args


def main() -> int:
    REPORTS.mkdir(exist_ok=True)
    args = arguments(sys.argv[1:])
    return subprocess.run([CARGO, *args], check=False, timeout=LIMIT_SECS).returncode


if __name__ == "__main__":
    sys.exit(main())

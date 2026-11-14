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
from reports import ATELIER_REPOSITORY, CANARY  # noqa: E402

CARGO = "/usr/local/cargo/bin/cargo"
REPORTS = Path("/reports")
LIMIT_SECS = 3600


def listed(args: list[str]) -> list:
    """The mutants `args` select; cargo-mutants prints nothing for none."""
    try:
        output = subprocess.run(
            [CARGO, *args, "--list", "--json"],
            check=True,
            capture_output=True,
            text=True,
            timeout=LIMIT_SECS,
        ).stdout
    except subprocess.CalledProcessError as error:
        print(error.stderr, file=sys.stderr)
        raise

    if not output.strip():
        return []

    mutants = json.loads(output)

    if not isinstance(mutants, list):
        raise ValueError("mutation selection is not a list")

    return mutants


def options(args: list[str], *extra: str) -> list[str]:
    """`args` with cargo-mutants options added before any test arguments."""
    end = args.index("--") if "--" in args else len(args)
    return [*args[:end], *extra, *args[end:]]


def without_diff(args: list[str]) -> list[str] | None:
    """`args` without their diff filter, or None when they have none."""
    for index, arg in enumerate(args):
        if arg in ("--in-diff", "-D"):
            return [*args[:index], *args[index + 2 :]]

        if arg.startswith("--in-diff="):
            return [*args[:index], *args[index + 1 :]]

    return None


def canary(args: list[str], why: str) -> tuple[list[str], dict]:
    """Mutate the canary instead of an empty diff; atelier's own CI only."""
    if os.environ.get("PROJECT") != ATELIER_REPOSITORY:
        raise ValueError(f"{why}; the canary is atelier's own, so this gate has no evidence")

    if not Path(CANARY).is_file():
        raise ValueError(f"{why}, and the canary {CANARY} is missing")

    rest = without_diff(args)
    assert rest is not None, "only a diff run falls back to the canary"
    mutants = listed(options(rest, "--file", CANARY))

    # A canary that shrinks to nothing is no evidence either.
    if not mutants:
        raise ValueError(f"the canary {CANARY} yields no mutant")

    selection = {
        "mode": "canary",
        "diff_selected": 0,
        "canary": CANARY,
        "selected": sorted(mutant.get("name") for mutant in mutants),
        "reason": why,
    }
    return options(rest, "--file", CANARY), selection


def select(args: list[str]) -> tuple[list[str], dict]:
    """The diff's mutants, or the canary's when the diff has none to give."""
    note = REPORTS / "base.txt"

    # Without a usable base the diff is not this change's; never call it one.
    if note.is_file():
        return canary(args, note.read_text(encoding="utf-8").strip())

    diff = listed(args)

    if not diff:
        base = os.environ.get("ATELIER_BASE", "the base")
        return canary(args, f"the diff against {base} selects no mutant")

    names = sorted(mutant.get("name") for mutant in diff)
    return args, {"mode": "diff", "diff_selected": len(diff), "selected": names}


def arguments(args: list[str]) -> list[str]:
    """Export Kani's results; record which mutants a run tests, and why."""
    if args and args[0] == "kani" and args[1:] != ["setup"]:
        export = ["-Z", "unstable-options", "--export-json"]
        return [*args, *export, str(REPORTS / "kani.json")]

    if not args or args[0] != "mutants" or any(arg.startswith("--list") for arg in args):
        return args

    if without_diff(args) is not None:
        args, selection = select(args)
    else:
        selection = {"mode": "configured"}

    (REPORTS / "mutants-selection.json").write_text(json.dumps(selection), encoding="utf-8")
    print(f"mutants selection: {json.dumps(selection)}", file=sys.stderr)
    # One report folder outside the checkout, whatever the gate's root.
    return options(args, "--output", str(REPORTS))


def main() -> int:
    REPORTS.mkdir(exist_ok=True)
    args = arguments(sys.argv[1:])
    return subprocess.run([CARGO, *args], check=False, timeout=LIMIT_SECS).returncode


if __name__ == "__main__":
    sys.exit(main())

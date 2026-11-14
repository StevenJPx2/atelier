"""Fail-closed checks for Kani 0.68 and cargo-mutants 27.1 CI reports.

Usage: python3 .github/ci/reports.py <gate> <rootpath>
Other gates rely on their process exit status and need no report.
Contracts: Kani's kani-driver/src/frontend/schema_utils.rs and cargo-mutants'
src/outcome.rs and src/output.rs at the pinned upstream release tags.
"""

import argparse
import json
import sys
from collections import Counter
from datetime import datetime
from pathlib import Path

MAX_REPORT_BYTES = 128 * 1024 * 1024
# atelier's own floor: both core proofs, wherever atelier's gates run.
ATELIER_PROOFS = Path("crates/core/src/machine/proofs.rs")
ATELIER_HARNESSES = frozenset(
    {
        "machine::proofs::a_run_follows_what_rungs_answers",
        "machine::proofs::"
        "a_move_outside_rungs_keeps_the_marks_and_the_resident_steps_right",
    }
)
PROPERTY_STATUSES = {
    "Success": "passed",
    "Failure": "failed",
    "Unreachable": "unreachable",
    "Undetermined": "undetermined",
    "Unknown": "undetermined",
    "Error": "solver_error",
    "Satisfied": "satisfied",
    "Unsatisfiable": "unsatisfiable",
    "Covered": "covered",
    "Uncovered": "uncovered",
}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def object_value(value: object, label: str) -> dict:
    require(isinstance(value, dict), f"{label} must be an object")
    return value


def array_value(value: object, label: str) -> list:
    require(isinstance(value, list), f"{label} must be an array")
    return value


def count(value: object, label: str) -> int:
    require(type(value) is int and value >= 0, f"{label} must be a nonnegative integer")
    return value


def unique_object(pairs: list) -> dict:
    result = {}
    for key, value in pairs:
        require(key not in result, f"duplicate JSON key: {key}")
        result[key] = value
    return result


def invalid_constant(value: str) -> None:
    raise ValueError(f"invalid JSON number: {value}")


def read_report(path: Path) -> dict:
    require(path.is_file() and not path.is_symlink(), f"missing regular report: {path}")
    with path.open("rb") as source:
        raw = source.read(MAX_REPORT_BYTES + 1)
    require(0 < len(raw) <= MAX_REPORT_BYTES, f"empty or oversized report: {path}")
    return object_value(
        json.loads(
            raw.decode("utf-8"),
            object_pairs_hook=unique_object,
            parse_constant=invalid_constant,
        ),
        str(path),
    )


def harness_entries(
    value: object, key: str, label: str, expected: frozenset | None = None
) -> dict:
    entries = {}
    for item in array_value(value, label):
        entry = object_value(item, label)
        name = entry.get(key)
        require(isinstance(name, str) and bool(name), f"missing {label} identity")
        require(name not in entries, f"duplicate {label} identity: {name}")
        entries[name] = entry
    require(bool(entries), f"no {label}")
    if expected is not None:
        require(frozenset(entries) == expected, f"incomplete {label}")
    return entries


def validate_properties(result: dict, entry: dict) -> None:
    checks = array_value(result.get("checks"), "checks")
    require(bool(checks), "harness has no property checks")
    statuses = Counter()
    for index, item in enumerate(checks, 1):
        check = object_value(item, "check")
        require(count(check.get("id"), "check.id") == index, "incomplete check identities")
        status = check.get("status")
        require(status in ("Success", "Unreachable"), f"unsuccessful check: {status}")
        statuses[PROPERTY_STATUSES[status]] += 1
    require(statuses["passed"] > 0, "harness has no passed checks")
    details = object_value(entry.get("property_details"), "property_details")
    require(details.get("error") is None, "property extraction error")
    require(
        count(details.get("total_properties"), "total_properties") == len(checks),
        "property total does not match checks",
    )
    for field in set(PROPERTY_STATUSES.values()):
        require(
            count(details.get(field), field) == statuses[field],
            f"property count mismatch: {field}",
        )


def validate_kani(report: dict, atelier: bool) -> None:
    metadata = object_value(report.get("metadata"), "metadata")
    require(metadata.get("kani_version") == "0.68.0", "unexpected Kani version")
    harnesses = harness_entries(report.get("harness_metadata"), "pretty_name", "metadata")
    names = frozenset(harnesses)
    if atelier:
        require(ATELIER_HARNESSES <= names, "atelier's core proofs did not all run")
    verification = object_value(report.get("verification_results"), "verification_results")
    summary = object_value(verification.get("summary"), "summary")
    require(summary.get("status") == "completed", "Kani run is not completed")
    for field in ("total_harnesses", "executed", "successful"):
        require(count(summary.get(field), field) == len(names), f"incomplete {field}")
    require(count(summary.get("failed"), "failed") == 0, "failed Kani harnesses")
    results = harness_entries(verification.get("results"), "harness_id", "results", names)
    errors = harness_entries(report.get("error_details"), "harness_id", "errors", names)
    properties = harness_entries(
        report.get("property_details"), "harness_id", "properties", names
    )
    harness_entries(report.get("cbmc"), "harness_id", "cbmc", names)
    for name, result in results.items():
        if atelier and name in ATELIER_HARNESSES:
            crate = harnesses[name].get("crate_name")
            require(crate == "atelier_core", "wrong harness crate")
        attributes = object_value(harnesses[name].get("attributes"), "attributes")
        require(
            attributes.get("kind") == "Proof" and attributes.get("should_panic") is False,
            "expected a non-panicking proof harness",
        )
        require(result.get("status") == "Success", f"unsuccessful harness: {name}")
        require(errors[name].get("has_errors") is False, f"harness errors: {name}")
        require(errors[name].get("error_type") is None, f"harness error type: {name}")
        require(
            errors[name].get("exit_status") in (None, "success"),
            f"harness exit failure: {name}",
        )
        validate_properties(result, properties[name])


def timestamp(value: object, label: str) -> datetime:
    require(isinstance(value, str) and bool(value), f"missing {label}")
    parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    require(parsed.tzinfo is not None, f"{label} must include a timezone")
    return parsed


def failure_status(value: object) -> bool:
    return (
        isinstance(value, dict)
        and set(value) == {"Failure"}
        and type(value["Failure"]) is int
        and value["Failure"] != 0
    )


def validate_phases(outcome: dict, summary: str) -> None:
    phases = [
        object_value(item, "phase")
        for item in array_value(outcome.get("phase_results"), "phase_results")
    ]
    names = [phase.get("phase") for phase in phases]
    expected = ["Build"] if summary == "Unviable" else ["Build", "Test"]
    require(names == expected, f"incomplete phases for {summary}")
    for phase in phases[:-1]:
        require(phase.get("process_status") == "Success", "unsuccessful build")
    final = phases[-1].get("process_status")
    if summary == "Success":
        require(final == "Success", "baseline tests did not succeed")
    else:
        require(failure_status(final), f"missing failure evidence for {summary}")


def validate_mutants(report: dict) -> None:
    require(report.get("cargo_mutants_version") == "27.1.0", "unexpected mutants version")
    start = timestamp(report.get("start_time"), "start_time")
    end = timestamp(report.get("end_time"), "end_time")
    require(end >= start, "mutants end_time precedes start_time")
    total = count(report.get("total_mutants"), "total_mutants")
    require(total > 0, "zero mutants tested")
    outcomes = array_value(report.get("outcomes"), "outcomes")
    require(len(outcomes) == total + 1, "outcome count must include mutants and baseline")
    counts = Counter()
    names = set()
    baseline = 0
    for item in outcomes:
        outcome = object_value(item, "outcome")
        scenario = outcome.get("scenario")
        summary = outcome.get("summary")
        if scenario == "Baseline":
            baseline += 1
            require(summary == "Success", "unsuccessful baseline")
        else:
            mutant = object_value(scenario, "scenario")
            require(set(mutant) == {"Mutant"}, "unknown mutant scenario")
            identity = object_value(mutant["Mutant"], "Mutant").get("name")
            require(isinstance(identity, str) and bool(identity.strip()), "missing mutant name")
            require(identity not in names, "duplicate mutant outcome")
            names.add(identity)
            require(summary in ("CaughtMutant", "Unviable"), f"bad mutant outcome: {summary}")
            counts[summary] += 1
        validate_phases(outcome, summary)
    require(baseline == 1 and len(names) == total, "incomplete baseline or mutant outcomes")
    expected = {
        "caught": counts["CaughtMutant"],
        "unviable": counts["Unviable"],
        "missed": 0,
        "timeout": 0,
        "success": 0,
    }
    for field, wanted in expected.items():
        require(count(report.get(field), field) == wanted, f"mutant count mismatch: {field}")


def validate(gate: str, root: Path) -> None:
    """Check `commands/kani` and `commands/mutants` by their gate name."""
    name = gate.rpartition("/")[2]
    if name == "kani":
        atelier = (root / ATELIER_PROOFS).is_file()
        validate_kani(read_report(root / "ci-reports" / "kani.json"), atelier)
    elif name == "mutants":
        validate_mutants(read_report(root / "mutants.out" / "outcomes.json"))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("gate")
    parser.add_argument("rootpath", type=Path)
    args = parser.parse_args(argv)
    try:
        validate(args.gate, args.rootpath)
    except (OSError, UnicodeError, ValueError, RecursionError) as error:
        print(f"{args.gate}: invalid CI report: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

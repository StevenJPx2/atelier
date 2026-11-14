"""Report contract regressions; execute only inside the CI container."""

import contextlib
import copy
import importlib.util
import io
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("reports.py")
SPEC = importlib.util.spec_from_file_location("ci_reports", SCRIPT)
reports = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(reports)


def kani_report(names: list[str] | None = None, crate: str = "atelier_core") -> dict:
    """Representative export from Kani 0.68's schema_utils.rs contract."""
    names = sorted(reports.ATELIER_HARNESSES) if names is None else names
    total = len(names)
    return {
        "metadata": {"kani_version": "0.68.0"},
        "harness_metadata": [
            {
                "pretty_name": name,
                "crate_name": crate,
                "attributes": {"kind": "Proof", "should_panic": False},
            }
            for name in names
        ],
        "verification_results": {
            "summary": {
                "total_harnesses": total,
                "executed": total,
                "status": "completed",
                "successful": total,
                "failed": 0,
            },
            "results": [
                {
                    "harness_id": name,
                    "status": "Success",
                    "checks": [{"id": 1, "status": "Success"}],
                }
                for name in names
            ],
        },
        "error_details": [
            {"harness_id": name, "has_errors": False} for name in names
        ],
        "property_details": [
            {
                "harness_id": name,
                "property_details": {
                    "total_properties": 1,
                    **{
                        field: int(field == "passed")
                        for field in set(reports.PROPERTY_STATUSES.values())
                    },
                },
            }
            for name in names
        ],
        "cbmc": [{"harness_id": name} for name in names],
    }


def phases(final: object = "Success", unviable: bool = False) -> list:
    if unviable:
        return [{"phase": "Build", "process_status": final}]
    return [
        {"phase": "Build", "process_status": "Success"},
        {"phase": "Test", "process_status": final},
    ]


def mutants_report() -> dict:
    """Matches the real 27.1 report, with a finished run and two mutants."""
    return {
        "cargo_mutants_version": "27.1.0",
        "start_time": "2026-11-13T15:49:29.548516Z",
        "end_time": "2026-11-13T15:50:29.548516Z",
        "total_mutants": 2,
        "caught": 1,
        "unviable": 1,
        "missed": 0,
        "timeout": 0,
        "success": 0,
        "outcomes": [
            {
                "scenario": "Baseline",
                "summary": "Success",
                "phase_results": phases(),
            },
            {
                "scenario": {"Mutant": {"name": "src/lib.rs:1: replace true"}},
                "summary": "CaughtMutant",
                "phase_results": phases({"Failure": 100}),
            },
            {
                "scenario": {"Mutant": {"name": "src/lib.rs:2: replace return"}},
                "summary": "Unviable",
                "phase_results": phases({"Failure": 101}, unviable=True),
            },
        ],
    }


class ReportCase(unittest.TestCase):
    def setUp(self) -> None:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        # Most cases check atelier's own floor, so the root holds its proofs.
        proofs = self.root / reports.ATELIER_PROOFS
        proofs.parent.mkdir(parents=True)
        proofs.write_text("", encoding="utf-8")

    def write(self, gate: str, value: object) -> Path:
        kani = gate.rpartition("/")[2] == "kani"
        relative = "ci-reports/kani.json" if kani else "mutants.out/outcomes.json"
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value), encoding="utf-8")
        return path

    def rejected(self, gate: str, value: object) -> None:
        self.write(gate, value)
        with self.assertRaises(ValueError):
            reports.validate(gate, self.root)


class ReportTests(ReportCase):
    def test_good_reports_through_cli(self) -> None:
        for gate, value in (("kani", kani_report()), ("mutants", mutants_report())):
            with self.subTest(gate=gate):
                self.write(gate, value)
                completed = subprocess.run(
                    [sys.executable, str(SCRIPT), gate, str(self.root)],
                    capture_output=True,
                    text=True,
                    timeout=10,
                    check=False,
                )
                self.assertEqual(completed.returncode, 0, completed.stderr)

    def test_matrix_gate_names_are_validated(self) -> None:
        for gate in ("commands/kani", "commands/mutants"):
            with self.subTest(gate=gate):
                self.assertEqual(reports.main([gate, str(self.root)]), 1)
        self.write("commands/kani", kani_report())
        self.write("commands/mutants", mutants_report())
        for gate in ("commands/kani", "commands/mutants"):
            with self.subTest(gate=gate, report="good"):
                self.assertEqual(reports.main([gate, str(self.root)]), 0)

    def test_cli_missing_report_fails(self) -> None:
        for gate in ("kani", "mutants", "commands/kani", "commands/mutants"):
            with self.subTest(gate=gate):
                completed = subprocess.run(
                    [sys.executable, str(SCRIPT), gate, str(self.root)],
                    capture_output=True,
                    text=True,
                    timeout=10,
                    check=False,
                )
                self.assertEqual(completed.returncode, 1)
                self.assertIn("invalid CI report", completed.stderr)

    def test_missing_empty_malformed_and_nonobject_reports(self) -> None:
        for gate in ("kani", "mutants"):
            with self.subTest(gate=gate, case="missing"):
                with self.assertRaises(ValueError):
                    reports.validate(gate, self.root)
            for text in ("", " ", "{", "null", "[]", "true", "{}", '{"x":NaN}'):
                with self.subTest(gate=gate, text=text):
                    self.write(gate, {}).write_text(text, encoding="utf-8")
                    with self.assertRaises(ValueError):
                        reports.validate(gate, self.root)

    def test_duplicate_json_keys_and_invalid_encoding(self) -> None:
        for gate in ("kani", "mutants"):
            for raw in (b'{"outcomes":[],"outcomes":[]}', b"\xff"):
                with self.subTest(gate=gate, raw=raw):
                    self.write(gate, {}).write_bytes(raw)
                    with self.assertRaises((ValueError, UnicodeError)):
                        reports.validate(gate, self.root)

    def test_oversized_reports_fail(self) -> None:
        original = reports.MAX_REPORT_BYTES
        self.addCleanup(setattr, reports, "MAX_REPORT_BYTES", original)
        reports.MAX_REPORT_BYTES = 1
        self.rejected("kani", kani_report())

    def test_other_gates_need_no_report(self) -> None:
        for gate in ("fmt", "commands/clippy", "nextest", "doctest", "unknown"):
            with self.subTest(gate=gate):
                self.assertEqual(reports.main([gate, str(self.root)]), 0)

    def test_main_reports_malformed_input_without_traceback(self) -> None:
        self.write("kani", {}).write_text("{", encoding="utf-8")
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            self.assertEqual(reports.main(["kani", str(self.root)]), 1)
        self.assertIn("invalid CI report", stderr.getvalue())


class KaniTests(ReportCase):
    def test_another_project_needs_only_consistent_complete_results(self) -> None:
        (self.root / reports.ATELIER_PROOFS).unlink()
        self.write("kani", kani_report(["proofs::holds"], crate="other"))
        reports.validate("commands/kani", self.root)
        self.rejected("kani", kani_report([], crate="other"))

    def test_atelier_requires_both_core_proofs(self) -> None:
        names = sorted(reports.ATELIER_HARNESSES)
        self.rejected("kani", kani_report(names[:1]))
        self.write("kani", kani_report([*names, "machine::proofs::another"]))
        reports.validate("kani", self.root)

    def test_summary_must_be_complete_and_consistent(self) -> None:
        for field, value in (
            ("status", "incomplete"),
            ("status", "failed"),
            ("executed", 1),
            ("successful", 1),
            ("total_harnesses", 0),
            ("total_harnesses", 3),
            ("failed", 1),
            ("failed", False),
            ("executed", 2.0),
        ):
            with self.subTest(field=field, value=value):
                report = kani_report()
                report["verification_results"]["summary"][field] = value
                self.rejected("kani", report)

    def test_missing_fields_fail(self) -> None:
        good = kani_report()
        for field in good:
            with self.subTest(field=field):
                report = copy.deepcopy(good)
                del report[field]
                self.rejected("kani", report)

    def test_every_array_must_cover_actual_harnesses_once(self) -> None:
        for key in ("harness_metadata", "error_details", "property_details", "cbmc"):
            for mode in ("empty", "partial", "duplicate", "unrelated"):
                with self.subTest(key=key, mode=mode):
                    report = kani_report()
                    if mode == "empty":
                        report[key] = []
                    elif mode == "partial":
                        report[key].pop()
                    elif mode == "duplicate":
                        report[key][1] = copy.deepcopy(report[key][0])
                    else:
                        identity = "harness_id"
                        if key == "harness_metadata":
                            identity = "pretty_name"
                        report[key][0][identity] = "unrelated::proof"
                    self.rejected("kani", report)

    def test_results_require_both_real_harnesses_and_success(self) -> None:
        for mode in ("empty", "partial", "duplicate", "unrelated", "failure"):
            with self.subTest(mode=mode):
                report = kani_report()
                results = report["verification_results"]["results"]
                if mode == "empty":
                    results.clear()
                elif mode == "partial":
                    results.pop()
                elif mode == "duplicate":
                    results[1] = copy.deepcopy(results[0])
                elif mode == "unrelated":
                    results[0]["harness_id"] = "a_run_follows_what_rungs_answers"
                else:
                    results[0]["status"] = "Failure"
                self.rejected("kani", report)

    def test_checks_cannot_be_empty_failed_or_unknown(self) -> None:
        for checks in (
            [],
            [{"id": 1, "status": "Failure"}],
            [{"id": 1, "status": "Undetermined"}],
            [{"id": 1, "status": "Error"}],
            [{"id": 1, "status": "Unexpected"}],
            [{"id": 2, "status": "Success"}],
        ):
            with self.subTest(checks=checks):
                report = kani_report()
                report["verification_results"]["results"][0]["checks"] = checks
                self.rejected("kani", report)

    def test_unreachable_checks_are_accounted_for(self) -> None:
        report = kani_report()
        report["verification_results"]["results"][0]["checks"].append(
            {"id": 2, "status": "Unreachable"}
        )
        properties = report["property_details"][0]["property_details"]
        properties["total_properties"] = 2
        properties["unreachable"] = 1
        self.write("kani", report)
        reports.validate("kani", self.root)

    def test_property_counts_and_error_details_fail_closed(self) -> None:
        for field, value in (
            ("total_properties", 0),
            ("passed", 0),
            ("failed", 1),
            ("undetermined", 1),
            ("solver_error", 1),
            ("passed", None),
            ("error", "extraction failed"),
        ):
            with self.subTest(field=field, value=value):
                report = kani_report()
                report["property_details"][0]["property_details"][field] = value
                self.rejected("kani", report)
        report = kani_report()
        report["error_details"][0]["has_errors"] = True
        self.rejected("kani", report)

    def test_wrong_crate_version_and_panicking_harness_fail(self) -> None:
        report = kani_report()
        report["metadata"]["kani_version"] = "0.67.0"
        self.rejected("kani", report)
        report = kani_report()
        report["harness_metadata"][0]["crate_name"] = "other"
        self.rejected("kani", report)
        report = kani_report()
        report["harness_metadata"][0]["attributes"]["should_panic"] = True
        self.rejected("kani", report)


class MutantsTests(ReportCase):
    def test_zero_mutants_is_not_a_successful_diff(self) -> None:
        report = mutants_report()
        report.update(total_mutants=0, caught=0, unviable=0)
        report["outcomes"] = report["outcomes"][:1]
        self.rejected("mutants", report)

    def test_empty_partial_duplicate_and_unknown_outcomes_fail(self) -> None:
        for mode in ("empty", "partial", "duplicate", "unknown", "no-baseline"):
            with self.subTest(mode=mode):
                report = mutants_report()
                outcomes = report["outcomes"]
                if mode == "empty":
                    outcomes.clear()
                elif mode == "partial":
                    outcomes.pop()
                elif mode == "duplicate":
                    outcomes[2] = copy.deepcopy(outcomes[1])
                elif mode == "unknown":
                    outcomes[1]["scenario"] = "unknown"
                else:
                    outcomes[0] = copy.deepcopy(outcomes[1])
                    outcomes[0]["scenario"]["Mutant"]["name"] = "src/lib.rs:3: other"
                    report["caught"] = 2
                self.rejected("mutants", report)

    def test_missing_fields_and_unfinished_reports_fail(self) -> None:
        for field in mutants_report():
            with self.subTest(field=field):
                report = mutants_report()
                del report[field]
                self.rejected("mutants", report)
        for end in (None, "", "invalid", "2026-11-12T00:00:00Z"):
            with self.subTest(end=end):
                report = mutants_report()
                report["end_time"] = end
                self.rejected("mutants", report)

    def test_counters_must_match_and_have_integer_types(self) -> None:
        for field in ("total_mutants", "caught", "unviable", "missed", "timeout", "success"):
            for value in (-1, False, "0", None, 99):
                with self.subTest(field=field, value=value):
                    report = mutants_report()
                    report[field] = value
                    self.rejected("mutants", report)

    def test_missed_timeouts_and_failures_are_rejected(self) -> None:
        for summary in ("MissedMutant", "Timeout", "Failure", "Success", "unknown"):
            with self.subTest(summary=summary):
                report = mutants_report()
                report["outcomes"][1]["summary"] = summary
                self.rejected("mutants", report)

    def test_failed_baseline_is_rejected(self) -> None:
        report = mutants_report()
        report["outcomes"][0]["summary"] = "Failure"
        self.rejected("mutants", report)
        report = mutants_report()
        report["outcomes"][0]["phase_results"] = phases({"Failure": 100})
        self.rejected("mutants", report)

    def test_incomplete_or_contradictory_phase_evidence_fails(self) -> None:
        for index, phase_results in (
            (0, []),
            (0, [{"phase": "Check", "process_status": "Success"}]),
            (1, phases()),
            (1, phases("Timeout")),
            (1, phases({"Failure": 0})),
            (1, phases({"Signalled": 9})),
            (2, phases("Success", unviable=True)),
            (2, []),
        ):
            with self.subTest(index=index, phases=phase_results):
                report = mutants_report()
                report["outcomes"][index]["phase_results"] = phase_results
                self.rejected("mutants", report)

    def test_unviable_mutants_are_accepted(self) -> None:
        report = mutants_report()
        report["outcomes"].pop(1)
        report.update(total_mutants=1, caught=0)
        self.write("mutants", report)
        reports.validate("mutants", self.root)


if __name__ == "__main__":
    unittest.main()

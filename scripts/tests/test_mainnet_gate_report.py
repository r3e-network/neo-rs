import argparse
import importlib.util
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).resolve().parents[1] / "mainnet_gate_report.py"


def load_module():
    spec = importlib.util.spec_from_file_location("mainnet_gate_report", MODULE_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def gate_output(span):
    return (
        "file exists : True\n"
        f"present : {span}\n"
        "missing : 0\n"
        "duplicates : 0\n"
        "malformed : 0\n"
        "out-of-range : 0\n"
        "GATE: PASS\n"
    )


def gate_loaded(start, end):
    return (
        f"loaded {end - start + 1} reference state roots for validation "
        f"covering heights [{start}, {end}]\n"
    )


class MainnetGateReportTests(unittest.TestCase):
    def setUp(self):
        self.module = load_module()
        tempdir = tempfile.TemporaryDirectory()
        self.addCleanup(tempdir.cleanup)
        self.tempdir = Path(tempdir.name)

    def make_args(self, import_text, import_exit_code=0, range_end=2, with_gate=True):
        gate_path = self.tempdir / "gate.log"
        import_path = self.tempdir / "import.log"
        if with_gate:
            gate_path.write_text(gate_output(range_end + 1), encoding="utf-8")
        import_path.write_text(import_text, encoding="utf-8")
        return argparse.Namespace(
            range_start=0,
            range_end=range_end,
            gate_output=str(gate_path) if with_gate else None,
            gate_ok=True,
            import_log=str(import_path),
            import_exit_code=import_exit_code,
            reference_file="reference.jsonl",
            reference_count=range_end + 1,
            config=None,
            store=None,
            acc_file=None,
            expected_stops=[],
            log_tail_n=0,
        )

    def test_gate_rejects_missing_or_fake_pass_marker(self):
        for replacement in ("GATE: FAIL\n", "GATE: PASSWORD\n"):
            with self.subTest(replacement=replacement.strip()):
                text = gate_output(3).replace("GATE: PASS\n", replacement)
                parsed = self.module.parse_gate_output(text)
                self.assertFalse(parsed["gate_pass_exact"])
                self.assertFalse(self.module.gate_effectively_passed(parsed, 3))

    def test_gate_tolerates_only_out_of_range_failures(self):
        base = gate_output(3).replace("out-of-range : 0\n", "out-of-range : 5\n")
        tolerated = base.replace("GATE: PASS\n", "GATE: FAIL\n  - 5 out-of-range heights\n")
        parsed = self.module.parse_gate_output(tolerated)
        self.assertEqual(parsed["gate_fail_reasons"], ["5 out-of-range heights"])
        self.assertTrue(self.module.gate_effectively_passed(parsed, 3))

        for reasons in (
            "  - 5 out-of-range heights\n  - 2 recorded fetch failures\n",
            "  - something unexpected\n",
            "",
        ):
            with self.subTest(reasons=reasons):
                text = base.replace("GATE: PASS\n", "GATE: FAIL\n" + reasons)
                parsed = self.module.parse_gate_output(text)
                self.assertFalse(self.module.gate_effectively_passed(parsed, 3))

    def test_gate_requires_explicit_zero_counters_and_exact_span(self):
        parsed = self.module.parse_gate_output(gate_output(3))
        self.assertTrue(self.module.gate_effectively_passed(parsed, 3))

        for key in ("present", "missing", "duplicates", "malformed"):
            with self.subTest(missing_counter=key):
                incomplete = dict(parsed)
                incomplete[key] = None
                self.assertFalse(self.module.gate_effectively_passed(incomplete, 3))

        extra_record = dict(parsed, present=4)
        self.assertFalse(self.module.gate_effectively_passed(extra_record, 3))

    def test_report_rejects_stale_final_height_only_log(self):
        args = self.make_args('"final_height":2\n')
        report, rc = self.module.build_report(args)
        self.assertEqual(rc, 1)
        self.assertEqual(report["status"], "partial")
        self.assertEqual(report["import"]["final_height"], 2)
        self.assertIsNone(report["verified_contiguous_prefix"])

    def test_exact_clean_without_loaded_gate_marker_is_rejected(self):
        args = self.make_args('"final_height":2\ncompleted .acc import\n')
        report, rc = self.module.build_report(args)
        self.assertEqual(rc, 1)
        self.assertEqual(report["status"], "partial")
        self.assertFalse(report["reference"]["gate"]["active"])
        self.assertIsNone(report["verified_contiguous_prefix"])

    def test_clean_with_covering_loaded_gate_marker_is_accepted(self):
        args = self.make_args(
            gate_loaded(0, 2) + '"final_height":2\ncompleted .acc import\n'
        )
        report, rc = self.module.build_report(args)
        self.assertEqual(rc, 0)
        self.assertEqual(report["status"], "clean")
        self.assertTrue(report["reference"]["gate"]["active"])
        self.assertEqual(report["reference"]["gate"]["count"], 3)
        self.assertEqual(report["reference"]["gate"]["range"], [0, 2])
        self.assertEqual(
            report["verified_contiguous_prefix"], {"start": 0, "end": 2}
        )

    def test_clean_with_non_covering_loaded_gate_marker_is_rejected(self):
        args = self.make_args(
            gate_loaded(0, 1) + '"final_height":2\ncompleted .acc import\n'
        )
        report, rc = self.module.build_report(args)
        self.assertEqual(rc, 1)
        self.assertEqual(report["status"], "partial")
        self.assertEqual(report["reference"]["gate"]["range"], [0, 1])
        self.assertIsNone(report["verified_contiguous_prefix"])

    def test_bounded_partial_run_has_null_prefix(self):
        args = self.make_args('"final_height":1\ncompleted .acc import\n')
        report, rc = self.module.build_report(args)
        self.assertEqual(rc, 1)
        self.assertEqual(report["status"], "partial")
        self.assertEqual(report["import"]["final_height"], 1)
        self.assertIsNone(report["verified_contiguous_prefix"])

    def test_report_requires_actual_gate_output(self):
        args = self.make_args(
            '"final_height":2\ncompleted .acc import\n', with_gate=False
        )
        report, rc = self.module.build_report(args)
        self.assertEqual(rc, 1)
        self.assertEqual(report["status"], "gate_failed")
        self.assertFalse(report["reference"]["gate"]["passed"])
        self.assertIsNone(report["reference"]["gate"]["raw"])
        self.assertIsNone(report["verified_contiguous_prefix"])

    def test_divergence_does_not_claim_prefix_on_resumed_store(self):
        computed = "0x" + "11" * 32
        expected = "0x" + "22" * 32
        mismatch = (
            f"state root mismatch at height 1: computed {computed}, "
            f"expected {expected}, put_count 1, del_count 0\n"
        )
        args = self.make_args(
            gate_loaded(0, 2) + mismatch, import_exit_code=1
        )
        report, rc = self.module.build_report(args)
        self.assertEqual(rc, 1)
        self.assertEqual(report["status"], "diverged")
        self.assertEqual(report["first_divergence"]["height"], 1)
        self.assertIsNone(report["verified_contiguous_prefix"])

    def test_aborted_flush_checkpoint_does_not_claim_prefix(self):
        args = self.make_args('"persisted_height":1\n', import_exit_code=1)
        report, rc = self.module.build_report(args)
        self.assertEqual(rc, 1)
        self.assertEqual(report["status"], "aborted")
        self.assertEqual(report["import"]["last_flush_height"], 1)
        self.assertIsNone(report["verified_contiguous_prefix"])


if __name__ == "__main__":
    unittest.main()

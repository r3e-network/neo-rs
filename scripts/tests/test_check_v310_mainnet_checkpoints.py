import importlib.util
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).resolve().parents[1] / "check-v310-mainnet-checkpoints.py"


def load_module():
    spec = importlib.util.spec_from_file_location(
        "check_v310_mainnet_checkpoints", MODULE_PATH
    )
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class CheckV310MainnetCheckpointsTests(unittest.TestCase):
    def test_all_pending_fails_by_default(self):
        module = load_module()
        result, reasons = module.checkpoint_validation_result(
            verified=0,
            pending=len(module.CHECKPOINTS),
            failures=[],
        )
        self.assertEqual(result, "FAIL")
        self.assertTrue(any("pending" in reason for reason in reasons))

    def test_explicit_diagnostic_mode_reports_incomplete(self):
        module = load_module()
        result, reasons = module.checkpoint_validation_result(
            verified=1,
            pending=len(module.CHECKPOINTS) - 1,
            failures=[],
            allow_incomplete=True,
        )
        self.assertEqual(result, "INCOMPLETE")
        self.assertTrue(any("pending" in reason for reason in reasons))

    def test_missing_application_log_remains_a_failure(self):
        module = load_module()
        failure = "checkpoint local application log unavailable"
        result, reasons = module.checkpoint_validation_result(
            verified=len(module.CHECKPOINTS) - 1,
            pending=0,
            failures=[failure],
            allow_incomplete=True,
        )
        self.assertEqual(result, "FAIL")
        self.assertIn(failure, reasons)

    def test_every_checkpoint_verified_passes(self):
        module = load_module()
        self.assertEqual(
            module.checkpoint_validation_result(
                verified=len(module.CHECKPOINTS),
                pending=0,
                failures=[],
            ),
            ("PASS", []),
        )

    def test_nested_application_artifact_difference_is_reported(self):
        module = load_module()
        local = {"executions": [{"stack": [{"type": "Integer", "value": "7"}]}]}
        public = {"executions": [{"stack": [{"type": "Integer", "value": "8"}]}]}
        differences = module.application_log_differences(local, public)
        self.assertTrue(any("stack[0].value" in difference for difference in differences))

    def test_application_envelope_is_ignored_but_execution_data_is_not(self):
        module = load_module()
        local = {"txid": "0xlocal", "executions": []}
        public = {"txid": "0xpublic", "executions": []}
        self.assertEqual(module.application_log_differences(local, public), [])


if __name__ == "__main__":
    unittest.main()

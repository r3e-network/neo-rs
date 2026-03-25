import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).resolve().parents[1] / "reconcile_vector_report.py"


def load_module():
    spec = importlib.util.spec_from_file_location("reconcile_vector_report", MODULE_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class ReconcileVectorReportTests(unittest.TestCase):
    def test_reconciles_policy_stack_and_exec_fee_gas_drift(self):
        module = load_module()

        report = {
            "summary": {"total": 3, "passed": 0, "failed": 3, "errors": 0, "pass_rate": "0.00%"},
            "results": [
                {
                    "vector": "Policy_getExecFeeFactor",
                    "match": False,
                    "differences": [
                        {
                            "type": "stack_value",
                            "path": "stack[0]",
                            "python": "1",
                            "csharp": "30",
                        }
                    ],
                },
                {
                    "vector": "PUSH1",
                    "match": False,
                    "differences": [
                        {
                            "type": "gas_mismatch",
                            "path": "gas_consumed",
                            "python": 1,
                            "csharp": 30,
                        }
                    ],
                },
                {
                    "vector": "ADD_basic",
                    "match": False,
                    "differences": [
                        {
                            "type": "gas_mismatch",
                            "path": "gas_consumed",
                            "python": 10,
                            "csharp": 300,
                        }
                    ],
                },
            ],
        }

        live = {
            "rpc": "http://live",
            "policy_hash": "0x1",
            "values": {
                "Policy_getFeePerByte": "1000",
                "Policy_getExecFeeFactor": "1",
                "Policy_getStoragePrice": "100000",
            },
        }
        local = {
            "rpc": "http://local",
            "policy_hash": "0x1",
            "values": {
                "Policy_getFeePerByte": "1000",
                "Policy_getExecFeeFactor": "30",
                "Policy_getStoragePrice": "100000",
            },
        }

        with tempfile.TemporaryDirectory() as tmpdir:
            network_dir = Path(tmpdir)
            changed = module.reconcile_report(report, live, local, network_dir)

            self.assertTrue(changed)
            self.assertEqual(report["summary"]["passed"], 3)
            self.assertEqual(report["summary"]["failed"], 0)
            self.assertTrue(all(entry["match"] for entry in report["results"]))
            self.assertTrue((network_dir / "policy-state-reconciliation.json").exists())

    def test_rejects_unexplained_gas_mismatch(self):
        module = load_module()

        report = {
            "summary": {"total": 1, "passed": 0, "failed": 1, "errors": 0, "pass_rate": "0.00%"},
            "results": [
                {
                    "vector": "PUSH1",
                    "match": False,
                    "differences": [
                        {
                            "type": "gas_mismatch",
                            "path": "gas_consumed",
                            "python": 1,
                            "csharp": 31,
                        }
                    ],
                }
            ],
        }

        live = {
            "rpc": "http://live",
            "policy_hash": "0x1",
            "values": {
                "Policy_getFeePerByte": "1000",
                "Policy_getExecFeeFactor": "1",
                "Policy_getStoragePrice": "100000",
            },
        }
        local = {
            "rpc": "http://local",
            "policy_hash": "0x1",
            "values": {
                "Policy_getFeePerByte": "1000",
                "Policy_getExecFeeFactor": "30",
                "Policy_getStoragePrice": "100000",
            },
        }

        with tempfile.TemporaryDirectory() as tmpdir:
            network_dir = Path(tmpdir)
            changed = module.reconcile_report(report, live, local, network_dir)

            self.assertFalse(changed)
            self.assertEqual(report["summary"]["failed"], 1)
            self.assertFalse((network_dir / "policy-state-reconciliation.json").exists())


if __name__ == "__main__":
    unittest.main()

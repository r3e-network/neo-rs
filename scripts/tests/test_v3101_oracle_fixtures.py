import json
import subprocess
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
VM_FIXTURE = REPO_ROOT / "neo-vm/tests/fixtures/csharp-v3.10.1-vm.json"
APPLICATION_FIXTURE = (
    REPO_ROOT / "neo-execution/tests/fixtures/csharp-v3.10.1-application.json"
)
VERIFY_SCRIPT = REPO_ROOT / "scripts/oracles/v3101/verify-recorded.py"

EXPECTED_CASES = {
    VM_FIXTURE: {
        "implicit_ret_exact",
        "implicit_ret_too_few",
        "implicit_ret_too_many",
        "relaxed_unreachable_malformed",
        "strict_unreachable_malformed",
        "strict_jump_to_end",
        "strict_convert_any",
        "context_at_script_end",
        "context_beyond_script_end",
        "call_to_script_end",
        "jump_to_script_end",
        "try_target_beyond_script_end",
        "endtry_target_beyond_script_end",
        "null_convert_map",
        "null_convert_pointer",
        "null_convert_interopinterface",
        "invalid_slot_store_preserves_operand",
        "invalid_static_index_preserves_operand",
        "invalid_local_index_preserves_operand",
        "invalid_argument_index_preserves_operand",
        "unhandled_throw_preserves_frames",
        "abortmsg_valid_utf8",
        "abortmsg_invalid_utf8",
    },
    APPLICATION_FIXTURE: {
        "runtime_load_script_invalid_jump_pre_basilisk",
        "runtime_load_script_convert_any_pre_basilisk",
        "runtime_load_script_invalid_jump_post_basilisk",
        "runtime_load_script_convert_any_post_basilisk",
        "fault_clears_notifications",
        "script_builder_struct_uses_packstruct",
        "jump_table_before_echidna",
        "jump_table_echidna_before_gorgon",
        "jump_table_gorgon_and_later",
    },
}

EXPECTED_ORACLES = {
    VM_FIXTURE: (
        "https://github.com/neo-project/neo-vm.git",
        "004cd6070a940405818d9357638277dd44407e2e",
    ),
    APPLICATION_FIXTURE: (
        "https://github.com/neo-project/neo.git",
        "d10e9ceecdabe3fcff719ee68ea5b76ba7e62c3d",
    ),
}


def load(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


class V3101OracleFixtureTests(unittest.TestCase):
    def test_fixture_case_sets_are_complete_and_exact(self):
        for path, expected_ids in EXPECTED_CASES.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                document = load(path)
                cases = document["cases"]
                case_ids = [case["id"] for case in cases]
                self.assertEqual(len(case_ids), len(set(case_ids)))
                self.assertEqual(set(case_ids), expected_ids)
                for case in cases:
                    self.assertIsInstance(case.get("operation"), str)
                    self.assertTrue(case.get("hardforks"))
                    self.assertIsInstance(case.get("observed"), dict)

    def test_fixture_oracles_are_pinned_and_generators_are_retained(self):
        for path, (repository, commit) in EXPECTED_ORACLES.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                oracle = load(path)["oracle"]
                self.assertEqual(oracle["repository"], repository)
                self.assertEqual(oracle["commit"], commit)
                self.assertEqual(oracle["version"], "3.10.1")
                generator = REPO_ROOT / oracle["generator"]
                self.assertTrue(generator.is_file(), generator)

    def test_recorded_verifier_accepts_exact_output_and_rejects_drift(self):
        fixture = load(VM_FIXTURE)
        recorded = {
            "schema": fixture["schema"],
            "oracle": {
                key: fixture["oracle"][key]
                for key in ("repository", "commit", "version")
            },
            "cases": [
                {
                    "id": case["id"],
                    "operation": case["operation"],
                    "observed": case["observed"],
                }
                for case in fixture["cases"]
            ],
        }

        with tempfile.TemporaryDirectory() as temp_dir:
            recorded_path = Path(temp_dir) / "recorded.json"
            recorded_path.write_text(json.dumps(recorded), encoding="utf-8")
            exact = subprocess.run(
                ["python3", str(VERIFY_SCRIPT), str(VM_FIXTURE), str(recorded_path)],
                check=False,
                capture_output=True,
                text=True,
            )
            self.assertEqual(exact.returncode, 0, exact.stderr)

            recorded["cases"][0]["observed"] = {"state": "FAULT"}
            recorded_path.write_text(json.dumps(recorded), encoding="utf-8")
            drifted = subprocess.run(
                ["python3", str(VERIFY_SCRIPT), str(VM_FIXTURE), str(recorded_path)],
                check=False,
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(drifted.returncode, 0)
            self.assertIn("observed result mismatch", drifted.stderr)


if __name__ == "__main__":
    unittest.main()

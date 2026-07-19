import importlib.util
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).resolve().parents[1] / "plan-mainnet-validation-stack.py"
REPO_ROOT = Path(__file__).resolve().parents[2]


def load_module():
    spec = importlib.util.spec_from_file_location("plan_mainnet_validation_stack", MODULE_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class MainnetValidationStackPlanTests(unittest.TestCase):
    def test_plan_includes_node_validator_and_checkpoint_commands(self):
        module = load_module()

        plan = module.build_plan(
            node_config=Path("neo_mainnet_validate.toml"),
            node_bin=Path("target/release/neo-node"),
            status_file=Path("/tmp/stateroot-validation.json"),
            resume_file=Path("/tmp/stateroot-last-validated"),
            log_dir=Path("logs/mainnet-validation"),
            batch=250,
            poll_interval=3,
            checkpoint_execute=False,
        )

        self.assertEqual(plan["mode"], "dry-run")
        self.assertEqual(
            plan["steps"][0]["command"],
            [
                "target/release/neo-node",
                "--config",
                "neo_mainnet_validate.toml",
                "--enable-stateroot",
                "--check-all",
            ],
        )
        self.assertIn("StateService MPT height", plan["steps"][0]["failure_hint"])
        self.assertEqual(
            plan["steps"][1]["command"],
            [
                "target/release/neo-node",
                "--config",
                "neo_mainnet_validate.toml",
                "--enable-stateroot",
            ],
        )

        validator_command = plan["steps"][2]["command"]
        self.assertIn("scripts/continuous-stateroot-validation.py", validator_command)
        self.assertIn("--local-config", validator_command)
        self.assertIn("neo_mainnet_validate.toml", validator_command)
        self.assertIn("--status-file", validator_command)
        self.assertIn("/tmp/stateroot-validation.json", validator_command)
        self.assertIn("--batch", validator_command)
        self.assertIn("250", validator_command)

        checkpoint_command = plan["steps"][3]["command"]
        self.assertIn("scripts/maintain-stateroot-checkpoints.py", checkpoint_command)
        self.assertIn("--node-config", checkpoint_command)
        self.assertIn("neo_mainnet_validate.toml", checkpoint_command)
        self.assertIn("--writer-pid", checkpoint_command)
        self.assertIn("<neo-node-pid>", checkpoint_command)
        self.assertIn("--watch-interval", checkpoint_command)
        self.assertIn("600", checkpoint_command)
        self.assertIn("--waiting-interval", checkpoint_command)
        self.assertIn("30", checkpoint_command)
        self.assertNotIn("--execute", checkpoint_command)

    def test_checkpoint_execute_flag_is_explicit_in_plan(self):
        module = load_module()

        plan = module.build_plan(
            node_config=Path("neo_mainnet_validate.toml"),
            node_bin=Path("target/release/neo-node"),
            status_file=Path("/tmp/stateroot-validation.json"),
            resume_file=Path("/tmp/stateroot-last-validated"),
            log_dir=Path("logs/mainnet-validation"),
            batch=500,
            poll_interval=5,
            checkpoint_execute=True,
        )

        self.assertIn("--execute", plan["steps"][3]["command"])

    def test_operations_doc_mentions_validation_stack_plan(self):
        text = (REPO_ROOT / "docs" / "operations.md").read_text(encoding="utf-8")

        self.assertIn("scripts/plan-mainnet-validation-stack.py", text)
        self.assertIn("scripts/run-mainnet-validation-stack.py", text)
        self.assertIn("scripts/prepare-clean-stateroot-validation.py", text)
        self.assertIn("state-root validator", text)
        self.assertIn("checkpoint maintainer", text)
        self.assertIn("--start", text)
        self.assertIn("--status", text)
        self.assertIn("--stop", text)


if __name__ == "__main__":
    unittest.main()

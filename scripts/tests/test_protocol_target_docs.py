import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]


class ProtocolTargetDocsTests(unittest.TestCase):
    def test_release_guide_names_current_neo_n3_target(self):
        text = (REPO_ROOT / "docs" / "RELEASE.md").read_text(encoding="utf-8")

        self.assertIn("currently v3.10.0", text)
        self.assertNotIn("currently v3.9.1", text)

    def test_csharp_compatibility_tests_do_not_treat_treasury_as_noncanonical(self):
        text = (
            REPO_ROOT / "tests" / "tests" / "csharp_compatibility_tests.rs"
        ).read_text(encoding="utf-8")

        self.assertNotIn("not in C# Neo v3.9.1's standard set", text)

    def test_active_protocol_utility_scripts_name_current_neo_n3_target(self):
        paths = [
            REPO_ROOT / "scripts" / "check-v391-mainnet-checkpoints.py",
            REPO_ROOT / "scripts" / "check-v310-mainnet-checkpoints.py",
            REPO_ROOT / "scripts" / "compare_fee_calculation_with_csharp.py",
            REPO_ROOT / "scripts" / "verify_fee_calculation.py",
        ]

        for path in paths:
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                text = path.read_text(encoding="utf-8")
                self.assertIn("v3.10.0", text)
                self.assertNotIn("v3.9.1", text)
                self.assertNotIn("v391-checkpoints", text)

    def test_legacy_v391_checkpoint_script_delegates_to_v310_script(self):
        legacy = REPO_ROOT / "scripts" / "check-v391-mainnet-checkpoints.py"
        canonical = REPO_ROOT / "scripts" / "check-v310-mainnet-checkpoints.py"

        self.assertTrue(canonical.exists())
        self.assertIn("check-v310-mainnet-checkpoints.py", legacy.read_text(encoding="utf-8"))

    def test_consistency_validator_uses_current_neo_n3_target(self):
        legacy = REPO_ROOT / "scripts" / "validate-v391-consistency.sh"
        canonical = REPO_ROOT / "scripts" / "validate-v310-consistency.sh"

        self.assertTrue(canonical.exists())
        canonical_text = canonical.read_text(encoding="utf-8")
        self.assertIn("Neo v3.10.0", canonical_text)
        self.assertIn("Neo:3.10.0", canonical_text)
        self.assertIn("compat-v310", canonical_text)
        self.assertNotIn("Neo v3.9.1", canonical_text)
        self.assertNotIn("Neo:3.9.1", canonical_text)
        self.assertNotIn("compat-v391", canonical_text)

        self.assertIn("validate-v310-consistency.sh", legacy.read_text(encoding="utf-8"))

    def test_current_protocol_compliance_spec_names_current_neo_n3_target(self):
        text = (
            REPO_ROOT / "openspec" / "specs" / "protocol-compliance-audit" / "spec.md"
        ).read_text(encoding="utf-8")

        self.assertIn("Neo N3 v3.10.0", text)
        self.assertNotIn("Neo N3 v3.9.1", text)

    def test_rpc_relay_height_preclassification_comment_names_current_reference(self):
        text = (
            REPO_ROOT / "neo-rpc" / "src" / "server" / "rpc_relay.rs"
        ).read_text(encoding="utf-8")

        self.assertIn("height pre-classification (v3.10.0)", text)
        self.assertNotIn("height pre-classification (v3.9.1)", text)

    def test_consistency_workflow_names_current_neo_n3_target(self):
        workflows = REPO_ROOT / ".github" / "workflows"
        canonical = workflows / "compatibility-v310.yml"
        legacy = workflows / "compatibility-v391.yml"

        self.assertTrue(
            canonical.exists(),
            "compatibility-v310.yml must be the canonical consistency workflow",
        )
        self.assertFalse(
            legacy.exists(),
            "compatibility-v391.yml must be removed in favour of compatibility-v310.yml",
        )

        text = canonical.read_text(encoding="utf-8")
        self.assertIn("Neo v3.10.0 Consistency", text)
        self.assertIn("validate-v310-consistency.sh", text)
        # The artifact upload path must match the directory the validator writes
        # to (reports/compat-v310); a stale compat-v391 path silently drops them.
        self.assertIn("reports/compat-v310", text)
        self.assertNotIn("Neo v3.9.1 Consistency", text)
        self.assertNotIn("compat-v391", text)
        self.assertNotIn("v391-consistency-", text)


if __name__ == "__main__":
    unittest.main()

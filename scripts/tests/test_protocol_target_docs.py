import re
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
COMPATIBILITY_TRIGGER_PATHS = [
    ".github/workflows/compatibility-v310.yml",
    "scripts/validate-v310-consistency.sh",
    "Cargo.lock",
    "Cargo.toml",
    "neo-blockchain/**",
    "neo-config/**",
    "neo-consensus/**",
    "neo-crypto/**",
    "neo-error/**",
    "neo-execution/**",
    "neo-indexer/**",
    "neo-io/**",
    "neo-manifest/**",
    "neo-mempool/**",
    "neo-native-contracts/**",
    "neo-network/**",
    "neo-node/**",
    "neo-oracle-service/**",
    "neo-payloads/**",
    "neo-primitives/**",
    "neo-rpc/**",
    "neo-runtime/**",
    "neo-serialization/**",
    "neo-state-service/**",
    "neo-static-files/**",
    "neo-storage/**",
    "neo-system/**",
    "neo-vm/**",
    "neo-wallets/**",
]
FUZZ_TRIGGER_PATHS = [
    "Cargo.lock",
    "Cargo.toml",
    "neo-config/**",
    "neo-crypto/**",
    "neo-error/**",
    "neo-io/**",
    "neo-manifest/**",
    "neo-network/**",
    "neo-payloads/**",
    "neo-primitives/**",
    "neo-runtime/**",
    "neo-serialization/**",
    "neo-storage/**",
    "neo-vm/**",
    "fuzz/**",
    ".github/workflows/fuzz.yml",
]


def workflow_block(text: str, key: str, indent: int) -> str:
    lines = text.splitlines()
    header = f"{' ' * indent}{key}:"
    starts = [index for index, line in enumerate(lines) if line == header]
    if len(starts) != 1:
        raise AssertionError(f"expected one {header!r} block, found {len(starts)}")

    start = starts[0]
    end = len(lines)
    for index in range(start + 1, len(lines)):
        line = lines[index]
        if line.strip() and len(line) - len(line.lstrip()) <= indent:
            end = index
            break
    return "\n".join(lines[start:end])


def workflow_trigger_paths(text: str, trigger: str) -> list[str]:
    trigger_block = workflow_block(text, trigger, 2)
    lines = trigger_block.splitlines()
    paths_header = "    paths:"
    try:
        start = lines.index(paths_header) + 1
    except ValueError as error:
        raise AssertionError(f"{trigger!r} trigger has no paths block") from error

    paths = []
    for line in lines[start:]:
        if line.startswith("    - "):
            paths.append(line.removeprefix("    - "))
        elif line.strip():
            break
    return paths


def assert_block_contains_once(test: unittest.TestCase, block: str, markers: list[str]):
    for marker in markers:
        with test.subTest(marker=marker):
            test.assertEqual(block.count(marker), 1)


def normalized(text: str) -> str:
    without_quote_prefixes = re.sub(r"(?m)^>\s?", "", text)
    return " ".join(without_quote_prefixes.split())


class ProtocolTargetDocsTests(unittest.TestCase):
    def test_ci_uses_locked_rust_and_tool_versions(self):
        text = (REPO_ROOT / ".github" / "workflows" / "ci.yml").read_text(
            encoding="utf-8"
        )

        expected_markers = {
            "fmt": [
                "dtolnay/rust-toolchain@1.89.0",
                "cargo metadata --locked --no-deps --format-version 1",
            ],
            "clippy": [
                "dtolnay/rust-toolchain@1.89.0",
                "cargo clippy --workspace --all-targets --profile test --locked -- -D warnings",
            ],
            "test": [
                "dtolnay/rust-toolchain@1.89.0",
                "tool: cargo-nextest@0.9.128",
                "cargo test --workspace --no-run --locked",
                "cargo nextest run --workspace --no-fail-fast --locked",
                "cargo test --workspace --doc --locked",
            ],
            "dependency-policy": [
                "dtolnay/rust-toolchain@1.89.0",
                "tool: cargo-deny@0.18.9",
                "cargo metadata --locked --no-deps --format-version 1",
                "cargo metadata --manifest-path fuzz/Cargo.toml --locked --no-deps --format-version 1",
                "cargo deny check advisories licenses sources --hide-inclusion-graph",
                "cargo deny --manifest-path fuzz/Cargo.toml check advisories licenses sources --hide-inclusion-graph",
            ],
        }
        for job, markers in expected_markers.items():
            with self.subTest(job=job):
                assert_block_contains_once(self, workflow_block(text, job, 2), markers)

    def test_compatibility_workflow_preserves_failures_and_all_protocol_triggers(self):
        text = (
            REPO_ROOT / ".github" / "workflows" / "compatibility-v310.yml"
        ).read_text(encoding="utf-8")

        for trigger in ("push", "pull_request"):
            with self.subTest(trigger=trigger):
                self.assertEqual(
                    workflow_trigger_paths(text, trigger),
                    COMPATIBILITY_TRIGGER_PATHS,
                )

        job = workflow_block(text, "consistency", 2)
        assert_block_contains_once(
            self,
            job,
            ["dtolnay/rust-toolchain@1.89.0"],
        )
        self.assertRegex(
            job,
            re.compile(
                r"if bash scripts/validate-v310-consistency\.sh.*?\n"
                r"\s+rc=0\n\s+break\n\s+else\n\s+rc=\$\?\n"
                r"\s+echo \"attempt \$attempt failed \(rc=\$rc\)\"",
                re.DOTALL,
            ),
        )
        self.assertNotRegex(job, re.compile(r"\n\s*fi\n\s*rc=\$\?"))

    def test_consistency_validator_handles_unavailable_rpc_selection_explicitly(self):
        text = (REPO_ROOT / "scripts" / "validate-v310-consistency.sh").read_text(
            encoding="utf-8"
        )

        self.assertRegex(
            text,
            re.compile(r'csharp_rpc="\$\(select_rpc .*?\)" \|\| csharp_rpc=""'),
        )
        self.assertRegex(
            text,
            re.compile(r'neogo_rpc="\$\(select_rpc .*?\)" \|\| neogo_rpc=""'),
        )
        self.assertIn("REFERENCE-UNREACHABLE", text)
        self.assertIn("NOT a parity failure", text)

    def test_fuzz_workflow_pins_tools_and_protects_the_standalone_lock(self):
        text = (REPO_ROOT / ".github" / "workflows" / "fuzz.yml").read_text(
            encoding="utf-8"
        )

        for trigger in ("push", "pull_request"):
            with self.subTest(trigger=trigger):
                self.assertEqual(workflow_trigger_paths(text, trigger), FUZZ_TRIGGER_PATHS)

        markers = [
            "dtolnay/rust-toolchain@nightly-2025-11-30",
            "cargo install cargo-fuzz --version 0.13.1 --locked",
            "cargo metadata --locked --no-deps --format-version 1",
            'lock_before="$(sha256sum Cargo.lock',
            'lock_after="$(sha256sum Cargo.lock',
            'if [ "$lock_before" != "$lock_after" ]',
        ]
        for job in (
            "fuzz-transaction",
            "fuzz-script",
            "fuzz-message",
            "fuzz-smoke-test",
        ):
            with self.subTest(job=job):
                assert_block_contains_once(self, workflow_block(text, job, 2), markers)

    def test_release_guide_names_current_neo_n3_target(self):
        text = (REPO_ROOT / "docs" / "RELEASE.md").read_text(encoding="utf-8")

        self.assertIn("currently v3.10.1", text)
        self.assertNotIn("currently v3.9.1", text)

    def test_csharp_compatibility_tests_do_not_treat_treasury_as_noncanonical(self):
        text = (
            REPO_ROOT / "tests" / "tests" / "protocol" / "csharp_compatibility_tests.rs"
        ).read_text(encoding="utf-8")

        self.assertNotIn("not in C# Neo v3.9.1's standard set", text)

    def test_active_protocol_utility_scripts_name_current_neo_n3_target(self):
        paths = [
            REPO_ROOT / "scripts" / "check-v310-mainnet-checkpoints.py",
            REPO_ROOT / "scripts" / "compare_fee_calculation_with_csharp.py",
            REPO_ROOT / "scripts" / "verify_fee_calculation.py",
        ]

        for path in paths:
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                text = path.read_text(encoding="utf-8")
                self.assertIn("v3.10.1", text)
                self.assertNotIn("v3.9.1", text)
                self.assertNotIn("v391-checkpoints", text)

    def test_legacy_v391_checkpoint_script_is_removed(self):
        legacy = REPO_ROOT / "scripts" / "check-v391-mainnet-checkpoints.py"
        canonical = REPO_ROOT / "scripts" / "check-v310-mainnet-checkpoints.py"

        self.assertTrue(canonical.exists())
        self.assertFalse(legacy.exists())

    def test_consistency_validator_uses_current_neo_n3_target(self):
        legacy = REPO_ROOT / "scripts" / "validate-v391-consistency.sh"
        canonical = REPO_ROOT / "scripts" / "validate-v310-consistency.sh"

        self.assertTrue(canonical.exists())
        canonical_text = canonical.read_text(encoding="utf-8")
        self.assertIn("Neo v3.10.1", canonical_text)
        self.assertIn("Neo:3.10.1", canonical_text)
        self.assertIn("compat-v310", canonical_text)
        self.assertNotIn("Neo v3.9.1", canonical_text)
        self.assertNotIn("Neo:3.9.1", canonical_text)
        self.assertNotIn("compat-v391", canonical_text)

        self.assertFalse(legacy.exists())

    def test_legacy_mainnet_smoke_scripts_delegate_to_strict_validation_gates(self):
        validate_mainnet = (REPO_ROOT / "scripts" / "validate-mainnet.sh").read_text(
            encoding="utf-8"
        )
        protocol_consistency = (
            REPO_ROOT / "scripts" / "protocol-consistency-test.sh"
        ).read_text(encoding="utf-8")

        for text in [validate_mainnet, protocol_consistency]:
            with self.subTest(script=text.splitlines()[1] if len(text.splitlines()) > 1 else ""):
                self.assertIn("validate-v310-consistency.sh", text)
                self.assertIn("mainnet-parity-check.sh", text)
                self.assertNotIn("Block #1000 hash verified", text)
                self.assertNotIn("Not available", text)

    def test_mainnet_parity_check_requires_state_root_evidence(self):
        text = (REPO_ROOT / "scripts" / "mainnet-parity-check.sh").read_text(
            encoding="utf-8"
        )

        self.assertIn("--- State roots ---", text)
        self.assertIn("getstateheight", text)
        self.assertIn("getstateroot", text)
        self.assertIn("extract_state_root_hash", text)
        self.assertIn("record_fail \"state root", text)
        self.assertNotIn("state root unavailable", text.lower())

    def test_current_protocol_compliance_spec_names_current_neo_n3_target(self):
        text = (
            REPO_ROOT / "openspec" / "specs" / "protocol-compliance-audit" / "spec.md"
        ).read_text(encoding="utf-8")

        self.assertIn("Neo N3 v3.10.1", text)
        self.assertNotIn("Neo N3 v3.9.1", text)

    def test_protocol_compatibility_audits_full_v3101_release_delta(self):
        text = (REPO_ROOT / "docs" / "protocol-compatibility.md").read_text(
            encoding="utf-8"
        )

        self.assertIn("d10e9ceecdabe3fcff719ee68ea5b76ba7e62c3d", text)
        self.assertIn("004cd6070a940405818d9357638277dd44407e2e", text)
        self.assertIn("v3.10.0...v3.10.1", text)
        for marker in {
            "df402675",
            "#4562",
            "d10e9cee",
            "#4575",
            "9f4795ab",
            "#4571",
            "f5ae5e82",
            "#4565",
            "e66e4dfc",
            "#4563",
            "6b1c90c6",
            "#4566",
            "7a8018e",
            "#581",
            "004cd60",
            "#587",
            "55c14029",
            "#4569",
            "abbc3a25",
            "#4570",
            "7f8454f4",
            "#4572",
            "7bb91ff5",
            "#4574",
            "HF_Huyao",
            "ApplicationEngine.AddFee",
            "StdLib.Itoa",
            "ReferenceCounter",
            "committee voter-reward",
            "Notary-sponsored",
            "ExtensiblePayload",
            "StorageKey.ToString",
        }:
            with self.subTest(marker=marker):
                self.assertIn(marker, text)

    def test_protocol_baseline_records_official_schedules_and_evidence_scope(self):
        text = (REPO_ROOT / "docs" / "protocol-compatibility.md").read_text(
            encoding="utf-8"
        )
        prose = normalized(text)

        for marker in (
            "Compatibility Target and Current Evidence",
            "7313f8087724e1de4caa88edd2ada58c1fe54abc",
            "explicitly loaded `Hardforks: {}`",
            "Full differential execution parity",
            "sustained live-peer interoperability",
            "complete MainNet replay and state parity",
            "authenticated checkpoint fast sync",
            "Code presence is not, by itself, evidence",
        ):
            with self.subTest(marker=marker):
                self.assertIn(marker, prose)

        self.assertIn("| HF_Gorgon | 12,020,000 | 17,960,000 |", text)
        self.assertIn("| HF_Huyao | not scheduled | not scheduled |", text)
        self.assertNotIn("not scheduled by preset", text)
        self.assertNotIn("## Neo N3 v3.10.1 Parity", text)

    def test_active_architecture_docs_record_the_immutable_canonical_vm_boundary(self):
        architecture = (REPO_ROOT / "docs" / "architecture.md").read_text(
            encoding="utf-8"
        )
        design = (REPO_ROOT / "design.md").read_text(encoding="utf-8")
        node_readme = (REPO_ROOT / "neo-node" / "README.md").read_text(
            encoding="utf-8"
        )

        revision = "3081e83db3716fd51dc58c0afc039290d2d07253"
        for document, text in (("architecture", architecture), ("design", design)):
            with self.subTest(document=document):
                self.assertIn(revision, text)
                self.assertIn("sole canonical", text)
                self.assertIn("non-canonical", text)

        self.assertNotIn("external sibling crate", architecture)
        self.assertNotIn("referenced by path", architecture)
        self.assertIn("44 ADRs", architecture)
        self.assertIn("44 ADRs", node_readme)
        self.assertIn("8 ordered dependency layers", node_readme)

    def test_adr_044_records_authority_semantics_and_evidence_limits(self):
        text = (REPO_ROOT / "design.md").read_text(encoding="utf-8")
        headings = re.findall(r"^### ADR-(\d{3}):", text, re.MULTILINE)
        prose = normalized(text)

        self.assertEqual(len(headings), 44)
        self.assertEqual(
            sorted(headings),
            [f"{number:03d}" for number in range(1, 45)],
        )
        self.assertIn("### ADR-044: Immutable VM boundary and canonical local execution", text)
        for marker in (
            "Reth and Polkadot/Substrate are architecture references only",
            "repeated compound IDs",
            "conflicting kind, shape, or content fails closed",
            "Before `HF_Domovoi`",
            "fresh immutable deep copy",
            "`RUSTSEC-2025-0141`",
            "Phase 2 plan 02-03",
            "complete MainNet replay/state parity",
            "authenticated checkpoint fast sync",
        ):
            with self.subTest(marker=marker):
                self.assertIn(marker, prose)

    def test_fast_sync_docs_do_not_claim_authenticated_checkpoint_sync(self):
        text = (REPO_ROOT / "docs" / "operations.md").read_text(encoding="utf-8")
        prose = normalized(text)

        for marker in (
            "Download, MD5-check, cache, and import",
            "accelerated full-history archive replay",
            "not authenticated checkpoint fast sync",
            "HTTPS protects transport and MD5 detects accidental corruption",
            "neither supplies an explicit checkpoint trust policy or authenticity proof",
            "replay/performance evidence gate",
            "not as a complete production release gate",
        ):
            with self.subTest(marker=marker):
                self.assertIn(marker, prose)

    def test_active_regression_test_names_use_current_neo_n3_target(self):
        stale_markers = []
        search_roots = [
            REPO_ROOT / "neo-crypto" / "src" / "tests",
            REPO_ROOT / "neo-payloads" / "src" / "tests",
            REPO_ROOT / "scripts" / "tests",
        ]

        for root in search_roots:
            for path in root.rglob("*"):
                if path.suffix not in {".py", ".rs"}:
                    continue
                for line_number, line in enumerate(
                    path.read_text(encoding="utf-8").splitlines(), 1
                ):
                    stripped = line.strip()
                    if (
                        (stripped.startswith("fn ") or stripped.startswith("def test_"))
                        and "v3100" in stripped
                    ):
                        stale_markers.append(
                            f"{path.relative_to(REPO_ROOT)}:{line_number}: {stripped}"
                        )

        self.assertEqual(
            [],
            stale_markers,
            "active regression test names should use v3101 for the current Neo N3 target",
        )

    def test_active_rust_sources_do_not_claim_v391_reference_target(self):
        stale_markers = []
        for root in REPO_ROOT.glob("neo-*/src"):
            for path in root.rglob("*.rs"):
                for line_number, line in enumerate(
                    path.read_text(encoding="utf-8").splitlines(), 1
                ):
                    if "v3.9.1" in line:
                        stale_markers.append(
                            f"{path.relative_to(REPO_ROOT)}:{line_number}: {line.strip()}"
                        )

        self.assertEqual(
            [],
            stale_markers,
            "active Rust sources must name Neo v3.10.1, not v3.9.1, when "
            "describing the current C# parity target",
        )

    def test_rpc_relay_height_preclassification_comment_names_current_reference(self):
        text = (
            REPO_ROOT / "neo-rpc" / "src" / "server" / "rpc_relay" / "block.rs"
        ).read_text(encoding="utf-8")

        self.assertIn("height pre-classification (v3.10.1)", text)
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
        self.assertIn("Neo v3.10.1 Consistency", text)
        self.assertIn("validate-v310-consistency.sh", text)
        # The artifact upload path must match the directory the validator writes
        # to (reports/compat-v310); a stale compat-v391 path silently drops them.
        self.assertIn("reports/compat-v310", text)
        self.assertNotIn("Neo v3.9.1 Consistency", text)
        self.assertNotIn("compat-v391", text)
        self.assertNotIn("v391-consistency-", text)


if __name__ == "__main__":
    unittest.main()

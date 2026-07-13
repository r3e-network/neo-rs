import os
import subprocess
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "restore-checkpoint.sh"


class RestoreCheckpointTests(unittest.TestCase):
    def test_restore_usage_lists_all_restore_options(self):
        result = subprocess.run(
            [str(SCRIPT)],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=False,
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("--chain-db", result.stdout)
        self.assertIn("--stateroot-db", result.stdout)
        self.assertIn("--keep-current", result.stdout)
        self.assertIn("--dry-run", result.stdout)
        self.assertIn("--yes", result.stdout)

    def test_restore_chain_only_checkpoint_removes_stale_stateroot(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            data_dir = root / "data"
            checkpoint_root = data_dir / "checkpoints"
            checkpoint = checkpoint_root / "h624433"
            source_chain = checkpoint / "mainnet"
            current_chain = data_dir / "mainnet"
            current_stateroot = data_dir / "Plugins" / "mainnet" / "StateRoot"

            source_chain.mkdir(parents=True)
            (source_chain / "CURRENT").write_text("MANIFEST-source\n", encoding="utf-8")
            (checkpoint / "CHECKPOINT_INFO").write_text(
                "\n".join(
                    [
                        "height=624433",
                        "stateroot_db=none",
                        "state_root_included=false",
                    ]
                )
                + "\n",
                encoding="utf-8",
            )

            current_chain.mkdir(parents=True)
            (current_chain / "CURRENT").write_text("MANIFEST-old\n", encoding="utf-8")
            current_stateroot.mkdir(parents=True)
            (current_stateroot / "CURRENT").write_text("MANIFEST-stale\n", encoding="utf-8")

            result = subprocess.run(
                [
                    str(SCRIPT),
                    "624433",
                    "--data-dir",
                    str(data_dir),
                    "--yes",
                ],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertEqual(
                (current_chain / "CURRENT").read_text(encoding="utf-8"),
                "MANIFEST-source\n",
            )
            self.assertFalse(
                current_stateroot.exists(),
                "chain-only restore must not leave a stale StateRoot beside the restored chain DB",
            )
            self.assertIn("state DB skipped", result.stdout)

    def test_restore_accepts_explicit_chain_and_stateroot_targets(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoint_root = root / "checkpoints"
            checkpoint = checkpoint_root / "h700000"
            source_chain = checkpoint / "mainnet"
            source_stateroot = checkpoint / "StateRoot"
            target_chain = root / "bounded-replay" / "data"
            target_stateroot = root / "bounded-replay" / "StateRoot"

            source_chain.mkdir(parents=True)
            source_stateroot.mkdir()
            (source_chain / "CURRENT").write_text("MANIFEST-chain-source\n", encoding="utf-8")
            (source_stateroot / "CURRENT").write_text(
                "MANIFEST-state-source\n", encoding="utf-8"
            )
            (checkpoint / "CHECKPOINT_INFO").write_text(
                "\n".join(
                    [
                        "height=700000",
                        "restore_verified=true",
                        "verified_height=700000",
                        "verified_stateroot_root=0xroot700000",
                        "verified_against_reference=true",
                    ]
                )
                + "\n",
                encoding="utf-8",
            )

            target_chain.mkdir(parents=True)
            target_stateroot.mkdir(parents=True)
            (target_chain / "CURRENT").write_text("MANIFEST-chain-old\n", encoding="utf-8")
            (target_stateroot / "CURRENT").write_text("MANIFEST-state-old\n", encoding="utf-8")

            result = subprocess.run(
                [
                    str(SCRIPT),
                    "700000",
                    "--root",
                    str(checkpoint_root),
                    "--chain-db",
                    str(target_chain),
                    "--stateroot-db",
                    str(target_stateroot),
                    "--allow-unverified",
                    "--yes",
                ],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertEqual(
                (target_chain / "CURRENT").read_text(encoding="utf-8"),
                "MANIFEST-chain-source\n",
            )
            self.assertEqual(
                (target_stateroot / "CURRENT").read_text(encoding="utf-8"),
                "MANIFEST-state-source\n",
            )

    def test_restore_mdbx_checkpoint_does_not_hardlink_checkpoint_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoint_root = root / "checkpoints"
            checkpoint = checkpoint_root / "h700000"
            source_chain = checkpoint / "mainnet"
            target_chain = root / "restore" / "mainnet"

            source_chain.mkdir(parents=True)
            (source_chain / "data.mdbx").write_text("chain-and-state", encoding="utf-8")
            (checkpoint / "CHECKPOINT_INFO").write_text(
                "\n".join(
                    [
                        "height=700000",
                        "storage_provider=mdbx",
                        "state_root_layout=coordinated_mdbx",
                        "state_root_included=true",
                        "restore_verified=true",
                        "verified_height=700000",
                        "verified_stateroot_root=0xroot700000",
                        "verified_against_reference=true",
                        "",
                    ]
                ),
                encoding="utf-8",
            )

            result = subprocess.run(
                [
                    str(SCRIPT),
                    "700000",
                    "--root",
                    str(checkpoint_root),
                    "--chain-db",
                    str(target_chain),
                    "--yes",
                ],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertNotEqual(
                (source_chain / "data.mdbx").stat().st_ino,
                (target_chain / "data.mdbx").stat().st_ino,
                "MDBX restore must not share checkpoint DB inodes",
            )
            self.assertIn("StateService table included", result.stdout)

    def test_restore_rejects_non_height_label_checkpoint(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoint_root = root / "checkpoints"
            checkpoint = checkpoint_root / "mainnet-bounded-700000-stable"
            source_chain = checkpoint / "data"
            target_chain = root / "bounded-replay" / "data"
            target_stateroot = root / "bounded-replay" / "StateRoot"

            source_chain.mkdir(parents=True)
            (source_chain / "CURRENT").write_text("MANIFEST-sample\n", encoding="utf-8")
            (checkpoint / "CHECKPOINT_INFO").write_text(
                "\n".join(
                    [
                        "label=mainnet-bounded-700000-stable",
                        "height=700000",
                        "mode=storage-sample",
                        "mpt_dir=missing-mpt",
                    ]
                )
                + "\n",
                encoding="utf-8",
            )
            target_stateroot.mkdir(parents=True)
            (target_stateroot / "CURRENT").write_text("MANIFEST-stale\n", encoding="utf-8")

            result = subprocess.run(
                [
                    str(SCRIPT),
                    "mainnet-bounded-700000-stable",
                    "--root",
                    str(checkpoint_root),
                    "--chain-db",
                    str(target_chain),
                    "--stateroot-db",
                    str(target_stateroot),
                    "--allow-unverified",
                    "--yes",
                ],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("no matching checkpoint found", result.stderr)
            self.assertFalse(target_chain.exists())
            self.assertTrue(target_stateroot.exists())

    def test_restore_latest_skips_newer_unverified_full_state_checkpoint_by_default(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoint_root = root / "checkpoints"
            verified = checkpoint_root / "h100"
            unverified = checkpoint_root / "h200"
            target_chain = root / "restore" / "data"
            target_stateroot = root / "restore" / "StateRoot"

            for checkpoint, chain_manifest, state_manifest, info in [
                (
                    verified,
                    "MANIFEST-verified-chain\n",
                    "MANIFEST-verified-state\n",
                    "\n".join(
                        [
                            "height=100",
                            "restore_verified=true",
                            "verified_height=100",
                            "verified_stateroot_root=0xroot100",
                            "verified_against_reference=true",
                            "",
                        ]
                    ),
                ),
                (
                    unverified,
                    "MANIFEST-unverified-chain\n",
                    "MANIFEST-unverified-state\n",
                    "height=200\n",
                ),
            ]:
                (checkpoint / "mainnet").mkdir(parents=True)
                (checkpoint / "StateRoot").mkdir()
                (checkpoint / "mainnet" / "CURRENT").write_text(
                    chain_manifest, encoding="utf-8"
                )
                (checkpoint / "StateRoot" / "CURRENT").write_text(
                    state_manifest, encoding="utf-8"
                )
                (checkpoint / "CHECKPOINT_INFO").write_text(info, encoding="utf-8")

            result = subprocess.run(
                [
                    str(SCRIPT),
                    "latest",
                    "--root",
                    str(checkpoint_root),
                    "--chain-db",
                    str(target_chain),
                    "--stateroot-db",
                    str(target_stateroot),
                    "--yes",
                ],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertEqual(
                (target_chain / "CURRENT").read_text(encoding="utf-8"),
                "MANIFEST-verified-chain\n",
            )
            self.assertEqual(
                (target_stateroot / "CURRENT").read_text(encoding="utf-8"),
                "MANIFEST-verified-state\n",
            )
            self.assertIn("h100", result.stdout)

    def test_restore_at_or_below_skips_newer_chain_only_checkpoint_by_default(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoint_root = root / "checkpoints"
            verified = checkpoint_root / "h100"
            chain_only = checkpoint_root / "h150"
            target_chain = root / "restore" / "data"
            target_stateroot = root / "restore" / "StateRoot"

            (verified / "mainnet").mkdir(parents=True)
            (verified / "StateRoot").mkdir()
            (verified / "mainnet" / "CURRENT").write_text(
                "MANIFEST-verified-chain\n", encoding="utf-8"
            )
            (verified / "StateRoot" / "CURRENT").write_text(
                "MANIFEST-verified-state\n", encoding="utf-8"
            )
            (verified / "CHECKPOINT_INFO").write_text(
                "\n".join(
                    [
                        "height=100",
                        "restore_verified=true",
                        "verified_height=100",
                        "verified_stateroot_root=0xroot100",
                        "verified_against_reference=true",
                        "",
                    ]
                ),
                encoding="utf-8",
            )

            (chain_only / "mainnet").mkdir(parents=True)
            (chain_only / "mainnet" / "CURRENT").write_text(
                "MANIFEST-chain-only\n", encoding="utf-8"
            )
            (chain_only / "CHECKPOINT_INFO").write_text(
                "height=150\nstate_root_included=false\n",
                encoding="utf-8",
            )

            result = subprocess.run(
                [
                    str(SCRIPT),
                    "--at-or-below",
                    "150",
                    "--root",
                    str(checkpoint_root),
                    "--chain-db",
                    str(target_chain),
                    "--stateroot-db",
                    str(target_stateroot),
                    "--yes",
                ],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertEqual(
                (target_chain / "CURRENT").read_text(encoding="utf-8"),
                "MANIFEST-verified-chain\n",
            )
            self.assertTrue((target_stateroot / "CURRENT").exists())
            self.assertIn("h100", result.stdout)

    def test_restore_latest_allow_unverified_preserves_highest_candidate_behavior(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoint_root = root / "checkpoints"
            verified = checkpoint_root / "h100"
            unverified = checkpoint_root / "h200"
            target_chain = root / "restore" / "data"
            target_stateroot = root / "restore" / "StateRoot"

            for checkpoint, chain_manifest, state_manifest, info in [
                (
                    verified,
                    "MANIFEST-verified-chain\n",
                    "MANIFEST-verified-state\n",
                    "\n".join(
                        [
                            "height=100",
                            "restore_verified=true",
                            "verified_height=100",
                            "verified_stateroot_root=0xroot100",
                            "verified_against_reference=true",
                            "",
                        ]
                    ),
                ),
                (
                    unverified,
                    "MANIFEST-unverified-chain\n",
                    "MANIFEST-unverified-state\n",
                    "height=200\n",
                ),
            ]:
                (checkpoint / "mainnet").mkdir(parents=True)
                (checkpoint / "StateRoot").mkdir()
                (checkpoint / "mainnet" / "CURRENT").write_text(
                    chain_manifest, encoding="utf-8"
                )
                (checkpoint / "StateRoot" / "CURRENT").write_text(
                    state_manifest, encoding="utf-8"
                )
                (checkpoint / "CHECKPOINT_INFO").write_text(info, encoding="utf-8")

            result = subprocess.run(
                [
                    str(SCRIPT),
                    "latest",
                    "--root",
                    str(checkpoint_root),
                    "--chain-db",
                    str(target_chain),
                    "--stateroot-db",
                    str(target_stateroot),
                    "--allow-unverified",
                    "--yes",
                ],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertEqual(
                (target_chain / "CURRENT").read_text(encoding="utf-8"),
                "MANIFEST-unverified-chain\n",
            )
            self.assertEqual(
                (target_stateroot / "CURRENT").read_text(encoding="utf-8"),
                "MANIFEST-unverified-state\n",
            )
            self.assertIn("h200", result.stdout)

    def test_restore_missing_target_lists_height_checkpoints_only(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoint_root = root / "checkpoints"
            standard = checkpoint_root / "h624433" / "mainnet"
            named = checkpoint_root / "mainnet-bounded-700000-stable" / "data"

            standard.mkdir(parents=True)
            named.mkdir(parents=True)
            (named.parent / "CHECKPOINT_INFO").write_text(
                "height=700000\nmode=storage-sample\nmpt_dir=missing-mpt\n",
                encoding="utf-8",
            )

            result = subprocess.run(
                [
                    str(SCRIPT),
                    "999999",
                    "--root",
                    str(checkpoint_root),
                    "--dry-run",
                ],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("h624433", result.stderr)
            self.assertNotIn("h700000", result.stderr)
            self.assertNotIn("mainnet-bounded-700000-stable", result.stderr)

    def test_restore_explicit_targets_ignore_unrelated_neo_node_process_name(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            fake_bin = root / "bin"
            checkpoint_root = root / "checkpoints"
            checkpoint = checkpoint_root / "h700000"
            source_chain = checkpoint / "mainnet"
            source_stateroot = checkpoint / "StateRoot"
            target_chain = root / "bounded-replay" / "data"
            target_stateroot = root / "bounded-replay" / "StateRoot"

            fake_bin.mkdir()
            pgrep = fake_bin / "pgrep"
            pgrep.write_text("#!/usr/bin/env bash\necho 12345\n", encoding="utf-8")
            pgrep.chmod(0o755)
            ps = fake_bin / "ps"
            ps.write_text("#!/usr/bin/env bash\nexit 0\n", encoding="utf-8")
            ps.chmod(0o755)

            source_chain.mkdir(parents=True)
            source_stateroot.mkdir()
            (source_chain / "CURRENT").write_text("MANIFEST-chain-source\n", encoding="utf-8")
            (source_stateroot / "CURRENT").write_text(
                "MANIFEST-state-source\n", encoding="utf-8"
            )
            (checkpoint / "CHECKPOINT_INFO").write_text(
                "\n".join(
                    [
                        "height=700000",
                        "restore_verified=true",
                        "verified_height=700000",
                        "verified_stateroot_root=0xroot700000",
                        "verified_against_reference=true",
                    ]
                )
                + "\n",
                encoding="utf-8",
            )

            env = os.environ.copy()
            env["PATH"] = f"{fake_bin}{os.pathsep}{env.get('PATH', '')}"

            result = subprocess.run(
                [
                    str(SCRIPT),
                    "700000",
                    "--root",
                    str(checkpoint_root),
                    "--chain-db",
                    str(target_chain),
                    "--stateroot-db",
                    str(target_stateroot),
                    "--yes",
                ],
                cwd=REPO_ROOT,
                env=env,
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertEqual(
                (target_chain / "CURRENT").read_text(encoding="utf-8"),
                "MANIFEST-chain-source\n",
            )

    def test_restore_rejects_checkpoint_missing_stateroot_without_chain_only_marker(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            data_dir = root / "data"
            checkpoint = data_dir / "checkpoints" / "h42"
            source_chain = checkpoint / "mainnet"

            source_chain.mkdir(parents=True)
            (source_chain / "CURRENT").write_text("MANIFEST-source\n", encoding="utf-8")

            result = subprocess.run(
                [
                    str(SCRIPT),
                    "42",
                    "--data-dir",
                    str(data_dir),
                    "--yes",
                ],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("missing StateRoot", result.stderr)

    def test_restore_rejects_unverified_checkpoint_by_default(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            data_dir = root / "data"
            checkpoint = data_dir / "checkpoints" / "h42"
            source_chain = checkpoint / "mainnet"
            source_state = checkpoint / "StateRoot"

            source_chain.mkdir(parents=True)
            source_state.mkdir()
            (source_chain / "CURRENT").write_text("MANIFEST-source\n", encoding="utf-8")
            (source_state / "CURRENT").write_text("MANIFEST-state\n", encoding="utf-8")
            (checkpoint / "CHECKPOINT_INFO").write_text(
                "height=42\n",
                encoding="utf-8",
            )

            result = subprocess.run(
                [
                    str(SCRIPT),
                    "42",
                    "--data-dir",
                    str(data_dir),
                    "--yes",
                ],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("refusing to restore unverified checkpoint", result.stderr)
            self.assertIn("restore verification", result.stderr)

    def test_restore_allows_explicit_unverified_override(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            data_dir = root / "data"
            checkpoint = data_dir / "checkpoints" / "h42"
            source_chain = checkpoint / "mainnet"
            source_state = checkpoint / "StateRoot"
            target_chain = data_dir / "mainnet"
            target_state = data_dir / "Plugins" / "mainnet" / "StateRoot"

            source_chain.mkdir(parents=True)
            source_state.mkdir()
            (source_chain / "CURRENT").write_text("MANIFEST-source\n", encoding="utf-8")
            (source_state / "CURRENT").write_text("MANIFEST-state\n", encoding="utf-8")
            (checkpoint / "CHECKPOINT_INFO").write_text(
                "height=42\n",
                encoding="utf-8",
            )

            result = subprocess.run(
                [
                    str(SCRIPT),
                    "42",
                    "--data-dir",
                    str(data_dir),
                    "--yes",
                    "--allow-unverified",
                ],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertTrue((target_chain / "CURRENT").exists())
            self.assertTrue((target_state / "CURRENT").exists())


if __name__ == "__main__":
    unittest.main()

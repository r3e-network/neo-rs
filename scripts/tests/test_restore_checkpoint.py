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

    def test_restore_accepts_legacy_storage_sample_checkpoint_label(self):
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
                "MANIFEST-sample\n",
            )
            self.assertFalse(target_stateroot.exists())
            self.assertIn("h700000", result.stdout)

    def test_restore_latest_considers_named_checkpoints_with_metadata_height(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoint_root = root / "checkpoints"
            lower = checkpoint_root / "h624433" / "mainnet"
            higher = checkpoint_root / "mainnet-bounded-700000-stable" / "data"
            target_chain = root / "restore" / "data"
            target_stateroot = root / "restore" / "StateRoot"

            lower.mkdir(parents=True)
            (lower / "CURRENT").write_text("MANIFEST-low\n", encoding="utf-8")
            higher.mkdir(parents=True)
            (higher / "CURRENT").write_text("MANIFEST-high\n", encoding="utf-8")
            (higher.parent / "CHECKPOINT_INFO").write_text(
                "height=700000\nmode=storage-sample\nmpt_dir=missing-mpt\n",
                encoding="utf-8",
            )

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
                "MANIFEST-high\n",
            )
            self.assertIn("h700000", result.stdout)

    def test_restore_at_or_below_considers_named_checkpoints_with_metadata_height(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoint_root = root / "checkpoints"
            low = checkpoint_root / "h624433" / "mainnet"
            mid = checkpoint_root / "mainnet-bounded-690000-stable" / "data"
            high = checkpoint_root / "mainnet-bounded-700000-stable" / "data"
            target_chain = root / "restore" / "data"
            target_stateroot = root / "restore" / "StateRoot"

            for path, manifest in [
                (low, "MANIFEST-low\n"),
                (mid, "MANIFEST-mid\n"),
                (high, "MANIFEST-high\n"),
            ]:
                path.mkdir(parents=True)
                (path / "CURRENT").write_text(manifest, encoding="utf-8")
            (mid.parent / "CHECKPOINT_INFO").write_text(
                "height=690000\nmode=storage-sample\nmpt_dir=missing-mpt\n",
                encoding="utf-8",
            )
            (high.parent / "CHECKPOINT_INFO").write_text(
                "height=700000\nmode=storage-sample\nmpt_dir=missing-mpt\n",
                encoding="utf-8",
            )

            result = subprocess.run(
                [
                    str(SCRIPT),
                    "--at-or-below",
                    "695000",
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
                "MANIFEST-mid\n",
            )
            self.assertIn("h690000", result.stdout)

    def test_restore_missing_target_lists_named_checkpoints(self):
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
            self.assertIn("h700000", result.stderr)
            self.assertIn("mainnet-bounded-700000-stable", result.stderr)

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


if __name__ == "__main__":
    unittest.main()

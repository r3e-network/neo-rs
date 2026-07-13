import subprocess
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "checkpoint-on-height.sh"


class CheckpointOnHeightTests(unittest.TestCase):
    def run_checkpoint(
        self,
        *,
        height,
        chain_db,
        checkpoint_root,
        extra_args=None,
    ):
        command = [
            str(SCRIPT),
            "none",
            "--once",
            "--height",
            str(height),
            "--chain-db",
            str(chain_db),
            "--root",
            str(checkpoint_root),
        ]
        if extra_args:
            command.extend(extra_args)
        return subprocess.run(
            command,
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=False,
        )

    def test_once_checkpoint_captures_coordinated_mdbx_environment(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            chain_db = root / "chain-db"
            checkpoint_root = root / "checkpoints"
            chain_db.mkdir()
            (chain_db / "mdbx.dat").write_text("chain-and-state", encoding="utf-8")

            result = self.run_checkpoint(
                height=42,
                chain_db=chain_db,
                checkpoint_root=checkpoint_root,
            )

            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertTrue((checkpoint_root / "h42" / "mainnet" / "mdbx.dat").exists())
            self.assertFalse((checkpoint_root / "h42" / "StateRoot").exists())
            info = (checkpoint_root / "h42" / "CHECKPOINT_INFO").read_text(
                encoding="utf-8"
            )
            self.assertIn(f"chain_db={chain_db}", info)
            self.assertIn(f"stateroot_db={chain_db}:neo_state_service", info)
            self.assertIn("state_root_layout=coordinated_mdbx", info)

    def test_mdbx_checkpoint_does_not_hardlink_live_database_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            chain_db = root / "chain-mdbx"
            checkpoint_root = root / "checkpoints"
            chain_db.mkdir()
            (chain_db / "mdbx.dat").write_text("chain-and-state", encoding="utf-8")

            result = self.run_checkpoint(
                height=42,
                chain_db=chain_db,
                checkpoint_root=checkpoint_root,
            )

            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertNotEqual(
                (chain_db / "mdbx.dat").stat().st_ino,
                (checkpoint_root / "h42" / "mainnet" / "mdbx.dat").stat().st_ino,
                "MDBX checkpoint must not share live DB inodes",
            )
            self.assertFalse((checkpoint_root / "h42" / "StateRoot").exists())
            info = (checkpoint_root / "h42" / "CHECKPOINT_INFO").read_text(
                encoding="utf-8"
            )
            self.assertIn("state_root_layout=coordinated_mdbx", info)
            self.assertIn("state_root_included=true", info)

    def test_retention_prunes_oldest_by_checkpoint_height_not_path_text(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            chain_db = root / "chain-db"
            checkpoint_root = root / "checkpoints"
            chain_db.mkdir()
            (chain_db / "mdbx.dat").write_text("chain-and-state", encoding="utf-8")

            for height in (20, 5000, 10000, 20000):
                result = self.run_checkpoint(
                    height=height,
                    chain_db=chain_db,
                    checkpoint_root=checkpoint_root,
                    extra_args=["--max", "3"],
                )
                self.assertEqual(result.returncode, 0, result.stderr + result.stdout)

            retained = sorted(
                int(path.name[1:])
                for path in checkpoint_root.iterdir()
                if path.is_dir() and path.name.startswith("h")
            )
            self.assertEqual(retained, [5000, 10000, 20000])

    def test_retention_keeps_three_verified_full_state_checkpoints_before_unverified_dirs(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            chain_db = root / "chain-db"
            checkpoint_root = root / "checkpoints"
            chain_db.mkdir()
            (chain_db / "mdbx.dat").write_text("chain-and-state", encoding="utf-8")

            for height in (10, 20, 30):
                checkpoint = checkpoint_root / f"h{height}"
                (checkpoint / "mainnet").mkdir(parents=True)
                (checkpoint / "CHECKPOINT_INFO").write_text(
                    "\n".join(
                        [
                            f"height={height}",
                            "storage_provider=mdbx",
                            "state_root_layout=coordinated_mdbx",
                            "state_root_included=true",
                            "restore_verified=true",
                            f"verified_height={height}",
                            f"verified_stateroot_root=0xroot{height}",
                            "verified_against_reference=true",
                            "",
                        ]
                    ),
                    encoding="utf-8",
                )

            for height in (40, 50):
                checkpoint = checkpoint_root / f"h{height}"
                (checkpoint / "mainnet").mkdir(parents=True)
                (checkpoint / "CHECKPOINT_INFO").write_text(
                    f"height={height}\n",
                    encoding="utf-8",
                )

            result = self.run_checkpoint(
                height=60,
                chain_db=chain_db,
                checkpoint_root=checkpoint_root,
                extra_args=[
                    "--max",
                    "3",
                    "--restore-verified",
                    "--verified-height",
                    "60",
                    "--verified-stateroot-root",
                    "0xroot60",
                    "--verified-against-reference",
                ],
            )
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)

            retained = sorted(
                int(path.name[1:])
                for path in checkpoint_root.iterdir()
                if path.is_dir() and path.name.startswith("h")
            )

            self.assertEqual(
                retained,
                [20, 30, 60],
                "unverified checkpoint directories should be pruned before verified full-state checkpoints",
            )
            self.assertFalse((checkpoint_root / "h40").exists())
            self.assertFalse((checkpoint_root / "h50").exists())

    def test_retention_rejects_max_below_three(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            chain_db = root / "chain-db"
            checkpoint_root = root / "checkpoints"
            chain_db.mkdir()
            (chain_db / "mdbx.dat").write_text("chain-and-state", encoding="utf-8")

            result = self.run_checkpoint(
                height=42,
                chain_db=chain_db,
                checkpoint_root=checkpoint_root,
                extra_args=["--max", "2"],
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("--max must be >= 3", result.stderr)

    def test_once_checkpoint_can_capture_chain_only_replay_db(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            chain_db = root / "bounded-replay-db"
            checkpoint_root = root / "checkpoints"
            chain_db.mkdir()
            (chain_db / "mdbx.dat").write_text("chain-only", encoding="utf-8")

            result = subprocess.run(
                [
                    str(SCRIPT),
                    "none",
                    "--once",
                    "--height",
                    "624433",
                    "--chain-db",
                    str(chain_db),
                    "--root",
                    str(checkpoint_root),
                    "--chain-only",
                ],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertTrue((checkpoint_root / "h624433" / "mainnet" / "mdbx.dat").exists())
            self.assertFalse((checkpoint_root / "h624433" / "StateRoot").exists())
            info = (checkpoint_root / "h624433" / "CHECKPOINT_INFO").read_text(
                encoding="utf-8"
            )
            self.assertIn(f"chain_db={chain_db}", info)
            self.assertIn("stateroot_db=none", info)
            self.assertIn("state_root_included=false", info)

    def test_once_checkpoint_records_explicit_restore_verification_metadata(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            chain_db = root / "chain-db"
            checkpoint_root = root / "checkpoints"
            chain_db.mkdir()
            (chain_db / "mdbx.dat").write_text("chain-and-state", encoding="utf-8")

            result = self.run_checkpoint(
                height=42,
                chain_db=chain_db,
                checkpoint_root=checkpoint_root,
                extra_args=[
                    "--restore-verified",
                    "--verified-height",
                    "42",
                    "--verified-stateroot-root",
                    "0xabc123",
                    "--verified-against-reference",
                ],
            )

            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            info = (checkpoint_root / "h42" / "CHECKPOINT_INFO").read_text(
                encoding="utf-8"
            )
            self.assertIn("restore_verified=true", info)
            self.assertIn("verified_height=42", info)
            self.assertIn("verified_stateroot_root=0xabc123", info)
            self.assertIn("verified_against_reference=true", info)

    def test_existing_verified_checkpoint_must_match_requested_root(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            chain_db = root / "chain-db"
            checkpoint_root = root / "checkpoints"
            chain_db.mkdir()
            (chain_db / "mdbx.dat").write_text("chain-and-state", encoding="utf-8")

            first = self.run_checkpoint(
                height=42,
                chain_db=chain_db,
                checkpoint_root=checkpoint_root,
                extra_args=[
                    "--restore-verified",
                    "--verified-height",
                    "42",
                    "--verified-stateroot-root",
                    "0xold",
                    "--verified-against-reference",
                ],
            )
            self.assertEqual(first.returncode, 0, first.stderr + first.stdout)

            second = self.run_checkpoint(
                height=42,
                chain_db=chain_db,
                checkpoint_root=checkpoint_root,
                extra_args=[
                    "--restore-verified",
                    "--verified-height",
                    "42",
                    "--verified-stateroot-root",
                    "0xnew",
                    "--verified-against-reference",
                ],
            )

            self.assertNotEqual(second.returncode, 0)
            self.assertIn("verified_stateroot_root", second.stderr)

    def test_verified_stateroot_root_requires_restore_verification(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            chain_db = root / "chain-db"
            checkpoint_root = root / "checkpoints"
            chain_db.mkdir()

            result = self.run_checkpoint(
                height=42,
                chain_db=chain_db,
                checkpoint_root=checkpoint_root,
                extra_args=["--verified-stateroot-root", "0xabc123"],
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("--verified-stateroot-root requires --restore-verified", result.stderr)

    def test_restore_verification_requires_verified_stateroot_root(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            chain_db = root / "chain-db"
            checkpoint_root = root / "checkpoints"
            chain_db.mkdir()

            result = self.run_checkpoint(
                height=42,
                chain_db=chain_db,
                checkpoint_root=checkpoint_root,
                extra_args=[
                    "--restore-verified",
                    "--verified-height",
                    "42",
                ],
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(
                "--restore-verified requires --verified-stateroot-root",
                result.stderr,
            )
            self.assertFalse((checkpoint_root / "h42").exists())

    def test_restore_verification_height_must_match_checkpoint_height(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            chain_db = root / "chain-db"
            checkpoint_root = root / "checkpoints"
            chain_db.mkdir()

            result = self.run_checkpoint(
                height=42,
                chain_db=chain_db,
                checkpoint_root=checkpoint_root,
                extra_args=[
                    "--restore-verified",
                    "--verified-height",
                    "41",
                    "--verified-stateroot-root",
                    "0xabc123",
                    "--verified-against-reference",
                ],
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("--verified-height must match --height", result.stderr)


if __name__ == "__main__":
    unittest.main()

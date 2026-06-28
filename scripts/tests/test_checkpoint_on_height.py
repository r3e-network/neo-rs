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
        stateroot_db,
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
            "--stateroot-db",
            str(stateroot_db),
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

    def test_once_checkpoint_accepts_explicit_chain_and_stateroot_dirs(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            chain_db = root / "chain-db"
            stateroot_db = root / "state-root-db"
            checkpoint_root = root / "checkpoints"
            chain_db.mkdir()
            stateroot_db.mkdir()
            (chain_db / "CURRENT").write_text("MANIFEST-000001\n", encoding="utf-8")
            (stateroot_db / "CURRENT").write_text("MANIFEST-000002\n", encoding="utf-8")

            result = self.run_checkpoint(
                height=42,
                chain_db=chain_db,
                stateroot_db=stateroot_db,
                checkpoint_root=checkpoint_root,
            )

            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertTrue((checkpoint_root / "h42" / "mainnet" / "CURRENT").exists())
            self.assertTrue((checkpoint_root / "h42" / "StateRoot" / "CURRENT").exists())
            info = (checkpoint_root / "h42" / "CHECKPOINT_INFO").read_text(
                encoding="utf-8"
            )
            self.assertIn(f"chain_db={chain_db}", info)
            self.assertIn(f"stateroot_db={stateroot_db}", info)

    def test_retention_prunes_oldest_by_checkpoint_height_not_path_text(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            chain_db = root / "chain-db"
            stateroot_db = root / "state-root-db"
            checkpoint_root = root / "checkpoints"
            chain_db.mkdir()
            stateroot_db.mkdir()
            (chain_db / "CURRENT").write_text("MANIFEST-000001\n", encoding="utf-8")
            (stateroot_db / "CURRENT").write_text("MANIFEST-000002\n", encoding="utf-8")

            for height in (20, 5000, 10000, 20000):
                result = self.run_checkpoint(
                    height=height,
                    chain_db=chain_db,
                    stateroot_db=stateroot_db,
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

    def test_once_checkpoint_can_capture_chain_only_replay_db(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            chain_db = root / "bounded-replay-db"
            checkpoint_root = root / "checkpoints"
            chain_db.mkdir()
            (chain_db / "CURRENT").write_text("MANIFEST-000001\n", encoding="utf-8")

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
            self.assertTrue((checkpoint_root / "h624433" / "mainnet" / "CURRENT").exists())
            self.assertFalse((checkpoint_root / "h624433" / "StateRoot").exists())
            info = (checkpoint_root / "h624433" / "CHECKPOINT_INFO").read_text(
                encoding="utf-8"
            )
            self.assertIn(f"chain_db={chain_db}", info)
            self.assertIn("stateroot_db=none", info)
            self.assertIn("state_root_included=false", info)


if __name__ == "__main__":
    unittest.main()

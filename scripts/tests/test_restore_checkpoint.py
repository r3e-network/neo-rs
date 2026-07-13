import os
import subprocess
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "restore-checkpoint.sh"


def write_checkpoint(
    root: Path,
    *,
    height: int,
    payload: str,
    state_root_included: bool = True,
    restore_verified: bool = True,
) -> Path:
    checkpoint = root / f"h{height}"
    source = checkpoint / "mainnet"
    source.mkdir(parents=True)
    (source / "mdbx.dat").write_text(payload, encoding="utf-8")
    lines = [
        f"height={height}",
        "storage_provider=mdbx",
        "state_root_layout=coordinated_mdbx",
        f"state_root_included={str(state_root_included).lower()}",
    ]
    if state_root_included and restore_verified:
        lines.extend(
            [
                "restore_verified=true",
                f"verified_height={height}",
                f"verified_stateroot_root=0xroot{height}",
                "verified_against_reference=true",
            ]
        )
    (checkpoint / "CHECKPOINT_INFO").write_text(
        "\n".join(lines) + "\n",
        encoding="utf-8",
    )
    return checkpoint


def run_restore(*args: str, env: dict[str, str] | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(
        [str(SCRIPT), *args],
        cwd=REPO_ROOT,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )


class RestoreCheckpointTests(unittest.TestCase):
    def test_restore_usage_lists_all_restore_options(self):
        result = run_restore()

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("--chain-db", result.stdout)
        self.assertNotIn("--stateroot-db", result.stdout)
        self.assertIn("--keep-current", result.stdout)
        self.assertIn("--dry-run", result.stdout)
        self.assertIn("--yes", result.stdout)

    def test_restore_chain_only_checkpoint_replaces_target_environment(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            data_dir = root / "data"
            checkpoint_root = data_dir / "checkpoints"
            target = data_dir / "mainnet"
            write_checkpoint(
                checkpoint_root,
                height=624433,
                payload="chain-only",
                state_root_included=False,
            )
            target.mkdir(parents=True)
            (target / "mdbx.dat").write_text("stale", encoding="utf-8")

            result = run_restore(
                "624433",
                "--data-dir",
                str(data_dir),
                "--yes",
            )

            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertEqual((target / "mdbx.dat").read_text(encoding="utf-8"), "chain-only")
            self.assertIn("StateService table not included", result.stdout)

    def test_restore_accepts_explicit_mdbx_target(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoint_root = root / "checkpoints"
            target = root / "bounded-replay" / "data"
            write_checkpoint(checkpoint_root, height=700000, payload="chain-and-state")
            target.mkdir(parents=True)
            (target / "mdbx.dat").write_text("old", encoding="utf-8")

            result = run_restore(
                "700000",
                "--root",
                str(checkpoint_root),
                "--chain-db",
                str(target),
                "--yes",
            )

            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertEqual(
                (target / "mdbx.dat").read_text(encoding="utf-8"),
                "chain-and-state",
            )

    def test_restore_mdbx_checkpoint_does_not_hardlink_checkpoint_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoint_root = root / "checkpoints"
            target = root / "restore" / "mainnet"
            checkpoint = write_checkpoint(
                checkpoint_root,
                height=700000,
                payload="chain-and-state",
            )
            source = checkpoint / "mainnet" / "mdbx.dat"

            result = run_restore(
                "700000",
                "--root",
                str(checkpoint_root),
                "--chain-db",
                str(target),
                "--yes",
            )

            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertNotEqual(source.stat().st_ino, (target / "mdbx.dat").stat().st_ino)
            self.assertIn("StateService table included", result.stdout)

    def test_restore_rejects_non_height_label_checkpoint(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoint_root = root / "checkpoints"
            sample = checkpoint_root / "mainnet-bounded-700000-stable"
            (sample / "data").mkdir(parents=True)
            (sample / "CHECKPOINT_INFO").write_text(
                "height=700000\nmode=storage-sample\n",
                encoding="utf-8",
            )
            target = root / "bounded-replay" / "data"

            result = run_restore(
                "mainnet-bounded-700000-stable",
                "--root",
                str(checkpoint_root),
                "--chain-db",
                str(target),
                "--allow-unverified",
                "--yes",
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("no matching checkpoint found", result.stderr)
            self.assertFalse(target.exists())

    def test_restore_latest_skips_newer_unverified_checkpoint_by_default(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoint_root = root / "checkpoints"
            target = root / "restore" / "data"
            write_checkpoint(checkpoint_root, height=100, payload="verified")
            write_checkpoint(
                checkpoint_root,
                height=200,
                payload="unverified",
                restore_verified=False,
            )

            result = run_restore(
                "latest",
                "--root",
                str(checkpoint_root),
                "--chain-db",
                str(target),
                "--yes",
            )

            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertEqual((target / "mdbx.dat").read_text(encoding="utf-8"), "verified")
            self.assertIn("h100", result.stdout)

    def test_restore_at_or_below_skips_newer_chain_only_checkpoint_by_default(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoint_root = root / "checkpoints"
            target = root / "restore" / "data"
            write_checkpoint(checkpoint_root, height=100, payload="verified")
            write_checkpoint(
                checkpoint_root,
                height=150,
                payload="chain-only",
                state_root_included=False,
            )

            result = run_restore(
                "--at-or-below",
                "150",
                "--root",
                str(checkpoint_root),
                "--chain-db",
                str(target),
                "--yes",
            )

            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertEqual((target / "mdbx.dat").read_text(encoding="utf-8"), "verified")
            self.assertIn("h100", result.stdout)

    def test_restore_latest_allow_unverified_uses_highest_candidate(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoint_root = root / "checkpoints"
            target = root / "restore" / "data"
            write_checkpoint(checkpoint_root, height=100, payload="verified")
            write_checkpoint(
                checkpoint_root,
                height=200,
                payload="unverified",
                restore_verified=False,
            )

            result = run_restore(
                "latest",
                "--root",
                str(checkpoint_root),
                "--chain-db",
                str(target),
                "--allow-unverified",
                "--yes",
            )

            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertEqual((target / "mdbx.dat").read_text(encoding="utf-8"), "unverified")
            self.assertIn("h200", result.stdout)

    def test_restore_missing_target_lists_height_checkpoints_only(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoint_root = root / "checkpoints"
            write_checkpoint(
                checkpoint_root,
                height=624433,
                payload="chain-only",
                state_root_included=False,
            )
            named = checkpoint_root / "mainnet-bounded-700000-stable"
            named.mkdir(parents=True)

            result = run_restore(
                "999999",
                "--root",
                str(checkpoint_root),
                "--dry-run",
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("h624433", result.stderr)
            self.assertNotIn("h700000", result.stderr)
            self.assertNotIn("mainnet-bounded-700000-stable", result.stderr)

    def test_restore_explicit_target_ignores_unrelated_process_names(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            fake_bin = root / "bin"
            checkpoint_root = root / "checkpoints"
            target = root / "bounded-replay" / "data"
            write_checkpoint(checkpoint_root, height=700000, payload="chain-and-state")

            fake_bin.mkdir()
            pgrep = fake_bin / "pgrep"
            pgrep.write_text("#!/usr/bin/env bash\necho 12345\n", encoding="utf-8")
            pgrep.chmod(0o755)
            env = os.environ.copy()
            env["PATH"] = f"{fake_bin}{os.pathsep}{env.get('PATH', '')}"

            result = run_restore(
                "700000",
                "--root",
                str(checkpoint_root),
                "--chain-db",
                str(target),
                "--yes",
                env=env,
            )

            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertEqual(
                (target / "mdbx.dat").read_text(encoding="utf-8"),
                "chain-and-state",
            )

    def test_restore_rejects_an_open_mdbx_lock_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            fake_bin = root / "bin"
            checkpoint_root = root / "checkpoints"
            target = root / "mainnet"
            write_checkpoint(checkpoint_root, height=700000, payload="chain-and-state")
            target.mkdir(parents=True)
            (target / "mdbx.lck").write_bytes(b"")

            fake_bin.mkdir()
            fuser = fake_bin / "fuser"
            fuser.write_text("#!/usr/bin/env bash\nexit 0\n", encoding="utf-8")
            fuser.chmod(0o755)
            env = os.environ.copy()
            env["PATH"] = f"{fake_bin}{os.pathsep}{env.get('PATH', '')}"

            result = run_restore(
                "700000",
                "--root",
                str(checkpoint_root),
                "--chain-db",
                str(target),
                "--yes",
                env=env,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("mdbx.lck is held", result.stderr)
            self.assertTrue((target / "mdbx.lck").exists())

    def test_restore_rejects_checkpoint_without_coordinated_mdbx_metadata(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            data_dir = root / "data"
            checkpoint = data_dir / "checkpoints" / "h42"
            source = checkpoint / "mainnet"
            source.mkdir(parents=True)
            (source / "mdbx.dat").write_text("source", encoding="utf-8")
            (checkpoint / "CHECKPOINT_INFO").write_text("height=42\n", encoding="utf-8")

            result = run_restore("42", "--data-dir", str(data_dir), "--yes")

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("not a coordinated MDBX checkpoint", result.stderr)

    def test_restore_rejects_unverified_checkpoint_by_default(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            data_dir = root / "data"
            write_checkpoint(
                data_dir / "checkpoints",
                height=42,
                payload="source",
                restore_verified=False,
            )

            result = run_restore("42", "--data-dir", str(data_dir), "--yes")

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("refusing to restore unverified checkpoint", result.stderr)
            self.assertIn("restore verification", result.stderr)

    def test_restore_allows_explicit_unverified_override(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            data_dir = root / "data"
            target = data_dir / "mainnet"
            write_checkpoint(
                data_dir / "checkpoints",
                height=42,
                payload="source",
                restore_verified=False,
            )

            result = run_restore(
                "42",
                "--data-dir",
                str(data_dir),
                "--yes",
                "--allow-unverified",
            )

            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertEqual((target / "mdbx.dat").read_text(encoding="utf-8"), "source")


if __name__ == "__main__":
    unittest.main()

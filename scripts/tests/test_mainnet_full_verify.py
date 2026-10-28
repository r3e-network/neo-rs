import contextlib
import importlib.util
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock


MODULE_PATH = Path(__file__).resolve().parents[1] / "mainnet-full-verify.py"
SPEC = importlib.util.spec_from_file_location("mainnet_full_verify", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise ImportError(f"unable to load module from {MODULE_PATH}")
verifier = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(verifier)

ROOT_A = "0x" + "ab" * 32
ROOT_B = "0x" + "cd" * 32
LOCAL = "http://127.0.0.1:10332"
REFERENCE = "http://seed1.neo.org:10332"


class FakeClient:
    def __init__(self, url, timeout=30.0):
        self.url = url
        self.timeout = timeout
        self.closed = False

    def close(self):
        self.closed = True


class RootClient:
    def __init__(self, result):
        self.url = "http://fake"
        self.result = result

    def call(self, method, params, retries):
        return self.result


class StrictRootTests(unittest.TestCase):
    def test_normalize_root_accepts_only_canonical_shapes(self):
        self.assertEqual(verifier.normalize_root("AB" * 32), ROOT_A)
        self.assertEqual(verifier.normalize_root("0X" + "AB" * 32), ROOT_A)
        self.assertEqual(verifier.normalize_root(ROOT_A), ROOT_A)

    def test_normalize_root_rejects_non_strings_and_malformed_roots(self):
        invalid = [
            None,
            True,
            1,
            b"ab" * 32,
            "",
            "0x",
            "0x" + "ab" * 31,
            "ab" * 32 + "a",
            "0x" + "gg" * 32,
            "root",
        ]
        for value in invalid:
            with self.subTest(value=value), self.assertRaises(ValueError):
                verifier.normalize_root(value)

    def test_fetch_stateroot_rejects_invalid_root(self):
        client = RootClient({"index": 7, "roothash": "not-a-root"})
        with self.assertRaisesRegex(RuntimeError, "invalid root"):
            verifier.fetch_stateroot(client, 7, retries=1)

    def test_fetch_stateroot_requires_exact_integer_index(self):
        invalid_indexes = [None, True, 7.0, "7", 8]
        for index in invalid_indexes:
            with self.subTest(index=index):
                client = RootClient({"index": index, "rootHash": ROOT_A})
                with self.assertRaisesRegex(RuntimeError, "index"):
                    verifier.fetch_stateroot(client, 7, retries=1)

        client = RootClient({"index": 7, "rootHash": ROOT_A})
        row = verifier.fetch_stateroot(client, 7, retries=1)
        self.assertEqual(row["height"], 7)
        self.assertEqual(row["roothash"], ROOT_A)


class CheckpointTests(unittest.TestCase):
    def make_checkpoint(self, path, local=LOCAL, reference=REFERENCE):
        checkpoint = verifier.Checkpoint(path, local, reference)
        checkpoint.add(3, ROOT_A)
        return checkpoint

    def test_persists_local_and_restores_only_identical_binding(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "checkpoint.json"
            self.make_checkpoint(path)
            payload = json.loads(path.read_text(encoding="utf-8"))
            self.assertEqual(payload["local"], LOCAL)
            self.assertEqual(payload["reference"], REFERENCE)

            restored = verifier.Checkpoint(path, LOCAL, REFERENCE)
            self.assertEqual(restored.heights, {3: ROOT_A})

            for kwargs in [
                {"local": "http://other:10332", "reference": REFERENCE},
                {"local": LOCAL, "reference": "http://other-reference"},
            ]:
                with self.subTest(**kwargs), contextlib.redirect_stderr(io.StringIO()):
                    isolated = verifier.Checkpoint(path, **kwargs)
                self.assertEqual(isolated.heights, {})

    def test_legacy_and_malformed_checkpoints_are_never_partially_trusted(self):
        valid_payload = {
            "local": LOCAL,
            "reference": REFERENCE,
            "heights": {"3": ROOT_A},
        }
        malformed_payloads = [
            {"reference": REFERENCE, "heights": {"3": ROOT_A}},
            [],
            {**valid_payload, "heights": []},
            {**valid_payload, "heights": {True: ROOT_A}},
            {**valid_payload, "heights": {"03": ROOT_A}},
            {**valid_payload, "heights": {"3": ROOT_A, "4": "bad"}},
            {**valid_payload, "heights": {"3": None}},
        ]
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "checkpoint.json"
            for payload in malformed_payloads:
                with self.subTest(payload=payload):
                    path.write_text(json.dumps(payload), encoding="utf-8")
                    stderr = io.StringIO()
                    with contextlib.redirect_stderr(stderr):
                        checkpoint = verifier.Checkpoint(path, LOCAL, REFERENCE)
                    self.assertEqual(checkpoint.heights, {})
                    self.assertIn("ignoring invalid checkpoint", stderr.getvalue())


class RunTests(unittest.TestCase):
    def make_args(self, tmp, *extra):
        parser = verifier.build_parser()
        return parser.parse_args(
            [
                "--local",
                LOCAL,
                "--reference",
                REFERENCE,
                "--start",
                "0",
                "--end",
                "2",
                "--parallel",
                "1",
                "--report",
                str(Path(tmp) / "report.json"),
                *extra,
            ]
        )

    def invoke(self, args):
        stdout = io.StringIO()
        stderr = io.StringIO()
        with mock.patch.object(verifier, "PooledRpcClient", FakeClient):
            with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                rc = verifier.run(args)
        report = json.loads(Path(args.report).read_text(encoding="utf-8"))
        return rc, report, stdout.getvalue(), stderr.getvalue()

    def test_fresh_first_missing_reference_fails_despite_later_matches(self):
        with tempfile.TemporaryDirectory() as tmp:
            args = self.make_args(tmp)

            def verify(height, *unused):
                if height == args.start:
                    return {
                        "height": height,
                        "status": "missing_reference",
                        "error": "reference unavailable",
                    }
                return {
                    "height": height,
                    "status": "match",
                    "local": ROOT_A,
                    "reference": ROOT_A,
                    "reference_source": REFERENCE,
                }

            with mock.patch.object(verifier, "_verify_height", side_effect=verify):
                rc, report, _, _ = self.invoke(args)

            self.assertEqual(rc, 1)
            self.assertFalse(report["passed"])
            self.assertEqual(report["matched"], 2)
            self.assertEqual(report["sample_count"], 3)
            self.assertIsNone(report["claimable_contiguous"])

    def test_resume_passes_only_when_every_sample_matched(self):
        for stride, end in [(1, 2), (2, 4)]:
            with self.subTest(stride=stride), tempfile.TemporaryDirectory() as tmp:
                checkpoint_path = Path(tmp) / "checkpoint.json"
                args = self.make_args(
                    tmp,
                    "--end",
                    str(end),
                    "--stride",
                    str(stride),
                    "--checkpoint",
                    str(checkpoint_path),
                )
                checkpoint = verifier.Checkpoint(
                    checkpoint_path, verifier.canonical_rpc_url(LOCAL), REFERENCE
                )
                for height in range(0, end + 1, stride):
                    checkpoint.add(height, ROOT_A)

                with mock.patch.object(verifier, "_verify_height") as verify:
                    rc, report, _, _ = self.invoke(args)
                verify.assert_not_called()
                self.assertEqual(rc, 0)
                self.assertTrue(report["passed"])
                self.assertEqual(report["matched"], report["sample_count"])
                self.assertEqual(
                    report["resumed_from_checkpoint"], report["sample_count"]
                )

    def test_resume_with_missing_sample_fails_in_contiguous_and_sampled_modes(self):
        for stride, end in [(1, 2), (2, 4)]:
            with self.subTest(stride=stride), tempfile.TemporaryDirectory() as tmp:
                checkpoint_path = Path(tmp) / "checkpoint.json"
                args = self.make_args(
                    tmp,
                    "--end",
                    str(end),
                    "--stride",
                    str(stride),
                    "--checkpoint",
                    str(checkpoint_path),
                )
                heights = list(range(0, end + 1, stride))
                checkpoint = verifier.Checkpoint(
                    checkpoint_path, verifier.canonical_rpc_url(LOCAL), REFERENCE
                )
                for height in heights[1:]:
                    checkpoint.add(height, ROOT_A)

                def verify(height, *unused):
                    return {
                        "height": height,
                        "status": "missing_reference",
                        "error": "reference unavailable",
                    }

                with mock.patch.object(verifier, "_verify_height", side_effect=verify):
                    rc, report, _, _ = self.invoke(args)
                self.assertEqual(rc, 1)
                self.assertFalse(report["passed"])
                self.assertEqual(report["matched"], len(heights) - 1)
                self.assertEqual(report["sample_count"], len(heights))

    def test_forced_height_is_refetched_with_complete_checkpoint(self):
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint_path = Path(tmp) / "checkpoint.json"
            args = self.make_args(
                tmp,
                "--checkpoint",
                str(checkpoint_path),
                "--force-mismatch-at",
                "1",
            )
            checkpoint = verifier.Checkpoint(
                checkpoint_path, verifier.canonical_rpc_url(LOCAL), REFERENCE
            )
            for height in range(3):
                checkpoint.add(height, ROOT_A)

            row = {
                "height": 1,
                "status": "mismatch",
                "local": verifier.SENTINEL_ROOT,
                "reference": ROOT_A,
                "reference_source": REFERENCE,
                "injected": True,
            }
            with mock.patch.object(
                verifier, "_verify_height", return_value=row
            ) as verify:
                rc, report, _, _ = self.invoke(args)

            verify.assert_called_once()
            self.assertEqual(verify.call_args.args[0], 1)
            self.assertEqual(verify.call_args.args[4], 1)
            self.assertEqual(rc, 1)
            self.assertFalse(report["passed"])
            self.assertEqual(report["resumed_from_checkpoint"], 2)
            self.assertEqual(report["matched"], 2)
            self.assertEqual(len(report["mismatches"]), 1)
            self.assertEqual(report["mismatches"][0]["height"], 1)
            self.assertTrue(report["mismatches"][0]["injected"])

    def test_force_mismatch_negative_control_still_fails(self):
        local = RootClient({"index": 7, "roothash": ROOT_A})
        reference = RootClient({"index": 7, "roothash": ROOT_A})
        reference.url = REFERENCE
        row = verifier._verify_height(
            7, local, [reference], retries=1, force_mismatch_at=7
        )
        self.assertEqual(row["status"], "mismatch")
        self.assertEqual(row["local"], verifier.SENTINEL_ROOT)
        self.assertEqual(row["reference"], ROOT_A)
        self.assertEqual(row["reference_source"], REFERENCE)
        self.assertTrue(row["injected"])

    def test_mismatch_report_includes_reference_source(self):
        with tempfile.TemporaryDirectory() as tmp:
            args = self.make_args(tmp, "--end", "0")
            row = {
                "height": 0,
                "status": "mismatch",
                "local": verifier.SENTINEL_ROOT,
                "reference": ROOT_A,
                "reference_source": REFERENCE,
                "injected": True,
            }
            with mock.patch.object(verifier, "_verify_height", return_value=row):
                rc, report, _, stderr = self.invoke(args)
            self.assertEqual(rc, 1)
            self.assertEqual(
                report["mismatches"][0]["reference_source"], REFERENCE
            )
            self.assertIn(f"reference_source={REFERENCE}", stderr)
            self.assertTrue(report["mismatches"][0]["injected"])


if __name__ == "__main__":
    unittest.main()

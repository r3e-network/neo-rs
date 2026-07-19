import os
import subprocess
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "validate_stored_roots.sh"


class ValidateStoredRootsTests(unittest.TestCase):
    ROOT = "0x" + "ab" * 32

    def run_validator(self, curl_body: str, *, local_exit: int = 0, curl_exit: int = 0):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            local = root / "local-root"
            local.write_text(
                f"#!/usr/bin/env bash\necho \"height=$1 root={self.ROOT}\"\nexit {local_exit}\n",
                encoding="utf-8",
            )
            local.chmod(0o755)
            curl = root / "curl"
            curl.write_text(
                "#!/usr/bin/env bash\nprintf '%s\\n' '"
                + curl_body
                + f"'\nexit {curl_exit}\n",
                encoding="utf-8",
            )
            curl.chmod(0o755)
            env = {
                **os.environ,
                "LOCAL_ROOT_BIN": str(local),
                "CURL_BIN": str(curl),
                "CSHARP_RPC": "http://example.invalid",
            }
            return subprocess.run(
                [str(SCRIPT), "10", "12", "1"],
                env=env,
                text=True,
                capture_output=True,
                check=False,
            )

    def test_complete_range_passes(self):
        result = self.run_validator(
            '{"jsonrpc":"2.0","id":1,"result":{"roothash":"'
            + self.ROOT
            + '"}}'
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("PASS: all 3 requested roots matched", result.stdout)

    def test_reference_failure_fails_closed(self):
        result = self.run_validator('{"jsonrpc":"2.0","id":1,"error":{}}')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("query failures: 3", result.stdout)
        self.assertIn("not completely verified", result.stderr)

    def test_malformed_reference_root_fails_closed(self):
        result = self.run_validator(
            '{"jsonrpc":"2.0","id":1,"result":{"roothash":"0xabc"}}'
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("query failures: 3", result.stdout)

    def test_local_probe_nonzero_status_cannot_pass_with_valid_output(self):
        result = self.run_validator(
            '{"jsonrpc":"2.0","id":1,"result":{"roothash":"'
            + self.ROOT
            + '"}}',
            local_exit=1,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("query failures: 3", result.stdout)

    def test_curl_nonzero_status_cannot_pass_with_valid_output(self):
        result = self.run_validator(
            '{"jsonrpc":"2.0","id":1,"result":{"roothash":"'
            + self.ROOT
            + '"}}',
            curl_exit=1,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("query failures: 3", result.stdout)

    def test_non_decimal_range_is_rejected_before_arithmetic(self):
        with tempfile.TemporaryDirectory() as tmp:
            probe = Path(tmp) / "probe"
            probe.write_text("#!/usr/bin/env bash\nexit 0\n", encoding="utf-8")
            probe.chmod(0o755)
            result = subprocess.run(
                [str(SCRIPT), "1+1", "12", "1"],
                env={**os.environ, "LOCAL_ROOT_BIN": str(probe)},
                text=True,
                capture_output=True,
                check=False,
            )
        self.assertEqual(result.returncode, 2)
        self.assertIn("decimal integers", result.stderr)


if __name__ == "__main__":
    unittest.main()

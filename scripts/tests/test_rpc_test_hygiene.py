import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
RPC_TESTS_DIR = REPO_ROOT / "neo-rpc" / "tests"
SERVER_CFG = '#![cfg(feature = "server")]'


class RpcTestHygieneTests(unittest.TestCase):
    def test_server_integration_tests_keep_crate_docs_before_cfg_gate(self):
        offenders = []
        for path in sorted(RPC_TESTS_DIR.glob("*.rs")):
            lines = path.read_text(encoding="utf-8").splitlines()
            if SERVER_CFG not in lines:
                continue
            cfg_index = lines.index(SERVER_CFG)
            doc_index = next(
                (index for index, line in enumerate(lines) if line.startswith("//!")),
                None,
            )
            if doc_index is None or doc_index > cfg_index:
                offenders.append(path.relative_to(REPO_ROOT).as_posix())

        self.assertEqual(
            offenders,
            [],
            "server-gated neo-rpc integration tests should put crate docs before "
            "#![cfg(feature = \"server\")] so -W missing-docs stays quiet",
        )


if __name__ == "__main__":
    unittest.main()

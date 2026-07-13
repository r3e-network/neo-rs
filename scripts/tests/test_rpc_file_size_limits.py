import unittest

from scripts.tests.file_size_policy import REPO_ROOT, assert_line_budget, rust_source_files


class RpcFileSizeLimitTests(unittest.TestCase):
    def test_rpc_sources_obey_the_repository_size_policy(self):
        assert_line_budget(
            self,
            rust_source_files(REPO_ROOT / "neo-rpc"),
            {},
            minimum_files=50,
        )


if __name__ == "__main__":
    unittest.main()

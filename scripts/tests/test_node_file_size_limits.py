import unittest

from scripts.tests.file_size_policy import (
    REPO_ROOT,
    RUST_SIZE_BASELINE,
    assert_line_budget,
    baseline_for_paths,
    rust_source_files,
)


class NodeFileSizeLimitTests(unittest.TestCase):
    def test_node_sources_obey_the_repository_size_policy(self):
        paths = rust_source_files(REPO_ROOT / "neo-node")
        assert_line_budget(
            self,
            paths,
            baseline_for_paths(paths, RUST_SIZE_BASELINE),
            minimum_files=50,
        )


if __name__ == "__main__":
    unittest.main()

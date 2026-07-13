import unittest

from scripts.tests.file_size_policy import (
    RUST_SIZE_BASELINE,
    assert_line_budget,
    rust_source_files,
)


class FileSizeLimitTests(unittest.TestCase):
    def test_rust_sources_obey_the_review_budget_or_exact_debt_ratchet(self):
        assert_line_budget(
            self,
            rust_source_files(),
            RUST_SIZE_BASELINE,
            minimum_files=500,
        )


if __name__ == "__main__":
    unittest.main()

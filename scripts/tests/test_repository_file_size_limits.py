import unittest

from scripts.tests.file_size_policy import (
    OPERATIONAL_PYTHON_SIZE_BASELINE,
    PYTHON_TEST_SIZE_BASELINE,
    REPO_ROOT,
    assert_line_budget,
)


class RepositoryFileSizeLimitTests(unittest.TestCase):
    def test_operational_python_obeys_the_review_budget_or_exact_debt_ratchet(self):
        assert_line_budget(
            self,
            sorted((REPO_ROOT / "scripts").glob("*.py")),
            OPERATIONAL_PYTHON_SIZE_BASELINE,
            minimum_files=10,
        )

    def test_python_tests_obey_the_review_budget_or_exact_debt_ratchet(self):
        assert_line_budget(
            self,
            sorted((REPO_ROOT / "scripts" / "tests").glob("test_*.py")),
            PYTHON_TEST_SIZE_BASELINE,
            minimum_files=5,
        )


if __name__ == "__main__":
    unittest.main()

import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]


class RepositoryFileSizeLimitTests(unittest.TestCase):
    def test_operational_python_scripts_keep_review_headroom(self):
        paths = sorted((REPO_ROOT / "scripts").glob("*.py"))

        self.assertGreater(len(paths), 10, "expected operational Python scripts")
        for path in paths:
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    900,
                    f"{path.relative_to(REPO_ROOT)} should be split before it nears the hard 1000-line limit",
                )

    def test_python_test_files_keep_review_headroom(self):
        paths = sorted((REPO_ROOT / "scripts" / "tests").glob("test_*.py"))

        self.assertGreater(len(paths), 5, "expected repository script tests")
        for path in paths:
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    900,
                    f"{path.relative_to(REPO_ROOT)} should keep review headroom below the hard 1000-line limit",
                )

    def test_python_test_files_stay_below_1000_lines(self):
        paths = sorted((REPO_ROOT / "scripts" / "tests").glob("test_*.py"))

        self.assertGreater(len(paths), 5, "expected repository script tests")
        for path in paths:
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    1000,
                    f"{path.relative_to(REPO_ROOT)} should be split before it exceeds 1000 lines",
                )


if __name__ == "__main__":
    unittest.main()

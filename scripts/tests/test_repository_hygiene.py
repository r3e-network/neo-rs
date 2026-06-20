import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]


class RepositoryHygieneTests(unittest.TestCase):
    def test_gitignore_excludes_local_test_and_runtime_artifacts(self):
        gitignore = (REPO_ROOT / ".gitignore").read_text(encoding="utf-8")
        required_patterns = [
            "__pycache__/",
            "*.py[cod]",
            "logs/",
            "Logs/",
            "*.log",
        ]

        for pattern in required_patterns:
            with self.subTest(pattern=pattern):
                self.assertIn(
                    pattern,
                    gitignore,
                    f".gitignore should exclude local artifact pattern {pattern}",
                )


if __name__ == "__main__":
    unittest.main()

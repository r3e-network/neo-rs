import unittest

from scripts.tests.file_size_policy import (
    RUST_SIZE_BASELINE,
    oversized_files,
    rust_source_files,
)


class FileSizeCatalogLimitTests(unittest.TestCase):
    def test_rust_debt_catalog_is_complete_and_exact(self):
        self.assertEqual(oversized_files(rust_source_files()), RUST_SIZE_BASELINE)


if __name__ == "__main__":
    unittest.main()

#!/usr/bin/env python3
"""Regression tests for indexed ACC extraction."""

import base64
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("extract-acc-range.py")


class IndexedAccExtractionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(dir=Path(__file__).parents[1] / "data")
        self.root = Path(self.temporary.name)
        self.acc = self.root / "test.acc"
        self.blocks = [b"genesis", b"block-one", b"block-two", b"block-three"]
        with self.acc.open("wb") as stream:
            stream.write(struct.pack("<II", 0, len(self.blocks)))
            for block in self.blocks:
                stream.write(struct.pack("<I", len(block)))
                stream.write(block)

    def tearDown(self):
        self.temporary.cleanup()

    def extract(self, start, end, expected_success=True):
        output = self.root / f"out-{start}-{end}"
        result = subprocess.run(
            [sys.executable, str(SCRIPT), str(self.acc), str(output), str(start), str(end)],
            capture_output=True,
            text=True,
            check=False,
        )
        if expected_success:
            self.assertEqual(result.returncode, 0, result.stderr)
            actual = [
                base64.b64decode((output / f"{height}.b64").read_bytes())
                for height in range(start, end + 1)
            ]
            self.assertEqual(actual, self.blocks[start : end + 1])
        else:
            self.assertNotEqual(result.returncode, 0)
        return result

    def test_indexed_ranges_preserve_exact_block_bytes_and_reuse_index(self):
        self.extract(1, 2)
        index = Path(str(self.acc) + ".offsets")
        self.assertTrue(index.is_file())
        modified = index.stat().st_mtime_ns
        self.extract(0, 3)
        self.extract(3, 3)
        self.assertEqual(index.stat().st_mtime_ns, modified)

    def test_changed_acc_is_rejected_instead_of_using_stale_offsets(self):
        self.extract(0, 1)
        with self.acc.open("ab") as stream:
            stream.write(b"changed")
        result = self.extract(2, 3, expected_success=False)
        self.assertIn("stale or invalid ACC offset index", result.stderr)

    def test_truncated_acc_cannot_create_an_index(self):
        self.acc.write_bytes(self.acc.read_bytes()[:-1])
        result = self.extract(0, 3, expected_success=False)
        self.assertIn("truncated", result.stderr)
        self.assertFalse(Path(str(self.acc) + ".offsets").exists())


if __name__ == "__main__":
    unittest.main()

#!/usr/bin/env python3
"""Backward-compatible wrapper for the v3.10.0 mainnet checkpoint verifier."""

from pathlib import Path
import runpy


if __name__ == "__main__":
    target = Path(__file__).with_name("check-v310-mainnet-checkpoints.py")
    runpy.run_path(str(target), run_name="__main__")

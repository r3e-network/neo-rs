#!/usr/bin/env python3
"""Validate and summarize repeated replay crash-recovery measurements."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import statistics


def summary(values: list[float]) -> dict[str, float]:
    return {
        "median": statistics.median(values),
        "min": min(values),
        "max": max(values),
        "stdev": statistics.stdev(values) if len(values) > 1 else 0.0,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("results", type=Path, nargs="+")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    documents = [json.loads(path.read_text(encoding="utf-8")) for path in args.results]
    if len({document["binary_sha256"] for document in documents}) != 1:
        raise ValueError("recovery results must use one binary")
    groups: dict[str, list[dict]] = {}
    for document in documents:
        if document["observed_durable_height"] != document["durable_height"]:
            raise ValueError("durable height mismatch")
        for key in ("resume", "tail_control"):
            run = document[key]
            if run["validation_errors"] or not run["state_root_verification"]:
                raise ValueError(f"invalid {key} state-root evidence")
        if document["prefill"]["return_code"] != 70:
            raise ValueError("prefill did not terminate at the intentional crash point")
        groups.setdefault(str(document["commit_interval"]), []).append(document)
    output = {
        "kind": "crash-recovery-summary",
        "binary_sha256": documents[0]["binary_sha256"],
        "sources": [str(path) for path in args.results],
        "groups": {},
    }
    for interval, selected in sorted(groups.items(), key=lambda item: int(item[0])):
        output["groups"][interval] = {
            "runs": len(selected),
            "replayed_uncommitted_blocks": selected[0]["replayed_uncommitted_blocks"],
            "tail_blocks": selected[0]["tail_blocks"],
            "resume_wall_seconds": summary(
                [document["resume"]["wall_seconds"] for document in selected]
            ),
            "tail_wall_seconds": summary(
                [document["tail_control"]["wall_seconds"] for document in selected]
            ),
            "recovery_overhead_seconds": summary(
                [document["recovery_overhead_seconds"] for document in selected]
            ),
            "all_recovery_roots_verified": True,
        }
    args.output.write_text(json.dumps(output, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(output, indent=2))
    print(f"result={args.output}")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Compile independent Coq models in isolation and retain complete evidence."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile


def run(command, cwd, log):
    try:
        result = subprocess.run(command, cwd=cwd, stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT, text=True, timeout=120)
        log.write_text(result.stdout, encoding="utf-8")
        return result.returncode
    except subprocess.TimeoutExpired:
        log.write_text("TIMEOUT after 120 seconds\n", encoding="utf-8")
        return 124


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--all", action="store_true", help="Include unverified legacy models")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    version = subprocess.run(["coqc", "--version"], check=True,
                             capture_output=True, text=True).stdout
    sources = sorted((root / "formal/coqlib").glob("*.v")) if args.all else [
        root / "formal/coqlib/rw_set_minimal.v"]
    results = []
    for source in sources:
        text = source.read_text(encoding="utf-8")
        # Inventory aid only; compilation and kernel checks are separate gates.
        stripped = re.sub(r"\(\*.*?\*\)", "", text, flags=re.S)
        admissions = len(re.findall(r"\b(?:Admitted|admit|Axiom|Axioms|Parameter|Parameters)\b", stripped))
        with tempfile.TemporaryDirectory(prefix="neo-coq-") as temp:
            workspace = Path(temp)
            (workspace / "coqlib").mkdir()
            # Copy source dependencies only: never reuse stale .vo artifacts.
            for dependency in (root / "formal/coqlib").glob("*.v"):
                shutil.copy2(dependency, workspace / "coqlib" / dependency.name)
            command = ["coqc", "-Q", "coqlib", "CoqLib", "coqlib/" + source.name]
            compiled = run(command, workspace, output / (source.stem + ".compile.log"))
            checked = None
            if compiled == 0:
                checked = run(["coqchk", "-silent", "-Q", "coqlib", "CoqLib",
                               "CoqLib." + source.stem], workspace,
                              output / (source.stem + ".kernel.log"))
        row = {"source": str(source.relative_to(root)), "sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
               "compile_exit": compiled, "kernel_exit": checked,
               "admission_or_assumption_markers": admissions,
               "command": command}
        results.append(row)
        print(source.name, "compile", compiled, "kernel", checked, "assumption markers", admissions, flush=True)
    (output / "summary.json").write_text(json.dumps({"toolchain": version,
        "mode": "legacy-inventory" if args.all else "verified-subset", "results": results}, indent=2), encoding="utf-8")
    return 0 if all(r["compile_exit"] == 0 and r["kernel_exit"] == 0 and
                    r["admission_or_assumption_markers"] == 0 for r in results) else 1


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env bash
# Coq (Rocq) example-proof pipeline check.
# Installed via: winget install Coq.CoqPlatform  (Rocq/Coq 9.0.1)
COQC="${COQC:-/c/Rocq-Platform~9.0~2025.08/bin/coqc.exe}"
MODEL_DIR="$(cd "$(dirname "$0")/../../formal/coqlib" && pwd)"
cd "$MODEL_DIR" || exit 2
echo "== coqc version =="
"$COQC" --version | head -1
echo "== compiling example_proof.v =="
rm -f example_proof.vo example_proof.vok example_proof.glob example_proof.vos
"$COQC" example_proof.v
echo "exit=$?"
ls -la example_proof.vo example_proof.vok 2>/dev/null
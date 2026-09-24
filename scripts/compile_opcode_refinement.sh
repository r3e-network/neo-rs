#!/bin/bash
# Compile opcode_price_refinement.v and produce .vo artifact
# Requires: Coq 8.19.2+, OCaml 4.14+

set -e

echo "=== Compiling opcode_price_refinement.v ==="
cd formal/coqlib

if ! command -v coqc &> /dev/null; then
    echo "❌ Coq not found in PATH. Please install Coq:"
    echo "   - Windows: Install Chocolatey (choco install coq)"
    echo "   - macOS: brew install coq"
    echo "   - Linux: sudo apt-get install coq"
    exit 1
fi

echo "Compiling with coqc..."
coqc opcode_price_refinement.v

if [ -f "opcode_price_refinement.vo" ]; then
    echo "✅ Successfully compiled!"
    ls -lh opcode_price_refinement.vo
    echo ""
    echo "The .vo file can be loaded in Coq proofs:"
    echo "  Require Import opcode_price_refinement."
else
    echo "❌ Compilation failed"
    exit 1
fi

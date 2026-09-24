#!/bin/bash
# Formal Verification Environment Setup Script
# Platform: Linux/macOS (WSL compatible)
# Target: Coq 8.19.2 + TLA+ Toolbox
# Estimated Time: 10-15 minutes

set -e  # Exit on any error

echo "============================================================"
echo "  Neo-N3 Formal Verification Environment Setup"
echo "============================================================"
echo ""
echo "This script will install:"
echo "  • OCaml 4.14.1 (compiler)"
echo "  • Coq 8.19.2 (proof assistant)"
echo "  • Essential Coq libraries (elpi, serapi, finmap)"
echo "  • TLA+ Toolbox prerequisites"
echo ""
read -p "Press Enter to continue..." || true

# ============================================================
# STEP 1: Check OS and Install Prerequisites                  
# ============================================================

echo ""
echo "=== Step 1: Checking Operating System ==="

if [[ "$OSTYPE" == "msys" ]]; then
    echo "⚠️  Detected Windows with MSYS2/Cygwin"
    echo "ℹ️  Recommendations:"
    echo "  • Use WSL2 for best compatibility"
    echo "  • Or use native PowerShell with Chocolatey packages"
    echo ""
    echo "Attempting installation anyway..."
elif [[ "$OSTYPE" == "darwin"* ]]; then
    echo "✅ Detected macOS"
    if ! command -v brew &> /dev/null; then
        echo "Installing Homebrew..."
        /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
    fi
elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
    echo "✅ Detected Linux"
    if ! command -v apt-get &> /dev/null; then
        echo "⚠️  Unsupported package manager (not apt)"
        exit 1
    fi
else
    echo "⚠️  Unknown OS: $OSTYPE"
    echo "Continuing anyway..."
fi

# ============================================================
# STEP 2: Install OPAM (OCaml Package Manager)                
# ============================================================

echo ""
echo "=== Step 2: Installing OPAM ==="

if command -v opam &> /dev/null; then
    echo "✅ OPAM already installed (version $(opam --version))"
else
    echo "Installing OPAM..."
    
    if [[ "$OSTYPE" == "darwin"* ]]; then
        brew install opam
    elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
        sudo apt-get update
        sudo apt-get install -y opam
    else
        curl -L https://github.com/ocaml/opam/releases/download/2.1.4/opam-2.1.4-x86_64-apple-darwin.tar.gz | tar xz
        mv opam /usr/local/bin/
    fi
    
    echo "Initializing OPAM..."
    opam init --yes --bare --disable-sandboxing
fi

# ============================================================
# STEP 3: Setup OCaml Environment                             
# ============================================================

echo ""
echo "=== Step 3: Configuring OCaml Environment ==="

eval $(opam env)

echo "Creating OCaml switch (4.14.1)..."
opam switch create neo-rs-formal 4.14.1 --yes

# Add to shell profile for persistence
if grep -q "opam env" ~/.bashrc 2>/dev/null; then
    echo "✓ OPAM already in .bashrc"
else
    echo "" >> ~/.bashrc
    echo "# OPAM environment for Neo-RS formal verification" >> ~/.bashrc
    echo "eval \$(opam env)" >> ~/.bashrc
    echo "✓ Added to .bashrc"
fi

# Reload shell
source ~/.bashrc

# ============================================================
# STEP 4: Install Coq and Libraries                           
# ============================================================

echo ""
echo "=== Step 4: Installing Coq 8.19.2 and Ecosystem ==="

COQ_VERSION="8.19.2"

echo "Installing Coq ${COQ_VERSION}..."
opam install coq==${COQ_VERSION} --yes

echo "Installing essential Coq libraries..."
opam install coq-elpi --yes         # Higher-order reasoning
opam install coq-serapi --yes       # Language server protocol  
opam install coq-finmap --yes       # Finite maps (for state models)
opam install coq-sat --yes          # SAT solver backend
opam install coq-fiat-crypto --yes  # Cryptographic primitives

echo "Verifying installation..."
coqc --version
opam list | grep "^coq " | head -5

# ============================================================
# STEP 5: Validate Installation                               
# ============================================================

echo ""
echo "=== Step 5: Validation ==="

# Test Coq compilation
cat > /tmp/test_coq.v << 'EOF'
Theorem test_addition : forall n m : nat, n + m = m + n.
Proof. intros. rewrite Nat.add_comm. reflexivity. Qed.
EOF

echo "Testing Coq compilation..."
coqc /tmp/test_coq.v 2>&1 | grep -q "Theorem" && echo "✅ Coq compiles proofs successfully"

# Clean up test file
rm /tmp/test_coq.v /tmp/test_coq.vo 2>/dev/null || true

# ============================================================
# STEP 6: Optional Tools                                      
# ============================================================

echo ""
echo "=== Step 6: Optional Tool Installation ==="

read -p "Install VSCode Coq plugin manifest? (y/n) " -n 1 -r
echo ""
if [[ $REPLY =~ ^[Yy]$ ]]; then
    echo "Downloading Coq plugin manifest..."
    mkdir -p ~/.vscode/extensions
    curl -L https://github.com/coq-community/docker-coq/releases/latest/download/vscocooq-plugin.txt > ~/.vscode/extensions/coq-community.vscoq.json
    echo "✓ Plugin manifest downloaded (restart VSCode to install)"
fi

read -p "Install Emacs Flymake-Coq support? (y/n) " -n 1 -r
echo ""
if [[ $REPLY =~ ^[Yy]$ ]]; then
    echo "Installing Flymake-Coq..."
    opam install flymake-coq --yes
    echo "✓ Installed (add to .emacs: (require 'flymake-coq))"
fi

# ============================================================
# STEP 7: TLA+ Setup (Optional)                               
# ============================================================

echo ""
echo "=== Step 7: TLA+ Model Checker Setup ==="

if command -v tlc &> /dev/null; then
    echo "✅ TLC already available in PATH"
else
    echo "ℹ️  TLA+ Toolbox is a Java GUI application"
    echo "Download from: https://www.tlapl us.org/Download/"
    echo ""
    echo "Alternatively, use Docker container:"
    echo "  docker pull hzp/tla-plus"
    echo ""
    read -p "Install TLA+ via Docker? (y/n) " -n 1 -r
    echo ""
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        if command -v docker &> /dev/null; then
            echo "Pulling TLA+ Docker image..."
            docker pull hzp/tla-plus
            echo "✓ Use: docker run --rm hzp/tla-plus tlc model.tla"
        else
            echo "Docker not installed. Skip TLA+ setup for now."
        fi
    fi
fi

# ============================================================
# FINAL SUMMARY                                               
# ============================================================

echo ""
echo "============================================================"
echo "  ✅ ENVIRONMENT SETUP COMPLETE!"
echo "============================================================"
echo ""
echo "Installed Components:"
echo "  ✓ OPAM package manager"
echo "  ✓ OCaml 4.14.1 compiler"
echo "  ✓ Coq 8.19.2 proof assistant"
echo "  ✓ Coq ecosystem libraries (elpi, serapi, finmap, sat)"
echo ""
echo "Next Steps:"
echo "  1. cd $PWD/formal"
echo "  2. make validate"
echo "  3. Review docs/formal-verification/QUICKSTART.md"
echo ""
echo "Troubleshooting:"
echo "  • If commands not found, run: eval \$(opam env)"
echo "  • For issues, visit: https://coq.zulipchat.com/"
echo ""
echo "Environment Details:"
echo "  • Coq version: $(coqc --version | head -1)"
echo "  • OCaml version: $(ocamlc --version)"
echo "  • OPAM switch: $(opam switch show)"
echo ""

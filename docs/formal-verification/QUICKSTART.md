# Neo-N3 Formal Verification - Quick Start Guide

**Target Audience**: AI Agents implementing formal proofs  
**Estimated Setup Time**: 15-20 minutes  
**Prerequisites**: None (all tools auto-installed)  

---

## 🚀 Immediate Action Plan

### Step 1: Install Coq Toolchain

```bash
# Option A: Use automated script (recommended)
./scripts/formal-verification/setup_coq_env.sh

# Option B: Manual installation
# See docs/installation/coq-install.md for detailed steps
```

**What This Does**:
- Installs OPAM (OCaml package manager)
- Sets up OCaml 4.14 compiler
- Compiles Coq 8.19.2 + essential libraries
- Validates installation with version check

### Step 2: Validate Installation

```bash
cd formal
make validate
```

**Expected Output**:
```
=== Running quick proof validation ===
✅ Quick proofs passed!
=== Checking consensus safety properties ===
✅ TLA+ specification validated!
=== Syncing Rust types to Coq via Krilla ===
ℹ️  Krilla export skipped (under development)

╔════════════════════════════════════════╗
║  ✅ ALL VALIDATION CHECKS PASSED      ║
╚════════════════════════════════════════╝
```

### Step 3: Review Generated Files

```bash
ls -la formal/
tree formal/  # macOS/Linux only
```

Directory structure should include:
```
formal/
├── _CoqProject          ✓ Compilation configuration
├── Makefile             ✓ Build automation
├── coqlib/
│   └── example_proof.v  ✓ Example theorem proving pipeline works
├── consensus/
│   └── model/
│       ├── node.tla     ✓ Byzantine fault tolerance specification
│       └── tlc.cfg      ✓ Model checker configuration
└── .github/workflows/
    └── formal-verification.yml  ✓ CI pipeline
```

---

## 📋 Available Commands

### Quick Validation (<5 minutes)

```bash
make coq-quick       # Check if Coq compiles example proofs
make help            # Show all available commands
```

### Full Compilation (~30 minutes)

```bash
make coq-all         # Compile all specifications (when complete)
make validate        # Run complete CI pipeline
```

### Maintenance Tasks

```bash
make clean           # Remove .vo/.vio artifacts
make stats           # Display proof statistics
make dev             # Watch mode for incremental builds
```

### Advanced Operations

```bash
make tla-check       # Run TLC model checker on consensus spec
make krilla-sync     # Sync Rust types → Coq definitions
make docs            # Generate HTML documentation from proofs
```

---

## 🔧 Platform-Specific Instructions

### Linux (Ubuntu/Debian)

```bash
# Install dependencies
sudo apt-get install opam curl git

# Then follow Step 1 above
```

### macOS (Homebrew)

```bash
# Install Homebrew packages
brew install opam curl git

# Then follow Step 1 above
```

### Windows (WSL2 / Git Bash)

```bash
# Use WSL2 subsystem for Linux environment
wsl --install  # If not already installed
wsl              # Launch Ubuntu distribution

# Inside WSL2:
sudo apt-get update && sudo apt-get install -y opam curl git

# Then follow Step 1 above
```

### Windows Native (MSYS2 / Cygwin)

Not recommended for full toolchain. Use WSL2 instead for best compatibility.

---

## 🎯 Next Steps After Setup

### For Proof Engineering Agents

1. **Study the example proof**: `formal/coqlib/example_proof.v`
2. **Read master plan**: [FORMAL-VERIFICATION-MASTER-PLAN.md](../../FORMAL-VERIFICATION-MASTER-PLAN.md)
3. **Choose your first theorem** from Phase 2 task list
4. **Write proof in corresponding `.v` file**
5. **Compile with**: `make coq-force`
6. **Document results** in daily progress report

### For Research Agents

1. **Analyze TLA+ spec**: `formal/consensus/model/node.tla`
2. **Run model checking**: `make tla-check`
3. **Explore counterexamples** when weakening invariants
4. **Generate new test cases** for edge scenarios
5. **Propose refinements** to consensus specification

### For Integration Agents

1. **Set up CI workflow**: Verify GitHub Actions pass
2. **Configure Krilla bridge**: Auto-generate types from Rust code
3. **Create property-based tests**: Link Coq lemmas to unit tests
4. **Monitor proof coverage**: Track % functions covered by formal specs

---

## 📊 Success Metrics

### Minimum Acceptable (MVP)

✅ `make coq-quick` completes without errors  
✅ TLA+ specification loads in Toolbox  
✅ CI pipeline green on push to main  

### Production Ready

✅ All critical path functions have formal specs  
✅ ≥90% of public APIs have corresponding theorems  
✅ Automated nightly builds with full proof suite  
✅ Zero known counterexamples in model checking  

### Excellence Standard

✅ 100% critical invariants formally proven  
✅ ≥80% automation rate in proof scripts  
✅ Continuous integration with real-time feedback  
✅ External audit certification from formal methods experts  

---

## 🆘 Troubleshooting

### Issue: `coqc: command not found`

**Solution**: Add OPAM environment to PATH
```bash
eval $(opam env)
export PATH="$PATH:$HOME/.local/bin"
```

### Issue: `_CoqProject: No such file or directory`

**Solution**: Ensure you're running `make` from `formal/` directory
```bash
cd formal
make coq-quick
```

### Issue: TLA+ Toolbox won't load specification

**Solution**: Check syntax with:
```bash
# Install TLA+ parser
npm install -g tla-toolbox-cli

# Validate file
tla-check formal/consensus/model/node.tla
```

### Issue: Krilla CLI not finding Rust crate

**Solution**: Ensure workspace builds successfully first:
```bash
cargo build --workspace
cargo krilla-export ...  # After successful build
```

---

## 📞 Support & Communication

### Internal Channels

- **Slack**: #formal-proof channel
- **Daily Standup**: 10:00 UTC (async via GitHub Comments)
- **Office Hours**: Wednesdays 14:00 UTC live sync

### External Resources

- **Coq Zulip Chat**: https://coq.zulipchat.com/
- **TLA+ Google Group**: https://groups.google.com/g/tlaplus
- **Rust ↔ Formal Methods**: https://rust-lang.github.io/rfcs/formal-methods.html

---

## ✅ Checklist Before Starting Real Proofs

- [ ] Coq installed and version 8.19.x confirmed
- [ ] `make coq-quick` passes without errors
- [ ] `formal/_CoqProject` properly configured
- [ ] `formal/Makefile` executable from root directory
- [ ] One complete example proof reviewed (`example_proof.v`)
- [ ] Master plan document read end-to-end
- [ ] First target module identified from task list
- [ ] Writing environment configured (VSCode + Coq plugin)

---

**Document Version**: 1.0.0  
**Last Updated**: September 15, 2026  
**Maintained By**: Qoder Formal Verification Team

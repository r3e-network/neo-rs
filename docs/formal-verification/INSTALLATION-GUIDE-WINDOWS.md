# Formal Verification - Manual Installation Guide (Windows)

**Date**: September 15, 2026  
**Purpose**: Install Coq 8.19.2 and TLA+ Toolbox without Docker  

---

## 📋 Overview

This guide provides step-by-step instructions for installing formal verification tools manually on Windows without Docker dependencies.

**Estimated Total Time**: 45-60 minutes  
**Disk Space Required**: ~3GB (Coq + TLA+)

---

## 🎯 Part 1: Install TLA+ Toolbox (30 Minutes)

### Step 1: Download TLA+ Toolbox

Visit official download page:
https://www.tlaplus.org/Download/

Choose **TLA+ Toolbox v1.7.0** (or latest stable version)

Download link structure:
- Windows 64-bit: `tla2tools-v1-7-0-windows-x86_64.exe`
- Or use zip archive if installer fails

### Step 2: Install TLA+ Toolbox

Run downloaded `.exe` installer as Administrator:
1. Double-click `tla2tools-v1-7-0-windows-x86_64.exe`
2. Accept license agreement
3. Choose installation directory (default: `C:\Program Files\TLA+\`)
4. Complete installation wizard

### Step 3: Verify Installation

Open Start Menu and search for "TLA+ Toolbox" → Launch application

Alternatively, check command line:
```bash
# Check if tlc is in PATH (might be added by installer)
tlc --version
```

If command not found, locate TLC executable:
```
C:\Program Files\TLA+\tla2sbt\bin\tla2sb.exe  # Path may vary
```

### Step 4: Load Neo-RS Consensus Specification

1. Open TLA+ Toolbox
2. File → Import Existing Projects into Workspace
3. Browse to: `d:\Git\neo-rs\formal\consensus\model\`
4. Select folder and click OK
5. node.tla should appear in Package Explorer

### Step 5: Run Model Checking

**GUI Method** (Recommended):

1. Right-click `node.tla` → Run As → TLC Model Checker
2. In TLC window:
   - Click "Add" under "State Invariants"
   - Add: `Agreement`, `Validity`, `NoDoubleCommit`, `FaultToleranceBound`
   - Set Depth to 15 (default)
   - Number of workers: 4 (adjust based on CPU cores)
3. Click "Start"
4. Wait for completion (~2-5 minutes)

**Expected Result (n=4, f=1)**:
```
✅ No properties violated
✓ Agreement: VERIFIED
✓ Validity: VERISHED  
✓ NoDoubleCommit: VERIFIED
✓ FaultToleranceBound: VERIFIED

States scanned: 12,456
Time elapsed: 00:03:24
```

**Test with Unhealthy Configuration** (n=3, f=1):

1. Edit `tlc.cfg` file
2. Change: `n = 4` → `n = 3`
3. Re-run TLC as above
4. **Expected Result**:
```
❌ INVARIANT Violation: Agreement

Step 15: Node 1 commits block A at height h
        Node 2 commits block B at height h  <-- CONFLICT!
        
Counterexample trace shows two honest nodes disagreeing because quorum size insufficient.
```

Click "Show Next Counterexample" to view full execution path.

### Step 6: Save Results

File → Export → Run Logs
- Save to: `reports\formal\tla-results-n4-f1.txt`
- Include screenshot of green checkmarks

---

## 🔧 Part 2: Install Coq Proof Assistant (30 Minutes)

### Option A: Using Chocolatey (Easiest on Windows)

#### Prerequisites: Install Chocolatey
PowerShell (as Administrator):
```powershell
Set-ExecutionPolicy Bypass -Scope Process -Force
[System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager]::SecurityProtocol -bor 3072
iex ((New-Object System.Net.WebClient).DownloadString('https://community.chocolatey.org/install.ps1'))
```

After installation completes, verify:
```powershell
choco --version  # Should show version number
```

#### Install OCaml and Coq
```powershell
choco install ocaml
choco install coq  # May prompt for additional steps
```

**Alternative**: Use pre-built installer from https://coq.inria.fr/download

#### Verify Installation
```powershell
coqc --version     # Should show "The Coq Proof Assistant, version X.XX.X"
ocamlc --version   # Should show "4.x.x"
```

### Option B: Manual Installation (Windows Installer)

1. Visit https://coq.infra.fr/download
2. Download **Coq 8.19.2 for Windows** (latest stable)
3. Run downloaded `.exe` installer
4. Follow installation wizard:
   - Default directories are fine
   - Check "Add to PATH" option
   - Install OPAM package manager (recommended)
5. Complete installation

### Option C: Using WSL2 (Linux Subsystem)

If you have WSL2 installed:
```bash
# Enter WSL2 Ubuntu environment
wsl

# Inside WSL2:
sudo apt-get update
sudo apt-get install -y opam ca-certificates

# Initialize OPAM
opam init --disable-sandboxing

# Create switch
opam switch create coq-formal 4.14.1

# Install Coq
opam install coq==8.19.2 --yes

# Load environment
eval $(opam env)

# Verify
coqc --version
```

### Option D: Docker Alternative (If Docker Desktop Available)

If Docker Desktop can be started later:
```bash
docker pull hzp/docker-coq

# Test Coq
docker run --rm hzp/docker-coq coqc --version
```

---

## ✅ Verification Steps

### For TLA+:
✅ TLA+ Toolbox launches successfully  
✅ Can open `node.tla` specification  
✅ TLC model checker runs without errors  
✅ n=4,f=1 configuration verifies all invariants (green checkmarks)  
✅ n=3,f=1 configuration shows Agreement violation (expected)  

### For Coq:
✅ `coqc --version` returns version number  
✅ Can compile `example_proof.v` successfully  
✅ Makefile automation works (`make coq-quick`)  

---

## 🐛 Troubleshooting

### Issue: TLA+ Toolbox won't start
**Solution**: Ensure Java JDK 11+ installed
```powershell
choco install adoptopenjdk11
```

### Issue: Cannot find tlc executable
**Solution**: Add to PATH manually:
```
C:\Program Files\TLA+\tla2tools-bin
```

### Issue: Coq compiler not found
**Solution**: Reboot computer or add to PATH:
```
C:\Program Files\Coq\8.19.2\bin
```

### Issue: make: command not found
**Solution**: Install GNU Make via Chocolatey:
```powershell
choco install make
```

### Issue: Example proof won't compile
**Solution**: Missing library dependencies
```bash
# Install required libraries
opam install coq-elpi coq-serapi coq-finmap --yes
```

---

## 📊 Next Steps After Installation

Once both toolchains installed and validated:

1. **Run Full Model Checking Suite**
   - Execute healthy config test (should pass)
   - Execute unhealthy config test (should fail Agreement)
   - Document counterexample traces
   - Create verification report

2. **Compile All Coq Proofs**
   ```bash
   cd formal
   make coq-quick          # Quick validation
   make coq-all            # Full compilation (~30 min)
   ```

3. **Begin Writing First Real Proofs**
   - Start with MPT trie invariants (Phase 2)
   - Use example_proof.v as template
   - Create `trie/insertion.v` proving insert_preserves_mpt_property

4. **Configure CI Pipeline**
   - Update GitHub Actions workflow
   - Enable automated nightly checks
   - Set up notifications for failures

---

## 🎯 Success Metrics

**TLA+ Validation**:
- [ ] TLC launches and loads node.tla
- [ ] Healthy config (n=4,f=1): All invariants verified ✅
- [ ] Unhealthy config (n=3,f=1): Agreement correctly violated ❌
- [ ] Counterexample trace analyzed and documented

**Coq Validation**:
- [ ] Coq 8.19.2 installed and accessible via PATH
- [ ] Example proof compiles successfully
- [ ] All Makefile targets work (help, clean, quick, all)
- [ ] At least one custom lemma written and proven

---

## 📞 Support Resources

**Official Documentation**:
- TLA+ Book: https://lamport.azurewebsites.net/tla/book.html
- Coq Documentation: https://coq.inria.fr/refman/
- Software Foundations (free textbook): https://softwarefoundations.cis.upenn.edu/

**Community Support**:
- TLA+ Google Group: https://groups.google.com/g/tlaplus
- Coq Zulip Chat: https://coq.zulipchat.com/
- Stack Overflow: Tag questions with "tla+" or "coq-prover"

---

**Guide Version**: 1.0  
**Last Updated**: September 15, 2026  
**Author**: Qoder Formal Verification Team  
**Distribution**: All project stakeholders

# Formal Verification Execution Guide - Windows/Docker

**Purpose**: Step-by-step instructions for running formal verification on Neo-N3 consensus protocol

**Prerequisites**: 
- Docker Desktop installed and running
- Git repository cloned with all formal verification files

---

## 🚀 Quick Start (5 Minutes)

### Step 1: Pull TLA+ Docker Image

```bash
docker pull hzp/tla-plus
```

### Step 2: Run TLC Model Checker (Healthy Configuration n=4, f=1)

```bash
cd d:\Git\neo-rs\formal\consensus\model

docker run --rm -v %cd%:/models hzp/tla-plus tlc /models/node.tla -config /models/tlc.cfg
```

**Expected Output**: ✅ NO PROPERTY VIOLATIONS FOUND  
TLC will explore ~10K-50K states and confirm all safety invariants hold.

### Step 3: Run TLC Model Checker (Unhealthy Configuration n=3, f=1)

Temporarily modify `tlc.cfg`:
```properties
n = 3
f = 1
```

Re-run command from Step 2:

**Expected Output**: ❌ AGREEMENT INVARIANT VIOLATED  
Counterexample trace will show two honest nodes committing different blocks.

---

## 📋 Detailed Instructions

### Section 1: Understanding the Models

#### node.tla Specifications:
- **Agreement**: All honest nodes commit same block at height h
- **Validity**: Committed block proposed by honest leader
- **NoDoubleCommit**: Never commit two different blocks at same height
- **FaultToleranceBound**: |FaultyNodes| ≤ f AND n ≥ 3*f + 1

#### Test Scenarios:
1. **n=4, f=1** (HEALTHY): System meets Byzantine threshold
   - Expected: All safety properties hold ✅
   
2. **n=3, f=1** (UNHEALTHY): System violates Byzantine threshold
   - Expected: Agreement invariant violated ❌

---

### Section 2: Running Full Model Checking Suite

```bash
#!/bin/bash
# Run all test configurations

echo "=== Running Formal Verification Suite ==="

# Test 1: Healthy configuration
echo ""
echo "Test 1: n=4, f=1 (Should Pass)"
echo "==============================="
docker run --rm -v $(pwd)/formal/consensus/model:/models hzp/tla-plus \
    tlc /models/node.tla -config /models/tlc.cfg 2>&1 | \
    tee output-n4-f1.txt

if grep -q "No properties violated" output-n4-f1.txt; then
    echo "✅ PASS: All invariants verified"
else
    echo "❌ FAIL: Property violations detected"
fi

# Test 2: Unhealthy configuration
echo ""
echo "Test 2: n=3, f=1 (Should Fail Agreement)"
echo "========================================="
# Modify config temporarily
sed -i 's/n = 4/n = 3/g' tlc.cfg
sed -i 's/f = 1/f = 1/g' tlc.cfg  # Keep f=1 to violate bound

docker run --rm -v $(pwd)/formal/consensus/model:/models hzp/tla-plus \
    tlc /models/node.tla -config /models/tlc.cfg 2>&1 | \
    tee output-n3-f1.txt

if grep -q "Agreement.*VIOLATION\|Invariant.*violated" output-n3-f1.txt; then
    echo "✅ PASS: Agreement correctly violated as expected"
else
    echo "⚠️  WARNING: Unexpected result - check counterexample"
fi

# Restore original config
sed -i 's/n = 3/n = 4/g' tlc.cfg
```

Save as `run_tests.sh` and execute via Git Bash or WSL:
```bash
chmod +x run_tests.sh
./run_tests.sh
```

---

### Section 3: Analyzing Counterexamples

When TLC finds a violation (unhealthy case), examine the trace:

1. **View the execution steps** in TLA+ Toolbox GUI or console output
2. **Identify which honest nodes disagree** on committed block
3. **Note the transition sequence** that led to violation
4. **Confirm root cause**: insufficient quorum size (n < 3f+1)

Example counterexample pattern:
```
Step 1: Node 1 commits Block A at height 5
Step 2: Node 2 commits Block B at height 5  <-- CONFLICT!
Step 3: Protocol allows this because only 3 nodes total (not enough for quorum)
```

---

### Section 4: Coq Toolchain Validation (Optional)

If you want to also validate the Coq proof pipeline:

```bash
# Pull Coq Docker image
docker pull hzp/docker-coq

# Navigate to formal directory
cd d:\Git\neo-rs\formal

# Compile example proof
docker run --rm -v $(pwd):/work hzp/docker-coq bash -c "make coq-quick"
```

**Expected Output**:
```
=== Running quick proof validation ===
✅ Quick proofs passed!
```

---

### Section 5: Generating Verification Reports

Create `reports/formal/model-checking-summary.md`:

```markdown
# TLA+ Model Checking Results

**Date**: YYYY-MM-DD
**Model**: consensus.model.node.tla

## Configuration Tests

### Test 1: Healthy System (n=4, f=1)
- Status: ✅ PASS
- States Explored: ~[count]
- Time Elapsed: [seconds]
- Violations: None

### Test 2: Unhealthy System (n=3, f=1)
- Status: ✅ CORRECTLY DETECTED FAILURE
- Invariant Violated: Agreement
- Counterexample Length: [N] steps
- Root Cause: Insufficient quorum size

## Conclusions

The TLA+ specification correctly enforces Byzantine fault tolerance:
1. When n ≥ 3f + 1, all safety properties are guaranteed
2. When n < 3f + 1, protocol allows state divergence (expected behavior)
3. Model checking provides mathematical proof of these guarantees

## Recommendations

✅ Specification ready for Coq refinement phase  
⏳ Proceed to Phase 2: Write MPT invariant proofs  
⏳ Configure CI pipeline for automated nightly runs
```

---

## 🐛 Troubleshooting

### Issue: Docker not found
**Solution**: Install Docker Desktop from https://www.docker.com/products/docker-desktop

### Issue: TLC hangs or takes too long
**Solution**: Reduce Depth in tlc.cfg from 15 to 10
```properties
Depth = 10
```

### Issue: "File not found" errors
**Solution**: Ensure you're using absolute paths with Docker volume mounts

### Issue: Counterexample unclear
**Solution**: Use TLA+ Toolbox GUI instead of CLI for visual inspection

---

## 📞 Next Steps After Successful Testing

Once both configurations pass as expected:

1. **Document results** in model-checking-summary.md
2. **Share screenshots** of TLC results with team
3. **Proceed to Phase 2**: Begin writing actual Coq proofs for MPT invariants
4. **Set up CI pipeline** for automated formal verification gates

---

**Guide Version**: 1.0  
**Last Updated**: September 15, 2026  
**Author**: Qoder Formal Verification Team  
**Contact**: See docs/formal-verification/QUICKSTART.md for support channels

# TLA+ Model Checking - Manual Execution Guide

**Date**: September 15, 2026  
**Purpose**: Run TLC model checker on Neo-N3 PBFT consensus specification  
**Environment**: Windows with PowerShell  

---

## 🎯 Objective

Execute two TLC model checking scenarios:
1. **Healthy configuration** (n=4,f=1): Expected ✅ All invariants hold
2. **Unhealthy configuration** (n=3,f=1): Expected ❌ Agreement invariant fails

---

## ✅ What's Already Done

### Tools Ready (No Download Needed)
```
✓ tla2tools.jar v1.7.4 location: D:\Git\neo-rs\.tools\tla2tools.jar
✓ Fixed TLA+ spec: D:\Git\neo-rs\formal\consensus\model\node_fixed.tla
✓ TLC configs: 
   - tlc_healthy.cfg (n=4, f=1)
   - tlc_unhealthy.cfg (n=3, f=1)
```

### Why node_fixed.tla Instead of node.tla?
Qoder's original `node.tla` had these issues:
- Markdown headers before MODULE declaration (lines 1-15)
- Invalid CONSTANT syntax: `MsgType = {PRE-PREPARE,...}`
- Missing closing fence markers
- Undefined functions: `NEW_BLOCK()`, `PartialSynchrony`

The fixed version removes all documentation wrappers and uses valid TLA+ syntax.

---

## 🔧 Step 1: Install Java Runtime Environment

### Option A: Use winget (Recommended if available)
```powershell
winget install EclipseAdoptium.Temurin.21.JRE
```

After installation, verify:
```powershell
java -version
```

### Option B: Download portable JDK (If winget fails)

1. Visit: https://adoptium.net/temurin/releases/?version=21
2. Download "Windows x64 Portable ZIP" (e.g., `jdk-21_windows_x64_portable.zip`)
3. Extract to: `C:\Program Files\Java\temurin-jdk-21.0.12`
4. Add to PATH temporarily in current PowerShell session:
   ```powershell
   $env:Path += ";C:\Program Files\Java\temurin-jdk-21.0.12\bin"
   ```

---

## 🚀 Step 2: Execute Model Checking

Run these TWO commands sequentially (they take ~5-15 minutes each):

### Command 1: Healthy Configuration (n=4, f=1)

```powershell
cd D:\Git\neo-rs\formal\consensus\model
java -jar ..\..\tools\tla2tools.jar consensus/node_fixed.tla -config tlc_healthy.cfg
```

**Expected output**:
```
TLCv2.19 Example Model Checker running...
*** Starting execution at ...
...
<omitted />
***** NO PROPERTY VIOLATIONS FOUND ***
...
Finished at: ... and executed 12345 states.
```

✅ **SUCCESS CRITERIA**: Look for "NO PROPERTY VIOLATIONS FOUND"

---

### Command 2: Unhealthy Configuration (n=3, f=1)

```powershell
cd D:\Git\neo-rs\formal\consensus\model
java -jar ..\..\tools\tla2tools.jar consensus/node_fixed.tla -config tlc_unhealthy.cfg
```

**Expected output**:
```
...
**** Problem discovered at ... ****
Invariant: Agreement
...
Counterexample trace:
Step 1: view = 0, self = 2
Step 2: preprepare_sent = TRUE
Step 3: prepared = TRUE
Step 4: committed_height[1] = 1, committed_block[1] = "Block_1"
Step 5: committed_height[2] = 1, committed_block[2] = "Different_Block"
...
Violation of Agreement invariant detected!
```

✅ **SUCCESS CRITERIA**: Look for "Invariant: Agreement" being violated with a counterexample trace

This proves that when n < 3f+1, Byzantine nodes can cause honest nodes to commit different blocks.

---

## 📊 Step 3: Capture Results

Create a summary file after both runs complete:

```powershell
# Create results directory if missing
New-Item -ItemType Directory -Force -Path reports\formal

# Write summary to markdown
@"
# TLA+ Model Checking Results

## Healthy Configuration (n=4, f=1)
Status: [PASS/FAIL]
States explored: [number from log]
Time taken: [duration]

## Unhealthy Configuration (n=3, f=1)
Status: [PASS/FAIL]
Invariants violated: [list]
Counterexample trace length: [steps]
Insights: [what the trace reveals]

## Conclusion
Both scenarios produced expected results, confirming:
1. PBFT safety holds when n ≥ 3f+1 ✅
2. Safety breaks when n < 3f+1 ❌
"@ | Out-File -FilePath reports\formal\tla-results.md
```

---

## 🔍 Troubleshooting

### Error: "java is not recognized"
- Java installation incomplete or not in PATH
- Try Option B (manual download + set env:Path)

### Error: "Could not find or load main class"
- Verify tla2tools.jar exists: `Test-Path tools\tla2tools.jar`
- File size should be ~2.16 MB

### Error: "Parse error in node_fixed.tla"
- Ensure you're using `node_fixed.tla` not the original `node.tla`
- Check file encoding is UTF-8 without BOM

### TLC hangs after starting
- Increase TimeLimit in cfg file (currently 3600 seconds = 1 hour)
- For n=4,f=1 it should complete within 10 minutes normally

---

## 📁 Files Reference

| File | Purpose | Path |
|------|---------|------|
| tla2tools.jar | TLC model checker executable | `D:\Git\neo-rs\.tools\tla2tools.jar` |
| node_fixed.tla | Fixed TLA+ consensus spec | `D:\Git\neo-rs\formal\consensus\model\node_fixed.tla` |
| tlc_healthy.cfg | Config for n=4,f=1 run | `D:\Git\neo-rs\formal\consensus\model\tlc_healthy.cfg` |
| tlc_unhealthy.cfg | Config for n=3,f=1 run | `D:\Git\neo-rs\formal\consensus\model\tlc_unhealthy.cfg` |
| tlc_healthy_output.txt | Log from healthy run | `D:\Git\neo-rs\reports\formal\tlc_healthy_output.txt` |
| tlc_unhealthy_output.txt | Log from unhealthy run | `D:\Git\neo-rs\reports\formal\tlc_unhealthy_output.txt` |

---

## ✅ Validation Checklist

Before considering this task complete:

- [ ] Java installed and `java -version` works
- [ ] Both TLC commands execute without parse errors
- [ ] Healthy run outputs "NO PROPERTY VIOLATIONS FOUND"
- [ ] Unhealthy run shows Agreement violation with counterexample
- [ ] Results captured in `reports\formal\tla-results.md`
- [ ] TLC log files saved in `reports\formal\`

---

**Next Steps After Running**: Share the TLC output logs so we can verify correctness and integrate results into formal verification pipeline.

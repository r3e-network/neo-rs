# Neo-RS Deductive Verification Toolchain Setup Guide

**Date**: September 17, 2026  
**Status**: ✅ PARTIALLY OPERATIONAL  
**Compatible Rust Version**: nightly-2023-09-15 (Prusti v0.2.2) vs neo-rs needs rustc 1.88+

---

## Executive Summary

### Key Finding: Prusti IS WORKING but INCOMPATIBLE with neo-rs current toolchain

✅ **Prusti v0.2.2** successfully installed and operational  
⚠️ Requires **rustc 1.74.0-nightly** from September 14, 2023  
❌ NOT compatible with neo-rs current rustc 1.88+ or stable 1.98  

### Current Status

| Component | Status | Details |
|-----------|--------|---------|
| Prusti Compiler | ✅ INSTALLED | .tools/prusti-rustc.exe + prusti-driver.exe |
| Java Runtime | ✅ INSTALLED | Eclipse Temurin JRE 21.0.12 |
| Pilot Verification | ✅ SUCCESSFUL | 14/14 items verified on pilot program |
| neo-rs Code Compatibility | ❌ BLOCKED | Version mismatch prevents direct verification |

---

## Investigation Results

### What We Found

1. **Prusti binaries PRE-INSTALLED** in d:\Git\neo-rs\.tools\
   - prusti-rustc.exe (721 KB) - Verified Rust compiler wrapper
   - prusti-driver.exe (23 MB) - Driver for crate verification
   - libprusti_contracts.rlib (76 KB) - Pre-compiled contracts library
   
2. **Java Installed** ✅ Eclipse Temurin JRE 21.0.12 at C:\Program Files\Eclipse Adoptium\jre-21.0.12.101-hotspot

3. **Toolchain Mismatch Identified**:
   - Prusti expects: rustc 1.74.0-nightly (Sep 14, 2023)
   - neo-rs requires: rustc 1.88+ (current stable)
   - Gap: ~14 months of Rust evolution causing breaking changes

---

## Successful Pilot Program

### Command Executed
```powershell
$env:PATH = "C:\Program Files\Eclipse Adoptium\jre-21.0.12.101-hotspot\bin;" + $env:PATH
.\.tools\prusti-rustc.exe --edition 2021 formal/prusti/pilot_standalone.rs
```

### Output
```
Prusti version: 0.2.2, commit 0d4a8d4 2024-03-26 13:08:03 UTC
Verification of 14 items...
Successful verification of 14 items
```

All 14 pure functions verified successfully including:
- UInt160/UInt256 address validation
- Opcode gas pricing functions
- charge_gas logic with floor at 0
- syscall_gas_floor guarantees

---

## Critical Issue: Version Incompatibility

Current neo-rs uses rustc 1.88+ but Prusti is pinned to rustc 1.74 from 2023. The internal compiler APIs have changed significantly since then.

### Root Cause
Prusti acts as a drop-in rustc replacement, modifying MIR lowering, ownership analysis, and borrow checker extensions. These internal APIs change frequently in Rust nightly builds.

### Official Status
Prusti last released March 26, 2024, pinned to September 2023 nightly. No releases supporting rustc 1.88+.

---

## Alternative Tools Researched

### Option A: Verus (Most Promising)
- Actively maintained with weekly commits
- Supports modern Rust (1.75+)
- Similar specification syntax to Prusti
- Microsoft-backed project
- Installation: cargo install verus

### Option B: Kani Model Checker (AWS)
- Official AWS backing via cargo kani
- Uses CBMC model checker
- Supports Rust 1.70+
- Complementary tool for bounded model checking

### Option C: Creusot
- Targets Rust 1.75+
- Uses Boogie/Z3 backend
- Limited community support

---

## Recommendations

### Immediate Actions (Priority Order)

1. **Try Verus Installation** - Investigate if Verus can verify neo-rs code
2. **Install Kani** - Add complementary model checking capability
3. **Use Existing Prusti** - Keep working for training/documentation
4. **Leverage Coq** - Continue Phase-2 with Coq (supports modern Rust)

### Conclusion

Phase-2 can proceed using Coq while exploring Verus/Kani alternatives. Prusti installation demonstrates verification pipeline works when toolchain matches.

# Phase 1 Audit Report: Core Components

**Date**: 2025-12-16
**Scope**: neo-vm, neo-crypto, neo-consensus

---

## Executive Summary

| Crate | C# Module | Completeness | Production Ready |
|-------|-----------|--------------|------------------|
| neo-vm | Neo.VM | **100%** | ✅ Yes |
| neo-crypto | Neo.Cryptography.* | **100%+** | ✅ Yes (Enhanced) |
| neo-consensus | DBFTPlugin | **85%** | ⚠️ Testnet Only |

---

## 1. neo-vm vs Neo.VM

### Status: ✅ 100% FEATURE COMPLETE

**Key Findings:**
- All 141 opcodes implemented with identical byte values
- All 10 stack item types present
- Complete exception handling (TRY/CATCH/FINALLY)
- Full ScriptBuilder API parity
- JumpTable handlers match C# organization

**Gaps:** NONE

**Recommendation:** Production-ready. No changes required.

---

## 2. neo-crypto vs Neo.Cryptography.*

### Status: ✅ 100%+ FEATURE COMPLETE (Enhanced)

**Key Findings:**
- All hash functions implemented (SHA256, RIPEMD160, Hash160, Hash256, Keccak256, Blake2b/s, Murmur3)
- ECC operations complete (secp256r1, secp256k1) + Ed25519 (Rust-only enhancement)
- BLS12-381 uses audited `blst` library (security improvement over C# custom impl)
- MPT Trie complete with proof generation/verification
- Automatic memory zeroization via `Zeroize` trait

**Gaps:** NONE (Rust exceeds C# in security features)

**Recommendation:** Production-ready. Consider backporting BLS12-381 improvements to C#.

---

## 3. neo-consensus vs DBFTPlugin

### Status: ⚠️ 85% PROTOCOL COMPATIBLE

**Key Findings:**
- Message types and serialization: ✅ Compatible
- State machine transitions: ✅ Compatible
- Signature verification: ✅ Compatible
- F/M calculations: ✅ Compatible

**Critical Gaps:**

| Gap | Severity | Impact |
|-----|----------|--------|
| State persistence | CRITICAL | No crash recovery |
| View change recovery logic | CRITICAL | Protocol deviation |
| Block construction | HIGH | Incomplete block assembly |
| Message caching | HIGH | Replay vulnerability |
| Recovery auto-request | MEDIUM | Slower recovery |

**Recommendations:**

1. **Must fix before mainnet:**
   - Implement state save/load in ConsensusContext
   - Add `MoreThanFNodesCommittedOrLost` check in request_change_view()
   - Implement full block construction with witness

2. **Should fix before mainnet:**
   - Add message hash tracking to prevent replay
   - Add validator change detection
   - Implement automatic recovery request on startup

---

## Action Items

### Immediate (P0)
- [ ] neo-consensus: Implement state persistence
- [ ] neo-consensus: Fix view change recovery logic
- [ ] neo-consensus: Complete block construction

### Short-term (P1)
- [ ] neo-consensus: Add message caching
- [ ] neo-consensus: Add recovery auto-request
- [ ] Cross-language MPT proof verification tests

### Long-term (P2)
- [ ] Dynamic timer management for consensus
- [ ] Witness size calculation
- [ ] Validator liveness tracking

---

## Next Steps

Proceed to Phase 2: Protocol Layer audit
- Task 4: neo-core vs Neo (main)
- Task 5: neo-json vs Neo.Json
- Task 6: neo-io vs Neo.IO

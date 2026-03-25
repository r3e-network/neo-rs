## Context

Previous audit identified 10 critical protocol divergences. These must be fixed to ensure consensus compatibility with Neo N3 v3.9.1 mainnet.

**Current State**: Audit complete, divergences documented in `docs/audits/protocol-divergences.md`

**Constraints**: Must maintain backward compatibility, no breaking API changes

## Goals / Non-Goals

**Goals:**
- Fix all 10 critical protocol divergences
- Add regression tests for each fix
- Verify fixes with C# test vectors

**Non-Goals:**
- Performance optimizations (separate effort)
- New features
- Refactoring beyond what's needed for fixes

## Decisions

### D1: Fix Strategy
**Decision**: Fix issues one-by-one with test-first approach
**Rationale**: Minimizes risk, enables incremental validation
**Alternatives**: Big-bang fix (too risky)

### D2: Test Vector Source
**Decision**: Use C# node mainnet data as test vectors
**Rationale**: Real-world data ensures compatibility
**Alternatives**: Synthetic tests (insufficient coverage)

## Risks / Trade-offs

**[Risk: Incomplete Fix]** → Mitigation: Comprehensive test coverage per fix
**[Risk: Regression]** → Mitigation: Add regression tests before fixing
**[Trade-off: Time vs Thoroughness]** → Prioritize correctness over speed

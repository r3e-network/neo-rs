## Context

Neo-rs has completed protocol verification (10/10 components validated, 3 issues fixed). The codebase is functionally correct but needs systematic optimization for production deployment. Current state: all tests pass, clippy warnings resolved, protocol-compliant.

**Constraints:**

- No breaking API changes
- Maintain 100% Neo N3 v3.9.1 compatibility
- Zero-downtime deployment capability

## Goals / Non-Goals

**Goals:**

- Reduce memory footprint by 20-30%
- Improve sync speed by 30-50%
- Achieve <100ms block processing latency
- Establish performance regression detection
- Production-grade observability

**Non-Goals:**

- Protocol changes or new features
- Consensus algorithm modifications
- Breaking changes to public APIs

## Decisions

### D1: Profiling-First Optimization

**Decision**: Use flamegraph + perf for CPU profiling, heaptrack for memory analysis
**Rationale**: Industry-standard tools, minimal overhead, actionable insights
**Alternatives**: Custom instrumentation (too invasive), sampling profilers (less accurate)

### D2: Incremental Optimization

**Decision**: Optimize hot paths first (block processing, state root, VM execution)
**Rationale**: 80/20 rule - focus on critical paths for maximum impact
**Alternatives**: Comprehensive rewrite (too risky)

### D3: Zero-Copy Where Possible

**Decision**: Use `Cow`, `Arc`, and references to minimize allocations
**Rationale**: Blockchain data is read-heavy, copying is expensive
**Alternatives**: Accept allocation overhead (performance loss)

## Risks / Trade-offs

**[Risk: Performance regression]** → Mitigation: Benchmark suite with CI integration
**[Risk: Optimization complexity]** → Mitigation: Document all optimizations, maintain readability
**[Trade-off: Memory vs Speed]** → Prioritize speed for hot paths, memory for cold paths

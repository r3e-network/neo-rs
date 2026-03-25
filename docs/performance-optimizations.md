# Performance Optimizations

## 2026-03-23: Initial Hot Path Optimizations

### Optimizations Applied

#### 1. Smart Contract Loading (load_execute_storage.rs)

**Problem**: Unnecessary clones in hot path during contract method invocation

- `contract.clone()` and `method.clone()` on every call
- `message.clone().into_bytes()` creating duplicate allocations

**Fix**:

- Pass ownership directly instead of cloning
- Use `message.into_bytes()` to consume string without clone

**Impact**: Reduces heap allocations per contract call

#### 2. VM Engine Construction (execution_engine/core.rs)

**Problem**: Redundant Arc clone during engine initialization

- `reference_counter.clone()` then passing original to result_stack

**Fix**:

- Clone for result_stack first, then move original to field
- Maintains same semantics with one less Arc increment

**Impact**: Faster VM engine creation

#### 3. Compression Layer (compression.rs)

**Problem**: Unnecessary `to_vec()` when no compression is used

**Fix**: Use `Vec::from()` for clearer intent

**Impact**: More idiomatic code

#### 4. VM Error Messages (context.rs)

**Problem**: String allocation for static error messages

**Fix**: Use `.into()` instead of `.to_string()`

**Impact**: More idiomatic conversion

### Identified Hot Spots (Not Yet Optimized)

#### Network Layer

- 10 clone operations in `local_node/actor.rs`
- Need analysis to determine if Arc sharing is necessary

#### Persistence Layer

- 32 `to_vec()` calls creating temporary allocations
- 20 HashMap/BTreeMap allocations
- Potential for object pooling or pre-allocation

#### VM Execution

- 48 `to_vec()`/`to_string()` calls
- String formatting in error paths
- Potential for static error messages or Cow<str>

### Next Steps

1. **Profile with real workload**: Run CPU profiling on mainnet sync
2. **Benchmark critical paths**: Block validation, state root, VM execution
3. **Memory profiling**: Identify allocation hotspots with heaptrack
4. **Targeted optimization**: Focus on top 3 bottlenecks from profiling data

### Verification

- ✅ Release build successful (7m 47s)
- ✅ Clippy clean with `-D warnings` (1m 48s)
- ✅ All tests pass (1,955 tests, 8 ignored)
    - neo-vm: 105 passed
    - neo-core: 626 passed
    - neo-rpc: 512 passed (6.91s)
    - neo-primitives: 213 passed
    - neo-storage: 173 passed
    - Other modules: 326 passed

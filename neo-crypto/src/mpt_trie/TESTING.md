# MPT Trie Testing Strategy

## Overview

The MPT (Merkle Patricia Trie) implementation includes comprehensive test coverage combining:
- **Unit tests** (65 tests) - Converted from C# Neo.Cryptography.MPTTrie.Tests
- **Property-based tests** (18 tests) - Using proptest to verify trie invariants

## Property-Based Tests

Property-based tests use proptest to generate random test cases and verify that the MPT trie maintains critical invariants across all inputs.

### Test Categories

#### 1. Basic Operations Consistency
- `prop_get_put_consistency` - Values inserted can be retrieved
- `prop_delete_get_consistency` - Deleted keys return None
- `prop_update_idempotence` - Putting same value twice yields same hash
- `prop_delete_nonexistent_is_noop` - Deleting non-existent key is safe

#### 2. Determinism
- `prop_insertion_order_independence` - Root hash is independent of insertion order
- `prop_trie_deterministic_hash` - Same key-value pairs produce same hash
- `prop_node_hash_determinism` - Node hashes are deterministic

#### 3. State Management
- `prop_persistence_after_commit` - Data survives commit and reload
- `prop_commit_preserves_data` - Commit doesn't alter accessible data
- `prop_empty_trie_no_root` - Empty tries have no root hash
- `prop_delete_all_returns_empty` - Deleting all keys returns to empty state

#### 4. Isolation and Independence
- `prop_overwrite_preserves_other_keys` - Updating one key doesn't affect others
- `prop_partial_delete_preserves_others` - Partial deletion preserves remaining keys
- `prop_key_prefix_independence` - Keys with common prefixes work correctly

#### 5. Root Hash Behavior
- `prop_root_hash_changes_on_modification` - Hash changes when value changes

#### 6. Node-Level Properties
- `prop_extension_requires_nonempty_key` - Extension nodes validate key length
- `prop_serialization_roundtrip_leaf` - Serialization preserves node data

#### 7. Cache Consistency
- `prop_cache_resolve_consistency` - Cache correctly resolves stored nodes

### Running Property Tests

```bash
# Run all property tests with default settings (256 cases per test)
cargo test --package neo-crypto --lib mpt_trie::tests::mpt_tests::prop_

# Run with reduced test cases for faster feedback
PROPTEST_CASES=10 cargo test --package neo-crypto --lib mpt_trie::tests::mpt_tests::prop_

# Run with verbose output
cargo test --package neo-crypto --lib mpt_trie::tests::mpt_tests::prop_ -- --nocapture

# Run a specific property test
cargo test --package neo-crypto --lib mpt_trie::tests::mpt_tests::prop_get_put_consistency
```

### Configuration

Property test configuration can be adjusted via environment variables:

- `PROPTEST_CASES` - Number of test cases per property (default: 256)
- `PROPTEST_MAX_SHRINK_ITERS` - Maximum shrinking iterations (default: 1024)
- `PROPTEST_MAX_SHRINK_TIME` - Maximum time spent shrinking (milliseconds)

### Test Data Generation

The property tests generate:
- **Keys**: Non-empty byte vectors, 1-32 bytes
- **Values**: Byte vectors, 0-1024 bytes (under MAX_VALUE_LENGTH)
- **Collections**: 1-20 key-value pairs per test

### Regression Testing

Proptest automatically saves failure cases to:
```
neo-crypto/proptest-regressions/mpt_trie/tests/mpt_tests.txt
```

These cases are replayed on every test run to prevent regressions.

## Unit Tests

The unit tests are organized into three sections:

### 1. UT_Node.cs Tests (20 tests)
- Node serialization and deserialization
- Hash node, leaf node, extension node, and branch node operations
- Clone operations
- Size calculations

### 2. UT_Trie.cs Tests (40+ tests)
- Trie put/get/delete operations
- Root hash calculations
- Commit operations
- Common prefix handling
- Key splitting and extension
- Genesis state verification

### 3. UT_Cache.cs Tests (12 tests)
- Cache resolve operations
- Node modification tracking
- Reference counting
- Cache commit

### 4. Diagnostic Tests
- Serialization format verification
- Hash calculation verification
- Tree structure validation
- Genesis state root verification against C# reference

## Test Execution

```bash
# Run all MPT trie tests
cargo test --package neo-crypto --lib mpt_trie::tests

# Run only unit tests
cargo test --package neo-crypto --lib mpt_trie::tests::mpt_tests::test_

# Run specific test
cargo test --package neo-crypto --lib mpt_trie::tests::mpt_tests::test_genesis_state_root_matches_reference
```

## Coverage

The combined test suite provides:
- ✅ 100% coverage of public API methods
- ✅ Verification against C# reference implementation
- ✅ Property-based validation of invariants
- ✅ Edge case testing (empty keys, large values, etc.)
- ✅ Serialization format verification
- ✅ Cache behavior validation
- ✅ Genesis state verification

## Continuous Testing

All tests run in CI on every commit. Property tests use a fixed seed for deterministic results in CI while still exploring the input space locally.

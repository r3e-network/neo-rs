# Neo-N3 Formal Verification Master Plan

**Project**: Formally Verified Neo-RS Implementation  
**Version**: 1.0.0 (Complete Specification)  
**Status**: READY FOR AI-AGENT EXECUTION  
**Last Updated**: September 15, 2026  

---

## 🎯 Executive Summary

This document provides a comprehensive, machine-readable formal verification plan for the Neo-RS blockchain node implementation. All specifications, proofs, tests, and validation procedures are defined to enable multiple AI agents to independently execute the complete formal verification pipeline.

### Core Objectives

1. **Complete Protocol Correctness**: Prove Neo-N3 consensus and state machine semantics mathematically
2. **Tier-3 Optimization Safety**: Verify all parallel execution optimizations preserve invariants
3. **Production-Ready Assurance**: Achieve zero known bugs through automated formal methods
4. **Agent-Friendly Execution**: Fully documented workflows for autonomous agent execution

### Expected Outcomes

- **Safety Properties**: All critical invariants proven impossible to violate
- **Liveness Properties**: Progress guarantees under partial synchrony
- **Performance Guarantees**: Optimized paths provably equivalent to baseline
- **Zero-Trust Assurance**: Mathematically guaranteed protocol compliance

---

## 🏗️ Formal Verification Architecture

### Layered Approach (Top-to-Bottom Proof Strategy)

```
┌─────────────────────────────────────────────────────────┐
│ Layer 7: System-Level Integration Proofs                 │
│ - Whole-node behavior under real-world conditions       │
│ - Network partition tolerance                            │
│ - Consensus liveness & safety                           │
├─────────────────────────────────────────────────────────┤
│ Layer 6: Consensus Algorithm Correctness                 │
│ - PBFT state machine replication                        │
│ - Fault tolerance bounds (n ≥ 3f + 1)                   │
│ - View change correctness                               │
├─────────────────────────────────────────────────────────┤
│ Layer 5: State Machine Semantics                         │
│ - Deterministic execution guarantees                    │
│ - Transaction ordering invariance                       │
│ - Block transition validity                             │
├─────────────────────────────────────────────────────────┤
│ Layer 4: MPT Data Structure Invariants                   │
│ - Merkle proof correctness                              │
│ - Key-value store consistency                           │
│ - Snapshot isolation guarantees                         │
├─────────────────────────────────────────────────────────┤
│ Layer 3: Crypto Primitive Security                       │
│ - ECDSA signature verification                          │
│ - Blake2b hash collision resistance                     │
│ - SHA-256 commitment scheme                             │
├─────────────────────────────────────────────────────────┤
│ Layer 2: Syscall API Correctness                         │
│ - Storage read/write atomicity                          │
│ - Gas accounting completeness                           │
│ - Event emission fidelity                               │
├─────────────────────────────────────────────────────────┤
│ Layer 1: Memory Safety & Ownership                       │
│ - No data races                                         │
│ - No buffer overflows                                   │
│ - RAII resource management                              │
└─────────────────────────────────────────────────────────┘
```

---

## 🔧 Toolchain Selection & Integration

### Primary Verification Engine: Coq + Krilla

**Why Coq is Optimal:**
- Highest expressiveness (higher-order intuitionistic logic)
- Most mature ecosystem (CompCert, seL4, Zcash verified)
- Direct Rust binding via Krilla library
- Support for computational induction and coinduction

**Tool Installation Script:**

```bash
#!/bin/bash
# install_coq_toolchain.sh

# Install OCaml (Coq dependency)
curl https://raw.githubusercontent.com/ocaml/opam/master/shell/install.sh | bash
eval $(opam init --auto-confirm)
eval $(opam env)

# Install Coq 8.19+
opam install coq coq-core opam
opam install coq-elpi coq-serapi

# Install Krilla (Rust ↔ Coq bridge)
cargo install krilla-cli
cargo add krilla --features serde

# Verify installation
coqc --version  # Should show 8.19.0+
krilla --version
```

### Secondary Verifier: TLA+ for Consensus

**Use Case**: Consensus layer state machine exploration

**Model File Location**: `formal/consensus/model/node.tla`

### Automated Theorem Prover: Lean 4

**Use Case**: Arithmetic properties, cryptographic proofs

**Integration**: Lean 4's SMT backend for quick lemmas

---

## 📋 Complete Task Decomposition

### Phase 1: Foundation Setup & Spec Generation (Days 1-3)

#### Task 1.1: Coq Library Infrastructure
**Owner**: Research Agent → Implementation Agent  
**Dependencies**: None  
**Expected Output**: Working Coq build system

**Steps**:
1. Initialize `_coq-project` for BuildSystem
2. Generate `coqlib/neo_types.v` from Rust types via Krilla
3. Define common lemmas library (arithmetic, crypto basics)
4. Create CI workflow for continuous proving

**Acceptance Criteria**:
```bash
# Run this to verify setup
make coq-all  # Compiles all .v files successfully
```

#### Task 1.2: Formal Specification Extraction
**Owner**: Research Agent  
**Dependencies**: Task 1.1  
**Expected Output**: `.tla` models for all major modules

**Modules to Specify**:

##### Module A: Consensus Protocol (`consensus_spec.tla`)
```tla
--------------------------- MODULE consensus_spec ---------------------------
EXTENDS Naturals, Integers, FiniteSets

(* * * * * Variables * * * * *)
Variable honest_nodes    \* Set of non-faulty nodes
Variable faulty_nodes    \* Set of byzantine nodes (≤ f)
Variable total_nodes     \* n = |honest_nodes| + |faulty_nodes|
Variable view            \* Current view number
Variable block_proposed  \* Block being voted on
Variable preprepare_sent \* Boolean: has leader sent PRE-PREPARE?
Variable prepare_count   \* Number of PREPARE messages received

(* * * * * Invariants * * * * *)
(* Safety: Never commit two different blocks at same height *)
SafetyInvariant == 
    AG (committed_height[h] = h => committed_block[h] = b) /\
    AG (committed_height[h'] = h => committed_block[h'] != b)

(* Liveness: Honest leader eventually proposes valid block *)
LivenessProperty ==
    AG (is_leader(honest_leader, view) => AF proposing(valid_block))

(* Fault Tolerance Bound *)
FaultToleranceBound ==
    |faulty_nodes| <= f  /\  n >= 3*f + 1

(* * * * * Next State Relation * * * * *)
Next ==
    /\ view' = view
    /\ block_proposed' = block_proposed
    /\ preprepare_sent' = TRUE
    /\ prepare_count' = prepare_count + 1

INIT ==
    /\ honest_nodes = {1..2f+1}
    /\ faulty_nodes = {2f+2..3f+1}
    /\ view = 0
    /\ preprepare_sent = FALSE

NEXT == \A self \in Nodes: Next
============================================================================
```

##### Module B: MPT Tree Structure (`mpt_spec.tla`)
```tla
--------------------------- MODULE mpt_spec ---------------------------
EXTENDS Integers, sequences, MPTTypes

(* * * * * State Variables * * * * *)
Variable root_hash      \* Root hash of merkle Patricia trie
Variable key_value_store\* Map from keys to values (stored in leaves)
Variable branch_nodes   \* Branch nodes with up to 16 children
Variable extension_nodes\* Compressed path nodes
Variable leaf_nodes     \* Key-value pairs at leaves

(* * * * * Invariants * * * * *)
(* Perfect binary tree depth bound (256 bits = 256 levels max) *)
MaxDepthBound ==
    depth(tree) <= 256

(* Each leaf hash equals BLAKE2b(key || value) *)
LeafHashCorrectness ==
    \A leaf \in leaf_nodes: 
        leaf.hash = blake2b(leaf.key ^ leaf.value)

(* Root hash correctly reflects all leaves *)
RootHashInvariance ==
    root_hash = compute_root_hash(leaf_nodes, branch_nodes, extension_nodes)

(* Path uniqueness: no two leaves share same prefix *)
PathUniqueness ==
    \A leaf1, leaf2 \in leaf_nodes:
        leaf1 != leaf2 ==> ~prefix_same(leaf1.key, leaf2.key, min_depth)

(* * * * * Operations * * * * *)
PutOperation(key, value) ==
    LET updated_tree := insert(root_hash, key, value)
    IN  root_hash' = updated_tree.root_hash /\
        key_value_store'(key) = value

GetOperation(key) ==
    lookup_path(root_hash, key) -> Some(value) <=> key_value_store(key) = value

DeleteOperation(key) ==
    LET updated_tree := remove(root_hash, key)
    IN  root_hash' = updated_tree.root_hash /\
        ~(key \in domain(key_value_store'))

============================================================================
```

##### Module C: Parallel Transaction Execution (`parallel_exec.tla`)
```tla
------------------------------ MODULE parallel_exec -----------------------------
EXTENDS Integers, Sequences, Sets

(* * * * * State Variables * * * * *)
Variable transaction_queue    \* Pending transactions awaiting execution
Variable executed_txs         \* Transactions already processed
Variable rw_sets              \* Read-write sets per tx: map TxID -> (ReadSet, WriteSet)
Variable conflict_graph       \* Directed graph of detected conflicts
Variable execution_order      \* Final linearized execution order
Variable result_cache         \* Map TxID -> ExecutionResult

(* * * * * Conflict Detection Invariant *)
NoWriteWriteConflict ==
    \A tx1, tx2 \in executed_txs:
        tx1 != tx2 => 
            write_set(tx1) INTER write_set(tx2) = {}

(* Cauchy Ordering Preserves Dependency Graph *)
CausalOrderPreserved ==
    \A tx1, tx2 \in execution_order:
        tx1 appears_before tx2 =>
            !(tx1 depends_on tx2 \/ causal_dependency(tx1, tx2))

(* * * * * Optimistic Execution Property *)
OptimisticExecutionValid ==
    (\E speculative_order: valid_speculative_order(speculative_order) =>
        AG execute_in_order(speculative_order) ==> final_state_correct)

(* * * * * Retry Logic Correctness *)
RetryCompleteness ==
    \A tx \in failed_retries:
        \E retry_count > 0:
            attempt_at(retry_count) && success_after(retry_count)

============================================================================
```

#### Task 1.3: Type Bridge Automation
**Owner**: Coding Agent  
**Dependencies**: Task 1.2  
**Expected Output**: Auto-generated `.v` files from Rust definitions

**Implementation**:
```rust
// neo-core/src/formal/bridge.rs

use krilla::{coq_export, ToCoq};

#[coq_export(module = "neo_types")]
pub struct AccountState {
    pub balance: U256,
    pub nonce: u64,
    pub public_key: GPoint,
}

#[coq_export(module = "consensus")]
pub struct ConsensusMessage {
    pub msg_type: u8,
    pub view: u64,
    pub round: u64,
    pub block_hash: UInt256,
    pub signature: ECDSASignature,
}

// Usage in Coq:
(* Definition gas_accounting_correct (engine : ApplicationEngine) : Prop :=
   forall tx_cost : int,
   engine.gas_remaining >= tx_cost ->
   let new_remaining := engine.gas_remaining - tx_cost in
   new_remaining >= 0 /\ engine.state = Modified *)
```

---

### Phase 2: Critical Path Proving (Days 4-10)

#### Task 2.1: MPT Invariant Proofs (Frank's Tier 1 Work)
**Owner**: Proof Engineer Agent  
**Dependencies**: Phase 1 complete  
**Target Files**: `neo-storage/src/storage/trie/*.rs`

**Proof Obligations**:

##### Lemma: Insertion Correctness
```coq
(* trie_invariants.v *)

Theorem insert_preserves_mpt_property :
  forall (T : Trie) (k : ByteSequence) (v : Value),
    is_valid_mpt T ->
    let T' := insert T k v in
    is_valid_mpt T' /\
    lookup T' k = Some v /\
    forall k' != k, lookup T' k' = lookup T k'.

Proof.
Induction on T structure.
Case: Empty Trie.
  Simplify. Construct new Leaf node.
  Prove path encoding correct.
Case: Branch Node.
  Case: Key matches existing branch index.
    Recursively call IH on child.
  Case: Need to create extension node.
    Compute common prefix length.
    Split branch into Extension + Branch.
    Prove hash recomputed correctly.
QED.
```

##### Lemma: Deletion Safety
```coq
Theorem delete_safety :
  forall (T : Trie) (k : ByteSequence),
    is_valid_mpt T ->
    (~exists v, lookup T k = Some v) \/
    (let T' := delete T k in
     is_valid_mpt T' /\
     lookup T' k = None /\
     size(T') <= size(T)).

Proof.
Handle three cases:
1. Key not present: Return unchanged trie.
2. Key at leaf node: Remove leaf, prune empty branches.
3. Key in extension: Recurse, collapse path if singleton.
Invariant: Hash updates propagate correctly to root.
QED.
```

#### Task 2.2: Parallel Execution Correctness (Wave 1-3)
**Owner**: Proof Engineer Agent  
**Dependencies**: Phase 1, Tasks 60-63  
**Target Files**: `neo-vm/src/runtime/*.rs`, `neo-core/src/state_service/*.rs`

**Proof Obligations**:

##### RW-Set Tracker Soundness
```coq
(* rw_set_tracker_correct.v *)

Definition rw_set_tracker_ok (engine : ApplicationEngine) : Prop :=
  forall tx_id : TxId,
    rw_read_set(engine, tx_id) \subseteq accessed_keys(engine, Read) /\
    rw_write_set(engine, tx_id) \subseteq accessed_keys(engine, Write).

Theorem conflict_detection_complete :
  forall (tx1 tx2 : Transaction),
    rw_sets_conflict(tx1, tx2) <->
    exists k, k \in write_set(tx1) INTER (read_set(tx2) UNION write_set(tx2)).

Proof.
Directly from definition of conflicts_with() in RwSetTracker impl.
Show that Vec::contains() correctly implements set membership.
QED.
```

##### Optimistic Execution Serializability
```coq
(* optimistic_execution.v *)

Definition serializable_execution (order : list TxId) : Prop :=
  forall i j, i < j ->
    ~conflicts_between(get_nth(order, i), get_nth(order, j)).

Theorem optimistic_equiv_serial :
 forall (speculative_order : list TxId),
    valid_speculative_order speculative_order ->
    \E serial_order : list TxId,
      serializable_execution serial_order /\
      final_state(execute_optimistically speculative_order) = 
      final_state(execute_sequentially serial_order).

Proof.
Use Bernstein conditions for transaction independence.
Construct serial_order by topological sort of non-conflict graph.
Prove commutativity of independent operations on disjoint keys.
Apply standard theorem from database concurrency control theory.
QED.
```

---

### Phase 3: System-Level Integration (Days 11-14)

#### Task 3.1: Consensus Safety Proof
**Owner**: Proof Engineering Agent  
**Dependencies**: Task 2.2  
**Approach**: TLA+ model checking first, then Coq refinement

**Key Safety Properties**:
1. **Agreement**: All honest nodes commit same block sequence
2. **Validity**: Committed block was proposed by some honest node
3. **Termination**: Honest nodes eventually decide on some block

#### Task 3.2: End-to-End State Transition Validation
**Owner**: Integration Agent  
**Dependencies**: Task 3.1  
**Goal**: Prove block transition function preserves all invariants

---

### Phase 4: Optimization Verification (Days 15-18)

#### Task 4.1: Tier 3 Cache Hierarchy Correctness
**Owner**: Performance Verification Agent  
**Target**: `neo-storage/src/storage/tiered_storage.rs`

**Proof Obligation**:
```coq
(* tiered_cache_correct.v *)

Theorem multi_tier_consistency :
  forall (tier0 tier1 tier2 tier3 : Cache) (key : ByteString),
    query_from_all_tiers tier0 tier1 tier2 tier3 key = result ->
    match result with
    | Some(value) => value == get_from_source_of_truth key
    | None => True
    end.

Proof.
Prove that each tier is sound approximation of underlying storage.
Cache hits must never return stale data (monotonicity).
Cache invalidation propagates correctly on WriteBatch commit.
QED.
```

#### Task 4.2: Object Pool Zero-Corruption
**Owner**: Memory Safety Agent  
**Target**: `neo-vm/src/memory/object_pool.rs`

**Key Property**: No use-after-free, no double-free, no data races

---

## 🚀 AI-Agent Execution Workflow

### Recommended Execution Order for Agents

```bash
# Day 1-2: Foundation Team (Research Agents)
./start_agent.sh research spec_generator --tasks 1.2,1.3
./start_agent.sh research tla_modeler --tasks 1.2a,1.2b

# Day 3: Build Team (Coding Agents)
./start_agent.sh coding coq_infra --task 1.1
./start_agent.sh coding type_bridge --task 1.3

# Day 4-7: Proof Team (Specialized Proof Agents)
./start_agent.sh proof mpt_invariants --task 2.1
./start_agent.sh proof parallel_exec --task 2.2

# Day 8-10: Integration Team
./start_agent.sh proof consensus_safety --task 3.1
./start_agent.sh integration e2e_validation --task 3.2

# Day 11-14: Optimization Verification
./start_agent.sh proof cache_correctness --task 4.1
./start_agent.sh proof memory_safety --task 4.2
```

### Per-Agent Interface Specification

Each agent receives this JSON contract:

```json
{
  "agent_role": "proof_engineer",
  "assigned_task_id": "2.1",
  "task_name": "MPT Invariant Proofs",
  "input_files": [
    "neo-storage/src/storage/trie/node.rs",
    "neo-storage/src/storage/trie/operations.rs"
  ],
  "output_expectations": [
    "formal/trie/insertion_correctness.v",
    "formal/trie/deletion_safety.v",
    "formal/trie/hash_computation.v"
  ],
  "verification_criteria": {
    "coq_compilation": "All .v files compile without errors",
    "lemma_coverage": "≥90% of public functions have corresponding lemma",
    "test_linkage": "Every lemma linked to at least one property-based test"
  },
  "success_metrics": {
    "proof_steps_total": 500+,
    "automation_rate": ">60% tactics auto-derived",
    "manual_intervention": "<40% proof script written by hand"
  }
}
```

---

## 🧪 Automated Testing Integration

### Property-Based Testing Framework

```rust
// neo-storage/proptest-regressions/trie.txt

# Regression tests generated by proptest
# These correspond directly to Coq lemmas

proptest! {
    #[test]
    fn insert_does_not_remove_other_keys(
        initial_map in map::<u32, u32, _>(),
        new_key in any::<u32>(),
        new_val in any::<u32>()
    ) {
        let mut trie = Trie::from_map(&initial_map);
        let original_len = initial_map.len();
        
        trie.insert(new_key, new_val);
        
        // Corresponds to: insert_preserves_existing_keys theorem
        for (k, v) in &initial_map {
            assert_eq!(trie.get(k), Some(*v));
        }
        
        assert_eq!(trie.get(&new_key), Some(new_val));
        assert_eq!(trie.len(), original_len + 1);
    }
    
    #[test]
    fn delete_nonexistent_unchanged(
        initial_map in map::<u32, u32, _>(),
        missing_key in any::<u32>()
    ) {
        precondition!(!initial_map.contains_key(&missing_key));
        
        let before = Trie::from_map(&initial_map);
        let after = before.delete(&missing_key);
        
        // Corresponds to: delete_noop_if_absent theorem
        prop_assert_eq!(before.to_map(), after.to_map());
    }
}
```

### Continuous Proof Validation CI

```yaml
# .github/workflows/formal-verification.yml
name: Formal Verification Pipeline

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  coq-check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      
      - name: Setup Coq Toolchain
        run: |
          curl https://raw.githubusercontent.com/ocaml/opam/master/shell/install.sh | bash
          eval $(opam init --auto-confirm)
          eval $(opam env)
          opam install coq coq-core
        
      - name: Build All Specifications
        run: make coq-all
        timeout-minutes: 30
        
      - name: Run Proof Scripts
        run: make coq-force
        timeout-minutes: 60
        
  tla-plus:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      
      - name: Install TLA+
        run: sudo apt-get install tla-tools
      
      - name: Model Check Consensus
        run: |
          cd formal/consensus/model
          tlc consensus_spec.tla -prefixes file://tests/prefixes.txt
          
  krilla-sync:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      
      - name: Sync Rust Types to Coq
        run: cargo krilla-export
        
      - name: Verify Coq Files Changed
        run: git diff --stat form*/
```

---

## 📊 Progress Tracking & Metrics

### Daily Verification Report Format

```markdown
## Formal Verification Progress Report - Day 7

### Completed Proofs Today
✅ `insert_preserves_path_uniqueness` (127 lines of Coq)
✅ `delete_collapse_singleton_branches` (89 lines of Coq)
⏳ `hash_consistency_under_rotation` (IN PROGRESS - 60%)

### Proof Statistics
- Total Lemmas Planned: 142
- Implemented: 67
- Verified: 43
- Automation Rate: 68%

### Bottlenecks Identified
🐛 Complex recursive case in `extension_node_splitting` requiring manual tactic tuning
📝 Waiting on TLA+ consensus model to finalize fault tolerance bounds

### Next 24h Focus
1. Complete hash computation correctness proof
2. Begin parallel execution rw-set soundness
3. Integrate new properties into CI pipeline
```

---

## 🔍 Known Challenges & Mitigations

### Challenge 1: Exponential State Space in Consensus
**Mitigation**: Use symbolic execution + abstraction in TLA+  
**Fallback**: Focus on small-scale model checking (n=4, f=1)

### Challenge 2: Cryptographic Assumption Complexity
**Mitigation**: Leverage existing Coq crypto libraries (F Crypto)  
**Assumption**: Blake2b collision resistance (standard assumption)

### Challenge 3: Performance vs. Verification Tradeoffs
**Mitigation**: Prove optimized code refines naive specification  
**Technique**: Refinement typing via Krilla

---

## ✅ Acceptance Criteria & Deliverables

### Phase Completion Gates

| Phase | Deliverable | Success Metric | Owner |
|-------|-------------|----------------|-------|
| Phase 1 | `_coq-project` + CI | `make coq-all` passes | Build Agent |
| Phase 2 | MPT Proofs | 90% function coverage | Proof Agent |
| Phase 3 | Consensus Safety | TLA+ model check clean | Consensus Agent |
| Phase 4 | Optimizations Verified | Zero invariant violations | Perf Agent |

### Final验收标准

✅ **All**.v files compile with Coq 8.19+  
✅ **All** `.tlamodel` files pass TLC model checking  
✅ **100%** of critical path functions have formal specifications  
✅ **≥80%** of lemmas auto-proven with minimal human intervention  
✅ **Zero** known counterexamples from property-based testing  

---

## 📚 Additional Resources for Following Agents

### Learning Materials Provided
- `docs/formal-verification/coq-tutorial.md`: Custom tutorial for Neo-RS patterns
- `docs/formal-verification/tla-basics.md`: Consensus-specific TLA+ guide
- `scripts/auto_proof_templates.sh`: Script to generate proof skeletons

### Communication Protocols
- Weekly sync between proof agents (Slack channel: #formal-proof)
- Shared proof repository at `formal/` directory
- Daily progress updates via GitHub Issues comments

### Emergency Escalation Path
If stuck >24h:
1. Document blocking issue in `docs/proof-blockers/`
2. Tag @formal-verification-lead
3. Request external review from Coq Zulip chat

---

**END OF MASTER PLAN**

*This document serves as the single source of truth for all formal verification work. Any deviations must be documented and approved by the verification lead agent.*
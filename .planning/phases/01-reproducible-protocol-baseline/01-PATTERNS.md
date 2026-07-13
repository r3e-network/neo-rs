# Phase 1: Reproducible Protocol Baseline - Pattern Map

**Mapped:** 2026-07-13
**Mapper:** generic-agent workaround for `gsd-pattern-mapper`
**Files analyzed:** 15 file families (40 concrete files inspected, plus the full dirty-worktree name inventory)
**Analogs found:** 15 / 15 file families

## Worktree Reconciliation

This is a brownfield map. The worktree was already broadly dirty when mapping
started, and those changes belong to the user. Planning must preserve them and
must not reset, regenerate, or normalize unrelated files.

The following Phase 1 behavior is already present in the live worktree and
should be treated as a baseline to verify, not work to reimplement:

- memoized `StackValue -> StackItem` compound reconstruction and adversarial
  alias/conflict tests;
- immutable pre-/post-Domovoi notification projection and behavioral tests;
- complete MainNet/TestNet hardfork boundary tests;
- full `(version, index, root_hash)` state-root vote identity and the missing
  distinct-index test;
- immutable root/fuzz `neo-vm-rs` pins, dependency lock remediation, generic
  bincode helper removal, and the documented cargo-deny exception.

The remaining Phase 1 edits identified by research are concentrated in the CI,
compatibility/fuzz workflows, validator shell handling, repository guards,
active architecture/protocol documentation, and ADR-044. In particular,
`scripts/tests/test_dependency_hygiene.py` and
`scripts/tests/test_protocol_target_docs.py` already exist; extend them rather
than creating replacement test files.

The much broader dirty set under `neo-manifest`, `neo-native-contracts`,
`neo-node`, `neo-payloads`, `neo-rpc`, and other crates is the existing
`neo-vm-rs` v0.2 `StackValue` migration surface. It is part of the locked
workspace verification surface, but research does not assign a fresh rewrite
of those files. Preserve it and let full locked check/test/clippy gates expose
any remaining call-site mismatch.

## File Classification

| New/Modified File Family | Current Phase State | Role | Data Flow | Closest Analog | Match Quality |
|---|---|---|---|---|---|
| `neo-vm/src/stack_item/{stack_item,array,buffer,map,struct_item}.rs` | implemented; verify | model/adapter | recursive graph transform | `neo-execution/src/interop/application_engine_helper.rs:400` | exact algorithm match |
| `neo-vm/src/tests/stack_item/stack_item.rs` | implemented; verify | test | adversarial transform | same file's mutation-based compound tests at line 105 | exact/same-suite |
| `neo-payloads/src/execution/notify_event_args.rs` and its test | implemented; verify | event model | event-driven projection | retained-state pattern in the same type at lines 37-75 | exact/same-module |
| `neo-execution/src/interop/application_engine_helper.rs` and runtime test | implemented; verify | domain service | event-driven graph transform | `StackValueConversion` in `neo-vm/src/stack_item/stack_item.rs:729` | exact algorithm match |
| `neo-config/src/settings/hardfork.rs` and settings tests | implemented; verify | config + test | lookup/table transform | table-driven schedule assertions in `neo-config/src/tests/settings/hardfork.rs:19` | exact |
| `neo-execution/src/application_engine/storage_ops/load_execute_storage.rs` and runtime sentinels | implemented; verify | domain service | state-machine execution | existing local-engine path and divergence test at runtime-test line 596 | exact/same-module |
| `neo-blockchain/src/state_root/{consensus.rs,tests/consensus.rs}` | implemented; verify | service/store + test | event-driven aggregation | competing-root/version tests in the same suite | exact/same-suite |
| root/fuzz manifests and locks, affected crate manifests, `deny.toml` | mostly implemented; add guards/gates | config/policy | dependency graph resolution | root/fuzz `neo-vm-rs` pins and current cargo-deny policy | exact |
| `neo-serialization/{Cargo.toml,src/codec/serialization.rs,src/lib.rs,src/tests/codec/serialization.rs}` | implemented; verify | codec utility | binary/JSON transform | `serialize_neo_binary` / `deserialize_neo_binary` in the same module | exact/same-module |
| `.github/workflows/ci.yml` | remaining | CI config | batch | locked container build in `Dockerfile:67`; existing CI job layout | role-match |
| `.github/workflows/compatibility-v310.yml`, `scripts/validate-v310-consistency.sh` | remaining | CI config + utility | request-response/batch | current validator endpoint probing plus research's verified retry form | role-match; retry has no correct local analog |
| `.github/workflows/fuzz.yml` | remaining | CI config | streaming/batch | standalone `fuzz/Cargo.toml`/`fuzz/Cargo.lock` boundary and current four-job layout | role-match |
| `scripts/tests/{test_dependency_hygiene.py,test_protocol_target_docs.py}` | remaining | repository test | file I/O/transform | `test_architecture_docs.py` and existing tests in both target files | exact |
| `Dockerfile`, `docker-compose.yml` | implemented; final proof remaining | build config | batch/file I/O | current explicit context and `cargo build --locked` | exact/same-file |
| `docs/{architecture.md,protocol-compatibility.md,getting-started.md,operations.md}`, `design.md` | partially remaining | documentation/ADR | file I/O/reference | upstream-authority table in protocol docs and ADR-043 format | exact |

`benches-package/Cargo.toml`, `neo-rpc/Cargo.toml`,
`neo-test-fixtures/Cargo.toml`, and `tests/Cargo.toml` are included in the
dependency-policy family because the live cargo-deny remediation touched their
workspace metadata. Do not broaden that work into crate refactors.

## Pattern Assignments

### Compound `StackValue` bridge (model/adapter, recursive graph transform)

**Targets:** `neo-vm/src/stack_item/stack_item.rs`, compound item types, and
`neo-vm/src/tests/stack_item/stack_item.rs`

**Primary analog:** `neo-vm/src/stack_item/stack_item.rs:729-808` (the now
implemented conversion context). Preserve this shape when resolving any
remaining bridge/compiler issue:

```rust
#[derive(Debug, Default)]
struct StackValueConversion {
    compounds: HashMap<u64, ConvertedCompound>,
}

fn convert(&mut self, value: StackValue) -> VmResult<StackItem> {
    if let Some(id) = stack_value_compound_id(&value) {
        if let Some(existing) = self.compounds.get(&id) {
            if stack_values_structurally_equal(&existing.definition, &value) {
                return Ok(existing.item.clone());
            }
            return Err(VmError::invalid_operation_msg(format!(
                "Conflicting neo-vm-rs compound definitions for id {id}"
            )));
        }
    }
    // Allocate/register an empty compound before recursively converting children.
}
```

**Test analog:** `neo-vm/src/tests/stack_item/stack_item.rs:143-184`. It mutates
one Buffer/Array/Struct/Map occurrence and observes the second occurrence. Copy
that observable-behavior style; equal IDs alone are not alias proof. The
conflicting-content and conflicting-kind cases at lines 187-205 establish the
fail-closed validation pattern.

**Do not copy:** the older per-type `deep_copy` methods as a graph conversion
strategy. They do not provide the one global ID registry required by this
adapter.

### Notification model and hardfork projection (model/service, event-driven transform)

**Targets:** `neo-payloads/src/execution/notify_event_args.rs`, its test,
`neo-execution/src/interop/application_engine_helper.rs`, and
`neo-execution/src/tests/interop/application_engine_runtime.rs`

**Ownership analog:** `neo-payloads/src/execution/notify_event_args.rs:37-75`
constructs and retains one immutable state array in the payload-layer event
type, while the execution layer owns the hardfork policy:

```rust
let state_array = readonly_state_array(&state);
Self { state, state_array, /* event fields */ }

pub fn state_array(&self) -> StackItem {
    self.state_array.clone()
}
```

**Hardfork branch analog:**
`neo-execution/src/interop/application_engine_helper.rs:293-301`:

```rust
let state = if self.is_hardfork_enabled(Hardfork::HfDomovoi) {
    readonly_array_stack_item(clone_notification_state(&notification.state)?)
} else {
    notification.state_array()
};
notification.try_to_stack_item_with_state_array(state)
```

**Graph-copy analog:** the same file at lines 400-480 uses one
`HashMap<CompoundKey, StackItem>`, inserts an empty clone before descending,
converts Buffer to ByteString, and marks every compound read-only after filling
it. Keep error adaptation at the crate boundary with
`map_err(|e| CoreError::other(e.to_string()))`.

**Test analog:** runtime-test lines 300-375 assert that pre-Domovoi calls reuse
the stored state and nested identities; the earlier Domovoi test asserts fresh
outer/nested identities on each call while preserving aliases and immutability.
Tests must assert identity, mutation rejection, and repeated-call behavior.

### Official hardfork schedule (config/test, table transform)

**Targets:** `neo-config/src/settings/hardfork.rs`,
`neo-config/src/tests/settings/hardfork.rs`, and
`neo-config/src/tests/settings/protocol.rs`

**Analog:** `neo-config/src/tests/settings/hardfork.rs:19-60` is the repository's
preferred protocol-constant pattern:

```rust
let expected = [
    (Hardfork::HfAspidochelone, 1_730_000),
    // every scheduled fork in declaration order
    (Hardfork::HfGorgon, 12_020_000),
];
assert_eq!(manager.get_hardforks().len(), expected.len());
for (hardfork, height) in expected {
    assert_eq!(manager.get_hardforks().get(&hardfork), Some(&height));
    assert!(!manager.is_enabled(hardfork, height - 1));
    assert!(manager.is_enabled(hardfork, height));
}
assert!(!manager.is_enabled(Hardfork::HfHuyao, u32::MAX));
```

Keep MainNet and TestNet as separate complete fixtures, cite official
`neo-node` v3.10.1 configuration, test `height - 1` and `height`, and assert the
unscheduled next fork remains absent. Do not infer heights from explorers or a
live tip.

### Canonical execution path (service/test, state-machine execution)

**Targets:**
`neo-execution/src/application_engine/storage_ops/load_execute_storage.rs` and
the runtime sentinels

**Analog:** `load_execute_storage.rs:89-104` keeps host attachment local and
executes only the hardfork-aware stateful engine:

```rust
let attached_here = self.attach_host();
let state = self.vm_engine.engine_mut().execute();
self.detach_host(attached_here);
if state == VMState::FAULT {
    self.capture_fault_exception_from_vm();
}
state
```

Pair the source guard at runtime-test lines 596-608 with the behavioral
zero-shift divergence sentinel at lines 612-644. The source assertion prevents
accidental dispatch; the behavior assertion proves why the unproven engine is
not canonical. Do not delete the behavioral test in favor of source matching.

### State-root vote isolation (service/store, event-driven aggregation)

**Targets:** `neo-blockchain/src/state_root/consensus.rs` and its test

**Analog:** `neo-blockchain/src/state_root/consensus.rs:97-150` defines a typed,
hashable identity and validates before mutating its pool:

```rust
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct StateRootIdentity {
    version: u8,
    index: u32,
    root_hash: UInt256,
}

if !validate_state_root_vote(/* ... */) {
    return None;
}
let entry = self.votes.entry(StateRootIdentity::from(&*state_root)).or_default();
entry.insert(validator_index, signature);
```

**Test analog:** `neo-blockchain/src/state_root/tests/consensus.rs:80-185`
builds two valid competing objects, contributes `M-1` votes to one, and proves
a valid signature for the other cannot complete the threshold. Keep one case
for each identity field (hash, version, index) and a positive control proving
matching votes still aggregate.

### Workspace-wide `StackValue` call sites (adapter/utility, transform)

**Targets:** the already-dirty v0.2 migration family across manifest, payload,
native-contract, node, RPC, serialization, and fuzz code

**Closest analogs:**

- `neo-manifest/src/manifest/support/stack_value_helpers.rs:5-47` destructures
  compound variants with their ID, validates shapes first, and returns typed
  `CoreError::invalid_format` rather than panicking.
- `neo-payloads/src/witness_rule/stack_projection.rs:25-45` allocates every new
  Array with `neo_vm_rs::next_stack_item_id()` and keeps VM projection in the
  owning protocol crate.
- `neo-rpc/src/application_logs/stack_json.rs:24-87` accepts IDs but omits them
  from RPC JSON, propagates `CoreResult`, and charges an explicit output budget
  during recursion.
- `fuzz/fuzz_targets/fuzz_transaction_parse.rs:16-26` sends arbitrary bytes
  through the production `Serializable` path and treats either success or a
  typed error as valid; panics remain failures.

These are verification analogs, not authorization to refactor every migrated
call site during Phase 1.

### Dependency graph and codec policy (config/utility, dependency/file transform)

**Targets:** root/fuzz manifests and locks, affected crate manifests,
`deny.toml`, and the `neo-serialization` helper surface

**Pin analog:** `Cargo.toml:238` and `fuzz/Cargo.toml:19` use the identical Git
URL, full 40-hex revision, version, feature set, and `default-features = false`.
Both locks record that exact source including the commit fragment. Repository
guards should parse TOML with `tomllib`; do not establish pin equality with an
unstructured grep.

```toml
neo-vm-rs = { git = "https://github.com/r3e-network/neo-vm-rs.git", rev = "3081e83db3716fd51dc58c0afc039290d2d07253", version = "0.2.0", default-features = false, features = ["std", "interpreter"] }
```

**Policy analog:** `deny.toml:12-21` keeps a narrow advisory exception with the
byte-compatibility reason and Phase 2 removal tracker; lines 52-55 deny unknown
sources and allow only the pinned VM repository. Preserve the rule that fuzz
must not contain bincode and root bincode must remain only for existing
consensus recovery until the versioned migration exists.

**Codec analog:** `neo-serialization/src/codec/serialization.rs:31-56` routes
consensus binary data through `neo_io::Serializable`, `BinaryWriter`, and
`MemoryReader`, validates empty input, and maps errors into `CoreError`.
Diagnostic JSON helpers remain explicitly non-consensus. Do not restore generic
bincode helpers to this public utility crate.

### Repository guards (test, file-I/O transform)

**Targets:** `scripts/tests/test_dependency_hygiene.py` and
`scripts/tests/test_protocol_target_docs.py`

**Manifest analog:** `test_dependency_hygiene.py:9-19`:

```python
with (REPO_ROOT / "Cargo.toml").open("rb") as handle:
    cargo = tomllib.load(handle)
self.assertNotIn("full", cargo["workspace"]["dependencies"]["hyper"].get("features", []))
```

Use the same structured approach for root/fuzz package metadata and lock
packages. Include assertion messages that state the invariant and operational
reason.

**Cross-file analog:** `test_protocol_target_docs.py:22-34` uses a fixed path
list plus `subTest`; lines 138-185 scan active source trees and report exact
`path:line` stale markers. `test_architecture_docs.py:101-116` derives expected
workspace membership from parsed `Cargo.toml` instead of duplicating a count.

Extend these suites to guard at least:

- identical immutable VM revisions in root/fuzz manifests and locks;
- `--locked` on every Cargo resolution command in Phase 1 workflows;
- pinned Rust, nextest, cargo-deny, nightly, and cargo-fuzz versions;
- compatibility path triggers including `neo-primitives/**`;
- failure-preserving retry structure and fallible `select_rpc` assignments;
- bincode absent from fuzz and scoped in root;
- corrected Gorgon schedule text, immutable Git dependency wording, and
  accepted ADR-044 markers.

### CI, compatibility, and fuzz workflows (config, batch/streaming)

**Targets:** `.github/workflows/{ci.yml,compatibility-v310.yml,fuzz.yml}` and
`scripts/validate-v310-consistency.sh`

**Job-layout analog:** keep the existing checkout, dependency setup, cache,
named command, and artifact-upload ordering. Strengthen commands in place; do
not introduce a second workflow framework.

**Locked-build analog:** `Dockerfile:67` already uses:

```dockerfile
RUN cargo build --release --locked -p neo-node
```

Apply Cargo's native `--locked` contract to metadata, check, Clippy, test,
nextest, doctest, and standalone fuzz metadata/check paths. Use workspace MSRV
`1.89` as the stable toolchain authority. Pin the installed nextest,
`cargo-deny 0.18.9`, dated nightly (`nightly-2025-11-30` from research), and
`cargo-fuzz 0.13.1` rather than floating channels/latest installs.

**Fuzz boundary:** `fuzz/Cargo.toml` is intentionally outside the root
workspace and owns `fuzz/Cargo.lock`. Run locked metadata before fuzzing and
compare the lock checksum before/after each fuzz command so a tool-side
resolution mutation fails the job.

**Validator boundary:** `select_rpc` returns nonzero when no endpoint is found
(`scripts/validate-v310-consistency.sh:191-196`). Under `set -e`, callers that
intend to treat unreachability as data must explicitly absorb that status:

```bash
csharp_rpc="$(select_rpc ...)" || csharp_rpc=""
neogo_rpc="$(select_rpc ...)" || neogo_rpc=""
```

Endpoint unreachability may produce a clearly labeled neutral supplemental
result, but must never be recorded as protocol parity.

### Container proof (build config, batch/file I/O)

**Targets:** `Dockerfile`, `docker-compose.yml`, and retained verification
evidence

**Analog:** `Dockerfile:4,30-67,93-101` pins Rust 1.89, sets one repository
workdir, copies the root lock and every workspace member from `context: .`, and
builds `neo-node --locked`. `docker-compose.yml:1-8` uses only the repository
root context. Preserve the explicit no-sibling boundary.

Final proof is behavioral: build `--pull --no-cache` from a detached clean
worktree anchored to one commit, run the image's `neo-node --version`, and
record commit SHA, exact command, exit status, image ID, and smoke output.
Cached images from before later edits are not evidence.

### Active docs and ADR (documentation, file-I/O/reference)

**Targets:** `docs/architecture.md`, `docs/protocol-compatibility.md`,
`design.md`; verify the already-updated `docs/getting-started.md` and
`docs/operations.md`

**Protocol-authority analog:** `docs/protocol-compatibility.md:31-49` records
the official tag, immutable core/VM commits, audited ranges, upstream PR, and
local coverage in one table. Use the same evidence form for schedule and VM
source claims. Correct the active schedule table at lines 91-102 to Gorgon
MainNet `12,020,000`, TestNet `17,960,000`, with Huyao unscheduled.

**ADR analog:** `design.md:2443-2512` (ADR-043) uses this exact shape:

```markdown
### ADR-044: <decision title>

**Status**: Accepted (implemented)

**Context**: ...

**Decision**:
- ...

**Trade-offs**:
- **Gaining**: ...
- **Cost**: ...
- **Constraint**: ...
- **Reversibility**: ...

**Consequences**:
- ...
```

ADR-044 should record the immutable VM revision, local hardfork-aware engine as
the sole canonical path pending differential proof, and bridge identity/
immutability semantics. Update ADR counts and architecture prose that still
calls `neo-vm-rs` a sibling/path dependency. Reth/Substrate may justify
architecture patterns (`design.md:427-472`) but must not justify Neo consensus
behavior.

## Shared Patterns

### Authority and Ownership

- Neo v3.10.1 official source/configuration is the protocol authority.
- `neo-config` owns activation schedules; `neo-execution` consumes them.
- `neo-vm` owns local object identity and mutability; lean `StackValue` is an
  exchange/projection type, not the owner of host reference semantics.
- `neo-blockchain` owns vote-pool identity; existing crypto/script builders own
  signature and multisig mechanics.
- Reth and Substrate are architecture references only.

### Error Handling and Validation

- Validate before mutating a graph, vote pool, lockfile, or evidence record.
- Rust libraries return crate-standard typed errors (`VmError` or
  `CoreError`) and use `map_err` at crate boundaries; no new panic path.
- Repository scripts use `set -euo pipefail`, but expected nonzero probe results
  must be caught at the assignment/branch where they occur.
- CI/build proof fails closed on lock drift, advisory/source policy failures,
  workflow retry exhaustion, and container build/smoke failures.

### Test Placement

Rust implementation files commonly include adjacent suites with
`#[cfg(test)] #[path = "..."] mod tests;`. Add regressions to those existing
suites. Repository invariants use Python `unittest` under `scripts/tests/` and
derive facts from TOML/current files where possible.

### No Authentication Pattern

This phase adds no user/API authentication surface. Do not add auth middleware
or credentials to protocol validation. External RPC endpoints are public,
supplemental evidence inputs and must be treated as unavailable when they fail
identity/network checks.

## No Exact Analog Found

These subpatterns have no correct implementation to copy from elsewhere in the
repository; use the verified research pattern directly:

| Subpattern | Applies To | Required Form |
|---|---|---|
| Failure-preserving workflow retry | `.github/workflows/compatibility-v310.yml` | Capture `$?` immediately inside `else`; retain the last nonzero status and `exit "$rc"`. The current `rc=$?` after `fi` is the bug, not an analog. |
| Standalone fuzz lock integrity | `.github/workflows/fuzz.yml` | Hash `fuzz/Cargo.lock` before locked metadata/fuzzing, compare afterward, and fail on mutation. No current job does this. |
| Final detached evidence record | Phase verification artifact | Record exact commit/tool versions/commands/exit codes/image ID/smoke output from one detached clean worktree. No current cached image or report is a final-evidence analog. |

Verified retry skeleton from `01-RESEARCH.md`:

```bash
rc=1
for attempt in 1 2 3; do
  if bash scripts/validate-v310-consistency.sh "${args[@]}"; then
    rc=0
    break
  else
    rc=$?
  fi
  echo "attempt $attempt failed (rc=$rc)" >&2
done
exit "$rc"
```

## Metadata

**Analog search scope:** `neo-vm`, `neo-execution`, `neo-payloads`,
`neo-config`, `neo-blockchain`, `neo-serialization`, representative migrated
call sites, `scripts/tests`, workflows, manifests/locks, cargo-deny, container
files, and active architecture/protocol documentation.

**Files scanned:** 40 concrete files plus complete `git status`/diff-name
inventories.

**Pattern extraction date:** 2026-07-13

**Planner directive:** Preserve the already-implemented semantic baseline,
finish only the remaining fail-closed build/documentation work, then run all
focused and full locked gates from the final source tree. Do not claim full
MainNet replay or live-network parity in Phase 1; those are later milestone
gates.

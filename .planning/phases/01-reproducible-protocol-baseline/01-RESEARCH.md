# Phase 1: Reproducible Protocol Baseline - Research

**Researched:** 2026-07-13
**Domain:** Neo N3 v3.10.1 protocol semantics, Rust dependency reproducibility, and consensus identity
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Phase Boundary

[VERIFIED: `.planning/phases/01-reproducible-protocol-baseline/01-CONTEXT.md`]

DATA_K4N8Q2XZ_START
Establish a reproducible Neo N3 v3.10.1 protocol baseline: every build surface
uses one immutable VM revision, official network hardfork schedules are encoded
and regression-tested, canonical execution cannot select an unproven engine,
and state-root vote aggregation cannot cross consensus identities. This phase
also closes the workspace, fuzz, dependency, and container verification gates
needed to trust that baseline.
DATA_K4N8Q2XZ_END

#### Protocol Authority

[VERIFIED: `.planning/phases/01-reproducible-protocol-baseline/01-CONTEXT.md`]

DATA_R9C3V7LM_START
Use the official `neo-project/neo-node` v3.10.1 source and network
configuration as the recorded authority for schedules and protocol behavior.
Retain exact dependency and container verification evidence rather than
claiming reproducibility from source inspection alone.
DATA_R9C3V7LM_END

### the agent's Discretion

[VERIFIED: `.planning/phases/01-reproducible-protocol-baseline/01-CONTEXT.md`]

DATA_T5H2W8PD_START
All implementation choices are at the agent's discretion because this is a
pure infrastructure phase. Neo v3.10.1 and its official network configuration
are the protocol authorities. Reth and Substrate are architecture references,
not sources of Neo consensus behavior. Canonical execution must remain on the
local hardfork-aware VM until differential evidence proves another interpreter
equivalent. Existing user changes in the dirty worktree must be preserved and
reconciled rather than replaced.
DATA_T5H2W8PD_END

### Deferred Ideas (OUT OF SCOPE)

[VERIFIED: `.planning/phases/01-reproducible-protocol-baseline/01-CONTEXT.md`]

DATA_B6J1S4FY_START
Fallible storage boundaries, database identity, coordinated lifecycle,
differential parity, live P2P interoperability, full MainNet replay, and
authenticated checkpoint fast sync belong to Phases 2 through 7.
DATA_B6J1S4FY_END
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| PROTO-01 | Canonical execution uses only hardfork-aware semantics proven against Neo v3.10.1, with every consensus-sensitive external dependency pinned to an immutable revision. | The pinned VM and local execution path are present, but compound graph reconstruction and pre-Domovoi notification behavior need behavioral regressions before the migration is complete. [VERIFIED: `Cargo.toml`, `neo-vm/src/stack_item/stack_item.rs`, official Neo v3.10.1 sources] |
| BUILD-01 | A clean checkout passes locked workspace, fuzz, container, and dependency-policy builds without a sibling repository or undeclared local input. | Root/fuzz pins and Docker isolation are mostly implemented; CI is not consistently locked, the retry shell masks failures, both cargo-deny runs are red, documentation is stale, and final clean-checkout evidence has not been generated from the latest tree. [VERIFIED: current manifests, workflows, Dockerfile, cargo-deny output, and git diff] |
| CONSENSUS-01 | State-root votes aggregate only when version, block index, and root hash all match, with adversarial tests for every competing identity. | The collector now keys the full tuple and tests version/root separation, but there is no explicit competing-index adversarial regression. [VERIFIED: `neo-blockchain/src/state_root/consensus.rs` and its five focused tests] |
</phase_requirements>

## Summary

Phase 1 is a brownfield reconciliation phase, not a greenfield implementation. The dirty worktree already pins `neo-vm-rs` v0.2.0 to revision `3081e83db3716fd51dc58c0afc039290d2d07253` in root and fuzz manifests, encodes the official Gorgon heights, routes canonical execution through the local VM, and keys state-root votes by version/index/hash. Focused tests for the currently implemented paths pass. [VERIFIED: current git diff, Cargo metadata, and focused test runs on 2026-07-13]

The implementation is not ready to close. Repeated compound `StackValue` IDs reconstruct as separate local `Arc` objects, generic conversion drops read-only state, and the existing round-trip test checks IDs rather than alias behavior. That adapter is used by the pre-Domovoi `Runtime.GetNotifications` path, where official Neo requires reuse of the stored immutable state object; from Domovoi onward it requires a new immutable deep copy that preserves internal aliases. [VERIFIED: local conversion code; official `neo` v3.10.1 commit `d10e9cee...`; official `neo-vm` v3.10.1 commit `004cd607...`]

Build proof is also incomplete. Root cargo-deny currently rejects `anyhow 1.0.102`, `crossbeam-epoch 0.9.18`, `bincode 1.3.3`, and the BSL-1.0 license. The fuzz check additionally rejects NCSA, yanked `num-bigint 0.4.7`, and the same bincode advisory. CI commands omit `--locked`, and the compatibility retry captures status zero after a failed `if`. The previously built Docker image predates later dependency/RPC edits, so it is not final evidence. [VERIFIED: commands executed during this research and image creation metadata]

**Primary recommendation:** Complete semantic reconciliation in Plan 01-01, then make every build surface fail closed and generate evidence from a detached clean worktree in Plan 01-02.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Official hardfork schedule | Protocol configuration (`neo-config`) | Execution host | Configuration owns activation heights; execution consumes the selected hardfork. [VERIFIED: workspace layer metadata and current call sites] |
| Compound VM identity | Stateful VM host (`neo-vm`) | Execution interop | The local host owns `Arc`, mutability, reference counting, and object identity; the lean external value only carries IDs and tree snapshots. [VERIFIED: local and pinned VM sources] |
| Canonical engine selection | Execution domain service (`neo-execution`) | Stateful VM host | `ApplicationEngine::execute_allow_fault` is the canonical entry point and the local VM supplies hardfork-aware behavior. [VERIFIED: `load_execute_storage.rs`] |
| State-root vote isolation | Blockchain node service (`neo-blockchain`) | Crypto/payload layers | The collector validates signatures and owns aggregation pools; lower layers provide keys, hashes, and witnesses. [VERIFIED: state-root consensus source] |
| Dependency and container reproducibility | Repository build/CI boundary | Application image | Root/fuzz locks, workflows, and Docker determine the exact source graph used to build `neo-node`. [VERIFIED: manifests and workflows] |
| Protocol evidence | Tests and phase verification | Documentation/ADR | Focused tests establish local invariants; clean locked gates and retained reports establish reproducibility. [VERIFIED: roadmap exit gates] |

## Standard Stack

### Core

| Component | Version / Revision | Purpose | Why Standard |
|-----------|--------------------|---------|--------------|
| Rust | edition 2024; MSRV 1.89 | Workspace compiler contract | Declared once in root and fuzz manifests and used by the Docker builder. [VERIFIED: manifests and Dockerfile] |
| Cargo lockfiles | root lock v4 plus independent `fuzz/Cargo.lock` | Deterministic dependency resolution | The fuzz crate is excluded from the workspace and therefore requires its own tracked lock. [VERIFIED: workspace membership and git index] |
| `neo-vm-rs` | v0.2.0 at `3081e83db3716fd51dc58c0afc039290d2d07253` | Lean VM semantics shared by the stateful host | The immutable Git revision is present in both manifests and both locks; the commit is dated 2026-07-02. [VERIFIED: Cargo metadata, lockfiles, and Git checkout] |
| Official Neo Node | tag v3.10.1 at `7313f8087724e1de4caa88edd2ada58c1fe54abc` | Network configuration authority | The tag contains the MainNet/TestNet operational schedules used by this phase. [VERIFIED: official Git tag and source] |
| Official Neo / Neo.VM | tags v3.10.1 at `d10e9cee...` / `004cd607...` | Protocol and VM semantic authority | These sources define notification copying, reference identity, and hardfork behavior. [VERIFIED: official Git tags and source] |

### Supporting

| Tool / Package | Verified Version | Purpose | When to Use |
|----------------|------------------|---------|-------------|
| `cargo-deny` | 0.18.9 installed | Advisory, yanked crate, source, and license policy | Run against root and fuzz locks after every deliberate lock update. [VERIFIED: local CLI and official cargo-deny docs] |
| `actionlint` | 1.7.10 installed | GitHub Actions structure and embedded shell lint | Run after any workflow edit; supplement it with a behavioral regression for retry status. [VERIFIED: local CLI run] |
| Docker | client/server 29.6.1 installed | Clean-context image proof | Build the final committed tree without a sibling context and smoke-test the resulting binary. [VERIFIED: local Docker CLI/daemon] |
| `anyhow` | 1.0.103, published 2026-06-25 | Remediate RUSTSEC-2026-0190 | Update the root lock to the first fixed release or later compatible locked release. [VERIFIED: RustSec and crates.io] |
| `crossbeam-epoch` | 0.9.20, published 2026-07-06 | Remediate RUSTSEC-2026-0204 | Update the root lock to the first fixed release. [VERIFIED: RustSec and crates.io] |
| `num-bigint` | 0.4.8, published 2026-07-05 | Replace yanked fuzz-lock 0.4.7 within the existing 0.4 range | Update only the standalone fuzz lock unless root resolution also changes deliberately. [VERIFIED: crates.io and current dependency trees] |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Immutable Git revision | Sibling path or branch checkout | Rejected: the build would depend on undeclared mutable filesystem/network state. [VERIFIED: BUILD-01 and removed workflow/Docker code] |
| Local hardfork-aware canonical VM | Automatic external interpreter dispatch | Rejected until Phase 3 differential evidence proves equivalence. [VERIFIED: locked context decision] |
| Cargo `--locked` | A shell check that lockfiles did not change | Use Cargo's native enforcement first; a post-command git diff is only an additional fuzz safeguard. [CITED: https://doc.rust-lang.org/cargo/commands/cargo-check.html] |
| Temporary documented bincode exception | Silent serializer replacement | The exception preserves crash-recovery compatibility until Phase 2 can introduce a versioned format; a silent byte change risks unsafe validator recovery. [VERIFIED: current persistence format and deferred lifecycle scope] |

**Deliberate lock updates (implementation, not research):**

```bash
cargo update -p anyhow --precise 1.0.103
cargo update -p crossbeam-epoch --precise 0.9.20
(cd fuzz && cargo update -p num-bigint --precise 0.4.8)
```

[RECOMMENDATION] Run these only in the dependency task, inspect both lock diffs, then use `--locked` everywhere else.

## Package Legitimacy Audit

The phase adds no new package name. The three registry packages whose locked versions must change were checked because dependency remediation still changes the supply-chain graph. [VERIFIED: manifests and package-legitimacy seam]

| Package | Registry | First Published | Weekly Downloads | Source Repo | Verdict | Disposition |
|---------|----------|-----------------|------------------|-------------|---------|-------------|
| `anyhow` | crates.io | 2019-10-05 | 13,314,664 | `github.com/dtolnay/anyhow` | OK | Approved existing dependency update. [VERIFIED: package-legitimacy check] |
| `crossbeam-epoch` | crates.io | 2017-11-26 | 9,080,902 | `github.com/crossbeam-rs/crossbeam` | OK | Approved existing transitive update. [VERIFIED: package-legitimacy check] |
| `num-bigint` | crates.io | 2016-04-14 | 7,960,932 | `github.com/rust-num/num-bigint` | OK | Approved existing transitive update. [VERIFIED: package-legitimacy check] |

**Packages removed due to SLOP verdict:** none. [VERIFIED: package-legitimacy check]

**Packages flagged as suspicious:** none. [VERIFIED: package-legitimacy check]

## Architecture Patterns

### System Architecture Diagram

```text
official neo-node v3.10.1 configs
             |
             v
 neo-config schedule + boundary tests
             |
             v
 block height -> active hardfork -> local neo-vm jump table
                                      |
 canonical execute_allow_fault --------+
             |
             +--X external interpreter (dormant until differential proof)

StackValue tree + stable compound IDs
             |
             v
 memoized graph reconstruction -> local Arc aliases + explicit mutability policy
             |
             v
 Runtime.GetNotifications -> pre-Domovoi stored immutable state
                          -> Domovoi+ new immutable deep copy

signed StateRoot vote
             |
             v
 signature validation -> pool(version, index, root_hash) -> threshold witness

final source commit
       |
       +-> root Cargo.lock --locked gates
       +-> fuzz Cargo.lock --locked build/fuzz guard
       +-> Docker clean context, no sibling input
       +-> retained command/image evidence
```

[VERIFIED: current architecture and official v3.10.1 notification flow]

### Recommended Project Structure

```text
neo-vm/                  # identity-aware StackValue/StackItem bridge and focused tests
neo-execution/           # hardfork-aware canonical engine and notification projection
neo-config/              # official network schedules and boundary regressions
neo-blockchain/          # full-identity state-root vote pools
scripts/tests/           # structured repository/workflow/documentation guards
.github/workflows/       # locked CI, compatibility, and fuzz entry points
fuzz/                    # independently locked fuzz package
design.md                # ADR-044 and architecture authority
```

[RECOMMENDATION] Keep the existing crate ownership. This phase needs semantic fixes and guards, not crate movement.

### Component Responsibilities

| Component | Files | Required Phase Work |
|-----------|-------|---------------------|
| External-to-local stack adapter | `neo-vm/src/stack_item/stack_item.rs`, compound types, tests | Reconstruct repeated IDs through one memo table; reject type/shape conflicts; test observable alias mutation, not only ID equality. [VERIFIED: current recursive constructor] |
| Notification projection | `neo-execution/src/interop/application_engine_helper.rs`, `neo-payloads/src/execution/notify_event_args.rs`, runtime tests | Store/reuse one immutable local state array before Domovoi; deep-copy immutable after Domovoi; do not round-trip a local graph through a format that cannot carry read-only state. [VERIFIED: local and official source comparison] |
| Hardfork schedule | `neo-config/src/settings/hardfork.rs`, settings tests | Keep official heights and make every scheduled activation boundary table-driven; keep Huyao absent. [VERIFIED: official configs] |
| Canonical execution | `neo-execution/src/application_engine/storage_ops/load_execute_storage.rs`, execution tests | Keep the local engine as the only canonical route and retain a behavioral divergence sentinel. [VERIFIED: current implementation] |
| State-root collector | `neo-blockchain/src/state_root/consensus.rs`, tests | Retain full identity key and add an explicit distinct-index adversarial test. [VERIFIED: current tests] |
| Reproducibility | root/fuzz manifests and locks, workflows, Dockerfile, compose | Enforce the same revision and locked resolution; fix retry status; prove a clean build. [VERIFIED: current diff] |
| Documentation | `docs/architecture.md`, `docs/protocol-compatibility.md`, `design.md`, doc tests | Remove sibling/path and unscheduled-Gorgon claims; add an accepted ADR for the immutable VM/canonical engine decision. [VERIFIED: current documentation search] |

### Pattern 1: Identity-Aware Graph Reconstruction

**What:** Convert a tree-shaped `StackValue` into the local object graph using one conversion context keyed by globally unique compound ID. Insert an empty local compound before converting children, and return an `Arc` clone on repeated IDs. [RECOMMENDATION]

**When to use:** Every `StackValue -> StackItem` conversion that can receive repeated Array, Struct, Map, or Buffer IDs. [VERIFIED: pinned `StackValue` identity contract]

**Required invariants:** A repeated ID has one compound kind, one logical object, consistent content, and observable shared mutation where mutable. Read-only state is a separate policy because `StackValue` carries no read-only bit. [VERIFIED: pinned enum shape and local compound implementations]

### Pattern 2: Protocol Source as Fixture Authority

**What:** Encode the complete official schedule in one table and test every `height - 1` / `height` boundary for MainNet and TestNet. Record tag and commit beside the test/documentation. [RECOMMENDATION]

**When to use:** Any protocol constant sourced from Neo configuration rather than inferred from node behavior. [VERIFIED: official configuration structure]

### Pattern 3: Fail-Closed Build Proof

**What:** Permit lock mutation only in an explicit dependency-update task. All CI, local verification, compatibility builds, and Docker builds consume committed locks and fail if Cargo would change them. [RECOMMENDATION]

**When to use:** Every build after the lock-update task. [CITED: https://doc.rust-lang.org/cargo/commands/cargo-check.html]

### Recommended Plan Split

1. **01-01 semantic correctness:** compound alias reconstruction; exact pre/post-Domovoi notification behavior; complete hardfork boundaries; canonical-engine sentinels; complete state-root identity tests. [RECOMMENDATION]
2. **01-02 reproducible proof:** dependency policy; locked CI/fuzz; retry failure propagation; Docker clean build; stale docs and ADR; final clean-worktree evidence. [RECOMMENDATION]

### Anti-Patterns to Avoid

- **ID equality as alias proof:** Equal IDs on separate `Arc` allocations do not propagate mutation or read-only flags. [VERIFIED: current constructors]
- **Round-tripping local state to copy it:** The external value does not encode local reference counters or read-only state. [VERIFIED: pinned `StackValue` enum]
- **Source-text-only canonical-engine proof:** Retain the source guard, but pair it with a script whose result differs on the unproven engine. [VERIFIED: current zero-shift regression]
- **Neutral compatibility as parity evidence:** An unreachable reference endpoint is infrastructure information, not a successful comparison. [VERIFIED: validator script behavior]
- **Running unlocked after updating a lock:** It can conceal an incomplete or inconsistent lock update. [CITED: https://doc.rust-lang.org/cargo/commands/cargo-check.html]

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Neo activation schedule | Heights inferred from explorers or current tip | Official `neo-node` v3.10.1 config files | Consensus replay requires the historical configured heights. [VERIFIED: locked authority] |
| Neo object-copy semantics | Structural clone that ignores aliases | Memoized graph copy matching official Neo.VM `DeepCopy` | Official copying preserves repeated references and applies immutability. [VERIFIED: official Neo.VM v3.10.1] |
| Lock enforcement | Grep/checksum wrapper | Cargo `--locked` plus tracked locks | Cargo owns dependency resolution semantics. [CITED: official Cargo docs] |
| Advisory/license scanning | Custom registry scripts | `cargo-deny` | Existing policy already covers advisories, yanks, licenses, bans, and sources. [VERIFIED: `deny.toml`] |
| Workflow validation | YAML parsing with string splitting | `actionlint` plus one focused semantic regression | Syntax tools cannot infer the retry-status bug, so both layers are needed. [VERIFIED: actionlint passes the currently buggy workflow] |
| State-root cryptography | New signature or multisig code | Existing `Secp256r1Crypto` and `RedeemScript` | The defect is aggregation identity, not cryptographic primitives. [VERIFIED: collector source] |

**Key insight:** Phase 1 should reduce unproven translation and build paths; it should not add a second protocol authority or another build mechanism. [RECOMMENDATION]

## Current Implementation Reconciliation

| Area | Already Present | Remaining Before Close |
|------|-----------------|------------------------|
| VM revision | Root and fuzz manifests/locks resolve the same v0.2.0 Git commit; sibling clones were removed. [VERIFIED: manifests, locks, workflow diff] | Add structured regression that asserts all surfaces share the exact revision and all build commands are locked. [RECOMMENDATION] |
| Compound migration | All workspace/fuzz call sites use compound IDs; a nested round-trip test passes. [VERIFIED: diff and focused test] | Preserve actual aliases, define mutability limits, and add repeated-ID behavioral tests. [RECOMMENDATION] |
| Canonical VM | `execute_allow_fault` invokes the local engine; zero-shift and source guard tests exist. [VERIFIED: source and tests] | Keep external dispatch unreachable and include both behavioral sentinels in the phase gate. [RECOMMENDATION] |
| Gorgon | MainNet 12,020,000 and TestNet 17,960,000 are encoded; Huyao is absent. [VERIFIED: official and local config] | Test every fork boundary and update docs that still say Gorgon is unscheduled. [RECOMMENDATION] |
| State-root votes | Pool key is `(version, index, root_hash)`; root/version tests pass. [VERIFIED: source and five tests] | Add the missing competing-index test. [RECOMMENDATION] |
| CI/fuzz | Paths and sibling checkout removal are corrected. [VERIFIED: workflow diff] | Add lock enforcement; fix retry status; pin or explicitly record tool versions; ensure fuzz cannot modify its lock unnoticed. [RECOMMENDATION] |
| Dependency policy | Non-yanked `bitcoin_hashes 0.14.0`, `lru 0.16.4`, and missing workspace licenses are already present. [VERIFIED: locks/manifests] | Remediate current root/fuzz failures and document the temporary bincode exception. [RECOMMENDATION] |
| Docker | Rust 1.89, all workspace crates, `unzip`, and no sibling build context are present; an earlier image built. [VERIFIED: Dockerfile, compose, image metadata] | Rebuild `--no-cache` from the final clean tree and smoke-test exact image ID. [RECOMMENDATION] |
| Documentation | Getting-started/operations edits remove some sibling assumptions. [VERIFIED: diff] | Fix `docs/architecture.md`, Gorgon table, the stale `design.md` sibling wording, and add ADR-044. [RECOMMENDATION] |

## Runtime State Inventory

| Category | Items Found | Action Required |
|----------|-------------|-----------------|
| Stored data | Consensus crash-recovery files are unversioned bincode 1.3.3 bytes. VM compound IDs are runtime identity and are omitted from ordinary serialized `StackValue` wire form. No Phase 1 database schema change was found. [VERIFIED: consensus persistence and pinned StackValue serde] | Do not silently replace the recovery format. Keep a narrowly documented temporary advisory exception in Phase 1 and move a versioned reader/writer migration to Phase 2 lifecycle work. [RECOMMENDATION] |
| Live service config | Built-in MainNet/TestNet schedules change after rebuilding the binary; no external UI-managed hardfork configuration was found. [VERIFIED: config loading and repo search] | Operators must restart onto the corrected binary; explicit loaded configs continue to be parsed by `ProtocolSettings`. [RECOMMENDATION] |
| OS-registered state | No repository-owned systemd, launchd, Task Scheduler, or pm2 registration embeds the VM sibling path or old Gorgon schedule. [VERIFIED: repository search] | None. |
| Secrets/env vars | No secret or environment variable is renamed by this phase. Compatibility endpoint override variables remain unchanged. [VERIFIED: workflow/script diff] | None. |
| Build artifacts / installed packages | Root/fuzz targets, Cargo Git cache, and `neo-rs:verification` can contain pre-final code; local Rust 1.89 is not installed even though Docker supplies it. [VERIFIED: environment audit and image timestamp] | Treat caches as non-evidence; verify in a detached clean worktree and rebuild a uniquely tagged image. [RECOMMENDATION] |

## Common Pitfalls

### Pitfall 1: Repeated IDs Without Shared Storage

**What goes wrong:** Two reconstructed arrays compare as the same reference by ID but mutating one does not change the other. [VERIFIED: current recursive conversion]

**Why it happens:** Every recursive occurrence calls a fresh `new_untracked_with_id`, so IDs survive while `Arc<Mutex<_>>` ownership does not. [VERIFIED: local constructors]

**How to avoid:** Use a conversion context and add a test that mutates one occurrence and observes the second. [RECOMMENDATION]

**Warning signs:** Tests assert only `id()` or round-trip enum fields. [VERIFIED: current regression]

### Pitfall 2: Wrong Notification Semantics Before Domovoi

**What goes wrong:** The projected state can become mutable, lose internal aliasing, or receive fresh identity when official Neo returns the stored immutable state. [VERIFIED: local/official source comparison]

**Why it happens:** The pre-Domovoi branch converts local state through `StackValue`, which cannot represent read-only flags and currently reconstructs each occurrence independently. [VERIFIED: local branch and enum]

**How to avoid:** Store one immutable local state array on the notification; return its `Arc` clone before Domovoi and perform a memoized immutable deep copy from Domovoi onward. [RECOMMENDATION]

**Warning signs:** A source-text test says the adapter is used but no script/test asserts alias, mutability, and identity across calls. [VERIFIED: current tests]

### Pitfall 3: Retry Failure Becomes Success

**What goes wrong:** After `if bash validator; then ...; fi` fails, assigning `rc=$?` records the status of the completed `if` statement, which is zero when no branch ran. [VERIFIED: Bash 5.2 reproduction and workflow lines 125-136]

**Why it happens:** Status is captured outside the `else` branch. [VERIFIED: current workflow]

**How to avoid:** Capture `$?` immediately inside `else`, retain the last nonzero status, and add a regression using an always-failing fake command. [RECOMMENDATION]

**Warning signs:** Logs say an attempt failed with `rc=0`, or all attempts fail but the step is green. [VERIFIED: shell semantics]

### Pitfall 4: Cargo-Deny Passes Only in One Graph

**What goes wrong:** The workspace graph can pass while the standalone fuzz lock remains yanked or license-incompatible. [VERIFIED: independent lockfiles and current failures]

**Why it happens:** `fuzz` is excluded from the workspace and resolves transitive versions independently. [VERIFIED: root workspace configuration]

**How to avoid:** Run the same deny configuration from both directories, allow BSL-1.0 and NCSA only after policy review, update fuzz `num-bigint`, and make the bincode exception explicit and scoped. [RECOMMENDATION]

**Warning signs:** Root and fuzz contain different `xxhash-rust` or `num-bigint` versions and only one deny invocation is retained. [VERIFIED: current locks]

### Pitfall 5: Stale Evidence

**What goes wrong:** A green test/image from before later lock, RPC, or documentation edits is reported as final. [VERIFIED: session chronology]

**Why it happens:** Large dirty-worktree changes outlive the command logs that originally validated them. [VERIFIED: current worktree state]

**How to avoid:** Record commit SHA, tool versions, exact commands, exit codes, Docker image ID, and smoke output from one final detached worktree. [RECOMMENDATION]

### Pitfall 6: Partial Boundary Coverage

**What goes wrong:** Correct configured heights can still have an off-by-one activation defect for an untested fork. [VERIFIED: current tests omit Cockatrice/Domovoi boundary assertions]

**Why it happens:** Handwritten assertions cover selected forks rather than iterating the entire official schedule. [VERIFIED: hardfork tests]

**How to avoid:** Table-drive all seven scheduled forks for both networks and assert Huyao remains absent. [RECOMMENDATION]

## Code Examples

### Correct Retry Status Capture

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

[VERIFIED: Bash 5.2 status behavior; recommended correction for current workflow]

### Locked Phase Gate

```bash
for pkg in $(cargo metadata --locked --no-deps --format-version 1 | jq -r '.packages[].name'); do
  cargo fmt --check --package "$pkg"
done
cargo check --workspace --tests --locked
cargo test --workspace --locked
cargo test --workspace --doc --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
(cd fuzz && cargo fmt --check && cargo check --locked --all-targets)
cargo deny check advisories licenses --hide-inclusion-graph
(cd fuzz && cargo deny check advisories licenses --hide-inclusion-graph)
actionlint -no-color
find scripts -type f -name '*.sh' -print0 | xargs -0 -n1 bash -n
```

[RECOMMENDATION] Run this only after focused tests and dependency remediation pass; run it again from the final detached worktree.

### Clean Docker Evidence

```bash
sha=$(git rev-parse --short=12 HEAD)
docker build --pull --no-cache -t "neo-rs:phase1-$sha" .
docker run --rm --entrypoint neo-node "neo-rs:phase1-$sha" --version
docker image inspect "neo-rs:phase1-$sha" --format '{{.Id}}'
```

[RECOMMENDATION] Execute in the detached verification worktree so the context contains only final tracked source.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Mutable sibling/path VM checkout | v0.2.0 immutable Git revision in root and fuzz | Current Phase 1 worktree | Removes an undeclared source input. [VERIFIED: git diff] |
| Automatic external VM fast path | Local hardfork-aware canonical engine | Current Phase 1 worktree | Removes known semantic divergence from canonical execution. [VERIFIED: execution diff] |
| State-root pool keyed by height | Pool keyed by version/index/root hash | Current Phase 1 worktree | Prevents competing identities from sharing a threshold. [VERIFIED: collector diff] |
| Gorgon omitted from operational presets | Official MainNet/TestNet activation heights | Neo Node v3.10.1 tag dated 2026-07-09; local Phase 1 worktree | Historical replay selects the Gorgon rules at official boundaries. [VERIFIED: official tag/config] |
| Best-effort build resolution | Locked root/fuzz/container proof | Required to finish Phase 1 | Makes lock drift a build failure rather than an implicit update. [VERIFIED: BUILD-01] |

**Deprecated/outdated:**

- Sibling `../neo-vm-rs` setup instructions are outdated and must be removed from active architecture/design docs. [VERIFIED: current manifest and docs search]
- The active protocol document's claim that Gorgon is unscheduled is outdated. [VERIFIED: official v3.10.1 configs]
- An unqualified claim of complete/live/state-root parity is not supported by Phase 1 evidence; full differential and MainNet replay proof remain later milestone gates. [VERIFIED: roadmap and current retained evidence]

## Assumptions Log

All implementation-relevant claims were verified against the current codebase, local command output, official immutable Neo tags, official Cargo/cargo-deny documentation, or the registry. No `[ASSUMED]` claim is used. [VERIFIED: source inventory below]

## Open Questions

1. **Where should the temporary bincode exception be tracked?**
   - What we know: `bincode 1.3.3` is unmaintained, generic `neo-serialization` helpers use it only in their own module/tests, and consensus recovery files depend on its existing bytes. [VERIFIED: usage search]
   - What's unclear: There is no issue identifier recorded in `deny.toml`; its policy requires written justification and tracking. [VERIFIED: `deny.toml`]
   - Recommendation: Remove bincode from the unused generic helper surface now, document a root-only RUSTSEC-2025-0141 exception with Phase 2 plan 02-03 as its removal tracker, and migrate recovery bytes under an explicit version in Phase 2. [RECOMMENDATION]

2. **Are live reference endpoints available for final supplemental validation?**
   - What we know: The validator can use configurable C#/NeoGo candidates and exits neutral when none is reachable. [VERIFIED: validator script]
   - What's unclear: Endpoint reachability is time-dependent and was not used as a Phase 1 prerequisite during research.
   - Recommendation: Run MainNet and TestNet validation when endpoints are reachable, retain artifacts, and never count a neutral/unreachable run as protocol parity proof. [RECOMMENDATION]

3. **What commit will be the evidence anchor?**
   - What we know: The worktree contains extensive pre-existing edits, and final evidence must describe one exact source tree. [VERIFIED: git status]
   - What's unclear: The phase implementation commits do not exist yet.
   - Recommendation: Generate the final evidence only after both phase plans commit their scoped work, using the resulting detached commit. [RECOMMENDATION]

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|-------------|-----------|---------|----------|
| Rust/Cargo | Workspace gates | Yes | 1.96.0 active | Docker provides Rust 1.89 for MSRV-aligned build. [VERIFIED: local tools/Dockerfile] |
| Rust 1.89 local toolchain | Exact local MSRV test | No | - | Use Docker now or install through rustup during implementation. [VERIFIED: rustup list] |
| Rust nightly | Fuzz execution | Yes | installed rolling nightly | Pin a dated nightly in CI for reproducible fuzz runs. [VERIFIED: rustup list/workflow] |
| `cargo-fuzz` | Fuzz smoke/long runs | Yes | 0.13.1 | CI install must pin this version instead of latest. [VERIFIED: local CLI/workflow] |
| `cargo-nextest` | Current CI test runner | No | - | Use `cargo test --workspace --locked` locally; CI installs nextest. [VERIFIED: local CLI/workflow] |
| `cargo-deny` | Supply-chain gate | Yes | 0.18.9 | None. [VERIFIED: local CLI] |
| Docker daemon | Container proof | Yes | 29.6.1 | CI Buildx if local daemon fails. [VERIFIED: local CLI/daemon] |
| `actionlint` | Workflow validation | Yes | 1.7.10 | None. [VERIFIED: local CLI] |
| ShellCheck | Embedded workflow shell lint | Yes | 0.9.0 | `actionlint` invokes it; `bash -n` remains syntax fallback. [VERIFIED: local tools] |
| Bash | Scripts/retry proof | Yes | 5.2.21 | None. [VERIFIED: local CLI] |
| Python | Repository architecture/protocol tests | Yes | 3.12.3 | None. [VERIFIED: local CLI] |
| jq | Workflow metadata and validation | Yes | 1.7 | None. [VERIFIED: local CLI] |
| dotnet | Optional C# reference tooling | Yes | 10.0.100 | Live official RPC references. [VERIFIED: local CLI] |
| Official reference RPCs | Supplemental compatibility run | Unknown | time-dependent | Run when reachable; retain an explicit unavailable result without treating it as success. [VERIFIED: script behavior] |

**Missing dependencies with no fallback:** none for planning or core local gates. [VERIFIED: environment audit]

**Missing dependencies with fallback:** local Rust 1.89 and cargo-nextest. [VERIFIED: environment audit]

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | Rust built-in test harness via Cargo 1.96 locally; Python `unittest` for repository guards |
| Config file | Root/fuzz Cargo manifests and locks; no separate Rust test config |
| Quick run command | `cargo test --locked -p neo-vm stack_value_round_trip_preserves_compound_identity && cargo test --locked -p neo-blockchain state_root::consensus::tests` |
| Full suite command | `cargo test --workspace --locked` plus the complete locked phase gate below |

[VERIFIED: detected test infrastructure]

### Phase Requirements -> Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| PROTO-01 | Repeated VM compound identity behaves as one object | unit | `cargo test --locked -p neo-vm stack_value` | Partial; Wave 0 alias test missing. [VERIFIED: current test] |
| PROTO-01 | Pre-/post-Domovoi notifications preserve official identity and immutability | integration | `cargo test --locked -p neo-execution get_notifications` | Partial; pre-Domovoi behavioral test missing. [VERIFIED: current tests] |
| PROTO-01 | Canonical engine returns local hardfork-aware semantics | integration | `cargo test --locked -p neo-execution canonical_execution_does_not_dispatch_to_external_interpreter && cargo test --locked -p neo-execution zero_shift_coerces_boolean_result_to_integer_like_neo_vm_v3101` | Existing tests. [VERIFIED: current tests] |
| PROTO-01 | Every MainNet/TestNet hardfork boundary matches official v3.10.1 | unit | `cargo test --locked -p neo-config hardfork` | Partial; selected boundaries only. [VERIFIED: current tests] |
| BUILD-01 | Root/fuzz dependency pins and locks are identical where required | repository test | `python3 -m unittest scripts.tests.test_dependency_hygiene` | Existing framework; new cases missing. |
| BUILD-01 | Workflows parse and failures propagate | lint + repository test | `actionlint -no-color && python3 -m unittest scripts.tests.test_protocol_target_docs` | Lint/tests exist; retry regression missing. |
| BUILD-01 | Both dependency graphs satisfy policy | supply-chain integration | root and fuzz `cargo deny check advisories licenses --hide-inclusion-graph` | Commands exist; currently fail. [VERIFIED: current output] |
| BUILD-01 | Standalone fuzz graph resolves without lock mutation | build | `(cd fuzz && cargo fmt --check && cargo check --locked --all-targets)` | Existing; currently passes. [VERIFIED: research run] |
| BUILD-01 | Final container builds without sibling input | clean-context smoke | `docker build --pull --no-cache ... && docker run ... neo-node --version` | Dockerfile exists; final evidence missing. |
| CONSENSUS-01 | Version, index, and root hash each isolate votes | unit/adversarial | `cargo test --locked -p neo-blockchain state_root::consensus::tests` | Version/root exist; index case missing. [VERIFIED: five tests] |

### Sampling Rate

- **Per task commit:** Run the focused crate test plus its repository guard. [RECOMMENDATION]
- **Per wave merge:** `cargo check --workspace --tests --locked`, affected crate tests, fuzz locked check when locks/code affect fuzz, and both cargo-deny commands after dependency changes. [RECOMMENDATION]
- **Phase gate:** Run format, locked workspace check, full tests/doctests, full Clippy with `-D warnings`, fuzz format/check, root/fuzz cargo-deny, actionlint, all Bash syntax checks, clean Docker build/smoke, and optional reachable-reference validation. [RECOMMENDATION]

### Wave 0 Gaps

- [ ] Extend `neo-vm/src/tests/stack_item/stack_item.rs` with a repeated-ID mutation/alias regression; the current test checks IDs only. [VERIFIED: current test]
- [ ] Extend `neo-execution/src/tests/interop/application_engine_runtime.rs` with pre-Domovoi repeated alias, read-only, and repeated-call identity behavior. [VERIFIED: official semantics/current gap]
- [ ] Add a distinct block-index vote isolation test to `neo-blockchain/src/state_root/tests/consensus.rs`. [VERIFIED: current test inventory]
- [ ] Replace selected hardfork assertions with complete table-driven boundary coverage in `neo-config/src/tests/settings/hardfork.rs`. [VERIFIED: current test inventory]
- [ ] Add structured root/fuzz pin, lock, workflow retry, and active-doc assertions under `scripts/tests/`. [RECOMMENDATION]
- [ ] Add a bincode exception/removal regression: fuzz must no longer resolve bincode; root may resolve it only through consensus while the documented exception is active. [RECOMMENDATION]

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | No | This phase adds no user authentication surface. [VERIFIED: phase scope] |
| V3 Session Management | No | This phase adds no browser/API session state. [VERIFIED: phase scope] |
| V4 Access Control | No | Runtime authorization is unchanged; build provenance is enforced through source/lock policy instead. [VERIFIED: phase diff] |
| V5 Input Validation | Yes | Reject conflicting compound IDs, validate workflow inputs, use locked dependency resolution, and fail on wrong vote identity. [RECOMMENDATION] |
| V6 Cryptography | Yes | Reuse existing secp256r1 verification and canonical multisig scripts; pin consensus-sensitive sources by immutable revision. [VERIFIED: collector and manifests] |

### Known Threat Patterns for Rust Node Build/Consensus

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Mutable/path dependency substitution | Spoofing / Tampering | Immutable Git revision in both manifests and locks; no sibling context. [VERIFIED: current remediation] |
| Lockfile drift during CI | Tampering | `--locked` on every resolution path and post-fuzz lock diff. [RECOMMENDATION] |
| Workflow failure-status masking | Repudiation / Availability | Capture status inside `else`, test all-fail behavior, retain logs. [RECOMMENDATION] |
| Compound ID type/shape conflict | Tampering / Denial of Service | One memoized ID registry; reject repeated ID with a different compound kind or inconsistent definition. [RECOMMENDATION] |
| Cross-root vote aggregation | Spoofing / Tampering | Signature validation and full `(version,index,root_hash)` pool key with adversarial cases. [VERIFIED: current design] |
| Unmaintained/yanked dependency | Supply-chain integrity | Update vulnerable/yanked releases, explicit license policy, narrowly documented temporary informational exception. [RECOMMENDATION] |
| Unreachable reference reported as parity | Repudiation | Record as unavailable/neutral and exclude from success evidence. [RECOMMENDATION] |

## Sources

### Primary (HIGH confidence)

- Current neo-rs source, tests, workflows, manifests, locks, Dockerfile, documentation, and git diff - implementation and gap inventory. [VERIFIED: local inspection]
- https://github.com/neo-project/neo-node/tree/v3.10.1/src/Neo.CLI - MainNet/TestNet configs; tag resolved to `7313f8087724e1de4caa88edd2ada58c1fe54abc`. [VERIFIED: Git ls-remote and shallow tag checkout]
- https://github.com/neo-project/neo/blob/v3.10.1/src/Neo/SmartContract/ApplicationEngine.Runtime.cs - immutable notification capture and lookup flow. [VERIFIED: tag `d10e9ceecdabe3fcff719ee68ea5b76ba7e62c3d`]
- https://github.com/neo-project/neo/blob/v3.10.1/src/Neo/SmartContract/NotifyEventArgs.cs - pre-/post-Domovoi state projection. [VERIFIED: official tag source]
- https://github.com/neo-project/neo-vm/blob/v3.10.1/src/Neo.VM/Types/Array.cs - memoized deep-copy alias and read-only behavior. [VERIFIED: tag `004cd6070a940405818d9357638277dd44407e2e`]
- Pinned `r3e-network/neo-vm-rs` revision `3081e83db3716fd51dc58c0afc039290d2d07253` - `StackValue` ID contract and retained alias machinery. [VERIFIED: Cargo checkout and Git revision]
- Local command evidence: focused tests, root/fuzz cargo-deny, actionlint, Bash syntax, locked fuzz check, tool versions, and Docker image metadata. [VERIFIED: research session 2026-07-13]

### Secondary (MEDIUM confidence)

- https://doc.rust-lang.org/cargo/commands/cargo-check.html and https://doc.rust-lang.org/cargo/commands/cargo-test.html - `--locked` behavior. [CITED: official Cargo docs]
- https://embarkstudios.github.io/cargo-deny/checks/advisories/cfg.html and https://embarkstudios.github.io/cargo-deny/checks/licenses/cfg.html - advisory/license policy. [CITED: official cargo-deny docs]
- https://rustsec.org/advisories/RUSTSEC-2025-0141 - bincode is unmaintained with no patched release. [CITED: RustSec advisory]
- crates.io records for `anyhow 1.0.103`, `crossbeam-epoch 0.9.20`, and `num-bigint 0.4.8` - version publication/yank state. [VERIFIED: crates.io API and cargo search/info]

### Tertiary (LOW confidence)

- None used for implementation decisions.

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH - exact manifests, locks, registry records, and tool versions were inspected.
- Architecture: HIGH - current call paths were traced and compared with immutable official Neo v3.10.1 sources.
- Pitfalls: HIGH - every named audit lead was reproduced or demonstrated directly.
- Environment: HIGH for installed tools; endpoint reachability intentionally remains unknown until execution.

**What might have been missed:** A downstream consumer outside this repository could call the public generic bincode helpers or depend on current `NotifyEventArgs.state` shape. The planner should use compiler errors and full workspace tests to enumerate in-repo consumers, document any public API break, and avoid claiming ecosystem-wide compatibility without a release audit. [VERIFIED: repository-only search boundary]

**Research date:** 2026-07-13
**Valid until:** 2026-08-12 for code architecture; recheck advisories and registry versions immediately before dependency changes.

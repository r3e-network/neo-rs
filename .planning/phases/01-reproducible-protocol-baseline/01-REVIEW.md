---
phase: 01-reproducible-protocol-baseline
reviewed: 2026-07-13T18:15:42Z
depth: standard
files_reviewed: 37
files_reviewed_list:
  - .github/workflows/ci.yml
  - .github/workflows/compatibility-v310.yml
  - .github/workflows/fuzz.yml
  - Cargo.toml
  - fuzz/Cargo.toml
  - deny.toml
  - Dockerfile
  - docker-compose.yml
  - scripts/tests/file_size_policy.py
  - scripts/tests/test_core_file_size_limits.py
  - scripts/tests/test_file_size_catalog_limits.py
  - scripts/tests/test_file_size_limits.py
  - scripts/tests/test_node_file_size_limits.py
  - scripts/tests/test_repository_file_size_limits.py
  - scripts/tests/test_rpc_file_size_limits.py
  - scripts/tests/test_dependency_hygiene.py
  - scripts/tests/test_protocol_target_docs.py
  - scripts/validate-v310-consistency.sh
  - docs/protocol-compatibility.md
  - docs/architecture.md
  - design.md
  - neo-vm/src/stack_item/stack_item.rs
  - neo-vm/src/tests/stack_item/stack_item.rs
  - neo-payloads/src/execution/notify_event_args.rs
  - neo-payloads/src/tests/execution/notify_event_args.rs
  - neo-execution/src/interop/application_engine_helper.rs
  - neo-execution/src/tests/interop/application_engine_runtime.rs
  - neo-config/src/settings/hardfork.rs
  - neo-config/src/tests/settings/hardfork.rs
  - neo-config/src/tests/settings/protocol.rs
  - neo-execution/src/application_engine/storage_ops/load_execute_storage.rs
  - neo-execution/src/application_engine/external_vm.rs
  - neo-execution/src/application_engine/mod.rs
  - neo-blockchain/src/state_root/consensus.rs
  - neo-blockchain/src/state_root/tests/consensus.rs
  - neo-node/src/node/fast_sync/package/manifest.rs
  - neo-node/src/tests/node/fast_sync/package/manifest.rs
findings:
  critical: 4
  warning: 8
  info: 1
  total: 13
status: issues_found
---

# Phase 1: Code Review Report

**Reviewed:** 2026-07-13T18:15:42Z
**Depth:** standard
**Files Reviewed:** 37
**Status:** issues_found

## Narrative Findings (AI reviewer)

## Summary

The protocol-semantic changes pass their focused suites, the reviewed
MainNet/TestNet hardfork tables match the official Neo v3.10.1 boundaries, and
the retained container evidence matches the recorded source commit. However,
four release blockers remain: fast-sync HTTPS can be downgraded by redirects,
the pinned fuzz action does not exist, the compatibility oracle can report an
unevaluated run as successful, and dependency-policy resolution is not locked.
Eight additional robustness and reproducibility defects remain across
consensus context binding, notification ownership, external-result graph
conversion, network bounds, dependency bans, action pinning, comparison-tool
identity, and CI coverage.

## Critical Issues

### CR-01: HTTPS-only package validation permits cleartext redirects

**File:** `neo-node/src/node/fast_sync/package/manifest.rs:36`

**Issue:** Both the official manifest request here and the package request in
the adjacent cache path use `reqwest::get`. Reqwest 0.12.28 follows up to ten
redirects by default with `https_only = false`. The initial URL check at lines
95-102 therefore does not prevent an accepted HTTPS endpoint from returning a
`Location: http://...` response. A manifest downgrade is especially serious:
an on-path attacker can replace the manifest, package URL, and MD5 together,
so the later MD5 comparison does not restore transport integrity. The tests
reject a directly supplied HTTP URL but do not exercise a downgrade redirect.

**Fix:** Build and reuse an explicit client for both manifest and package
requests:

```rust
let client = reqwest::Client::builder()
    .https_only(true)
    .redirect(reqwest::redirect::Policy::limited(10))
    .build()?;
```

Also reject a response whose final URL is not HTTPS before consuming its body,
and add local-server regressions for HTTPS-to-HTTP redirects on both requests.

### CR-02: Every fuzz job pins a nonexistent GitHub Action ref

**File:** `.github/workflows/fuzz.yml:70`

**Issue:** Lines 70, 111, 152, and 194 use
`dtolnay/rust-toolchain@nightly-2025-11-30`. The dated nightly is a toolchain
name, not a branch, tag, or commit in the action repository. GitHub returns 404
for its `action.yml` and no matching ref from `git ls-remote`, so all four jobs
fail before installing Rust. `test_protocol_target_docs.py` currently enforces
the broken spelling.

**Fix:** Pin `dtolnay/rust-toolchain` to a reviewed full commit SHA and pass
`nightly-2025-11-30` through the action's `toolchain` input. Update the workflow
guard to assert both identities and, where practical, validate pinned action
refs rather than accepting any `owner/repo@...` string.

### CR-03: Compatibility validation reports success without a usable oracle

**File:** `scripts/validate-v310-consistency.sh:860`

**Issue:** When both external references are unavailable, the comparison
function returns success and line 913 prints `Validation completed
successfully`, turning an unevaluated parity run into positive evidence. The
one-reference-unavailable path is also malformed: lines 847-848 store an empty
URL while line 412 calls `curl` unconditionally with both URLs, producing exit
3 instead of a controlled result.

**Fix:** Model comparison as an explicit tri-state (`matched`, `mismatched`,
`reference-unreachable`). Never emit the success marker for an unreachable
oracle, make partial availability deterministic, and add behavioral tests for
zero, one, and two reachable implementations.

### CR-04: CI dependency-policy checks are not locked

**File:** `.github/workflows/ci.yml:130`

**Issue:** Both `cargo deny` invocations resolve Cargo metadata without the
supported global `--locked` flag. This violates Phase 1's stated fail-closed
contract for every dependency-resolving command and allows CI to evaluate a
graph different from the committed lock files.

**Fix:** Run `cargo deny --locked check ...` for both root and fuzz manifests,
update the workflow assertions, and correct the retained evidence commands so
the documented proof matches the enforced command.

## Warnings

### WR-01: Vote pools do not bind the signing network or validator context

**File:** `neo-blockchain/src/state_root/consensus.rs:141-149`

**Issue:** `StateRootIdentity` keys a pool by version, index, and root hash, but
`network` and the ordered `validators` slice are supplied afresh on every
`add_vote` call. For an `M = 3` pool, two signatures accepted under network A
and a third accepted under network B enter the same map and return
`Some(StateRoot)`. The resulting witness verifies under neither network.
Changing or reordering the validator set has the same failure mode because
stored signatures are not revalidated against the context used to build the
witness. Current node composition normally supplies a stable context, but the
exported collector does not enforce that invariant.

**Fix:** Store the network and exact ordered validator set with each vote pool
on its first vote, reject subsequent context changes, and aggregate against
the stored context. Revalidate the selected quorum before returning a witness.
Add cross-network, reordered-validator, and changed-validator regression tests.

### WR-02: Notification state has two independently mutable sources of truth

**File:** `neo-payloads/src/execution/notify_event_args.rs:31-34`

**Issue:** The public `state: Vec<StackItem>` can be replaced or extended after
construction, while the new private `state_array` permanently snapshots the
constructor input. `to_stack_value` and post-Domovoi projection read `state`,
but pre-Domovoi projection returns `state_array`. A caller can therefore make
the same notification serialize one state and return a different state from
`GetNotifications`, with behavior changing at Domovoi. Neo v3.10.1 stores one
get-only `Array State`; it has no parallel mutable representation.

**Fix:** Store one immutable array as the canonical notification state, make
the raw vector private, and expose read-only accessors for payload/indexer
consumers. Derive StackValue and both hardfork projections from that same
array. Add a regression proving all projections share one state source.

### WR-03: External result-stack aliases are split into distinct local objects

**File:** `neo-execution/src/application_engine/external_vm.rs:403-407`

**Issue:** Each result-stack value is converted with a separate
`StackItem::try_from` call, and each call creates a fresh
`StackValueConversion` memo table. A script that leaves `DUP` copies of one
Array, Struct, Map, or Buffer on the result stack therefore produces distinct
local allocations carrying the same numeric ID. Mutation no longer propagates
between the aliases, while ID-based equality/reference logic can still treat
them as one object. The module is currently non-canonical, but this makes the
retained differential path an unreliable parity probe.

**Fix:** Add a neo-vm graph-conversion API that converts an entire
`Vec<StackValue>` with one memo table, and use it for both result validation and
materialization. Add a focused external-interpreter test that returns a
duplicated compound and verifies shared allocation by mutation.

### WR-04: The manifest request can hang or buffer an unbounded response

**File:** `neo-node/src/node/fast_sync/package/manifest.rs:36-43`

**Issue:** The convenience client has no connect, read-idle, or total timeout,
and `.json()` buffers the response without a manifest-size limit. A stalled or
malfunctioning endpoint can leave fast sync waiting indefinitely; an oversized
response can consume unbounded memory before JSON decoding fails. The adjacent
package download similarly needs an explicit byte/disk bound rather than only
checking `Content-Length` after the stream finishes.

**Fix:** Reuse a configured client with explicit connect and total/read-idle
timeouts. Read the manifest through a capped byte stream before deserializing,
and enforce configured package-size and free-space limits before and during
the ZIP download. Add stalled-response and over-limit tests.

### WR-05: The external comparison environment is mutable

**File:** `scripts/validate-v310-consistency.sh:114`

**Issue:** The compatibility oracle fetches `neo-execution-specs` from mutable
`main`, upgrades pip, and installs open-ended dependencies. Identical neo-rs
source can therefore execute different comparison code and dependencies at
different times, undermining reproducibility and making parity failures hard
to diagnose.

**Fix:** Pin a reviewed full execution-specs commit, install its Python
environment from a hash-locked dependency set without upgrading tooling at
runtime, and retain the source/dependency identities in validation reports.

### WR-06: Configured dependency bans are never enforced

**File:** `deny.toml:23`

**Issue:** The policy configures bans, including `wildcards = "deny"`, but CI
and retained evidence run only `advisories licenses sources`. The configured
bans therefore provide no gate.

**Fix:** Include `bans` in locked root and fuzz `cargo deny` checks and update
tests/evidence to assert the complete policy surface. The current graphs pass
with duplicate-version warnings.

### WR-07: CI actions are mutable and workflows retain default write access

**File:** `.github/workflows/ci.yml:48`

**Issue:** The Phase 1 workflows use mutable action tags, including
`cache-apt-pkgs-action@latest`, and define no top-level token permissions.
Repository settings currently grant the default workflow token write access,
so an upstream tag movement has unnecessary repository privileges.

**Fix:** Pin every third-party action in all three workflows to a reviewed full
commit SHA, add top-level `permissions: contents: read`, and grant narrower job
permissions only where a job demonstrably needs them.

### WR-08: Repository policy guards do not run on pull requests

**File:** `.github/workflows/ci.yml:114`

**Issue:** No CI job runs the Python repository suite. The exact file-size
ratchet and workflow/dependency assertions therefore passed only during the
retained evidence run and can silently regress on subsequent pull requests.

**Fix:** Add `python3 -m unittest discover -s scripts/tests -p 'test_*.py'` as a
required CI step or job, preserving its failure status.

## Info

### IN-01: State-root tests never verify the generated witness

**File:** `neo-blockchain/src/state_root/tests/consensus.rs:49-55`

**Issue:** The positive test checks only the verification-script hash and the
invocation-script byte length, so a future regression that pushes arbitrary or
incorrectly ordered 64-byte values would still pass. Lines 73-76 also describe
a short-signature test but only pass an empty map to the aggregator.

**Fix:** Verify the completed witness against the root sign data and validator
set, and submit explicit 63-byte and 65-byte signatures through `add_vote`.

---

_Reviewed: 2026-07-13T18:15:42Z_
_Reviewer: the agent (gsd-code-reviewer generic-agent workaround)_
_Depth: standard_

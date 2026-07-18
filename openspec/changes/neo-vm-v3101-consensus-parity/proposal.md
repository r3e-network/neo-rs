## Why

The single-VM cutover removed the unsafe `StackItem`/`StackValue` boundary, but a source-level differential audit against official Neo and Neo.VM v3.10.1 found consensus-relevant execution mismatches. These must be fixed and covered by behavioral evidence before the node can credibly claim full MainNet replay compatibility.

## What Changes

- Keep workspace `neo-vm` as the only interpreter and `StackItem` as the only mutable runtime value model; do not reintroduce `neo-vm-rs` or graph conversion adapters.
- Match Neo.VM v3.10.1 script loading, implicit return, control-flow target, exception, slot, conversion, and script-builder semantics, including historical pre-hardfork behavior.
- Make `System.Runtime.LoadScript` use strict script validation independently of the relaxed contract-script loader.
- Match Neo v3.10.1 application fault cleanup so notifications emitted before a later fault are not retained in execution artifacts.
- Add focused differential regressions for every corrected semantic and identify each test with the relevant upstream behavior.
- Treat complete MainNet replay and state-root agreement as release evidence, not as an inference from unit tests.

## Capabilities

### New Capabilities

- `neo-vm-consensus-execution`: Consensus-critical NeoVM v3.10.1 execution, script validation, control flow, exception handling, and runtime-value behavior.

### Modified Capabilities

- `protocol-compliance-audit`: Requires explicit differential and replay evidence before declaring Neo N3 v3.10.1 protocol compatibility complete.

## Impact

The change affects `neo-vm`, `neo-execution`, execution artifact production in `neo-blockchain`, and their focused tests. Public Rust APIs may become stricter where the current behavior differs from official Neo.VM, but no Neo wire-format change is intended. The external `neo-vm-rs` dependency and `StackValue` conversion layer remain removed.

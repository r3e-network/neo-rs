# Phase 1 Plan 03 Summary: Single-VM Remediation

**Completed:** 2026-07-14

## Outcome

`neo-vm` is now the only Neo N3 VM and runtime value authority in the
workspace. Canonical execution no longer contains an alternate interpreter,
and consensus-facing crates no longer translate between independent stack
graphs.

## Delivered

- Removed the alternate interpreter and its canonical dispatch path.
- Ported the shared VM metadata required by the local engine into
  `neo-vm/src/vm_types/` and retained MIT provenance.
- Removed the external dependency from workspace and fuzz manifests and locks.
- Replaced conversion-based interoperable and serializer APIs with direct
  `StackItem` APIs.
- Migrated payload, manifest, execution, native-contract, RPC, blockchain, and
  node tooling consumers and tests.
- Introduced `RpcStackItem` as an immutable client transport DTO; server-side
  execution continues to use `StackItem`.
- Fixed application-log max-size error rendering so the protocol-visible text
  remains `Max size reached` rather than a doubly wrapped error.
- Superseded ADR-044 with ADR-045 and corrected maintained planning and operator
  documentation.

## Evidence Boundary

This remediation establishes one executable VM implementation and reduces the
surface that later differential testing must prove. It does not establish full
Neo v3.10.1 differential parity, sustained live-peer interoperability, full
MainNet replay/state parity, or authenticated checkpoint fast sync. Those
remain Phase 3 through Phase 6 release gates.

See `01-VM-CUTOVER-EVIDENCE.md` for retained commands and results.

## Follow-Up Consensus Audit

The subsequent `neo-vm-v3101-consensus-parity` audit corrected implicit return,
lazy versus strict script loading, control-flow bounds, fault cleanup, null
conversion, struct packing, slot mutation order, unhandled throws, and strict
UTF-8 behavior against pinned Neo/Neo.VM v3.10.1 sources. Locked workspace,
policy, fuzz, and dependency gates pass. Twenty-nine recorded C# observations
now cover the corrected VM and ApplicationEngine semantics; MainNet
replay/state-root agreement remains open release evidence.

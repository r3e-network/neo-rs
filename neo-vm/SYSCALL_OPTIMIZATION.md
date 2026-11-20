# Syscall Dispatch and Metadata Ownership

## Overview

This document describes how the VM dispatches syscalls and where syscall
metadata lives.

## Single Owner: The Registering Layer

Syscall metadata — canonical name, GAS base price and required call flags — is
**not** owned by `neo-vm`. It is registered explicitly by the layer that
implements the service:

- Neo host services are registered by `ApplicationEngine`
  (`neo-smart-contract/src/application_engine/{contract,runtime,storage,crypto,iterator}.rs`)
  through `register_host_service(name, price, call_flags, handler)`.
- Extension syscalls register their own descriptors with
  `InteropService::register(_host_descriptor)`.

Each registration is stored once, per engine, as a
`VmInteropDescriptor` in `InteropService.descriptors`. The same descriptor is
what the VM dispatches from and what `ApplicationEngine` enforces (price and
call flags in `ApplicationEngine::invoke_syscall`), so the two can never
disagree.

Storage permissions follow the C# per-operation split and are asserted by
`storage_syscalls_require_per_operation_flags`:

| Syscall | Required flags |
|---|---|
| `System.Storage.GetContext`, `System.Storage.GetReadOnlyContext`, `System.Storage.AsReadOnly` | `READ_STATES` |
| `System.Storage.Get`, `System.Storage.Find` | `READ_STATES` |
| `System.Storage.Put`, `System.Storage.Delete` | `WRITE_STATES` |
| `System.Storage.Local.Get`, `System.Storage.Local.Find` (Faun) | `READ_STATES` |
| `System.Storage.Local.Put`, `System.Storage.Local.Delete` (Faun) | `WRITE_STATES` |

## Dispatch

`ExecutionEngine::on_syscall` and `InteropService::invoke_by_hash` both perform a
single lookup in the registered descriptor map:

1. Unknown hash → error (`syscall ... not registered`).
2. Descriptor without a VM callback (`handler: None`) → the call is forwarded to
   the `InteropHost`, which owns the implementation, the pricing and the
   permission check.
3. Descriptor with a VM callback → the VM validates `required_call_flags` and
   invokes the callback directly.

There is no VM-side built-in syscall table, no static `OnceLock` registry and no
second copy of gas prices or permission bits. A previous "fast path" cuckoo hash
table duplicated all three and drifted from the registration (it required
`READ_STATES | WRITE_STATES` for every `System.Storage.*` call, among other
differences), which is why it was removed.

## Testing

- `neo-vm/tests/syscall_metadata_ownership_test.rs` — the VM ships no built-in
  table; registration is the only metadata source; unregistered hashes are
  rejected.
- `neo-smart-contract/src/application_engine/state.rs`
  (`default_interop_tests`) — per-operation storage flags and the full
  registered price table, kept in sync with
  `formal/coqlib/gas_price_refinement.v`.

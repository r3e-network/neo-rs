# Awaitable Native-to-Contract Call: Design (CallFromNativeContractAsync port)

> Read-only scout output, 2026-06-10. Unblocks PolicyContract.recoverFund's
> balance sweep and the C#-exact _deploy-before-notification ordering in
> ContractManagement deploy/update.

## Design Report: Awaitable Contract Call from Native Frame

Based on my exploration of the C# Neo implementation, the Rust neo-rs engine, and the PolicyContract's blocked recoverFund method, here is the complete design specification.

---

### 1. How C# Implements It: CallFromNativeContractAsync Suspension Model

**C# Implementation** (`neo-rs/neo_csharp/src/Neo/SmartContract/ApplicationEngine.cs:416-434`):

```csharp
internal ContractTask CallFromNativeContractAsync(UInt160 callingScriptHash, UInt160 hash, string method, params StackItem[] args)
{
    var contextNew = CallContractInternal(hash, method, CallFlags.All, false, args);
    var state = contextNew.GetState<ExecutionContextState>();
    state.NativeCallingScriptHash = callingScriptHash;
    ContractTask task = new();
    contractTasks.Add(contextNew, task.GetAwaiter());  // <-- Registration hook
    return task;
}

internal ContractTask<T> CallFromNativeContractAsync<T>(UInt160 callingScriptHash, UInt160 hash, string method, params StackItem[] args)
{
    var contextNew = CallContractInternal(hash, method, CallFlags.All, true, args);  // <-- true = has_return_value
    var state = contextNew.GetState<ExecutionContextState>();
    state.NativeCallingScriptHash = callingScriptHash;
    ContractTask<T> task = new();
    contractTasks.Add(contextNew, task.GetAwaiter());  // <-- Registration hook
    return task;
}
```

**Suspension Model** (`neo-rs/neo_csharp/src/Neo/SmartContract/ContractTask.cs` & `ContractTaskAwaiter.cs`):

1. **ContractTask** is a custom awaitable type (implements `INotifyCompletion` pattern).
2. **ContractTaskAwaiter<T>** holds the result via `SetResult(T result)` method, called when the child context unwinds (`ContextUnloaded`).
3. **Native code returns immediately** — the native method ends with `await task`, which:
   - Pushes a `ContractTask` onto a `Dictionary<ExecutionContext, ContractTaskAwaiter>` (keyed by the child context).
   - Yields control back to the VM step loop.
   - The VM executes the child context to completion.
4. **When the child context completes** (`ContextUnloaded`, line 465-470):
   - `contractTasks.Remove(context, out var awaiter)` retrieves the awaiter.
   - `awaiter.SetResult(engine)` resumes the native method from the suspension point with the return value on the evaluation stack.
   - The native method's async method builder resumes execution after the `await`.

**Observable Semantics**:
- **Fees**: Child context fees are `Add`'ed to the parent fee counter.
- **Notifications**: Child notifications are appended to parent list; on exception, they are rolled back.
- **Snapshot visibility**: Child context gets a `SnapshotCache.CloneCache()`, committed when the child context unloads without exception.
- **Exception propagation**: `UncaughtException` triggers rollback of notifications; the awaiter receives an exception via `SetException()` (implicit in the method builder's `ContextUnloaded` logic).

**Key code anchor**: `ApplicationEngine.cs:ContextUnloaded` (line 436-471) is the bridge: when a child context unwinds, it wakes the native method's continuation via the awaiter.

---

### 2. Rust Engine Execution Model for Native Calls

**Current Queue-Only Mechanism** (`neo-execution/src/application_engine_contract.rs:407-442`):

```rust
pub fn queue_contract_call_from_native(
    &mut self,
    calling_script_hash: UInt160,
    contract_hash: UInt160,
    method: impl Into<String>,
    args: Vec<StackItem>,
) {
    self.pending_native_calls.push(PendingNativeCall { … });
}

pub fn process_pending_native_calls(&mut self) -> Result<()> {
    if self.pending_native_calls.is_empty() { return Ok(()); }
    let pending = std::mem::take(&mut self.pending_native_calls);
    for call in pending.into_iter().rev() {
        self.call_from_native_contract_dynamic(
            &call.calling_script_hash,
            &call.contract_hash,
            &call.method,
            call.args,
        )?;
    }
    Ok(())
}
```

**System.Contract.CallNative Handler** (`application_engine_contract.rs:238-379`):

```rust
fn contract_call_native_handler(
    app: &mut ApplicationEngine,
    engine: &mut ExecutionEngine,
) -> VmResult<()> {
    // 1. Extract version, context, method metadata from state_arc
    // 2. Pop args from evaluation stack
    // 3. Call app.call_native_contract(script_hash, &method_name, &args)
    //    -> This invokes the native method synchronously
    // 4. Push result back to evaluation stack (line 362-369)
    // 5. AFTER native returns: app.process_pending_native_calls() (line 372)
    //    -> Queued calls are loaded into the VM, execute on next step
    Ok(())
}
```

**Native Method Invocation** (`application_engine/fees_events_native.rs:190-293`):

```rust
pub fn call_native_contract(
    &mut self,
    contract_hash: UInt160,
    method: &str,
    args: &[Vec<u8>],  // <-- Serialized args, not StackItems
) -> Result<Vec<u8>> {  // <-- Returns serialized result
    // Resolve native contract from registry
    // Check active, permissions, whitelist
    let result = native.invoke(self, method, args)?;  // <-- Synchronous invoke
    Ok(result)
}
```

**Execution Architecture**:
- **invoke()** is called synchronously within the CallNative handler's closure.
- The handler holds `&mut ApplicationEngine`, so the native method can mutate state (snapshot, notifications, fees).
- **No context switching** — the native method runs directly as Rust code, not in the VM.
- **All state mutations** are reflected in the same `ApplicationEngine` instance.
- Calls to user contracts are queued; the VM must explicitly call `process_pending_native_calls()` to execute them.

**What State Must Be Saved/Restored for Sync Re-entry**:
- The current **invocation stack context** (must be suspended or nested).
- The **snapshot cache** (must be isolated for the callee's writes, yet visible for reads).
- **Fee counters** (child's CPU/storage fees must accumulate to parent's).
- **Notifications** (child's notifications append to parent's).
- **Call flags** (child context inherits `CallFlags::ALL` in C#; child's `WRITE_STATES | ALLOW_NOTIFY` may be further restricted).

---

### 3. Three Candidate Designs with Trade-offs

#### **(a) Nested-Engine Execution** — Spawn a read-only child engine, then make it write-through

**Mechanism**:
- From within `invoke()`, instantiate a second `ApplicationEngine` with:
  - A fresh `snapshot_cache` cloned from the parent's snapshot.
  - The same `protocol_settings`, `block_height`, `gas_limit`, `storage_price`, `exec_fee_factor`.
  - A fresh execution context loaded for the callee contract/method.
- Execute the child engine to completion (via a new `ExecutionEngine` step loop or a helper that runs until `HALT/FAULT`).
- **Write-through**: Child's snapshot mutations must be committed back to the parent's snapshot_cache.
- **Fee/Notification Accounting**: After child halts, transfer its fees and notifications to the parent.

**Pros**:
- **Cleanest isolation**: Child engine is completely independent; no borrow-checker trickery.
- **Mirrors C# nested-context logic**: Similar to how C# loads a child context onto the invocation stack.
- **Clear failure semantics**: Child exception doesn't corrupt parent state (child's snapshot stays isolated until commit succeeds).

**Cons**:
- **Performance overhead**: Allocates a full `ApplicationEngine` struct, spawns a new `ExecutionEngine` step loop, clones the snapshot.
- **Snapshot clone cost**: For large snapshots, cloning is expensive.
- **Complexity of write-through**: Must carefully merge child's `DataCache` changes back into parent's snapshot; storage keys must be tracked.

**Example precedent in codebase**:
- `neo-wallets/src/asset_descriptor.rs:63-101`: Spawns a fresh `ApplicationEngine` with a cloned snapshot for a read-only probe of `decimals` and `symbol`. Demonstrates the pattern, though it never commits back (read-only).

---

#### **(b) Re-entrant Same-Engine Execution** — Push the callee context onto the live invocation stack, loop the VM until it unwinds

**Mechanism**:
- From within the native method (which is currently executing in a synchronous `invoke()` call):
  - Call `self.call_from_native_contract_dynamic(...)` to push the callee's execution context onto the live VM's invocation stack.
  - Return control to the **caller's `invoke()` method** with a special "waiting" sentinel value.
  - The `System.Contract.CallNative` handler detects "waiting" and **re-enters the VM step loop** to execute pending contexts.
  - When the callee context unwinds (via `RETURN`), its result is pushed to the evaluation stack.
  - The handler resumes the native method's execution (via a state machine or a second-pass invocation).
  - Native method resumes after the pseudo-`await` and reads the result from the stack.

**Pros**:
- **No snapshot clone**: Reuses the same snapshot_cache; mutations are live.
- **No extra engine allocation**: Reuses the same `ApplicationEngine` and `ExecutionEngine`.
- **Fees/notifications are naturally accumulated**: No explicit transfer needed; child writes directly to parent's counters.

**Cons**:
- **Borrow-checker complexity**: `invoke(&mut self)` must somehow return control to `contract_call_native_handler` (which borrowed `&mut ApplicationEngine`), then re-acquire the borrow to resume. Rust's borrow rules make this very hard without unsafe or runtime-like machinery.
- **State machine complexity**: The native method must be re-entrant (or be split into state-machine tasks). C# uses async/await; Rust doesn't have native async in this context.
- **Two-pass semantics**: The native method's logic flow becomes: invoke → queue context → return to handler → handler loops VM → native method resumes. This breaks the illusion of synchronous execution and makes reasoning about error handling harder.
- **Snapshot visibility unclear**: While the same snapshot is mutated, the _timing_ of when mutations become visible to the native method's next `invoke()` call is subtle (mid-step, not at call boundaries).

**Borrow-checker blocker**:
- `invoke(&mut self)` is called from within `contract_call_native_handler(&mut ApplicationEngine, &mut ExecutionEngine)`.
- To resume the VM loop (which needs `&mut ExecutionEngine`), the handler would need to drop the `&mut self` borrow, but the native method's continuation is still logically "inside" `invoke()`.
- This would require `RefCell<ExecutionEngine>` or unsafe code to work around Rust's borrow semantics.

---

#### **(c) Continuation-Passing Natives** — Split native methods into resumable state machines, driven by the existing queue

**Mechanism**:
- A native method that needs to call a user contract does **not** call it directly. Instead, it:
  1. Validates preconditions, prepares arguments.
  2. Returns a special `CallFromNativeResult::Suspended { token: CallToken, callee_hash, method, args, on_return: fn(...) -> Result<Vec<u8>> }`.
  3. The dispatcher enqueues a `PendingNativeCall` and adds a **continuation callback** to a map keyed by `token`.
  4. When the callee completes, the dispatcher looks up the continuation by token and invokes it with the result.
  5. The continuation is a function pointer (or closure) that continues the native method's logic with the returned value.

**Pros**:
- **Uses existing queue infrastructure**: No new VM machinery needed; `process_pending_native_calls()` already exists.
- **No borrow-checker pain**: Continuations are just function pointers, safe to pass around.
- **Minimal overhead**: No snapshot clone, no engine allocation.

**Cons**:
- **Programmer burden**: Every native method that calls a user contract must be manually split into a pre-call phase and post-call continuation(s). Not ergonomic for complex control flow.
- **Error handling is awkward**: Exceptions in the callee must be passed to the continuation; error paths must be threaded through explicitly.
- **Type safety**: Continuations must encode the return type statically (or use `Any`). For `CallFromNativeContractAsync<T>`, the continuation must know `T` at definition time.
- **No suspension from within native**: If a native method calls two user contracts sequentially, it must be split into three phases (pre-first, post-first→pre-second, post-second). Nested or conditional calls become unreadable.
- **Code duplication**: Common patterns (like recoverFund's "get balance, then transfer") must be factored out as separate functions, not expressed as sequential calls in one method.

---

### 4. Recommendation: Nested-Engine Execution (Design A)

**Selected Design**: **Nested-Engine Execution with Write-Through**

**Rationale**:
1. **C# Semantic Match**: C# uses nested contexts on the invocation stack. A nested engine mirrors this: it's logically a "child" execution that's independent until committed.
2. **Borrow-Checker Safe**: Rust's ownership model is perfectly suited to this. No unsafe, no RefCell, no state machines.
3. **Clear Failure Handling**: If the callee faults, the child engine's snapshot is discarded, and the native method can decide how to respond. No partial mutations pollute the parent.
4. **Realistic Performance**: The snapshot clone is a one-time cost at the call site. For typical transactions, the number of calls from native → user contract is small (1–3). The asset descriptor pattern proves this is acceptable.
5. **Precedent**: The codebase already uses this pattern (`asset_descriptor.rs`), so implementation expertise exists.

**Implementation Steps** (First Pass):

**Step 1: Define the Awaitable Contract-Call Return Type**
- Create `neo-execution/src/application_engine_contract.rs` new type `AsyncContractResult<T>` that holds a `oneshot::channel` receiver (or similar sync mechanism).
- When the callee context completes, write the result to the channel.

**Step 2: Implement `call_from_native_contract_async<T>`**
- New public method on `ApplicationEngine`:
  ```rust
  pub fn call_from_native_contract_async<T>(
      &mut self,
      calling_script_hash: UInt160,
      contract_hash: UInt160,
      method: &str,
      args: Vec<StackItem>,
  ) -> Result<AsyncContractResult<T>> {
      // 1. Clone the current snapshot: Arc::clone(&self.snapshot_cache)
      // 2. Create a child ApplicationEngine with the cloned snapshot
      // 3. Load the callee contract and method onto the child engine
      // 4. Run the child engine to completion (execute_allow_fault)
      // 5. Extract the result from the child's evaluation stack
      // 6. Accumulate child's fees/notifications into parent
      // 7. Return the result (wrapped in a sync Result<T>, not async/await)
  }
  ```
  - **Key**: No actual suspension. This is a blocking call that executes the child to completion and returns the result immediately. The "async" naming is misleading; it's better called `call_from_native_contract_blocking<T>`.

**Step 3: Update PolicyContract.recoverFund**
- Change lines 1415-1425 from:
  ```rust
  Err(CoreError::invalid_operation(
      "recoverFund: result-returning contract calls from a native frame are not supported",
  ))
  ```
  to:
  ```rust
  let balance = engine.call_from_native_contract_async::<BigInt>(
      Self::script_hash(),
      token,
      "balanceOf",
      vec![StackItem::from_byte_string(account.to_bytes())],
  )?;
  
  if balance.is_positive() {
      let transfer_ok = engine.call_from_native_contract_async::<bool>(
          Self::script_hash(),
          token,
          "transfer",
          vec![
              StackItem::from_byte_string(account.to_bytes()),
              StackItem::from_byte_string(TREASURY_HASH.to_bytes()),
              StackItem::from_int(balance),
              StackItem::null(),
          ],
      )?;
      Ok(vec![u8::from(transfer_ok)])
  } else {
      Ok(vec![u8::from(false)])
  }
  ```

**Step 4: Fee and Notification Accounting**
- When child engine completes:
  - `parent_engine.gas_consumed += child_engine.gas_consumed`
  - `parent_engine.notifications.extend(child_engine.notifications)`
  - On child fault: roll back parent's notifications count (or discard child notifications entirely).

**Step 5: Snapshot Commit Strategy**
- Child engine's `snapshot_cache` is a clone, so its mutations don't affect the parent until explicitly committed.
- After child `HALT` and before returning: `parent_snapshot_cache.merge_delta(child_snapshot_cache)`
- Storage keys written by child are now visible to parent.
- If child `FAULT`s: discard the child snapshot (mutations roll back implicitly).

**File Anchors**:
- **Main implementation**: `neo-execution/src/application_engine_contract.rs` (new fn `call_from_native_contract_async`)
- **PolicyContract integration**: `neo-native-contracts/src/policy_contract.rs:1415-1425` (recoverFund method)
- **Precedent/reference**: `neo-wallets/src/asset_descriptor.rs:63-101` (read-only nested engine pattern)
- **Fees/notifications merging**: `neo-execution/src/application_engine/fees_events_native.rs` & `mod.rs` (extend gas/notification accumulators)

---

### Summary

**C# Does It**: Loads child context onto invocation stack, registers awaiter in a dict, yields to VM, resumes native method when child context unloads.

**Rust Engine Current State**: Fire-and-forget queue (`pending_native_calls`); user-contract calls execute AFTER the native method returns.

**Design A (Recommended)**: Nested-engine execution—clone the snapshot, run a child `ApplicationEngine` to completion, commit the result. Synchronous from the native method's perspective; safe, clear, precedented.

**Why Not B**: Borrow-checker + re-entrance = unsafe or RefCell hacks; state machines too complex.

**Why Not C**: Continuation-passing is tedious; manual splitting of control flow; only viable for fire-and-forget calls.

**Next Steps**: Implement `call_from_native_contract_async<T>` in `application_engine_contract.rs`, integrate with `PolicyContract.recoverFund`, test snapshot commit semantics, verify fee/notification accounting matches C#.

# Code Quality Phase 2 Fixes — All Priority Items Resolved

**Date:** 2026-07-03
**Status:** `cargo check --workspace` — zero errors, zero warnings

---

## Fix Summary

### 1. neo-gui Mutex::lock().unwrap() → .expect() (FIXED)
**Files:** `neo-gui/src/shell/app.rs`, `monitoring.rs`, `integrations.rs`, `dashboard.rs`, `plugins.rs`, `wallet.rs`, `rpc_explorer.rs`

**Issue:** 9 instances of `Mutex::lock().unwrap()` across 7 GUI files. Poisoned mutexes would panic the GUI thread with no diagnostic context.

**Fix:** Replaced all `.unwrap()` with `.expect("descriptive_message")` providing clear context for debugging:
- `NodeState mutex poisoned` (5 locations)
- `PollerCfg mutex poisoned` (2 locations)
- `RPC output mutex poisoned` (2 locations)
- `Wallet output mutex poisoned` (1 location)
- `Integration status mutex poisoned` (1 location)

**Severity:** Medium

---

### 2. neo-tee slice-to-array .unwrap() → .expect() (FIXED)
**Files:** `neo-tee/src/enclave/runtime.rs:563`, `neo-tee/src/mempool/fair_ordering.rs:140`

**Issue:** `.try_into().unwrap()` on slice-to-array conversions in TEE code. Though the conversions are logically safe (len check precedes the call, or source is `[u8;32]`), `.expect()` provides clearer diagnostics.

**Fix:**
- `runtime.rs`: Already inside `data.len() >= 8` guard; changed to `.expect("data[..8] must be valid since len >= 8")`
- `fair_ordering.rs`: Source is `[u8;32]` (always ≥ 8); changed to `.expect("[u8;32][..8] must convert to [u8;8]")`

**Severity:** Medium

---

### 3. Token Tracker Storage Error Logging (FIXED)
**Files:** `neo-rpc/src/plugins/tokens_tracker/trackers/{nep_11,nep_17}/{nep11_tracker,nep17_tracker}.rs`

**Issue:** 10 instances of `let _ = self.base.put/delete(...)` silently swallowing storage errors. If RocksDB writes fail, token tracker state silently diverges from chain state.

**Fix:** Replaced all 10 instances with `if let Err(e) = ... { TrackerBase::log(...) }` using the existing `TrackerBase::log` API:
- NEP-11 tracker: `transfer sent` (1), `transfer received` (1), `balance to` (1), `balance from` (1), `delete balance` (1), `NFT balance to` (1) = 6
- NEP-17 tracker: `transfer sent` (1), `transfer received` (1), `delete balance` (1), `balance put` (1) = 4

**Severity:** Medium

---

### 4. MAX_REDIRECTS Duplication Removed (FIXED)
**Files:** `neo-oracle-service/src/service/processing/url.rs`

**Issue:** `const MAX_REDIRECTS: u8 = 2` defined in `url.rs` was `#[allow(dead_code)]` and never used. The canonical definition in `https/process.rs:14` is the one actually used.

**Fix:** Removed the dead constant from `url.rs`. Only `https/process.rs` defines `MAX_REDIRECTS` now.

**Severity:** Low

---

### 5. TaskManagerService.shutdown #[allow(dead_code)] Removed (FIXED)
**Files:** `neo-network/src/service/task_manager.rs:203`

**Issue:** The `shutdown: CancellationToken` field had `#[allow(dead_code)]` but it IS actively read at line 257 (`self.shutdown.cancel()`).

**Fix:** Removed the `#[allow(dead_code)]` annotation. The field is correctly detected as used by the compiler without the annotation.

**Severity:** Low

---

### 6. FALSE POSITIVES Confirmed (No Fix Needed)

- **neo-consensus recovery.rs** — Already uses `?` propagation; no `let _ = writer.` patterns exist
- **neo-rpc rpc_server_state/mod.rs** — Already uses `?` propagation; no `let _ = writer.` patterns exist

---

## Compilation Verification

```bash
$ cargo check --workspace
    Checking neo-gui v0.10.0
    Checking neo-network v0.10.0
    Checking neo-oracle-service v0.10.0
    Checking neo-tee v0.10.0
    Checking neo-system v0.10.0
    Checking neo-rpc v0.10.0
    Checking neo-node v0.10.0
    Checking neo-tests v0.10.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.56s
```

Zero errors, zero warnings across all 27 crates.

---

## Files Changed

| # | File | Change | Lines |
|---|------|--------|-------|
| 1 | `neo-gui/src/shell/app.rs` | `.unwrap()` → `.expect()` (6 instances) | ~6 |
| 2 | `neo-gui/src/screens/observe/monitoring.rs` | `.unwrap()` → `.expect()` (1) | 1 |
| 3 | `neo-gui/src/screens/observe/integrations.rs` | `.unwrap()` → `.expect()` (1) | 1 |
| 4 | `neo-gui/src/screens/dashboard.rs` | `.unwrap()` → `.expect()` (1) | 1 |
| 5 | `neo-gui/src/screens/operate/plugins.rs` | `.unwrap()` → `.expect()` (1) | 1 |
| 6 | `neo-gui/src/screens/interact/wallet.rs` | `.unwrap()` → `.expect()` (1) | 1 |
| 7 | `neo-gui/src/screens/interact/rpc_explorer.rs` | `.unwrap()` → `.expect()` (1) | 1 |
| 8 | `neo-tee/src/enclave/runtime.rs` | `.unwrap()` → `.expect()` (1) | 1 |
| 9 | `neo-tee/src/mempool/fair_ordering.rs` | `.unwrap()` → `.expect()` (1) | 1 |
| 10 | `neo-rpc/.../nep11_tracker.rs` | `let _ =` → `if let Err(e) = ... log`(6) | ~18 |
| 11 | `neo-rpc/.../nep17_tracker.rs` | `let _ =` → `if let Err(e) = ... log`(4) | ~12 |
| 12 | `neo-oracle-service/.../url.rs` | Remove dead `MAX_REDIRECTS` | -3 |
| 13 | `neo-network/.../task_manager.rs` | Remove `#[allow(dead_code)]` | -1 |

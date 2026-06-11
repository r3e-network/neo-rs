# A3 — ProtocolSettings unification recipe (neo-core typed -> neo-config canonical)

> Source: workflow w6p5olbl4 (2026-05-30). csharp-parity agent succeeded; dep-feasibility
> and consumer-map agents FAILED structured output, so synthesis compensated with its own
> investigation. **Two claims were independently re-verified and CORRECTED below.**

## CORRECTIONS (verified vs C# v3.9.1 + neo-rs usage, 2026-05-30)

1. **`max_block_size` is NOT a safe delete.** It is read by neo-node consensus
   (`neo-node/src/consensus.rs:667` and `:918`) for live block-size validation, and the
   sibling constant `neo_primitives::constants::MAX_BLOCK_SIZE` (2_097_152) drives
   `neo-core/src/validation.rs:106`. C# v3.9.1 `ProtocolSettings.cs` indeed has NO
   `MaxBlockSize` (confirmed); the cap lives in the dBFT plugin (`DbftSettings.MaxBlockSize`,
   code-default 262144, shipped DBFTPlugin.json 2097152). Correct fix = MOVE the cap from
   ProtocolSettings into the consensus/dBFT settings and update the two consensus.rs readers
   — a consensus-behaviour refactor sequenced AFTER the struct relocation, NOT an in-place
   delete folded into A3.
2. **2MB is not "wrong"** — it matches the shipped DBFTPlugin.json default; only the field's
   *location* is the parity issue.

Remainder (dep additions, Settings.protocol serde/private() gap, ms_per_block ->
milliseconds_per_block across 22 neo-rpc client sites, no dependency cycle) spot-checked sound.

---
# Execution Recipe: Unify `ProtocolSettings` into `neo-config` (typed struct canonical)

Goal: make neo-core's **typed** `ProtocolSettings` (`/Users/jinghuiliao/git/r3e/neo-rs/neo-core/src/protocol_settings.rs`) the single canonical type, physically host it in the lower-layer `neo-config` crate, and delete neo-config's **string-based** struct (`/Users/jinghuiliao/git/r3e/neo-rs/neo-config/src/protocol.rs`). Keep `cargo check --workspace` green; do not regress C# v3.9.1 parity.

---

## 1. Verdict

**Feasible, but NOT a drop-in move.** The two structs are genuinely different types with incompatible APIs, and the "lower-layer host" has a non-trivial internal consumer of the type being deleted. Three blocking caveats must be handled inside the migration:

1. **No dependency cycle, but new deps are required.** `neo-config` is currently a leaf crate (deps: `serde`, `toml`, `uuid`, `dirs`, `thiserror`). The typed struct pulls in `neo_crypto::ECPoint`, `neo_primitives::{Hardfork, constants}`, and the neo-core-local `HardforkManager` + `impl_default_via_new!` macro. Verified: neither `neo-primitives` nor `neo-crypto` depends on `neo-config` (`grep neo-config neo-primitives/Cargo.toml neo-crypto/Cargo.toml` → empty), so `neo-config → {neo-primitives, neo-crypto}` introduces no cycle. `HardforkManager` must move down with the struct (it lives in neo-core today and is the only neo-core-internal coupling besides the macro).

2. **`neo-config::Settings` embeds the string struct as a serialized TOML field.** `/Users/jinghuiliao/git/r3e/neo-rs/neo-config/src/settings.rs:22` has `pub protocol: ProtocolSettings` inside `#[derive(Serialize, Deserialize)] Settings`, and `settings.rs:348-358` call `ProtocolSettings::mainnet()/testnet()/private(...)`. The typed struct (a) does **not** derive `Serialize` (only `Debug, Clone, PartialEq`), (b) has no `private()` constructor, and (c) has no `native_activation_heights`/`HardforkHeights` fields. So the canonical struct must gain `Serialize` + a `private()` constructor before `Settings` can hold it, OR `Settings.protocol` must be decoupled. Verified mitigations: `HardforkHeights`/`NativeActivationHeights` have **zero external consumers** (`grep` → empty outside `protocol.rs`), and `neo_config::Settings` itself has **zero external consumers** (`grep` → empty outside `neo-config/src`). So `Settings.protocol` is internal-only and safe to adapt.

3. **The RPC client depends on the string struct's exact field/method names**, which differ from the typed struct. All 22 string-struct consumers are in `neo-rpc/src/client/**`. The load-bearing API surface (measured): `ProtocolSettings::default_settings()` (69×), `.address_version` (33×), `.network` (3×), `.ms_per_block` (2× read + 2× `= 2` mutation in tests), the builder `.protocol_settings(...)` (12×), `Arc<ProtocolSettings>` storage, and `Option<ProtocolSettings>::unwrap_or_default()`. The killer mismatch: **string `.ms_per_block: u64` vs typed `.milliseconds_per_block: u32`** — these consumers will not compile against the typed struct without an edit or a shim.

**Net verdict:** Proceed. The struct move is mechanical; the two real risks are (i) the `Settings.protocol` serde/`private()` gap and (ii) the `ms_per_block → milliseconds_per_block` rename in neo-rpc client. Both are addressed in the ordered steps below, each ending green.

---

## 2. C# parity actions (do these IN PLACE in `neo-core/src/protocol_settings.rs` BEFORE the move)

Apply these to the typed struct first, while it still lives in neo-core, so each parity fix lands on a green build before any file relocation. Citing the parity audit:

1. **Remove `max_block_size` entirely (discrepancy: `EXTRA_IN_RUST`).** v3.9.1 `ProtocolSettings.cs` has no `MaxBlockSize` and no `MaxBlockSystemFee` property (verified by full read + grep over `neo_csharp/src/Neo/`). The block-size cap now lives only in the DBFT plugin (`DbftSettings.MaxBlockSize = 262144`/256KB), not in `ProtocolSettings`. The Rust field carries a stale doc-comment ("Matches C# MaxBlockSize property") and a wrong value (2MB ≠ 256KB).
   - Delete the field declaration `pub max_block_size: u32,` (lines 61-63).
   - Delete its two assignments `max_block_size: constants::MAX_BLOCK_SIZE as u32,` in `mainnet()` (line 138) and `testnet()` (line 190).
   - Leave `neo_primitives::constants::MAX_BLOCK_SIZE` alone — it is still legitimately used elsewhere (e.g. `MAX_ARRAY_SIZE`/`MAX_ITEM_SIZE` aliases in `neo-primitives/src/constants.rs`, and neo-core wire-limit tests). Only the `ProtocolSettings` field is removed.
   - **`MaxBlockSystemFee`: nothing to do.** The typed struct already correctly omits it (audit verdict `match`).

2. **Do NOT "fix" the `Default == mainnet()` semantics in this migration.** The audit flags `Default::default() == mainnet()` (vs C# neutral `Default`) and the missing `Custom` static as divergences, but: (a) the entire Rust codebase relies on `ProtocolSettings::default()` yielding live mainnet (e.g. 69 `default_settings()` call-sites in neo-rpc client, plus `NeoSystem::new(ProtocolSettings::default(), …)` across neo-rpc server), and (b) changing default semantics is a behavioral change orthogonal to a struct relocation and would break dozens of call-sites. **Out of scope for a build-green move.** Record it as a known, pre-existing divergence; do not touch it here.

3. **Hardfork enum ordering — verify, don't change.** The audit's only `unverified` consensus-critical item is whether `HardforkManager::all()` ordering matches C# `Enum.GetValues(Hardfork)`. Resolved by reading `neo-core/src/hardfork.rs`: `HardforkManager::all()` delegates to `Hardfork::all()` (defined in `neo-primitives` via the `__protocol_enum!` macro), returning `[HfAspidochelone, HfBasilisk, HfCockatrice, HfDomovoi, HfEchidna, HfFaun, HfGorgon]` in declaration order. `ensure_omitted_hardforks()` and `validate_hardfork_sequence()` both iterate this single ordering, so the leading-zero backfill and continuity checks are internally consistent. **No code change** — the move preserves this exactly because `HardforkManager` moves with the struct. (Independent confirmation against the C# enum order remains a separate audit item, but the relocation neither helps nor hurts it.)

All other audited fields are byte-exact matches (`MillisecondsPerBlock=15000`, `MaxTransactionsPerBlock=512`, `MemoryPoolMaxTransactions=50000`, `MaxTraceableBlocks=2102400`, `InitialGasDistribution=5_200_000_000_000_000`, `AddressVersion=0x35`, `MaxValidUntilBlockIncrement=5760`) and require no change.

---

## 3. Dependency changes

### 3a. `neo-config/Cargo.toml` — deps to ADD
The struct + `HardforkManager` + the `impl_default_via_new!` macro require:
```toml
# add under [dependencies] in /Users/jinghuiliao/git/r3e/neo-rs/neo-config/Cargo.toml
neo-primitives = { workspace = true }   # Hardfork enum, constants::{MAINNET_MAGIC, TESTNET_MAGIC,
                                         #   ADDRESS_VERSION, MAX_TRACEABLE_BLOCKS, INITIAL_GAS_DISTRIBUTION}
neo-crypto     = { workspace = true }   # ECPoint
hex            = { workspace = true }   # parse_committee hex::decode
serde_json     = { workspace = true }   # from_value / Value in load_from_stream/from_value
parking_lot    = { workspace = true }   # RwLock for HardforkManager::instance() static
```
`serde` is already present (the typed struct's `ProtocolConfiguration` derives `Deserialize`; you will additionally derive `Serialize`/`Deserialize` on the struct itself — see §5 step 2c). `std::sync::LazyLock` is std, no dep. Confirm each of `neo-primitives`, `neo-crypto`, `hex`, `serde_json`, `parking_lot` exists in `[workspace.dependencies]` (they are used by neo-core today, so they do).

### 3b. Resolution for each neo-core coupling
| Coupling in `protocol_settings.rs` | Source today | Resolution after move |
|---|---|---|
| `crate::constants::{MAINNET_MAGIC, TESTNET_MAGIC, ADDRESS_VERSION, MAX_TRACEABLE_BLOCKS, INITIAL_GAS_DISTRIBUTION}` | neo-core `constants.rs` which is just `pub use neo_primitives::constants::*` | Repoint imports to `neo_primitives::constants::*` directly. Verified these 5 names all exist in `neo-primitives/src/constants.rs`. `MAX_BLOCK_SIZE` import is dropped (field removed in §2). |
| `crate::hardfork::{Hardfork, HardforkManager}` | `Hardfork` re-exported from neo-primitives; `HardforkManager` defined in neo-core `hardfork.rs` | **`HardforkManager` must move down with the struct** (it has no neo-core-specific deps beyond the macro). `Hardfork` import becomes `neo_primitives::Hardfork`. |
| `crate::impl_default_via_new!(HardforkManager)` | macro in `neo-core/src/macros.rs:115` | The macro just generates `impl Default { fn default() { Self::new() } }`. Replace the one usage with a hand-written `impl Default for HardforkManager { fn default() -> Self { Self::new() } }` in neo-config (avoids moving the macro and a third coupling). |
| `neo_crypto::ECPoint` | neo-crypto | Add `neo-crypto` dep (§3a); import path unchanged. |

### 3c. Symbols that must move down with the struct
- **`HardforkManager`** (and its `INSTANCE: LazyLock<RwLock<…>>` static, `is_enabled`, `register`, `get_hardforks`, `all`, `mainnet`, `testnet`, `new`, `instance`, and the free `is_hardfork_enabled(Hardfork, u32)` fn) — all of `neo-core/src/hardfork.rs` content except the `pub use neo_primitives::{Hardfork, HardforkParseError}` re-export, which neo-config will also re-export.
- **Nothing else.** `find_file`, `load`, `load_from_stream`, `from_value`, `from_raw`, `ensure_omitted_hardforks`, `validate_hardfork_sequence`, `parse_committee*`, `application_root`, and `struct ProtocolConfiguration` are all self-contained (std + serde + the deps above).

---

## 4. API reconciliation (string surface → typed replacement)

The deleted string struct's surface (`neo-config/src/protocol.rs`) maps onto the typed struct as follows. Items marked **PORT** must be added to the canonical struct; **MAP** is a field/name change at call-sites; **REPOINT** is import-only.

| String-struct item | Typed-struct replacement | Action |
|---|---|---|
| `ProtocolSettings::mainnet()` / `testnet()` | identical names exist on typed struct | REPOINT (import from new path) |
| `ProtocolSettings::private(magic: u32)` | **does not exist** on typed struct | **PORT**: add `pub fn private(network_magic: u32) -> Self` to the typed struct — base on `testnet()` but `network: network_magic`, `validators_count: 1`, `standby_committee: vec![]`, `seed_list: vec![]`, `milliseconds_per_block: 1000`, `hardforks: Self::ensure_omitted_hardforks(HashMap::new())`. Only consumer is `neo-config/src/settings.rs:358`. |
| `ProtocolSettings::default_settings()` | identical name exists (`returns mainnet()`) | REPOINT — used 69× in neo-rpc client; no rename needed. |
| `ProtocolSettings::default()` | identical (`impl Default → default_settings`) | REPOINT |
| `.network: u32` | `.network: u32` | MAP (same name/type) — no change |
| `.address_version: u8` | `.address_version: u8` | MAP (same) — no change |
| `.ms_per_block: u64` | `.milliseconds_per_block: u32` | **MAP (rename + cast)**: at the 4 client sites rename `ms_per_block`→`milliseconds_per_block`. `wallet_api.rs:361` `... .ms_per_block / 2` → `... .milliseconds_per_block / 2` (now `u32`; the surrounding `std::cmp::max(1, …)` and division stay valid). `wallet_api/tests.rs:751,796` `settings.ms_per_block = 2;` → `settings.milliseconds_per_block = 2;`. |
| `.max_valid_until_block_increment: u32` | same name/type | MAP — no change (not used by client) |
| `.validators_count: u32` (string) | `.validators_count: i32` (typed) | type widens to signed; no client read of this field — internal-only. No action for consumers. |
| `.max_transactions_per_block`, `.memory_pool_max_transactions`, `.max_traceable_blocks`, `.initial_gas_distribution` | present on typed (note `initial_gas_distribution` is `u64` typed vs `i64` string; `memory_pool_max_transactions` is `i32` both) | MAP — no client consumer; no action |
| `.standby_validators: Vec<String>` (field) | `.standby_validators() -> Vec<ECPoint>` (method) | Different shape, but **no client consumer reads it**; the server/node/test consumers (`rpc_server_node/tests.rs:160`, `neo-node/src/consensus.rs:311,333`, etc.) already call the typed **method** `.standby_validators()`. No action. |
| `.standby_committee` | `.standby_committee: Vec<ECPoint>` (field on typed) | no string consumer; no action |
| `.seed_list: Vec<String>` | same | MAP — no change |
| `.is_hardfork_enabled(name: &str, height: u32) -> bool` | typed: `is_hardfork_enabled(hardfork: Hardfork, block_height: u32) -> bool` | **API SHAPE DIFFERS** but **no neo-rpc consumer calls the string `&str` variant** (the only `&str` callers were the string struct's own unit tests, which get deleted with the file). Server/native code already uses the typed `Hardfork` enum variant. **No PORT needed** — do not add a `&str` overload (it would re-introduce stringly-typed parity risk). |
| `.committee_count() -> u32` (returns `21.max(validators_count)`) | typed: `committee_members_count() -> usize` (returns `standby_committee.len()`) | **Different semantics**, but **no consumer outside the deleted file** calls `committee_count()` (grep → only `protocol.rs` tests). **No action**; the typed `committee_members_count()` is the canonical one. |
| `.time_per_block() -> Duration` | typed: `.time_per_block() -> Duration` (same) | MAP — no change |
| `native_activation_heights: NativeActivationHeights` + `hardforks: HardforkHeights` named sub-structs | typed: `hardforks: HashMap<Hardfork, u32>`; no native-heights field | **Drop.** Zero external consumers (grep confirmed). The only holder is `Settings.protocol`; reconcile per §5 step 2c/4. |
| `#[derive(Serialize, Deserialize)]` on the struct | typed derives only `Debug, Clone, PartialEq` + a separate `Deserialize`-only `ProtocolConfiguration` | **PORT (Serialize) — required for `Settings.protocol` TOML round-trip.** Add `Serialize, Deserialize` to the canonical struct (see §5 step 2c for the `ECPoint`/`Hardfork`-map serde caveat). |

**Named-network constructors summary:** keep `mainnet()`, `testnet()`, `default_settings()` as-is (REPOINT only); **add `private(u32)`** (PORT). **`is_hardfork_enabled`:** the typed `Hardfork`-enum signature is canonical; the `&str` variant is intentionally dropped (no surviving consumer, and stringly-typed lookup is the anti-pattern this unification removes).

---

## 5. Ordered steps (each ends in a green build)

Run the verification block (§5, step 7) after every numbered step. Steps are mechanical unless flagged **[JUDGMENT]**.

**Step 0 — branch & baseline (mechanical).** Already on `refactor/restore-green-baseline`; create a sub-branch. Run the full verification block once to capture the green baseline.

**Step 1 — C# parity fixes IN PLACE in `neo-core/src/protocol_settings.rs` (mechanical).**
- Remove the `max_block_size` field (decl lines 61-63) and its two assignments (lines 138, 190).
- Remove the now-unused `MAX_BLOCK_SIZE` from the `constants` import if it becomes dead.
- `cargo check -p neo-core` → green. (No relocation yet.)

**Step 2 — prepare the canonical struct's API + serde (mostly mechanical, one [JUDGMENT]).** Still in `neo-core/src/protocol_settings.rs`:
- (2a) Add `pub fn private(network_magic: u32) -> Self` (body per §4).
- (2b) Add a doc-comment noting `Default == mainnet()` is a deliberate, pre-existing divergence from C# neutral `Default` (per §2 item 2).
- (2c) **[JUDGMENT]** Add `Serialize, Deserialize` to the struct so it can be a `Settings.protocol` field. `Vec<ECPoint>` and `HashMap<Hardfork, u32>` must serialize: confirm `ECPoint` and `Hardfork` implement `Serialize`/`Deserialize` in neo-crypto/neo-primitives. If `ECPoint` does **not** derive serde, the lower-risk choice is to **NOT** derive serde on the canonical struct and instead change `Settings.protocol`'s representation (keep TOML config as the existing `ProtocolConfiguration`-shaped section and construct the typed struct via `from_raw`), since `Settings` has no external consumers and only round-trips in tests. Decide based on the serde-impl check; either path keeps the build green. This is the single design decision in the migration.
- `cargo check -p neo-core` → green.

**Step 3 — move the files into `neo-config` (mechanical).**
- Add the deps from §3a to `neo-config/Cargo.toml`.
- Create `neo-config/src/hardfork.rs` = content of `neo-core/src/hardfork.rs`, replacing `crate::impl_default_via_new!(HardforkManager);` with an explicit `impl Default`. Keep `pub use neo_primitives::{Hardfork, HardforkParseError};`.
- Create the new canonical `neo-config/src/protocol_settings.rs` = the (now parity-fixed, serde-ready) neo-core file, with imports repointed: `crate::constants::*` → `neo_primitives::constants::*`; `crate::hardfork::{…}` → `crate::hardfork::{…}` (now local to neo-config) or `neo_primitives::Hardfork` + `crate::hardfork::HardforkManager`.
- In `neo-config/src/lib.rs`: add `mod hardfork; mod protocol_settings;` and `pub use protocol_settings::ProtocolSettings; pub use hardfork::{Hardfork, HardforkManager};`. **Do not yet** remove the old `mod protocol;` line — keep both temporarily to stay green (name clash avoided because old is `protocol::ProtocolSettings`, new is `protocol_settings::ProtocolSettings`; re-export only one — see step 5).
- `cargo check -p neo-config` → green (new type compiles in its new home; old string type still present, unexported-or-shadowed).

**Step 4 — re-export from neo-core for back-compat (mechanical).**
- Delete `neo-core/src/protocol_settings.rs` and `neo-core/src/hardfork.rs`.
- In `neo-core/src/lib.rs`: replace `pub mod protocol_settings;` + `pub use protocol_settings::ProtocolSettings;` (lines 142, 286) with `pub use neo_config::ProtocolSettings; pub mod protocol_settings { pub use neo_config::ProtocolSettings; }` and `pub mod hardfork { pub use neo_config::{Hardfork, HardforkManager, HardforkParseError}; pub use neo_config::hardfork::is_hardfork_enabled; }` so every existing `neo_core::ProtocolSettings`, `neo_core::protocol_settings::ProtocolSettings`, and `neo_core::hardfork::*` path keeps resolving. (Verified these are the exact paths used by neo-rpc server/node/tests.)
- `cargo check -p neo-core --features runtime` → green. neo-node, neo-rpc server consumers unaffected (they go through neo-core re-exports and already use typed-struct field/method names).

**Step 5 — repoint the string-based consumers (mechanical, in `neo-rpc/src/client/**`).**
- The 22 `use neo_config::ProtocolSettings;` lines now resolve to the **typed** struct automatically (once step 6 removes the string one). Before that, in `neo-config/src/lib.rs` switch the re-export `pub use protocol::ProtocolSettings;` → `pub use protocol_settings::ProtocolSettings;` so `neo_config::ProtocolSettings` IS the typed struct.
- Apply the `ms_per_block → milliseconds_per_block` MAP at exactly 4 sites: `neo-rpc/src/client/wallet_api.rs:361`, `neo-rpc/src/client/wallet_api/tests.rs:751` and `:796` (and any builder default). No other field/method names differ for client consumers (`.address_version`, `.network`, `default_settings()`, `.protocol_settings(...)`, `unwrap_or_default()`, `Arc<ProtocolSettings>` all carry over).
- `neo-config/src/settings.rs`: `ProtocolSettings::private(0x01020304)` now resolves (step 2a). If §5/2c chose the no-serde path, adapt the `Settings.protocol` field type/serde accordingly.
- `cargo check -p neo-rpc --features client` and `--features server` → green.

**Step 6 — delete `neo-config/src/protocol.rs` (mechanical).**
- Remove the file and its `mod protocol;` + `pub use protocol::ProtocolSettings;` from `neo-config/src/lib.rs`.
- The string struct's own unit tests (`test_mainnet_settings` asserting `standby_validators.len() == 21`, `test_committee_count`, `test_hardfork_enabled` with `&str`) are deleted with the file — they tested the deleted API and have no typed equivalent obligation.
- `cargo check -p neo-config` → green.

**Step 7 — verification commands (run after every step; final gate).**
```bash
cd /Users/jinghuiliao/git/r3e/neo-rs
cargo check --workspace --all-targets
cargo check -p neo-rpc --features server --all-targets      # exercises server + client (server pulls client)
cargo check -p neo-node --all-targets                        # pulls neo-core/full
cargo check -p neo-tests --all-targets                       # tests/ crate (workspace neo-core)
# parity regression guard:
cargo test -p neo-config protocol                            # mainnet/testnet/private + hardfork heights
cargo test -p neo-core  hardfork                             # ensure_omitted / validate_sequence ordering
```
A green `cargo check --workspace --all-targets` plus the two `cargo test` runs is the completion gate.

---

## 6. Risk & rollback

**Top risks and detection:**

1. **Serialization drift on `Settings.protocol` (highest).** Adding `Serialize`/`Deserialize` to the typed struct changes the on-disk TOML shape vs the old string struct (`standby_validators: Vec<String>` of hex vs `standby_committee: Vec<ECPoint>`; `ms_per_block` key vs `milliseconds_per_block`; presence/absence of `native_activation_heights`). **Detect:** the existing `neo-config` toml round-trip tests (`Settings::from_file`/`to_string_pretty`) — run `cargo test -p neo-config`. Because `Settings` has zero external consumers, blast radius is contained to neo-config's own tests; if they fail, take the no-serde path (§5/2c) and reconstruct `protocol` via `from_raw`.

2. **`ms_per_block` rename missed at a call-site.** A missed site fails to compile (good — it cannot silently pass). **Detect:** `grep -rn "ms_per_block" neo-rpc/ neo-config/` must return zero after step 5; `cargo check -p neo-rpc --features server` must be green.

3. **Default-semantics divergence carried forward (pre-existing, not introduced).** `default()==mainnet()` is unchanged by this move; the audit's neutral-`Default`/`Custom` divergence persists. **Detect/track:** leave the §2-item-2 doc-comment as the record. Do **not** let reviewers "fix" it inside this PR — that would change 70+ call-sites' behavior and is out of scope.

4. **Hardfork ordering/backfill regression.** The move copies `HardforkManager::all()`→`Hardfork::all()` verbatim, so ordering is preserved; risk is only a transcription error. **Detect:** `cargo test -p neo-core hardfork` (the `test_mainnet_hardforks`/`test_testnet_hardforks`/`test_global_hardfork_manager` cases assert exact activation heights and that `HfFaun`/`HfGorgon` stay disabled).

5. **`max_block_size` removal breaks an unseen reader.** **Detect:** `grep -rn "\.max_block_size\b" --include=*.rs .` (excluding `neo_csharp/`) before and after step 1 — must be empty of struct-field reads; `MAX_BLOCK_SIZE` constant usages are unaffected and intentionally retained.

**Rollback:** Each step is an isolated commit ending green, so `git revert <step>` restores a buildable state. The riskiest commit (step 5/6, deleting the string struct) is reversible by un-deleting `protocol.rs` and restoring the `mod protocol; pub use protocol::ProtocolSettings;` re-export, since the typed and string types coexist without name clash until the final re-export switch. Keep steps 5 and 6 as separate commits so the string struct can be resurrected without also reverting the consumer renames.

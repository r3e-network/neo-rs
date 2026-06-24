# neo-execution-specs v3.10.0 parity audit — 116 confirmed divergences


## crypto-bls-hash

### [non-consensus] BloomFilter per-hash-function seed derivation differs from C# (p*0xFBA4C795+nTweak)
- spec: `crypto/bloom_filter.py:50,67`
- csharp: `Cryptography/BloomFilter.cs:53,74,88-89,99-100`
- fix: In src/neo/crypto/bloom_filter.py, change the per-function seed in BOTH add() (line 50) and check() (line 66) from murmur32(element, self.seed + i) to murmur32(element, (i * 0xFBA4C795 + self.seed) & 0xFFFFFFFF), matching C#'s _seeds[p] = (uint)p * 0xFBA4C795 + nTweak. The murmur32 core already matches; only the seed-array derivation needs fixing. Low priority (P2P/SPV parity only; module currently unwired in spec).

## crypto-ecc

### [consensus] ECPoint.decode does not validate compressed/uncompressed encoding length (accepts malformed input C# rejects with FormatException)
- spec: `crypto/ecc/point.py:41-58`
- csharp: `Cryptography/ECC/ECPoint.cs:78-103`
- fix: Two-part fix. (1) In ECPoint.decode (point.py:46-56) enforce exact lengths matching C# DecodePoint: raise ValueError if data[0] in (0x02,0x03) and len(data)!=33, or data[0]==0x04 and len(data)!=65 (ExpectedECPointLength=32 for secp256r1/k1). (2) Critically, make CryptoLib.verify_with_ecdsa (crypto_lib.py:204-227) NOT swallow the malformed-encoding error the way C# doesn't: the post-Cockatrice verifyWithECDsa V2 path must let the format/length error propagate so the engine FAULTs (C# VerifyWithECDsaV2 has no try/catch; V0/V1 catch only ArgumentException, and FormatException is not ArgumentException, so they fault too). Narrow the `except` so it does not convert a malformed-pubkey-length error into a `return False`. Otherwise the spec still HALTs-false where C# faults. Add a pinning test feeding a 35-byte (valid-33 + trailing) and a short pubkey, asserting FAULT, not False/True.

### [consensus] ECPoint.decode rejects off-curve uncompressed points that C# DecodePoint accepts
- spec: `crypto/ecc/point.py:46-51`
- csharp: `Cryptography/ECC/ECPoint.cs:89-99`
- fix: Do NOT apply the proposed fix (removing the on-curve check from ECPoint.decode) — it would not achieve parity and would silently weaken point validation. Instead model HF_Gorgon's VerifyWithECDsaV2: register a third verifyWithECDsa variant active_in=HF_GORGON (deprecate the Cockatrice variant at HF_GORGON) whose body does NOT swallow decode/curve errors. Concretely, the Gorgon path should let an invalid/off-curve pubkey (ValueError from ECPoint.decode, or an ECDsa-construction failure) PROPAGATE so the engine faults, matching C# CryptoLib.cs:106-112 (no try/catch) + Crypto.cs:256-259 (CryptographicException->ArgumentException) + NativeContract.cs:492-494 (engine.Throw). The pre-Gorgon (Cockatrice V1) variant should keep the catch-and-return-False semantics. Note this is a broader divergence than the narrow decode claim; the on-curve check in decode is fine and arguably correct — keep it. Recommend adding a conformance vector: verifyWithECDsa with a 65-byte 0x04 in-range off-curve pubkey must FAULT under HF_Gorgon and return False under HF_Cockatrice.

### [non-consensus] ECPoint.decode treats empty data and 0x00 prefix as the infinity point instead of erroring like C#
- spec: `crypto/ecc/point.py:43-44`
- csharp: `Cryptography/ECC/ECPoint.cs:78-101`
- fix: Remove the infinity branch in point.py decode() and raise instead.

### [non-consensus] ECPoint.decode/encode does not range-check coordinates against the field prime
- spec: `crypto/ecc/point.py:46-56`
- csharp: `Cryptography/ECC/ECFieldElement.cs:24-30`
- fix: Two-part fix:
1. In crypto/ecc/point.py decode(): after building x (and y for the 0x04 uncompressed case), raise ValueError if x >= curve.p (and y >= curve.p for uncompressed) BEFORE _is_on_curve/_decompress_y, mirroring ECFieldElement's value >= curve.Q guard (ECFieldElement.cs:26-27). encode() needs no change (it serializes a constructed point). Note types/ec_point.py:decode has the same missing check (its 0x04 path at lines 32-35 does not even call _is_on_curve) and should get the same guard.
2. To match C# exactly, the caller behavior must also be corrected: native crypto_lib.py verify_with_ecdsa (the HF_Gorgon+ V2 equivalent, registered active_in=HF_COCKATRICE at crypto_lib.py:68-75) currently catches ValueError and returns False (crypto_lib.py:226). C# V2 does NOT catch the decode exception — it FAULTS. So the post-Gorgon path should let the out-of-range decode error propagate to a VM fault rather than swallow it to False, while the pre-Gorgon (Cockatrice→Gorgon V1) path should keep returning False. Since the spec currently models only one post-Cockatrice verifyWithECDsa, this also exposes a missing V1-vs-V2 (HF_Gorgon) split that should be added to fully match v3.10.0.

## native-contract-mgmt

### [consensus] deploy missing post-Aspidochelone CallFlags.All check, manifest validation, Helper.Check(Script,Abi), Policy.IsBlocked check, and _deploy callback
- spec: `native/contract_management.py:230-270`
- csharp: `SmartContract/Native/ContractManagement.cs:254-303`
- fix: In spec contract_management.py deploy (230-270) reproduce C# Deploy 254-303 in order: (1) if HF_Aspidochelone enabled, fetch current execution-context CallFlags and raise InvalidOperationException unless CallFlags.All is set; (2) ensure script_container is a Transaction; (3) charge fee = max(storage_price*(len(nef)+len(manifest)), min_fee) * FeeFactor (10000) via the fee path (add the FeeFactor multiplier that is currently missing); (4) deserialize/validate NefFile (parse + checksum verify) and parse ContractManifest, then run Helper.Check(Script(nef.script, HF_Basilisk-strict), abi); (5) raise if Policy.IsBlocked(hash); (6) keep the already-exists check; (7) raise if not manifest.IsValid(engine.Limits, hash); (8) store sealed state + hash record; (9) invoke OnDeployAsync -> call the deployed contract's _deploy(data, update=False) BEFORE emitting the Deploy notification (move the notification into the OnDeploy path). Verify against an in-tree dotnet NefFile/ContractManifest.IsValid fixture.

### [consensus] OnPersist native-init/hardfork-activation pipeline absent; spec uses a genesis-only initialize() with no per-hardfork InitializeAsync invocation
- spec: `native/contract_management.py:377-388`
- csharp: `SmartContract/Native/ContractManagement.cs:71-118`
- fix: Add `ContractManagement.on_persist(engine)` mirroring C# `OnPersistAsync`: loop all native contracts in registration order; for each compute `is_initialize_block(settings, persisting_block.index) -> (bool, hfs)`; when true, read the existing `Prefix_Contract` record (GetAndChange) — if absent, write a fresh `Prefix_Contract` ContractState + `Prefix_ContractHash` record and, when the native's `active_in is None`, call its `initialize(engine)` (the genesis InitializeAsync(null) seeder); if present, bump update_counter and refresh nef/manifest; then for each hf in hfs call the hardfork-specific initializer; finally emit Deploy (new) or Update (existing) notification. Route the existing dead `initialize()` seeders (ContractManagement/NeoToken/GasToken/Oracle/Policy) through this loop instead of leaving them uncalled. Implement `is_initialize_block`/`get_contract_state(settings, index)` on NativeContract to mirror C# semantics (genesis-active when ActiveIn is null; activation height for hardfork-gated natives; hfs = newly-active hardforks at this index). Note: C# has no separate public Initialize() — all seeding lives in InitializeAsync driven by OnPersistAsync at the correct block height, so the spec must drive it from the persist loop, not a one-shot genesis call.

### [consensus] GetContractHashes Id is read little-endian and filtered to >=0 differently; spec also enumerates only contract-hash prefix but uses signed LE while C# reads big-endian Int32
- spec: `native/contract_management.py:172-187`
- csharp: `SmartContract/Native/ContractManagement.cs:192-202`
- fix: Fix the integer-to-key encoding to big-endian to match C# `BinaryPrimitives.WriteInt32BigEndian`, on BOTH the write and read sides (not just the GetContractHashes read as the auditor proposed).

Primary fix in native_contract.py StorageKey.create (lines 97-98): encode int args big-endian instead of little-endian, e.g. `arg.to_bytes(4, 'big')`. NOTE: this is a shared helper used by other natives; if changing it globally is risky, instead pass the id already big-endian for PREFIX_CONTRACT_HASH at the write sites (contract_management.py:264, 369) and the lookup site (contract_management.py:165), and change the read at contract_management.py:183 to `int.from_bytes(key_bytes[1:5], 'big', signed=True)`. All four sites must use the same (big-endian) order so single lookups and iteration order both match C#.

The `Id >= 0` filter the auditor proposed is optional/harmless (no negative ids are ever stored under PREFIX_CONTRACT_HASH in either implementation) and is not required for parity, though adding it faithfully mirrors C# ContractManagement.cs:198.

### [non-consensus] destroy is not Gorgon-versioned: spec always uses pre-Gorgon (block-after-erase) ordering and skips Policy block/whitelist side-effects entirely
- spec: `native/contract_management.py:93-94, 357-375`
- csharp: `SmartContract/Native/ContractManagement.cs:383-437`
- fix: Split spec destroy into Gorgon-gated DestroyV0 (active true..HF_Gorgon) and DestroyV1 (active HF_Gorgon), routed by block height, both delegating to a shared destroy_internal(engine, block_before_erase). destroy_internal must: resolve calling contract (return if absent); if block_before_erase, call Policy.BlockAccountInternal(engine, hash) + Policy.CleanWhitelist(engine, contract) BEFORE deleting; delete contract state, Prefix_ContractHash mapping, and contract storage; if not block_before_erase, call Policy.BlockAccountInternal + Policy.CleanWhitelist AFTER deleting; then send the Destroy notification. Policy.BlockAccountInternal must write a Prefix_BlockedAccount entry (empty bytes pre-HF_Faun; engine time value + NEO.VoteInternal(account, None) vote-revoke under HF_Faun) and refuse native accounts. Policy.CleanWhitelist must delete all Prefix_WhitelistedFeeContracts entries for the contract and emit a WhitelistChanged notification per removed entry. Verify Policy.BlockAccountInternal/CleanWhitelist exist in the spec's PolicyContract first; if absent they must be ported too.

### [non-consensus] update missing CallFlags.All check, fee charge, ushort.MaxValue update-counter cap, name-immutability check, manifest validation, whitelist clean, and _deploy(update=true) callback
- spec: `native/contract_management.py:325-355`
- csharp: `SmartContract/Native/ContractManagement.cs:327-376`
- fix: Rewrite native/contract_management.py update() to match C# ContractManagement.Update (ContractManagement.cs:327-376) step-for-step: (1) if HF_Aspidochelone enabled, fault unless current context CallFlags has CallFlags.All; (2) keep not-both-null check; (3) charge engine.add_fee(storage_price * FeeFactor(10000) * ((len(nef) if nef else 0) + (len(manifest) if manifest else 0))) using the engine's pico-fee AddFee (NOT add_gas) so the v3.10.0 pico-fee scaling matches; (4) load+get_and_change contract; (5) fault if update_counter == 65535 (ushort.MaxValue); (6) replace NEF (fault if empty) before whitelist clean; (7) call Policy.clean_whitelist(engine, contract) — which must first be implemented in policy.py to mirror PolicyContract.CleanWhitelist; (8) for manifest: parse, fault if name != contract.Manifest.Name, fault if not is_valid(limits, contract.hash), then replace; (9) Helper.Check(script, abi) with HF_Basilisk gating; (10) increment update_counter; (11) route the "Update" notification through an OnDeployAsync(update=true) path that first invokes the contract's _deploy method (requires adding a CallFromNativeContract-style native->contract call primitive to the spec engine) and then sends the Update notification with contract.hash. Mirror the same _deploy callback into deploy() with update=false for consistency. This requires building the missing _deploy/OnDeployAsync native-call infrastructure and clean_whitelist in the spec.

## native-crypto-std

### [consensus] jsonSerialize lacks JNumber safe-integer bound — no fault on large integers
- spec: `native/std_lib.py:261-262 (_to_json_value returns int unchanged) + json_serialize at std_lib.py:247-249 (json.dumps writes full big integer)`
- csharp: `SmartContract/JsonSerializer.cs:120-127 (throws InvalidOperationException if integer > JNumber.MAX_SAFE_INTEGER or < MIN_SAFE_INTEGER; writes (double)integer)`
- fix: Add a JNumber safe-integer bound to the jsonSerialize int branch in std_lib.py _to_json_value lines 261-262: fault when item exceeds 9007199254740991 or is below negative 9007199254740991, matching JsonSerializer.cs lines 123-124. bool is handled earlier so unaffected. Error must propagate as engine FAULT.</parameter>
</invoke>


### [consensus] jsonDeserialize truncates fractional numbers instead of faulting
- spec: `native/std_lib.py:282-283 (_from_json_value: isinstance float -> int(value))`
- csharp: `SmartContract/JsonSerializer.cs:196 (if (num.Value % 1) != 0 throw FormatException)`
- fix: In std_lib.py 282-283, fault when the parsed float has a nonzero fractional part, mirroring C# JsonSerializer.cs 196, and only convert to int for whole-number floats so 1.0 still yields 1.

### [consensus] atoi base-16 accepts '0x' prefix and sign; base-10 accepts whitespace/underscores/non-ASCII digits (more lenient than .NET NumberStyles)
- spec: `native/std_lib.py:330-344 (base16: removeprefix('0x'), startswith('-','+') int(value,16)) and :330-331 (base10: int(value,10))`
- csharp: `SmartContract/Native/StdLib.cs:108-113 (base10 NumberStyles.AllowLeadingSign; base16 NumberStyles.AllowHexSpecifier with InvariantCulture)`
- fix: In std_lib.py atoi: (1) base-10 — replace int(value,10) with a strict parser allowing only an optional single leading ASCII sign ('+'/'-') followed by ASCII digits [0-9]; fault (raise) on leading/trailing whitespace, underscores, and any non-ASCII (e.g. Arabic-Indic) digits, matching NumberStyles.AllowLeadingSign + InvariantCulture. (2) base-16 — remove the sign branch (lines 333-334) and the removeprefix('0x') (line 336); accept ONLY bare hex digits [0-9a-fA-F] and fault on any '0x'/'0X' prefix or '+'/'-' sign, matching NumberStyles.AllowHexSpecifier. Preserve the existing two's-complement-by-leading-nibble interpretation (high nibble in 8-f => negative), since bare-hex '80'->-128 already matches C#. Both base parsers must raise on invalid input so the VM faults exactly where C# does.

### [consensus] memorySearch backward search has off-by-one (inclusive end) and a start==0 whole-buffer special case absent in C#
- spec: `native/std_lib.py:411-414 (if backward and start==0: start=len(mem)-1; idx = mem[:start+1].rfind(value))`
- csharp: `SmartContract/Native/StdLib.cs:237-240 (backward: return mem.AsSpan(0, start).LastIndexOf(value))`
- fix: In native/std_lib.py memory_search, replace the backward branch so it scans the prefix [0, start) EXCLUSIVE of index `start` and drop the start==0 special-case. I.e. remove lines `if backward and start == 0: start = len(mem) - 1` and change `idx = mem[: start + 1].rfind(value)` to `idx = mem[:start].rfind(value)`. This makes start==0 backward return -1 (empty prefix) and excludes index `start`, matching C#'s `mem.AsSpan(0, start).LastIndexOf(value)`. Verified: with this change the spec matches C# for all tested single- and multi-byte values across start in [0,3].

### [consensus] base64Decode silently drops invalid characters instead of faulting
- spec: `native/std_lib.py:358-362 (base64.b64decode(s) without validate=True)`
- csharp: `SmartContract/Native/StdLib.cs:132-136 (Convert.FromBase64String(s))`
- fix: Do NOT use validate=True (it would fault on whitespace that C# accepts, creating a new opposite divergence). Instead mirror C#'s exact whitespace-skip then strict-decode. In native/std_lib.py base64_decode (line 362), replace `return base64.b64decode(s)` with: strip only the four chars C# ignores then validate strictly, e.g. `filtered = s.translate({0x20: None, 0x09: None, 0x0A: None, 0x0D: None}); return base64.b64decode(filtered, validate=True)`. This faults on non-whitespace invalid chars (matching C# THROW on '!') while still accepting space/tab/CR/LF (matching C#). Recommend a follow-up vector test for padding edge cases. The claim's 'QUJD ' example is a false positive (spec already matches C# there).

### [non-consensus] verifyWithECDsa missing post-Gorgon V2 (strict, throws on bad signature length) — spec returns False where C# faults
- spec: `native/crypto_lib.py:68-75 (only active_in=HF_COCKATRICE registration, no Gorgon variant) + verify_with_ecdsa returns False on len(signature)!=64 at crypto_lib.py:202-203`
- csharp: `SmartContract/Native/CryptoLib.cs:106-112 (VerifyWithECDsaV2, ActiveIn=HF_Gorgon) calling Crypto.VerifySignature which throws FormatException at Cryptography/Crypto.cs:278-279`
- fix: Add a third verifyWithECDsa registration in crypto_lib.py with active_in=Hardfork.HF_GORGON and set deprecated_in=Hardfork.HF_GORGON on the existing Cockatrice variant (so the Cockatrice variant is active only in [Cockatrice, Gorgon)). The new Gorgon handler must use STRICT semantics mirroring C# Crypto.VerifySignature: raise (fault) when len(signature) != 64 (FormatException-equivalent) instead of returning False. The unsupported-curveHash case needs no special handling in the new handler because the NamedCurveHash enum marshalling already faults on out-of-range values (matching C# V2's NotSupportedException). Do NOT change the Cockatrice variant's curveHash handling — it already faults via enum marshalling, matching C# V1 (KeyNotFoundException). The auditor's proposed change to "make the Cockatrice..Gorgon variant fault on unsupported curveHash" is unnecessary; that path already faults. This is low urgency / not production-reachable until a network schedules HF_Gorgon, but should be fixed for v3.10.0 code-level parity.

### [non-consensus] verifyWithEd25519 missing post-Gorgon V1 (throws on wrong signature/pubkey size) — spec returns False where C# faults
- spec: `native/crypto_lib.py:76-82 (single registration active_in=HF_ECHIDNA) + verify_with_ed25519 returns False on bad sizes at crypto_lib.py:282-285`
- csharp: `SmartContract/Native/CryptoLib.cs:153-166 (VerifyWithEd25519V1, ActiveIn=HF_Gorgon) throws FormatException on bad sizes; vs V0 at CryptoLib.cs:175-192 (Echidna..Gorgon) returns false`
- fix: In native/crypto_lib.py _register_methods, deprecate the existing Echidna verifyWithEd25519 at Gorgon and add a Gorgon V1 variant that faults on bad sizes. Concretely: change the current registration (lines 76-82) to add `deprecated_in=Hardfork.HF_GORGON`, and add a second `_register_method("verifyWithEd25519", self.verify_with_ed25519_v1, cpu_fee=1<<15, call_flags=CallFlags.NONE, active_in=Hardfork.HF_GORGON, manifest_parameter_names=[...])`. Implement verify_with_ed25519_v1 to raise (mirroring C# FormatException → VM fault) when len(signature)!=64 or len(pubkey)!=32, e.g. raise a ValueError/Format-style error that the engine converts to FAULT, then perform the ed25519 verification; the Echidna V0 method keeps returning False on bad sizes. This matches CryptoLib.cs:153-166 (V1 throw, ActiveIn=HF_Gorgon) and CryptoLib.cs:175-192 (V0 return false, Echidna..Gorgon).

### [non-consensus] jsonSerialize encodes ByteString as base64 instead of C# UTF-8 string
- spec: `native/std_lib.py:263-264 (_to_json_value: bytes -> base64.b64encode)`
- csharp: `SmartContract/JsonSerializer.cs:117-118 (SerializeToByteArray) and :48-52 (Serialize): ByteString/Buffer -> buffer.GetString() (UTF-8 decode)`
- fix: Serialize ByteString and Buffer stack items as their strict-UTF-8-decoded string (faulting on invalid UTF-8) to match C# JsonSerializer.SerializeToByteArray -> buffer.GetString() (StrictUTF8.GetString). Apply to BOTH spec paths: (1) smartcontract/json_serializer.py:68-76 — for BYTESTRING/BUFFER, replace `base64.b64encode(...)` with a strict-UTF-8 decode of the bytes that raises on invalid UTF-8 (e.g. `item.value.decode('utf-8')` with strict errors), and likewise `_key_to_string` line 115-116 for ByteString map keys; (2) native/std_lib.py:263-264 — replace the `isinstance(item, bytes)` base64 branch with a strict-UTF-8 decode that faults on invalid UTF-8, and fix the dict-key path (line 270) to UTF-8-decode bytes keys rather than `str(k)`. Determine which path the native jsonSerialize handler actually dispatches to and fix it; fix the other to avoid latent reuse. Add a fixture: jsonSerialize(b'ABC') == "ABC" (C#), and jsonSerialize of a non-UTF-8 ByteString must FAULT (matching StrictUTF8 ExceptionFallback). Note jsonDeserialize already correctly maps JSON strings back to ByteString(utf-8 bytes), so the round-trip is consistent only once serialize uses UTF-8.

### [non-consensus] jsonSerialize output bytes differ (separator spacing + non-ASCII escaping)
- spec: `native/std_lib.py:247-249 (json.dumps default separators ', '/': ' and ensure_ascii=True)`
- csharp: `SmartContract/JsonSerializer.cs:97-101 (Utf8JsonWriter Indented=false: no spaces, raw UTF-8 output)`
- fix: Do NOT use the claimed fix (ensure_ascii=False is wrong — C# escapes non-ASCII, it does not emit raw UTF-8). To byte-match C# Utf8JsonWriter+JavaScriptEncoder.Default, the spec must implement a custom JSON encoder, not rely on json.dumps flags: (1) compact separators `(',', ':')`; (2) keep non-ASCII escaped as `\uXXXX` (ensure_ascii stays effectively True); (3) UPPERCASE the hex digits in every `\uXXXX` escape (Python emits lowercase); (4) additionally escape the HTML-safe set that JavaScriptEncoder.Default escapes — at minimum `<`(<), `>`(>), `&`(&), `+`(+), `'`('), and write `"` as `"` rather than `\"`; preserve C#'s short escapes for control chars (\t,\n,\r,\b,\f) and surrogate-pair form for astral code points. Easiest correct approach: post-process json.dumps output (separators=(',',':'), ensure_ascii=True) to uppercase \u hex and apply the extra HTML-char escapes, OR port the JavaScriptEncoder.Default escaping table directly. (Also, separately, fix _to_json_value to UTF-8-decode bytes like C# GetString() instead of base64.)

### [non-consensus] recoverSecp256K1 rejects 65-byte recovery ids 2 and 3 that C# accepts
- spec: `native/crypto_lib.py:301-319 (65-byte: v=signature[64]; if v>=27 v-=27; if v not in (0,1) return None)`
- csharp: `SmartContract/Native/CryptoLib.cs:42-58 (RecoverSecp256K1 -> Crypto.ECRecover) and Cryptography/Crypto.cs:417-421 (recId = v>=27 ? v-27 : v; valid range [0..3])`
- fix: In native/crypto_lib.py recover_secp256k1, for the 65-byte branch change the guard `if v not in (0, 1): return None` (line 318) to accept normalized recId in [0..3]: `if v not in (0, 1, 2, 3): return None`. The existing math already handles the decomposition correctly — x = r + (v >> 1) * n (line 330) matches C# iPart = recId>>1, and (y % 2) != (v & 1) (line 338) matches C# yBit = recId&1. Keep the existing `if x >= p: return None` check (lines 331-332), which mirrors C#'s `x.CompareTo(s_prime) >= 0` fault, so common-case recId 2/3 inputs (x = r+n >= p) still return None on both sides; only the rare r < p-n window now recovers a key as C# does. The 64-byte EIP-2098 branch is unaffected (C# also restricts compact format to recId {0,1} at Crypto.cs:440).

## native-gas-fungible

### [non-consensus] GAS OnPersist guards network-fee mint with `> 0` instead of always minting; negative totalNetworkFee halts instead of faulting
- spec: `src/neo/native/gas_token.py:71-77`
- csharp: `neo_csharp/src/Neo/SmartContract/Native/GasToken.cs:55-57`
- fix: Remove the `if total_network_fee > 0:` guard in src/neo/native/gas_token.py:71. Always compute validators/primary and call `self.mint(engine, primary, total_network_fee, False)` unconditionally. The spec's `mint` already early-returns on amount==0 (fungible_token.py:133) and raises ValueError on amount<0 (fungible_token.py:131-132), so the unconditional call faults on negative totalNetworkFee exactly like C#'s Mint, while remaining a no-op at zero. This makes the spec byte-for-byte path-equivalent to C# GasToken.OnPersistAsync for all three sign cases. (Low practical impact: negative totalNetworkFee is unreachable for valid blocks; this is parity hardening for malformed-block handling.)

## native-ledger

### [consensus] IsTraceableBlock gating omitted from every ledger getter
- spec: `native/ledger.py:179-246 (get_block, get_transaction, get_transaction_height, get_transaction_signers, get_transaction_vm_state, get_transaction_from_block)`
- csharp: `SmartContract/Native/LedgerContract.cs:263,351,359,367,375,391`
- fix: Add private IsTraceableBlock(engine, index): currentIndex equals current_index(snapshot); mtb equals Policy.get_max_traceable_blocks(snapshot) under HF_Echidna else ProtocolSettings.MaxTraceableBlocks; return index leq currentIndex and index plus mtb gt currentIndex. Gate each getter: get_block returns None when block index not traceable; get_transaction and get_transaction_signers return None, get_transaction_vm_state returns 0 NONE, get_transaction_height returns minus1 when not traceable on state.block_index; get_transaction_from_block returns None when resolved block index not traceable.

### [consensus] getTransactionFromBlock out-of-range txIndex returns null instead of faulting
- spec: `native/ledger.py:226-227,244-246`
- csharp: `SmartContract/Native/LedgerContract.cs:392-393`
- fix: Raise an out of range fault for negative or too large txIndex after block resolution in ledger.py instead of returning None

### [consensus] on_persist does not store Conflicts attribute records
- spec: `native/ledger.py:299-308`
- csharp: `SmartContract/Native/LedgerContract.cs:59-70`
- fix: In neo/native/ledger.py on_persist (after writing each real tx state at ~line 308), iterate the transaction's Conflicts attributes (TransactionAttributeType.CONFLICTS == 0x21) and, for each, write a dummy TransactionState(block_index=block.index, transaction=None) under Prefix_Transaction+attr.hash, and the same dummy under Prefix_Transaction+attr.hash+signer.account for every signer of the tx. Use the get_and_change/FromReplica-equivalent so a pre-existing malicious stub is overwritten. Crucially the stub must have transaction=None (matching C# new TransactionState(){BlockIndex=...}) so contains_transaction (which requires transaction is not None) still returns False for it, while a ContainsConflictHash-style reader returns True. Separately, transaction_verifier._has_conflicts (transaction_verifier.py:122) should be switched from contains_transaction to a ContainsConflictHash-equivalent (signer-keyed) reader for full behavioral parity, though the state-root fix is the on_persist write itself.

### [consensus] Block stored as full block instead of TrimmedBlock; getBlock returns full block bytes
- spec: `native/ledger.py:296-297 (stores _serialize_block(block)), 190-192 (get_block returns raw stored value)`
- csharp: `SmartContract/Native/LedgerContract.cs:51 (stores TrimmedBlock.Create(block).ToArray()), 247,262 (reads/returns TrimmedBlock)`
- fix: Store a TrimmedBlock serialization under PREFIX_BLOCK in on_persist instead of the full block, and have the contract-callable get_block return the TrimmedBlock stack item rather than raw full-block bytes.

### [non-consensus] GetTransactionState does not suppress conflict stubs (Transaction == null)
- spec: `native/ledger.py:171-177`
- csharp: `SmartContract/Native/LedgerContract.cs:332-333`
- fix: In ledger.py:171-177 make get_transaction_state mirror C# state?.Transaction is null ? null : state: after state = TransactionState.from_bytes(item.value), return state if state.transaction is not None else None. This hides conflict stubs from get_transaction_height (returns -1), get_transaction_vm_state (NONE), get_transaction_signers, and get_transaction, matching LedgerContract.cs:333. Separately, for true consensus reachability the spec also needs the missing conflict-stub write path in on_persist (C# OnPersistAsync lines 59-70: for each Conflicts attribute write a BlockIndex-only TransactionState at Prefix_Transaction|attr.Hash and at Prefix_Transaction|attr.Hash|signer per signer).

## native-neo

### [consensus] PostPersist GAS distribution to committee + voter-reward accumulation entirely missing
- spec: `src/neo/native/neo_token.py:612-647 (no post_persist override)`
- csharp: `SmartContract/Native/NeoToken.cs:253-284 (PostPersistAsync)`
- fix: Add a NeoToken.post_persist(engine) override mirroring C# PostPersistAsync (NeoToken.cs:253-284): compute m=committee_members_count, n=validators_count, index=block.index % m, gas_per_block=get_gas_per_block(snapshot); mint gas_per_block*COMMITTEE_REWARD_RATIO//100 GAS to the script hash of committee[index] pubkey via GAS.mint(engine, account, amount, False). Then, if ShouldRefreshCommittee(block.index, m) (i.e. (index+1) % m == 0, matching C# semantics), compute voter_reward_of_each_committee = gas_per_block*VOTER_REWARD_RATIO*VOTE_FACTOR*m // (m+n) // 100 and, for each committee member with votes>0, add factor*voter_reward_of_each_committee//votes (factor=2 for index<n else 1) into the Prefix_VoterRewardPerCommittee (23) StorageItem (GetAndChange, default BigInteger.Zero). Use BigInteger-equivalent (Python int) arithmetic with floor division to match C# integer truncation exactly, reading the committee from the cached/serialized committee storage in the same sorted order.

### [consensus] OnBalanceChanging not overridden: candidate votes, voters count, and GAS distribution not updated on NEO balance change
- spec: `src/neo/native/neo_token.py (no _on_balance_changing override); base no-op at src/neo/native/fungible_token.py:236-240`
- csharp: `SmartContract/Native/NeoToken.cs:87-102 (OnBalanceChanging)`
- fix: Implement the full GAS-distribution / vote-accounting machinery in NeoToken (src/neo/native/neo_token.py), mirroring C# NeoToken.cs:

1. Add a _distribute_gas(engine, account, state) helper (mirrors DistributeGas, NeoToken.cs:124-144): if engine.persisting_block is None return None; datoshi = self._calculate_bonus(snapshot, state, persisting_block.index); set state.balance_height = persisting_block.index; if state.vote_to is not None: state.last_gas_per_vote = int(snapshot.get(Prefix_VoterRewardPerCommittee||vote_to)) or 0; return (account, datoshi) if datoshi != 0 else None. Queue the distribution on the current execution context (engine state list).

2. Override _on_balance_changing in NeoToken (mirrors NeoToken.cs:87-102): call _distribute_gas and queue the result; if amount != 0 and state.vote_to is not None: voters_count.add(amount); candidate = get_and_change(Prefix_Candidate||vote_to); candidate.votes += amount; _check_candidate(...).

3. Override _post_transfer (or add a post-transfer mint step) to mint each queued GasDistribution via the GAS contract (mirrors PostTransferAsync, NeoToken.cs:104-110).

4. Update vote_internal to call _distribute_gas + update last_gas_per_vote (set to latest gas-per-vote when voting to a new candidate, else 0) and mint the distribution, mirroring C# VoteInternal (NeoToken.cs:485,494-516).

5. Add post_persist to NeoToken (mirrors PostPersistAsync, NeoToken.cs:253-284): mint committee member's per-block reward (gasPerBlock*CommitteeRewardRatio/100) and, when ShouldRefreshCommittee, accumulate voterRewardOfEachCommittee = gasPerBlock*VoterRewardRatio*VoteFactor*m/(m+n)/100 into Prefix_VoterRewardPerCommittee for each committee member with votes>0 (validators double-weighted, factor=2 for index<n).

Note: the auditor's proposed fix covers only items 1-2-3 partially; items 4 and 5 (vote-path GAS mint and PostPersist voter-reward accumulation) are also required, otherwise Prefix_VoterRewardPerCommittee stays unwritten and vote rewards remain zero.

### [consensus] unclaimed_gas: missing end==expectEnd validation (different fault-vs-halt)
- spec: `src/neo/native/neo_token.py:314-321`
- csharp: `SmartContract/Native/NeoToken.cs:357-365`
- fix: In src/neo/native/neo_token.py unclaimed_gas (line 314), before reading the account state, compute expect_end = engine.persisting_block.index if engine.persisting_block is not None else (Ledger.current_index(engine.snapshot) + 1), and raise an out-of-range error (the spec's mapped-to-VM-FAULT exception type, e.g. ValueError/the engine's argument-out-of-range equivalent) when end != expect_end. This must occur before the PREFIX_ACCOUNT storage lookup, mirroring C# NeoToken.cs:359-360 (expectEnd = PersistingBlock?.Index ?? Ledger.CurrentIndex(snapshot)+1; ThrowIfNotEqual(end, expectEnd)). Use the same persisting-block-or-current-index+1 fallback so behavior matches whether or not a persisting block is set.

### [consensus] _calculate_bonus uses a single gas_per_block value instead of summing historical GasPerBlock records
- spec: `src/neo/native/neo_token.py:323-353 (uses get_gas_per_block * blocks)`
- csharp: `SmartContract/Native/NeoToken.cs:155-189 (CalculateReward / GetSortedGasRecords)`
- fix: In neo_token.py:_calculate_bonus, replace the single-value neo_holder_reward computation with a backward scan over PREFIX_GAS_PER_BLOCK records mirroring C# CalculateReward/GetSortedGasRecords: iterate records with index <= (end - 1) in descending index order; maintain a moving `end` cursor and accumulate sum_gas_per_block += gas_per_block_i * (cur_end - index) while index > start, setting cur_end = index; on the first record with index <= start, add gas_per_block_i * (cur_end - start) and stop. Then neo_holder_reward = state.balance * sum_gas_per_block * NEO_HOLDER_REWARD_RATIO // 100 // self._total_amount. Use end-1 as the seek boundary (matching GetSortedGasRecords(snapshot, end-1)), not end. Leave the vote_reward branch unchanged.

### [consensus] _refresh_committee ignores EffectiveVoterTurnout / candidate-count fallback to StandbyCommittee
- spec: `src/neo/native/neo_token.py:619-646`
- csharp: `SmartContract/Native/NeoToken.cs:622-635 (ComputeCommitteeMembers)`
- fix: In src/neo/native/neo_token.py, rewrite the committee computation to mirror C# ComputeCommitteeMembers: read votersCount from PREFIX_VOTERS_COUNT, compute voter_turnout = votersCount / TOTAL_AMOUNT, and if voter_turnout < EFFECTIVE_VOTER_TURNOUT OR len(candidates) < committee_members_count, use protocol_settings.standby_committee (each member with its current votes, defaulting to 0 when not a candidate) preserving StandbyCommittee order; otherwise use candidates sorted by (votes desc, pubkey) taking the top committee_members_count. In on_persist/_refresh_committee, capture the previous committee public keys before overwriting, and after recomputing, if HF_Cockatrice is enabled and the new membership != previous, emit send_notification(self.hash, "CommitteeChanged", [old_array, new_array]). Also fix initialize() to seed PREFIX_COMMITTEE with the StandbyCommittee (pubkey + votes=0 for each) instead of an empty StorageItem, matching C# InitializeAsync.

### [consensus] _refresh_committee/get_candidates omit Policy.IsBlocked filtering of candidates
- spec: `src/neo/native/neo_token.py:497-506 (get_candidates) and 619-646`
- csharp: `SmartContract/Native/NeoToken.cs:547-554 (GetCandidatesInternal)`
- fix: In neo_token.py, add a blocked-account exclusion to the candidate iteration so it flows to get_candidates, get_all_candidates, and _refresh_committee. In get_candidates (around line 501-505), after confirming state.registered, also skip the candidate when Policy is_blocked is true for the candidate's signature-redeem-script hash: compute script = Contract.create_signature_redeem_script(pubkey) (single-key CHECKSIG redeem script), then account_hash = UInt160(hash160(script)), and skip if PolicyContract.is_blocked(snapshot, account_hash). This requires resolving the Policy native contract from the snapshot (as C# does via Policy.IsBlocked). The hash must be the signature-redeem-script ToScriptHash of the PublicKey, NOT the raw pubkey bytes. This single change propagates to get_all_candidates (delegates to get_candidates) and _refresh_committee (calls get_candidates), matching C# GetCandidatesInternal at NeoToken.cs:553.

### [consensus] vote_internal does not update LastGasPerVote / does not queue GAS distribution
- spec: `src/neo/native/neo_token.py:442-495`
- csharp: `SmartContract/Native/NeoToken.cs:464-516 (VoteInternal)`
- fix: In neo_token.py vote_internal, mirror C# VoteInternal: (1) before mutating votes, call a distribute-gas equivalent that computes the bonus via _calculate_bonus at engine.persisting_block.index, sets state.balance_height = persisting_block.index, and (if state.vote_to is not None) sets state.last_gas_per_vote = the current Prefix_VoterRewardPerCommittee value of state.vote_to (default 0); (2) after determining the new target, when vote_to_bytes is not None and != old target set state.last_gas_per_vote = latest VoterRewardPerCommittee of the new target, else (clearing the vote) set state.last_gas_per_vote = 0; (3) at the end, if the distribute step produced a non-zero amount, mint that GAS to the account via the GAS token (call-on-payment true), exactly as C# lines 485,494-499,507-514. Note: the spec also lacks the equivalent gas distribution in the balance-change/transfer path (no on_balance_changing), which should be addressed separately for full parity, but is outside this unit.

### [consensus] onNEP17Payment missing amount==registerPrice check, candidate registration, and GAS burn
- spec: `src/neo/native/neo_token.py:578-588`
- csharp: `SmartContract/Native/NeoToken.cs:374-389 (OnNEP17Payment)`
- fix: Implement the full Echidna onNEP17Payment in spec neo_token.py on_nep17_payment (after the existing GAS-caller check): (1) raise (ArgumentException equivalent) if amount != self.get_register_price(engine.snapshot); (2) decode pubkey = ECPoint.decode(data span, Secp256r1); (3) call the internal register routine (factor out register_candidate's body into a register_internal that does witness check + CandidateState create/update + CandidateStateChanged notification) and raise if it returns False; (4) burn the received GAS: get the GasToken contract and call its burn(engine, NEO.Hash, amount). Ensure faults are thrown (abort) on amount mismatch, decode failure, and register failure to match C#'s throw-vs-silent-halt semantics. Keep the method gated to HF_Echidna behavior consistent with C# (the no-op caller-only check is the correct pre-Echidna shape; the registration+burn is the Echidna shape).

### [non-consensus] get_gas_per_block returns wrong record (lowest index, not the record effective at current height)
- spec: `src/neo/native/neo_token.py:271-283`
- csharp: `SmartContract/Native/NeoToken.cs:309-312, 341-347`
- fix: Make get_gas_per_block height-relative like C#. Change its signature/call sites to obtain the current ledger height (equivalent to Ledger.CurrentIndex(snapshot)+1), then seek BACKWARD over Prefix_GasPerBlock from key Prefix_GasPerBlock|(height) to the prefix boundary and return the value of the first (highest index <= height) record, defaulting to 5 GAS only if none. Concretely, in src/neo/native/neo_token.py:271-283 replace the forward-overwrite loop with a backward range scan bounded by current_index+1, mirroring C# GetSortedGasRecords(snapshot, CurrentIndex+1).First(). As a follow-up (separate from this claim), align _calculate_bonus (neo_token.py:323-340) with C# CalculateReward's piecewise windowed sum over GetSortedGasRecords so multiple GasPerBlock rate changes are accounted for.

### [non-consensus] get_committee returns committee in stored order instead of OrderBy(pubkey)
- spec: `src/neo/native/neo_token.py:524-530`
- csharp: `SmartContract/Native/NeoToken.cs:576-579 (GetCommittee)`
- fix: In src/neo/native/neo_token.py get_committee (line 530), sort the parsed committee ascending by public key before returning, matching C# `.OrderBy(p => p)`. Keep the stored cache in votes-descending order (used by get_next_block_validators which takes the top-N before its own sort). Concretely: `return sorted(self._parse_committee(item.value))`. Do NOT change _refresh_committee storage order (it must stay votes-descending so get_next_block_validators selects the highest-voted N before re-sorting). get_committee_address and get_next_block_validators need no change.

## native-notary-framework

### [consensus] Notary deposit storage encoding: VarInt+4byte vs Struct IInteroperable
- spec: `native/notary.py:57-71`
- csharp: `SmartContract/Native/Notary.cs:340-356`
- fix: Replace the hand-rolled Deposit.serialize/deserialize (native/notary.py:57-71) with C#'s BinarySerializer Struct-of-Integers encoding. serialize(): emit 0x41 (Struct), VarInt(2), then for each of [Amount, Till] emit 0x21 (Integer) + WriteVarBytes(value's signed two's-complement little-endian bytes, empty when the value is zero). deserialize(): parse the Struct via the same StackItem binary deserializer (type 0x41, count, two Integer elements), with Till coerced as (uint) of the parsed BigInteger to match C#'s (uint)@struct[1].GetInteger() truncation. Prefer routing through the existing StackItem BinarySerializer used elsewhere in the spec rather than re-implementing per-field, to guarantee zero/empty-bytes and varint-length parity. The 4-byte fixed Till and the VarInt-amount layout must both be removed since the raw bytes determine the state root.

### [consensus] Notary.verify omits NotaryAssisted attribute, self-signer scope, and sender-deposit-sufficiency checks
- spec: `native/notary.py:132-181`
- csharp: `SmartContract/Native/Notary.cs:112-135`
- fix: In native/notary.py verify(), after the len(signature)==64 check and before fetching/verifying against notary nodes, add the three C# preconditions against the script-container transaction:
1. Resolve tx = engine.script_container as a Transaction; if tx is None OR tx has no attribute of type 0x22 (NOTARY_ASSISTED) -> return False (mirror Notary.cs:115-116).
2. Iterate tx.signers; if any signer.account == self.hash and signer.scopes != WitnessScope.None -> return False; break on the matching signer (mirror Notary.cs:117-124).
3. If tx.sender == self.hash: if len(tx.signers) != 2 -> return False; payer = tx.signers[1].account; deposit = self._get_deposit(snapshot, payer); if deposit is None or deposit.amount < (tx.network_fee + tx.system_fee) -> return False (mirror Notary.cs:125-131).
Only after these pass, proceed to GetNotaryNodes + signature verification, and use the transaction's GetSignData(network) as the verified message (Notary.cs:133) rather than the bare tx hash for full parity.

### [consensus] Notary.verify uses container hash instead of tx.GetSignData(network) as signed message
- spec: `native/notary.py:168-174`
- csharp: `SmartContract/Native/Notary.cs:133-134`
- fix: In native/notary.py:168, replace `message = getattr(engine.script_container, 'hash', None)` (bare tx hash) with the C# GetSignData preimage and verify over it: compute `sign_data = struct.pack('<I', engine.get_network()) + bytes(tx.hash)` (network magic uint32 LE concatenated with the 32-byte tx hash), then SHA-256 it before passing to verify_signature (since verify_signature uses Prehashed and does not hash). Concretely: `digest = sha256(sign_data)` and call `verify_signature(digest, signature, pubkey_bytes, SECP256R1)`. Equivalently, add a transaction `get_sign_data(network)` helper mirroring C# Helper.GetSignData and feed `sha256(get_sign_data(network))` to verify_signature. Also obtain the network magic from the engine/protocol settings rather than hardcoding. (Separately, for full parity, add the missing NotaryAssisted-attribute, signer-scope-None, and Sender==Hash deposit-sufficiency checks from Notary.cs:115-131.)

### [consensus] Notary.onNEP17Payment missing till lower-bound, first-deposit minimum, allowedChangeTill, and exact 2-element data validation
- spec: `native/notary.py:305-352`
- csharp: `SmartContract/Native/Notary.cs:146-179`
- fix: Port the full C# OnNEP17Payment into spec native/notary.py on_nep17_payment: (1) require `data` be an array of exactly 2 elements else raise FormatException (remove the len>=2 tolerance); (2) to=from, override with data[0] only if not Null, deserialize as UInt160; (3) till = uint truncation of int(data[1]); (4) allowedChangeTill = (tx.Sender == to); (5) currentHeight = Ledger.CurrentIndex; raise if till < currentHeight+2; (6) load deposit (GetAndChange-equivalent); raise if deposit is not None and till < deposit.till; (7) if deposit is None: feePerKey = Policy.GetAttributeFeeV1(NotaryAssisted); raise if amount < 2*feePerKey; deposit = Deposit(0,0); if not allowedChangeTill: till = currentHeight + 5760 (DEFAULT_DEPOSIT_DELTA_TILL); (8) elif not allowedChangeTill: till = deposit.till; (9) deposit.amount += amount; deposit.till = till (UNCONDITIONAL); put deposit. Also remove the spec-only `amount <= 0` check to match C# exactly (or keep as a harmless invariant, but it is not in C#).

### [consensus] Notary.withdraw mints GAS instead of transferring, and uses wrong expiry comparison
- spec: `native/notary.py:227-279`
- csharp: `SmartContract/Native/Notary.cs:238-251`
- fix: In notary.py withdraw (lines 272-277), replace the GasToken.mint call with a GAS transfer FROM the Notary contract hash TO the recipient, and fault on failure to match C#: gas.transfer(engine, self.hash, receive, amount, None) (with self.hash as the calling/from account so the native witness path applies), and raise an error (e.g. RuntimeError) if the transfer returns False, mirroring C#'s InvalidOperationException at Notary.cs:248. Do NOT change PREFIX_TOTAL_SUPPLY. Leave the expiry-rejection condition as-is (it is equivalent to C#); separately consider whether block_index should derive from Ledger.CurrentIndex rather than persisting_block.index, but that is a distinct lower-priority item not required by this fix.

### [consensus] Notary.setMaxNotValidBeforeDelta uses wrong bounds (only value<1) instead of [ValidatorsCount, maxVUBIncrement/2]
- spec: `native/notary.py:289-303`
- csharp: `SmartContract/Native/Notary.cs:270-281`
- fix: In native/notary.py:289-303, replace the `if value < 1` guard with C#-equivalent bounds. Compute max_vub_increment from the snapshot via the same Echidna-gated path C# uses (NeoSystemExtensions.GetMaxValidUntilBlockIncrement: pre-Echidna -> settings.MaxValidUntilBlockIncrement, post-Echidna -> Policy.get_max_valid_until_block_increment(snapshot)), and use the engine's actual ProtocolSettings.ValidatorsCount (NOT a default/constant). Then raise (FormatException-equivalent) when `value > max_vub_increment // 2 or value < validators_count`, BEFORE the committee check and store. Keep ordering: bounds-check -> check_committee -> store, matching Notary.cs:273-280. (The existing uint decode already rejects negatives/0 if the binding mirrors C#'s `uint value`.)

### [consensus] Notary missing OnPersist (deposit fee deduction + notary reward minting)
- spec: `native/notary.py:73-371`
- csharp: `SmartContract/Native/Notary.cs:61-90`
- fix: Implement Notary.on_persist in /Users/jinghuiliao/git/r3e/neo-execution-specs/src/neo/native/notary.py mirroring C# Notary.OnPersistAsync (Notary.cs:61-90), gated to active hardfork (HF_Echidna+):

1. n_fees = 0; notaries = None. Iterate engine.persisting_block.transactions.
2. For each tx with a NotaryAssisted attribute (type 0x22): lazily fetch notaries = RoleManagement.get_designated_by_role(snapshot, Role.P2P_NOTARY, Ledger.current_index(snapshot)+1) (matches GetNotaryNodes which uses CurrentIndex+1, Notary.cs:288-291); n_fees += attr.n_keys + 1.
3. If tx.sender == self.hash: payer = tx.signers[1].account; deposit = self._get_deposit(snapshot, payer); if deposit is not None: deposit.amount -= tx.system_fee + tx.network_fee; if deposit.amount == 0: self._remove_deposit(snapshot, payer); else self._put_deposit(snapshot, payer, deposit). (C# uses GetAndChange + Sign==0; replicate the write-back since the spec's _get_deposit returns a copy.)
4. If n_fees == 0 or notaries is None: return.
5. single_reward = n_fees * Policy.get_attribute_fee(snapshot, 0x22) // len(notaries)  (Policy.GetAttributeFeeV1, integer division/truncation toward zero as in C# long division — verify the spec uses floor-toward-zero for the positive operands here; values are non-negative so Python // is fine).
6. For each notary pubkey: mint single_reward GAS to Contract.create_signature_redeem_script(notary).to_script_hash() via GasToken.mint(engine, hash, single_reward, call_on_payment=False).

Also ensure the spec's native on_persist dispatch (application_engine.py:1988-1993) respects contract activation, or guard inside Notary.on_persist with self.is_hardfork_enabled(...)/_contract_activations, so the logic only runs once Notary is active. Use the existing _get_deposit/_put_deposit/_remove_deposit helpers and the same get_attribute_fee path already used by gas_token.py:88-94.

### [non-consensus] Notary.lockDepositUntil missing till >= CurrentIndex+2 lower bound
- spec: `native/notary.py:193-225`
- csharp: `SmartContract/Native/Notary.cs:189-199`
- fix: In lock_deposit_until (native/notary.py), after obtaining the snapshot and before (or right where) the deposit checks occur, add the C# lower-bound guard: compute the current ledger index (equivalent of Ledger.CurrentIndex(snapshot)) and `if till < current_index + 2: return False`. To mirror C# ordering exactly, place this check immediately after the witness check and before the deposit fetch. Comment per C#: deposit must be valid at least until the next block after the persisting block.

### [non-consensus] Notary.initialize seeds default unconditionally rather than only at ActiveIn hardfork
- spec: `native/notary.py:121-130`
- csharp: `SmartContract/Native/Notary.cs:52-59`
- fix: Gate notary.py 121 write on Echidna per Notary.cs 54-57; also add the missing ContractManagement OnPersist dispatch calling initialize per hardfork.

## native-policy

### [consensus] setExecFeeFactor uses pre-Faun range/units unconditionally (rejects valid post-Faun pico values, stores wrong magnitude)
- spec: `native/policy.py:258-272`
- csharp: `neo_csharp/src/Neo/SmartContract/Native/PolicyContract.cs:513-524`
- fix: Add a Faun branch so the validation max is FeeFactor times MaxExecFeeFactor when Faun is enabled and store the raw value

### [consensus] getAttributeFee/setAttributeFee miss attribute-type validation and the V0/V1 NotaryAssisted gating
- spec: `native/policy.py:79-85,147-153,467-496`
- csharp: `neo_csharp/src/Neo/SmartContract/Native/PolicyContract.cs:265-301,459-501`
- fix: In native/policy.py, replace the single get_attribute_fee/set_attribute_fee with C#-faithful per-era handlers. Add a shared internal helper that, given (attribute_type, allow_notary_assisted), faults (raises the engine fault exception) when attribute_type is not one of the defined TransactionAttributeType values {0x01,0x11,0x20,0x21,0x22} OR (not allow_notary_assisted and attribute_type == 0x22) — mirroring C# PolicyContract.cs:291-297 / 487-493. Keep the existing value-range check in set (value>MAX_ATTRIBUTE_FEE). Then dual-register both methods: getAttributeFee/setAttributeFee V0 with deprecated_in=Hardfork.HF_ECHIDNA and allow_notary_assisted=False, and V1 with active_in=Hardfork.HF_ECHIDNA and allow_notary_assisted=True (the spec's _register_method already supports active_in/deprecated_in and lookup_method picks the active variant). This reproduces C#'s [ContractMethod(true, HF_Echidna,...)] V0 and [ContractMethod(HF_Echidna,...)] V1 behavior exactly.

### [non-consensus] ExecFeeFactor not stored as pico-GAS post-Faun (state-root divergence + missing Faun migration)
- spec: `native/policy.py:241-272,750-760`
- csharp: `neo_csharp/src/Neo/SmartContract/Native/PolicyContract.cs:152-167,187-193,514-524`
- fix: Valid as proposed, to achieve byte-exact native-storage/state-root parity: add an HF_Faun migration that multiplies the stored Prefix_ExecFeeFactor by FeeFactor (10000) at the Faun boundary (mirroring InitializeAsync, PolicyContract.cs:152-158), so post-Faun the stored value is in pico units (300000); make get_exec_fee_factor (policy.py:249-251) divide the stored value by FeeFactor when Faun is enabled (mirroring GetExecFeeFactor, PolicyContract.cs:189-192); make get_exec_pico_fee_factor return the raw stored value rather than stored*10000 (policy.py:256 → PolicyContract.cs:211). This requires the spec to gain a per-hardfork initialize hook (initialize(engine, hardfork)) that is actually invoked at hardfork boundaries — which it currently lacks. Separately (recommended), gate set_exec_fee_factor's upper bound on Faun (max = FeeFactor*MAX_EXEC_FEE_FACTOR post-Faun) and store the raw pico value, per PolicyContract.cs:514-523. Because no observable read/gas/fault changes today, this is low priority unless/until the spec compares byte-exact native state roots against C#.

### [non-consensus] setMillisecondsPerBlock does not emit MillisecondsPerBlockChanged notification
- spec: `native/policy.py:307-320`
- csharp: `neo_csharp/src/Neo/SmartContract/Native/PolicyContract.cs:438-450`
- fix: In native/policy.py set_milliseconds_per_block, after the committee check and BEFORE the storage write, capture old = self.get_milliseconds_per_block(engine.snapshot); perform item.set(value); then emit engine.send_notification(self.hash, "MillisecondsPerBlockChanged", [old, value]) with both as integer stack items, in the order [old, new], matching C# PolicyContract.cs:448-449 and the registered event params. Read old before writing so the pre-change value is captured (mirrors C#'s GetMillisecondsPerBlock called before GetAndChange.Set).

### [non-consensus] setWhitelistFeeContract / removeWhitelistFeeContract do not emit WhitelistFeeChanged notification
- spec: `native/policy.py:557-592`
- csharp: `neo_csharp/src/Neo/SmartContract/Native/PolicyContract.cs:345-366,400-429`
- fix: In native/policy.py, after the storage mutation in each method, emit the notification matching C# arg shape (the WhitelistFeeChanged event is already declared and engine.send_notification already works):

set_whitelist_fee_contract (after item.set(fixed_fee), ~line 577):
  if hasattr(engine, "send_notification"):
      engine.send_notification(self.hash, "WhitelistFeeChanged", [contract_hash.data, method, arg_count, fixed_fee])

remove_whitelist_fee_contract (after engine.snapshot.delete(key), ~line 592):
  if hasattr(engine, "send_notification"):
      engine.send_notification(self.hash, "WhitelistFeeChanged", [contract_hash.data, method, arg_count, None])  # None -> StackItem.Null for the fee slot

Use the same ByteString(contract_hash) encoding the spec uses for Hash160 notification args (mirror how recover_fund passes account), and ensure the fee element serializes to StackItem.Null on remove. No gas/state changes are needed beyond adding the notifications.

### [non-consensus] Echidna policy keys seeded at genesis from constants instead of at HF_Echidna from ProtocolSettings
- spec: `native/policy.py:767-780`
- csharp: `neo_csharp/src/Neo/SmartContract/Native/PolicyContract.cs:144-150`
- fix: Give the native initialize dispatch a hardfork parameter (mirroring C# InitializeAsync(engine, hardfork?) driven by ContractManagement.OnPersist per active hardfork). In PolicyContract: at genesis (hardfork == ActiveIn/None) seed ONLY FeePerByte/ExecFeeFactor/StoragePrice. Move the MillisecondsPerBlock/MaxValidUntilBlockIncrement/MaxTraceableBlocks seeds to an HF_Echidna-gated branch, sourcing values from engine.ProtocolSettings.{milliseconds_per_block,max_valid_until_block_increment,max_traceable_blocks} instead of DEFAULT_* constants. Add the missing seed Prefix_AttributeFee[NotaryAssisted=0x22] = DefaultNotaryAssistedAttributeFee (10000000) in that same HF_Echidna branch. (Matches PolicyContract.cs:138-150.)

### [cosmetic] recoverFund RequiredCallFlags is States|AllowNotify instead of CallFlags.All
- spec: `native/policy.py:132-138`
- csharp: `neo_csharp/src/Neo/SmartContract/Native/PolicyContract.cs:630-631`
- fix: Optional metadata-accuracy fix: change spec native/policy.py:136 recoverFund registration from call_flags=CallFlags.STATES | CallFlags.ALLOW_NOTIFY to call_flags=CallFlags.ALL (= STATES | ALLOW_CALL | ALLOW_NOTIFY) to match C# PolicyContract.cs:630 RequiredCallFlags = CallFlags.All. This has no effect on the serialized manifest (safe stays false) or on current execution (the spec's _invoke_nep17_method bypasses flag masking), so it is purely descriptor-value alignment. A genuine behavioral fix would additionally require the spec to route recoverFund's balanceOf/transfer calls through the engine's _call_contract_internal path so that required_call_flags actually constrains sub-calls as in C#; absent that, the flag value is inert.

## native-role-oracle

### [consensus] designateAsRole missing duplicate-pubkey rejection (v3.10.0 new check)
- spec: `native/role_management.py:129-168`
- csharp: `SmartContract/Native/RoleManagement.cs:81-82`
- fix: In role_management.py designate_as_role, after the non-empty check (line 149) and before sorting (line 162), reject duplicates by comparing encoded forms: encoded = [n.encode(compressed=True) for n in nodes]; if len(set(encoded)) != len(encoded): raise (fault) with message "Duplicate publickeys are not allowed". This is unconditional (not hardfork-gated), matching C# v3.10.0 RoleManagement.cs:81-82. Optionally also add the C# upper-bound check `len(nodes) > 32 -> fault` (RoleManagement.cs:67-68) to fully match the "between 1 and 32" validation.

### [consensus] designateAsRole missing upper-bound nodes count check (>32)
- spec: `native/role_management.py:146-149`
- csharp: `SmartContract/Native/RoleManagement.cs:67-68`
- fix: In role_management.py designate_as_role, change the empty check at lines 148-149 to also enforce the upper bound, matching C# RoleManagement.cs:67-68. Replace `if not nodes: raise ValueError("Nodes list must not be empty")` with `if len(nodes) == 0 or len(nodes) > 32: raise ValueError(f"Nodes count {len(nodes)} must be between 1 and 32")`. Place this check before the committee assertion to match C# ordering (C# checks nodes bounds at line 67 before AssertCommittee at line 71). The raise must produce a VM FAULT in the spec's engine so the result matches C#.

### [consensus] designateAsRole missing 'Role already designated' guard
- spec: `native/role_management.py:164-168`
- csharp: `SmartContract/Native/RoleManagement.cs:78-79`
- fix: In neo-execution-specs/src/neo/native/role_management.py designate_as_role, before the put (around line 164-168), compute the storage key first and FAULT if it already exists, mirroring C# RoleManagement.cs:78-79: e.g. `if snapshot.contains(key): raise <fault-equivalent>("Role already designated")`. Build `key` before the existence check and reuse it for the put. Equivalently, switch snapshot.put(key, data) to snapshot.add(key, data) (data_cache.py add() already raises on an existing key), which reproduces C#'s SnapshotCache.Add semantics. Recommended to also add, in the same handler, the two adjacent missing guards to fully match C#: reject `len(nodes) > 32` (C# line 67) and reject duplicate public keys (`len(set) != len(nodes)`, C# lines 81-82). Ensure the raised exception maps to a VM FAULT, not a Python-level error that HALTs differently.

### [consensus] designateAsRole defaults block index to 0 when persisting block is null instead of faulting
- spec: `native/role_management.py:157-159`
- csharp: `SmartContract/Native/RoleManagement.cs:73-74`
- fix: In native/role_management.py designate_as_role, replace the index-0 default with a fault when the persisting block is absent, mirroring C# RoleManagement.cs:73-74. Concretely, after obtaining snapshot, do: `block = getattr(snapshot, "persisting_block", None); if block is None: raise InvalidOperationException("Persisting block is null")` (use the spec's fault/VMException type so it FAULTs rather than HALTs), then compute `block_index = block.index`. This makes the stored key use index = block.index + 1 only when a real persisting block exists, and faults otherwise — identical to C# v3.10.0.

### [consensus] Designation storage key index uses little-endian instead of big-endian
- spec: `native/role_management.py:164-166`
- csharp: `SmartContract/Native/RoleManagement.cs:77; SmartContract/KeyBuilder.cs:118 (AddBigEndian(uint))`
- fix: In role_management.py, encode the designation index suffix as 4-byte BIG-endian on write (line 165: change (block_index + 1).to_bytes(4, "little") to .to_bytes(4, "big")) and decode big-endian on read (line 120: change int.from_bytes(suffix[:4], "little") to "big"). This makes the persisted storage keys byte-identical to C# StorageKey.Create(uint) / KeyBuilder.AddBigEndian(uint) and eliminates the state-root divergence. The read result was already correct (max-scan), so only the on-disk key bytes change. Separately recommended (broader parity, not required for this unit): fix the little-endian integer encoding in native_contract.py StorageKey.create (line 98) and smartcontract/key_builder.py (line 17) to big-endian to match C# AddBigEndian for all natives.

### [consensus] getDesignatedByRole missing index upper-bound (currentIndex+1) check
- spec: `native/role_management.py:95-96`
- csharp: `SmartContract/Native/RoleManagement.cs:54-56`
- fix: In neo-execution-specs/src/neo/native/role_management.py get_designated_by_role, after the role and index<0 checks, read the Ledger current index for the snapshot and raise (FAULT) when `current_index + 1 < index`, matching C# RoleManagement.cs:54-56. Concretely: obtain current_index = LedgerContract.current_index(snapshot) (the same Ledger.CurrentIndex the C# uses), then `if current_index + 1 < index: raise ValueError(...)` before the find/seek loop. Keep the existing index<0 guard (the spec uses a signed Python int whereas C# uses uint, so the lower bound is still needed). No change to the backward-seek logic itself.

### [consensus] OracleRequest stored with custom binary layout instead of C# IInteroperable stack-item serialization
- spec: `native/oracle.py:62-122, 404-407`
- csharp: `SmartContract/Native/OracleRequest.cs:71-83; OracleContract.cs:268`
- fix: Replace OracleRequest.serialize()/deserialize() (native/oracle.py:62-197) and _store_request/get_request (oracle.py:404-407, 450-456) with the stack-item path: build the Array [OriginalTxid bytes (ByteString), GasForResponse (Integer), Url (ByteString), Filter or Null, CallbackContract bytes (ByteString), CallbackMethod (ByteString), UserData (ByteString)] and serialize via the spec's existing smartcontract/binary_serializer.py BinarySerializer.serialize (which already emits the C#-compatible 0x40/0x21/0x28/0x00 + VarInt/VarBytes format), deserialize via BinarySerializer.deserialize + FromStackItem-equivalent. Match C# semantics exactly: GasForResponse as a minimal-length Integer stack item (not fixed 8 bytes), Filter absence as a Null (0x00) element, and store UserData already pre-serialized (BinarySerializer of the user_data stack item) as in OracleContract.cs:266 before wrapping. Apply the same stack-item-Array treatment to the IdList storage (oracle.py:431-448) to match C# InteroperableList<ulong>.

### [consensus] Oracle.request omits fee charging, GAS mint, and contract-caller check
- spec: `native/oracle.py:305-387`
- csharp: `SmartContract/Native/OracleContract.cs:244-257`
- fix: In native/oracle.py request() (after the gasForResponse>=MinGasForResponse validation, before storing the request), mirror C# OracleContract.cs:244-285:
1. engine.AddFee(self.get_price(snapshot) * FeeFactor)  # FeeFactor = 10000
2. engine.AddFee(gas_for_response * FeeFactor)
3. Mint gas_for_response GAS to the Oracle contract Hash via the GAS native (GAS.mint(engine, oracle_hash, gas_for_response, call_on_payment=False)) — place this BEFORE the IsContract check to match C# ordering.
4. After incrementing the request id, fault when the caller is not a deployed contract: if not ContractManagement.is_contract(snapshot, engine.calling_script_hash): raise InvalidOperationError("Only contracts can make Oracle requests").
Note ordering: C# does the two AddFee + Mint, then GetAndChange(request id)/Add(1), THEN the IsContract throw, then stores request/IdList. Preserve that order so partial state/fee effects on a faulting tx match (the whole tx reverts on fault anyway, but ordering also affects which exception type/path is hit).
Separately (beyond the original claim but a real divergence in the same method): emit the OracleRequest notification — engine.send_notification(oracle_hash, "OracleRequest", [id, calling_script_hash.to_array(), url, filter or Null]) — at the end of request(), matching OracleContract.cs:280-285.

### [consensus] Oracle.request user_data serialized as raw bytes/empty rather than BinarySerializer of the StackItem
- spec: `native/oracle.py:353-360`
- csharp: `SmartContract/Native/OracleContract.cs:266`
- fix: In oracle.py request() (around lines 353-360), replace the bytes-only/empty-fallback logic with a binary StackItem serialization equivalent to C# BinarySerializer.Serialize(userData, 512, MaxStackSize): emit the StackItemType byte followed by the value (var-bytes for Integer/ByteString/Buffer, recursive count+elements for Array/Map, etc.), enforcing a 512-byte cap on the SERIALIZED size and faulting (raising the VM fault exception, not just ValueError) when exceeded — and also faulting on too many items per MaxStackSize. Store the resulting serialized bytes as user_data so OracleRequest.serialize() wraps the already-typed payload. The MAX_USER_DATA_LENGTH (512) check must be applied to the serialized length, not the raw input length, to match C#.

### [non-consensus] designateAsRole does not emit the Designation notification
- spec: `native/role_management.py:129-168`
- csharp: `SmartContract/Native/RoleManagement.cs:88-98`
- fix: In role_management.py designate_as_role, after snapshot.put(key, data) (line 168), emit the Designation notification matching C# RoleManagement.cs:88-98. Gate on HF_Echidna using the existing is_hardfork_enabled helper: if Echidna is enabled, build old_nodes = encoded compressed points from get_designated_by_role(snapshot, role, block_index) (the prior designation at index = persisting_block.index, i.e. the C# index-1 since C# uses storage index = PersistingBlock.Index+1) and new_nodes = encoded compressed points of the supplied nodes, then engine.send_notification(self.hash, "Designation", [int(role), block_index, old_nodes, new_nodes]); otherwise engine.send_notification(self.hash, "Designation", [int(role), block_index]). Note block_index here is engine.PersistingBlock.Index (the value put in the notification), while the storage key uses block_index+1 — match C#'s exact index arithmetic. Wrap the state values as VM Array/ByteString stack items consistent with the spec's StackItem model.

### [non-consensus] Oracle.finish missing invocation-stack/counter guards
- spec: `native/oracle.py:489-553`
- csharp: `SmartContract/Native/OracleContract.cs:99-100`
- fix: In native/oracle.py finish() (after acquiring engine, before/at the top of the method, oracle.py:497), add the two entry-point guards mirroring C# OracleContract.cs:99-100: if engine.invocation_stack length != 2 raise (FAULT), and if engine.get_invocation_counter() != 1 raise (FAULT). Use whatever fault/exception type the spec uses to signal a VM FAULT (matching how other native methods signal faults), so a nested or re-entrant call to Oracle.finish FAULTs exactly as C# does instead of proceeding. This must run before the OracleResponse-attribute lookup so the fault ordering matches C#.

### [non-consensus] Oracle.finish does not emit OracleResponse notification and uses incorrect GAS refund/callback semantics
- spec: `native/oracle.py:523-553`
- csharp: `SmartContract/Native/OracleContract.cs:106-108`
- fix: Rewrite OracleContract to match C# v3.10.0:

1. request() (oracle.py:305-388): after validation, charge engine.AddFee(get_price(snapshot) * FeeFactor) and engine.AddFee(gas_for_response * FeeFactor); mint gas_for_response GAS to the Oracle contract's own hash (GAS.Mint(engine, OracleContract.hash, gas_for_response, false)); require ContractManagement.is_contract(calling_script_hash) else fault; store UserData as BinarySerializer.Serialize(userData, MaxUserDataLength, MaxStackSize) rather than only-when-bytes raw.

2. finish() (oracle.py:489-553): add guards — fault if invocation-stack depth != 2 and if invocation-counter != 1; do NOT remove the request and do NOT mint any GAS here (remove lines 523-538); emit engine.send_notification(OracleContract.hash, "OracleResponse", Array[response.Id, request.original_txid bytes]); deserialize request.user_data via the binary stack-item deserializer; call back via CallFromNativeContract-equivalent with args (url, deserialized_user_data, int(code), result).

3. Add a post_persist(engine) override: iterate engine.persisting_block.transactions, for each tx carrying an OracleResponse attribute look up the request, delete Prefix_Request key, remove the id from the Prefix_IdList list (deleting the key when empty), and accumulate node payouts: nodes = RoleManagement.get_designated_by_role(Role.Oracle, block.index) mapped to signature-redeem-script hashes; index = response.id % len(nodes); nodes[index] += get_price(snapshot); after the loop, GAS.Mint each node with positive accrued GAS. Remove the gas_for_response refund-to-callback logic entirely.

## sc-application-engine

### [consensus] CheckMultisig swallows wrong-length/invalid signatures instead of faulting post-Gorgon (HF_Gorgon); also missing m==0 / m>n fault
- spec: `smartcontract/application_engine.py:2056-2117`
- csharp: `SmartContract/ApplicationEngine.Crypto.cs:60-79; Cryptography/Crypto.cs:276-279`
- fix: In smartcontract/application_engine.py _crypto_check_multisig: (1) replace the push(Integer(0))/return guards with raises that fault the VM — raise when len(pubkeys)==0 (n==0), len(signatures)==0 (m==0), and len(signatures) > len(pubkeys) (m>n), mirroring C# ArgumentException at ApplicationEngine.Crypto.cs:64-66. (2) Gate the per-signature path on HF_Gorgon using is_hardfork_enabled(engine, Hardfork.HF_GORGON): post-Gorgon, do NOT skip wrong-length signatures — call the verify path that raises a FormatException-equivalent on len(sig)!=64 (so the VM faults), matching Crypto.VerifySignature (Crypto.cs:278-279). Pre-Gorgon keep VerifySignatureV0 semantics (false on bad length, no fault). (3) Match C# loop semantics: do not pre-skip/continue on bad pubkey/sig length post-Gorgon; let verification raise. Optionally (separate fix) charge the dynamic fee add_gas(CheckSigPrice * n * exec_fee_factor) per C# line 67. After the change, an empty/oversized signature or pubkey count, and a wrong-length signature under HF_Gorgon, fault the VM (VMState.FAULT) instead of returning Integer(0)/skipping.

### [consensus] RuntimeNotify does not validate event name/argument types against the contract manifest (HF_Basilisk)
- spec: `smartcontract/application_engine.py:727-748`
- csharp: `SmartContract/ApplicationEngine.Runtime.cs:357-386`
- fix: In _runtime_notify (application_engine.py:727-748), add HF_Basilisk-gated validation mirroring C# RuntimeNotify before send_notification. Gate on the engine's hardfork check (as used elsewhere in the spec). When Basilisk is enabled: (1) resolve the current contract via the execution context state / _parse_contract_manifest; raise InvalidOperationException('Notifications are not allowed in dynamic scripts.') if there is no contract; (2) look up the event by exact name (ordinal) in manifest.abi.events; raise InvalidOperationException(f'Event `{name}` does not exist.') if absent; (3) raise InvalidOperationException('The number of the arguments does not match the formal parameters of the event.') if len(event.parameters) != len(state); (4) for each parameter, port C# CheckItemType (ApplicationEngine.Runtime.cs:467+) — Any=>true; Boolean/Integer exact type; ByteArray=>Any|ByteString|Buffer; String=>ByteString|Buffer with strict-UTF8 round-trip; Hash160/Hash256/PublicKey/Signature => Any or (ByteString|Buffer of exact length 20/32/33/64); reject Pointer; etc. — and raise InvalidOperationException on the first mismatch. When Basilisk is NOT enabled, keep the V1 behavior but still reject dynamic scripts (null contract) per RuntimeNotifyV1 (line 392-393). All these raises propagate as VM faults, matching C#.

### [consensus] SendNotification missing MaxNotificationCount limit (HF_Echidna, Application trigger)
- spec: `smartcontract/application_engine.py:186-188`
- csharp: `SmartContract/ApplicationEngine.Runtime.cs:406-420 (MaxNotificationCount=512 at :42)`
- fix: In ApplicationEngine.send_notification (src/neo/smartcontract/application_engine.py:186-188), before appending, add the C# guard. Define a module/class constant MAX_NOTIFICATION_COUNT = 512. Then in send_notification, gate exactly as C#: if the HF_Echidna hardfork is enabled AND self.trigger == TriggerType.APPLICATION AND len(self._notifications) >= 512, raise InvalidOperationException(f"Maximum number of notifications `512` is reached.") before the append. Use the existing hardfork check: from neo.smartcontract.interop_service import _is_hardfork_enabled and Hardfork.HF_ECHIDNA (e.g. `if _is_hardfork_enabled(self, Hardfork.HF_ECHIDNA) and self.trigger == TriggerType.APPLICATION and len(self._notifications) >= MAX_NOTIFICATION_COUNT: raise ...`). Do NOT apply the limit for System/Verification triggers (matches C# comment at Runtime.cs:409-411). The check must run on every emit so the fault triggers exactly when the 513th notification would be added, matching C# count>=512 semantics.

### [consensus] _contract_call does not reject blocked target contracts
- spec: `smartcontract/application_engine.py:1144-1178`
- csharp: `SmartContract/ApplicationEngine.cs:577-580`
- fix: In _contract_call (smartcontract/application_engine.py), after resolving the target contract and before _call_contract_internal (i.e. before line 1178), query PolicyContract.is_blocked for the target hash and raise InvalidOperationException (fault) when blocked, matching C# CallContractInternal lines 579-580. Equivalently, move the check into _call_contract_internal at its entry (line 1480) so it covers every caller of that method, mirroring C#'s placement inside CallContractInternal. The block check must run only for the dynamic System.Contract.Call path semantics (C# applies it in CallContractInternal which both the deployed and native overloads funnel through); use the same Prefix_BlockedAccount storage lookup the spec already implements in policy.is_blocked. Note native contracts cannot be blocked (block_account rejects native hashes), so the check is effectively a no-op for native targets, matching C#.

### [consensus] _contract_call does not validate argument count against method parameters
- spec: `smartcontract/application_engine.py:1144-1178, 1480-1545`
- csharp: `SmartContract/ApplicationEngine.cs:572-573, 608`
- fix: Resolve ABI method by name and arg count in _contract_call and _call_with_token to match GetMethod(name,args.Count); fault when no method of that arity exists; add a redundant args-count check in _call_contract_internal like ApplicationEngine.cs:608.

### [non-consensus] CheckSig does not fault on wrong-length signature/pubkey post-Gorgon (HF_Gorgon)
- spec: `smartcontract/application_engine.py:2017-2054`
- csharp: `SmartContract/ApplicationEngine.Crypto.cs:46-51; Cryptography/Crypto.cs:276-279 (VerifySignature) vs :186-188 (VerifySignatureV0)`
- fix: Make _crypto_check_sig (application_engine.py:2017-2054) match C# fault semantics, splitting two cases:
1) Malformed pubkey: must FAULT (raise) in BOTH pre- and post-Gorgon paths, mirroring ECPoint.DecodePoint throwing FormatException (ECPoint.cs:85-86,91-92,100-101). Replace the `if len(pubkey_bytes) not in (33,65): push(Integer(0))` early-return (2045-2047) with point decoding that raises on a bad prefix/length, and do NOT swallow that decode error in the try/except.
2) Malformed signature length (!=64): branch on HF_Gorgon. When HF_Gorgon is enabled (self.is_hardfork_enabled(Hardfork.HF_GORGON) at the engine's persisting height), len(sig)!=64 must raise/FAULT (matching Crypto.VerifySignature, Crypto.cs:278-279). When HF_Gorgon is NOT enabled, keep the current push(Integer(0)) (matching VerifySignatureV0, Crypto.cs:188).
Note the claim's proposed fix is incorrect for the pubkey case (it gates the pubkey fault on Gorgon); the pubkey decode-fault is unconditional in C#. The narrow try/except for ValueError/TypeError should only wrap the actual signature-verification math (curve verify) returning false on legitimate verify failure, not mask malformed-input faults. Add the same fault semantics consideration to _crypto_check_multisig (2056+), which shares the same byte-extraction pattern.

## sc-interop-prices

### [non-consensus] CreateStandardAccount/CreateMultisigAccount handler fee is not HF_Aspidochelone-gated (always charges CheckSigPrice)
- spec: `src/neo/smartcontract/syscalls/contract.py:115,147`
- csharp: `neo_csharp/src/Neo/SmartContract/ApplicationEngine.Contract.cs:124-127,141-144`
- fix: Fix the LIVE path, not syscalls/contract.py (which is dead code). In application_engine.py: (1) change the registration at lines 337-338 to charge 0 as the descriptor price (matching C# FixedPrice=0), and (2) inside the handler methods _contract_create_standard_account (line 1932) and _contract_create_multisig_account (line 1943) add an explicit fee: standard -> fee = (1<<15) if is_hardfork_enabled(HF_ASPIDOCHELONE) else (1<<8); multisig -> fee = (1<<15)*n if is_hardfork_enabled(HF_ASPIDOCHELONE) else (1<<8) (flat 1<<8, NOT *n, pre-Aspidochelone, matching ApplicationEngine.Contract.cs:142-143). Then charge fee * exec_fee_factor. NOTE: the spec's ApplicationEngine has no exec-fee-factor at all (add_gas at line 180 accumulates raw values; descriptor prices charged raw at interop_service.py:90), so to reach true v3.10.0 gas parity the spec must first introduce _execFeeFactor scaling across all syscall/opcode charges; gating these two handlers alone is necessary but not sufficient. Optionally delete the unused free functions in syscalls/contract.py:101-153 to avoid future mis-audits.

## sc-manifest-nef

### [consensus] MethodToken serialization uses fixed 4-byte method length and 2-byte CallFlags instead of var-string + 1-byte CallFlags
- spec: `src/neo/contract/nef.py:22-49 (MethodToken.serialize/deserialize); duplicated in src/neo/smartcontract/nef_file.py:70-75 and 143-152`
- csharp: `neo_csharp/src/Neo/SmartContract/MethodToken.cs:68-75 (Serialize) and 57-66 (Deserialize); WriteVarString at neo_csharp/src/Neo/Extensions/IO/BinaryWriterExtensions.cs:136-139; CallFlags:byte at neo_csharp/src/Neo/SmartContract/CallFlags.cs:20`
- fix: In src/neo/contract/nef.py MethodToken.serialize/deserialize, and the duplicated inline loops in src/neo/smartcontract/nef_file.py (serialize lines 70-75, deserialize lines 143-152): (1) replace the fixed 4-byte method-length prefix `struct.pack('<I', len(method_bytes))` / `struct.unpack_from('<I', ...)` (+4 offset) with a var-int length prefix (use write_var_bytes / read_var_int already present in nef.py, or _write_var_int/_read_var_int in nef_file.py); (2) replace the 2-byte `struct.pack('<H', call_flags)` / `struct.unpack_from('<H', ...)` (+2 offset) with a single byte (data.append(call_flags & 0xFF) and read one byte, +1 offset). This makes the token byte layout, NEF compute_checksum, and the derived contract hash match C# v3.10.0 for any NEF containing method tokens. Optionally (separate validation-gap, beyond this claim) add C#'s deserialize checks: ReadVarString cap of 32, reject method names starting with '_', and reject `call_flags & ~CallFlags.All != 0`.

### [consensus] ContractManifest.from_json / ContractGroup.from_json / ContractPermission.from_json omit C# validation (empty-name, empty supported-standard, duplicate keys, signature length 64, empty method names)
- spec: `src/neo/smartcontract/manifest/contract_manifest.py:38-50; src/neo/smartcontract/manifest/contract_group.py:45-51; src/neo/smartcontract/manifest/contract_permission.py:37-46`
- csharp: `neo_csharp/src/Neo/SmartContract/Manifest/ContractManifest.cs:125-134; neo_csharp/src/Neo/SmartContract/Manifest/ContractGroup.cs:63-64; neo_csharp/src/Neo/SmartContract/Manifest/ContractPermission.cs:92-94`
- fix: Part 1: add validation to spec from_json methods. ContractManifest.from_json rejects empty name, empty supportedstandards strings, and duplicate group keys/standards/permission-contracts/trusts. ContractGroup.from_json rejects signature length != 64. ContractPermission.from_json rejects empty/duplicate methods. Part 2 essential: wire validation into native/contract_management.py deploy (line 246) and update (line 347) by calling ContractManifest.from_json on the parsed manifest plus IsValid, since the current path stores raw bytes and never calls from_json. update must also require the new name equals the existing contract name (ContractManagement.cs:366-367). Part 1 alone has no effect without Part 2.

### [non-consensus] MethodToken deserialization omits method-name '_' prefix rejection and CallFlags validity check
- spec: `src/neo/contract/nef.py:34-49 (MethodToken.deserialize); src/neo/smartcontract/nef_file.py:140-153`
- csharp: `neo_csharp/src/Neo/SmartContract/MethodToken.cs:60-65`
- fix: The proposed fix (add the three checks to MethodToken.deserialize) is correct but INCOMPLETE. Full parity requires:
1. In nef.py MethodToken.deserialize / nef_file.py token loop: change the method length from fixed 4-byte uint32 to a var-int read (read_var_int) capped at 32, raising on length > 32 (C# ReadVarString(32)); likewise change serialize/to_array to write the method via var-int+bytes, not struct.pack('<I', ...) — this fixes both the missing cap AND the byte-layout/checksum mismatch.
2. Reject method names starting with '_' (FormatException equivalent / ValueError).
3. Reject call_flags with bits outside CallFlags.All (0b0000_1111): if (call_flags & ~0x0F) != 0: raise.
4. Critically, make ContractManagement.deploy and update actually deserialize the NEF (NefFile.deserialize(nef_file)) and let the ValueError propagate (fault the tx), mirroring C# AsSerializable<NefFile>() at ContractManagement.cs:275 and :356 — otherwise the validations never run on the consensus-critical deploy/update path.
5. Remove or harden the raw-script fallback in application_engine._extract_script_from_nef (lines 1569-1573) so a malformed stored NEF faults rather than silently executing raw bytes.

### [non-consensus] NefFile token array has no max-count bound (C# caps at 128)
- spec: `src/neo/contract/nef.py:152-156; src/neo/smartcontract/nef_file.py:138-153`
- csharp: `neo_csharp/src/Neo/SmartContract/NefFile.cs:131 (ReadSerializableArray<MethodToken>(128))`
- fix: In both spec NEF deserializers, enforce token_count <= 128 immediately after reading the count, raising an error when exceeded, matching C# ReadSerializableArray<MethodToken>(128):
- src/neo/contract/nef.py:152 — after `token_count, offset = read_var_int(data, offset)`, add `if token_count > 128: raise ValueError("Max length exceeded")` before the loop.
- src/neo/smartcontract/nef_file.py:138 — after `token_count, offset = cls._read_var_int(data, offset)`, add the same `> 128` guard before the loop.
For exact C# fidelity the cap should be applied as part of the var-int read (reject before consuming token bytes), which a pre-loop guard achieves since the count is decoded first.

### [non-consensus] Contract parameter type JSON name uses snake_case lowercasing instead of C# PascalCase, breaking multi-word types round-trip
- spec: `src/neo/smartcontract/manifest/contract_parameter_definition.py:18-30 (to_json uses self.type.name.lower(); from_json uses json.get('type').upper() then ContractParameterType[...])`
- csharp: `neo_csharp/src/Neo/SmartContract/Manifest/ContractParameterDefinition.cs:70-76 (ToJson: Type.ToString()) and 52-63 (FromJson: Enum.Parse<ContractParameterType>)`
- fix: In contract_parameter_definition.py (and the parallel contract_method_descriptor.py returntype path), replace the enum.name.lower()/.upper() scheme with an explicit bidirectional mapping to the exact C# PascalCase tokens: Any, Boolean, Integer, ByteArray, String, Hash160, Hash256, PublicKey, Signature, Array, Map, InteropInterface, Void. to_json should emit the PascalCase token; from_json should look up the token in a case-sensitive dict (matching C# Enum.Parse default). Additionally, in from_json raise a FormatException-equivalent when the name is empty (mirror C# line 59-60) and reject Void / undefined types for parameter definitions (mirror C# line 61-62). This makes both ABI JSON emission and parsing byte-for-byte compatible with C# v3.10.0 manifests.

## sc-serializers-params

### [consensus] JSON serialize emits exact integer; C# bounds to safe-integer range and writes as double
- spec: `smartcontract/json_serializer.py:63-66`
- csharp: `neo_csharp/src/Neo/SmartContract/JsonSerializer.cs:120-127`
- fix: Add the C# safe-integer bound to the spec. In the in-contract StdLib path (native/std_lib.py _to_json_value INTEGER branch, lines 261-262) and the reference serializer (smartcontract/json_serializer.py:63-66), raise/fault when the integer is greater than 9007199254740991 (2^53-1) or less than -9007199254740991, instead of serializing it. For in-range integers the plain integer literal is already byte-identical to C# (double)integer shortest-round-trip output, so only the bounds check is needed.

### [consensus] JSON serialize allows Boolean/Integer map keys; C# only allows ByteString keys
- spec: `smartcontract/json_serializer.py:108-117`
- csharp: `neo_csharp/src/Neo/SmartContract/JsonSerializer.cs:136`
- fix: In json_serializer.py _key_to_string (lines 108-117), reject any non-ByteString key: remove the Boolean branch (111-112) and Integer branch (113-114); for a ByteString key, decode the bytes as STRICT UTF-8 (raising on invalid UTF-8, matching C# ToStrictUtf8String) rather than base64 (line 116). Raise to fault on anything else. This mirrors C# SerializeToByteArray (JsonSerializer.cs:136 + 146). Note: the proposed fix's "GetString (UTF8)" must be STRICT UTF-8 (error on invalid sequences), not lenient decode, to preserve C#'s fault behavior. Separately (out of scope but recommended in the same pass): align ByteString/Buffer VALUE encoding at lines 71/76 to strict UTF-8 GetString() instead of base64 to match JsonSerializer.cs:118 — the value-side path has the same divergence.

### [consensus] JSON serialize encodes ByteString/Buffer as base64; C# uses raw UTF8 string (GetString)
- spec: `smartcontract/json_serializer.py:68-76,116`
- csharp: `neo_csharp/src/Neo/SmartContract/JsonSerializer.cs:117-119,146`
- fix: In smartcontract/json_serializer.py, emit ByteString/Buffer JSON string values (lines 71, 76) and ByteString map keys (line 116) as the STRICT UTF-8 decoding of the bytes, matching C# GetString() = ToStrictUtf8String(). The decode must be strict: on bytes that are not valid UTF-8, raise (fault) the same way C#'s StrictUTF8 decoder throws (FormatException), so non-UTF8 inputs fault rather than producing a lenient/base64 string. Concretely, replace `base64.b64encode(item.value).decode('ascii')` with something equivalent to `item.value.decode('utf-8')` using strict error handling (Python's default errors='strict' raises UnicodeDecodeError, which must be surfaced as a serialization fault). Do NOT use lenient/replacement decoding, or the non-UTF8 case will halt where C# faults. The auditor's proposed fix ("UTF8 decoding") is correct in direction but must specify the strict/fault semantics.

### [consensus] JSON deserialize bounds nodes by depth/per-collection length instead of cumulative MaxStackSize
- spec: `smartcontract/json_serializer.py:32-33,131-132,147,153,156-158`
- csharp: `neo_csharp/src/Neo/SmartContract/JsonSerializer.cs:176,185-188,213`
- fix: Replace the depth>128 + per-collection len>MAX_ITEMS checks in _from_json (json_serializer.py:131-132,147-148,153-154) with a single shared, decrementing node budget seeded at MaxStackSize (2048), mirroring C# JsonSerializer.Deserialize. Concretely: thread a mutable counter (e.g. a one-element list or an instance attribute) seeded to 2048 in deserialize(); at the top of _from_json decrement once per node and fault ('Max stack size reached') when it would go below zero (i.e. check-then-decrement: `if budget == 0: raise; budget -= 1`); inside the dict branch, additionally check-then-decrement once per map property BEFORE recursing into each value (matching C# line 213). Remove the independent per-list/per-dict len>2048 checks (they are subsumed by, and off-by-one against, the cumulative budget). The existing depth>128 guard may be kept as a harmless extra safety bound (it never fires before the budget on wide inputs and a 128-deep chain already consumes >=128 budget), but it is not what C# uses. After the fix, flat array of 2048 ints, [arr1024,arr1024], and map of 1024 props all correctly FAULT, matching C# v3.10.0.

### [consensus] JSON serialize MAX_SIZE = 1MB instead of C# MaxItemSize = 131070
- spec: `smartcontract/json_serializer.py:32,41-42`
- csharp: `neo_csharp/src/Neo/SmartContract/Native/StdLib.cs:49; neo_csharp_vm/src/Neo.VM/ExecutionEngineLimits.cs:40`
- fix: Fix the actual native path, not the dead class. In /Users/jinghuiliao/git/r3e/neo-execution-specs/src/neo/native/std_lib.py:247-249, enforce the engine's MaxItemSize (131070, from vm/limits.py MAX_ITEM_SIZE) on json_serialize output and fault when exceeded, mirroring C# JsonSerializer.SerializeToByteArray (JsonSerializer.cs:154,157). Concretely: after computing `result = json.dumps(self._to_json_value(item)).encode("utf-8")`, raise an engine fault (InvalidOperationException equivalent) when `len(result) > 131070` rather than 1MB. Ideally pass the engine's limits in so the cap is engine.limits.max_item_size, not a hardcoded constant. Note the C# semantics also check incrementally during serialization, but a final-size fault at the 131070 boundary matches the observable HALT/FAULT outcome for all-or-nothing serialization. Changing smartcontract/json_serializer.py MAX_SIZE (the auditor's proposed fix) is NOT sufficient — that class is unused by the native path; either wire the native jsonSerialize through a fixed JsonSerializer with maxSize=131070, or add the cap directly in std_lib.json_serialize.

### [consensus] Binary deserialize Map key not constrained to PrimitiveType
- spec: `smartcontract/binary_serializer.py:213-222`
- csharp: `neo_csharp/src/Neo/SmartContract/BinarySerializer.cs:153`
- fix: In binary_serializer.py MAP branch (around line 219), after deserializing key_item, validate it is a PrimitiveType — i.e. type in {BOOLEAN, INTEGER, BYTESTRING} — and raise (fault) otherwise, before `result[key_item] = map_value`. Mirror C#'s `(PrimitiveType)key` cast. Do NOT include Buffer (StackItemType.BUFFER) in the allowed set: in C# Buffer derives from StackItem, not PrimitiveType, so a Buffer key also throws InvalidCastException and must fault. Allowed key types are exactly Boolean, Integer, ByteString.

### [non-consensus] Binary deserialize bounds item count per-container instead of cumulative MaxStackSize
- spec: `smartcontract/binary_serializer.py:32,197,206,215`
- csharp: `neo_csharp/src/Neo/SmartContract/BinarySerializer.cs:108,115,123-124`
- fix: Thread a cumulative item counter through smartcontract/binary_serializer.py `_deserialize_item` to mirror C#'s flat-loop `deserialized.Count > maxItems` (BinarySerializer.cs:123-124). Replace the three isolated `if count > cls.MAX_ITEMS` checks (lines 197/206/215) with a running total that increments for EVERY node produced — including each container node itself and each leaf — and raise when the running total exceeds MAX_ITEMS (strict `>`, value 2048 = MaxStackSize). Easiest faithful approach: convert to an explicit work-stack matching C# (push container placeholders, `undeserialized += count` / `count*2` for Map, increment `deserialized` count and check `> 2048` after each item). Keep `_read_var_int` count-capping at maxItems too. SEPARATELY (out of this unit's strict scope but should be flagged): iterators.py:126-133 swallows the FormatException and returns ByteString(data) whereas C# StorageIterator.cs:52 lets it fault the VM; and native/std_lib.py:189-245 `_binary_deserialize` is an entirely independent, much-simpler deserializer (only 1-byte container counts, no item/size limits) that diverges from C# BinarySerializer far more broadly and is the actual System.Binary.Deserialize consensus path — both warrant their own audit findings.

## sc-syscalls-contract-crypto-iter

### [consensus] CheckMultisig count-validation returns False instead of faulting (ArgumentException)
- spec: `smartcontract/syscalls/crypto.py:71-79`
- csharp: `SmartContract/ApplicationEngine.Crypto.cs:64-66`
- fix: In the ACTUALLY-REGISTERED handler `_crypto_check_multisig` (application_engine.py:2056-2117), replace the single early-return branch at line 2092-2094 (`if len(pubkeys) < len(signatures) or len(signatures) == 0: self.push(Integer(0)); return`) with three explicit fault cases mirroring ApplicationEngine.Crypto.cs:64-66 — n==0, m==0, m>n — each raising/triggering an UNCATCHABLE VM fault (bypassing the spec's execute_throw TRY/CATCH routing, e.g. via the VMAbortException-style path used at execution_engine.py:115-117, or directly setting VMState.FAULT), since C#'s ArgumentException is not a CatchableException. Apply the same fix to the dead duplicate at syscalls/crypto.py:71-79 (or delete that unused module). Keep the message order/text matching C# is cosmetic; the load-bearing change is HALT-with-False → uncatchable FAULT.

### [non-consensus] CheckSig/CheckMultisig do not fault on wrong-length signature post-HF_Gorgon (no Gorgon gating)
- spec: `smartcontract/syscalls/crypto.py:33-38, 87-94`
- csharp: `SmartContract/ApplicationEngine.Crypto.cs:46-79; Cryptography/Crypto.cs:276-279`
- fix: In smartcontract/syscalls/crypto.py, gate verifier behavior by IsHardforkEnabled(HF_Gorgon). Pre-Gorgon: keep current semantics (length!=64 → False → HALT), matching VerifySignatureV0. Post-Gorgon: do NOT swallow a wrong-length signature — raise a fault-causing exception (the spec's equivalent of FormatException) so the VM FAULTS instead of pushing Boolean(False). Specifically: for crypto_check_sig, when Gorgon is enabled and len(signature)!=64, propagate an exception that triggers VM FAULT rather than returning False. For crypto_check_multisig, when Gorgon is enabled, remove the try/except around the verification loop so a wrong-length signature faults mid-loop (after the CheckSigPrice*n fee is already added, matching C#'s AddFee-before-loop ordering at ApplicationEngine.Crypto.cs:67-71). This requires the spec's verify_signature (or the syscall layer) to expose a Gorgon-strict path that raises on length!=64 instead of returning False. Add the HF_Gorgon hardfork to the spec's hardfork enum/config if not present.

### [non-consensus] CreateStandardAccount fee not gated on HF_Aspidochelone
- spec: `smartcontract/syscalls/contract.py:114-115`
- csharp: `SmartContract/ApplicationEngine.Contract.cs:121-129`
- fix: In smartcontract/syscalls/contract.py, gate ONLY the base price on HF_Aspidochelone (do NOT add an execFeeFactor multiply — the spec applies that at the runner boundary). For contract_create_standard_account (line 115): replace engine.add_gas(1 << 15) with `fee = (1 << 15) if _is_aspidochelone_enabled(engine) else (1 << 8); engine.add_gas(fee)`. For contract_create_multisig_account (line 147): replace engine.add_gas((1 << 15) * n) with `fee = ((1 << 15) * n) if _is_aspidochelone_enabled(engine) else (1 << 8); engine.add_gas(fee)` (note: C# pre-Aspidochelone multisig fee is a FLAT 1<<8, not 1<<8*n). Reuse the existing hardfork-enabled helper pattern (e.g. NativeContract.is_hardfork_enabled(engine, Hardfork.HF_ASPIDOCHELONE) as in runtime_random.py).

## sc-syscalls-storage

### [consensus] Storage.Put gas charge is a flat constant, not the C# newDataSize×StoragePrice×FeeFactor formula
- spec: `smartcontract/application_engine.py:1048-1049 (live path); duplicated wrong model in smartcontract/syscalls/storage.py:144`
- csharp: `SmartContract/ApplicationEngine.Storage.cs:236-259`
- fix: Replace the flat STORAGE_WRITE charge in application_engine.py:1029-1049 `_storage_put` with the C# model. Before mutating the snapshot: look up the existing item, compute newDataSize via the four-branch differential — (a) key absent: newDataSize = len(key)+len(value); (b) key present and len(value)==0: 0; (c) len(value) <= len(old_value): (len(value)-1)//4 + 1; (d) old empty (len(old_value)==0): len(value); (e) else: (len(old_value)-1)//4 + 1 + len(value) - len(old_value). Charge `add_gas(newDataSize * storage_price)` where storage_price = PolicyContract.GetStoragePrice (default 100000) — note the spec's add_gas is in plain GAS-datoshi units so the C# FeeFactor=10000 pico multiplier is NOT applied here (it cancels in C# FeeConsumed). Charge before writing the new value, matching C# order. Also add the MaxStorageKeySize/MaxStorageValueSize validation (ApplicationEngine.Storage.cs:230-233) that the live path currently omits. The dead syscalls/storage.py variant should be deleted or aligned to the same model to avoid future confusion.

### [consensus] Storage.Put does not validate key length > 64 (MaxStorageKeySize) — halts where C# faults
- spec: `smartcontract/application_engine.py:1029-1049`
- csharp: `SmartContract/ApplicationEngine.Storage.cs:230-231`
- fix: Add a key-over-64 fault and value-over-65535 fault to _storage_put before snapshot.put and gas, matching C# Put.

### [consensus] Storage.Put does not validate value length > 65535 (MaxStorageValueSize) — halts where C# faults
- spec: `smartcontract/application_engine.py:1029-1049`
- csharp: `SmartContract/ApplicationEngine.Storage.cs:232-233`
- fix: In _storage_put (application_engine.py:1044-1048), before the read-only check, after computing key_bytes and value_bytes add: if len(key_bytes) > 64 (MaxStorageKeySize): raise a faulting exception (ArgumentException-equivalent); if len(value_bytes) > 65535 (MaxStorageValueSize / ushort.MaxValue): raise a faulting exception. Place both length checks before the is_read_only check to mirror C# order (key -> value -> read-only). This affects both System.Storage.Put and System.Storage.Local.Put since the latter delegates to _storage_put. Define the limits as named constants (MaxStorageKeySize=64, MaxStorageValueSize=65535) for parity with ApplicationEngine.Storage.cs:24,29.

### [consensus] Storage.Find performs no FindOptions validation — halts on invalid/conflicting options where C# faults
- spec: `smartcontract/application_engine.py:1009-1027 and smartcontract/iterators.py StorageIterator (no validation)`
- csharp: `SmartContract/ApplicationEngine.Storage.cs:181-203`
- fix: In the spec's Storage.Find handler, validate options BEFORE constructing the iterator, mirroring C# ApplicationEngine.Storage.cs:183-202. Apply to the live path _storage_find (application_engine.py:1022, after reading `options`, before line 1026) and also to syscalls/storage.py:storage_find (after line 189) — or factor a shared validate_find_options(options) helper. Checks (raise a VM fault, e.g. InvalidOperationException/ValueError that faults, on any violation):
1. options & ~FindOptions.All (191) != 0  -> reject (out-of-range bit, e.g. 64)
2. KEYS_ONLY set together with any of VALUES_ONLY/DESERIALIZE_VALUES/PICK_FIELD0/PICK_FIELD1 -> reject
3. VALUES_ONLY set together with KEYS_ONLY or REMOVE_PREFIX -> reject
4. PICK_FIELD0 and PICK_FIELD1 both set -> reject
5. (PICK_FIELD0 or PICK_FIELD1) without DESERIALIZE_VALUES -> reject
The fault must occur at the Find call site (so the iterator is never pushed), matching C# ordering. Add coverage for options=64, KeysOnly|ValuesOnly, ValuesOnly|RemovePrefix, PickField0|PickField1, and PickField0 without DeserializeValues -> all must FAULT.

## vm-comparison-types

### [non-consensus] CONVERT of Null to a concrete defined type faults in spec but returns Null in C#
- spec: `vm/instructions/types.py:91-95`
- csharp: `neo_csharp_vm/src/Neo.VM/Types/Null.cs:27-32`
- fix: In vm/instructions/types.py _convert_to, mirror Null.ConvertTo exactly. Move the Null/ANY-item check BEFORE the `item.type == target_type` early-return (line 88), so the Null→Any case is handled correctly. When the item is Null (item is NULL or item.type == ANY): if target_type == ANY or target_type is not a defined StackItemType, fault (raise InvalidOperationException — C# throws InvalidCastException for type==Any); otherwise (any defined non-Any target: Boolean/Integer/ByteString/Buffer/Array/Struct/Map/InteropInterface/Pointer) return the same Null item unchanged (push Null back, do NOT raise). Concretely: replace lines 91-95 so that the non-ANY defined-target branch returns NULL (item) instead of raising, and the ANY (or undefined) target branch raises. This makes `PUSHNULL; CONVERT <concrete>` halt with Null on the stack and `PUSHNULL; CONVERT Any` fault, matching C# v3.10.0.

### [non-consensus] CONVERT of Null to Any returns Null in spec but faults in C#
- spec: `vm/instructions/types.py:87-89`
- csharp: `neo_csharp_vm/src/Neo.VM/Types/Null.cs:27-31`
- fix: In vm/instructions/types.py `_convert_to`, move the Null/ANY special-case ABOVE the `if item.type == target_type: return item` early return. Concretely, before line 88, add: `if item is NULL or item.type == StackItemType.ANY:` then `if target_type == StackItemType.ANY: raise InvalidOperationException("Cannot convert Null to Any")` else `raise InvalidOperationException(f"Cannot convert Null to {StackItemType(target_type).name}")`. This makes Null→ANY (and any ANY-typed item→ANY) fault, matching C# Null.ConvertTo throwing InvalidCastException on StackItemType.Any. The existing line-94 `return NULL` for target ANY is wrong and should be removed/inverted to a raise. Note: the same-type early return must not be allowed to fire for Null before this check.

## vm-compound-splice

### [consensus] PICKITEM does not handle ByteString/PrimitiveType operand (faults instead of returning byte)
- spec: `vm/instructions/compound.py:220-239`
- csharp: `neo_csharp_vm/src/Neo.VM/JumpTable/JumpTable.Compound.cs:384-392`
- fix: Add primitive-type handling to pickitem in vm/instructions/compound.py before the final else, matching C#'s `case PrimitiveType` branch. For ByteString: span = x.get_span(); for Integer: span = little-endian bytes of x.value, empty if zero (per Integer.cs Memory); for Boolean: span = b'\x01' if x.value else b'\x00' (per Boolean.cs). Then bounds-check idx against len(span) — raise on out-of-range (this routes through TRY/CATCH as a catchable fault, matching C# CatchableException) — and push Integer(span[idx]). Note: the claim's proposed fix only covers ByteString; to fully match C# v3.10.0 the fix must also cover Integer and Boolean operands (both are PrimitiveType in C#), and the spec's Integer/Boolean types currently lack get_span so their spans must be computed inline as described.

### [consensus] VALUES does not clone Struct sub-items (reference aliasing divergence)
- spec: `vm/instructions/compound.py:203-217`
- csharp: `neo_csharp_vm/src/Neo.VM/JumpTable/JumpTable.Compound.cs:336-352`
- fix: In vm/instructions/compound.py `values()`, clone top-level Struct elements before adding, for BOTH the Map and Array branches (C# clones in either case): replace `result.add(value)` / `result.add(item)` with `result.add(value.clone() if isinstance(value, Struct) else value)` and `result.add(item.clone() if isinstance(item, Struct) else item)` respectively. Import Struct in the module. Non-Struct items remain added by reference, matching C#. Additionally, to fully mirror C# `Struct.Clone(engine.Limits)`, `Struct.clone()` (types/struct.py:15-23) should enforce the MaxStackSize-1 sub-item limit and raise on overflow (currently unbounded); the missing clone is the consensus-critical part, the limit is a secondary parity hardening.

### [consensus] SETITEM on Buffer rejects negative byte values; C# accepts sbyte range (-128..255)
- spec: `vm/instructions/compound.py:267-274`
- csharp: `neo_csharp_vm/src/Neo.VM/JumpTable/JumpTable.Compound.cs:470-481`
- fix: In vm/instructions/compound.py Buffer branch of setitem (lines 271-274), match C#: change the range check from `if val < 0 or val > 255` to `if val < -128 or val > 255` (i.e. reject only outside [sbyte.MinValue, byte.MaxValue]), and store the two's-complement byte: `x[idx] = val & 0xFF` (so -1 -> 0xFF). The `& 0xFF` is also required because Python bytearray assignment rejects negative ints, so a bare `x[idx] = val` for a negative value would still raise. Optionally add the C# primitive-type guard (`value is PrimitiveType`) before get_integer to fault on non-primitive Buffer values, matching JumpTable.Compound.cs:475-476, though that is a separate (also divergent) check from the one in this claim.

### [consensus] Compound/splice index-and-type errors are all catchable in the spec; C# splits CatchableException vs non-catchable InvalidOperationException
- spec: `vm/instructions/compound.py:227,231,261,265,273,297,301; engine routing src/neo/vm/execution_engine.py:118-134`
- csharp: `neo_csharp_vm/src/Neo.VM/ExecutionEngine.cs:158-173; JumpTable.Compound.cs:373/380/389/397/444/474 vs 281/296/305/310/402/476/479/484`
- fix: Two fixes in the spec.

1) Catchability classification (execution_engine.py + compound.py + splice.py): Introduce a CatchableException type and only route THAT through execute_throw; make all other compound/splice errors bypass TRY/CATCH and FAULT immediately. Concretely: (a) define `class CatchableException(InvalidOperationException)` (or a dedicated marker); (b) in execution_engine.py:118-134 change the catch-all so that only `CatchableException` (plus the existing exclusions) is routed via execute_throw, while any other non-OutOfGas/non-Abort exception sets uncaught_exception and state=FAULT directly without searching the try-stack — mirroring C# ExecutionEngine.cs:162 vs 171/OnFault; (c) in compound.py raise CatchableException ONLY at the C# CatchableException sites: pickitem array out-of-range (226), map-key-not-found (230), primitive/buffer out-of-range (235), setitem array out-of-range (260) and buffer out-of-range (269) — keep plain (non-catchable) for the buffer-non-primitive/overflow checks (272-273), every type-mismatch default (239/276/etc.), HASKEY index/type, REMOVE array out-of-range (296) and REMOVE type default (304), PACK/NEWARRAY/NEWSTRUCT size+type, UNPACK/SIZE/VALUES/KEYS defaults; (d) in splice.py keep all MEMCPY/SUBSTR/LEFT/RIGHT/NEWBUFFER errors non-catchable (plain). Note: do NOT model spec's "Map size limit exceeded" in setitem (264-265) as catchable — C# SETITEM has no such Map-size check, which is a separate item to reconcile.

2) REMOVE on Map missing key (compound.py:299-302): remove the `if key not in x: raise` branch; make it a silent no-op when the key is absent to match C# Map.Remove returning null (Types/Map.cs:149-150) and the handler ignoring null (Compound.cs:538-545). I.e. `if key in x: del x[key]` and otherwise do nothing.

### [non-consensus] HASKEY: negative index halts (returns false) instead of faulting; missing MaxItemSize bound check
- spec: `vm/instructions/compound.py:176-189`
- csharp: `neo_csharp_vm/src/Neo.VM/JumpTable/JumpTable.Compound.cs:269-312`
- fix: The auditor's proposed fix is correct for the post-Gorgon path but INCOMPLETE — HASKEY is hardfork-gated in C# and the spec has no gating. Implement both:

POST-Gorgon (HF_Gorgon enabled) in haskey(): for Array, Buffer AND ByteString operands compute index=int(key.get_integer()); if index < 0 or index >= engine.limits.max_item_size, raise a non-catchable fault (InvalidOperationException matching C# 'The index {index} is invalid for OpCode HASKEY'); else push Boolean(index < count/size). Add the missing ByteString operand branch (push index < len(span)). Map unchanged (push key in x).

PRE-Gorgon (HF_Gorgon disabled): for Array, Buffer AND ByteString, fault only when index < 0 (no MaxItemSize upper bound); else push Boolean(index < count/size). Add ByteString branch. Map unchanged.

Gate the two behaviors on HF_Gorgon exactly as C# ApplicationEngine.Create does (Gorgon -> DefaultJumpTable bounds check; otherwise -> HasKey_Before543 negative-only). Reference: C# JumpTable.Compound.cs:269-312 (post-Gorgon) and ApplicationEngine.cs:444-487 (HasKey_Before543); gating at ApplicationEngine.cs:711-727; MaxItemSize=131070 at ExecutionEngineLimits.cs:40.

### [non-consensus] SETITEM on Buffer with non-primitive value should fault before range check
- spec: `vm/instructions/compound.py:267-274`
- csharp: `neo_csharp_vm/src/Neo.VM/JumpTable/JumpTable.Compound.cs:475-476`
- fix: In vm/instructions/compound.py setitem Buffer branch (267-274), after the bounds check, reject non-primitive values with an UNCATCHABLE fault mirroring C# InvalidOperationException (e.g. raise VMAbortException, or whatever the spec uses to bypass try/catch routing — same mechanism the engine special-cases at execution_engine.py:115-117), rather than letting value.get_integer() raise a catchable TypeError. For full C# parity the byte-range overflow check (272-273) must ALSO be uncatchable, since C# (JumpTable.Compound.cs:478-479) throws InvalidOperationException (not CatchableException) there too. Concretely: (1) add `if not isinstance(value, (Integer, Boolean, ByteString)): <uncatchable fault>` before converting; (2) make the `val < 0 or val > 255` overflow path uncatchable as well. Leave the index bounds check (269-270) catchable, matching C#'s CatchableException at line 474. Broader (optional) fix: have the engine distinguish C# CatchableException-equivalents from InvalidOperationException so the whole VM faults uncatchably on InvalidOperationException.

### [non-consensus] Key/operand type not enforced as PrimitiveType for HASKEY/PICKITEM/SETITEM/REMOVE (non-catchable type fault missing)
- spec: `vm/instructions/compound.py:178,222,256,292`
- csharp: `neo_csharp_vm/src/Neo.VM/JumpTable/JumpTable.Compound.cs:271,365,436,522`
- fix: Enforce PrimitiveType on the popped key in haskey/pickitem/setitem/remove BEFORE any key usage, and make the failure a NON-catchable fault to match C# Pop<PrimitiveType>() (InvalidCastException -> OnFault). Critically, a plain InvalidOperationException/TypeError is NOT sufficient: the spec's loop (execution_engine.py:118-134) routes every generic Exception through execute_throw, which would leave the fault catchable and still diverge from C#. Introduce a dedicated non-catchable exception (mirroring the VMAbortException handling at execution_engine.py:115-117 that bypasses execute_throw and faults immediately), and raise it when the key is not an instance of (Integer, ByteString, Boolean). Add it at compound.py:178 (haskey), 222 (pickitem), 256 (setitem, after popping value/key), and 292 (remove), guarding the key before get_integer()/dict use. Note the analogous already-catchable PACKMAP check (compound.py:41-42) is a separate, related finding (C# PackMap also uses Pop<PrimitiveType>()).

## vm-control-flow

### [consensus] CALLA failure (non-Pointer or cross-script pointer) is catchable in spec but uncatchable FAULT in C#
- spec: `src/neo/vm/instructions/control_flow.py:223-230`
- csharp: `neo_csharp_vm/src/Neo.VM/JumpTable/JumpTable.Control.cs:373-379 (Pop<Pointer>() and InvalidOperationException), routed via ExecutionEngine.cs:171-174 OnFault`
- fix: Fix at the loop-routing level, not CALLA-specifically. The spec must stop routing engine-internal errors (InvalidOperationException, InvalidCastException-equivalents, etc.) through execute_throw as catchable VM exceptions. Introduce a dedicated catchable-exception type (mirroring C#'s CatchableException) and change execution_engine.py:118 so that ONLY instances of that catchable type are routed through self.execute_throw; all other Exception subclasses (incl. InvalidOperationException raised by calla at control_flow.py:226-229) must set state = VMState.FAULT directly (uncatchable), matching C#'s outer catch -> OnFault. With this change, CALLA's non-Pointer and cross-script failures fault uncatchably as in C# v3.10.0, with no CALLA-specific code change. (Note: the claim's assertion that "no new type is needed" is slightly imprecise — the spec currently lacks a CatchableException equivalent, so the fix does require adding such a classification or an explicit allowlist; the routing-fix mechanism itself is correct.)

### [non-consensus] Spec routes ALL runtime exceptions through VM try/catch; C# only catches CatchableException (ASSERT false becomes catchable in spec, uncatchable in C#)
- spec: `src/neo/vm/execution_engine.py:118-134 (generic `except Exception` -> execute_throw); ASSERT source src/neo/vm/instructions/control_flow.py:257-260`
- csharp: `neo_csharp_vm/src/Neo.VM/ExecutionEngine.cs:158-174 (only `catch (CatchableException ex) when (Limits.CatchEngineExceptions)` routes to ExecuteThrow; all other exceptions hit `catch (Exception e) { OnFault(e); }`); Assert throws plain Exception at JumpTable.Control.cs:413-418`
- fix: Introduce a `CatchableException` exception type in the spec and make `execute_next` route ONLY `CatchableException` through `execute_throw` (gated on `Limits.CatchEngineExceptions`, default true). Concretely: (1) define `class CatchableException(Exception)`; (2) change the PICKITEM/SETITEM/REMOVE out-of-range raises in instructions/compound.py to raise `CatchableException` (mirroring C# JumpTable.Compound.cs:373/380/389/397/444/474); (3) in execution_engine.py:118 replace the broad `except Exception as e` with `except CatchableException as e:` (route to execute_throw) plus a separate `except Exception as e:` that faults directly (set state=FAULT, uncaught_exception = ByteString of message) — matching C#'s `catch (Exception e){ OnFault(e); }`. Keep OutOfGasException re-raised and VMAbortException/VMUnhandledException handling unchanged. Result: ASSERT-false (control_flow.py:260), unknown opcode, jump-out-of-range, GetInteger overflow, CALLA type/script errors fault uncatchably; only the three out-of-range Compound ops remain catchable.

## vm-engine

### [consensus] MaxStackSize is never enforced via the reference counter (PostExecuteInstruction missing)
- spec: `src/neo/vm/execution_engine.py:103-111 (execute_next instruction loop); src/neo/vm/reference_counter.py:33-71`
- csharp: `neo_csharp_vm/src/Neo.VM/ExecutionEngine.cs:166 (PostExecuteInstruction call) and :302-305; neo_csharp_vm/src/Neo.VM/ReferenceCounter.cs:57-61 (PostExecuteInstruction throws when Count > MaxStackSize)`
- fix: Add ReferenceCounter.post_execute_instruction(limits) raising InvalidOperationException when _count exceeds limits.max_stack_size, and call it in ExecutionEngine.execute_next after the handler and before move_next inside the existing try/except so it routes to FAULT. Remove the per-EvaluationStack _max_size=2048 cap and per-Array/per-Map 2048 caps so the only limit is the global reference count, matching C# exactly. Files: src/neo/vm/execution_engine.py lines 103-111, src/neo/vm/reference_counter.py, src/neo/vm/evaluation_stack.py lines 22-26, src/neo/vm/instructions/compound.py lines 248 and 264.

### [non-consensus] ReferenceCounter does not implement the v3.10.0 StackReferences recursive counting algorithm
- spec: `src/neo/vm/reference_counter.py:38-71 (add_reference/remove_reference/check_zero_referred)`
- csharp: `neo_csharp_vm/src/Neo.VM/ReferenceCounter.cs:36-82 (AddStackReference/RemoveStackReference recurse into CompoundType.SubItems on the StackReferences 0<->count transition)`
- fix: Reimplement ReferenceCounter (src/neo/vm/reference_counter.py) to mirror C# v3.10.0: (1) add_reference(item, count=1) increments a global _references_count by count, and for a CompoundType increments a per-compound stack_references by count, recursing add_reference into SubItems ONLY when stack_references transitions to == count (0->count). (2) remove_reference decrements _references_count and the compound's stack_references, recursing remove_reference into SubItems ONLY when stack_references returns to 0. (3) Drop the flat id-map and the per-placement _ref_add/_ref_remove counting in array.py/map.py/slot.py for SUB-ITEM counting (the eval-stack push/pop and the compound-mutation opcodes in JumpTable.Compound must drive AddStackReference/RemoveStackReference with count=1 exactly as C# JumpTable.Compound.cs does), so the subtree count flows from the recursion, not from constructor/placement. (4) Add a per-CompoundType stack_references field on Array/Struct/Map (and ensure Slot participates). (5) Add a PostExecuteInstruction()-equivalent that raises InvalidOperationException when count > limits.max_stack_size, and CALL it after every instruction in execution_engine.py (after handler(self,instr), before/around move_next), routing the raise through the VM try/catch like other faults. Validate against the C# UT_ReferenceCounter.cs cases (TestCircularReferences step sequence, TestCheckZeroReferred FAULT at MaxStackSize+1, TestCheckZeroReferred_SetItemArray==5, TestPostExecuteInstruction).

## vm-gas-prices

### [consensus] gas.py opcode prices halved for Stack/Slot/ISNULL/ISTYPE ops (1 vs C# 2)
- spec: `src/neo/vm/gas.py:83-150,228-229`
- csharp: `neo_csharp/src/Neo/SmartContract/ApplicationEngine.OpCodePrices.cs:92-156,219-220`
- fix: In src/neo/vm/gas.py set the base price to 2 (1<<1) for: DEPTH, DROP, NIP, DUP, OVER, SWAP, ROT, REVERSE3, REVERSE4 (lines 83-96); every LDSFLD0..LDSFLD6/LDSFLD, STSFLD0..STSFLD6/STSFLD, LDLOC0..LDLOC6/LDLOC, STLOC0..STLOC6/STLOC, LDARG0..LDARG6/LDARG, STARG0..STARG6/STARG slot opcode (lines 103-150); and ISNULL/ISTYPE (lines 228-229). This matches C# OpCodePrices (ApplicationEngine.OpCodePrices.cs) and the spec's own OPCODE_PRICE_TABLE_V391. (Separately, also worth fixing in the same file: INITSLOT 16->64, NEWARRAY/NEWARRAY_T/NEWSTRUCT 2048->512 — but those are outside this claim.)

### [consensus] gas.py INITSLOT base price 16 instead of C# 64
- spec: `src/neo/vm/gas.py:102`
- csharp: `neo_csharp/src/Neo/SmartContract/ApplicationEngine.OpCodePrices.cs:108`
- fix: In /Users/jinghuiliao/git/r3e/neo-execution-specs/src/neo/vm/gas.py line 102, change `OpCode.INITSLOT: 16,` to `OpCode.INITSLOT: 64,` (i.e. 1 << 6) to match C# ApplicationEngine.OpCodePrices.cs:108. Leave INITSSLOT (line 101) at 16 (1<<4) — it already matches.

### [consensus] gas.py NEWARRAY/NEWARRAY_T/NEWSTRUCT base price 2048 instead of C# 512
- spec: `src/neo/vm/gas.py:209,210,212`
- csharp: `neo_csharp/src/Neo/SmartContract/ApplicationEngine.OpCodePrices.cs:203,204,206`
- fix: In src/neo/vm/gas.py set OPCODE_PRICE[OpCode.NEWARRAY] = OPCODE_PRICE[OpCode.NEWARRAY_T] = OPCODE_PRICE[OpCode.NEWSTRUCT] = 512 (1<<9), replacing the current 2048 at lines 209, 210, 212. Leave NEWARRAY0/NEWSTRUCT0 at 16 (already correct). No other code change needed: get_price() and execution_engine.py:105 consume the table directly.

## vm-numeric-bitwise

### [consensus] POW: spec does not truncate exponent to int32 before shift-bounds check
- spec: `vm/instructions/numeric.py:111-116`
- csharp: `neo_csharp_vm/src/Neo.VM/JumpTable/JumpTable.Numeric.cs:175-178`
- fix: In vm/instructions/numeric.py pow_ (line 111), truncate the popped exponent to a signed 32-bit int BEFORE the shift-bounds check, matching C#'s `(int)` cast: e.g. `raw = int(engine.pop().get_integer()); exponent = ((raw & 0xFFFFFFFF) ^ 0x80000000) - 0x80000000` (wrap into [-2^31, 2^31-1]). Then drop the explicit `if exponent < 0: raise` pre-check and rely on `assert_shift` to reject negatives (assert_shift already faults on shift<0), so that large exponents truncating into [0,256] HALT with `value**exponent` as in C#. Apply the same int32 truncation to shl (line 171) and shr (line 182) to fix the identical missing-truncation in those opcodes.

### [consensus] SHL: spec does not truncate shift to int32 before shift-bounds check
- spec: `vm/instructions/numeric.py:171-177`
- csharp: `neo_csharp_vm/src/Neo.VM/JumpTable/JumpTable.Numeric.cs:239-242`
- fix: Truncate the popped shift/exponent to a signed 32-bit int before assert_shift, mirroring C#'s `(int)BigInteger` (low 32 bits, two's-complement, non-throwing). In numeric.py add a helper, e.g. `def _to_int32(v): m = int(v) & 0xFFFFFFFF; return m - (1<<32) if m >= (1<<31) else m`, then use it in shl (line 171), shr (line 182), and pow (line 111): `shift = _to_int32(engine.pop().get_integer())` before `engine.limits.assert_shift(shift)`. This makes large shift values that wrap into [0,256] HALT (as C# does) instead of FAULTing, and values that wrap to negative or >256 still FAULT consistently with C#. Apply to all three handlers (SHL/SHR/POW) for full parity.

### [consensus] MODPOW: modulus==1 short-circuit returns 0 even when exponent==-1, where C# faults
- spec: `vm/instructions/numeric.py:155-157`
- csharp: `neo_csharp_vm/src/Neo.VM/JumpTable/JumpTable.Numeric.cs:223-225`
- fix: In numeric.py modpow, gate the modulus==1 short-circuit to the non-inverse path only: change lines 155-166 so that when exponent == -1 the code routes to the modular-inverse computation regardless of modulus==1, and the inverse path faults when modulus < 2 (mirroring C# ModInverse's ThrowIfLessThan(modulus, 2)). Concretely: move `if modulus == 1: push(Integer(0)); return` into the `else` (exponent != -1) branch, and in the `exponent == -1` branch add `if modulus < 2: raise InvalidOperationException(...)` (and, for full C# ModInverse parity, also fault when value <= 0) before computing pow(value, -1, modulus). This makes MODPOW(value, -1, 1) FAULT like C# while preserving the modulus==1 -> 0 result for non-negative exponents.

### [non-consensus] MODPOW: modular inverse accepts non-positive base where C# ModInverse faults
- spec: `vm/instructions/numeric.py:158-163`
- csharp: `neo_csharp_vm/src/Neo.VM/Utility.cs:82-96`
- fix: In numeric.py modpow, mirror C# control flow: evaluate the exponent==-1 (modular-inverse) branch the SAME way C# does — before any modulus==1 shortcut. For the inverse branch, fault when value <= 0 and when modulus < 2 (matching ModInverse's ThrowIfNegativeOrZero(value) and ThrowIfLessThan(modulus,2)) BEFORE computing pow(value,-1,modulus); only compute the inverse for value>0, modulus>=2. Keep the existing not-coprime ValueError→fault. Note: the current unconditional `if modulus == 1: push(0); return` (lines 155-157) must NOT short-circuit the exponent==-1 case — restrict that shortcut to exponent!=-1 (BigInteger.ModPow(value,exp,1)==0), or move the exponent==-1 handling ahead of it, so MODPOW with exponent=-1/modulus=1 faults like C# instead of pushing 0.

## vm-push-stack-slot

### [consensus] INITSLOT reverses argument order, mapping LDARG/STARG indices opposite to C#
- spec: `vm/instructions/slot.py:62-68`
- csharp: `neo_csharp_vm/src/Neo.VM/JumpTable/JumpTable.Slot.cs:53-61`
- fix: Remove the `items.reverse()` call at /Users/jinghuiliao/git/r3e/neo-execution-specs/src/neo/vm/instructions/slot.py:67 (and delete the now-wrong comment at line 66). The popped list must be stored in pop order: arguments[i] = the i-th popped item (i.e. items[0] = first pop = top of evaluation stack), matching C# `items[i] = engine.Pop()` followed by `new Slot(items, rc)` with no reversal. After the fix, INITSLOT builds: `items = [engine.pop() for _ in range(arg_count)]; ctx.arguments = engine.create_slot_from_items(items)`.

## vm-stackitem-types

### [consensus] EQUAL/NOTEQUAL on ByteString returns wrong result (base _equals_impl always False)
- spec: `src/neo/vm/types/byte_string.py:8-56 (no _equals_impl) + src/neo/vm/types/stack_item.py:68-78`
- csharp: `neo_csharp_vm/src/Neo.VM/Types/ByteString.cs:47-78; JumpTable.Bitwisee.cs:85-89`
- fix: Add a real `_equals_impl` to ByteString (byte_string.py) that mirrors C# ByteString.cs:54-78: compare `self._value == other._value` only when `other` is a ByteString, and enforce MaxComparableSize — raise InvalidOperationException if `len(self._value) > limit` or `limit == 0`, treat the comparison budget as `max(len(self._value), len(other._value), 1)`, and throw if `len(other._value) > limit`. Because `ExecutionEngineLimits` (limits.py:16-24) lacks a `max_comparable_size` field, either add that field (default MAX_COMPARABLE_SIZE=65536) and thread the mutable budget through `StackItem.equals`/`_equals_impl`, or reference the module constant MAX_COMPARABLE_SIZE for the bound check. Apply the same `_equals_impl` override to Boolean (boolean.py) comparing `_value` for two Boolean operands, since Boolean is broken by the identical mechanism. (Buffer is a reference type in C# and correctly uses identity, so the base `return False` for distinct Buffers is acceptable and should not be changed.)

### [consensus] EQUAL/NOTEQUAL on Boolean returns wrong result (base _equals_impl always False)
- spec: `src/neo/vm/types/boolean.py:7-40 (no _equals_impl)`
- csharp: `neo_csharp_vm/src/Neo.VM/Types/Boolean.cs:43-48; Types/StackItem.cs:122-130`
- fix: Add `_equals_impl` to Boolean (src/neo/vm/types/boolean.py): `def _equals_impl(self, other, limits): return isinstance(other, Boolean) and self._value == other._value` (type already checked by base equals, so the value compare matches Boolean.cs:43-48). Separately/also fix ByteString (src/neo/vm/types/byte_string.py), which has the same defect: add `_equals_impl` doing a content compare, and to fully match C# ByteString.Equals(other, ref maxComparableSize) it should fault when both operands exceed limits.MaxComparableSize (ExecutionEngineLimits.MaxComparableSize, default 65536) before comparing. Optionally consider routing all primitive types through `_equals_impl` consistently; Integer already implements it (integer.py:47-51) and is correct.

### [consensus] EQUAL/NOTEQUAL on Struct does not perform structural comparison (always False; missing limits/NotSupported semantics)
- spec: `src/neo/vm/types/struct.py:8-23 (no _equals_impl); src/neo/vm/types/array.py:11 (no _equals_impl)`
- csharp: `neo_csharp_vm/src/Neo.VM/Types/Struct.cs:76-122`
- fix: Add Struct._equals_impl(self, other, limits) in src/neo/vm/types/struct.py implementing the iterative deep comparison from C# Struct.cs:81-122 using two explicit stacks: initialize count=limits.MaxStackSize and max_comparable_size=limits.MaxComparableSize; on each pop decrement count and raise (VM fault) when it underflows; if node a is a ByteString, compare bytes while debiting max_comparable_size by its length (raise on budget exceed); otherwise debit max_comparable_size by 1 (raise when already 0), and if a is a Struct require b is a Struct with equal Count (else False) and push children, else fall back to a.equals(b). Return True only when both stacks drain equal. Note the limits param in the spec is currently typed `object` and may be passed as None for some callers; ensure engine.limits provides MaxStackSize/MaxComparableSize (it does in the EQUAL path). Array intentionally keeps reference-only equality (matches C# base). Out of scope but worth a separate finding: ByteString and Boolean also lack _equals_impl, so distinct equal-valued ByteStrings/Booleans likewise compare unequal under EQUAL — verify and fix separately.

### [non-consensus] ByteString.get_integer / get_boolean omit the Integer.MaxSize (32-byte) cast guard
- spec: `src/neo/vm/types/byte_string.py:26-36`
- csharp: `neo_csharp_vm/src/Neo.VM/Types/ByteString.cs:80-92`
- fix: In neo-execution-specs/src/neo/vm/types/byte_string.py, add the 32-byte (Integer.MAX_SIZE) guard to BOTH conversions, raising the same exception type the VM treats as a FAULT (the spec's invalid-cast / conversion error used elsewhere for bad casts), matching ByteString.cs:80-92:

  def get_boolean(self) -> bool:
      if len(self._value) > 32:  # Integer.MAX_SIZE
          raise <VmFault/InvalidCastError>()
      return any(b != 0 for b in self._value)

  def get_integer(self) -> BigInteger:
      if len(self._value) > 32:  # Integer.MAX_SIZE
          raise <VmFault/InvalidCastError>()
      return BigInteger.from_bytes_le(self._value)

Use the spec's existing fault-raising exception (the kind caught by the engine to produce FAULT) rather than a bare Python OverflowError/TypeError, so the abort path matches C#'s caught InvalidCastException. Reference the shared constant (Integer.MAX_SIZE / BigInteger.MAX_SIZE == 32) instead of a literal.

### [non-consensus] Struct.clone() lacks the MaxStackSize subitem limit (no fault on oversized clone)
- spec: `src/neo/vm/types/struct.py:15-23`
- csharp: `neo_csharp_vm/src/Neo.VM/Types/Struct.cs:38-67`
- fix: Two-part fix (both needed): (1) Port C# Struct.Clone(limits) into spec Struct.clone(limits) with the BFS queue + count=MaxStackSize-1 budget, raising InvalidOperationException("Beyond struct subitem clone limits!") when count<0, per Struct.cs:38-67. (2) Wire it into the three opcode handlers that currently skip struct value-cloning: in values() (compound.py:211-215) clone each item that is a Struct before adding; in append() (compound.py:242-250) clone item if it is a Struct before x.add; in setitem() (compound.py:253-276) clone value if it is a Struct before assignment — matching JumpTable.Compound.cs:348/418/435. Without part (2) the limit is unreachable AND structs are wrongly aliased by reference. DUP needs no change (C# does not clone there).
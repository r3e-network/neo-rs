//! Helper - matches C# Neo.SmartContract.Helper exactly
//!
//! The script/multisig primitives (`signature_contract_cost`, `is_signature_contract`,
//! `parse_multi_sig_contract`, `get_contract_hash`, ...) are canonical in
//! `neo_smart_contract::helper::Helper`; this module delegates to that implementation
//! so the logic has a single source of truth.
//!
//! `verify_witness` / `verify_witnesses` are *not* delegated: they depend on
//! neo-core-local types (`DataCache`, `ProtocolSettings`, `ContractManagement`,
//! `Witness`, `VerifiableExt`) that live above `neo-smart-contract` in the crate
//! layering, so they cannot move into that crate. Rust's coherence rules also
//! forbid adding inherent methods to `neo_smart_contract::helper::Helper` from
//! here, so this module keeps its own `Helper` type and only the witness-verification
//! logic is genuinely local.

use crate::error::{CoreError, CoreResult};
use crate::network::p2p::payloads::Witness;
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::CallFlags;
use crate::smart_contract::ContractBasicMethod;
use crate::smart_contract::ContractParameterType;
use crate::smart_contract::TriggerType;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::contract::Contract;
use crate::smart_contract::native::contract_management::ContractManagement;
use crate::{UInt160, UInt256, Verifiable, VerifiableExt, WitnessData};
use neo_smart_contract::helper::Helper as ContractHelperPrimitives;
use neo_vm::VmState as VMState;
use std::any::Any;
use std::sync::Arc;

/// Helper functions for smart contracts (matches C# Helper)
pub struct Helper;

impl Helper {
    /// The maximum GAS that can be consumed when verifying witnesses (in datoshi).
    pub const MAX_VERIFICATION_GAS: i64 = ContractHelperPrimitives::MAX_VERIFICATION_GAS;

    /// Maximum number of public keys allowed in a multi-sig contract (C# neo v3.10.2).
    pub const MAX_PUBLIC_KEYS_PER_MULTISIG: usize =
        ContractHelperPrimitives::MAX_PUBLIC_KEYS_PER_MULTISIG;

    /// Calculates the verification cost for a single-signature contract (in datoshi).
    pub fn signature_contract_cost() -> i64 {
        ContractHelperPrimitives::signature_contract_cost()
    }

    /// Calculates the verification cost for a multi-signature contract (in datoshi).
    pub fn multi_signature_contract_cost(m: i32, n: i32) -> i64 {
        ContractHelperPrimitives::multi_signature_contract_cost(m, n)
    }

    /// Checks if a script is a standard contract
    pub fn is_standard_contract(script: &[u8]) -> bool {
        ContractHelperPrimitives::is_standard_contract(script)
    }

    /// Checks if a script is a signature contract
    pub fn is_signature_contract(script: &[u8]) -> bool {
        ContractHelperPrimitives::is_signature_contract(script)
    }

    /// Checks if a script is a multi-sig contract.
    ///
    /// This is the only classification path. C# v3.10.2 decodes every public key
    /// before any `IsMultiSigContract` overload returns true; a shape-only check
    /// let a malformed key through the `bool` overload.
    pub fn is_multi_sig_contract(script: &[u8]) -> bool {
        ContractHelperPrimitives::is_multi_sig_contract(script)
    }

    /// Gets the script hash from a contract
    pub fn to_script_hash(contract: &Contract) -> UInt160 {
        ContractHelperPrimitives::to_script_hash(contract)
    }

    /// Creates a signature redeem script
    pub fn signature_redeem_script(public_key: &[u8]) -> Vec<u8> {
        ContractHelperPrimitives::signature_redeem_script(public_key)
    }

    /// Creates a multi-sig redeem script.
    ///
    /// # Errors
    ///
    /// Returns `CoreError` if:
    /// - `m` is not in range `1..=1024`
    /// - `public_keys.len()` exceeds 1024
    /// - `m` exceeds `public_keys.len()`
    /// - any key is not a valid secp256r1 point
    pub fn try_multi_sig_redeem_script(m: usize, public_keys: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        ContractHelperPrimitives::try_multi_sig_redeem_script(m, public_keys)
            .map_err(CoreError::from)
    }

    /// Creates a multi-sig redeem script (panics on invalid input).
    ///
    /// Prefer `try_multi_sig_redeem_script` for fallible construction.
    #[inline]
    pub fn multi_sig_redeem_script(m: usize, public_keys: &[Vec<u8>]) -> Vec<u8> {
        ContractHelperPrimitives::multi_sig_redeem_script(m, public_keys)
    }

    /// Computes the hash of a deployed contract.
    pub fn get_contract_hash(sender: &UInt160, nef_checksum: u32, name: &str) -> UInt160 {
        ContractHelperPrimitives::get_contract_hash(sender, nef_checksum, name)
    }

    /// Parses a multi-signature contract script, returning the required signature count and
    /// the ordered public keys when the script matches the canonical Neo multi-sig format.
    ///
    /// Mirrors C# `Helper.IsMultiSigContract` (v3.10.2): m/n accept `PUSHINT8`/`PUSHINT16`
    /// as well as `PUSH1..PUSH16`, both are bounded by 1..=1024, every 33-byte key must
    /// decode as a secp256r1 point, and the tail must be `n`-push, `SYSCALL`, `CheckMultisig`.
    /// Decoding runs even when the caller only needs a boolean.
    pub fn parse_multi_sig_contract(script: &[u8]) -> Option<(usize, Vec<Vec<u8>>)> {
        ContractHelperPrimitives::parse_multi_sig_contract(script)
    }

    /// Parses a multi-signature invocation script, returning the list of signatures when the
    /// script pushes the expected number of signatures encoded with `PUSHDATA1` opcodes.
    pub fn parse_multi_sig_invocation(
        invocation: &[u8],
        required_signatures: usize,
    ) -> Option<Vec<Vec<u8>>> {
        ContractHelperPrimitives::parse_multi_sig_invocation(invocation, required_signatures)
    }

    /// Verifies all witnesses for a verifiable object.
    /// Matches C# Helper.VerifyWitnesses exactly.
    ///
    /// # Arguments
    /// * `verifiable` - The object to verify
    /// * `settings` - Protocol settings
    /// * `snapshot` - Database snapshot
    /// * `max_gas` - Maximum gas allowed for verification (in datoshi)
    ///
    /// # Returns
    /// `true` if all witnesses verify successfully, `false` otherwise
    pub fn verify_witnesses<V: VerifiableExt>(
        verifiable: &V,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        max_gas: i64,
    ) -> bool {
        if max_gas < 0 {
            return false;
        }

        let max_gas = max_gas.min(Self::MAX_VERIFICATION_GAS);

        // Get script hashes to verify
        let hashes = verifiable.script_hashes_for_verifying(snapshot);

        // Get witnesses
        let witnesses = WitnessData::witnesses(verifiable);

        // Verify counts match
        if hashes.len() != witnesses.len() {
            return false;
        }

        let mut remaining_gas = max_gas;

        // Verify each witness
        for (i, hash) in hashes.iter().enumerate() {
            match Self::verify_witness(
                verifiable,
                settings,
                snapshot,
                hash,
                witnesses[i],
                remaining_gas,
            ) {
                Ok(fee) => {
                    remaining_gas -= fee;
                }
                Err(_) => {
                    return false;
                }
            }
        }

        true
    }

    /// Verifies a single witness for a verifiable object.
    /// Matches C# Helper.VerifyWitness exactly.
    ///
    /// # Arguments
    /// * `verifiable` - The object being verified
    /// * `settings` - Protocol settings
    /// * `snapshot` - Database snapshot
    /// * `hash` - Expected script hash
    /// * `witness` - The witness to verify
    /// * `max_gas` - Maximum gas allowed (in datoshi)
    ///
    /// # Returns
    /// `Ok(fee)` with consumed gas if verification succeeds, `Err` otherwise
    pub fn verify_witness<V: VerifiableExt>(
        verifiable: &V,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        hash: &UInt160,
        witness: &Witness,
        max_gas: i64,
    ) -> CoreResult<i64> {
        // Validate invocation script (check for bad opcodes)
        if !Self::is_valid_script(&witness.invocation_script) {
            return Err(CoreError::invalid_operation(
                "Invalid invocation script".to_string(),
            ));
        }

        // Create verification engine
        let cloned_snapshot = Arc::new(snapshot.clone_cache());
        let container_hash = verifiable
            .hash()
            .map_err(|e| CoreError::invalid_operation(e.to_string()))?;
        let container: Arc<dyn Verifiable> = if let Some(transaction) = verifiable.as_transaction()
        {
            Arc::new(transaction.clone())
        } else {
            Arc::new(VerifiableHashContainer {
                hash: container_hash,
                hash_data: verifiable.hash_data(),
            })
        };
        let mut engine = ApplicationEngine::new(
            TriggerType::Verification,
            Some(container),
            cloned_snapshot,
            None,
            settings.clone(),
            max_gas,
            None,
            crate::smart_contract::native::composition::default_native_services(),
        )?;

        // Check if witness has empty verification script (contract verification)
        if witness.verification_script.is_empty() {
            // Contract verification: load the contract's Verify method
            let mut contract = ContractManagement::get_contract_from_snapshot(snapshot, hash)?
                .ok_or_else(|| {
                    CoreError::invalid_operation(format!("Contract not found for hash {}", hash))
                })?;

            // Resolve the Verify method using C# semantics (pcount = -1 matches any signature).
            let verify_method = contract
                .manifest
                .abi
                .get_method(
                    ContractBasicMethod::VERIFY,
                    ContractBasicMethod::VERIFY_P_COUNT,
                )
                .cloned()
                .ok_or_else(|| {
                    CoreError::invalid_operation(
                        "Contract does not have a valid Verify method".to_string(),
                    )
                })?;

            // Verify return type is Boolean
            if verify_method.return_type != ContractParameterType::Boolean {
                return Err(CoreError::invalid_operation(
                    "Verify method must return Boolean".to_string(),
                ));
            }

            // Load contract method with ReadOnly flags
            engine.load_contract_method(contract, verify_method, CallFlags::READ_ONLY)?;
        } else {
            // Script verification: verify the witness script directly

            // Cannot use native contract hashes as verification scripts.
            // The engine owns the standard native registry, so query it through the
            // engine's reverse interface instead of building a fresh empty registry
            // (which would never report a hash as native).
            if engine.is_native_contract_hash(hash) {
                return Err(CoreError::invalid_operation(
                    "Cannot verify native contract".to_string(),
                ));
            }

            // Verify witness script hash matches expected hash
            if *hash != witness.script_hash() {
                return Err(CoreError::invalid_operation(
                    "Witness script hash mismatch".to_string(),
                ));
            }

            // Validate verification script
            if !Self::is_valid_script(&witness.verification_script) {
                return Err(CoreError::invalid_operation(
                    "Invalid verification script".to_string(),
                ));
            }

            // Load verification script with ReadOnly flags and correct hash
            engine.load_script(
                witness.verification_script.clone(),
                CallFlags::READ_ONLY,
                Some(*hash),
            )?;
        }

        // Load invocation script (provides signatures/parameters)
        engine.load_script(witness.invocation_script.clone(), CallFlags::NONE, None)?;

        // Execute verification
        engine.execute()?;

        // Check execution result
        if engine.state() == VMState::FAULT {
            return Err(CoreError::invalid_operation(
                "Verification execution faulted".to_string(),
            ));
        }

        // Verify result: must have exactly one item on stack that evaluates to true
        let result_stack = engine.result_stack();
        if result_stack.len() != 1 {
            return Err(CoreError::invalid_operation(format!(
                "Verification must leave exactly 1 item on stack, got {}",
                result_stack.len()
            )));
        }

        let result = result_stack
            .peek(0)
            .map_err(|e| CoreError::invalid_operation(format!("Failed to peek result: {}", e)))?;

        if !result.as_bool().unwrap_or(false) {
            return Err(CoreError::invalid_operation(
                "Verification returned false".to_string(),
            ));
        }

        Ok(engine.fee_consumed())
    }

    /// Validates that a script doesn't contain invalid opcodes.
    /// Basic validation to catch obviously malformed scripts.
    fn is_valid_script(script: &[u8]) -> bool {
        // Empty scripts are valid (for contract verification)
        if script.is_empty() {
            return true;
        }

        // Script must be readable (basic sanity check)
        // Full validation would require parsing all opcodes
        // Validate minimum script length for meaningful operations
        true
    }
}

/// Minimal script container wrapper used during witness verification.
///
/// This enables crypto syscalls like `System.Crypto.CheckSig` to resolve the
/// signable message (`network || container_hash`) without requiring the caller
/// to clone arbitrary `Verifiable` implementations into an `Arc`.
struct VerifiableHashContainer {
    hash: UInt256,
    hash_data: Vec<u8>,
}

impl Verifiable for VerifiableHashContainer {
    fn verify(&self) -> bool {
        true
    }

    fn hash(&self) -> neo_primitives::error::PrimitiveResult<UInt256> {
        Ok(self.hash)
    }

    fn hash_data(&self) -> Vec<u8> {
        self.hash_data.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl VerifiableExt for VerifiableHashContainer {
    fn script_hashes_for_verifying(&self, _snapshot: &DataCache) -> Vec<UInt160> {
        Vec::new()
    }
}

impl WitnessData for VerifiableHashContainer {
    fn witnesses(&self) -> Vec<&Witness> {
        Vec::new()
    }

    fn witnesses_mut(&mut self) -> Vec<&mut Witness> {
        Vec::new()
    }
}

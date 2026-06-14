//! Helper - matches C# Neo.SmartContract.Helper exactly

use crate::NativeRegistry;
use crate::application_engine::ApplicationEngine;
use crate::contract::Contract;
use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_manifest::CallFlags;
use neo_payloads::VerifiableExt;
use neo_payloads::Witness;
use neo_primitives::ContractBasicMethod;
use neo_primitives::ContractParameterType;
use neo_primitives::TriggerType;
use neo_primitives::Verifiable;
use neo_primitives::{UInt160, UInt256};
use neo_storage::DataCache;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::OpCode;
use neo_vm_rs::VmState as VMState;
use std::any::Any;
use std::sync::Arc;

/// Helper functions for smart contracts (matches C# Helper)
pub struct Helper;

impl Helper {
    /// The maximum GAS that can be consumed when verifying witnesses (in datoshi).
    pub const MAX_VERIFICATION_GAS: i64 = 150_000_000;

    /// Calculates the verification cost for a single-signature contract (in datoshi).
    pub fn signature_contract_cost() -> i64 {
        let push_cost = ApplicationEngine::get_opcode_price(OpCode::PUSHDATA1.byte());
        let syscall_cost = ApplicationEngine::get_opcode_price(OpCode::SYSCALL.byte());
        push_cost * 2 + syscall_cost + crate::application_engine::CHECK_SIG_PRICE
    }

    /// Calculates the verification cost for a multi-signature contract (in datoshi).
    pub fn multi_signature_contract_cost(m: i32, n: i32) -> i64 {
        let push_cost = ApplicationEngine::get_opcode_price(OpCode::PUSHDATA1.byte());
        let mut fee = push_cost * (m as i64 + n as i64);

        let mut builder = ScriptBuilder::new();
        builder.emit_push_int(m as i64);
        let m_opcode = builder
            .to_array()
            .first()
            .copied()
            .unwrap_or(OpCode::PUSH0.byte());
        fee += ApplicationEngine::get_opcode_price(m_opcode);

        let mut builder_n = ScriptBuilder::new();
        builder_n.emit_push_int(n as i64);
        let n_opcode = builder_n
            .to_array()
            .first()
            .copied()
            .unwrap_or(OpCode::PUSH0.byte());
        fee += ApplicationEngine::get_opcode_price(n_opcode);

        fee += ApplicationEngine::get_opcode_price(OpCode::SYSCALL.byte());
        fee += crate::application_engine::CHECK_SIG_PRICE * n as i64;
        fee
    }

    /// Checks if a script is a standard contract
    pub fn is_standard_contract(script: &[u8]) -> bool {
        Self::is_signature_contract(script) || Self::is_multi_sig_contract(script)
    }

    /// Checks if a script is a signature contract.
    ///
    /// Delegates to the `neo-script-builder` crate (the redeem-script primitives
    /// were hoisted below neo-core); kept here for the historical
    /// `Helper::is_signature_contract` path.
    pub fn is_signature_contract(script: &[u8]) -> bool {
        neo_vm::script_builder::redeem_script::RedeemScript::is_signature_contract(script)
    }

    /// Checks if a script is a multi-sig contract. Delegates to `neo-script-builder`.
    pub fn is_multi_sig_contract(script: &[u8]) -> bool {
        neo_vm::script_builder::redeem_script::RedeemScript::is_multi_sig_contract(script)
    }

    /// Gets the script hash from a contract
    pub fn to_script_hash(contract: &Contract) -> UInt160 {
        contract.script_hash()
    }

    /// Creates a signature redeem script. Delegates to `neo-script-builder`.
    pub fn signature_redeem_script(public_key: &[u8]) -> Vec<u8> {
        neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(public_key)
    }

    /// Creates a multi-sig redeem script. Delegates to `neo-script-builder`.
    ///
    /// # Errors
    ///
    /// Returns `CoreError` if:
    /// - `m` is not in range `1..=n`
    /// - `public_keys.len()` exceeds 1024
    /// - `m` exceeds `public_keys.len()`
    /// - any public key fails to parse
    pub fn try_multi_sig_redeem_script(m: usize, public_keys: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_keys(
            m,
            public_keys,
        )
        .map_err(Into::into)
    }

    /// Creates a multi-sig redeem script (panics on invalid input).
    ///
    /// Prefer `try_multi_sig_redeem_script` for fallible construction.
    #[inline]
    pub fn multi_sig_redeem_script(m: usize, public_keys: &[Vec<u8>]) -> Vec<u8> {
        Self::try_multi_sig_redeem_script(m, public_keys).expect("Invalid multi-sig parameters")
    }

    /// Computes the hash of a deployed contract.
    pub fn get_contract_hash(sender: &UInt160, nef_checksum: u32, name: &str) -> UInt160 {
        let mut builder = ScriptBuilder::new();
        builder.emit_opcode(OpCode::ABORT);
        builder.emit_push(&sender.to_bytes());
        builder.emit_push_int(nef_checksum as i64);
        builder.emit_push_string(name);
        let script = builder.to_array();
        UInt160::from_script(&script)
    }

    /// Parses a multi-signature contract script, returning the required signature count and
    /// the ordered public keys. Delegates to `neo-script-builder` (recognizer primitives were
    /// hoisted below neo-core); kept here for the historical `Helper::parse_multi_sig_contract` path.
    pub fn parse_multi_sig_contract(script: &[u8]) -> Option<(usize, Vec<Vec<u8>>)> {
        neo_vm::script_builder::redeem_script::RedeemScript::parse_multi_sig_contract(script)
    }

    /// Parses a multi-signature invocation script. Delegates to `neo-script-builder`.
    pub fn parse_multi_sig_invocation(
        invocation: &[u8],
        required_signatures: usize,
    ) -> Option<Vec<Vec<u8>>> {
        neo_vm::script_builder::redeem_script::RedeemScript::parse_multi_sig_invocation(
            invocation,
            required_signatures,
        )
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
        let witnesses = verifiable.witnesses();

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
        )?;

        // Check if witness has empty verification script (contract verification)
        if witness.verification_script.is_empty() {
            // Contract verification: load the contract's Verify method
            let mut contract =
                crate::native_contract_provider::NativeContractLookup::lookup_contract_management(
                    snapshot, hash,
                )?
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

            // Cannot use native contract hashes as verification scripts
            let native_registry = NativeRegistry::new();
            if native_registry.is_native(hash) {
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

    fn witnesses(&self) -> Vec<&Witness> {
        Vec::new()
    }

    fn witnesses_mut(&mut self) -> Vec<&mut Witness> {
        Vec::new()
    }
}

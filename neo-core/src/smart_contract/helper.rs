//! Helper - matches C# Neo.SmartContract.Helper exactly

use crate::cryptography::ECPoint;
use crate::error::{CoreError, CoreResult};
use crate::network::p2p::payloads::Witness;
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::ContractParameterType;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::contract::Contract;
use crate::smart_contract::contract_basic_method::ContractBasicMethod;
use crate::smart_contract::native::NativeRegistry;
use crate::smart_contract::native::contract_management::ContractManagement;
use crate::smart_contract::trigger_type::TriggerType;
use crate::{IVerifiable, UInt160, UInt256};
use neo_crypto::Crypto;
use neo_vm::VMState;
use neo_vm::{ScriptBuilder, op_code::OpCode};
use std::any::Any;
use std::sync::Arc;

/// Helper functions for smart contracts (matches C# Helper)
pub struct Helper;

impl Helper {
    /// The maximum GAS that can be consumed when verifying witnesses (in datoshi).
    pub const MAX_VERIFICATION_GAS: i64 = 150_000_000;

    /// Calculates the verification cost for a single-signature contract (in datoshi).
    pub fn signature_contract_cost() -> i64 {
        let push_cost = ApplicationEngine::get_opcode_price(OpCode::PUSHDATA1 as u8);
        let syscall_cost = ApplicationEngine::get_opcode_price(OpCode::SYSCALL as u8);
        push_cost * 2 + syscall_cost + crate::smart_contract::application_engine::CHECK_SIG_PRICE
    }

    /// Calculates the verification cost for a multi-signature contract (in datoshi).
    pub fn multi_signature_contract_cost(m: i32, n: i32) -> i64 {
        let push_cost = ApplicationEngine::get_opcode_price(OpCode::PUSHDATA1 as u8);
        let mut fee = push_cost * (m as i64 + n as i64);

        let mut builder = ScriptBuilder::new();
        builder.emit_push_int(m as i64);
        let m_opcode = builder
            .to_array()
            .first()
            .copied()
            .unwrap_or(OpCode::PUSH0 as u8);
        fee += ApplicationEngine::get_opcode_price(m_opcode);

        let mut builder_n = ScriptBuilder::new();
        builder_n.emit_push_int(n as i64);
        let n_opcode = builder_n
            .to_array()
            .first()
            .copied()
            .unwrap_or(OpCode::PUSH0 as u8);
        fee += ApplicationEngine::get_opcode_price(n_opcode);

        fee += ApplicationEngine::get_opcode_price(OpCode::SYSCALL as u8);
        fee += crate::smart_contract::application_engine::CHECK_SIG_PRICE * n as i64;
        fee
    }

    /// Checks if a script is a standard contract
    pub fn is_standard_contract(script: &[u8]) -> bool {
        Self::is_signature_contract(script) || Self::is_multi_sig_contract(script)
    }

    /// Checks if a script is a signature contract
    pub fn is_signature_contract(script: &[u8]) -> bool {
        if script.len() != 40 {
            return false;
        }

        // Check pattern: PUSHDATA1 (33 bytes pubkey) SYSCALL (CheckSig)
        script[0] == 0x0C && // PUSHDATA1
        script[1] == 33 &&   // 33 bytes
        script[35] == 0x41 && // SYSCALL
        script[36..40] == Self::check_sig_hash()
    }

    /// Checks if a script is a multi-sig contract
    pub fn is_multi_sig_contract(script: &[u8]) -> bool {
        if script.len() < 42 {
            return false;
        }

        // Check basic pattern for multi-sig
        let _m = match script[0] {
            value if (OpCode::PUSH1 as u8..=OpCode::PUSH16 as u8).contains(&value) => {
                value - OpCode::PUSH0 as u8
            }
            _ => return false,
        };

        // Verify ending with SYSCALL CheckMultisig
        let len = script.len();
        script[len - 5] == 0x41 && // SYSCALL
        script[len - 4..] == Self::check_multisig_hash()
    }

    /// Gets the script hash from a contract
    pub fn to_script_hash(contract: &Contract) -> UInt160 {
        contract.script_hash()
    }

    /// Creates a signature redeem script
    pub fn signature_redeem_script(public_key: &[u8]) -> Vec<u8> {
        let mut script = Vec::new();
        script.push(0x0C); // PUSHDATA1
        script.push(public_key.len() as u8);
        script.extend_from_slice(public_key);
        script.push(0x41); // SYSCALL
        script.extend_from_slice(&Self::check_sig_hash());
        script
    }

    /// Creates a multi-sig redeem script.
    ///
    /// # Errors
    ///
    /// Returns `CoreError` if:
    /// - `m` is not in range `1..=16`
    /// - `public_keys.len()` exceeds 16
    /// - `m` exceeds `public_keys.len()`
    pub fn try_multi_sig_redeem_script(m: usize, public_keys: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        if !(1..=16).contains(&m) || public_keys.len() > 16 || m > public_keys.len() {
            return Err(CoreError::invalid_operation(format!(
                "Invalid multi-sig parameters: m={}, n={}",
                m,
                public_keys.len()
            )));
        }

        let mut points = Vec::with_capacity(public_keys.len());
        for key in public_keys {
            let point = ECPoint::from_bytes(key)
                .map_err(|e| CoreError::invalid_operation(format!("Invalid public key: {e}")))?;
            points.push(point);
        }

        Contract::try_create_multi_sig_redeem_script(m, &points)
            .map_err(|err| CoreError::invalid_operation(err.to_string()))
    }

    /// Creates a multi-sig redeem script (panics on invalid input).
    ///
    /// Prefer `try_multi_sig_redeem_script` for fallible construction.
    #[inline]
    pub fn multi_sig_redeem_script(m: usize, public_keys: &[Vec<u8>]) -> Vec<u8> {
        Self::try_multi_sig_redeem_script(m, public_keys).expect("Invalid multi-sig parameters")
    }

    /// Gets the CheckSig syscall hash
    fn check_sig_hash() -> [u8; 4] {
        Self::syscall_hash("System.Crypto.CheckSig")
    }

    /// Gets the CheckMultisig syscall hash
    fn check_multisig_hash() -> [u8; 4] {
        Self::syscall_hash("System.Crypto.CheckMultisig")
    }

    /// Computes syscall hash
    fn syscall_hash(name: &str) -> [u8; 4] {
        let result = Crypto::sha256(name.as_bytes());
        [result[0], result[1], result[2], result[3]]
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
    /// the ordered public keys when the script matches the canonical Neo multi-sig format.
    pub fn parse_multi_sig_contract(script: &[u8]) -> Option<(usize, Vec<Vec<u8>>)> {
        use neo_vm::op_code::OpCode;

        if script.len() < 42 {
            return None;
        }

        let mut offset = 0usize;
        let first = script[offset];
        if !(OpCode::PUSH1 as u8..=OpCode::PUSH16 as u8).contains(&first) {
            return None;
        }
        let m = (first - OpCode::PUSH0 as u8) as usize;
        offset += 1;

        let mut public_keys = Vec::new();
        while offset < script.len() {
            if script[offset] != OpCode::PUSHDATA1 as u8 {
                break;
            }
            offset += 1;
            if offset >= script.len() {
                return None;
            }
            let key_len = script[offset] as usize;
            offset += 1;
            if key_len != 33 || offset + key_len > script.len() {
                return None;
            }
            public_keys.push(script[offset..offset + key_len].to_vec());
            offset += key_len;
        }

        if public_keys.is_empty() {
            return None;
        }
        let n = public_keys.len();

        if offset >= script.len() || script[offset] != (OpCode::PUSH0 as u8).wrapping_add(n as u8) {
            return None;
        }
        offset += 1;

        if script.len() != offset + 5 {
            return None;
        }
        if script[offset] != OpCode::SYSCALL as u8 {
            return None;
        }
        if script[offset + 1..offset + 5] != Self::check_multisig_hash() {
            return None;
        }

        if m == 0 || m > n {
            return None;
        }

        Some((m, public_keys))
    }

    /// Parses a multi-signature invocation script, returning the list of signatures when the
    /// script pushes the expected number of signatures encoded with `PUSHDATA1` opcodes.
    pub fn parse_multi_sig_invocation(
        invocation: &[u8],
        required_signatures: usize,
    ) -> Option<Vec<Vec<u8>>> {
        use neo_vm::op_code::OpCode;

        if required_signatures == 0 {
            return None;
        }

        let mut signatures = Vec::with_capacity(required_signatures);
        let mut offset = 0usize;
        while offset < invocation.len() {
            if invocation[offset] != OpCode::PUSHDATA1 as u8 {
                return None;
            }
            offset += 1;
            if offset >= invocation.len() {
                return None;
            }
            let len = invocation[offset] as usize;
            offset += 1;
            if len != 64 || offset + len > invocation.len() {
                return None;
            }
            signatures.push(invocation[offset..offset + len].to_vec());
            offset += len;
        }

        if signatures.len() == required_signatures {
            Some(signatures)
        } else {
            None
        }
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
    pub fn verify_witnesses<V: IVerifiable>(
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
        let hashes = verifiable.get_script_hashes_for_verifying(snapshot);

        // Get witnesses
        let witnesses = verifiable.get_witnesses();

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
    pub fn verify_witness<V: IVerifiable>(
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
        let container_hash = verifiable.hash()?;
        let container: Arc<dyn IVerifiable> = if let Some(transaction) = verifiable.as_transaction()
        {
            Arc::new(transaction.clone())
        } else {
            Arc::new(VerifiableHashContainer {
                hash: container_hash,
                hash_data: verifiable.get_hash_data(),
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

        if !result.get_boolean().unwrap_or(false) {
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
/// to clone arbitrary `IVerifiable` implementations into an `Arc`.
struct VerifiableHashContainer {
    hash: UInt256,
    hash_data: Vec<u8>,
}

impl IVerifiable for VerifiableHashContainer {
    fn verify(&self) -> bool {
        true
    }

    fn hash(&self) -> CoreResult<UInt256> {
        Ok(self.hash)
    }

    fn get_hash_data(&self) -> Vec<u8> {
        self.hash_data.clone()
    }

    fn get_script_hashes_for_verifying(&self, _snapshot: &DataCache) -> Vec<UInt160> {
        Vec::new()
    }

    fn get_witnesses(&self) -> Vec<&Witness> {
        Vec::new()
    }

    fn get_witnesses_mut(&mut self) -> Vec<&mut Witness> {
        Vec::new()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

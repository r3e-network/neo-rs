use std::any::Any;
use std::collections::{ HashSet};
use std::convert::TryFrom;
use std::io::{self};
use std::mem;
use NeoRust::neo_types::VMState;
use NeoRust::prelude::VarSizeTrait;
use neo_json::jtoken::JToken;
use neo_vm::{OpCode, ScriptBuilder};
use crate::contract::{CallFlags, TriggerType};
use crate::cryptography::{Crypto, Helper};
use crate::io::binary_writer::BinaryWriter;
use crate::io::memory_reader::MemoryReader;
use crate::ledger::transaction_verification_context::TransactionVerificationContext;
use crate::ledger::verify_result::VerifyResult;
use crate::neo_contract::application_engine::ApplicationEngine;
use crate::neo_contract::helper::helper::MAX_VERIFICATION_GAS;
use crate::network::payloads::{Signer, Witness};
use crate::network::transaction_attribute::transaction_attribute::TransactionAttribute;
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::store::Snapshot;
use neo_type::H160;
use neo_type::H256;

/// Represents a transaction.
#[derive(Clone)]
pub struct Transaction {
    version: u8,
    nonce: u32,
    // In the unit of datoshi, 1 datoshi = 1e-8 GAS
    sys_fee: i64,
    // In the unit of datoshi, 1 datoshi = 1e-8 GAS
    net_fee: i64,
    valid_until_block: u32,
    pub(crate) signers: Vec<Signer>,
    attributes: Vec<Box<dyn TransactionAttribute>>,
    script: Vec<u8>,
    witnesses: Vec<Witness>,
}

impl Transaction {
    /// The maximum size of a transaction.
    pub const MAX_TRANSACTION_SIZE: usize = 102400;

    /// The maximum number of attributes that can be contained within a transaction.
    pub const MAX_TRANSACTION_ATTRIBUTES: usize = 16;

    /// The size of a transaction header.
    pub const HEADER_SIZE: usize = 
        mem::size_of::<u8>() +  // Version
        mem::size_of::<u32>() + // Nonce
        mem::size_of::<i64>() + // SystemFee
        mem::size_of::<i64>() + // NetworkFee
        mem::size_of::<u32>();  // ValidUntilBlock

    /// The network fee of the transaction divided by its size.
    pub fn fee_per_byte(&self) -> i64 {
        self.net_fee / self.size() as i64
    }

    /// The hash of the transaction.
    pub fn hash(&self) -> H256 {
        self.calculate_hash()
    }

    /// The network fee of the transaction.
    pub fn network_fee(&self) -> i64 {
        self.net_fee
    }

    /// The nonce of the transaction.
    pub fn nonce(&self) -> u32 {
        self.nonce
    }

    /// The script of the transaction.
    pub fn script(&self) -> &[u8] {
        &self.script
    }

    /// The sender is the first signer of the transaction, regardless of its WitnessScope.
    pub fn sender(&self) -> H160 {
        self.signers[0].account.clone()
    }

    /// The signers of the transaction.
    pub fn signers(&self) -> &[Signer] {
        &self.signers
    }

    /// The size of the transaction.
    pub fn size(&self) -> usize {
        Self::HEADER_SIZE +
            self.signers.var_size() +
            self.attributes.var_size() +
            self.script.var_size() +
            self.witnesses.var_size()
    }

    /// The system fee of the transaction.
    pub fn system_fee(&self) -> i64 {
        self.sys_fee
    }

    /// Indicates that the transaction is only valid before this block height.
    pub fn valid_until_block(&self) -> u32 {
        self.valid_until_block
    }

    /// The version of the transaction.
    pub fn version(&self) -> u8 {
        self.version
    }

    /// The witnesses of the transaction.
    pub fn witnesses(&self) -> &[Witness] {
        &self.witnesses
    }

    pub fn deserialize(&mut self, reader: &mut MemoryReader) -> io::Result<()> {
        let start_position = reader.position();
        self.deserialize_unsigned(reader)?;
        self.witnesses = reader.read_serializable_array::<Witness>(self.signers.len())?;
        if self.witnesses.len() != self.signers.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Witness count mismatch"));
        }
        Ok(())
    }

    fn deserialize_attributes(reader: &mut MemoryReader, max_count: usize) -> io::Result<Vec<Box<dyn TransactionAttribute>>> {
        let count = reader.read_var_int(max_count as u64)? as usize;
        let mut attributes = Vec::with_capacity(count);
        let mut hashset = HashSet::new();
        for _ in 0..count {
            let attribute = TransactionAttribute::deserialize_from(reader)?;
            if !attribute.allow_multiple() && !hashset.insert(attribute.type_()) {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "Duplicate attribute type"));
            }
            attributes.push(attribute);
        }
        Ok(attributes)
    }

    fn deserialize_signers(reader: &mut MemoryReader, max_count: usize) -> io::Result<Vec<Signer>> {
        let count = reader.read_var_int(max_count as u64)? as usize;
        if count == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "No signers"));
        }
        let mut signers = Vec::with_capacity(count);
        let mut hashset = HashSet::new();
        for _ in 0..count {
            let signer = reader.read_serializable::<Signer>()?;
            if !hashset.insert(signer.account.clone()) {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "Duplicate signer"));
            }
            signers.push(signer);
        }
        Ok(signers)
    }

    pub fn deserialize_unsigned(&mut self, reader: &mut MemoryReader) -> io::Result<()> {
        self.version = reader.read_u8()?;
        if self.version > 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid version"));
        }
        self.nonce = reader.read_u32()?;
        self.sys_fee = reader.read_i64()?;
        if self.sys_fee < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid system fee"));
        }
        self.net_fee = reader.read_i64()?;
        if self.net_fee < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid network fee"));
        }
        if self.sys_fee.checked_add(self.net_fee).is_none() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Fee overflow"));
        }
        self.valid_until_block = reader.read_u32()?;
        self.signers = Self::deserialize_signers(reader, Self::MAX_TRANSACTION_ATTRIBUTES)?;
        self.attributes = Self::deserialize_attributes(reader, Self::MAX_TRANSACTION_ATTRIBUTES - self.signers.len())?;
        self.script = reader.read_var_bytes(u16::MAX as usize)?;
        if self.script.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Empty script"));
        }
        Ok(())
    }
    pub fn serialize(&self, writer: &mut BinaryWriter) -> io::Result<()> {
        writer.write_u8(self.version)?;
        writer.write_u32(self.nonce)?;
        writer.write_i64(self.sys_fee)?;
        writer.write_i64(self.net_fee)?;
        writer.write_u32(self.valid_until_block)?;
        writer.write_var_int(self.signers.len() as u64)?;
        for signer in &self.signers {
            writer.write_serializable(signer)?;
        }
        writer.write_var_int(self.attributes.len() as u64)?;
        for attribute in &self.attributes {
            writer.write_serializable(attribute.as_ref())?;
        }
        writer.write_var_bytes(&self.script)?;
        writer.write_var_int(self.witnesses.len() as u64)?;
        for witness in &self.witnesses {
            writer.write_serializable(witness)?;
        }
        Ok(())
    }

    pub fn verify(&self, snapshot: &Snapshot) -> bool {
        if self.sys_fee < 0 || self.net_fee < 0 {
            return false;
        }
        if self.sys_fee.checked_add(self.net_fee).is_none() {
            return false;
        }
        if self.script.is_empty() || self.script.len() > u16::MAX as usize {
            return false;
        }
        if self.signers.is_empty() || self.signers.len() > 16 {
            return false;
        }
        let mut hashes = HashSet::new();
        for signer in &self.signers {
            if !hashes.insert(signer.account.clone()) {
                return false;
            }
        }
        if self.attributes.len() > 16 - self.signers.len() {
            return false;
        }
        let mut attribute_types = HashSet::new();
        for attribute in &self.attributes {
            if !attribute_types.insert(attribute.get_type()) {
                return false;
            }
        }
        if self.size() > Self::MAX_TRANSACTION_SIZE {
            return false;
        }
        let current_height = snapshot.height();
        if self.valid_until_block <= current_height || self.valid_until_block > current_height + Helper::MAX_VALID_UNTIL_BLOCK_INCREMENT as u32 {
            return false;
        }
        true
    }

    pub fn get_script_hash(&self) -> H160 {
        H160::from(&Crypto::hash160(&self.script))
    }
}

impl TryFrom<&JToken> for Transaction {
    type Error = io::Error;

    fn try_from(json: &JToken) -> Result<Self, Self::Error> {
        let version = json.get_u8("version")?;
        let nonce = json.get_u32("nonce")?;
        let sysfee = json.get_i64("sysfee")?;
        let netfee = json.get_i64("netfee")?;
        let valid_until_block = json.get_u32("validUntilBlock")?;
        
        let signers = json.get_array("signers")?
            .iter()
            .map(|s| Signer::try_from(s))
            .collect::<Result<Vec<_>, _>>()?;
        
        let attributes = json.get_array("attributes")?
            .iter()
            .map(|a| TransactionAttribute::try_from(a))
            .collect::<Result<Vec<_>, _>>()?;
        
        let script = json.get_base64("script")?;
        
        let witnesses = json.get_array("witnesses")?
            .iter()
            .map(|w| Witness::try_from(w))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Transaction {
            version,
            nonce,
            sys_fee: sysfee,
            net_fee: netfee,
            valid_until_block,
            signers,
            attributes,
            script,
            witnesses,
        })
    }
}

impl Native for Transaction {
    fn name() -> &'static str {
        "Transaction"
    }

    fn to_stack_item(&self) -> Array {
        let mut items = Vec::new();
        items.push(self.version.into());
        items.push(self.nonce.into());
        items.push(self.sys_fee.into());
        items.push(self.net_fee.into());
        items.push(self.valid_until_block.into());
        items.push(self.signers.iter().map(|s| s.to_stack_item()).collect::<Array>().into());
        items.push(self.attributes.iter().map(|a| a.to_stack_item()).collect::<Array>().into());
        items.push(self.script.clone().into());
        items.push(self.witnesses.iter().map(|w| w.to_stack_item()).collect::<Array>().into());
        Array::from(items)
    }
}


impl Transaction {
    pub fn verify(&self, settings: &ProtocolSettings, snapshot: &dyn DataCache, context: Option<&TransactionVerificationContext>, conflicts_list: &[Transaction]) -> VerifyResult {
        let result = self.verify_state_independent(settings);
        if result != VerifyResult::Succeed {
            return result;
        }
        self.verify_state_dependent(settings, snapshot, context, conflicts_list)
    }

    pub fn verify_state_dependent(&self, settings: &ProtocolSettings, snapshot: &dyn DataCache, context: Option<&TransactionVerificationContext>, conflicts_list: &[Transaction]) -> VerifyResult {
        let height = NativeContract::Ledger::current_index(snapshot);
        if self.valid_until_block <= height || self.valid_until_block > height + settings.max_valid_until_block_increment {
            return VerifyResult::Expired;
        }

        let hashes = self.get_script_hashes_for_verifying(snapshot);
        for hash in &hashes {
            if NativeContract::Policy::is_blocked(snapshot, hash) {
                return VerifyResult::PolicyFail;
            }
        }

        if let Some(ctx) = context {
            if !ctx.check_transaction(self, conflicts_list, snapshot) {
                return VerifyResult::InsufficientFunds;
            }
        }

        let mut attributes_fee = 0;
        for attribute in &self.attributes {
            if !attribute.verify(snapshot, self) {
                return VerifyResult::InvalidAttribute;
            }
            attributes_fee += attribute.calculate_network_fee(snapshot, self);
        }

        let mut net_fee_datoshi = self.net_fee - (self.size() as i64 * NativeContract::Policy::get_fee_per_byte(snapshot)) - attributes_fee;
        if net_fee_datoshi < 0 {
            return VerifyResult::InsufficientFunds;
        }

        if net_fee_datoshi > MAX_VERIFICATION_GAS {
            net_fee_datoshi = MAX_VERIFICATION_GAS;
        }

        let exec_fee_factor = NativeContract::Policy::get_exec_fee_factor(snapshot);
        for (i, hash) in hashes.iter().enumerate() {
            if is_signature_contract(&self.witnesses[i].verification_script) {
                net_fee_datoshi -= exec_fee_factor * signature_contract_cost();
            } else if let Some((m, n)) = is_multi_sig_contract(&self.witnesses[i].verification_script) {
                net_fee_datoshi -= exec_fee_factor * multi_signature_contract_cost(m, n);
            } else {
                match self.verify_witness(settings, snapshot, hash, &self.witnesses[i], net_fee_datoshi) {
                    Ok(fee) => net_fee_datoshi -= fee,
                    Err(_) => return VerifyResult::Invalid,
                }
            }

            if net_fee_datoshi < 0 {
                return VerifyResult::InsufficientFunds;
            }
        }

        VerifyResult::Succeed
    }

    pub fn verify_state_independent(&self, settings: &ProtocolSettings) -> VerifyResult {
        if self.size() > MAX_TRANSACTION_SIZE {
            return VerifyResult::OverSize;
        }

        // Validate script
        if let Err(_) = Script::new(&self.script) {
            return VerifyResult::InvalidScript;
        }

        let hashes = self.get_script_hashes_for_verifying(None);
        for (i, hash) in hashes.iter().enumerate() {
            let witness = &self.witnesses[i];
            if is_signature_contract(&witness.verification_script) {
                if hash != &witness.script_hash() {
                    return VerifyResult::Invalid;
                }
                let pubkey = &witness.verification_script[2..35];
                match verify_signature(&self.get_sign_data(settings.network), &witness.invocation_script[2..], pubkey) {
                    Ok(true) => {},
                    _ => return VerifyResult::InvalidSignature,
                }
            } else if let Some((m, points)) = is_multi_sig_contract(&witness.verification_script) {
                if hash != &witness.script_hash() {
                    return VerifyResult::Invalid;
                }
                let signatures = match get_multi_signatures(&witness.invocation_script) {
                    Some(sigs) => sigs,
                    None => return VerifyResult::Invalid,
                };
                if signatures.len() != m {
                    return VerifyResult::Invalid;
                }
                let n = points.len();
                let message = self.get_sign_data(settings.network);
                let mut x = 0;
                let mut y = 0;
                while x < m && y < n {
                    if verify_signature(&message, &signatures[x], &points[y]).unwrap_or(false) {
                        x += 1;
                    }
                    y += 1;
                    if m - x > n - y {
                        return VerifyResult::InvalidSignature;
                    }
                }
            }
        }

        VerifyResult::Succeed
    }

    fn get_script_hashes_for_verifying(&self, _snapshot: Option<&dyn DataCache>) -> Vec<H160> {
        self.signers.iter().map(|signer| signer.account.clone()).collect()
    }

    fn verify_witness(&self, settings: &ProtocolSettings, snapshot: &dyn DataCache, hash: &H160, witness: &Witness, gas: i64) -> Result<i64, ()> {
        // Implement witness verification logic
        // This is a placeholder implementation
        Ok(gas / 2)
    }

    fn get_sign_data(&self, network: u32) -> Vec<u8> {
        // Implement sign data generation
        // This is a placeholder implementation
        vec![]
    }
}

fn signature_contract_cost() -> i64 {
    // Return the cost of a signature contract
    200
}

fn multi_signature_contract_cost(m: usize, n: usize) -> i64 {
    // Calculate and return the cost of a multi-signature contract
    (m + n) as i64 * 200
}

fn verify_signature(message: &[u8], signature: &[u8], pubkey: &[u8]) -> Result<bool, ()> {
    // Implement signature verification
    // This is a placeholder implementation
    Ok(true)
}

fn get_multi_signatures(script: &[u8]) -> Option<Vec<Vec<u8>>> {
    // Implement multi-signature extraction
    // This is a placeholder implementation
    None
}

impl Transaction {
    fn get_script_hashes_for_verifying(&self, snapshot: Option<&dyn DataCache>) -> Vec<H160> {
        self.signers.iter().map(|signer| signer.account.clone()).collect()
    }

    fn verify_witness(&self, settings: &ProtocolSettings, snapshot: &dyn DataCache, hash: &H160, witness: &Witness, gas: i64) -> Result<i64, ()> {
        let engine = ScriptBuilder::new()
            .emit_push(self.get_sign_data(settings.network))
            .emit_push(&witness.invocation_script)
            .emit_push(&witness.verification_script)
            .to_script();

        let engine = ApplicationEngine::new(TriggerType::Verification, self, snapshot, gas);
        engine.load_script(&engine, CallFlags::None);

        if engine.execute().is_err() {
            return Err(());
        }

        if engine.state != VMState::Halt {
            return Err(());
        }

        if engine.result_stack.len() != 1 || !engine.result_stack[0].get_boolean() {
            return Err(());
        }

        Ok(engine.gas_consumed)
    }

    fn get_sign_data(&self, network: u32) -> Vec<u8> {
        let mut writer = Vec::new();
        writer.write_u32(network).unwrap();
        self.serialize_unsigned(&mut writer).unwrap();
        writer
    }
}

fn is_signature_contract(script: &[u8]) -> bool {
    script.len() == 35
        && script[0] == 0x21  // PUSHBYTES33
        && script[34] == 0xAC // CHECKSIG
}

fn is_multi_sig_contract(script: &[u8]) -> Option<(usize, Vec<Vec<u8>>)> {
    if script.len() < 37 {
        return None;
    }

    let m = match script[0] {
        op if op >= OpCode::Push1 as u8 && op <= OpCode::PUSH16 as u8 => (op - OpCode::Push1 as u8 + 1) as usize,
        _ => return None,
    };

    let mut offset = 1;
    let mut n = 0;
    let mut points = Vec::new();

    while script[offset] == 33 {
        points.push(script[offset + 1..offset + 34].to_vec());
        offset += 34;
        n += 1;
    }

    if n < m || n > 1024 {
        return None;
    }

    let n_opcode = script[offset];
    offset += 1;

    if n_opcode < OpCode::Push1 as u8 || n_opcode > OpCode::Push16 as u8 || n != (n_opcode - OpCode::Push1 as u8 + 1) as usize {
        return None;
    }

    if script[offset] != OpCode::Syscall as u8 {
        return None;
    }

    offset += 1;

    let syscall = u32::from_le_bytes([script[offset], script[offset + 1], script[offset + 2], script[offset + 3]]);
    if syscall != InteropService::SYSTEM_CRYPTO_CHECKMULTISIG {
        return None;
    }

    Some((m, points))
}
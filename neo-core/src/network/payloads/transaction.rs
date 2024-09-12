use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::io::{self, Read, Write};
use std::mem;
use crate::cryptography::{Crypto, Helper};
use crate::io::binary_writer::BinaryWriter;
use crate::io::memory_reader::MemoryReader;
use crate::network::payloads::{Signer, Witness};
use crate::uint160::UInt160;
use crate::uint256::UInt256;

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
    pub fn hash(&self) -> UInt256 {
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
    pub fn sender(&self) -> UInt160 {
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
            if !attribute_types.insert(attribute.type_id()) {
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

    pub fn get_script_hash(&self) -> UInt160 {
        UInt160::from_slice(&Crypto::hash160(&self.script))
    }

    pub fn size(&self) -> usize {
        let mut size = Self::HEADER_SIZE;
        size += self.signers.iter().map(|s| s.size()).sum::<usize>();
        size += self.attributes.iter().map(|a| a.size()).sum::<usize>();
        size += self.script.len();
        size += self.witnesses.iter().map(|w| w.size()).sum::<usize>();
        size
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

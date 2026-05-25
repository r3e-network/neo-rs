//
// serialization.rs - Transaction serialization and deserialization
//

use super::*;

impl Transaction {
    /// Serialize without witnesses.
    pub fn serialize_unsigned(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u8(self.version)?;
        writer.write_u32(self.nonce)?;
        writer.write_i64(self.system_fee)?;
        writer.write_i64(self.network_fee)?;
        writer.write_u32(self.valid_until_block)?;

        serialize_array(&self.signers, writer)?;
        serialize_array(&self.attributes, writer)?;

        if self.script.len() > u16::MAX as usize {
            return Err(IoError::invalid_data(
                "Transaction script exceeds maximum length",
            ));
        }
        writer.write_var_bytes(&self.script)?;

        Ok(())
    }

    /// Deserialize unsigned transaction data.
    pub fn deserialize_unsigned(reader: &mut MemoryReader) -> IoResult<Self> {
        let version = reader.read_u8()?;
        if version > 0 {
            return Err(IoError::invalid_data("Invalid transaction version"));
        }

        let nonce = reader.read_u32()?;

        let system_fee = reader.read_i64()?;
        if system_fee < 0 {
            return Err(IoError::invalid_data("Invalid system fee"));
        }

        let network_fee = reader.read_i64()?;
        if network_fee < 0 {
            return Err(IoError::invalid_data("Invalid network fee"));
        }

        if system_fee + network_fee < system_fee {
            return Err(IoError::invalid_data("Invalid combined fee"));
        }

        let valid_until_block = reader.read_u32()?;

        // Read signers
        let signers = Self::deserialize_signers(reader, MAX_TRANSACTION_ATTRIBUTES)?;

        // Read attributes
        let attributes =
            Self::deserialize_attributes(reader, MAX_TRANSACTION_ATTRIBUTES - signers.len())?;

        // Read script
        let script = reader.read_var_bytes(u16::MAX as usize)?;
        if script.is_empty() {
            return Err(IoError::invalid_data("Script length cannot be zero"));
        }

        Ok(Self {
            version,
            nonce,
            system_fee,
            network_fee,
            valid_until_block,
            signers,
            attributes,
            script,
            witnesses: Vec::new(),
            _hash: Mutex::new(None),
            _size: Mutex::new(None),
        })
    }

    pub(super) fn deserialize_signers(
        reader: &mut MemoryReader,
        max_count: usize,
    ) -> IoResult<Vec<Signer>> {
        let count = reader.read_var_int(max_count as u64)? as usize;
        if count == 0 {
            return Err(IoError::invalid_data("Signer count cannot be zero"));
        }
        if count > max_count {
            return Err(IoError::invalid_data("Too many signers"));
        }

        let mut signers = Vec::with_capacity(count);
        let mut hashset = HashSet::new();

        for _ in 0..count {
            let signer = <Signer as Serializable>::deserialize(reader)?;
            if !hashset.insert(signer.account) {
                return Err(IoError::invalid_data("Duplicate signer"));
            }
            signers.push(signer);
        }

        Ok(signers)
    }

    pub(super) fn deserialize_attributes(
        reader: &mut MemoryReader,
        max_count: usize,
    ) -> IoResult<Vec<TransactionAttribute>> {
        let count = reader.read_var_int(max_count as u64)? as usize;
        if count > max_count {
            return Err(IoError::invalid_data("Too many attributes"));
        }

        let mut attributes = Vec::with_capacity(count);
        let mut hashset = HashSet::new();

        for _ in 0..count {
            let attribute = <TransactionAttribute as Serializable>::deserialize(reader)?;
            if !attribute.allow_multiple() && !hashset.insert(attribute.get_type()) {
                return Err(IoError::invalid_data("Duplicate attribute"));
            }
            attributes.push(attribute);
        }

        Ok(attributes)
    }
}

impl Serializable for Transaction {
    fn size(&self) -> usize {
        let mut size_guard = self._size.lock();
        if let Some(size) = *size_guard {
            return size;
        }

        let size = HEADER_SIZE
            + get_var_size_serializable_slice(&self.signers)
            + get_var_size_serializable_slice(&self.attributes)
            + get_var_size_bytes(&self.script)
            + get_var_size_serializable_slice(&self.witnesses);

        *size_guard = Some(size);
        size
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        self.serialize_unsigned(writer)?;
        serialize_array(&self.witnesses, writer)
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let mut tx = Self::deserialize_unsigned(reader)?;

        tx.witnesses = deserialize_exact_array(reader, tx.signers.len(), "Witness count mismatch")?;

        tx.invalidate_size();
        Ok(tx)
    }
}

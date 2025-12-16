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

        // Write signers
        writer.write_var_uint(self.signers.len() as u64)?;
        for signer in &self.signers {
            writer.write_serializable(signer)?;
        }

        // Write attributes
        writer.write_var_uint(self.attributes.len() as u64)?;
        for attr in &self.attributes {
            writer.write_serializable(attr)?;
        }

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
            + get_var_size(self.signers.len() as u64)
            + self.signers.iter().map(|s| s.size()).sum::<usize>()
            + get_var_size(self.attributes.len() as u64)
            + self.attributes.iter().map(|a| a.size()).sum::<usize>()
            + get_var_size(self.script.len() as u64)
            + self.script.len()
            + get_var_size(self.witnesses.len() as u64)
            + self.witnesses.iter().map(|w| w.size()).sum::<usize>();

        *size_guard = Some(size);
        size
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        self.serialize_unsigned(writer)?;
        if self.witnesses.len() != self.signers.len() {
            return Err(IoError::invalid_data(
                "Witness count must match signer count",
            ));
        }
        writer.write_var_uint(self.witnesses.len() as u64)?;
        for witness in &self.witnesses {
            writer.write_serializable(witness)?;
        }
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let mut tx = Self::deserialize_unsigned(reader)?;

        let witness_count = reader.read_var_int(tx.signers.len() as u64)? as usize;
        if witness_count != tx.signers.len() {
            return Err(IoError::invalid_data("Witness count mismatch"));
        }

        let mut witnesses = Vec::with_capacity(witness_count);
        for _ in 0..witness_count {
            witnesses.push(<Witness as Serializable>::deserialize(reader)?);
        }
        tx.witnesses = witnesses;

        tx.invalidate_size();
        Ok(tx)
    }
}

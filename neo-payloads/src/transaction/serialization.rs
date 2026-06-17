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

        SerializeHelper::serialize_array(&self.signers, writer)?;
        SerializeHelper::serialize_array(&self.attributes, writer)?;

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

        if system_fee.checked_add(network_fee).is_none() {
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
        let mut hashset = HashSet::new();
        let signers = SerializeHelper::deserialize_array_with(reader, max_count, |reader| {
            let signer = <Signer as Serializable>::deserialize(reader)?;
            if !hashset.insert(signer.account) {
                return Err(IoError::invalid_data("Duplicate signer"));
            }
            Ok(signer)
        })?;

        if signers.is_empty() {
            return Err(IoError::invalid_data("Signer count cannot be zero"));
        }

        Ok(signers)
    }

    pub(super) fn deserialize_attributes(
        reader: &mut MemoryReader,
        max_count: usize,
    ) -> IoResult<Vec<TransactionAttribute>> {
        let mut hashset = HashSet::new();
        SerializeHelper::deserialize_array_with(reader, max_count, |reader| {
            let attribute = <TransactionAttribute as Serializable>::deserialize(reader)?;
            if !attribute.allow_multiple() && !hashset.insert(attribute.type_id()) {
                return Err(IoError::invalid_data("Duplicate attribute"));
            }
            Ok(attribute)
        })
    }
}

impl Serializable for Transaction {
    fn size(&self) -> usize {
        let mut size_guard = self._size.lock();
        if let Some(size) = *size_guard {
            return size;
        }

        let size = HEADER_SIZE
            + SerializeHelper::get_var_size_serializable_slice(&self.signers)
            + SerializeHelper::get_var_size_serializable_slice(&self.attributes)
            + SerializeHelper::get_var_size_bytes(&self.script)
            + SerializeHelper::get_var_size_serializable_slice(&self.witnesses);

        *size_guard = Some(size);
        size
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        self.serialize_unsigned(writer)?;
        SerializeHelper::serialize_array(&self.witnesses, writer)
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let mut tx = Self::deserialize_unsigned(reader)?;

        tx.witnesses = SerializeHelper::deserialize_exact_array(
            reader,
            tx.signers.len(),
            "Witness count mismatch",
        )?;

        tx.invalidate_size();
        Ok(tx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_primitives::WitnessScope;

    fn write_unsigned_header(writer: &mut BinaryWriter) {
        writer.write_u8(0).unwrap(); // Version
        writer.write_u32(0x0102_0304).unwrap(); // Nonce
        writer.write_i64(0).unwrap(); // SystemFee
        writer.write_i64(0).unwrap(); // NetworkFee
        writer.write_u32(42).unwrap(); // ValidUntilBlock
    }

    fn signer(seed: u8) -> Signer {
        Signer::new(
            UInt160::from_bytes(&[seed; 20]).expect("valid UInt160"),
            WitnessScope::NONE,
        )
    }

    #[test]
    fn deserialize_unsigned_rejects_combined_fee_overflow_without_panic() {
        let mut writer = BinaryWriter::new();
        writer.write_u8(0).unwrap(); // Version
        writer.write_u32(0x0102_0304).unwrap(); // Nonce
        writer.write_i64(i64::MAX).unwrap(); // SystemFee
        writer.write_i64(1).unwrap(); // NetworkFee

        let bytes = writer.into_bytes();
        let mut reader = MemoryReader::new(&bytes);

        let err = Transaction::deserialize_unsigned(&mut reader)
            .expect_err("C# rejects SystemFee + NetworkFee overflow");
        assert!(
            err.to_string().contains("Invalid combined fee"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn deserialize_unsigned_rejects_empty_signers_like_csharp() {
        let mut writer = BinaryWriter::new();
        write_unsigned_header(&mut writer);
        writer.write_var_int(0).unwrap();

        let bytes = writer.into_bytes();
        let mut reader = MemoryReader::new(&bytes);
        let err = Transaction::deserialize_unsigned(&mut reader)
            .expect_err("C# rejects transactions without signers");

        assert!(
            err.to_string().contains("Signer count cannot be zero"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn deserialize_unsigned_rejects_duplicate_signers_like_csharp() {
        let duplicate = signer(0x11);
        let mut writer = BinaryWriter::new();
        write_unsigned_header(&mut writer);
        SerializeHelper::serialize_array(&[duplicate.clone(), duplicate], &mut writer).unwrap();

        let bytes = writer.into_bytes();
        let mut reader = MemoryReader::new(&bytes);
        let err = Transaction::deserialize_unsigned(&mut reader)
            .expect_err("C# rejects duplicate transaction signers");

        assert!(
            err.to_string().contains("Duplicate signer"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn deserialize_unsigned_rejects_duplicate_nonrepeatable_attributes_like_csharp() {
        let mut writer = BinaryWriter::new();
        write_unsigned_header(&mut writer);
        SerializeHelper::serialize_array(&[signer(0x12)], &mut writer).unwrap();
        SerializeHelper::serialize_array(
            &[
                TransactionAttribute::HighPriority,
                TransactionAttribute::HighPriority,
            ],
            &mut writer,
        )
        .unwrap();

        let bytes = writer.into_bytes();
        let mut reader = MemoryReader::new(&bytes);
        let err = Transaction::deserialize_unsigned(&mut reader)
            .expect_err("C# rejects duplicate non-repeatable attributes");

        assert!(
            err.to_string().contains("Duplicate attribute"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn deserialize_unsigned_rejects_empty_script_like_csharp() {
        let mut writer = BinaryWriter::new();
        write_unsigned_header(&mut writer);
        SerializeHelper::serialize_array(&[signer(0x13)], &mut writer).unwrap();
        SerializeHelper::serialize_array::<TransactionAttribute>(&[], &mut writer).unwrap();
        writer.write_var_bytes(&[]).unwrap();

        let bytes = writer.into_bytes();
        let mut reader = MemoryReader::new(&bytes);
        let err = Transaction::deserialize_unsigned(&mut reader)
            .expect_err("C# rejects empty transaction scripts");

        assert!(
            err.to_string().contains("Script length cannot be zero"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn deserialize_rejects_witness_count_mismatch_like_csharp() {
        let mut writer = BinaryWriter::new();
        write_unsigned_header(&mut writer);
        SerializeHelper::serialize_array(&[signer(0x14)], &mut writer).unwrap();
        SerializeHelper::serialize_array::<TransactionAttribute>(&[], &mut writer).unwrap();
        writer.write_var_bytes(&[0x40]).unwrap();
        writer.write_var_int(0).unwrap();

        let bytes = writer.into_bytes();
        let mut reader = MemoryReader::new(&bytes);
        let err = <Transaction as Serializable>::deserialize(&mut reader)
            .expect_err("C# requires witnesses == signers");

        assert!(
            err.to_string().contains("Witness count mismatch"),
            "unexpected error: {err}"
        );
    }
}

//
// core.rs - Transaction constructor, getters/setters, and basic methods
//

use super::*;

impl Transaction {
    /// Creates a new transaction.
    pub fn new() -> Self {
        let mut nonce_bytes = [0u8; 4];
        OsRng.fill_bytes(&mut nonce_bytes);
        Self {
            version: 0,
            nonce: u32::from_le_bytes(nonce_bytes),
            system_fee: 0,
            network_fee: 0,
            valid_until_block: 0,
            signers: Vec::new(),
            attributes: Vec::new(),
            script: Vec::new(),
            witnesses: Vec::new(),
            _hash: Mutex::new(None),
            _size: Mutex::new(None),
        }
    }

    /// Clears the cached hash. Called when transaction content changes.
    #[inline]
    pub(super) fn invalidate_hash(&self) {
        *self._hash.lock() = None;
    }

    /// Clears the cached size. Called when transaction content changes.
    #[inline]
    pub(super) fn invalidate_size(&self) {
        *self._size.lock() = None;
    }

    /// Clears both cached hash and size.
    #[inline]
    pub(super) fn invalidate_cache(&self) {
        self.invalidate_hash();
        self.invalidate_size();
    }

    /// Gets the version of the transaction.
    pub fn version(&self) -> u8 {
        self.version
    }

    /// Sets the version of the transaction.
    pub fn set_version(&mut self, version: u8) {
        self.version = version;
        self.invalidate_hash();
    }

    /// Gets the nonce of the transaction.
    pub fn nonce(&self) -> u32 {
        self.nonce
    }

    /// Sets the nonce of the transaction.
    pub fn set_nonce(&mut self, nonce: u32) {
        self.nonce = nonce;
        self.invalidate_hash();
    }

    /// Gets the system fee of the transaction.
    pub fn system_fee(&self) -> i64 {
        self.system_fee
    }

    /// Sets the system fee of the transaction.
    pub fn set_system_fee(&mut self, system_fee: i64) {
        self.system_fee = system_fee;
        self.invalidate_hash();
    }

    /// Gets the network fee of the transaction.
    pub fn network_fee(&self) -> i64 {
        self.network_fee
    }

    /// Sets the network fee of the transaction.
    pub fn set_network_fee(&mut self, network_fee: i64) {
        self.network_fee = network_fee;
        self.invalidate_hash();
    }

    /// Gets the valid until block of the transaction.
    pub fn valid_until_block(&self) -> u32 {
        self.valid_until_block
    }

    /// Sets the valid until block of the transaction.
    pub fn set_valid_until_block(&mut self, valid_until_block: u32) {
        self.valid_until_block = valid_until_block;
        self.invalidate_hash();
    }

    /// Gets the signers of the transaction.
    pub fn signers(&self) -> &[Signer] {
        &self.signers
    }

    /// Sets the signers of the transaction.
    pub fn set_signers(&mut self, signers: Vec<Signer>) {
        self.signers = signers;
        self.invalidate_cache();
    }

    /// Adds a signer to the transaction.
    pub fn add_signer(&mut self, signer: Signer) {
        self.signers.push(signer);
        self.invalidate_cache();
    }

    /// Gets the attributes of the transaction.
    pub fn attributes(&self) -> &[TransactionAttribute] {
        &self.attributes
    }

    /// Sets the attributes of the transaction.
    pub fn set_attributes(&mut self, attributes: Vec<TransactionAttribute>) {
        self.attributes = attributes;
        self.invalidate_cache();
    }

    /// Adds a single attribute to the transaction.
    pub fn add_attribute(&mut self, attribute: TransactionAttribute) {
        self.attributes.push(attribute);
        self.invalidate_cache();
    }

    /// Gets the script of the transaction.
    pub fn script(&self) -> &[u8] {
        &self.script
    }

    /// Sets the script of the transaction.
    pub fn set_script(&mut self, script: Vec<u8>) {
        self.script = script;
        self.invalidate_cache();
    }

    /// Gets the witnesses of the transaction.
    pub fn witnesses(&self) -> &[Witness] {
        &self.witnesses
    }

    /// Sets the witnesses of the transaction.
    pub fn set_witnesses(&mut self, witnesses: Vec<Witness>) {
        self.witnesses = witnesses;
        self.invalidate_size();
    }

    /// Adds a witness to the transaction.
    pub fn add_witness(&mut self, witness: Witness) {
        self.witnesses.push(witness);
        self.invalidate_size();
    }

    /// Returns the transaction hash (C# compatibility helper).
    pub fn get_hash(&self) -> UInt256 {
        self.hash()
    }

    /// Returns the unsigned serialization used for hashing.
    pub fn hash_data(&self) -> Vec<u8> {
        match self.try_get_hash_data() {
            Ok(data) => data,
            Err(e) => {
                tracing::error!("Failed to serialize transaction unsigned data: {:?}", e);
                Vec::new()
            }
        }
    }

    /// Returns the unsigned serialization used for hashing, or an error if the
    /// transaction cannot be represented on the wire.
    pub fn try_get_hash_data(&self) -> CoreResult<Vec<u8>> {
        let mut writer = BinaryWriter::new();
        Self::serialize_unsigned(self, &mut writer)?;
        Ok(writer.into_bytes())
    }

    /// Serializes the transaction into bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        match self.try_to_bytes() {
            Ok(data) => data,
            Err(e) => {
                tracing::error!("Transaction serialization failed: {:?}", e);
                Vec::new()
            }
        }
    }

    /// Serializes the transaction into bytes, returning an error on invalid
    /// wire representation instead of silently returning an empty buffer.
    pub fn try_to_bytes(&self) -> CoreResult<Vec<u8>> {
        let mut writer = BinaryWriter::new();
        <Self as Serializable>::serialize(self, &mut writer)?;
        Ok(writer.into_bytes())
    }

    /// Deserializes a transaction from bytes.
    pub fn from_bytes(bytes: &[u8]) -> IoResult<Self> {
        let mut reader = MemoryReader::new(bytes);
        <Self as Serializable>::deserialize(&mut reader)
    }

    /// Gets the hash of the transaction.
    pub fn hash(&self) -> UInt256 {
        match self.try_hash() {
            Ok(hash) => hash,
            Err(e) => {
                tracing::error!("Transaction serialization failed: {:?}", e);
                UInt256::zero()
            }
        }
    }

    /// Gets the hash of the transaction, failing closed if unsigned
    /// serialization fails.
    pub fn try_hash(&self) -> CoreResult<UInt256> {
        let mut hash_guard = self._hash.lock();
        if let Some(hash) = *hash_guard {
            return Ok(hash);
        }

        let mut writer = BinaryWriter::new();
        self.serialize_unsigned(&mut writer)?;
        let hash = UInt256::from(Crypto::sha256(&writer.into_bytes()));
        *hash_guard = Some(hash);
        Ok(hash)
    }

    /// Gets the sender (first signer) of the transaction.
    /// The sender will pay the fees of the transaction.
    pub fn sender(&self) -> Option<UInt160> {
        self.signers.first().map(|s| s.account)
    }

    /// Gets the fee per byte.
    pub fn fee_per_byte(&self) -> i64 {
        let size = self.size();
        if size == 0 {
            0
        } else {
            self.network_fee / size as i64
        }
    }

    /// Gets the first attribute of the specified type.
    pub fn get_attribute(
        &self,
        attr_type: TransactionAttributeType,
    ) -> Option<&TransactionAttribute> {
        self.attributes
            .iter()
            .find(|attr| TransactionAttribute::type_id(attr) == attr_type)
    }

    /// Gets all attributes of the specified type.
    pub fn get_attributes(
        &self,
        attr_type: TransactionAttributeType,
    ) -> Vec<&TransactionAttribute> {
        self.attributes
            .iter()
            .filter(|attr| TransactionAttribute::type_id(attr) == attr_type)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WitnessScope;

    fn transaction_with_script(script: Vec<u8>) -> Transaction {
        let mut tx = Transaction::new();
        tx.set_version(0);
        tx.set_nonce(0x0102_0304);
        tx.set_system_fee(1);
        tx.set_network_fee(1);
        tx.set_valid_until_block(42);
        tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
        tx.set_attributes(Vec::new());
        tx.set_script(script);
        tx.set_witnesses(vec![Witness::empty()]);
        tx
    }

    #[test]
    fn try_get_hash_data_rejects_oversized_script() {
        let tx = transaction_with_script(vec![OpCode::NOP.byte(); u16::MAX as usize + 1]);

        assert!(tx.try_get_hash_data().is_err());
    }

    #[test]
    fn try_to_bytes_rejects_oversized_script() {
        let tx = transaction_with_script(vec![OpCode::NOP.byte(); u16::MAX as usize + 1]);

        assert!(tx.try_to_bytes().is_err());
    }

    #[test]
    fn try_hash_rejects_oversized_script_without_caching_zero_hash() {
        let tx = transaction_with_script(vec![OpCode::NOP.byte(); u16::MAX as usize + 1]);

        assert!(tx.try_hash().is_err());
        assert!(!matches!(*tx._hash.lock(), Some(hash) if hash == UInt256::zero()));
    }

    #[test]
    fn verifiable_hash_rejects_oversized_script() {
        let tx = transaction_with_script(vec![OpCode::NOP.byte(); u16::MAX as usize + 1]);

        assert!(<Transaction as crate::Verifiable>::hash(&tx).is_err());
    }
}

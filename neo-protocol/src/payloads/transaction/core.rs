// Transaction construction, caches, accessors, and hashing.
use super::{Signer, Transaction, TransactionAttribute, Witness};
use neo_crypto::Crypto;
use neo_io::{BinaryWriter, IoResult, MemoryReader, Serializable};
use neo_primitives::{TransactionAttributeType, UInt160, UInt256};
use parking_lot::Mutex;
use rand::RngCore;
use rand::rngs::OsRng;

impl Transaction {
    /// Creates a transaction with a random little-endian nonce.
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
    #[inline]
    fn invalidate_hash(&self) {
        *self._hash.lock() = None;
    }
    #[inline]
    pub(super) fn invalidate_size(&self) {
        *self._size.lock() = None;
    }
    #[inline]
    fn invalidate_cache(&self) {
        self.invalidate_hash();
        self.invalidate_size();
    }

    /// Gets the version.
    pub fn version(&self) -> u8 {
        self.version
    }
    /// Sets the version and invalidates the hash.
    pub fn set_version(&mut self, version: u8) {
        self.version = version;
        self.invalidate_hash();
    }
    /// Gets the nonce.
    pub fn nonce(&self) -> u32 {
        self.nonce
    }
    /// Sets the nonce and invalidates the hash.
    pub fn set_nonce(&mut self, nonce: u32) {
        self.nonce = nonce;
        self.invalidate_hash();
    }
    /// Gets the system fee.
    pub fn system_fee(&self) -> i64 {
        self.system_fee
    }
    /// Sets the system fee and invalidates the hash.
    pub fn set_system_fee(&mut self, system_fee: i64) {
        self.system_fee = system_fee;
        self.invalidate_hash();
    }
    /// Gets the network fee.
    pub fn network_fee(&self) -> i64 {
        self.network_fee
    }
    /// Sets the network fee and invalidates the hash.
    pub fn set_network_fee(&mut self, network_fee: i64) {
        self.network_fee = network_fee;
        self.invalidate_hash();
    }
    /// Gets the expiration height.
    pub fn valid_until_block(&self) -> u32 {
        self.valid_until_block
    }
    /// Sets the expiration height and invalidates the hash.
    pub fn set_valid_until_block(&mut self, valid_until_block: u32) {
        self.valid_until_block = valid_until_block;
        self.invalidate_hash();
    }
    /// Gets signers in verification order.
    pub fn signers(&self) -> &[Signer] {
        &self.signers
    }
    /// Replaces signers and invalidates both caches.
    pub fn set_signers(&mut self, signers: Vec<Signer>) {
        self.signers = signers;
        self.invalidate_cache();
    }
    /// Adds a signer and invalidates both caches.
    pub fn add_signer(&mut self, signer: Signer) {
        self.signers.push(signer);
        self.invalidate_cache();
    }
    /// Gets attributes.
    pub fn attributes(&self) -> &[TransactionAttribute] {
        &self.attributes
    }
    /// Replaces attributes and invalidates both caches.
    pub fn set_attributes(&mut self, attributes: Vec<TransactionAttribute>) {
        self.attributes = attributes;
        self.invalidate_cache();
    }
    /// Adds an attribute and invalidates both caches.
    pub fn add_attribute(&mut self, attribute: TransactionAttribute) {
        self.attributes.push(attribute);
        self.invalidate_cache();
    }
    /// Gets the script.
    pub fn script(&self) -> &[u8] {
        &self.script
    }
    /// Replaces the script and invalidates both caches.
    pub fn set_script(&mut self, script: Vec<u8>) {
        self.script = script;
        self.invalidate_cache();
    }
    /// Gets witnesses.
    pub fn witnesses(&self) -> &[Witness] {
        &self.witnesses
    }
    /// Replaces witnesses and invalidates only the size.
    pub fn set_witnesses(&mut self, witnesses: Vec<Witness>) {
        self.witnesses = witnesses;
        self.invalidate_size();
    }
    /// Adds a witness and invalidates only the size.
    pub fn add_witness(&mut self, witness: Witness) {
        self.witnesses.push(witness);
        self.invalidate_size();
    }
    /// Returns the transaction hash.
    pub fn get_hash(&self) -> UInt256 {
        self.hash()
    }
    /// Returns unsigned bytes, or the historical empty fallback on failure.
    pub fn hash_data(&self) -> Vec<u8> {
        match self.try_get_hash_data() {
            Ok(data) => data,
            Err(e) => {
                tracing::error!("Failed to serialize transaction unsigned data: {:?}", e);
                Vec::new()
            }
        }
    }
    /// Returns unsigned bytes, preserving low-level wire errors.
    pub fn try_get_hash_data(&self) -> IoResult<Vec<u8>> {
        let mut writer = BinaryWriter::new();
        Self::serialize_unsigned(self, &mut writer)?;
        Ok(writer.into_bytes())
    }
    /// Returns serialized bytes, or the historical empty fallback on failure.
    pub fn to_bytes(&self) -> Vec<u8> {
        match self.try_to_bytes() {
            Ok(data) => data,
            Err(e) => {
                tracing::error!("Transaction serialization failed: {:?}", e);
                Vec::new()
            }
        }
    }
    /// Returns serialized bytes, preserving low-level wire errors.
    pub fn try_to_bytes(&self) -> IoResult<Vec<u8>> {
        let mut writer = BinaryWriter::new();
        <Self as Serializable>::serialize(self, &mut writer)?;
        Ok(writer.into_bytes())
    }
    /// Deserializes from bytes, retaining the legacy trailing-byte behavior.
    pub fn from_bytes(bytes: &[u8]) -> IoResult<Self> {
        <Self as Serializable>::deserialize(&mut MemoryReader::new(bytes))
    }
    /// Gets the hash, failing closed on invalid unsigned encoding.
    pub fn hash(&self) -> UInt256 {
        self.try_hash()
            .unwrap_or_else(|e| panic!("Transaction unsigned serialization failed: {e:?}"))
    }
    /// Gets the single-SHA256 hash; failures do not populate the cache.
    pub fn try_hash(&self) -> IoResult<UInt256> {
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
    /// Gets signer accounts in verification order.
    pub fn signer_accounts(&self) -> Vec<UInt160> {
        self.signers.iter().map(|s| s.account).collect()
    }
    /// Gets the first signer, who pays the fees.
    pub fn sender(&self) -> Option<UInt160> {
        self.signers.first().map(|s| s.account)
    }
    /// Gets the integer fee per serialized byte.
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
    use neo_primitives::WitnessScope;
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
        let tx = transaction_with_script(vec![0x21; u16::MAX as usize + 1]);
        assert!(tx.try_get_hash_data().is_err());
    }
    #[test]
    fn try_to_bytes_rejects_oversized_script() {
        let tx = transaction_with_script(vec![0x21; u16::MAX as usize + 1]);
        assert!(tx.try_to_bytes().is_err());
    }
    #[test]
    fn try_hash_rejects_oversized_script_without_caching_zero_hash() {
        let tx = transaction_with_script(vec![0x21; u16::MAX as usize + 1]);
        assert!(tx.try_hash().is_err());
        assert!(!matches!(*tx._hash.lock(), Some(hash) if hash == UInt256::zero()));
        assert!(tx._hash.lock().is_none());
    }
    #[test]
    fn verifiable_hash_rejects_oversized_script() {
        let tx = transaction_with_script(vec![0x21; u16::MAX as usize + 1]);
        assert!(<Transaction as neo_primitives::Verifiable>::hash(&tx).is_err());
    }
    #[test]
    fn primitive_hash_error_keeps_legacy_codec_prefix() {
        let tx = transaction_with_script(vec![0x21; u16::MAX as usize + 1]);
        let low = tx.try_hash().unwrap_err();
        let actual = <Transaction as neo_primitives::Verifiable>::hash(&tx).unwrap_err();
        let expected = neo_primitives::PrimitiveError::invalid_data(format!("Codec error: {low}"));
        assert_eq!(actual.to_string(), expected.to_string());
    }

    fn warm_transaction() -> Transaction {
        let tx = transaction_with_script(vec![0x21, 0x00]);
        let _ = tx.try_hash().unwrap();
        let _ = neo_io::Serializable::size(&tx);
        tx
    }

    fn warm_transaction_clone() -> Transaction {
        let tx = warm_transaction().clone();
        let _ = neo_io::Serializable::size(&tx);
        tx
    }

    fn assert_variable_width_mutation_invalidates_both_caches(tx: &Transaction) {
        assert!(tx._hash.lock().is_none());
        assert!(tx._size.lock().is_none());
    }

    fn assert_fixed_width_mutation_invalidates_only_hash(tx: &Transaction) {
        assert!(tx._hash.lock().is_none());
        assert!(tx._size.lock().is_some());
    }

    #[test]
    fn clone_preserves_warm_hash_and_recomputes_size() {
        let tx = warm_transaction();
        let cloned = tx.clone();
        assert_eq!(*cloned._hash.lock(), *tx._hash.lock());
        assert!(cloned._size.lock().is_none());
        assert_eq!(cloned.try_hash().unwrap(), tx.try_hash().unwrap());
        assert_eq!(
            neo_io::Serializable::size(&cloned),
            neo_io::Serializable::size(&tx)
        );
        assert!(cloned._size.lock().is_some());
    }

    #[test]
    fn every_unsigned_mutator_invalidates_cloned_caches() {
        let mut tx = warm_transaction_clone();
        tx.set_version(1);
        assert_fixed_width_mutation_invalidates_only_hash(&tx);

        let mut tx = warm_transaction_clone();
        tx.set_nonce(0x0506_0708);
        assert_fixed_width_mutation_invalidates_only_hash(&tx);

        let mut tx = warm_transaction_clone();
        tx.set_system_fee(2);
        assert_fixed_width_mutation_invalidates_only_hash(&tx);

        let mut tx = warm_transaction_clone();
        tx.set_network_fee(2);
        assert_fixed_width_mutation_invalidates_only_hash(&tx);

        let mut tx = warm_transaction_clone();
        tx.set_valid_until_block(43);
        assert_fixed_width_mutation_invalidates_only_hash(&tx);

        let mut tx = warm_transaction_clone();
        tx.set_signers(Vec::new());
        assert_variable_width_mutation_invalidates_both_caches(&tx);

        let mut tx = warm_transaction_clone();
        tx.set_attributes(Vec::new());
        assert_variable_width_mutation_invalidates_both_caches(&tx);

        let mut tx = warm_transaction_clone();
        tx.set_script(vec![0x21, 0x01]);
        assert_variable_width_mutation_invalidates_both_caches(&tx);
    }

    #[test]
    fn witness_mutation_invalidates_only_cloned_size_cache() {
        let mut tx = warm_transaction_clone();
        let hash = tx.try_hash().unwrap();
        tx.set_witnesses(vec![Witness::empty(), Witness::empty()]);
        assert_eq!(*tx._hash.lock(), Some(hash));
        assert!(tx._size.lock().is_none());
        assert_eq!(tx.try_hash().unwrap(), hash);
    }
}

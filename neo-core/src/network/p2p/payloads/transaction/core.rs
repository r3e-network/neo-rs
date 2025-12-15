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
    pub fn get_hash_data(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        if let Err(e) = Self::serialize_unsigned(self, &mut writer) {
            tracing::error!("Failed to serialize transaction unsigned data: {:?}", e);
            return Vec::new();
        }
        writer.into_bytes()
    }

    /// Serializes the transaction into bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        if let Err(e) = <Self as Serializable>::serialize(self, &mut writer) {
            tracing::error!("Transaction serialization failed: {:?}", e);
            return Vec::new();
        }
        writer.into_bytes()
    }

    /// Deserializes a transaction from bytes.
    pub fn from_bytes(bytes: &[u8]) -> IoResult<Self> {
        let mut reader = MemoryReader::new(bytes);
        <Self as Serializable>::deserialize(&mut reader)
    }

    /// Gets the hash of the transaction.
    pub fn hash(&self) -> UInt256 {
        let mut hash_guard = self._hash.lock();
        if let Some(hash) = *hash_guard {
            return hash;
        }

        // Calculate hash from serialized unsigned data
        let mut writer = BinaryWriter::new();
        // Serialization of a valid transaction should never fail
        if let Err(e) = self.serialize_unsigned(&mut writer) {
            tracing::error!("Transaction serialization failed: {:?}", e);
            return UInt256::zero();
        }
        let hash = UInt256::from(sha256(&writer.into_bytes()));
        *hash_guard = Some(hash);
        hash
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
            .find(|attr| attr.get_type() == attr_type)
    }

    /// Gets all attributes of the specified type.
    pub fn get_attributes(
        &self,
        attr_type: TransactionAttributeType,
    ) -> Vec<&TransactionAttribute> {
        self.attributes
            .iter()
            .filter(|attr| attr.get_type() == attr_type)
            .collect()
    }
}

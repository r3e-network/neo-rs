use tokio::io::AsyncWriteExt;
use neo_base::hash::Sha256;
use crate::io::binary_writer::BinaryWriter;
use crate::network::payloads::IVerifiable;
use neo_type::H256;

/// Calculates the hash of an `IVerifiable`.
///
/// # Arguments
///
/// * `verifiable` - The `IVerifiable` object to hash.
///
/// # Returns
///
/// The hash of the object.
pub fn calculate_hash(verifiable: &dyn IVerifiable) -> H256 {
    let mut buffer = Vec::new();
    let mut writer = BinaryWriter::new(&mut buffer);
    verifiable.serialize_unsigned(&mut writer);
    let _ = writer.flush();
    H256::new(&buffer.to_vec().sha256())
}

/// Gets the data of an `IVerifiable` object to be hashed.
///
/// # Arguments
///
/// * `verifiable` - The `IVerifiable` object to hash.
/// * `network` - The magic number of the network.
///
/// # Returns
///
/// The data to hash.
pub fn get_sign_data(verifiable: &dyn IVerifiable, network: u32) -> Vec<u8> {
    let mut ms = MemoryStream::new();
    let mut writer = BinaryWriter::new(&mut ms);
    writer.write_u32(network).unwrap();
    writer.write_fixed_bytes(&verifiable.hash()).unwrap();
    writer.flush().unwrap();
    ms.to_vec()
}
use crate::context::ValidatorInfo;
use neo_primitives::{UInt160, UInt256};

pub(in crate::service) fn compute_merkle_root(hashes: &[UInt256]) -> UInt256 {
    use neo_crypto::Crypto;

    match hashes.len() {
        0 => UInt256::zero(),
        1 => hashes[0],
        _ => {
            let mut level: Vec<UInt256> = hashes.to_vec();
            while level.len() > 1 {
                if level.len() % 2 == 1 {
                    level.push(*level.last().unwrap());
                }
                let mut next = Vec::with_capacity(level.len() / 2);
                for pair in level.chunks(2) {
                    let mut buf = Vec::with_capacity(64);
                    buf.extend_from_slice(&pair[0].to_bytes());
                    buf.extend_from_slice(&pair[1].to_bytes());
                    let h = Crypto::hash256(&buf);
                    next.push(UInt256::from(h));
                }
                level = next;
            }
            level[0]
        }
    }
}

pub(in crate::service) fn compute_next_consensus_address(validators: &[ValidatorInfo]) -> UInt160 {
    use neo_crypto::ECPoint;
    use neo_vm::script_builder::ScriptBuilder;

    if validators.is_empty() {
        return UInt160::zero();
    }

    let n = validators.len();
    let f = (n - 1) / 3;
    let m = n - f;

    let mut keys: Vec<ECPoint> = validators.iter().map(|v| v.public_key.clone()).collect();
    keys.sort();

    let mut builder = ScriptBuilder::new();
    builder.emit_push_int(m as i64);
    for key in &keys {
        builder.emit_push(key.as_bytes());
    }
    builder.emit_push_int(n as i64);
    builder
        .emit_syscall("System.Crypto.CheckMultisig")
        .expect("infallible: in-memory emit");
    UInt160::from_script(&builder.to_array())
}

#[allow(clippy::too_many_arguments)]
pub(in crate::service) fn compute_header_hash(
    version: u32,
    prev_hash: UInt256,
    merkle_root: UInt256,
    timestamp: u64,
    nonce: u64,
    index: u32,
    primary_index: u8,
    next_consensus: UInt160,
) -> UInt256 {
    use neo_io::BinaryWriter;

    let mut writer = BinaryWriter::new();
    writer
        .write_u32(version)
        .expect("infallible: in-memory write");
    writer
        .write_serializable(&prev_hash)
        .expect("infallible: in-memory write");
    writer
        .write_serializable(&merkle_root)
        .expect("infallible: in-memory write");
    writer
        .write_u64(timestamp)
        .expect("infallible: in-memory write");
    writer
        .write_u64(nonce)
        .expect("infallible: in-memory write");
    writer
        .write_u32(index)
        .expect("infallible: in-memory write");
    writer
        .write_u8(primary_index)
        .expect("infallible: in-memory write");
    writer
        .write_serializable(&next_consensus)
        .expect("infallible: in-memory write");

    // Matches C# `IVerifiable.CalculateHash()` (single SHA-256 over unsigned bytes).
    UInt256::from(neo_crypto::Crypto::sha256(&writer.into_bytes()))
}

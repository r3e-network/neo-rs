// Copyright (C) 2015-2025 The Neo Project.
// Licensed under the MIT License. See LICENSE file for details.
use super::validate_length;
use neo_io::serializable::helper::get_var_size_bytes;
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_primitives::OracleResponseCode;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Maximum byte length of the result field.
pub const MAX_RESULT_SIZE: usize = u16::MAX as usize;

/// An oracle response transaction attribute, without native verification policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OracleResponse {
    /// The oracle request ID.
    pub id: u64,
    /// The primitive response code.
    pub code: OracleResponseCode,
    /// Response result bytes.
    pub result: Vec<u8>,
}
impl OracleResponse {
    /// Creates an oracle response.
    pub fn new(id: u64, code: OracleResponseCode, result: Vec<u8>) -> Self {
        Self { id, code, result }
    }
    /// Serializes without the attribute type byte.
    pub fn serialize_without_type(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        <Self as Serializable>::serialize(self, writer)
    }
}

/// Well-known script hash of the Oracle native contract.
///
/// Native contract hashes are protocol-defined constants, so the protocol
/// layer can embed the value directly instead of depending on the native
/// contract registry (which lives in a higher layer).
pub const ORACLE_CONTRACT_HASH: &str = "0xfe924b7cfe89ddd271abaf7210a80a7e11178758";

/// Interop name of the contract-call syscall used by the fixed script.
const CONTRACT_CALL_INTEROP: &str = "System.Contract.Call";

/// Returns the fixed script for oracle response transactions.
///
/// This is the single source of truth for the NeoVM script that every oracle
/// response transaction must carry (and the only script accepted by oracle
/// response verification). It invokes the Oracle native contract's `finish`
/// method with `CallFlags::ALL`:
///
/// ```text
/// NEWARRAY0
/// PUSHINT CallFlags::ALL          (0x0f -> PUSH15)
/// PUSHDATA1 "finish"
/// PUSHDATA1 <Oracle contract hash>
/// SYSCALL System.Contract.Call
/// ```
///
/// Reference: C# Neo `OracleResponse.FixedScript`.
///
/// Layering note: the engine-side builders (neo-native-contracts oracle
/// verification, neo-ledger attribute validation, neo-system response
/// transaction adapter) all delegate to this function, so the script can never
/// drift between the producer and the verifier of oracle response transactions.
pub fn get_fixed_script() -> Vec<u8> {
    static ORACLE_FINISH_SCRIPT: OnceLock<Vec<u8>> = OnceLock::new();
    ORACLE_FINISH_SCRIPT
        .get_or_init(build_oracle_finish_script)
        .clone()
}

/// Builds the oracle finish script.
///
/// On any builder failure (a programming error, since the interop name is a
/// compile-time constant well below the 252-byte syscall limit) the result is
/// an empty script so that verification comparisons fail closed.
fn build_oracle_finish_script() -> Vec<u8> {
    let oracle_hash = match neo_primitives::UInt160::parse(ORACLE_CONTRACT_HASH) {
        Ok(hash) => hash,
        Err(err) => {
            tracing::error!(
                ?err,
                "Failed to parse the Oracle native contract hash constant"
            );
            return Vec::new();
        }
    };
    let mut builder = neo_vm::ScriptBuilder::new();
    builder.emit_opcode(neo_vm::OpCode::NEWARRAY0);
    builder.emit_push_int(neo_primitives::CallFlags::ALL.bits() as i64);
    builder.emit_push(b"finish");
    builder.emit_push(&oracle_hash.to_array());
    if let Err(err) = builder.emit_syscall(CONTRACT_CALL_INTEROP) {
        tracing::error!(
            ?err,
            "Failed to build the OracleContract finish syscall script"
        );
        return Vec::new();
    }
    builder.to_array()
}
impl Serializable for OracleResponse {
    fn size(&self) -> usize {
        8 + 1 + get_var_size_bytes(&self.result)
    }
    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u64(self.id)?;
        writer.write_u8(self.code as u8)?;
        validate_length(self.result.len(), MAX_RESULT_SIZE, "Result")?;
        writer.write_var_bytes(&self.result)?;
        Ok(())
    }
    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let id = reader.read_u64()?;
        let code_byte = reader.read_u8()?;
        let code = OracleResponseCode::from_byte(code_byte)
            .ok_or_else(|| IoError::invalid_data("Invalid response code"))?;
        // This check remains deserialize-only; construction/serialization do not strengthen it.
        let result = if code == OracleResponseCode::Success {
            reader.read_var_bytes(MAX_RESULT_SIZE)?
        } else {
            let bytes = reader.read_var_bytes(MAX_RESULT_SIZE)?;
            if !bytes.is_empty() {
                return Err(IoError::invalid_data(
                    "Non-success response cannot have result",
                ));
            }
            bytes
        };
        Ok(Self { id, code, result })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Golden bytes of the oracle fixed script.
    ///
    /// Layout (matching the authoritative construction and the VM call-vector
    /// pattern `PUSHDATA1 <method> PUSHDATA1 <hash> SYSCALL 627d5b52`):
    /// NEWARRAY0, PUSH15 (`CallFlags::ALL`), PUSHDATA1 "finish",
    /// PUSHDATA1 of the well-known Oracle contract hash (little-endian),
    /// SYSCALL of the `System.Contract.Call` interop hash `0x627d5b52`.
    /// Matches C# Neo `OracleResponse.FixedScript`.
    const EXPECTED_FIXED_SCRIPT_HEX: &str =
        "c21f0c0666696e6973680c14588717117e0aa81072afab71d2dd89fe7c4b92fe41627d5b52";

    #[test]
    fn fixed_script_matches_golden_bytes() {
        let script = get_fixed_script();
        assert_eq!(
            hex::encode(&script),
            EXPECTED_FIXED_SCRIPT_HEX,
            "oracle fixed script must be byte-identical to the consensus-required script"
        );
    }

    #[test]
    fn fixed_script_is_deterministic() {
        // The OnceLock cache must not change the produced bytes between calls.
        assert_eq!(get_fixed_script(), get_fixed_script());
    }

    #[test]
    fn fixed_script_embeds_well_known_oracle_hash() {
        let script = get_fixed_script();
        assert_eq!(script.len(), 37, "fixed script length is stable");
        // Layout: NEWARRAY0(1) PUSH15(1) PUSHDATA1+"finish"(8) PUSHDATA1+hash(22) SYSCALL(5).
        assert_eq!(script[0], 0xc2, "NEWARRAY0");
        assert_eq!(script[1], 0x1f, "PUSH15 encodes CallFlags::ALL");
        assert_eq!(
            &script[2..10],
            &[0x0c, 0x06, b'f', b'i', b'n', b'i', b's', b'h'][..],
            "the pushed method name is exactly \"finish\""
        );
        assert_eq!(script[10], 0x0c, "PUSHDATA1");
        assert_eq!(script[11], 20, "pushed hash length");
        let embedded_hash = &script[12..32];
        let expected = neo_primitives::UInt160::parse(ORACLE_CONTRACT_HASH)
            .expect("well-known oracle contract hash constant")
            .to_array();
        assert_eq!(embedded_hash, &expected[..]);
        assert_eq!(script[32], 0x41, "SYSCALL");
        // The interop hash 0x627d5b52 is serialized big-endian in the script
        // (the trailing bytes of the golden hex ...41627d5b52).
        assert_eq!(&script[33..37], &0x627d5b52u32.to_be_bytes()[..]);
    }
}

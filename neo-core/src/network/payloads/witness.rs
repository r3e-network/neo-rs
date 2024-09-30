use std::convert::TryFrom;
use std::io::{self};
use crate::io::binary_writer::BinaryWriter;
use crate::io::serializable_trait::SerializableTrait;
use crate::io::memory_reader::MemoryReader;
use neo_type::{ScriptHash, ToScriptHash};
use getset::{Getters, Setters};
use NeoRust::prelude::VarSizeTrait;
use neo_base::encoding::base64::ToBase64;

/// Represents a witness of an `IVerifiable` object.
#[derive(Clone, Default, Debug, Getters, Setters)]
pub struct Witness {
    /// The invocation script of the witness. Used to pass arguments for `verification_script`.
    #[getset(get = "pub", set = "pub")]
    pub invocation_script: Vec<u8>,

    /// The verification script of the witness. It can be empty if the contract is deployed.
    #[getset(get = "pub", set = "pub")]
    pub verification_script: Vec<u8>,

    #[getset(get = "pub", set = "pub")]
    script_hash: Option<ScriptHash>,
}

impl Witness {
    // This is designed to allow a MultiSig 21/11 (committee)
    // Invocation = 11 * (64 + 2) = 726
    const MAX_INVOCATION_SCRIPT: usize = 1024;

    // Verification = m + (PUSH_PubKey * 21) + length + null + syscall = 1 + ((2 + 33) * 21) + 2 + 1 + 5 = 744
    const MAX_VERIFICATION_SCRIPT: usize = 1024;

    /// The hash of the `verification_script`.
    pub fn script_hash(&mut self) -> ScriptHash {
        if self.script_hash.is_none() {
            self.script_hash = Some(self.verification_script.to_script_hash());
        }
        self.script_hash.unwrap()
    }
}

impl SerializableTrait for Witness {
    fn size(&self) -> usize {
        self.invocation_script.var_size() + self.verification_script.var_size()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> io::Result<()> {
        writer.write_var_bytes(&self.invocation_script)?;
        writer.write_var_bytes(&self.verification_script)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, std::io::Error> {
        let invocation_script = reader.read_var_bytes(Self::MAX_INVOCATION_SCRIPT)?;
        let verification_script = reader.read_var_bytes(Self::MAX_VERIFICATION_SCRIPT)?;
        Ok(Self {
            invocation_script,
            verification_script,
            script_hash: None,
        })
    }
}

impl Witness {
    /// Converts the witness to a JSON object.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "invocation": self.invocation_script.to_base64_std(),
            "verification": self.verification_script.to_base64_std()
        })
    }
}

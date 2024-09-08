use std::convert::TryFrom;
use std::io::{self, Read, Write};
use std::mem;
use crate::io::iserializable::ISerializable;
use crate::uint160::UInt160;

/// Represents a witness of an `IVerifiable` object.
pub struct Witness {
    /// The invocation script of the witness. Used to pass arguments for `verification_script`.
    pub invocation_script: Vec<u8>,

    /// The verification script of the witness. It can be empty if the contract is deployed.
    pub verification_script: Vec<u8>,

    script_hash: Option<UInt160>,
}

impl Witness {
    // This is designed to allow a MultiSig 21/11 (committee)
    // Invocation = 11 * (64 + 2) = 726
    const MAX_INVOCATION_SCRIPT: usize = 1024;

    // Verification = m + (PUSH_PubKey * 21) + length + null + syscall = 1 + ((2 + 33) * 21) + 2 + 1 + 5 = 744
    const MAX_VERIFICATION_SCRIPT: usize = 1024;

    /// The hash of the `verification_script`.
    pub fn script_hash(&mut self) -> UInt160 {
        if self.script_hash.is_none() {
            self.script_hash = Some(Helper::to_script_hash(&self.verification_script));
        }
        self.script_hash.unwrap()
    }

    pub fn size(&self) -> usize {
        self.invocation_script.get_var_size() + self.verification_script.get_var_size()
    }
}

impl ISerializable for Witness {
    fn size(&self) -> usize {
        todo!()
    }

    fn serialize<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_var_bytes(&self.invocation_script)?;
        writer.write_var_bytes(&self.verification_script)?;
        Ok(())
    }

    fn deserialize<R: Read>(&mut self, reader: &mut R) -> io::Result<()> {
        self.invocation_script = reader.read_var_bytes(Self::MAX_INVOCATION_SCRIPT)?;
        self.verification_script = reader.read_var_bytes(Self::MAX_VERIFICATION_SCRIPT)?;
        Ok(())
    }
}

impl Witness {
    /// Converts the witness to a JSON object.
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("invocation", base64::encode(&self.invocation_script));
        json.insert("verification", base64::encode(&self.verification_script));
        json
    }
}

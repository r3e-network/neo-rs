use std::convert::TryFrom;
use std::io::{self, Write};
use NeoRust::builder::ScriptBuilder;
use NeoRust::codec::VarSizeTrait;
use neo_json::jtoken::JToken;
use crate::io::binary_writer::BinaryWriter;
use crate::io::iserializable::ISerializable;
use crate::io::memory_reader::MemoryReader;
use crate::network::Payloads::{OracleResponseCode, Transaction, TransactionAttribute, TransactionAttributeType, WitnessScope};
use crate::persistence::DataCache;

pub struct OracleResponse {
    /// The ID of the oracle request.
    pub id: u64,

    /// The response code for the oracle request.
    pub code: OracleResponseCode,

    /// The result for the oracle request.
    pub result: Vec<u8>,
}

impl OracleResponse {
    /// Indicates the maximum size of the `result` field.
    pub const MAX_RESULT_SIZE: usize = u16::MAX as usize;

    /// Represents the fixed value of the `Transaction.script` field of the oracle responding transaction.
    pub const FIXED_SCRIPT: Vec<u8> = {
        let mut sb = ScriptBuilder::new();
        sb.contract_call(NativeContract::Oracle.hash(), "finish");
        sb.to_vec()
    };
}

impl ISerializable for OracleResponse {
    fn size(&self) -> usize {
        todo!()
    }

    fn serialize(&self, writer: &mut BinaryWriter) {
        todo!()
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, std::io::Error> {
        todo!()
    }
}

impl TransactionAttribute for OracleResponse {
    fn get_type(&self) -> TransactionAttributeType {
        TransactionAttributeType::OracleResponse
    }

    fn allow_multiple(&self) -> bool {
        false
    }

    fn size(&self) -> usize {
        std::mem::size_of::<u64>() +  // Id
        std::mem::size_of::<OracleResponseCode>() +  // ResponseCode
        self.result.var_size()  // Result
    }

    fn deserialize_without_type(&mut self, reader: &mut MemoryReader) -> io::Result<()> {
        self.id = reader.read_u64()?;
        self.code = OracleResponseCode::try_from(reader.read_u8()?).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid OracleResponseCode"))?;
        self.result = reader.read_var_bytes(Self::MAX_RESULT_SIZE)?;
        
        if self.code != OracleResponseCode::Success && !self.result.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Result must be empty for non-success codes"));
        }
        
        Ok(())
    }

    fn serialize_without_type(&self, writer: &mut dyn Write) -> io::Result<()> {
        writer.write_all(&self.id.to_le_bytes())?;
        writer.write_all(&[self.code as u8])?;
        writer.write_var_bytes(&self.result)?;
        Ok(())
    }

    fn to_json(&self) -> JToken {
        let mut json = JToken::new_object();
        json.insert("id".to_string(), JValue::Number(self.id.into()));
        json.insert("code".to_string(), JValue::Number(self.code as u8 as i64));
        json.insert("result".to_string(), JValue::String(base64::encode(&self.result)));
        json
    }

    fn verify(&self, snapshot: &dyn DataCache, tx: &Transaction) -> bool {
        if tx.signers.iter().any(|p| p.scopes != WitnessScope::None) {
            return false;
        }
        if tx.script != Self::FIXED_SCRIPT {
            return false;
        }
        let request = NativeContract::Oracle.get_request(snapshot, self.id);
        if request.is_none() {
            return false;
        }
        if tx.network_fee + tx.system_fee != request.unwrap().gas_for_response {
            return false;
        }
        let oracle_account = Contract::get_bft_address(
            NativeContract::RoleManagement.get_designated_by_role(
                snapshot,
                Role::Oracle,
                NativeContract::Ledger.current_index(snapshot) + 1
            )
        );
        tx.signers.iter().any(|p| p.account == oracle_account)
    }
}

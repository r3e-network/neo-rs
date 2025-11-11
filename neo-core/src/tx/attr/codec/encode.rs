use neo_base::encoding::{NeoEncode, NeoWrite};

use super::super::{TxAttr, TxAttrType};

impl NeoEncode for TxAttr {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        match self {
            TxAttr::HighPriority => writer.write_u8(TxAttrType::HighPriority as u8),
            TxAttr::OracleResponse(resp) => {
                writer.write_u8(TxAttrType::OracleResponse as u8);
                writer.write_u64(resp.id);
                writer.write_u8(resp.code as u8);
                writer.write_var_bytes(resp.result.as_slice());
            }
            TxAttr::NotValidBefore(value) => {
                writer.write_u8(TxAttrType::NotValidBefore as u8);
                writer.write_u64(value.height);
            }
            TxAttr::Conflicts(conflict) => {
                writer.write_u8(TxAttrType::Conflicts as u8);
                conflict.hash.neo_encode(writer);
            }
            TxAttr::NotaryAssisted(attr) => {
                writer.write_u8(TxAttrType::NotaryAssisted as u8);
                writer.write_u8(attr.nkeys);
            }
        }
    }
}

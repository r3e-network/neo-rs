use super::{
    oracle::OracleResponse,
    structs::{Conflicts, NotValidBefore, NotaryAssisted},
};

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type")]
pub enum TxAttr {
    HighPriority,
    OracleResponse(OracleResponse),
    NotValidBefore(NotValidBefore),
    Conflicts(Conflicts),
    NotaryAssisted(NotaryAssisted),
}

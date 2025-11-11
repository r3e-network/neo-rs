#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum TxAttrType {
    HighPriority = 0x01,
    OracleResponse = 0x11,
    NotValidBefore = 0x20,
    Conflicts = 0x21,
    NotaryAssisted = 0x22,
}

impl TxAttrType {
    pub fn allow_multiple(self) -> bool {
        self == Self::Conflicts
    }
}

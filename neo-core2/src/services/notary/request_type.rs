#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestType {
    /// Signature represents standard single signature request type.
    Signature = 0x01,
    /// MultiSignature represents m out of n multisignature request type.
    MultiSignature = 0x02,
    /// Contract represents contract witness type.
    Contract = 0x03,
}

/// Identifier assigned to each validator position.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    neo_base::NeoEncodeDerive,
    neo_base::NeoDecodeDerive,
)]
pub struct ValidatorId(pub u16);

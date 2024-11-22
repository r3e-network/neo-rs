use thiserror::Error;

#[derive(Debug, Error)]
pub enum TypeError {
    #[error("invalid conversion")]
    InvalidConversion,

    #[error("too big")]
    TooBig,

    #[error("item is read-only")]
    ReadOnly,

    #[error("too big: uncomparable")]
    TooBigComparable,

    #[error("too big: integer")]
    TooBigInteger,

    #[error("too big: map key")]
    TooBigKey,

    #[error("too big: size")]
    TooBigSize,

    #[error("too big: many elements")]
    TooBigElements,
}

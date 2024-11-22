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



#[derive(Debug, errors::Error)]
pub enum CheckedEqError {
    #[error("checked_eq: {0:?} exceed max comparable size")]
    ExceedMaxComparableSize(StackItemType),

    #[error("checked_eq: exceed max nest limit: {0}")]
    ExceedMaxNestLimit(usize),
}

#[derive(Debug, errors::Error)]
pub enum CastError {
    #[error("cast: from {0:?} to {1} invalid: {2}")]
    InvalidCast(StackItemType, &'static str, &'static str),
    
    #[error("from TypeError: {0}")]
    FromTypeError(#[from] TypeError),
}

impl CastError {
    #[inline]
    pub fn item_type(&self) -> StackItemType {
        match self {
            InvalidCast(item_type, _, _) => *item_type,
            FromTypeError(err) => err.item_type(),
        }
    }
}

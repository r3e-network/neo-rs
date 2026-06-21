//! Change view reason - Why validators request a view change.

use neo_primitives::protocol_enum;

protocol_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    #[allow(missing_docs)]
    /// Change view reason enum matching C# `ChangeViewReason` exactly
    pub ChangeViewReason {
        #[default]
        Timeout = 0x0,
        ChangeAgreement = 0x1,
        TxNotFound = 0x2,
        TxRejectedByPolicy = 0x3,
        TxInvalid = 0x4,
        BlockRejectedByPolicy = 0x5,
    }
}

#[cfg(test)]
#[path = "tests/change_view_reason.rs"]
mod tests;

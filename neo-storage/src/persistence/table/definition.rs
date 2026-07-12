//! Logical table identity and namespace contracts.

use std::fmt::Debug;

use super::codec::{TableCodec, TableEncode};

/// Physical namespace containing a logical table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableNamespace {
    /// Neo consensus-visible contract-storage bytes.
    Data,
    /// Node-local metadata excluded from contract scans and state roots.
    Maintenance,
}

/// Statically typed logical table over an existing storage namespace.
///
/// A table binds one key type, value type, and their codecs. Marker types live
/// in the domain that owns the persisted record, keeping protocol knowledge out
/// of storage backends. Implementations must preserve any established byte
/// representation because table identity does not authorize a schema change.
pub trait Table: Debug + Send + Sync + 'static {
    /// Typed lookup key.
    type Key: 'static;
    /// Typed stored value.
    type Value: 'static;
    /// Stable key encoder. Point reads do not require key decoding.
    type KeyCodec: TableEncode<Self::Key>;
    /// Stable value encoder and strict decoder.
    type ValueCodec: TableCodec<Self::Value>;

    /// Diagnostic table name used in errors and schema documentation.
    const NAME: &'static str;
    /// Physical namespace that owns this logical table.
    const NAMESPACE: TableNamespace;
}

//! Typed point-read capability for concrete storage providers.

use super::codec::{TableDecode, TableEncode};
use super::{Table, TableNamespace};
use crate::persistence::TransactionalStore;
use crate::{StorageError, StorageResult};

/// Typed logical-table reads available on every transactional store.
///
/// This blanket capability remains statically dispatched. Runtime backend
/// selection can use the concrete `RuntimeStore` enum while downstream table
/// accesses still monomorphize over the table and its codecs.
pub trait TableProvider: TransactionalStore {
    /// Reads and strictly decodes one logical-table value.
    fn table_get<T: Table>(&self, key: &T::Key) -> StorageResult<Option<T::Value>> {
        let encoded = <T::KeyCodec as TableEncode<T::Key>>::encode(key)
            .map_err(|error| table_codec_error::<T>("encode key", error))?;
        let bytes = match T::NAMESPACE {
            TableNamespace::Data => self.try_get_bytes(encoded.as_ref()),
            TableNamespace::Maintenance => self.maintenance_metadata(encoded.as_ref())?,
        };
        bytes
            .map(|bytes| {
                <T::ValueCodec as TableDecode<T::Value>>::decode(&bytes)
                    .map_err(|error| table_codec_error::<T>("decode value", error))
            })
            .transpose()
    }

    /// Returns whether one logical-table key exists.
    fn table_contains<T: Table>(&self, key: &T::Key) -> StorageResult<bool> {
        self.table_get::<T>(key).map(|value| value.is_some())
    }
}

impl<S: TransactionalStore> TableProvider for S {}

fn table_codec_error<T: Table>(operation: &'static str, error: StorageError) -> StorageError {
    StorageError::serialization(format!("{operation} for table {}: {error}", T::NAME))
}

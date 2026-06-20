//! Designation storage keys and lookup helpers for RoleManagement.

use neo_storage::StorageKey;
use neo_storage::persistence::{DataCache, SeekDirection};

use super::RoleManagement;

/// Finds the serialized node-list value for `role_byte` effective at `index`:
/// the entry with the greatest designation index that is <= `index`.
///
/// Designations are stored under key `(RoleManagement.ID, [role_byte, index_be])`.
/// A `Backward` prefix scan yields them in descending designation-index order, so
/// the first one with `designation_index <= index` is the effective designation,
/// matching C#'s `FindRange((role, index), (role), Backward).FirstOrDefault`.
pub(super) fn find_designation_value(
    snapshot: &DataCache,
    role_byte: u8,
    index: u32,
) -> Option<Vec<u8>> {
    let prefix = designation_prefix_key(role_byte);
    for (key, item) in snapshot.find(Some(&prefix), SeekDirection::Backward) {
        let key_bytes = key.key();
        if key_bytes.len() >= 5 {
            let designation_index =
                u32::from_be_bytes([key_bytes[1], key_bytes[2], key_bytes[3], key_bytes[4]]);
            if designation_index <= index {
                return Some(item.value_bytes().into_owned());
            }
        }
    }
    None
}

/// The designation prefix key `(RoleManagement.ID, [role_byte])`.
pub(super) fn designation_prefix_key(role_byte: u8) -> StorageKey {
    crate::keys::prefixed_key(RoleManagement::ID, role_byte, &[])
}

/// The designation storage key `(RoleManagement.ID, [role_byte, index_be])`.
pub(crate) fn designation_key(role_byte: u8, index: u32) -> StorageKey {
    crate::keys::prefixed_u32_be_key(RoleManagement::ID, role_byte, index)
}

use core::fmt;

use bitflags::bitflags;

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct StorageFindOptions: u8 {
        const NONE = 0;
        const KEYS_ONLY = 0b0000_0001;
        const REMOVE_PREFIX = 0b0000_0010;
        const VALUES_ONLY = 0b0000_0100;
        const DESERIALIZE_VALUES = 0b0000_1000;
        const PICK_FIELD0 = 0b0001_0000;
        const PICK_FIELD1 = 0b0010_0000;
        const BACKWARDS = 0b1000_0000;
        const ALL = Self::KEYS_ONLY.bits()
            | Self::REMOVE_PREFIX.bits()
            | Self::VALUES_ONLY.bits()
            | Self::DESERIALIZE_VALUES.bits()
            | Self::PICK_FIELD0.bits()
            | Self::PICK_FIELD1.bits()
            | Self::BACKWARDS.bits();
    }
}

impl Default for StorageFindOptions {
    fn default() -> Self {
        StorageFindOptions::NONE
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageFindOptionsError {
    UnknownFlags(u8),
    ConflictingKeysOnly,
    ConflictingValuesOnly,
    ConflictingPickFields,
    PickFieldWithoutDeserialize,
}

impl fmt::Display for StorageFindOptionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageFindOptionsError::UnknownFlags(bits) => {
                write!(f, "unknown StorageFindOptions bits: {bits:#04x}")
            }
            StorageFindOptionsError::ConflictingKeysOnly => {
                write!(f, "KeysOnly cannot be combined with value-only options")
            }
            StorageFindOptionsError::ConflictingValuesOnly => {
                write!(f, "ValuesOnly cannot be combined with KeysOnly or RemovePrefix")
            }
            StorageFindOptionsError::ConflictingPickFields => {
                write!(f, "PickField0 and PickField1 are mutually exclusive")
            }
            StorageFindOptionsError::PickFieldWithoutDeserialize => {
                write!(f, "PickField requires DeserializeValues to be set")
            }
        }
    }
}

impl StorageFindOptions {
    pub fn validate(self) -> Result<(), StorageFindOptionsError> {
        if self.bits() & !StorageFindOptions::ALL.bits() != 0 {
            return Err(StorageFindOptionsError::UnknownFlags(self.bits()));
        }

        let keys_only = self.contains(StorageFindOptions::KEYS_ONLY);
        let values_only = self.contains(StorageFindOptions::VALUES_ONLY);
        let deserialize = self.contains(StorageFindOptions::DESERIALIZE_VALUES);
        let pick0 = self.contains(StorageFindOptions::PICK_FIELD0);
        let pick1 = self.contains(StorageFindOptions::PICK_FIELD1);
        let remove_prefix = self.contains(StorageFindOptions::REMOVE_PREFIX);

        if keys_only && (values_only || deserialize || pick0 || pick1) {
            return Err(StorageFindOptionsError::ConflictingKeysOnly);
        }

        if values_only && (keys_only || remove_prefix) {
            return Err(StorageFindOptionsError::ConflictingValuesOnly);
        }

        if pick0 && pick1 {
            return Err(StorageFindOptionsError::ConflictingPickFields);
        }

        if (pick0 || pick1) && !deserialize {
            return Err(StorageFindOptionsError::PickFieldWithoutDeserialize);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{StorageFindOptions as Opt, StorageFindOptionsError as Error};

    #[test]
    fn rejects_unknown_bits() {
        let options = Opt::from_bits_retain(0b0100_0000);
        assert!(matches!(options.validate(), Err(Error::UnknownFlags(_))));
    }

    #[test]
    fn keys_only_conflicts_with_value_flags() {
        let options = Opt::KEYS_ONLY | Opt::DESERIALIZE_VALUES;
        assert_eq!(options.validate(), Err(Error::ConflictingKeysOnly));
    }
}

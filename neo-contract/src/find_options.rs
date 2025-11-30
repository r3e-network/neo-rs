//! FindOptions - matches C# Neo.SmartContract.FindOptions exactly

use bitflags::bitflags;

bitflags! {
    /// Specify the options to be used during the search (matches C# FindOptions)
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct FindOptions: u8 {
        /// No option is set. The results will be an iterator of (key, value)
        const NONE = 0;

        /// Indicates that only keys need to be returned. The results will be an iterator of keys
        const KEYS_ONLY = 1 << 0;

        /// Indicates that the prefix byte of keys should be removed before return
        const REMOVE_PREFIX = 1 << 1;

        /// Indicates that only values need to be returned. The results will be an iterator of values
        const VALUES_ONLY = 1 << 2;

        /// Indicates that values should be deserialized before return
        const DESERIALIZE_VALUES = 1 << 3;

        /// Indicates that only the field 0 of the deserialized values need to be returned.
        /// This flag must be set together with DeserializeValues
        const PICK_FIELD0 = 1 << 4;

        /// Indicates that only the field 1 of the deserialized values need to be returned.
        /// This flag must be set together with DeserializeValues
        const PICK_FIELD1 = 1 << 5;

        /// Indicates that results should be returned in backwards (descending) order
        const BACKWARDS = 1 << 7;

        /// This value is only for internal use, and shouldn't be used in smart contracts
        const ALL = Self::KEYS_ONLY.bits() | Self::REMOVE_PREFIX.bits() | Self::VALUES_ONLY.bits() |
                    Self::DESERIALIZE_VALUES.bits() | Self::PICK_FIELD0.bits() | Self::PICK_FIELD1.bits() |
                    Self::BACKWARDS.bits();
    }
}

#[allow(non_upper_case_globals)]
impl FindOptions {
    /// Alias for NONE (C# naming convention)
    pub const None: FindOptions = FindOptions::NONE;
    /// Alias for KEYS_ONLY (C# naming convention)
    pub const KeysOnly: FindOptions = FindOptions::KEYS_ONLY;
    /// Alias for REMOVE_PREFIX (C# naming convention)
    pub const RemovePrefix: FindOptions = FindOptions::REMOVE_PREFIX;
    /// Alias for VALUES_ONLY (C# naming convention)
    pub const ValuesOnly: FindOptions = FindOptions::VALUES_ONLY;
    /// Alias for DESERIALIZE_VALUES (C# naming convention)
    pub const DeserializeValues: FindOptions = FindOptions::DESERIALIZE_VALUES;
    /// Alias for PICK_FIELD0 (C# naming convention)
    pub const PickField0: FindOptions = FindOptions::PICK_FIELD0;
    /// Alias for PICK_FIELD1 (C# naming convention)
    pub const PickField1: FindOptions = FindOptions::PICK_FIELD1;
    /// Alias for BACKWARDS (C# naming convention)
    pub const Backwards: FindOptions = FindOptions::BACKWARDS;
    /// Alias for ALL (C# naming convention)
    pub const All: FindOptions = FindOptions::ALL;
}

impl Default for FindOptions {
    fn default() -> Self {
        FindOptions::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_options_values() {
        assert_eq!(FindOptions::NONE.bits(), 0);
        assert_eq!(FindOptions::KEYS_ONLY.bits(), 1);
        assert_eq!(FindOptions::REMOVE_PREFIX.bits(), 2);
        assert_eq!(FindOptions::VALUES_ONLY.bits(), 4);
        assert_eq!(FindOptions::DESERIALIZE_VALUES.bits(), 8);
        assert_eq!(FindOptions::PICK_FIELD0.bits(), 16);
        assert_eq!(FindOptions::PICK_FIELD1.bits(), 32);
        assert_eq!(FindOptions::BACKWARDS.bits(), 128);
    }

    #[test]
    fn test_find_options_combinations() {
        let opts = FindOptions::KEYS_ONLY | FindOptions::BACKWARDS;
        assert!(opts.contains(FindOptions::KEYS_ONLY));
        assert!(opts.contains(FindOptions::BACKWARDS));
        assert!(!opts.contains(FindOptions::VALUES_ONLY));
    }

    #[test]
    fn test_find_options_default() {
        assert_eq!(FindOptions::default(), FindOptions::NONE);
    }
}

use bitflags::bitflags;

/// Specify the options to be used during the search.
bitflags! {
    pub struct FindOptions: u8 {
        /// No option is set. The results will be an iterator of (key, value).
        const NONE = 0;

        /// Indicates that only keys need to be returned. The results will be an iterator of keys.
        const KEYS_ONLY = 1 << 0;

        /// Indicates that the prefix byte of keys should be removed before return.
        const REMOVE_PREFIX = 1 << 1;

        /// Indicates that only values need to be returned. The results will be an iterator of values.
        const VALUES_ONLY = 1 << 2;

        /// Indicates that values should be deserialized before return.
        const DESERIALIZE_VALUES = 1 << 3;

        /// Indicates that only the field 0 of the deserialized values need to be returned. This flag must be set together with DESERIALIZE_VALUES.
        const PICK_FIELD0 = 1 << 4;

        /// Indicates that only the field 1 of the deserialized values need to be returned. This flag must be set together with DESERIALIZE_VALUES.
        const PICK_FIELD1 = 1 << 5;

        /// Indicates that results should be returned in backwards (descending) order.
        const BACKWARDS = 1 << 7;

        /// This value is only for internal use, and shouldn't be used in smart contracts.
        const ALL = Self::KEYS_ONLY.bits() | Self::REMOVE_PREFIX.bits() | Self::VALUES_ONLY.bits() |
                    Self::DESERIALIZE_VALUES.bits() | Self::PICK_FIELD0.bits() | Self::PICK_FIELD1.bits() |
                    Self::BACKWARDS.bits();
    }
}

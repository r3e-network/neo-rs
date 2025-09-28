//! FindOptions - matches C# Neo.SmartContract.FindOptions exactly

use bitflags::bitflags;

bitflags! {
    /// Specify the options to be used during the search (matches C# FindOptions)
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FindOptions: u8 {
        /// No option is set. The results will be an iterator of (key, value)
        const None = 0;

        /// Indicates that only keys need to be returned. The results will be an iterator of keys
        const KeysOnly = 1 << 0;

        /// Indicates that the prefix byte of keys should be removed before return
        const RemovePrefix = 1 << 1;

        /// Indicates that only values need to be returned. The results will be an iterator of values
        const ValuesOnly = 1 << 2;

        /// Indicates that values should be deserialized before return
        const DeserializeValues = 1 << 3;

        /// Indicates that only the field 0 of the deserialized values need to be returned.
        /// This flag must be set together with DeserializeValues
        const PickField0 = 1 << 4;

        /// Indicates that only the field 1 of the deserialized values need to be returned.
        /// This flag must be set together with DeserializeValues
        const PickField1 = 1 << 5;

        /// Indicates that results should be returned in backwards (descending) order
        const Backwards = 1 << 7;

        /// This value is only for internal use, and shouldn't be used in smart contracts
        const All = Self::KeysOnly.bits() | Self::RemovePrefix.bits() | Self::ValuesOnly.bits() |
                    Self::DeserializeValues.bits() | Self::PickField0.bits() | Self::PickField1.bits() |
                    Self::Backwards.bits();
    }
}

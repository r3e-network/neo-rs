// Copyright (C) 2015-2024 The Neo Project.
//
// iserializable.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_io::{BinaryWriter, MemoryReader};

/// Represents NEO objects that can be serialized.
pub trait ISerializable {
    /// The size of the object in bytes after serialization.
    fn size(&self) -> usize;

    /// Serializes the object using the specified `BinaryWriter`.
    ///
    /// # Arguments
    ///
    /// * `writer` - The `BinaryWriter` for writing data.
    fn serialize(&self, writer: &mut BinaryWriter);

    /// Deserializes the object using the specified `MemoryReader`.
    ///
    /// # Arguments
    ///
    /// * `reader` - The `MemoryReader` for reading data.
    fn deserialize(&mut self, reader: &mut MemoryReader);
}

// Copyright (C) 2015-2024 The Neo Project.
//
// application_engine_iterator.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo::prelude::*;
use neo::vm::types::StackItem;
use neo::smart_contract::iterators::Iterator as NeoIterator;

impl ApplicationEngine {
    /// The `InteropDescriptor` of System.Iterator.Next.
    /// Advances the iterator to the next element of the collection.
    pub static SYSTEM_ITERATOR_NEXT: InteropDescriptor = register_syscall(
        "System.Iterator.Next",
        ApplicationEngine::iterator_next,
        1 << 15,
        CallFlags::None
    );

    /// The `InteropDescriptor` of System.Iterator.Value.
    /// Gets the element in the collection at the current position of the iterator.
    pub static SYSTEM_ITERATOR_VALUE: InteropDescriptor = register_syscall(
        "System.Iterator.Value",
        ApplicationEngine::iterator_value,
        1 << 4,
        CallFlags::None
    );

    /// The implementation of System.Iterator.Next.
    /// Advances the iterator to the next element of the collection.
    ///
    /// # Arguments
    ///
    /// * `iterator` - The iterator to be advanced.
    ///
    /// # Returns
    ///
    /// `true` if the iterator was successfully advanced to the next element; `false` if the iterator has passed the end of the collection.
    fn iterator_next(engine: &mut ApplicationEngine) -> Result<bool, Box<dyn std::error::Error>> {
        let iterator = engine.pop_as::<NeoIterator>()?;
        Ok(iterator.next())
    }

    /// The implementation of System.Iterator.Value.
    /// Gets the element in the collection at the current position of the iterator.
    ///
    /// # Arguments
    ///
    /// * `iterator` - The iterator to be used.
    ///
    /// # Returns
    ///
    /// The element in the collection at the current position of the iterator.
    fn iterator_value(engine: &mut ApplicationEngine) -> Result<StackItem, Box<dyn std::error::Error>> {
        let iterator = engine.pop_as::<NeoIterator>()?;
        Ok(iterator.value(engine.reference_counter.clone()))
    }
}

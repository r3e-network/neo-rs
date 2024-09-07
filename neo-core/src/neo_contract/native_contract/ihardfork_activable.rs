// Copyright (C) 2015-2024 The Neo Project.
//
// ihardfork_activable.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_sdk::prelude::*;
use neo_sdk::types::Hardfork;

/// Trait for contracts that can be activated or deprecated in specific hardforks
pub trait IHardforkActivable {
    /// The hardfork in which this contract or feature becomes active
    fn active_in(&self) -> Option<Hardfork>;

    /// The hardfork in which this contract or feature becomes deprecated
    fn deprecated_in(&self) -> Option<Hardfork>;
}

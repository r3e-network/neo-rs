// Copyright (C) 2015-2025 The Neo Project.
//
// mod.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! Sign module for Neo blockchain
//!
//! This module provides signing functionality matching the C# Neo.Sign namespace.

pub mod i_signer;
pub mod sign_exception;
pub mod signer_manager;

pub use i_signer::ISigner;
pub use sign_exception::SignException;
pub use signer_manager::SignerManager;

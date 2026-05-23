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

#[allow(dead_code)]
pub mod i_signer;
#[allow(dead_code)]
pub mod sign_exception;
#[allow(dead_code)]
pub mod signer_manager;

#[allow(unused_imports)]
pub use i_signer::ISigner;
#[allow(unused_imports)]
pub use sign_exception::SignException;
#[allow(unused_imports)]
pub use signer_manager::SignerManager;

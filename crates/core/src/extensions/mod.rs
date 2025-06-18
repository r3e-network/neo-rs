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

//! Extension traits for Neo blockchain types.

pub mod byte_extensions;
pub mod uint160_extensions;

pub use byte_extensions::ByteExtensions;
pub use uint160_extensions::UInt160Extensions;

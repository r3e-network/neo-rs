// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

#[cfg(not(feature = "std"))]
use thiserror_no_std as thiserror;

pub use thiserror::Error;

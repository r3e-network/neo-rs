// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_mempool_unverified.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use serde::{Deserialize, Serialize};

/// Model describing transactions pending re-verification in the mempool.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RpcMempoolUnverified {
    /// Transaction hashes awaiting re-verification.
    pub hashes: Vec<String>,
}

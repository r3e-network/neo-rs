// Copyright (C) 2015-2025 The Neo Project.
//
// UtilsScriptHashModel mirrors Neo.Plugins.RestServer.Models.Utils.UtilsScriptHashModel.
// It exposes a script hash string representation so JSON payloads match the
// C# behaviour (which uses the UInt160 converter).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UtilsScriptHashModel {
    /// Script hash of the wallet account (lowercase hex string without 0x prefix).
    pub script_hash: String,
}

impl UtilsScriptHashModel {
    pub fn new(script_hash: impl Into<String>) -> Self {
        Self {
            script_hash: script_hash.into(),
        }
    }
}

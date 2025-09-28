// Copyright (C) 2015-2025 The Neo Project.
//
// i_log_handler.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.
use crate::{
    smart_contract::application_engine::ApplicationEngine,
    smart_contract::log_event_args::LogEventArgs,
};

/// Log handler interface matching C# ILogHandler exactly
pub trait ILogHandler {
    /// The handler of Log event from the ApplicationEngine.
    /// Triggered when a contract calls System.Runtime.Log.
    /// Matches C# ApplicationEngine_Log_Handler method
    fn application_engine_log_handler(
        &self,
        sender: &ApplicationEngine,
        log_event_args: &LogEventArgs,
    );
}

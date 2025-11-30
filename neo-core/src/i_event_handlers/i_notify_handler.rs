// Copyright (C) 2015-2025 The Neo Project.
//
// i_notify_handler.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.
use crate::{
    smart_contract::application_engine::ApplicationEngine,
    smart_contract::notify_event_args::NotifyEventArgs,
};

/// Notify handler interface matching C# INotifyHandler exactly
pub trait INotifyHandler {
    /// The handler of Notify event from ApplicationEngine
    /// Triggered when a contract calls System.Runtime.Notify.
    /// Matches C# ApplicationEngine_Notify_Handler method
    fn application_engine_notify_handler(
        &self,
        sender: &ApplicationEngine,
        notify_event_args: &NotifyEventArgs,
    );
}

// Copyright (C) 2015-2024 The Neo Project.
//
// idle.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use std::sync::Once;

pub mod actors {
    pub struct Idle;

    impl Idle {
        pub fn instance() -> &'static Idle {
            static INSTANCE: Once = Once::new();
            static mut SINGLETON: Option<Idle> = None;

            INSTANCE.call_once(|| {
                unsafe {
                    SINGLETON = Some(Idle);
                }
            });

            unsafe { SINGLETON.as_ref().unwrap() }
        }
    }
}

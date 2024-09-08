
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

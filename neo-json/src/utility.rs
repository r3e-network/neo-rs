use std::sync::OnceLock;
use encoding_rs::Encoding;

pub mod json {
    use super::*;

    pub struct Utility;

    impl Utility {
        pub fn strict_utf8() -> &'static Encoding {
            static STRICT_UTF8: OnceLock<&Encoding> = OnceLock::new();
            STRICT_UTF8.get_or_init(|| {
                &encoding_rs::UTF_8
            })
        }
    }
}

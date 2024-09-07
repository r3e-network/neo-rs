
use std::sync::OnceLock;
use encoding_rs::Encoding;

pub mod json {
    use super::*;

    pub struct Utility;

    impl Utility {
        pub fn strict_utf8() -> &'static Encoding {
            static STRICT_UTF8: OnceLock<Encoding> = OnceLock::new();
            STRICT_UTF8.get_or_init(|| {
                let mut utf8 = *encoding_rs::UTF_8;
                utf8.set_decoder_fallback(encoding_rs::DecoderFallback::Strict);
                utf8.set_encoder_fallback(encoding_rs::EncoderFallback::Strict);
                utf8
            })
        }
    }
}

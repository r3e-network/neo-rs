// Copyright @ 2025 - Present, R3E Network
// All Rights Reserved

use hmac::{Hmac, Mac};
use sha2::Sha256;

pub trait HmacSha256 {
    fn hmac_sha256(&self, data: &[u8]) -> [u8; 32];
}

impl HmacSha256 for [u8] {
    #[inline]
    fn hmac_sha256(&self, data: &[u8]) -> [u8; 32] {
        let mut hmac = Hmac::<Sha256>::new_from_slice(self).expect("Any key length should be OK");
        hmac.update(data);
        hmac.finalize().into_bytes().into()
    }
}

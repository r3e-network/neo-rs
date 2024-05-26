// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

use sha2::Digest;

pub trait Sha256 {
    fn sha256(&self) -> [u8; 32];
}

impl<T: AsRef<[u8]>> Sha256 for T {
    #[inline]
    fn sha256(&self) -> [u8; 32] {
        let mut h = sha2::Sha256::new();
        h.update(self);
        h.finalize().into()
    }
}

pub trait SlicesSha256 {
    fn slices_sha256(self) -> [u8; 32];
}

impl<T: Iterator> SlicesSha256 for T
where
    <T as Iterator>::Item: AsRef<[u8]>,
{
    #[inline]
    fn slices_sha256(self) -> [u8; 32] {
        let mut h = sha2::Sha256::new();
        self.for_each(|s| h.update(s));

        h.finalize().into()
    }
}

pub trait Ripemd160 {
    fn ripemd160(&self) -> [u8; 20];
}

impl<T: AsRef<[u8]>> Ripemd160 for T {
    #[inline]
    fn ripemd160(&self) -> [u8; 20] {
        let mut h = ripemd::Ripemd160::new();
        h.update(self);
        h.finalize().into()
    }
}
use super::SignatureBytes;

pub trait Secp256r1Sign {
    fn secp256r1_sign<T: AsRef<[u8]>>(
        &self,
        message: T,
    ) -> Result<SignatureBytes, super::SignError>;
}

pub trait Secp256r1Verify {
    fn secp256r1_verify<T: AsRef<[u8]>>(
        &self,
        message: T,
        signature: &SignatureBytes,
    ) -> Result<(), super::VerifyError>;
}

//! PKCS#11 HSM support

#[cfg(feature = "pkcs11")]
mod pkcs11_signer;

#[cfg(feature = "pkcs11")]
pub use pkcs11_signer::Pkcs11Signer;

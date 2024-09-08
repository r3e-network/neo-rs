
use neo::prelude::*;
use neo::sys;
use neo::vm::{InteropDescriptor, CallFlags};
use neo::crypto::{ECCurve, Crypto};
use neo::types::UInt160;
use std::convert::TryFrom;

impl ApplicationEngine {
    /// The price of System.Crypto.CheckSig.
    /// In the unit of datoshi, 1 datoshi = 1e-8 GAS
    pub const CHECK_SIG_PRICE: i64 = 1 << 15;

    /// The `InteropDescriptor` of System.Crypto.CheckSig.
    /// Checks the signature for the current script container.
    pub static SYSTEM_CRYPTO_CHECK_SIG: InteropDescriptor = register_syscall(
        "System.Crypto.CheckSig",
        ApplicationEngine::check_sig,
        Self::CHECK_SIG_PRICE,
        CallFlags::None
    );

    /// The `InteropDescriptor` of System.Crypto.CheckMultisig.
    /// Checks the signatures for the current script container.
    pub static SYSTEM_CRYPTO_CHECK_MULTISIG: InteropDescriptor = register_syscall(
        "System.Crypto.CheckMultisig",
        ApplicationEngine::check_multisig,
        0,
        CallFlags::None
    );

    /// The implementation of System.Crypto.CheckSig.
    /// Checks the signature for the current script container.
    ///
    /// # Arguments
    ///
    /// * `pubkey` - The public key of the account.
    /// * `signature` - The signature of the current script container.
    ///
    /// # Returns
    ///
    /// `true` if the signature is valid; otherwise, `false`.
    fn check_sig(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        let signature = self.pop_as::<Vec<u8>>()?;
        let pubkey = self.pop_as::<Vec<u8>>()?;

        let script_container = self.script_container()?;
        let network = self.protocol_settings.network_id;

        match Crypto::verify_signature(&script_container.get_sign_data(network), &signature, &pubkey, ECCurve::Secp256r1) {
            Ok(result) => Ok(result),
            Err(_) => Ok(false),
        }
    }

    /// The implementation of System.Crypto.CheckMultisig.
    /// Checks the signatures for the current script container.
    ///
    /// # Arguments
    ///
    /// * `pubkeys` - The public keys of the accounts.
    /// * `signatures` - The signatures of the current script container.
    ///
    /// # Returns
    ///
    /// `true` if the signatures are valid; otherwise, `false`.
    fn check_multisig(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        let signatures = self.pop_as::<Vec<Vec<u8>>>()?;
        let pubkeys = self.pop_as::<Vec<Vec<u8>>>()?;

        let m = signatures.len();
        let n = pubkeys.len();

        if n == 0 || m == 0 || m > n {
            return Err("Invalid number of pubkeys or signatures".into());
        }

        self.add_fee(Self::CHECK_SIG_PRICE * n as i64 * self.exec_fee_factor as i64)?;

        let script_container = self.script_container()?;
        let network = self.protocol_settings.network_id;
        let message = script_container.get_sign_data(network);

        let mut i = 0;
        let mut j = 0;

        while i < m && j < n {
            if Crypto::verify_signature(&message, &signatures[i], &pubkeys[j], ECCurve::Secp256r1).unwrap_or(false) {
                i += 1;
            }
            j += 1;
            if m - i > n - j {
                return Ok(false);
            }
        }

        Ok(true)
    }
}

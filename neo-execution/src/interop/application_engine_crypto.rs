//! ApplicationEngine.Crypto - matches C# Neo.SmartContract.ApplicationEngine.Crypto.cs

use crate::ApplicationEngine;
use neo_config::Hardfork;
use neo_crypto::Crypto;
use neo_crypto::{CryptoError, ECCurve, ECPoint, Secp256r1Crypto};
use neo_error::{CoreError, CoreResult};
use neo_manifest::CallFlags;
use neo_vm::VmResult;
use neo_vm::execution_engine::ExecutionEngine;

/// The price of CheckSig in GAS (1 << 15 = 32768 * 30 = 983040). Re-exported
/// from `application_engine` so the value has a single source of truth.
pub use crate::application_engine::CHECK_SIG_PRICE;

/// The base price of CheckMultisig is zero (matches C# InteropDescriptor).
/// The syscall charges `CHECK_SIG_PRICE * n` where `n` is the number of public keys.
pub const CHECK_MULTISIG_PRICE: i64 = 0;

impl ApplicationEngine {
    /// Verifies a signature using secp256r1 (NIST P-256)
    pub fn crypto_check_sig(&mut self) -> CoreResult<bool> {
        // Neo VM calling convention: the first parameter is on top of the stack.
        // For standard signature witnesses, the invocation script pushes the signature and
        // the verification script pushes the public key, leaving `pubkey` on top.
        let public_key = self.pop_bytes()?;
        let signature = self.pop_bytes()?;

        let message_bytes = self.get_sign_data()?;

        self.verify_signature(&message_bytes, &public_key, &signature)
    }

    /// Verifies multiple signatures (m-of-n multisig)
    /// Stack order: pubkeys on top, signatures below
    /// Each can be either an Array of byte arrays, or an integer count followed by that many byte arrays
    pub fn crypto_check_multisig(&mut self) -> CoreResult<bool> {
        // Pop public keys first (top of stack)
        let public_keys = self.pop_sig_elements()?;
        let n = public_keys.len();
        if n == 0 || n > 1024 {
            return Err(CoreError::other("Invalid public key count"));
        }

        // Matches C# ApplicationEngine.CheckMultisig:
        // AddFee(CheckSigPrice * n * ExecFeeFactor). v3.10.1 centralizes
        // factorization so overflow must not silently undercharge.
        let fee_units = CHECK_SIG_PRICE
            .checked_mul(
                i64::try_from(n).map_err(|_| CoreError::other("Invalid public key count"))?,
            )
            .ok_or_else(|| CoreError::other("CheckMultisig fee overflow"))?;
        self.add_cpu_fee(fee_units)?;

        // Pop signatures second
        let signatures = self.pop_sig_elements()?;
        let m = signatures.len();
        if m == 0 || m > n {
            return Err(CoreError::other("Invalid signature count"));
        }

        let message_bytes = self.get_sign_data()?;

        let mut verified = 0;
        let mut key_index = 0;

        for signature in &signatures {
            while key_index < public_keys.len() {
                match self.verify_signature(&message_bytes, &public_keys[key_index], signature) {
                    Ok(true) => {
                        verified += 1;
                        key_index += 1;
                        break;
                    }
                    Ok(false) => {
                        key_index += 1;
                    }
                    Err(err) => return Err(err),
                }
            }
            // Early exit if remaining signatures exceed remaining keys
            if m - verified > n - key_index {
                return Ok(false);
            }
        }

        Ok(verified >= m)
    }

    fn get_sign_data(&self) -> CoreResult<Vec<u8>> {
        let container = self
            .get_script_container()
            .ok_or_else(|| CoreError::other("No script container available"))?;
        let network = self.protocol_settings().network;
        // Single canonical `network (u32 LE) ‖ hash` preimage builder.
        neo_payloads::get_sign_data_vec(container.as_ref(), network)
    }

    /// Pop signature elements from stack - handles both Array format and N+items format
    fn pop_sig_elements(&mut self) -> CoreResult<Vec<Vec<u8>>> {
        let item = self.pop()?;

        match &item {
            neo_vm::stack_item::StackItem::Array(arr) => {
                // Array format: extract all byte arrays
                let items = arr.items();
                let mut result = Vec::with_capacity(items.len());
                for item in items {
                    result.push(
                        item.as_bytes()
                            .map_err(|e| CoreError::other(e.to_string()))?,
                    );
                }
                Ok(result)
            }
            _ => {
                // Integer format: pop N items
                let count = item
                    .as_int()
                    .map_err(|e| CoreError::other(e.to_string()))?
                    .try_into()
                    .map_err(|_| CoreError::other("Count out of range"))?;
                let count: usize = count;
                if count == 0 || count > 1024 {
                    return Err(CoreError::other("Invalid element count"));
                }
                let mut result = Vec::with_capacity(count);
                for _ in 0..count {
                    result.push(self.pop_bytes()?);
                }
                Ok(result)
            }
        }
    }

    /// SHA256 hash
    pub fn crypto_sha256(&mut self) -> CoreResult<()> {
        let data = self.pop_bytes()?;
        self.push_bytes(Crypto::sha256(&data).to_vec())
    }

    /// RIPEMD160 hash
    pub fn crypto_ripemd160(&mut self) -> CoreResult<()> {
        let data = self.pop_bytes()?;
        self.push_bytes(Crypto::ripemd160(&data).to_vec())
    }

    /// Verifies a signature using secp256r1.
    ///
    /// Mirrors C# v3.10.1 `CheckSig`: the public key is decoded before the
    /// signature length is judged. Before Gorgon, non-64-byte signatures return
    /// `false`; from Gorgon onward strict `VerifySignature` faults instead.
    fn verify_signature(
        &self,
        message: &[u8],
        public_key: &[u8],
        signature: &[u8],
    ) -> CoreResult<bool> {
        if signature.len() != 64 {
            // C# decodes the public key first (argument evaluation), so an
            // invalid key faults here before the signature length is judged.
            if ECPoint::decode(public_key, ECCurve::secp256r1()).is_err() {
                if pubkey_coord_out_of_field(public_key) {
                    return Ok(false);
                }
                return Err(CoreError::other("Invalid public key"));
            }
            if self.is_hardfork_enabled(Hardfork::HfGorgon) {
                return Err(CoreError::other("Invalid signature length"));
            }
            return Ok(false);
        }
        let signature = <&[u8; 64]>::try_from(signature)
            .map_err(|_| CoreError::other("Invalid signature length"))?;

        if public_key.len() != 33 && public_key.len() != 65 {
            return Err(CoreError::other("Invalid public key length"));
        }

        match Secp256r1Crypto::verify(message, signature, public_key) {
            Ok(verified) => Ok(verified),
            Err(CryptoError::InvalidSignature { .. }) => Ok(false),
            Err(CryptoError::InvalidKey { .. } | CryptoError::InvalidPoint { .. }) => {
                // C# ECFieldElement throws ArgumentException for a coordinate
                // >= Q, which CheckSig/CheckMultisig catch and return false; all
                // other decode failures (bad prefix/length, off-curve) fault.
                if pubkey_coord_out_of_field(public_key) {
                    Ok(false)
                } else {
                    Err(CoreError::other("Invalid public key"))
                }
            }
            Err(err) => Err(CoreError::other(err.to_string())),
        }
    }
}

/// secp256r1 field prime Q (big-endian, 32 bytes).
const SECP256R1_Q: [u8; 32] = [
    0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
];

/// Returns true iff `pk` has a correct length+prefix but a coordinate `>= Q`.
///
/// This is exactly the C# `ECPoint.DecodePoint` case that raises
/// `ArgumentException` (caught by `CheckSig`/`CheckMultisig` → `false`).
/// Wrong prefix/length is a C# `FormatException`/`IndexOutOfRange` (fault), and
/// an in-field-but-off-curve point is an `Arithmetic`/`Cryptographic` exception
/// (fault) — both keep returning `Err` so the syscall faults, matching C#.
fn pubkey_coord_out_of_field(pk: &[u8]) -> bool {
    fn ge_q(coord: &[u8]) -> bool {
        for i in 0..32 {
            if coord[i] != SECP256R1_Q[i] {
                return coord[i] > SECP256R1_Q[i];
            }
        }
        true
    }
    match pk.first() {
        Some(0x02 | 0x03) if pk.len() == 33 => ge_q(&pk[1..33]),
        Some(0x04) if pk.len() == 65 => ge_q(&pk[1..33]) || ge_q(&pk[33..65]),
        _ => false,
    }
}

// Handler functions for syscall registration
fn crypto_check_sig_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    match app.crypto_check_sig() {
        Ok(result) => app
            .push_boolean(result)
            .map_err(|e| neo_vm::VmError::InteropService {
                service: "System.Crypto.CheckSig".to_string(),
                error: e.to_string(),
            }),
        Err(e) => Err(neo_vm::VmError::InteropService {
            service: "System.Crypto.CheckSig".to_string(),
            error: e.to_string(),
        }),
    }
}

fn crypto_check_multisig_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    match app.crypto_check_multisig() {
        Ok(result) => app
            .push_boolean(result)
            .map_err(|e| neo_vm::VmError::InteropService {
                service: "System.Crypto.CheckMultisig".to_string(),
                error: e.to_string(),
            }),
        Err(e) => Err(neo_vm::VmError::InteropService {
            service: "System.Crypto.CheckMultisig".to_string(),
            error: e.to_string(),
        }),
    }
}

/// Registers crypto-related interop services
impl ApplicationEngine {
    pub(crate) fn register_crypto_interops(&mut self) -> VmResult<()> {
        self.register_host_service(
            "System.Crypto.CheckSig",
            CHECK_SIG_PRICE,
            CallFlags::NONE,
            crypto_check_sig_handler,
        )?;
        self.register_host_service(
            "System.Crypto.CheckMultisig",
            CHECK_MULTISIG_PRICE,
            CallFlags::NONE,
            crypto_check_multisig_handler,
        )?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/interop/application_engine_crypto.rs"]
mod tests;

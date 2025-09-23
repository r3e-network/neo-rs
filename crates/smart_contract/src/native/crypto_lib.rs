use crate::application_engine::ApplicationEngine;
use crate::native::{crypto_lib_bls12_381, NativeContract, NativeMethod};
use crate::{Error, Result};
use neo_core::UInt160;
use neo_core::crypto_utils::{NeoHash, Secp256k1Crypto, Secp256r1Crypto, Ed25519Crypto};

pub struct CryptoLib {
    hash: UInt160,
    methods: Vec<NativeMethod>,
}

impl CryptoLib {
    pub fn new() -> Self {
        // CryptoLib contract hash: 0x726cb6e0cd8628a1350a611384688911ab75f51b
        let hash = UInt160::from_bytes(&[
            0x72, 0x6c, 0xb6, 0xe0, 0xcd, 0x86, 0x28, 0xa1, 0x35, 0x0a, 0x61, 0x13, 0x84, 0x68,
            0x89, 0x11, 0xab, 0x75, 0xf5, 0x1b,
        ])
        .expect("Valid CryptoLib contract hash");

        let methods = vec![
            // Hash functions
            NativeMethod::safe("sha256".to_string(), 1 << 15),
            NativeMethod::safe("ripemd160".to_string(), 1 << 15),

            // ECDSA functions
            NativeMethod::safe("verifyWithECDsa".to_string(), 1 << 15),
            NativeMethod::safe("verifyWithECDsaSecp256k1".to_string(), 1 << 15),
            NativeMethod::safe("verifyWithECDsaSecp256r1".to_string(), 1 << 15),

            // Multi-signature verification
            NativeMethod::safe("checkMultisig".to_string(), 1 << 16),
            NativeMethod::safe("checkMultisigWithECDsaSecp256k1".to_string(), 1 << 16),
            NativeMethod::safe("checkMultisigWithECDsaSecp256r1".to_string(), 1 << 16),

            // BLS12-381 functions
            NativeMethod::safe("bls12381Add".to_string(), 1 << 19),
            NativeMethod::safe("bls12381Equal".to_string(), 1 << 5),
            NativeMethod::safe("bls12381Mul".to_string(), 1 << 19),
            NativeMethod::safe("bls12381Pairing".to_string(), 1 << 20),
            NativeMethod::safe("bls12381Serialize".to_string(), 1 << 16),
            NativeMethod::safe("bls12381Deserialize".to_string(), 1 << 16),
        ];

        Self { hash, methods }
    }

    /// SHA256 hash function backed by the shared cryptography crate.
    fn sha256(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        let data = args
            .get(0)
            .ok_or_else(|| {
                Error::NativeContractError("sha256 requires data argument".to_string())
            })?;

        Ok(Crypto::sha256(data))
    }

    /// RIPEMD160 hash function backed by the shared cryptography crate.
    fn ripemd160(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        let data = args
            .get(0)
            .ok_or_else(|| {
                Error::NativeContractError("ripemd160 requires data argument".to_string())
            })?;

        Ok(Crypto::ripemd160(data))
    }

    /// Verify ECDSA signature (default secp256r1)
    fn verify_with_ecdsa(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        self.verify_with_curve(
            args,
            ECCurve::secp256r1(),
            "verifyWithECDsa requires message, signature, and public key arguments",
        )
    }

    /// Verify ECDSA signature with secp256k1 curve
    fn verify_with_ecdsa_secp256k1(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        self.verify_with_curve(
            args,
            ECCurve::secp256k1(),
            "verifyWithECDsaSecp256k1 requires message, signature, and public key arguments",
        )
    }

    /// Verify ECDSA signature with secp256r1 curve (Neo's default)
    fn verify_with_ecdsa_secp256r1(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        self.verify_with_curve(
            args,
            ECCurve::secp256r1(),
            "verifyWithECDsaSecp256r1 requires message, signature, and public key arguments",
        )
    }

    fn verify_with_curve(
        &self,
        args: &[Vec<u8>],
        curve: ECCurve,
        error_msg: &str,
    ) -> Result<Vec<u8>> {
        if args.len() < 3 {
            return Err(Error::NativeContractError(error_msg.to_string()));
        }

        let message = &args[0];
        let signature = &args[1];
        let public_key = &args[2];

        if signature.len() != 64 || public_key.is_empty() {
            return Ok(vec![0]);
        }

        let is_valid = Crypto::verify_signature_with_curve(
            message,
            signature,
            public_key,
            &curve,
            HashAlgorithm::Sha256,
        );

        Ok(vec![if is_valid { 1 } else { 0 }])
    }

    /// Check multi-signature (default secp256r1)
    fn check_multisig(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        self.check_multisig_with_curve(
            args,
            ECCurve::secp256r1(),
            "checkMultisig requires message, signatures, and public keys arguments",
        )
    }

    /// Check multi-signature with secp256k1 curve
    fn check_multisig_secp256k1(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        self.check_multisig_with_curve(
            args,
            ECCurve::secp256k1(),
            "checkMultisigWithECDsaSecp256k1 requires message, signatures, and public keys arguments",
        )
    }

    /// Check multi-signature with secp256r1 curve (Neo's default)
    fn check_multisig_secp256r1(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        self.check_multisig_with_curve(
            args,
            ECCurve::secp256r1(),
            "checkMultisigWithECDsaSecp256r1 requires message, signatures, and public keys arguments",
        )
    }

    fn check_multisig_with_curve(
        &self,
        args: &[Vec<u8>],
        curve: ECCurve,
        error_msg: &str,
    ) -> Result<Vec<u8>> {
        if args.len() < 3 {
            return Err(Error::NativeContractError(error_msg.to_string()));
        }

        let message = &args[0];
        let signatures_data = &args[1];
        let public_keys_data = &args[2];

        let signatures = self.parse_signature_array(signatures_data)?;
        let public_keys = self.parse_public_key_array(public_keys_data)?;

        if signatures.len() > public_keys.len() {
            return Ok(vec![0]);
        }

        let mut sig_index = 0;
        for pubkey in &public_keys {
            if sig_index >= signatures.len() {
                break;
            }

            if Crypto::verify_signature_with_curve(
                message,
                &signatures[sig_index],
                pubkey,
                &curve,
                HashAlgorithm::Sha256,
            ) {
                sig_index += 1;
            }
        }

        Ok(vec![if sig_index == signatures.len() { 1 } else { 0 }])
    }

    /// BLS12-381 point addition
    fn bls12381_add(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() != 2 {
            return Err(Error::NativeContractError(
                "bls12381Add requires two point arguments".to_string(),
            ));
        }

        crypto_lib_bls12_381::add(&args[0], &args[1])
    }

    /// BLS12-381 equality check
    fn bls12381_equal(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() != 2 {
            return Err(Error::NativeContractError(
                "bls12381Equal requires two point arguments".to_string(),
            ));
        }

        let are_equal = crypto_lib_bls12_381::equals(&args[0], &args[1])?;
        Ok(vec![if are_equal { 1 } else { 0 }])
    }

    /// BLS12-381 scalar multiplication
    fn bls12381_mul(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() != 3 {
            return Err(Error::NativeContractError(
                "bls12381Mul requires point, scalar, and negation arguments".to_string(),
            ));
        }

        let neg = args[2].first().copied().map(|b| b != 0).ok_or_else(|| {
            Error::NativeContractError(
                "bls12381Mul negation flag must contain at least one byte".to_string(),
            )
        })?;

        crypto_lib_bls12_381::mul(&args[0], &args[1], neg)
    }

    /// BLS12-381 pairing operation
    fn bls12381_pairing(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() != 2 {
            return Err(Error::NativeContractError(
                "bls12381Pairing requires G1 and G2 point arguments".to_string(),
            ));
        }

        crypto_lib_bls12_381::pairing(&args[0], &args[1])
    }

    /// BLS12-381 point serialization
    fn bls12381_serialize(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() != 1 {
            return Err(Error::NativeContractError(
                "bls12381Serialize requires point argument".to_string(),
            ));
        }

        crypto_lib_bls12_381::serialize(&args[0])
    }

    /// BLS12-381 point deserialization
    fn bls12381_deserialize(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() != 1 {
            return Err(Error::NativeContractError(
                "bls12381Deserialize requires serialized point argument".to_string(),
            ));
        }

        crypto_lib_bls12_381::deserialize(&args[0])
    }

    /// Parse signature array from VM stack item
    fn parse_signature_array(&self, data: &[u8]) -> Result<Vec<Vec<u8>>> {
        if data.is_empty() {
            return Ok(vec![]);
        }

        let mut signatures = Vec::new();
        let mut offset = 0;

        while offset + 64 <= data.len() {
            signatures.push(data[offset..offset + 64].to_vec());
            offset += 64;
        }

        Ok(signatures)
    }

    /// Parse public key array from VM stack item
    fn parse_public_key_array(&self, data: &[u8]) -> Result<Vec<Vec<u8>>> {
        if data.is_empty() {
            return Ok(vec![]);
        }

        let mut public_keys = Vec::new();
        let mut offset = 0;

        while offset + 33 <= data.len() {
            public_keys.push(data[offset..offset + 33].to_vec());
            offset += 33;
        }

        Ok(public_keys)
    }
}

impl NativeContract for CryptoLib {
    fn hash(&self) -> UInt160 {
        self.hash
    }

    fn name(&self) -> &str {
        "CryptoLib"
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        match method {
            // Hash functions
            "sha256" => self.sha256(args),
            "ripemd160" => self.ripemd160(args),

            // ECDSA verification
            "verifyWithECDsa" => self.verify_with_ecdsa(args),
            "verifyWithECDsaSecp256k1" => self.verify_with_ecdsa_secp256k1(args),
            "verifyWithECDsaSecp256r1" => self.verify_with_ecdsa_secp256r1(args),

            // Multi-signature verification
            "checkMultisig" => self.check_multisig(args),
            "checkMultisigWithECDsaSecp256k1" => self.check_multisig_secp256k1(args),
            "checkMultisigWithECDsaSecp256r1" => self.check_multisig_secp256r1(args),

            // BLS12-381 functions
            "bls12381Add" => self.bls12381_add(args),
            "bls12381Equal" => self.bls12381_equal(args),
            "bls12381Mul" => self.bls12381_mul(args),
            "bls12381Pairing" => self.bls12381_pairing(args),
            "bls12381Serialize" => self.bls12381_serialize(args),
            "bls12381Deserialize" => self.bls12381_deserialize(args),

            _ => Err(Error::NativeContractError(format!(
                "Unknown CryptoLib method: {}",
                method
            ))),
        }
    }
}

impl Default for CryptoLib {
    fn default() -> Self {
        Self::new()
    }
}

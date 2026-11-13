//! Multi-signature witness construction for wallet contract accounts.
//!
//! Neo N3 m-of-n verification scripts push `m`, the public keys in ascending
//! order, `n`, and the `System.Crypto.CheckMultisig` syscall. At verification
//! time the script's `m` push doubles as the signature-count prefix consumed
//! by the syscall, so a valid invocation script must push exactly `m`
//! signatures, ordered so each signature lines up with its public key
//! position. This builder collects one signature slot per verification-script
//! public key and emits that canonical encoding, mirroring the C#
//! `ContractParametersContext` flow in which every signer fills only its own
//! parameter slot.
//!
//! The produced witnesses are byte-level compatible with the ledger-side
//! verifier in `neo_smart_contract::witness_verification` and the
//! `System.Crypto.CheckMultisig` syscall implementation.

use crate::account::Witness;
use crate::key_pair::KeyPair;
use crate::wallet::{WalletError, WalletResult};
use neo_smart_contract::contract::Contract;
use neo_smart_contract::helper::Helper as ContractHelper;

/// `PUSHDATA1` opcode byte (matches `neo_vm::OpCode::PUSHDATA1`).
const OP_PUSHDATA1: u8 = 0x0C;

/// Neo N3 signatures are 64-byte secp256r1 `r||s` values.
const SIGNATURE_LENGTH: usize = 64;

/// Incremental builder for m-of-n multi-signature witnesses.
#[derive(Debug, Clone)]
pub struct MultiSigWitnessBuilder {
    /// The verification (redeem) script of the multi-signature contract.
    script: Vec<u8>,
    /// Public keys in verification-script order (ascending for canonical scripts).
    public_keys: Vec<Vec<u8>>,
    /// Required signature count `m` declared by the script.
    required: usize,
    /// One signature slot per public key; `None` until that key has signed.
    signatures: Vec<Option<Vec<u8>>>,
}

impl MultiSigWitnessBuilder {
    /// Creates a builder for a canonical m-of-n multi-signature contract.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::SigningFailed`] when the contract script is not
    /// a canonical Neo multi-signature script.
    pub fn from_contract(contract: &Contract) -> WalletResult<Self> {
        let Some((required, public_keys)) =
            ContractHelper::parse_multi_sig_contract(&contract.script)
        else {
            return Err(WalletError::SigningFailed(
                "contract is not a canonical m-of-n multi-signature script".to_string(),
            ));
        };
        let signatures = vec![None; public_keys.len()];
        Ok(Self {
            script: contract.script.clone(),
            public_keys,
            required,
            signatures,
        })
    }

    /// Returns the number of signatures required by the contract (`m`).
    #[must_use]
    pub fn required(&self) -> usize {
        self.required
    }

    /// Returns the verification-script public keys in script order.
    #[must_use]
    pub fn public_keys(&self) -> &[Vec<u8>] {
        &self.public_keys
    }

    /// Returns the number of filled signature slots.
    #[must_use]
    pub fn collected(&self) -> usize {
        self.signatures.iter().flatten().count()
    }

    /// Returns `true` once at least `m` signature slots are filled.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.collected() >= self.required
    }

    /// Records a signature produced by another signer for the given public key.
    ///
    /// Returns `Ok(false)` when the public key is not part of the contract;
    /// such signers simply do not participate in this witness.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::SigningFailed`] when the signature is not a
    /// 64-byte secp256r1 value.
    pub fn add_signature(&mut self, public_key: &[u8], signature: Vec<u8>) -> WalletResult<bool> {
        if signature.len() != SIGNATURE_LENGTH {
            return Err(WalletError::SigningFailed(format!(
                "multi-signature slot expects a {}-byte secp256r1 signature, got {}",
                SIGNATURE_LENGTH,
                signature.len()
            )));
        }
        match self.slot_index(public_key) {
            Some(index) => {
                self.signatures[index] = Some(signature);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Signs the preimage with the supplied key pair, filling that key's slot.
    ///
    /// Returns `Ok(false)` when the key pair is not part of the contract.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::SigningFailed`] when signing fails.
    pub fn sign_with(&mut self, key_pair: &KeyPair, sign_data: &[u8]) -> WalletResult<bool> {
        let signature = key_pair.sign(sign_data)?;
        self.add_signature(&key_pair.compressed_public_key(), signature)
    }

    /// Builds the canonical m-of-n witness.
    ///
    /// The invocation script pushes exactly `m` signatures — the first `m`
    /// filled slots in verification-script key order — as `PUSHDATA1 0x40
    /// <signature>` chunks. The verification script is the contract script
    /// itself.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::SigningFailed`] while fewer than `m` signatures
    /// are collected; emitting a partial witness would produce an invalid
    /// transaction that the syscall's count prefix would reject anyway.
    pub fn build_witness(&self) -> WalletResult<Witness> {
        if !self.is_complete() {
            return Err(WalletError::SigningFailed(format!(
                "multi-signature witness requires {} signatures, collected {}",
                self.required,
                self.collected()
            )));
        }
        let mut invocation = Vec::with_capacity(self.required * (2 + SIGNATURE_LENGTH));
        let mut pushed = 0usize;
        for slot in &self.signatures {
            if pushed == self.required {
                break;
            }
            if let Some(signature) = slot {
                invocation.push(OP_PUSHDATA1);
                invocation.push(SIGNATURE_LENGTH as u8);
                invocation.extend_from_slice(signature);
                pushed += 1;
            }
        }
        debug_assert_eq!(
            pushed, self.required,
            "is_complete guarantees enough filled slots to push m signatures"
        );
        Ok(Witness::new_with_scripts(invocation, self.script.clone()))
    }

    /// Returns the slot index of the given public key, if it participates.
    fn slot_index(&self, public_key: &[u8]) -> Option<usize> {
        self.public_keys
            .iter()
            .position(|key| key.as_slice() == public_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_smart_contract::witness_verification::verify_witness_multi_signature;

    const MESSAGE: &[u8] = b"task-18 multisig witness round trip";

    fn generate_keys(count: usize) -> Vec<KeyPair> {
        (0..count)
            .map(|_| KeyPair::generate().expect("test key pair"))
            .collect()
    }

    /// Deterministically finds a pair whose C# ECPoint order differs from its
    /// compressed-byte order. The old verifier used the latter and misaligned
    /// signatures for precisely this valid contract shape.
    fn keys_with_distinct_orderings() -> Vec<KeyPair> {
        let candidates: Vec<KeyPair> = (1u8..=32)
            .map(|value| KeyPair::from_private_key(&[value; 32]).expect("fixed test key"))
            .collect();
        for (left_index, left) in candidates.iter().enumerate() {
            for right in candidates.iter().skip(left_index + 1) {
                let point_order = left
                    .get_public_key_point()
                    .expect("left point")
                    .cmp(&right.get_public_key_point().expect("right point"));
                let compressed_order = left
                    .compressed_public_key()
                    .cmp(&right.compressed_public_key());
                if point_order != compressed_order {
                    return vec![left.clone(), right.clone()];
                }
            }
        }
        panic!("fixed P-256 key set must contain differing orderings");
    }

    fn multisig_contract(m: usize, keys: &[KeyPair]) -> Contract {
        let points: Vec<neo_crypto::ECPoint> = keys
            .iter()
            .map(|key| key.get_public_key_point().expect("test public key"))
            .collect();
        Contract::create_multi_sig_contract(m, &points)
    }

    /// Extracts the invocation signatures in push order for verifier round trips.
    fn invocation_signatures(witness: &Witness) -> Vec<Vec<u8>> {
        witness
            .invocation_script
            .chunks(2 + SIGNATURE_LENGTH)
            .map(|chunk| chunk[2..].to_vec())
            .collect()
    }

    #[test]
    fn witness_round_trips_against_production_verifier() {
        let keys = generate_keys(3);
        let contract = multisig_contract(2, &keys);
        let account = contract.script_hash();

        let mut builder =
            MultiSigWitnessBuilder::from_contract(&contract).expect("canonical m-of-n");
        assert_eq!(builder.required(), 2);
        // Sign out of order to prove slots are keyed by public key, not insertion.
        assert!(builder.sign_with(&keys[2], MESSAGE).expect("in-set signer"));
        assert!(builder.sign_with(&keys[1], MESSAGE).expect("in-set signer"));
        assert!(builder.is_complete());
        let witness = builder.build_witness().expect("complete witness");

        // Byte format: exactly m PUSHDATA1 <0x40> <64-byte signature> chunks.
        assert_eq!(witness.invocation_script.len(), 2 * (2 + SIGNATURE_LENGTH));
        for chunk in witness.invocation_script.chunks(2 + SIGNATURE_LENGTH) {
            assert_eq!(chunk[0], OP_PUSHDATA1);
            assert_eq!(chunk[1], SIGNATURE_LENGTH as u8);
        }

        let signatures = invocation_signatures(&witness);
        let public_keys = builder.public_keys().to_vec();
        assert!(
            verify_witness_multi_signature(MESSAGE, &account, 2, &public_keys, &signatures)
                .expect("verifier must not fault")
        );
        assert!(
            !verify_witness_multi_signature(b"tampered", &account, 2, &public_keys, &signatures)
                .expect("verifier must not fault")
        );
    }

    #[test]
    fn externally_collected_signature_fills_its_key_slot() {
        let keys = keys_with_distinct_orderings();
        let contract = multisig_contract(2, &keys);
        let account = contract.script_hash();

        let mut builder =
            MultiSigWitnessBuilder::from_contract(&contract).expect("canonical m-of-n");
        // Simulate a second signer exporting only the raw signature bytes.
        let remote_signature = keys[1].sign(MESSAGE).expect("remote signing");
        assert!(
            builder
                .add_signature(&keys[1].compressed_public_key(), remote_signature)
                .expect("well-formed signature")
        );
        assert_eq!(builder.collected(), 1);
        assert!(builder.sign_with(&keys[0], MESSAGE).expect("local signer"));
        assert!(builder.is_complete());

        let witness = builder.build_witness().expect("complete witness");
        let signatures = invocation_signatures(&witness);
        let public_keys = builder.public_keys().to_vec();
        assert!(
            verify_witness_multi_signature(MESSAGE, &account, 2, &public_keys, &signatures)
                .expect("verifier must not fault")
        );
    }

    #[test]
    fn build_fails_closed_until_m_signatures_are_collected() {
        let keys = generate_keys(3);
        let contract = multisig_contract(2, &keys);
        let mut builder =
            MultiSigWitnessBuilder::from_contract(&contract).expect("canonical m-of-n");
        assert!(builder.sign_with(&keys[0], MESSAGE).expect("in-set signer"));
        assert!(!builder.is_complete());
        assert!(builder.build_witness().is_err());
    }

    #[test]
    fn key_outside_the_contract_is_ignored() {
        let keys = generate_keys(2);
        let contract = multisig_contract(2, &keys);
        let mut builder =
            MultiSigWitnessBuilder::from_contract(&contract).expect("canonical m-of-n");
        let foreign = KeyPair::generate().expect("foreign key pair");
        assert!(
            !builder
                .sign_with(&foreign, MESSAGE)
                .expect("out-of-set signer")
        );
        assert_eq!(builder.collected(), 0);
        let foreign_signature = foreign.sign(MESSAGE).expect("foreign signature");
        assert!(
            !builder
                .add_signature(&foreign.compressed_public_key(), foreign_signature)
                .expect("well-formed signature")
        );
        assert_eq!(builder.collected(), 0);
    }

    #[test]
    fn malformed_signature_length_is_rejected() {
        let keys = generate_keys(2);
        let contract = multisig_contract(1, &keys);
        let mut builder =
            MultiSigWitnessBuilder::from_contract(&contract).expect("canonical m-of-n");
        assert!(
            builder
                .add_signature(&keys[0].compressed_public_key(), vec![0u8; 63])
                .is_err()
        );
    }

    #[test]
    fn non_multisig_contract_is_rejected() {
        let key = KeyPair::generate().expect("test key pair");
        let single =
            Contract::create_signature_contract(key.get_public_key_point().expect("point"));
        assert!(MultiSigWitnessBuilder::from_contract(&single).is_err());
    }
}

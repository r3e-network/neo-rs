//! Witness construction for wallet contract accounts.
//!
//! Two layers live here:
//!
//! * [`MultiSigWitnessBuilder`] owns the m-of-n specifics: parsing a canonical
//!   `CheckMultisig` verification script, filling one slot per script public key
//!   and emitting the canonical `m`-signature invocation script.
//! * [`ContractSignatureContext`] is the crate-level entry point used by the RPC
//!   wallet layers. It resolves whichever script shape an account holds
//!   (`CheckSig`, `CheckMultisig`, or an opaque deployed/stub script), collects
//!   signatures in verification-script order and builds the final witness.
//!
//! Parsing of the scripts themselves stays in `neo_smart_contract::helper`.
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
use crate::signing::witness_from_signature;
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
        Self::from_script(&contract.script)
    }

    /// Creates a builder by parsing a multi-signature verification script.
    ///
    /// The threshold and key order come from
    /// [`ContractHelper::parse_multi_sig_contract`], which accepts the canonical
    /// `PUSH1..PUSH16` (`0x11..=0x20`) and `PUSHINT8`/`PUSHINT16` encodings.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::SigningFailed`] when `script` is not a canonical
    /// Neo multi-signature script.
    pub fn from_script(script: &[u8]) -> WalletResult<Self> {
        let Some((required, public_keys)) = ContractHelper::parse_multi_sig_contract(script) else {
            return Err(WalletError::SigningFailed(
                "script is not a canonical m-of-n multi-signature script".to_string(),
            ));
        };
        let signatures = vec![None; public_keys.len()];
        Ok(Self {
            script: script.to_vec(),
            public_keys,
            required,
            signatures,
        })
    }

    /// Returns the verification (redeem) script of the contract.
    #[must_use]
    pub fn script(&self) -> &[u8] {
        &self.script
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

    /// Returns `true` when `public_key` is one of the contract's keys.
    #[must_use]
    pub fn contains_key(&self, public_key: &[u8]) -> bool {
        self.slot_index(public_key).is_some()
    }

    /// Returns the signature collected for `public_key`, if that key has signed.
    #[must_use]
    pub fn signature_for(&self, public_key: &[u8]) -> Option<&[u8]> {
        self.slot_index(public_key)
            .and_then(|index| self.signatures[index].as_deref())
    }

    /// Returns the collected `(public_key, signature)` pairs in verification-script order.
    #[must_use]
    pub fn collected_signatures(&self) -> Vec<(&[u8], &[u8])> {
        self.public_keys
            .iter()
            .zip(&self.signatures)
            .filter_map(|(key, signature)| {
                signature
                    .as_deref()
                    .map(|signature| (key.as_slice(), signature))
            })
            .collect()
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
                "multi-signature slot expects a {SIGNATURE_LENGTH}-byte secp256r1 signature, got {}",
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

/// Outcome of offering a signature to a [`ContractSignatureContext`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureAddOutcome {
    /// The signature filled the slot belonging to its public key.
    Added,
    /// The slot already held a signature; the stored value is unchanged.
    Duplicate,
    /// The public key does not take part in this contract's verification script.
    UnknownKey,
}

impl SignatureAddOutcome {
    /// Returns `true` when the signature was newly recorded.
    #[must_use]
    pub const fn is_added(self) -> bool {
        matches!(self, Self::Added)
    }
}

/// Signature collection and witness construction for one wallet contract.
///
/// A wallet account holds a single verification script, and each script shape
/// needs a different invocation script:
///
/// * `CheckSig` (`Helper::is_signature_contract`): one 64-byte signature pushed
///   as `PUSHDATA1 0x40 <signature>`.
/// * `CheckMultisig`: `m` signatures pushed in verification-script key order,
///   delegated to [`MultiSigWitnessBuilder`] so signature alignment is decided
///   by the script rather than by map iteration order.
/// * Anything else (a deployed contract or a hash-only stub): no key-based
///   verification is recognisable, so only the declared parameter count is
///   tracked and the invocation script carries whatever was collected.
///
/// The RPC wallet server and the RPC client keep their transport, fee and
/// presentation code and delegate signature collection and witness assembly here.
#[derive(Debug, Clone)]
pub struct ContractSignatureContext {
    /// The verification (redeem) script the witness is paired with.
    script: Vec<u8>,
    /// Script-shape specific signature slots.
    kind: SigningKind,
}

/// Script-shape specific signature collection state.
#[derive(Debug, Clone)]
enum SigningKind {
    /// Standard `CheckSig` contract with exactly one key slot.
    Signature(SignatureSlot),
    /// Canonical m-of-n `CheckMultisig` contract.
    MultiSignature(MultiSigWitnessBuilder),
    /// Script without recognisable key-based verification.
    Parameters(ParameterSlots),
}

/// Signature slot of a single-signature contract.
#[derive(Debug, Clone)]
struct SignatureSlot {
    /// Public key embedded in the verification script.
    public_key: Vec<u8>,
    /// Signature for that key, once available.
    signature: Option<Vec<u8>>,
}

/// Signature slots of a script whose parameters are only known by count.
#[derive(Debug, Clone)]
struct ParameterSlots {
    /// Number of signatures the contract requires.
    required: usize,
    /// Collected `(public_key, signature)` pairs in arrival order.
    signatures: Vec<(Vec<u8>, Vec<u8>)>,
}

impl ContractSignatureContext {
    /// Creates a context for a verification script.
    ///
    /// `parameter_count` is the fallback signature requirement, used only when
    /// `script` is neither a `CheckSig` nor a `CheckMultisig` contract (for
    /// example a hash-only stub resolved through RPC).
    #[must_use]
    pub fn new(script: &[u8], parameter_count: usize) -> Self {
        let kind = if let Ok(builder) = MultiSigWitnessBuilder::from_script(script) {
            SigningKind::MultiSignature(builder)
        } else if ContractHelper::is_signature_contract(script) {
            // `is_signature_contract` guarantees `PUSHDATA1 0x21 <33-byte key>`.
            SigningKind::Signature(SignatureSlot {
                public_key: script[2..35].to_vec(),
                signature: None,
            })
        } else {
            SigningKind::Parameters(ParameterSlots {
                required: parameter_count,
                signatures: Vec::new(),
            })
        };
        Self {
            script: script.to_vec(),
            kind,
        }
    }

    /// Creates a context from a wallet contract, using its parameter list as the
    /// fallback requirement for unrecognised scripts.
    #[must_use]
    pub fn from_contract(contract: &Contract) -> Self {
        Self::new(&contract.script, contract.parameter_list.len())
    }

    /// Returns the verification (redeem) script of the contract.
    #[must_use]
    pub fn script(&self) -> &[u8] {
        &self.script
    }

    /// Returns the number of signatures the contract requires.
    #[must_use]
    pub fn required(&self) -> usize {
        match &self.kind {
            SigningKind::Signature(_) => 1,
            SigningKind::MultiSignature(builder) => builder.required(),
            SigningKind::Parameters(slots) => slots.required,
        }
    }

    /// Returns the number of signatures collected so far.
    #[must_use]
    pub fn collected(&self) -> usize {
        match &self.kind {
            SigningKind::Signature(slot) => usize::from(slot.signature.is_some()),
            SigningKind::MultiSignature(builder) => builder.collected(),
            SigningKind::Parameters(slots) => slots.signatures.len(),
        }
    }

    /// Returns `true` once the required number of signatures is collected.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        match &self.kind {
            SigningKind::Signature(slot) => slot.signature.is_some(),
            SigningKind::MultiSignature(builder) => builder.is_complete(),
            SigningKind::Parameters(slots) => slots.signatures.len() >= slots.required,
        }
    }

    /// Returns the script public key of a `CheckSig` contract, if this is one.
    #[must_use]
    pub fn signature_public_key(&self) -> Option<&[u8]> {
        match &self.kind {
            SigningKind::Signature(slot) => Some(&slot.public_key),
            _ => None,
        }
    }

    /// Returns the public keys the verification script consumes, in script order.
    #[must_use]
    pub fn public_keys(&self) -> Vec<&[u8]> {
        match &self.kind {
            SigningKind::Signature(slot) => vec![slot.public_key.as_slice()],
            SigningKind::MultiSignature(builder) => {
                builder.public_keys().iter().map(Vec::as_slice).collect()
            }
            SigningKind::Parameters(slots) => slots
                .signatures
                .iter()
                .map(|(key, _)| key.as_slice())
                .collect(),
        }
    }

    /// Returns the `(public_key, signature)` pairs collected so far, in
    /// verification-script order (arrival order for unrecognised scripts).
    #[must_use]
    pub fn collected_signatures(&self) -> Vec<(&[u8], &[u8])> {
        match &self.kind {
            SigningKind::Signature(slot) => slot
                .signature
                .as_deref()
                .map(|signature| vec![(slot.public_key.as_slice(), signature)])
                .unwrap_or_default(),
            SigningKind::MultiSignature(builder) => builder.collected_signatures(),
            SigningKind::Parameters(slots) => slots
                .signatures
                .iter()
                .map(|(key, signature)| (key.as_slice(), signature.as_slice()))
                .collect(),
        }
    }

    /// Records a signature produced by another signer for the given public key.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::SigningFailed`] when the signature is not a 64-byte
    /// secp256r1 value.
    pub fn add_signature(
        &mut self,
        public_key: &[u8],
        signature: Vec<u8>,
    ) -> WalletResult<SignatureAddOutcome> {
        if signature.len() != SIGNATURE_LENGTH {
            return Err(WalletError::SigningFailed(format!(
                "signature slot expects a {SIGNATURE_LENGTH}-byte secp256r1 signature, got {}",
                signature.len()
            )));
        }
        match &mut self.kind {
            SigningKind::Signature(slot) => {
                if slot.public_key.as_slice() != public_key {
                    return Ok(SignatureAddOutcome::UnknownKey);
                }
                if slot.signature.is_some() {
                    return Ok(SignatureAddOutcome::Duplicate);
                }
                slot.signature = Some(signature);
                Ok(SignatureAddOutcome::Added)
            }
            SigningKind::MultiSignature(builder) => {
                if !builder.contains_key(public_key) {
                    return Ok(SignatureAddOutcome::UnknownKey);
                }
                if builder.signature_for(public_key).is_some() {
                    return Ok(SignatureAddOutcome::Duplicate);
                }
                let _ = builder.add_signature(public_key, signature)?;
                Ok(SignatureAddOutcome::Added)
            }
            SigningKind::Parameters(slots) => {
                if let Some((_, existing)) = slots
                    .signatures
                    .iter_mut()
                    .find(|(key, _)| key.as_slice() == public_key)
                {
                    *existing = signature;
                    return Ok(SignatureAddOutcome::Duplicate);
                }
                slots.signatures.push((public_key.to_vec(), signature));
                Ok(SignatureAddOutcome::Added)
            }
        }
    }

    /// Signs the preimage with the supplied key pair, filling that key's slot.
    ///
    /// Returns [`SignatureAddOutcome::UnknownKey`] when the key pair is not part
    /// of the contract.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::SigningFailed`] when signing fails.
    pub fn sign_with(
        &mut self,
        key_pair: &KeyPair,
        sign_data: &[u8],
    ) -> WalletResult<SignatureAddOutcome> {
        let signature = key_pair.sign(sign_data)?;
        self.add_signature(&key_pair.compressed_public_key(), signature)
    }

    /// Builds the witness (invocation script plus verification script).
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::SigningFailed`] while fewer than the required
    /// number of signatures are collected; emitting a partial witness would
    /// produce a transaction the verification script rejects.
    pub fn build_witness(&self) -> WalletResult<Witness> {
        match &self.kind {
            SigningKind::Signature(slot) => {
                let Some(signature) = slot.signature.as_deref() else {
                    return Err(WalletError::SigningFailed(
                        "signature contract witness requires its single signature".to_string(),
                    ));
                };
                witness_from_signature(signature, &self.script)
            }
            SigningKind::MultiSignature(builder) => builder.build_witness(),
            SigningKind::Parameters(slots) => {
                if slots.signatures.len() < slots.required {
                    return Err(WalletError::SigningFailed(format!(
                        "contract witness requires {} signatures, collected {}",
                        slots.required,
                        slots.signatures.len()
                    )));
                }
                let mut invocation = Vec::with_capacity(slots.required * (2 + SIGNATURE_LENGTH));
                for (_, signature) in slots.signatures.iter().take(slots.required) {
                    invocation.push(OP_PUSHDATA1);
                    invocation.push(SIGNATURE_LENGTH as u8);
                    invocation.extend_from_slice(signature);
                }
                Ok(Witness::new_with_scripts(invocation, self.script.clone()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_primitives::{ContractParameterType, UInt160};
    use neo_smart_contract::witness_verification::{
        verify_witness_multi_signature, verify_witness_signature,
    };

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
            .invocation_script()
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
        assert_eq!(
            witness.invocation_script().len(),
            2 * (2 + SIGNATURE_LENGTH)
        );
        for chunk in witness.invocation_script().chunks(2 + SIGNATURE_LENGTH) {
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

    #[test]
    fn context_reads_threshold_from_canonical_push_opcodes() {
        let keys = generate_keys(3);
        let contract = multisig_contract(2, &keys);
        let account = contract.script_hash();
        // The threshold push is PUSH2 (0x12), not the legacy 0x51..=0x60 range.
        assert_eq!(contract.script[0], neo_vm::OpCode::PUSH2.byte());

        let mut context = ContractSignatureContext::from_contract(&contract);
        assert_eq!(context.required(), 2, "2-of-3 must not fall back to one");
        assert_eq!(context.collected(), 0);
        assert!(!context.is_complete());
        assert!(context.build_witness().is_err());

        // Sign out of order: slots are keyed by public key, not arrival order.
        assert_eq!(
            context.sign_with(&keys[2], MESSAGE).expect("in-set signer"),
            SignatureAddOutcome::Added
        );
        assert_eq!(
            context.sign_with(&keys[1], MESSAGE).expect("in-set signer"),
            SignatureAddOutcome::Added
        );
        assert_eq!(context.collected(), 2);
        assert!(context.is_complete());

        let witness = context.build_witness().expect("complete witness");
        assert_eq!(
            witness.invocation_script().len(),
            2 * (2 + SIGNATURE_LENGTH)
        );
        let signatures = invocation_signatures(&witness);
        let public_keys: Vec<Vec<u8>> = context
            .public_keys()
            .iter()
            .map(|key| key.to_vec())
            .collect();
        assert!(
            verify_witness_multi_signature(MESSAGE, &account, 2, &public_keys, &signatures)
                .expect("verifier must not fault")
        );
    }

    #[test]
    fn context_emits_signatures_in_verification_script_order() {
        let keys = keys_with_distinct_orderings();
        let contract = multisig_contract(2, &keys);
        let account = contract.script_hash();
        let (_, script_keys) =
            ContractHelper::parse_multi_sig_contract(&contract.script).expect("canonical script");

        let mut context = ContractSignatureContext::from_contract(&contract);
        // Add the second script key first; the emitted order must still follow the script.
        assert!(
            context
                .add_signature(
                    &keys[1].compressed_public_key(),
                    keys[1].sign(MESSAGE).expect("remote signing")
                )
                .expect("well-formed signature")
                .is_added()
        );
        assert!(
            context
                .sign_with(&keys[0], MESSAGE)
                .expect("local signer")
                .is_added()
        );

        let public_keys: Vec<&[u8]> = context.public_keys();
        let expected: Vec<&[u8]> = script_keys.iter().map(Vec::as_slice).collect();
        assert_eq!(public_keys, expected);

        let witness = context.build_witness().expect("complete witness");
        let emitted = invocation_signatures(&witness);
        let owned_keys: Vec<Vec<u8>> = public_keys.iter().map(|key| key.to_vec()).collect();
        assert!(
            verify_witness_multi_signature(MESSAGE, &account, 2, &owned_keys, &emitted)
                .expect("verifier must not fault")
        );
    }

    #[test]
    fn context_handles_single_signature_contracts() {
        let key = KeyPair::from_private_key(&[7u8; 32]).expect("test key pair");
        let contract =
            Contract::create_signature_contract(key.get_public_key_point().expect("point"));
        let account = contract.script_hash();

        let mut context = ContractSignatureContext::from_contract(&contract);
        assert_eq!(context.required(), 1);
        let script_public_key = key.compressed_public_key();
        assert_eq!(
            context.signature_public_key(),
            Some(script_public_key.as_slice())
        );
        assert!(context.build_witness().is_err());
        assert!(
            context
                .sign_with(&key, MESSAGE)
                .expect("local signer")
                .is_added()
        );
        assert!(context.is_complete());

        let witness = context.build_witness().expect("complete witness");
        assert_eq!(witness.invocation_script().len(), 2 + SIGNATURE_LENGTH);
        assert_eq!(witness.verification_script(), contract.script);
        assert!(
            verify_witness_signature(&witness, MESSAGE, &account).expect("verifier must not fault")
        );
    }

    #[test]
    fn context_reports_duplicates_and_foreign_keys() {
        let keys = generate_keys(2);
        let contract = multisig_contract(2, &keys);
        let mut context = ContractSignatureContext::from_contract(&contract);

        assert_eq!(
            context.sign_with(&keys[0], MESSAGE).expect("in-set signer"),
            SignatureAddOutcome::Added
        );
        assert_eq!(
            context.sign_with(&keys[0], MESSAGE).expect("in-set signer"),
            SignatureAddOutcome::Duplicate
        );
        let foreign = KeyPair::generate().expect("foreign key pair");
        assert_eq!(
            context
                .sign_with(&foreign, MESSAGE)
                .expect("out-of-set signer"),
            SignatureAddOutcome::UnknownKey
        );
        assert_eq!(context.collected(), 1);
        assert!(
            context
                .add_signature(&keys[0].compressed_public_key(), vec![0u8; 63])
                .is_err()
        );
    }

    #[test]
    fn context_tracks_scripts_without_recognisable_keys() {
        // A hash-only stub carries no verification script and no parameters.
        let stub = ContractSignatureContext::new(&[], 0);
        assert_eq!(stub.required(), 0);
        assert!(stub.is_complete());
        assert!(
            stub.build_witness()
                .expect("empty witness")
                .invocation_script()
                .is_empty()
        );

        // A deployed contract resolved through RPC only knows its parameter count.
        let contract =
            Contract::create_with_hash(UInt160::zero(), vec![ContractParameterType::Signature]);
        let mut deployed = ContractSignatureContext::from_contract(&contract);
        assert_eq!(deployed.required(), 1);
        assert!(!deployed.is_complete());
        assert_eq!(
            deployed
                .add_signature(&[0x02; 33], vec![0xAB; SIGNATURE_LENGTH])
                .expect("opaque slot"),
            SignatureAddOutcome::Added
        );
        assert!(deployed.is_complete());
        let witness = deployed.build_witness().expect("complete witness");
        assert_eq!(witness.invocation_script().len(), 2 + SIGNATURE_LENGTH);
    }
}

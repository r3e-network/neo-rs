use super::*;
use crate::key_pair::KeyPair;
use neo_primitives::WitnessScope;
use neo_protocol::payloads::Signer;
use neo_smart_contract::helper::Helper as ContractHelper;

const NETWORK: u32 = 0x334F_454E;

/// Deterministic chain stub: fixed height, balance and fee estimate.
struct StubChain {
    height: u32,
    balance: BigInt,
    network_fee: i64,
}

#[async_trait::async_trait]
impl TransactionChainQuery for StubChain {
    type Error = String;

    async fn block_height(&self) -> Result<u32, Self::Error> {
        Ok(self.height)
    }

    async fn gas_balance(&self, _account: &UInt160) -> Result<BigInt, Self::Error> {
        Ok(self.balance.clone())
    }

    async fn network_fee(&self, _transaction: &Transaction) -> Result<i64, Self::Error> {
        Ok(self.network_fee)
    }
}

/// Signer stub that signs with a fixed key set.
struct StubSigner {
    keys: Vec<KeyPair>,
}

#[async_trait::async_trait]
impl TransactionSigner for StubSigner {
    async fn sign(
        &self,
        request: &SignerRequest<'_>,
    ) -> Result<Vec<AccountSignature>, TransactionAuthorizationError> {
        // A real signer only signs for keys that take part in the contract.
        let context = ContractSignatureContext::from_contract(request.contract);
        let participants = context.public_keys();
        let mut signatures = Vec::new();
        for key in &self.keys {
            let public_key = key.compressed_public_key();
            if !participants.contains(&public_key.as_slice()) {
                continue;
            }
            let signature = key
                .sign(&request.sign_data)
                .map_err(|err| TransactionAuthorizationError::SigningFailed(err.to_string()))?;
            signatures.push(AccountSignature::new(public_key, signature));
        }
        Ok(signatures)
    }
}

fn signature_contract(key: &KeyPair) -> Contract {
    Contract::create_signature_contract(key.get_public_key_point().expect("point"))
}

fn transaction_for(accounts: &[UInt160]) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(7);
    tx.set_script(vec![0x40]); // RET
    tx.set_valid_until_block(100);
    tx.set_signers(
        accounts
            .iter()
            .map(|account| Signer::new(*account, WitnessScope::GLOBAL))
            .collect(),
    );
    tx
}

#[test]
fn registration_rejects_unknown_signers_and_reports_progress() {
    let key = KeyPair::from_private_key(&[3u8; 32]).expect("key");
    let contract = signature_contract(&key);
    let mut authorizer = TransactionAuthorizer::new(
        transaction_for(&[contract.script_hash()]),
        NETWORK,
        AuthorizationLimits::new(10),
    );

    assert_eq!(
        authorizer.register_witness_only(&UInt160::zero()),
        Err(TransactionAuthorizationError::UnknownSigner(UInt160::zero()))
    );
    assert_eq!(authorizer.unresolved_signers().len(), 1);
    authorizer.register_contract(&contract).expect("signer");
    assert!(authorizer.unresolved_signers().is_empty());
    assert!(!authorizer.is_complete());
    assert_eq!(authorizer.pending_signers(), vec![contract.script_hash()]);
}

#[test]
fn witness_only_stub_is_immediately_complete() {
    let key = KeyPair::from_private_key(&[4u8; 32]).expect("key");
    let signer = signature_contract(&key).script_hash();
    let stub = UInt160::zero();
    let mut authorizer = TransactionAuthorizer::new(
        transaction_for(&[signer, stub]),
        NETWORK,
        AuthorizationLimits::new(10),
    );
    authorizer.register_witness_only(&stub).expect("stub");
    assert_eq!(authorizer.unresolved_signers(), vec![signer]);
    assert!(!authorizer.is_complete());
}

#[tokio::test]
async fn collect_signatures_fills_every_pending_slot_in_signer_order() {
    let keys: Vec<KeyPair> = (1u8..=3)
        .map(|seed| KeyPair::from_private_key(&[seed; 32]).expect("key"))
        .collect();
    let points: Vec<neo_crypto::ECPoint> = keys
        .iter()
        .map(|key| key.get_public_key_point().expect("point"))
        .collect();
    let multi = Contract::create_multi_sig_contract(2, &points);
    let single = signature_contract(&keys[0]);
    let mut authorizer = TransactionAuthorizer::new(
        transaction_for(&[single.script_hash(), multi.script_hash()]),
        NETWORK,
        AuthorizationLimits::new(10),
    );
    // Register the second signer first: witness order must still follow the
    // transaction signer list.
    authorizer.register_contract(&multi).expect("multisig");
    authorizer.register_contract(&single).expect("single");

    let signer = StubSigner { keys: keys.clone() };
    authorizer
        .collect_signatures(&signer)
        .await
        .expect("signatures");
    assert!(authorizer.is_complete());

    let witnesses = authorizer.witnesses().expect("witnesses");
    assert_eq!(witnesses.len(), 2);
    assert_eq!(witnesses[0].verification_script(), single.script.as_slice());
    assert_eq!(witnesses[0].invocation_script().len(), 2 + SIGNATURE_LENGTH);
    assert_eq!(witnesses[1].verification_script(), multi.script.as_slice());
    assert_eq!(
        witnesses[1].invocation_script().len(),
        2 * (2 + SIGNATURE_LENGTH)
    );
}

#[test]
fn duplicate_and_unknown_signatures_fail_closed() {
    let key = KeyPair::from_private_key(&[5u8; 32]).expect("key");
    let contract = signature_contract(&key);
    let account = contract.script_hash();
    let mut authorizer = TransactionAuthorizer::new(
        transaction_for(&[account]),
        NETWORK,
        AuthorizationLimits::new(10),
    );
    authorizer.register_contract(&contract).expect("signer");
    let sign_data = authorizer
        .signer_request(&account)
        .expect("request")
        .sign_data;
    let signature = AccountSignature::new(
        key.compressed_public_key(),
        key.sign(&sign_data).expect("sign"),
    );
    authorizer
        .add_signature(&account, signature.clone())
        .expect("first signature");
    assert!(authorizer.is_complete());

    let duplicate = authorizer
        .add_signature(&account, signature)
        .expect_err("second round must not overwrite");
    assert!(duplicate.to_string().contains("AddSignature failed"));

    let stranger = KeyPair::from_private_key(&[9u8; 32]).expect("key");
    let unknown = AccountSignature::new(
        stranger.compressed_public_key(),
        stranger.sign(&sign_data).expect("sign"),
    );
    assert_eq!(
        authorizer.add_signature(&account, unknown),
        Err(TransactionAuthorizationError::UnknownSigningKey(account))
    );
}

#[tokio::test]
async fn fee_estimate_and_funding_rules_use_the_chain_port() {
    let key = KeyPair::from_private_key(&[6u8; 32]).expect("key");
    let contract = signature_contract(&key);
    let mut authorizer = TransactionAuthorizer::new(
        transaction_for(&[contract.script_hash()]),
        NETWORK,
        AuthorizationLimits::new(10).with_max_network_fee(1_000),
    );
    authorizer.register_contract(&contract).expect("signer");

    let chain = StubChain {
        height: 90,
        balance: BigInt::from(50),
        network_fee: 100,
    };
    assert_eq!(authorizer.estimate_network_fee(&chain).await, Ok(100));
    assert!(authorizer.tx().witnesses().is_empty());
    authorizer.validate_structure().expect("structure");
    authorizer
        .ensure_valid_until_window(&chain)
        .await
        .expect("window");

    let error = authorizer
        .ensure_sufficient_funds(&chain)
        .await
        .expect_err("balance below fee");
    assert!(matches!(
        error,
        TransactionAuthorizationError::InsufficientFunds { .. }
    ));

    authorizer.set_network_fee(1_001);
    assert!(matches!(
        authorizer.validate_structure(),
        Err(TransactionAuthorizationError::FeeLimitExceeded { .. })
    ));
}

#[test]
fn valid_until_window_is_enforced() {
    let key = KeyPair::from_private_key(&[7u8; 32]).expect("key");
    let contract = signature_contract(&key);
    let authorizer = TransactionAuthorizer::new(
        transaction_for(&[contract.script_hash()]),
        NETWORK,
        AuthorizationLimits::new(5),
    );
    authorizer.validate_valid_until(95).expect("inside window");
    assert!(authorizer.validate_valid_until(100).is_err());
    assert!(authorizer.validate_valid_until(89).is_err());
}

#[test]
fn single_signature_public_key_comes_from_the_script() {
    let key = KeyPair::from_private_key(&[8u8; 32]).expect("key");
    let contract = signature_contract(&key);
    let account = contract.script_hash();
    let mut authorizer = TransactionAuthorizer::new(
        transaction_for(&[account]),
        NETWORK,
        AuthorizationLimits::new(10),
    );
    authorizer.register_contract(&contract).expect("signer");
    let expected = ContractHelper::is_signature_contract(&contract.script)
        .then(|| contract.script[2..35].to_vec());
    assert_eq!(authorizer.single_signature_public_key(&account), expected);
    assert_eq!(
        authorizer.single_signature_public_key(&account),
        Some(key.compressed_public_key())
    );
}

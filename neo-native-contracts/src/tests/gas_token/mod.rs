//! # neo-native-contracts::tests::gas_token
//!
//! Test module grouping Native GAS token state, accounting, and transfer
//! behavior. coverage for neo-native-contracts.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-native-contracts; it may assemble
//! fixtures but must not introduce production behavior.
//!
//! ## Contents
//!
//! - Test modules and fixtures: grouped coverage for the surrounding domain.

use super::*;
use neo_primitives::{CallFlags, ContractParameterType};
use neo_serialization::BinarySerializer;
use neo_vm_rs::ExecutionEngineLimits;

#[test]
fn gas_transfer_from_calling_contract_uses_contract_as_witness() {
    use neo_execution::native_contract::build_native_contract_state;
    use neo_payloads::{Signer, Transaction};
    use neo_primitives::{TriggerType, WitnessScope};
    use std::sync::Arc;

    crate::install();

    let cache = DataCache::new(false);
    let contract_account = UInt160::from_bytes(&[0x48; 20]).unwrap();
    let recipient = UInt160::from_bytes(&[0x23; 20]).unwrap();
    GasToken::new()
        .write_gas_account(&cache, &contract_account, &BigInt::from(1_000))
        .unwrap();
    crate::test_support::deploy_native(
        &cache,
        &build_native_contract_state(&GasToken, &ProtocolSettings::default(), 0),
    );

    let mut tx = Transaction::new();
    tx.set_signers(vec![Signer::new(
        UInt160::from_bytes(&[0x99; 20]).unwrap(),
        WitnessScope::NONE,
    )]);

    let snapshot = Arc::new(cache);
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(Arc::new(tx)),
        Arc::clone(&snapshot),
        None,
        ProtocolSettings::default(),
        10_000_000,
        None,
    )
    .expect("engine builds");

    engine
        .load_script(vec![neo_vm_rs::OpCode::RET.byte()], CallFlags::ALL, None)
        .expect("load calling contract context");
    let entry = engine.current_context().cloned().expect("entry context");
    let state = entry.get_state_with_factory::<neo_execution::ExecutionContextState, _>(
        neo_execution::ExecutionContextState::new,
    );
    state.lock().script_hash = Some(contract_account);

    engine
        .call_contract_dynamic(
            &GasToken::script_hash(),
            "transfer",
            CallFlags::ALL,
            vec![
                StackItem::from_byte_string(contract_account.to_bytes()),
                StackItem::from_byte_string(recipient.to_bytes()),
                StackItem::from_int(BigInt::from(375)),
                StackItem::null(),
            ],
        )
        .expect("load GAS transfer");

    assert_eq!(engine.execute_allow_fault(), neo_vm_rs::VmState::HALT);
    assert_eq!(
        GasToken::new()
            .read_gas_account(&snapshot, &contract_account)
            .unwrap(),
        Some(BigInt::from(625))
    );
    assert_eq!(
        GasToken::new()
            .read_gas_account(&snapshot, &recipient)
            .unwrap(),
        Some(BigInt::from(375))
    );
}

#[test]
fn native_contract_surface() {
    let c = GasToken::new();
    let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        ["symbol", "decimals", "totalSupply", "balanceOf", "transfer"]
    );
    // Metadata getters are zero-fee; the state reads are ReadStates getters.
    let symbol = c.methods().iter().find(|m| m.name == "symbol").unwrap();
    assert!(symbol.safe && symbol.cpu_fee == 0 && symbol.required_call_flags == 0);
    let balance = c.methods().iter().find(|m| m.name == "balanceOf").unwrap();
    assert_eq!(balance.required_call_flags, CallFlags::READ_STATES.bits());
    // transfer: not safe, States|AllowCall|AllowNotify, StorageFee 50,
    // (Hash160, Hash160, Integer, Any) -> Boolean.
    let transfer = c.methods().iter().find(|m| m.name == "transfer").unwrap();
    assert!(!transfer.safe);
    assert_eq!(transfer.cpu_fee, 1 << 17);
    assert_eq!(transfer.storage_fee, 50);
    assert_eq!(
        transfer.required_call_flags,
        (CallFlags::STATES | CallFlags::ALLOW_CALL | CallFlags::ALLOW_NOTIFY).bits()
    );
    assert_eq!(
        transfer.parameters,
        vec![
            ContractParameterType::Hash160,
            ContractParameterType::Hash160,
            ContractParameterType::Integer,
            ContractParameterType::Any
        ]
    );
    assert_eq!(transfer.return_type, ContractParameterType::Boolean);
}

#[test]
fn compute_gas_transfer_matches_csharp_balance_arithmetic() {
    let amt = BigInt::from(100);

    // amount == 0 -> no movement (succeeds, emits) regardless of balances.
    assert_eq!(
        GasToken::compute_gas_transfer(None, None, false, &BigInt::zero()),
        GasTransferOutcome::NoMovement
    );

    // amount > 0, from has no account -> insufficient.
    assert_eq!(
        GasToken::compute_gas_transfer(None, None, false, &amt),
        GasTransferOutcome::InsufficientBalance
    );
    // amount > 0, from underfunded -> insufficient.
    assert_eq!(
        GasToken::compute_gas_transfer(Some(BigInt::from(99)), None, false, &amt),
        GasTransferOutcome::InsufficientBalance
    );
    // from == to with sufficient balance -> no movement.
    assert_eq!(
        GasToken::compute_gas_transfer(
            Some(BigInt::from(100)),
            Some(BigInt::from(100)),
            true,
            &amt
        ),
        GasTransferOutcome::NoMovement
    );
    // Exact balance -> deduct to zero deletes the from-entry; to credited.
    assert_eq!(
        GasToken::compute_gas_transfer(Some(BigInt::from(100)), None, false, &amt),
        GasTransferOutcome::Move {
            from_new: BigInt::zero(),
            delete_from: true,
            to_new: BigInt::from(100)
        }
    );
    // Partial balance -> from keeps the remainder; existing to is added to.
    assert_eq!(
        GasToken::compute_gas_transfer(Some(BigInt::from(250)), Some(BigInt::from(7)), false, &amt),
        GasTransferOutcome::Move {
            from_new: BigInt::from(150),
            delete_from: false,
            to_new: BigInt::from(107)
        }
    );
}

#[test]
fn gas_account_storage_round_trips() {
    use neo_storage::persistence::DataCache;
    let cache = DataCache::new(false);
    let account = UInt160::from_bytes(&[3u8; 20]).unwrap();

    assert!(
        GasToken::new()
            .read_gas_account(&cache, &account)
            .unwrap()
            .is_none()
    );
    GasToken::new()
        .write_gas_account(&cache, &account, &BigInt::from(12345))
        .unwrap();
    let expected = BinarySerializer::serialize(
        &StackItem::from_struct(vec![StackItem::from_int(BigInt::from(12345))]),
        &ExecutionEngineLimits::default(),
    )
    .unwrap();
    assert_eq!(
        cache
            .get(&GasToken::account_key(&account))
            .unwrap()
            .value_bytes()
            .as_ref(),
        expected.as_slice()
    );
    assert_eq!(
        GasToken::new().read_gas_account(&cache, &account).unwrap(),
        Some(BigInt::from(12345))
    );
    // The single-field Struct[Balance] layout is what read_nep17_balance reads.
    assert_eq!(
        crate::read_nep17_balance(&cache, GasToken::ID, &account).unwrap(),
        BigInt::from(12345)
    );
    GasToken::new().delete_gas_account(&cache, &account);
    assert!(
        GasToken::new()
            .read_gas_account(&cache, &account)
            .unwrap()
            .is_none()
    );
}

#[test]
fn gas_account_storage_uses_stack_value_projection() {
    let source = include_str!("../../gas_token/mod.rs");
    let read_start = source
        .find("fn read_gas_account(")
        .expect("read_gas_account helper exists");
    let start = source
        .find("fn write_gas_account(")
        .expect("write_gas_account helper exists");
    let end = source[start..]
        .find("fn delete_gas_account")
        .map(|offset| start + offset)
        .expect("delete_gas_account follows write_gas_account");
    let reader = &source[read_start..start];
    let helper = &source[start..end];

    // After the FungibleToken-helper extraction, the gas account readers
    // delegate (de)serialization to the shared crate::deserialize_account_state
    // / serialize_account_state helpers instead of inlining the plumbing.
    assert!(reader.contains("crate::deserialize_account_state"));
    assert!(!reader.contains("StackValue::Struct"));
    assert!(!reader.contains("stack_value_as_bigint"));
    assert!(!reader.contains("BinarySerializer::deserialize("));
    assert!(!reader.contains("deserialize_stack_value_with_limits"));

    assert!(helper.contains("crate::AccountState::new"));
    assert!(helper.contains("crate::serialize_account_state"));
    assert!(!helper.contains("StackValue::Struct"));
    assert!(!helper.contains("StackItem::from_struct"));
    assert!(!helper.contains("BinarySerializer::serialize("));
    assert!(!helper.contains("serialize_stack_value_default"));
}

#[test]
fn balance_of_absent_account_is_zero() {
    use neo_storage::persistence::DataCache;
    let cache = DataCache::new(false);
    let account = UInt160::from_bytes(&[1u8; 20]).unwrap();
    // C# BalanceOf returns BigInteger.Zero when the account has no entry.
    assert_eq!(
        GasToken::balance_of(&cache, &account).unwrap(),
        BigInt::from(0)
    );
}

#[test]
fn total_supply_invoke_reads_full_bigint() {
    use neo_primitives::TriggerType;
    use std::sync::Arc;

    let cache = DataCache::new(false);
    let supply = BigInt::from(i64::MAX) + BigInt::from(42);
    cache.add(
        GasToken::total_supply_key(),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&supply)),
    );
    let snapshot = Arc::new(cache);
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        None,
        ProtocolSettings::default(),
        10_000_000,
        None,
    )
    .expect("engine builds");

    let encoded = GasToken::new()
        .invoke(&mut engine, "totalSupply", &[])
        .expect("totalSupply");
    assert_eq!(BigInt::from_signed_bytes_le(&encoded), supply);
}

/// C# `FungibleToken.OnManifestCompose` (FungibleToken.cs:68-71): the
/// generated GAS manifest declares NEP-17 regardless of the hardfork
/// configuration or height.
#[test]
fn manifest_declares_nep17() {
    use neo_execution::native_contract::build_native_contract_state;

    let state = build_native_contract_state(&GasToken, &ProtocolSettings::default(), 0);
    assert_eq!(state.manifest.supported_standards, ["NEP-17"]);
    let later = build_native_contract_state(&GasToken, &ProtocolSettings::default(), u32::MAX);
    assert_eq!(later.manifest.supported_standards, ["NEP-17"]);
}

/// C# `FungibleToken.Burn`: a negative amount faults, zero is a no-op, an
/// under-funded account faults, a partial burn debits balance and supply
/// (emitting `Transfer(account, null, amount)`), and a full burn deletes
/// the account entry.
#[test]
fn gas_burn_debits_balance_and_supply() {
    use neo_primitives::TriggerType;
    use std::sync::Arc;

    let cache = DataCache::new(false);
    let account = UInt160::from_bytes(&[9u8; 20]).unwrap();
    GasToken::new()
        .write_gas_account(&cache, &account, &BigInt::from(100))
        .unwrap();
    let supply_key = GasToken::total_supply_key();
    cache.add(
        supply_key.clone(),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(100))),
    );
    let snapshot = Arc::new(cache);
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::clone(&snapshot),
        None,
        ProtocolSettings::default(),
        10_000_000,
        None,
    )
    .expect("engine builds");

    // Negative -> fault; zero -> no-op (no event, no state change).
    assert!(
        GasToken::new()
            .gas_burn(&mut engine, &account, &BigInt::from(-1))
            .is_err()
    );
    GasToken::new()
        .gas_burn(&mut engine, &account, &BigInt::from(0))
        .unwrap();
    assert!(engine.notifications().is_empty());
    assert_eq!(
        GasToken::new()
            .read_gas_account(&snapshot, &account)
            .unwrap(),
        Some(BigInt::from(100))
    );

    // Partial burn: balance and supply shrink, Transfer(account, null, 30).
    GasToken::new()
        .gas_burn(&mut engine, &account, &BigInt::from(30))
        .unwrap();
    assert_eq!(
        GasToken::new()
            .read_gas_account(&snapshot, &account)
            .unwrap(),
        Some(BigInt::from(70))
    );
    assert_eq!(
        BigInt::from_signed_bytes_le(&snapshot.get(&supply_key).unwrap().value_bytes()),
        BigInt::from(70)
    );
    assert_eq!(engine.notifications().len(), 1);

    // Over-burn -> fault, state unchanged.
    assert!(
        GasToken::new()
            .gas_burn(&mut engine, &account, &BigInt::from(71))
            .is_err()
    );
    assert_eq!(
        GasToken::new()
            .read_gas_account(&snapshot, &account)
            .unwrap(),
        Some(BigInt::from(70))
    );

    // Full burn deletes the account entry; the supply reaches zero (stored
    // as the canonical empty-bytes BigInteger).
    GasToken::new()
        .gas_burn(&mut engine, &account, &BigInt::from(70))
        .unwrap();
    assert!(
        GasToken::new()
            .read_gas_account(&snapshot, &account)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        BigInt::from_signed_bytes_le(&snapshot.get(&supply_key).unwrap().value_bytes()),
        BigInt::from(0)
    );
}

/// `GasToken::initialize` and `GasToken::on_persist` against the C# oracle
/// (GasToken.cs:29-58): the genesis InitialGasDistribution mint, the
/// per-transaction fee burns, the network-fee mint to the primary validator,
/// and the NotaryAssisted attribute deduction.
#[cfg(test)]
mod persist_tests {
    use super::*;
    use std::sync::Arc;

    use neo_crypto::ECPoint;
    use neo_payloads::{Block, Header, NotaryAssisted, Signer, Transaction};
    use neo_primitives::{TriggerType, WitnessScope};

    use crate::test_support::{sample_committee, seed_committee};

    /// The signature-contract address of `points[primary]` after the C#
    /// `GetNextBlockValidators` ordering (take ValidatorsCount, sort ascending).
    fn primary_address(points: &[ECPoint], validators_count: usize, primary: usize) -> UInt160 {
        let mut sorted: Vec<ECPoint> = points.iter().take(validators_count).cloned().collect();
        sorted.sort();
        UInt160::from_script(&Contract::create_signature_redeem_script(
            sorted[primary].clone(),
        ))
    }

    fn fee_tx(sender: UInt160, system_fee: i64, network_fee: i64) -> Transaction {
        let mut tx = Transaction::new();
        tx.set_signers(vec![Signer::new(sender, WitnessScope::NONE)]);
        tx.set_system_fee(system_fee);
        tx.set_network_fee(network_fee);
        tx
    }

    fn on_persist_engine(snapshot: Arc<DataCache>, block: Block) -> ApplicationEngine {
        // C# runs the native OnPersist script with gas limit 0.
        ApplicationEngine::new(
            TriggerType::OnPersist,
            None,
            snapshot,
            Some(block),
            ProtocolSettings::default(),
            0,
            None,
        )
        .expect("engine builds")
    }

    fn seed_gas(cache: &DataCache, account: &UInt160, balance: i64) {
        GasToken::new()
            .write_gas_account(cache, account, &BigInt::from(balance))
            .unwrap();
        let supply_key = GasToken::total_supply_key();
        let supply = cache
            .get(&supply_key)
            .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes()))
            .unwrap_or_else(BigInt::zero)
            + BigInt::from(balance);
        cache.update(
            supply_key,
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&supply)),
        );
    }

    fn balance(cache: &DataCache, account: &UInt160) -> BigInt {
        GasToken::balance_of(cache, account).unwrap()
    }

    /// C# `GasToken.InitializeAsync` (GasToken.cs:29-37): the genesis pass
    /// mints `InitialGasDistribution` (52M GAS) to the BFT address of the
    /// standby validators and emits `Transfer(null, bft, amount)`.
    #[test]
    fn initialize_mints_initial_gas_distribution_to_bft_address() {
        let settings = ProtocolSettings::default();
        let snapshot = Arc::new(DataCache::new(false));
        let mut engine = ApplicationEngine::new(
            TriggerType::OnPersist,
            None,
            Arc::clone(&snapshot),
            None,
            settings.clone(),
            0,
            None,
        )
        .expect("engine builds");

        NativeContract::initialize(&GasToken::new(), &mut engine).expect("initialize");

        let bft = crate::NeoToken::bft_address(&settings.standby_validators()).unwrap();
        let expected = BigInt::from(settings.initial_gas_distribution);
        assert_eq!(
            balance(&snapshot, &bft),
            expected,
            "52M GAS to the BFT address"
        );
        let supply_key = GasToken::total_supply_key();
        assert_eq!(
            BigInt::from_signed_bytes_le(&snapshot.get(&supply_key).unwrap().value_bytes()),
            expected,
            "total supply equals the initial distribution"
        );
        assert_eq!(engine.notifications().len(), 1);
        let transfer = &engine.notifications()[0];
        assert_eq!(transfer.event_name, "Transfer");
        assert_eq!(transfer.script_hash, GasToken::script_hash());
        assert!(
            matches!(transfer.state[0], StackItem::Null),
            "from = null (mint)"
        );
        assert_eq!(transfer.state[1].as_bytes().unwrap(), bft.to_bytes());
        assert_eq!(transfer.state[2].as_int().unwrap(), expected);
    }

    /// C# `GasToken.OnPersistAsync` (GasToken.cs:39-58): each sender is burned
    /// `SystemFee + NetworkFee`; the summed network fees are minted to the
    /// primary validator's signature address (validators sorted ascending,
    /// indexed by the block's PrimaryIndex).
    #[test]
    fn on_persist_burns_fees_and_mints_network_fees_to_primary() {
        let settings = ProtocolSettings::default();
        let validators_count = usize::try_from(settings.validators_count).unwrap();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        let sender_a = UInt160::from_bytes(&[0xA1; 20]).unwrap();
        let sender_b = UInt160::from_bytes(&[0xB2; 20]).unwrap();
        seed_gas(&cache, &sender_a, 10_0000_0000);
        seed_gas(&cache, &sender_b, 5_0000_0000);
        let snapshot = Arc::new(cache);

        let mut header = Header::new();
        header.set_index(1);
        header.set_primary_index(1);
        let block = Block::from_parts(
            header,
            vec![
                fee_tx(sender_a, 3_0000_0000, 1_0000_0000),
                fee_tx(sender_b, 2_0000_0000, 5000_0000),
            ],
        );
        let mut engine = on_persist_engine(Arc::clone(&snapshot), block);
        NativeContract::on_persist(&GasToken::new(), &mut engine).expect("on_persist");

        // Burns: sender_a 4 GAS of 10, sender_b 2.5 GAS of 5.
        assert_eq!(balance(&snapshot, &sender_a), BigInt::from(6_0000_0000i64));
        assert_eq!(balance(&snapshot, &sender_b), BigInt::from(2_5000_0000i64));
        // Mint: 1.5 GAS total network fees to the primary validator
        // (sorted validator index 1).
        let primary = primary_address(&committee, validators_count, 1);
        assert_eq!(balance(&snapshot, &primary), BigInt::from(1_5000_0000i64));
        // Supply: 15 GAS seeded - 6.5 burned + 1.5 minted = 10.
        let supply_key = GasToken::total_supply_key();
        assert_eq!(
            BigInt::from_signed_bytes_le(&snapshot.get(&supply_key).unwrap().value_bytes()),
            BigInt::from(10_0000_0000i64)
        );
        // Notifications: Transfer(a, null, 4), Transfer(b, null, 2.5),
        // Transfer(null, primary, 1.5) — burn, burn, mint, in C# order.
        let events: Vec<(bool, bool, BigInt)> = engine
            .notifications()
            .iter()
            .map(|n| {
                (
                    matches!(n.state[0], StackItem::Null),
                    matches!(n.state[1], StackItem::Null),
                    n.state[2].as_int().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            events,
            vec![
                (false, true, BigInt::from(4_0000_0000i64)),
                (false, true, BigInt::from(2_5000_0000i64)),
                (true, false, BigInt::from(1_5000_0000i64)),
            ]
        );
    }

    /// C# `Burn` throws on an under-funded sender (`Balance < amount` ->
    /// InvalidOperationException), faulting the whole block.
    #[test]
    fn on_persist_faults_when_a_sender_cannot_pay_its_fees() {
        let cache = DataCache::new(false);
        seed_committee(&cache, &sample_committee());
        let sender = UInt160::from_bytes(&[0xC3; 20]).unwrap();
        seed_gas(&cache, &sender, 100);
        let snapshot = Arc::new(cache);

        let mut header = Header::new();
        header.set_index(1);
        header.set_primary_index(0);
        let block = Block::from_parts(header, vec![fee_tx(sender, 200, 0)]);
        let mut engine = on_persist_engine(snapshot, block);
        assert!(NativeContract::on_persist(&GasToken::new(), &mut engine).is_err());
    }

    /// C# GasToken.cs:47-53: a NotaryAssisted transaction deducts
    /// `(NKeys + 1) * GetAttributeFeeV1(NotaryAssisted)` from the primary's
    /// mint (the deducted share is minted to notary nodes by the Notary
    /// contract instead).
    #[test]
    fn on_persist_deducts_notary_assisted_share_from_the_primary_mint() {
        let settings = ProtocolSettings::default();
        let validators_count = usize::try_from(settings.validators_count).unwrap();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        // The Echidna-default NotaryAssisted attribute fee: 0.1 GAS per key.
        cache.add(
            crate::PolicyContract::attribute_fee_key(
                neo_primitives::TransactionAttributeType::NotaryAssisted.to_byte(),
            ),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(1000_0000i64))),
        );
        let sender = UInt160::from_bytes(&[0xD4; 20]).unwrap();
        seed_gas(&cache, &sender, 10_0000_0000);
        let snapshot = Arc::new(cache);

        let mut tx = fee_tx(sender, 1_0000_0000, 2_0000_0000);
        tx.set_attributes(vec![TransactionAttribute::NotaryAssisted(
            NotaryAssisted::new(2),
        )]);
        let mut header = Header::new();
        header.set_index(1);
        header.set_primary_index(0);
        let block = Block::from_parts(header, vec![tx]);
        let mut engine = on_persist_engine(Arc::clone(&snapshot), block);
        NativeContract::on_persist(&GasToken::new(), &mut engine).expect("on_persist");

        // Burn untouched by the attribute: 3 GAS off the sender.
        assert_eq!(balance(&snapshot, &sender), BigInt::from(7_0000_0000i64));
        // Mint: 2 GAS network fee - (2 + 1) * 0.1 GAS = 1.7 GAS.
        let primary = primary_address(&committee, validators_count, 0);
        assert_eq!(balance(&snapshot, &primary), BigInt::from(1_7000_0000i64));
    }

    /// An empty block burns nothing and mints nothing (C# `Mint` returns early
    /// on a zero amount), but still resolves the validator set.
    #[test]
    fn on_persist_is_a_value_noop_for_an_empty_block() {
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        let snapshot = Arc::new(cache);

        let mut header = Header::new();
        header.set_index(2);
        header.set_primary_index(0);
        let block = Block::from_parts(header, Vec::new());
        let mut engine = on_persist_engine(Arc::clone(&snapshot), block);
        NativeContract::on_persist(&GasToken::new(), &mut engine).expect("on_persist");
        assert!(
            engine.notifications().is_empty(),
            "no Transfer for a zero mint"
        );
        let supply_key = GasToken::total_supply_key();
        assert!(snapshot.get(&supply_key).is_none(), "supply untouched");
    }

    /// C# indexes `validators[block.PrimaryIndex]`: an index outside the
    /// validator set is an IndexOutOfRangeException (block fault).
    #[test]
    fn on_persist_faults_on_a_primary_index_outside_the_validator_set() {
        let cache = DataCache::new(false);
        seed_committee(&cache, &sample_committee());
        let snapshot = Arc::new(cache);

        let mut header = Header::new();
        header.set_index(1);
        header.set_primary_index(200);
        let block = Block::from_parts(header, Vec::new());
        let mut engine = on_persist_engine(snapshot, block);
        assert!(NativeContract::on_persist(&GasToken::new(), &mut engine).is_err());
    }
}

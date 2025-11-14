use neo_core::network::p2p::payloads::{signer::Signer, transaction::Transaction};
use neo_core::persistence::DataCache;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::native::{GasToken, NativeContract, NeoToken};
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::{IVerifiable, UInt160, WitnessScope};
use num_bigint::BigInt;
use num_traits::Zero;
use std::sync::Arc;

#[test]
fn neo_token_hash_matches_reference() {
    let neo = NeoToken::new();
    assert_eq!(
        hex::encode(neo.hash().to_array()),
        "ef4073a0f2b305a38ec4050e4d3d28bc40ea63f5"
    );
    assert_eq!(neo.symbol(), "NEO");
    assert_eq!(neo.decimals(), 0);
}

#[test]
fn gas_token_hash_matches_reference() {
    let gas = GasToken::new();
    assert_eq!(
        hex::encode(gas.hash().to_array()),
        "d2a4cff31913016155e38e474a2c06d08be276cf"
    );
    assert_eq!(gas.symbol(), "GAS");
    assert_eq!(gas.decimals(), 8);
}

fn make_engine(snapshot: Arc<DataCache>, signer: UInt160) -> ApplicationEngine {
    let mut container = Transaction::new();
    container.set_signers(vec![Signer::new(signer, WitnessScope::GLOBAL)]);
    let script_container: Arc<dyn IVerifiable> = Arc::new(container);
    ApplicationEngine::new(
        TriggerType::Application,
        Some(script_container),
        snapshot,
        None,
        Default::default(),
        200_000_000,
        None,
    )
    .expect("engine")
}

fn sample_account(tag: u8) -> UInt160 {
    let bytes = [tag; 20];
    UInt160::from_bytes(&bytes).unwrap()
}

#[test]
fn gas_token_mint_burn_and_transfer_update_balances() {
    let snapshot = Arc::new(DataCache::new(false));
    let context_engine_snapshot = Arc::clone(&snapshot);
    let gas = GasToken::new();
    let account_a = sample_account(0xAA);
    let account_b = sample_account(0xBB);
    let amount = BigInt::from(1_000_000);

    let mut engine = make_engine(context_engine_snapshot, account_a.clone());
    engine.set_current_script_hash(Some(gas.hash()));

    gas.mint(&mut engine, &account_a, &amount, false)
        .expect("mint succeeds");

    let balance_a = gas.balance_of_snapshot(snapshot.as_ref(), &account_a);
    assert_eq!(balance_a, amount);
    let balance_b = gas.balance_of_snapshot(snapshot.as_ref(), &account_b);
    assert!(balance_b.is_zero());

    // transfer half to account_b
    let transfer_bytes = amount.clone().to_signed_bytes_le();
    let from_bytes = account_a.to_bytes();
    let to_bytes = account_b.to_bytes();
    let transfer_args = vec![from_bytes, to_bytes, transfer_bytes.clone()];
    let transfer_result = gas
        .invoke(&mut engine, "transfer", &transfer_args)
        .expect("transfer call");
    assert_eq!(transfer_result, vec![1]);

    let balance_a_after = gas.balance_of_snapshot(snapshot.as_ref(), &account_a);
    let balance_b_after = gas.balance_of_snapshot(snapshot.as_ref(), &account_b);
    assert!(balance_a_after < balance_a);
    assert_eq!(balance_b_after.clone() + balance_a_after.clone(), amount);

    gas.burn(&mut engine, &account_a, &balance_a_after)
        .expect("burn succeeds");
    assert!(gas
        .balance_of_snapshot(snapshot.as_ref(), &account_a)
        .is_zero());
}

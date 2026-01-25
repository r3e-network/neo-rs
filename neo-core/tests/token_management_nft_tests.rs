use neo_core::hardfork::{Hardfork, HardforkManager};
use neo_core::ledger::block_header::BlockHeader;
use neo_core::ledger::Block;
use neo_core::network::p2p::payloads::Witness;
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::native::{NativeContract, TokenManagement};
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::{UInt160, UInt256};
use num_bigint::BigInt;
use std::collections::HashMap;
use std::sync::Arc;

const TEST_GAS_LIMIT: i64 = 3_000_000_000;

fn protocol_settings_with_faun() -> ProtocolSettings {
    let mut settings = ProtocolSettings::default();
    let mut hardforks = HashMap::new();
    hardforks.insert(Hardfork::HfFaun, 0);
    settings.hardforks = hardforks;
    settings
}

fn make_snapshot_with_genesis(settings: &ProtocolSettings) -> Arc<DataCache> {
    let snapshot = Arc::new(DataCache::new(false));
    let genesis = neo_core::ledger::create_genesis_block(settings);

    let mut on_persist = ApplicationEngine::new(
        TriggerType::OnPersist,
        None,
        Arc::clone(&snapshot),
        Some(genesis.clone()),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("on persist engine");
    on_persist.native_on_persist().expect("native on persist");

    let mut post_persist = ApplicationEngine::new(
        TriggerType::PostPersist,
        None,
        Arc::clone(&snapshot),
        Some(genesis),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("post persist engine");
    post_persist
        .native_post_persist()
        .expect("native post persist");

    snapshot
}

fn make_block(index: u32) -> Block {
    let header = BlockHeader {
        index,
        previous_hash: UInt256::zero(),
        merkle_root: UInt256::zero(),
        timestamp: 1,
        nonce: 0,
        primary_index: 0,
        next_consensus: UInt160::zero(),
        witnesses: vec![Witness::empty()],
        ..Default::default()
    };
    Block::new(header, Vec::new())
}

fn sample_account(tag: u8) -> UInt160 {
    let bytes = [tag; 20];
    UInt160::from_bytes(&bytes).unwrap()
}

#[test]
fn nft_create_and_mint() {
    let settings = protocol_settings_with_faun();
    let snapshot = make_snapshot_with_genesis(&settings);
    let token_mgmt = TokenManagement::new();
    let owner = sample_account(0x01);

    let block = make_block(1);

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::clone(&snapshot),
        Some(block),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine");

    let name = b"TestNFT";
    let symbol = b"TST";
    let create_args = vec![owner.to_bytes(), name.to_vec(), symbol.to_vec(), vec![1]];

    let result = engine
        .call_native_contract(token_mgmt.hash(), "createNonFungible", &create_args)
        .expect("createNonFungible call");
    assert_eq!(result.len(), 20);

    let asset_id = UInt160::from_bytes(&result).expect("asset id");

    let account = sample_account(0x02);
    let mint_args = vec![asset_id.to_bytes(), account.to_bytes()];

    let mint_result = engine
        .call_native_contract(token_mgmt.hash(), "mintNFT", &mint_args)
        .expect("mintNFT call");
    assert_eq!(mint_result.len(), 20);

    let nft_id = UInt160::from_bytes(&mint_result).expect("nft id");
    assert!(!nft_id.is_zero());

    let get_info_args = vec![nft_id.to_bytes()];

    let info_result = engine
        .call_native_contract(token_mgmt.hash(), "getNFTInfo", &get_info_args)
        .expect("getNFTInfo call");
    assert!(!info_result.is_empty());
}

#[test]
fn nft_burn() {
    let settings = protocol_settings_with_faun();
    let snapshot = make_snapshot_with_genesis(&settings);
    let token_mgmt = TokenManagement::new();
    let owner = sample_account(0x01);

    let block = make_block(1);

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::clone(&snapshot),
        Some(block),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine");

    let name = b"BurnableNFT";
    let symbol = b"BRN";
    let create_args = vec![owner.to_bytes(), name.to_vec(), symbol.to_vec(), vec![1]];

    let result = engine
        .call_native_contract(token_mgmt.hash(), "createNonFungible", &create_args)
        .expect("createNonFungible call");
    let asset_id = UInt160::from_bytes(&result).expect("asset id");

    engine.set_current_script_hash(Some(owner));
    engine.set_calling_script_hash(Some(owner));

    let account = sample_account(0x02);
    let mint_args = vec![asset_id.to_bytes(), account.to_bytes()];

    let nft_result = engine
        .call_native_contract(token_mgmt.hash(), "mintNFT", &mint_args)
        .expect("mintNFT call");
    let nft_id = UInt160::from_bytes(&nft_result).expect("nft id");

    engine.set_calling_script_hash(Some(account));

    snapshot.commit();

    let burn_args = vec![nft_id.to_bytes()];

    let burn_result = engine
        .call_native_contract(token_mgmt.hash(), "burnNFT", &burn_args)
        .expect("burnNFT call");
    assert_eq!(burn_result, vec![1]);

    let get_info_args = vec![nft_id.to_bytes()];

    let info_result = engine
        .call_native_contract(token_mgmt.hash(), "getNFTInfo", &get_info_args)
        .expect("getNFTInfo call");
    assert!(info_result.is_empty());
}

#[test]
fn nft_transfer() {
    let settings = protocol_settings_with_faun();
    let snapshot = make_snapshot_with_genesis(&settings);
    let token_mgmt = TokenManagement::new();
    let owner = sample_account(0x01);

    let block = make_block(1);

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::clone(&snapshot),
        Some(block),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine");

    let name = b"TransferableNFT";
    let symbol = b"TRN";
    let create_args = vec![owner.to_bytes(), name.to_vec(), symbol.to_vec(), vec![1]];

    let result = engine
        .call_native_contract(token_mgmt.hash(), "createNonFungible", &create_args)
        .expect("createNonFungible call");
    let asset_id = UInt160::from_bytes(&result).expect("asset id");

    engine.set_current_script_hash(Some(owner));
    engine.set_calling_script_hash(Some(owner));

    let from_account = sample_account(0x02);
    let to_account = sample_account(0x03);

    let mint_args = vec![asset_id.to_bytes(), from_account.to_bytes()];

    let nft_result = engine
        .call_native_contract(token_mgmt.hash(), "mintNFT", &mint_args)
        .expect("mintNFT call");
    let nft_id = UInt160::from_bytes(&nft_result).expect("nft id");

    engine.set_calling_script_hash(Some(from_account));

    snapshot.commit();

    let transfer_args = vec![
        nft_id.to_bytes(),
        from_account.to_bytes(),
        to_account.to_bytes(),
        Vec::new(),
    ];

    let transfer_result = engine
        .call_native_contract(token_mgmt.hash(), "transferNFT", &transfer_args)
        .expect("transferNFT call");
    assert_eq!(transfer_result, vec![1]);

    let get_nfts_args = vec![asset_id.to_bytes()];

    let nfts_result = engine
        .call_native_contract(token_mgmt.hash(), "getNFTs", &get_nfts_args)
        .expect("getNFTs call");
    assert!(!nfts_result.is_empty());
}

#[test]
fn nft_get_nfts_of_owner() {
    let settings = protocol_settings_with_faun();
    let snapshot = make_snapshot_with_genesis(&settings);
    let token_mgmt = TokenManagement::new();
    let owner = sample_account(0x01);

    let block = make_block(1);

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::clone(&snapshot),
        Some(block),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine");

    let name = b"MultiNFT";
    let symbol = b"MFT";
    let create_args = vec![owner.to_bytes(), name.to_vec(), symbol.to_vec(), vec![1]];

    let result = engine
        .call_native_contract(token_mgmt.hash(), "createNonFungible", &create_args)
        .expect("createNonFungible call");
    let asset_id = UInt160::from_bytes(&result).expect("asset id");

    engine.set_current_script_hash(Some(owner));
    engine.set_calling_script_hash(Some(owner));

    let holder = sample_account(0x05);

    for _ in 0..3 {
        let mint_args = vec![asset_id.to_bytes(), holder.to_bytes()];

        let _ = engine
            .call_native_contract(token_mgmt.hash(), "mintNFT", &mint_args)
            .expect("mintNFT call");
    }

    let get_owner_nfts_args = vec![holder.to_bytes()];

    let owner_nfts_result = engine
        .call_native_contract(token_mgmt.hash(), "getNFTsOfOwner", &get_owner_nfts_args)
        .expect("getNFTsOfOwner call");
    assert!(!owner_nfts_result.is_empty());
}

#[test]
fn nft_multiple_mints_same_block() {
    let settings = protocol_settings_with_faun();
    let snapshot = make_snapshot_with_genesis(&settings);
    let token_mgmt = TokenManagement::new();
    let owner = sample_account(0x01);

    let block = make_block(1);

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::clone(&snapshot),
        Some(block),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine");

    let name = b"UniqueNFT";
    let symbol = b"UNQ";
    let create_args = vec![owner.to_bytes(), name.to_vec(), symbol.to_vec(), vec![1]];

    let result = engine
        .call_native_contract(token_mgmt.hash(), "createNonFungible", &create_args)
        .expect("createNonFungible call");
    let asset_id = UInt160::from_bytes(&result).expect("asset id");

    engine.set_current_script_hash(Some(owner));
    engine.set_calling_script_hash(Some(owner));

    let account = sample_account(0x06);

    let mut nft_ids = Vec::new();
    for i in 0..5 {
        let mint_args = vec![asset_id.to_bytes(), account.to_bytes()];

        let nft_result = engine
            .call_native_contract(token_mgmt.hash(), "mintNFT", &mint_args)
            .expect("mintNFT call");
        let nft_id = UInt160::from_bytes(&nft_result).expect("nft id");
        nft_ids.push(nft_id);

        let get_info_args = vec![nft_id.to_bytes()];

        let info_result = engine
            .call_native_contract(token_mgmt.hash(), "getNFTInfo", &get_info_args)
            .expect("getNFTInfo call");
        assert!(!info_result.is_empty(), "NFT {} should exist", i);
    }

    let unique_nfts: std::collections::HashSet<_> = nft_ids.into_iter().collect();
    assert_eq!(unique_nfts.len(), 5, "All NFT IDs should be unique");
}

#[test]
fn nft_owner_verification() {
    let settings = protocol_settings_with_faun();
    let snapshot = make_snapshot_with_genesis(&settings);
    let token_mgmt = TokenManagement::new();
    let owner = sample_account(0x01);

    let block = make_block(1);

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::clone(&snapshot),
        Some(block),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine");

    let name = b"OwnerTestNFT";
    let symbol = b"OWN";
    let create_args = vec![owner.to_bytes(), name.to_vec(), symbol.to_vec(), vec![1]];

    let result = engine
        .call_native_contract(token_mgmt.hash(), "createNonFungible", &create_args)
        .expect("createNonFungible call");
    let asset_id = UInt160::from_bytes(&result).expect("asset id");

    engine.set_current_script_hash(Some(owner));
    engine.set_calling_script_hash(Some(owner));

    let holder = sample_account(0x10);
    let mint_args = vec![asset_id.to_bytes(), holder.to_bytes()];

    let nft_result = engine
        .call_native_contract(token_mgmt.hash(), "mintNFT", &mint_args)
        .expect("mintNFT call");
    let nft_id = UInt160::from_bytes(&nft_result).expect("nft id");

    let get_info_args = vec![nft_id.to_bytes()];

    let info_result = engine
        .call_native_contract(token_mgmt.hash(), "getNFTInfo", &get_info_args)
        .expect("getNFTInfo call");
    assert!(!info_result.is_empty());
}

#[test]
fn nft_transfer_verification() {
    let settings = protocol_settings_with_faun();
    let snapshot = make_snapshot_with_genesis(&settings);
    let token_mgmt = TokenManagement::new();
    let owner = sample_account(0x01);

    let block = make_block(1);

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::clone(&snapshot),
        Some(block),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine");

    let name = b"TransferVerifyNFT";
    let symbol = b"TVN";
    let create_args = vec![owner.to_bytes(), name.to_vec(), symbol.to_vec(), vec![1]];

    let result = engine
        .call_native_contract(token_mgmt.hash(), "createNonFungible", &create_args)
        .expect("createNonFungible call");
    let asset_id = UInt160::from_bytes(&result).expect("asset id");

    engine.set_current_script_hash(Some(owner));
    engine.set_calling_script_hash(Some(owner));

    let from_account = sample_account(0x11);
    let to_account = sample_account(0x12);

    let mint_args = vec![asset_id.to_bytes(), from_account.to_bytes()];

    let nft_result = engine
        .call_native_contract(token_mgmt.hash(), "mintNFT", &mint_args)
        .expect("mintNFT call");
    let nft_id = UInt160::from_bytes(&nft_result).expect("nft id");

    engine.set_calling_script_hash(Some(from_account));

    snapshot.commit();

    let transfer_args = vec![
        nft_id.to_bytes(),
        from_account.to_bytes(),
        to_account.to_bytes(),
        Vec::new(),
    ];

    let transfer_result = engine
        .call_native_contract(token_mgmt.hash(), "transferNFT", &transfer_args)
        .expect("transferNFT call");
    assert_eq!(transfer_result, vec![1]);

    engine.set_calling_script_hash(Some(to_account));

    let get_info_args = vec![nft_id.to_bytes()];

    let info_result = engine
        .call_native_contract(token_mgmt.hash(), "getNFTInfo", &get_info_args)
        .expect("getNFTInfo call");
    assert!(!info_result.is_empty());
}

#[test]
fn nft_burn_verification() {
    let settings = protocol_settings_with_faun();
    let snapshot = make_snapshot_with_genesis(&settings);
    let token_mgmt = TokenManagement::new();
    let owner = sample_account(0x01);

    let block = make_block(1);

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::clone(&snapshot),
        Some(block),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine");

    let name = b"BurnVerifyNFT";
    let symbol = b"BVN";
    let create_args = vec![owner.to_bytes(), name.to_vec(), symbol.to_vec(), vec![1]];

    let result = engine
        .call_native_contract(token_mgmt.hash(), "createNonFungible", &create_args)
        .expect("createNonFungible call");
    let asset_id = UInt160::from_bytes(&result).expect("asset id");

    engine.set_current_script_hash(Some(owner));
    engine.set_calling_script_hash(Some(owner));

    let holder = sample_account(0x13);
    let mint_args = vec![asset_id.to_bytes(), holder.to_bytes()];

    let nft_result = engine
        .call_native_contract(token_mgmt.hash(), "mintNFT", &mint_args)
        .expect("mintNFT call");
    let nft_id = UInt160::from_bytes(&nft_result).expect("nft id");

    engine.set_calling_script_hash(Some(holder));

    snapshot.commit();

    let burn_args = vec![nft_id.to_bytes()];

    let burn_result = engine
        .call_native_contract(token_mgmt.hash(), "burnNFT", &burn_args)
        .expect("burnNFT call");
    assert_eq!(burn_result, vec![1]);

    let get_info_args = vec![nft_id.to_bytes()];

    let info_result = engine
        .call_native_contract(token_mgmt.hash(), "getNFTInfo", &get_info_args)
        .expect("getNFTInfo call");
    assert!(info_result.is_empty(), "NFT should not exist after burn");
}

#[test]
fn nft_balance_of() {
    let settings = protocol_settings_with_faun();
    let snapshot = make_snapshot_with_genesis(&settings);
    let token_mgmt = TokenManagement::new();
    let owner = sample_account(0x01);

    let block = make_block(1);

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::clone(&snapshot),
        Some(block),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine");

    let name = b"BalanceNFT";
    let symbol = b"BAL";
    let create_args = vec![owner.to_bytes(), name.to_vec(), symbol.to_vec(), vec![1]];

    let result = engine
        .call_native_contract(token_mgmt.hash(), "createNonFungible", &create_args)
        .expect("createNonFungible call");
    let asset_id = UInt160::from_bytes(&result).expect("asset id");

    engine.set_current_script_hash(Some(owner));
    engine.set_calling_script_hash(Some(owner));

    let holder = sample_account(0x20);

    for _ in 0..3 {
        let mint_args = vec![asset_id.to_bytes(), holder.to_bytes()];

        let _ = engine
            .call_native_contract(token_mgmt.hash(), "mintNFT", &mint_args)
            .expect("mintNFT call");
    }

    let balance_args = vec![asset_id.to_bytes(), holder.to_bytes()];

    let balance_result = engine
        .call_native_contract(token_mgmt.hash(), "balanceOf", &balance_args)
        .expect("balanceOf call");

    let balance = BigInt::from_signed_bytes_le(&balance_result);
    assert_eq!(balance, BigInt::from(3), "Holder should have 3 NFTs");
}
